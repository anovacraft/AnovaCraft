//! Formula columns: a column whose values are computed, row by row, from a
//! Rhai expression over the other columns.
//!
//! # Writing formulas
//!
//! A formula is a Rhai expression (or a few statements, the last giving the
//! value) evaluated once per row. Inside it:
//!
//! * A column whose name is a plain identifier (letters, digits and `_`, not
//!   starting with a digit) is a variable holding the current row's value:
//!   `Weight / Height ** 2`. Any column, whatever its name, can be read with
//!   `col("Body mass")`.
//! * Numbers are decimal (`/` always divides exactly, so `1 / 2` is `0.5`),
//!   dates and times are numbers of seconds since 1970-01-01 00:00 UTC, and a
//!   missing number is `NaN` (a missing text value is `""`).
//! * The result may be a number, `true`/`false` (stored as 1 / 0), text, or
//!   `()` / `NaN` for a missing value. A row whose evaluation fails is missing.
//!
//! The function library ([`FUNCTIONS`]) covers missing values, mathematics,
//! probability distributions, text and regular expressions, dates, other rows
//! (`row()`, `lag`, `lead`), column summaries optionally by group (`col_mean`,
//! `col_sd`, `col_quantile`, …), cumulative sums, ranks and seeded random
//! numbers.
//!
//! # Recalculation
//!
//! [`refresh`] recomputes the formula columns affected by a change, in
//! dependency order. Every [`crate::editing::Edit`] calls it, so formula
//! columns follow edits, undo and redo. A formula's references are found by
//! scanning its text ([`references`]): identifier columns, and quoted names
//! given to the column functions. Renaming a column rewrites the formulas that
//! use it ([`renamed`]); formulas may not form a cycle ([`check`]).
//!
//! Cell edits, pastes, new rows, deletions and sorts report the rows they
//! touched ([`Changed::Partial`]). A row-wise formula ([`is_row_wise`])
//! recomputes only those rows; one that reads other rows recomputes them all.
//! While some rows are recomputed, the evaluation watches for reads beyond
//! the current row and falls back to every row if one happens.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::rc::Rc;

use rhai::{Dynamic, Engine, EvalAltResult, FLOAT, INT, ImmutableString, Position, Scope};
use stat_core::ContinuousDistribution;
use stat_core::descriptive::{QuantileMethod, quantile_sorted};
use stat_core::ranking::{TieRule, ranks_with_ties};
use stat_core::rng::Rng;
use stat_signal::Interpolation;
use stat_data::temporal::{civil_from_days, days_in_month, seconds_from_civil};
use stat_data::{CellValue, Column, ColumnData, ColumnKind, DataTable, MeasurementLevel, format_number};

use crate::script_text::{RESERVED, string};

/// Most Rhai operations one row's evaluation may take (stops runaway loops).
const MAX_OPERATIONS_PER_ROW: u64 = 200_000;

/// A formula that cannot be used, with where in its text the problem is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormulaError {
    /// What is wrong.
    pub message: String,
    /// Line (from 1) and column (from 1) in the formula, when known.
    pub position: Option<(usize, usize)>,
}

impl fmt::Display for FormulaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.position {
            Some((line, column)) => write!(f, "{} (line {line}, column {column})", self.message),
            None => f.write_str(&self.message),
        }
    }
}

impl std::error::Error for FormulaError {}

fn plain_error(message: impl Into<String>) -> FormulaError {
    FormulaError { message: message.into(), position: None }
}

fn position_of(p: Position) -> Option<(usize, usize)> {
    p.line().map(|line| (line, p.position().unwrap_or(1)))
}

/// One computed value.
#[derive(Debug, Clone, PartialEq)]
pub enum FormulaValue {
    /// No value.
    Missing,
    /// A number (or a date-time in seconds).
    Number(f64),
    /// Text.
    Text(String),
}

impl FormulaValue {
    /// Display text (empty when missing).
    #[must_use]
    pub fn text(&self) -> String {
        match self {
            Self::Missing => String::new(),
            Self::Number(x) => format_number(*x),
            Self::Text(s) => s.clone(),
        }
    }
}

/// The values a formula gives on some rows.
#[derive(Debug, Clone, PartialEq)]
pub struct Evaluation {
    /// One value per row evaluated.
    pub values: Vec<FormulaValue>,
    /// Rows (table indices) whose evaluation failed, with the message of the
    /// first failure.
    pub failures: Vec<(usize, String)>,
}

impl Evaluation {
    /// The column type the values suggest: text when any value is text,
    /// otherwise numeric.
    #[must_use]
    pub fn natural_kind(&self) -> ColumnKind {
        if self.values.iter().any(|v| matches!(v, FormulaValue::Text(_))) {
            ColumnKind::Text
        } else {
            ColumnKind::Numeric
        }
    }

    /// A summary of the failures, if any ("2 rows could not be computed; row
    /// 5: …"), with rows numbered from 1 as in the data view.
    #[must_use]
    pub fn failure_summary(&self) -> Option<String> {
        let (row, message) = self.failures.first()?;
        let n = self.failures.len();
        let rows = if n == 1 { "1 row".to_owned() } else { format!("{n} rows") };
        Some(format!("{rows} could not be computed and will be missing; row {}: {message}", row + 1))
    }
}

// ---------------------------------------------------------------------------
// Scanning formula text
// ---------------------------------------------------------------------------

/// Functions whose quoted arguments name columns.
pub const COLUMN_FUNCTIONS: &[&str] = &[
    "col",
    "lag",
    "lead",
    "col_mean",
    "col_sd",
    "col_min",
    "col_max",
    "col_sum",
    "col_count",
    "col_quantile",
    "cum_sum",
    "rank",
    "rank_ties",
    "p_adjust",
    "percent_of_range",
    "fraction_of_total",
    "subtract_baseline",
    "savgol",
    "moving_average",
    "running_median",
    "exp_smooth",
    "gaussian_smooth",
    "derivative",
    "cum_integral",
    "interpolate",
    "interpolate_spline",
    "interpolate_pchip",
    "lowpass_filter",
    "highpass_filter",
    "bandpass_filter",
    "bandstop_filter",
    "envelope",
    "instant_phase",
    "wavelet_denoise",
];

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Ident(String),
    /// A string literal and its value.
    Str(String),
    /// Any other character (operators, brackets, digits).
    Other(char),
}

/// The tokens of `text` with their byte ranges (comments and white space
/// dropped). Only what reference finding needs is distinguished.
fn scan(text: &str) -> Vec<(Token, std::ops::Range<usize>)> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let end_of = |k: usize| chars.get(k).map_or(text.len(), |(i, _)| *i);
    let mut out = Vec::new();
    let mut k = 0;
    while k < chars.len() {
        let (start, c) = chars[k];
        let next = chars.get(k + 1).map(|(_, c)| *c);
        if c.is_whitespace() {
            k += 1;
        } else if c == '/' && next == Some('/') {
            while k < chars.len() && chars[k].1 != '\n' {
                k += 1;
            }
        } else if c == '/' && next == Some('*') {
            // Block comments nest in Rhai.
            let mut depth = 0;
            while k < chars.len() {
                let (c, n) = (chars[k].1, chars.get(k + 1).map(|(_, c)| *c));
                if c == '/' && n == Some('*') {
                    depth += 1;
                    k += 2;
                } else if c == '*' && n == Some('/') {
                    depth -= 1;
                    k += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    k += 1;
                }
            }
        } else if c == '"' || c == '`' || c == '\'' {
            let mut value = String::new();
            k += 1;
            while k < chars.len() && chars[k].1 != c {
                if chars[k].1 == '\\' && c != '`' && k + 1 < chars.len() {
                    k += 1;
                    value.push(match chars[k].1 {
                        'n' => '\n',
                        't' => '\t',
                        'r' => '\r',
                        '0' => '\0',
                        other => other,
                    });
                } else {
                    value.push(chars[k].1);
                }
                k += 1;
            }
            k += 1;
            out.push((Token::Str(value), start..end_of(k)));
        } else if c.is_ascii_alphabetic() || c == '_' {
            let mut j = k;
            while j < chars.len() && (chars[j].1.is_ascii_alphanumeric() || chars[j].1 == '_') {
                j += 1;
            }
            out.push((Token::Ident(text[start..end_of(j)].to_owned()), start..end_of(j)));
            k = j;
        } else if c.is_ascii_digit() {
            // Numbers, including `1e5` and `0x1F`, are one token.
            let mut j = k;
            while j < chars.len() && (chars[j].1.is_ascii_alphanumeric() || chars[j].1 == '_' || chars[j].1 == '.') {
                j += 1;
            }
            out.push((Token::Other('0'), start..end_of(j)));
            k = j;
        } else {
            out.push((Token::Other(c), start..end_of(k + 1)));
            k += 1;
        }
    }
    out
}

/// Where a formula refers to a column: a bare identifier, or a quoted name
/// given to a column function.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Reference {
    Ident(String),
    Quoted(String),
}

/// A name the formula declares itself: a `let` / `const` variable, a
/// function, or a `for` loop variable (`for x in …`, `for (x, i) in …`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Declared {
    /// Index of the declaring token.
    at: usize,
    /// Index of the token that ends its scope: the closing brace of a loop's
    /// body (`None`: the rest of the formula, for `let`, `const` and `fn`).
    until: Option<usize>,
}

/// The names `tokens` declare (see [`Declared`]). Shared by reference finding
/// and renaming so that both agree on what is a local name.
fn declared_names(tokens: &[(Token, std::ops::Range<usize>)]) -> Vec<Declared> {
    let ident = |i: usize| match tokens.get(i) {
        Some((Token::Ident(n), _)) => Some(n.as_str()),
        _ => None,
    };
    let is = |i: usize, c: char| matches!(tokens.get(i), Some((Token::Other(o), _)) if *o == c);
    let mut out = Vec::new();
    for i in 0..tokens.len() {
        match ident(i) {
            Some("let" | "const" | "fn") if ident(i + 1).is_some() => out.push(Declared { at: i + 1, until: None }),
            Some("for") => {
                // The variables: one name, or a parenthesized list.
                let mut vars = Vec::new();
                let mut k = i + 1;
                if ident(k).is_some() {
                    vars.push(k);
                    k += 1;
                } else if is(k, '(') {
                    k += 1;
                    while k < tokens.len() && !is(k, ')') {
                        if ident(k).is_some() {
                            vars.push(k);
                        }
                        k += 1;
                    }
                }
                // Their scope is the loop body: the next `{ … }` block.
                let until = (k..tokens.len()).find(|&j| is(j, '{')).and_then(|open| {
                    let mut depth = 0usize;
                    (open..tokens.len()).find(|&j| {
                        if is(j, '{') {
                            depth += 1;
                        } else if is(j, '}') {
                            depth -= 1;
                        }
                        depth == 0
                    })
                });
                out.extend(vars.into_iter().map(|at| Declared { at, until }));
            }
            _ => {}
        }
    }
    out
}

/// Every reference in `text` with its byte range, whether or not such a
/// column exists.
fn find_references(text: &str) -> Vec<(Reference, std::ops::Range<usize>)> {
    let tokens = scan(text);
    let declared = declared_names(&tokens);
    let name_of = |i: usize| match &tokens[i].0 {
        Token::Ident(n) => n.as_str(),
        _ => "",
    };
    // A name is local at token `i` when the formula declared it before `i`
    // and its scope has not ended (`let` / `const` / `fn` names: from there
    // on; loop variables: to the end of the loop body).
    let is_local = |name: &str, i: usize| {
        declared.iter().any(|d| d.at < i && d.until.is_none_or(|u| i < u) && name_of(d.at) == name)
    };
    let mut out = Vec::new();
    // The function each open bracket belongs to (`None` for grouping).
    let mut calls: Vec<Option<&str>> = Vec::new();
    for (i, (token, range)) in tokens.iter().enumerate() {
        let prev = i.checked_sub(1).map(|p| &tokens[p].0);
        let next = tokens.get(i + 1).map(|t| &t.0);
        match token {
            Token::Other('(') => {
                let name = match prev {
                    Some(Token::Ident(f)) => Some(f.as_str()),
                    _ => None,
                };
                calls.push(name);
            }
            Token::Other(')') => {
                calls.pop();
            }
            Token::Ident(name) => {
                let is_call = next == Some(&Token::Other('('));
                let is_member = prev == Some(&Token::Other('.'));
                let is_declared = declared.iter().any(|d| d.at == i);
                if !is_declared && !is_call && !is_member && !is_local(name, i) {
                    out.push((Reference::Ident(name.clone()), range.clone()));
                }
            }
            Token::Str(value) => {
                if calls.last().copied().flatten().is_some_and(|f| COLUMN_FUNCTIONS.contains(&f)) {
                    out.push((Reference::Quoted(value.clone()), range.clone()));
                }
            }
            Token::Other(_) => {}
        }
    }
    out
}

/// `true` if `name` can be written bare in a formula.
#[must_use]
pub fn is_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !RESERVED.contains(&name)
}

/// How to write a reference to column `name` in a formula: bare when it is an
/// identifier, else `col("…")`.
#[must_use]
pub fn column_reference(name: &str) -> String {
    if is_identifier(name) { name.to_owned() } else { format!("col({})", string(name)) }
}

/// The columns of `table` that `formula` uses, as indices (ascending).
#[must_use]
pub fn references(table: &DataTable, formula: &str) -> Vec<usize> {
    let mut used: Vec<usize> = find_references(formula)
        .into_iter()
        .filter_map(|(r, _)| match r {
            Reference::Ident(name) => table.find(&name).filter(|_| is_identifier(&name)),
            Reference::Quoted(name) => table.find(&name),
        })
        .collect();
    used.sort_unstable();
    used.dedup();
    used
}

/// `formula` with references to column `old` changed to `new`.
#[must_use]
pub fn renamed(formula: &str, old: &str, new: &str) -> String {
    // A bare `new` would mean the formula's own variable of that name, if it
    // declares one; the column is then written `col("new")`.
    let tokens = scan(formula);
    let shadowed = declared_names(&tokens).iter().any(|d| matches!(&tokens[d.at].0, Token::Ident(n) if n == new));
    let bare_ok = is_identifier(new) && !shadowed;
    let mut out = String::with_capacity(formula.len());
    let mut last = 0;
    for (r, range) in find_references(formula) {
        let replacement = match r {
            Reference::Ident(name) if name == old && is_identifier(old) => {
                if bare_ok {
                    new.to_owned()
                } else {
                    format!("col({})", string(new))
                }
            }
            Reference::Quoted(name) if name == old => {
                // `col("new")` reads better bare when `new` is an identifier.
                let in_col = formula[..range.start].trim_end().strip_suffix('(').is_some_and(|before| {
                    before.trim_end().ends_with("col")
                        && !before.trim_end().ends_with("_col")
                        && formula[range.end..].trim_start().starts_with(')')
                });
                if in_col && bare_ok {
                    let open = formula[..range.start].trim_end().len() - 1;
                    let call_start = formula[..open].trim_end().len() - 3;
                    let close = range.end + (formula[range.end..].len() - formula[range.end..].trim_start().len());
                    out.push_str(&formula[last..call_start]);
                    out.push_str(new);
                    last = close + 1;
                    continue;
                }
                string(new)
            }
            _ => continue,
        };
        out.push_str(&formula[last..range.start]);
        out.push_str(&replacement);
        last = range.end;
    }
    out.push_str(&formula[last..]);
    out
}

/// The formula columns of `table` that are not on a cycle, in an order where
/// each comes after the formula columns it uses, and the cycles (groups of
/// columns whose formulas use each other, directly or through others).
///
/// Uses Tarjan's strongly connected components algorithm (Tarjan 1972,
/// "Depth-first search and linear graph algorithms", *SIAM J. Comput.*
/// 1(2)): components come out after every component they use, so the
/// single-column components, in that order, are a valid evaluation order;
/// larger components (and a column using itself) are the cycles.
fn evaluation_plan(table: &DataTable) -> (Vec<usize>, Vec<Vec<usize>>) {
    let formula_columns: Vec<usize> = (0..table.n_cols()).filter(|&j| table.columns()[j].formula.is_some()).collect();
    let uses: HashMap<usize, Vec<usize>> = formula_columns
        .iter()
        .map(|&j| {
            let f = table.columns()[j].formula.as_deref().unwrap_or_default();
            (j, references(table, f).into_iter().filter(|u| table.columns()[*u].formula.is_some()).collect())
        })
        .collect();
    struct Tarjan<'a> {
        uses: &'a HashMap<usize, Vec<usize>>,
        index: HashMap<usize, usize>,
        low: HashMap<usize, usize>,
        stack: Vec<usize>,
        on_stack: HashSet<usize>,
        components: Vec<Vec<usize>>,
    }
    impl Tarjan<'_> {
        fn visit(&mut self, v: usize) {
            let i = self.index.len();
            self.index.insert(v, i);
            self.low.insert(v, i);
            self.stack.push(v);
            self.on_stack.insert(v);
            for &w in self.uses.get(&v).map_or(&[][..], Vec::as_slice) {
                let reach = if !self.index.contains_key(&w) {
                    self.visit(w);
                    self.low[&w]
                } else if self.on_stack.contains(&w) {
                    self.index[&w]
                } else {
                    continue;
                };
                let low = self.low[&v].min(reach);
                self.low.insert(v, low);
            }
            if self.low[&v] == self.index[&v] {
                let mut component = Vec::new();
                while let Some(w) = self.stack.pop() {
                    self.on_stack.remove(&w);
                    component.push(w);
                    if w == v {
                        break;
                    }
                }
                // Discovery order, which follows the uses round the cycle.
                component.reverse();
                self.components.push(component);
            }
        }
    }
    let mut tarjan = Tarjan {
        uses: &uses,
        index: HashMap::new(),
        low: HashMap::new(),
        stack: Vec::new(),
        on_stack: HashSet::new(),
        components: Vec::new(),
    };
    for &j in &formula_columns {
        if !tarjan.index.contains_key(&j) {
            tarjan.visit(j);
        }
    }
    let mut order = Vec::new();
    let mut cycles = Vec::new();
    for component in tarjan.components {
        let self_use = |j: usize| uses.get(&j).is_some_and(|u| u.contains(&j));
        if component.len() == 1 && !self_use(component[0]) {
            order.push(component[0]);
        } else {
            cycles.push(component);
        }
    }
    (order, cycles)
}

// ---------------------------------------------------------------------------
// Evaluation
// ---------------------------------------------------------------------------

type Fallible<T> = std::result::Result<T, Box<EvalAltResult>>;

fn fail<T>(message: impl Into<String>) -> Fallible<T> {
    Err(message.into().into())
}

/// What the library functions can see while a formula runs.
struct Context {
    n_rows: usize,
    /// The columns the formula names (by name).
    columns: HashMap<String, Column>,
    /// The current row (table index).
    row: Cell<usize>,
    /// Per-row results of column summaries, by function, column and group.
    summaries: RefCell<HashMap<String, Rc<Vec<f64>>>>,
    regexes: RefCell<HashMap<String, Rc<regex::Regex>>>,
    /// Interpolants of `interpolate…`, by method and columns.
    interpolators: RefCell<HashMap<String, Option<Rc<stat_signal::Interpolator>>>>,
    seed: Cell<u64>,
    rng: RefCell<Option<Rng>>,
    /// Set when the evaluation used more than the current row's values
    /// (another row, a column summary, the row's position, a random draw),
    /// so recomputing only some rows would not do.
    other_rows: Cell<bool>,
}

impl Context {
    fn column(&self, name: &str) -> Fallible<&Column> {
        self.columns.get(name).map_or_else(
            || fail(format!("there is no column named {name:?} (column names in functions are written in quotes)")),
            Ok,
        )
    }

    /// Value of `name` at `row` as a formula sees it (`None` past the ends).
    fn value_at(&self, name: &str, row: Option<usize>) -> Fallible<Dynamic> {
        let col = self.column(name)?;
        if row != Some(self.row.get()) {
            self.other_rows.set(true);
        }
        Ok(match row.filter(|&r| r < self.n_rows) {
            Some(r) => cell(col, r),
            None => match col.data {
                ColumnData::Text(_) => Dynamic::from(ImmutableString::new()),
                _ => Dynamic::from_float(f64::NAN),
            },
        })
    }

    fn numbers(&self, name: &str) -> Fallible<&[f64]> {
        self.column(name)?.as_numeric().map_or_else(|| fail(format!("column {name:?} holds text, not numbers")), Ok)
    }

    /// The group (`by` column's text) of each row, or one group.
    fn groups(&self, by: Option<&str>) -> Fallible<Vec<usize>> {
        let Some(by) = by else { return Ok(vec![0; self.n_rows]) };
        let col = self.column(by)?;
        let mut ids: HashMap<String, usize> = HashMap::new();
        Ok((0..self.n_rows)
            .map(|r| {
                let n = ids.len();
                *ids.entry(col.cell_text(r)).or_insert(n)
            })
            .collect())
    }

    /// `kind` of summary of `name` (by group), for the current row.
    fn summary(&self, kind: &str, name: &str, by: Option<&str>, p: f64) -> Fallible<f64> {
        self.other_rows.set(true);
        let key = format!("{kind}\u{1}{name}\u{1}{}\u{1}{p}", by.unwrap_or("\u{2}"));
        if let Some(v) = self.summaries.borrow().get(&key) {
            return Ok(v[self.row.get()]);
        }
        let x = self.numbers(name)?;
        let groups = self.groups(by)?;
        let n_groups = groups.iter().max().map_or(0, |m| m + 1);
        let per_row: Vec<f64> = match kind {
            "cum_sum" => {
                let mut running = vec![0.0; n_groups];
                groups
                    .iter()
                    .zip(x)
                    .map(|(&g, &v)| {
                        if !v.is_nan() {
                            running[g] += v;
                        }
                        running[g]
                    })
                    .collect()
            }
            k if k.starts_with("p_adjust:") => {
                let method = &k["p_adjust:".len()..];
                let mut out = vec![f64::NAN; x.len()];
                for g in 0..n_groups {
                    let members: Vec<usize> = (0..x.len()).filter(|&r| groups[r] == g).collect();
                    let p: Vec<f64> = members.iter().map(|&r| x[r]).collect();
                    let adjusted = match method {
                        "q" => stat_inference::pvalues::storey_q_values(&p, 0.5).map(|q| q.q_values),
                        m => match stat_inference::pvalues::AdjustMethod::from_key(m) {
                            Some(m) => stat_inference::pvalues::adjust_p_values(&p, m),
                            None => return fail(format!("unknown adjustment {m:?}")),
                        },
                    };
                    // A group with no p-values leaves its rows missing.
                    let Ok(adjusted) = adjusted else {
                        if p.iter().all(|v| v.is_nan()) {
                            continue;
                        }
                        return fail(format!("column {name:?}: p-values must lie between 0 and 1"));
                    };
                    for (&r, v) in members.iter().zip(adjusted) {
                        out[r] = v;
                    }
                }
                out
            }
            "percent_of_range" | "fraction_of_total" => {
                let t = if kind == "percent_of_range" {
                    crate::CurveTransform::PercentOfRange
                } else {
                    crate::CurveTransform::FractionOfTotal
                };
                crate::curve_transform(x, &groups, t)
            }
            k if k.starts_with("subtract_baseline:") => {
                let points: usize = k["subtract_baseline:".len()..].parse().unwrap_or(0);
                crate::curve_transform(x, &groups, crate::CurveTransform::SubtractFirst(points))
            }
            k if k.starts_with("signal:") => signal_transform(&k["signal:".len()..], x, &groups, n_groups)?,
            k if k.starts_with("xy:") => {
                let (what, x_name) = k["xy:".len()..].split_once('\u{3}').unwrap_or(("", ""));
                xy_transform(what, self.numbers(x_name)?, x, &groups, n_groups)
            }
            k if k.starts_with("sigproc:") => {
                match crate::signal_analysis::formula_transform(&k["sigproc:".len()..], x, &groups) {
                    Ok(v) => v,
                    Err(e) => return fail(format!("column {name:?}: {e}")),
                }
            }
            k if k == "rank" || k.starts_with("rank:") => {
                let rule = k.strip_prefix("rank:").and_then(TieRule::from_key).unwrap_or_default();
                let mut ranks = vec![f64::NAN; x.len()];
                for g in 0..n_groups {
                    let members: Vec<usize> = (0..x.len()).filter(|&r| groups[r] == g).collect();
                    let values: Vec<f64> = members.iter().map(|&r| x[r]).collect();
                    for (&r, v) in members.iter().zip(ranks_with_ties(&values, rule)) {
                        ranks[r] = v;
                    }
                }
                ranks
            }
            _ => {
                let mut values: Vec<Vec<f64>> = vec![Vec::new(); n_groups];
                for (&g, &v) in groups.iter().zip(x) {
                    if !v.is_nan() {
                        values[g].push(v);
                    }
                }
                let stat: Vec<f64> = values.iter_mut().map(|v| group_statistic(kind, v, p)).collect();
                groups.iter().map(|&g| stat[g]).collect()
            }
        };
        let value = per_row[self.row.get()];
        self.summaries.borrow_mut().insert(key, Rc::new(per_row));
        Ok(value)
    }

    /// `method` interpolant of `y_name` against `x_name` evaluated at `at`.
    fn interpolate(&self, method: Interpolation, y_name: &str, x_name: &str, at: f64) -> Fallible<f64> {
        self.other_rows.set(true);
        let key = format!("{}\u{1}{y_name}\u{1}{x_name}", method.key());
        let cached = self.interpolators.borrow().get(&key).cloned();
        let f = match cached {
            Some(f) => f,
            None => {
                let (x, y) = sorted_pairs(self.numbers(x_name)?, self.numbers(y_name)?, None);
                let f = stat_signal::Interpolator::new(&x, &y, method).ok().map(Rc::new);
                self.interpolators.borrow_mut().insert(key, f.clone());
                f
            }
        };
        Ok(f.map_or(f64::NAN, |f| if at.is_finite() { f.eval(at) } else { f64::NAN }))
    }

    fn regex(&self, pattern: &str) -> Fallible<Rc<regex::Regex>> {
        if let Some(r) = self.regexes.borrow().get(pattern) {
            return Ok(Rc::clone(r));
        }
        let r = Rc::new(regex::Regex::new(pattern).map_err(|e| format!("invalid regular expression: {e}"))?);
        self.regexes.borrow_mut().insert(pattern.to_owned(), Rc::clone(&r));
        Ok(r)
    }

    fn random(&self) -> f64 {
        self.other_rows.set(true);
        let mut rng = self.rng.borrow_mut();
        rng.get_or_insert_with(|| {
            let mut seeder = Rng::new(self.seed.get() ^ 0x5DEE_CE66_D1CE_4E5B);
            Rng::new(seeder.next_u64() ^ (self.row.get() as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15))
        })
        .next_f64()
    }
}

/// The summary kind of `p_adjust` with `method` (a method key, or `"q"` for
/// Storey q-values).
fn adjust_kind(method: &str) -> Fallible<String> {
    let m = method.trim().to_lowercase();
    if m == "q" || stat_inference::pvalues::AdjustMethod::from_key(&m).is_some() {
        Ok(format!("p_adjust:{m}"))
    } else {
        fail(format!(
            "unknown adjustment {method:?}: use \"bonferroni\", \"sidak\", \"holm\", \"hochberg\", \"bh\", \"by\" \
             or \"q\""
        ))
    }
}

/// A summary statistic of one group's non-missing values (sorted in place
/// for quantiles); `NaN` when undefined.
/// Rows of each group (in row order) whose value is present.
fn group_members(x: &[f64], groups: &[usize], n_groups: usize) -> Vec<Vec<usize>> {
    let mut members = vec![Vec::new(); n_groups];
    for (r, (&g, &v)) in groups.iter().zip(x).enumerate() {
        if !v.is_nan() {
            members[g].push(r);
        }
    }
    members
}

/// Smoother named by `spec` (`savgol:w:o`, `moving_average:w`,
/// `running_median:w`, `exp_smooth:alpha`, `gaussian_smooth:sigma`) applied
/// to each group's present values in row order; missing values stay missing.
fn signal_transform(spec: &str, x: &[f64], groups: &[usize], n_groups: usize) -> Fallible<Vec<f64>> {
    use stat_signal::{MovingAverage, Smoother};
    let parts: Vec<&str> = spec.split(':').collect();
    let int = |i: usize| parts.get(i).and_then(|s| s.parse::<usize>().ok()).unwrap_or(0);
    let real = |i: usize| parts.get(i).and_then(|s| s.parse::<f64>().ok()).unwrap_or(f64::NAN);
    let smoother = match parts[0] {
        "savgol" => Smoother::SavitzkyGolay { window: int(1), order: int(2), derivative: 0 },
        "moving_average" => Smoother::MovingAverage { window: int(1), kind: MovingAverage::Centred },
        "running_median" => Smoother::RunningMedian { window: int(1) },
        "exp_smooth" => Smoother::Exponential { alpha: real(1) },
        "gaussian_smooth" => Smoother::Gaussian { sigma: real(1) },
        other => return fail(format!("unknown smoother {other:?}")),
    };
    let mut out = vec![f64::NAN; x.len()];
    for members in group_members(x, groups, n_groups) {
        if members.is_empty() {
            continue;
        }
        let values: Vec<f64> = members.iter().map(|&r| x[r]).collect();
        let smoothed = stat_signal::smooth(&values, &smoother, 1.0).map_err(|e| format!("{}: {e}", parts[0]))?;
        for (&r, v) in members.iter().zip(smoothed) {
            out[r] = v;
        }
    }
    Ok(out)
}

/// `derivative` or `cum_integral` of `y` against `x` within each group: the
/// group's rows sorted by x (duplicate x averaged), the result given back to
/// every row at its x. Groups with fewer than two distinct x stay missing.
fn xy_transform(what: &str, x: &[f64], y: &[f64], groups: &[usize], n_groups: usize) -> Vec<f64> {
    use stat_signal::Integration;
    let mut out = vec![f64::NAN; y.len()];
    let present: Vec<f64> = x.iter().zip(y).map(|(&a, &b)| if a.is_finite() && b.is_finite() { 0.0 } else { f64::NAN }).collect();
    for members in group_members(&present, groups, n_groups) {
        let (xs, ys) = sorted_pairs(x, y, Some(&members));
        if xs.len() < 2 {
            continue;
        }
        let result = match what {
            "derivative" => stat_signal::gradient(&xs, &ys),
            _ => stat_signal::cumulative_integral(&xs, &ys, Integration::Trapezoid),
        };
        let Ok(result) = result else { continue };
        for &r in &members {
            if let Ok(i) = xs.binary_search_by(|v| v.total_cmp(&x[r])) {
                out[r] = result[i];
            }
        }
    }
    out
}

/// The (x, y) pairs of `members` (or all rows) with both present, sorted by x,
/// with the y of duplicate x averaged.
fn sorted_pairs(x: &[f64], y: &[f64], members: Option<&[usize]>) -> (Vec<f64>, Vec<f64>) {
    let all: Vec<usize>;
    let rows = match members {
        Some(m) => m,
        None => {
            all = (0..x.len().min(y.len())).collect();
            &all
        }
    };
    let mut pairs: Vec<(f64, f64)> =
        rows.iter().map(|&r| (x[r], y[r])).filter(|(a, b)| a.is_finite() && b.is_finite()).collect();
    pairs.sort_by(|a, b| a.0.total_cmp(&b.0));
    let (mut xs, mut ys): (Vec<f64>, Vec<f64>) = (Vec::new(), Vec::new());
    let mut count = 0.0;
    for (a, b) in pairs {
        if xs.last() == Some(&a) {
            let last = ys.last_mut().expect("paired with xs");
            count += 1.0;
            *last += (b - *last) / count;
        } else {
            xs.push(a);
            ys.push(b);
            count = 1.0;
        }
    }
    (xs, ys)
}

fn group_statistic(kind: &str, v: &mut [f64], p: f64) -> f64 {
    let n = v.len() as f64;
    match kind {
        "col_count" => n,
        "col_sum" => stat_core::summation::sum(v),
        _ if v.is_empty() => f64::NAN,
        "col_mean" => stat_core::summation::sum(v) / n,
        "col_min" => v.iter().copied().fold(f64::INFINITY, f64::min),
        "col_max" => v.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        "col_sd" => {
            if v.len() < 2 {
                return f64::NAN;
            }
            let mean = stat_core::summation::sum(v) / n;
            let ss = stat_core::summation::sum_iter(v.iter().map(|x| (x - mean) * (x - mean)));
            (ss / (n - 1.0)).sqrt()
        }
        "col_quantile" => {
            v.sort_by(f64::total_cmp);
            quantile_sorted(v, p, QuantileMethod::default()).unwrap_or(f64::NAN)
        }
        _ => f64::NAN,
    }
}

/// A cell as a formula value: numbers (and date-times) as decimals, missing
/// numbers as `NaN`, text as strings.
fn cell(col: &Column, r: usize) -> Dynamic {
    match &col.data {
        ColumnData::Numeric(v) | ColumnData::DateTime { values: v, .. } => Dynamic::from_float(v[r]),
        ColumnData::Text(v) => Dynamic::from(ImmutableString::from(v[r].as_str())),
    }
}

/// A numeric argument: integers and decimals, `()` as missing.
fn num(x: &Dynamic) -> Fallible<f64> {
    if let Ok(f) = x.as_float() {
        Ok(f)
    } else if let Ok(i) = x.as_int() {
        Ok(i as f64)
    } else if let Ok(b) = x.as_bool() {
        Ok(if b { 1.0 } else { 0.0 })
    } else if x.is_unit() {
        Ok(f64::NAN)
    } else if x.is_string() {
        let s = x.clone().into_immutable_string().unwrap_or_default();
        if s.trim().is_empty() {
            Ok(f64::NAN)
        } else {
            fail(format!("expected a number, got the text {:?}", s.as_str()))
        }
    } else {
        fail(format!("expected a number, got {}", x.type_name()))
    }
}

/// Text of any value, numbers written as in the data view.
fn text_of(x: &Dynamic) -> String {
    if let Ok(f) = x.as_float() {
        if f.is_nan() { String::new() } else { format_number(f) }
    } else if let Ok(i) = x.as_int() {
        i.to_string()
    } else if x.is_unit() {
        String::new()
    } else {
        x.to_string()
    }
}

fn whole(x: f64, what: &str) -> Fallible<i64> {
    if x.is_finite() && x.fract() == 0.0 { Ok(x as i64) } else { fail(format!("{what} must be a whole number")) }
}

const DAY: f64 = 86_400.0;

/// Calendar parts of `seconds`: (year, month, day, seconds into the day).
fn civil(seconds: f64) -> (i64, u32, u32, f64) {
    let days = (seconds / DAY).floor();
    let (y, m, d) = civil_from_days(days as i64);
    (y, m, d, seconds - days * DAY)
}

fn unit_seconds(unit: &str) -> Option<f64> {
    Some(match unit.trim_end_matches('s') {
        "second" => 1.0,
        "minute" => 60.0,
        "hour" => 3600.0,
        "day" => DAY,
        "week" => 7.0 * DAY,
        _ => return None,
    })
}

fn add_months(seconds: f64, months: i64) -> f64 {
    let (y, m, d, rest) = civil(seconds);
    let total = y * 12 + i64::from(m) - 1 + months;
    let (y, m) = (total.div_euclid(12), (total.rem_euclid(12) + 1) as u32);
    let d = d.min(days_in_month(y, m));
    seconds_from_civil(y, m, d, 0, 0, 0.0) + rest
}

/// Whole calendar months from `a` to `b` (negative when `b` is earlier).
fn months_between(a: f64, b: f64) -> f64 {
    if b < a {
        return -months_between(b, a);
    }
    // A month is complete when the day of the month (and time of day) is
    // reached again, without clamping to shorter months.
    let (ya, ma, da, ta) = civil(a);
    let (yb, mb, db, tb) = civil(b);
    let mut n = (yb - ya) * 12 + i64::from(mb) - i64::from(ma);
    if (db, tb) < (da, ta) {
        n -= 1;
    }
    n as f64
}

const MONTH_NAMES: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const DAY_NAMES: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];

/// ISO weekday, 1 (Monday) to 7 (Sunday).
fn weekday(seconds: f64) -> u32 {
    // 1970-01-01 was a Thursday.
    ((seconds / DAY).floor() as i64 + 3).rem_euclid(7) as u32 + 1
}

fn day_of_year(seconds: f64) -> u32 {
    let (y, ..) = civil(seconds);
    ((seconds - seconds_from_civil(y, 1, 1, 0, 0, 0.0)) / DAY).floor() as u32 + 1
}

/// `seconds` written with `%`-codes: `%Y` year, `%y` two-digit year, `%m`
/// month, `%d` day, `%H` hour, `%M` minute, `%S` second, `%j` day of the year,
/// `%B`/`%b` month name (full/short), `%A`/`%a` day name, `%%` a percent sign.
fn format_date(seconds: f64, pattern: &str) -> String {
    if !seconds.is_finite() {
        return String::new();
    }
    let (y, m, d, rest) = civil(seconds);
    let rest = rest.floor() as u32;
    let mut out = String::new();
    let mut chars = pattern.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('Y') => out.push_str(&y.to_string()),
            Some('y') => out.push_str(&format!("{:02}", y.rem_euclid(100))),
            Some('m') => out.push_str(&format!("{m:02}")),
            Some('d') => out.push_str(&format!("{d:02}")),
            Some('H') => out.push_str(&format!("{:02}", rest / 3600)),
            Some('M') => out.push_str(&format!("{:02}", rest / 60 % 60)),
            Some('S') => out.push_str(&format!("{:02}", rest % 60)),
            Some('j') => out.push_str(&format!("{:03}", day_of_year(seconds))),
            Some('B') => out.push_str(MONTH_NAMES[m as usize - 1]),
            Some('b') => out.push_str(&MONTH_NAMES[m as usize - 1][..3]),
            Some('A') => out.push_str(DAY_NAMES[weekday(seconds) as usize - 1]),
            Some('a') => out.push_str(&DAY_NAMES[weekday(seconds) as usize - 1][..3]),
            Some('%') => out.push('%'),
            Some(other) => {
                out.push('%');
                out.push(other);
            }
            None => out.push('%'),
        }
    }
    out
}

/// Column `k` (from 1) of the spline basis with knots `[a, t₁, …, b]`
/// (boundary knots first and last) at `x`; NaN for a missing `x`.
fn spline_column(x: &Dynamic, knots: &rhai::Array, kind: stat_linear::SplineKind, k: INT) -> Fallible<FLOAT> {
    let x = num(x)?;
    let knots: Vec<f64> = knots.iter().map(num).collect::<Fallible<_>>()?;
    let (Some(&a), Some(&b)) = (knots.first(), knots.last()) else {
        return fail("a spline needs its boundary knots");
    };
    if knots.len() < 2 {
        return fail("a spline needs its boundary knots");
    }
    let basis = stat_linear::SplineBasis::new(kind, (a, b), knots[1..knots.len() - 1].to_vec())
        .map_err(|e| -> Box<EvalAltResult> { e.to_string().into() })?;
    let column = usize::try_from(k).ok().filter(|&k| k >= 1 && k <= basis.n_columns());
    let Some(column) = column else {
        return fail(format!("the spline has columns 1 to {}", basis.n_columns()));
    };
    Ok(basis.evaluate(x)[column - 1])
}

/// Register `f` under `name` for one numeric argument of any number type.
fn unary(engine: &mut Engine, name: &str, f: fn(f64) -> f64) {
    engine.register_fn(name, move |a: Dynamic| -> Fallible<FLOAT> { Ok(f(num(&a)?)) });
}

fn binary(engine: &mut Engine, name: &str, f: fn(f64, f64) -> f64) {
    engine.register_fn(name, move |a: Dynamic, b: Dynamic| -> Fallible<FLOAT> { Ok(f(num(&a)?, num(&b)?)) });
}

fn ternary(engine: &mut Engine, name: &str, f: fn(f64, f64, f64) -> f64) {
    engine.register_fn(name, move |a: Dynamic, b: Dynamic, c: Dynamic| -> Fallible<FLOAT> {
        Ok(f(num(&a)?, num(&b)?, num(&c)?))
    });
}

fn or_nan(r: stat_core::Result<f64>) -> f64 {
    r.unwrap_or(f64::NAN)
}

fn count(k: f64) -> Option<u64> {
    (k.is_finite() && k >= 0.0 && k.fract() == 0.0).then_some(k as u64)
}

/// The engine formulas run in, with the function library bound to `ctx`.
fn engine(ctx: &Rc<Context>) -> Engine {
    let mut engine = Engine::new();
    engine.set_max_operations(MAX_OPERATIONS_PER_ROW);
    engine.set_max_expr_depths(64, 32);
    engine.set_strict_variables(true);
    // Let the `/` below replace whole-number division.
    engine.set_fast_operators(false);
    engine.on_print(|_| {});
    engine.on_debug(|_, _, _| {});
    engine.disable_symbol("eval");

    // Exact division for whole numbers too.
    engine.register_fn("/", |a: INT, b: INT| -> FLOAT { a as f64 / b as f64 });

    // Spline basis columns, as saved prediction formulas of spline terms use them.
    engine.register_fn("natural_spline", |x: Dynamic, knots: rhai::Array, k: INT| -> Fallible<FLOAT> {
        spline_column(&x, &knots, stat_linear::SplineKind::Natural, k)
    });
    engine.register_fn("bspline", |x: Dynamic, knots: rhai::Array, degree: INT, k: INT| -> Fallible<FLOAT> {
        let degree = u8::try_from(degree).map_err(|_| -> Box<EvalAltResult> { "bspline: bad degree".into() })?;
        spline_column(&x, &knots, stat_linear::SplineKind::BSpline { degree }, k)
    });
    // Missing values.
    engine.register_fn("missing", || Dynamic::UNIT);
    engine.register_fn("is_missing", |x: Dynamic| -> bool {
        x.is_unit()
            || x.as_float().is_ok_and(f64::is_nan)
            || (x.is_string() && x.into_immutable_string().unwrap_or_default().is_empty())
    });
    engine.register_fn("if_missing", |x: Dynamic, other: Dynamic| -> Dynamic {
        let missing = x.is_unit()
            || x.as_float().is_ok_and(f64::is_nan)
            || (x.is_string() && x.clone().into_immutable_string().unwrap_or_default().is_empty());
        if missing { other } else { x }
    });

    // Mathematics (any number type).
    for (name, f) in [
        ("sqrt", f64::sqrt as fn(f64) -> f64),
        ("exp", f64::exp),
        ("ln", f64::ln),
        ("log", f64::log10),
        ("log10", f64::log10),
        ("log2", f64::log2),
        ("sin", f64::sin),
        ("cos", f64::cos),
        ("tan", f64::tan),
        ("asin", f64::asin),
        ("acos", f64::acos),
        ("atan", f64::atan),
        ("tanh", f64::tanh),
        ("floor", f64::floor),
        ("ceiling", f64::ceil),
        ("round", f64::round),
        ("abs", f64::abs),
        ("sign", |x: f64| if x.is_nan() || x == 0.0 { x } else { x.signum() }),
        ("logit", |p: f64| (p / (1.0 - p)).ln()),
        ("logistic", |x: f64| 1.0 / (1.0 + (-x).exp())),
        ("lambert_w", stat_core::special::lambert_w),
        ("wright_omega", stat_core::special::wright_omega),
    ] {
        unary(&mut engine, name, f);
    }
    binary(&mut engine, "round", |x, digits| {
        let scale = 10f64.powi(digits as i32);
        (x * scale).round() / scale
    });
    binary(&mut engine, "pow", f64::powf);
    binary(&mut engine, "atan2", f64::atan2);
    binary(&mut engine, "min", |a, b| if a.is_nan() || b.is_nan() { f64::NAN } else { a.min(b) });
    binary(&mut engine, "max", |a, b| if a.is_nan() || b.is_nan() { f64::NAN } else { a.max(b) });
    binary(&mut engine, "modulo", f64::rem_euclid);

    // Probability distributions.
    unary(&mut engine, "normal_cdf", |x| or_nan(stat_core::Normal::standard().cdf(x)));
    ternary(&mut engine, "normal_cdf", |x, m, s| or_nan(stat_core::Normal::new(m, s).and_then(|d| d.cdf(x))));
    unary(&mut engine, "normal_pdf", |x| stat_core::Normal::standard().pdf(x));
    ternary(&mut engine, "normal_pdf", |x, m, s| stat_core::Normal::new(m, s).map_or(f64::NAN, |d| d.pdf(x)));
    unary(&mut engine, "normal_quantile", |p| or_nan(stat_core::Normal::standard().quantile(p)));
    ternary(&mut engine, "normal_quantile", |p, m, s| or_nan(stat_core::Normal::new(m, s).and_then(|d| d.quantile(p))));
    binary(&mut engine, "t_cdf", |x, df| or_nan(stat_core::StudentsT::new(df).and_then(|d| d.cdf(x))));
    binary(&mut engine, "t_quantile", |p, df| or_nan(stat_core::StudentsT::new(df).and_then(|d| d.quantile(p))));
    binary(&mut engine, "chisq_cdf", |x, df| or_nan(stat_core::ChiSquared::new(df).and_then(|d| d.cdf(x))));
    binary(&mut engine, "chisq_quantile", |p, df| or_nan(stat_core::ChiSquared::new(df).and_then(|d| d.quantile(p))));
    ternary(&mut engine, "f_cdf", |x, a, b| or_nan(stat_core::FisherSnedecor::new(a, b).and_then(|d| d.cdf(x))));
    ternary(&mut engine, "f_quantile", |p, a, b| {
        or_nan(stat_core::FisherSnedecor::new(a, b).and_then(|d| d.quantile(p)))
    });
    ternary(&mut engine, "binomial_pmf", |k, n, p| match (count(k), count(n)) {
        (Some(k), Some(n)) => or_nan(stat_core::discrete::binomial_pmf(k, n, p)),
        _ => f64::NAN,
    });
    ternary(&mut engine, "binomial_cdf", |k, n, p| match (count(k), count(n)) {
        (Some(k), Some(n)) => or_nan(stat_core::discrete::binomial_cdf(k, n, p)),
        _ => f64::NAN,
    });
    binary(&mut engine, "poisson_pmf", |k, m| {
        count(k).map_or(f64::NAN, |k| or_nan(stat_core::discrete::poisson_pmf(k, m)))
    });
    binary(&mut engine, "poisson_cdf", |k, m| {
        count(k).map_or(f64::NAN, |k| or_nan(stat_core::discrete::poisson_cdf(k, m)))
    });

    // Text.
    engine.register_fn("text", |x: Dynamic| text_of(&x));
    engine.register_fn("number", |x: Dynamic| -> FLOAT {
        if x.is_string() {
            x.into_immutable_string().unwrap_or_default().trim().parse::<f64>().unwrap_or(f64::NAN)
        } else {
            num(&x).unwrap_or(f64::NAN)
        }
    });
    for arity in 2..=6 {
        match arity {
            2 => engine.register_fn("concat", |a: Dynamic, b: Dynamic| format!("{}{}", text_of(&a), text_of(&b))),
            3 => engine.register_fn("concat", |a: Dynamic, b: Dynamic, c: Dynamic| {
                [a, b, c].iter().map(text_of).collect::<String>()
            }),
            4 => engine.register_fn("concat", |a: Dynamic, b: Dynamic, c: Dynamic, d: Dynamic| {
                [a, b, c, d].iter().map(text_of).collect::<String>()
            }),
            5 => engine.register_fn("concat", |a: Dynamic, b: Dynamic, c: Dynamic, d: Dynamic, e: Dynamic| {
                [a, b, c, d, e].iter().map(text_of).collect::<String>()
            }),
            _ => engine.register_fn(
                "concat",
                |a: Dynamic, b: Dynamic, c: Dynamic, d: Dynamic, e: Dynamic, f: Dynamic| {
                    [a, b, c, d, e, f].iter().map(text_of).collect::<String>()
                },
            ),
        };
    }
    engine.register_fn("upper", |s: &str| s.to_uppercase());
    engine.register_fn("lower", |s: &str| s.to_lowercase());
    engine.register_fn("trim", |s: &str| s.trim().to_owned());
    engine.register_fn("length", |s: &str| s.chars().count() as INT);
    engine.register_fn("substr", |s: &str, start: INT, len: INT| -> String {
        let start = usize::try_from(start).unwrap_or(0);
        let len = usize::try_from(len).unwrap_or(0);
        s.chars().skip(start).take(len).collect()
    });
    engine.register_fn("substr", |s: &str, start: INT| -> String {
        s.chars().skip(usize::try_from(start).unwrap_or(0)).collect()
    });
    engine.register_fn("find", |s: &str, what: &str| -> INT {
        s.find(what).map_or(-1, |byte| s[..byte].chars().count() as INT)
    });
    engine.register_fn("replace", |s: &str, what: &str, with: &str| -> String {
        if what.is_empty() { s.to_owned() } else { s.replace(what, with) }
    });
    engine.register_fn("word", |s: &str, n: INT, sep: &str| -> String {
        let n = usize::try_from(n).unwrap_or(usize::MAX);
        if sep.is_empty() {
            s.split_whitespace().nth(n).unwrap_or_default().to_owned()
        } else {
            s.split(sep).nth(n).unwrap_or_default().to_owned()
        }
    });
    engine.register_fn("word", |s: &str, n: INT| -> String {
        s.split_whitespace().nth(usize::try_from(n).unwrap_or(usize::MAX)).unwrap_or_default().to_owned()
    });
    // Multiple-response (check-all-that-apply) lists: items between separators.
    engine.register_fn("has_item", |s: &str, item: &str, sep: &str| -> bool {
        crate::split_items(s, sep).iter().any(|i| i == item.trim())
    });
    engine.register_fn("has_item", |s: &str, item: &str| -> bool {
        crate::split_items(s, ";").iter().any(|i| i == item.trim())
    });
    engine.register_fn("item_count", |s: &str, sep: &str| crate::split_items(s, sep).len() as INT);
    engine.register_fn("item_count", |s: &str| crate::split_items(s, ";").len() as INT);
    let c = Rc::clone(ctx);
    engine.register_fn("regex_match", move |s: &str, pattern: &str| -> Fallible<bool> {
        Ok(c.regex(pattern)?.is_match(s))
    });
    let c = Rc::clone(ctx);
    engine.register_fn("regex_find", move |s: &str, pattern: &str| -> Fallible<String> {
        Ok(c.regex(pattern)?.find(s).map(|m| m.as_str().to_owned()).unwrap_or_default())
    });
    let c = Rc::clone(ctx);
    engine.register_fn("regex_replace", move |s: &str, pattern: &str, with: &str| -> Fallible<String> {
        Ok(c.regex(pattern)?.replace_all(s, with).into_owned())
    });

    // Dates and times (seconds since 1970-01-01 00:00 UTC).
    unary(&mut engine, "year", |d| if d.is_finite() { civil(d).0 as f64 } else { f64::NAN });
    unary(&mut engine, "month", |d| if d.is_finite() { f64::from(civil(d).1) } else { f64::NAN });
    unary(&mut engine, "day", |d| if d.is_finite() { f64::from(civil(d).2) } else { f64::NAN });
    unary(&mut engine, "hour", |d| if d.is_finite() { (civil(d).3 / 3600.0).floor() } else { f64::NAN });
    unary(&mut engine, "minute", |d| if d.is_finite() { (civil(d).3 / 60.0).floor() % 60.0 } else { f64::NAN });
    unary(&mut engine, "second", |d| if d.is_finite() { civil(d).3 % 60.0 } else { f64::NAN });
    unary(&mut engine, "weekday", |d| if d.is_finite() { f64::from(weekday(d)) } else { f64::NAN });
    unary(&mut engine, "day_of_year", |d| if d.is_finite() { f64::from(day_of_year(d)) } else { f64::NAN });
    unary(&mut engine, "date_part", |d| if d.is_finite() { (d / DAY).floor() * DAY } else { f64::NAN });
    let make_date = |y: f64, m: f64, d: f64| -> Fallible<FLOAT> {
        if [y, m, d].iter().any(|v| v.is_nan()) {
            return Ok(f64::NAN);
        }
        let (y, m, d) = (whole(y, "the year")?, whole(m, "the month")?, whole(d, "the day")?);
        match (u32::try_from(m), u32::try_from(d)) {
            (Ok(m @ 1..=12), Ok(d)) if d >= 1 && d <= days_in_month(y, m) => Ok(seconds_from_civil(y, m, d, 0, 0, 0.0)),
            _ => fail(format!("{y}-{m}-{d} is not a date")),
        }
    };
    engine.register_fn("make_date", move |y: Dynamic, m: Dynamic, d: Dynamic| make_date(num(&y)?, num(&m)?, num(&d)?));
    engine.register_fn(
        "make_datetime",
        move |y: Dynamic, m: Dynamic, d: Dynamic, h: Dynamic, mi: Dynamic, s: Dynamic| -> Fallible<FLOAT> {
            Ok(make_date(num(&y)?, num(&m)?, num(&d)?)? + num(&h)? * 3600.0 + num(&mi)? * 60.0 + num(&s)?)
        },
    );
    engine.register_fn("date_diff", |a: Dynamic, b: Dynamic, unit: &str| -> Fallible<FLOAT> {
        let (a, b) = (num(&a)?, num(&b)?);
        if !a.is_finite() || !b.is_finite() {
            return Ok(f64::NAN);
        }
        match unit.trim_end_matches('s') {
            "month" => Ok(months_between(a, b)),
            "year" => Ok((months_between(a, b) / 12.0).trunc()),
            _ => unit_seconds(unit).map_or_else(|| fail(unit_error(unit)), |s| Ok((b - a) / s)),
        }
    });
    engine.register_fn("date_add", |d: Dynamic, n: Dynamic, unit: &str| -> Fallible<FLOAT> {
        let (d, n) = (num(&d)?, num(&n)?);
        if !d.is_finite() || !n.is_finite() {
            return Ok(f64::NAN);
        }
        match unit.trim_end_matches('s') {
            "month" => Ok(add_months(d, whole(n, "the number of months")?)),
            "year" => Ok(add_months(d, 12 * whole(n, "the number of years")?)),
            _ => unit_seconds(unit).map_or_else(|| fail(unit_error(unit)), |s| Ok(d + n * s)),
        }
    });
    engine.register_fn("format_date", |d: Dynamic, pattern: &str| -> Fallible<String> {
        Ok(format_date(num(&d)?, pattern))
    });

    // Rows.
    let c = Rc::clone(ctx);
    engine.register_fn("row", move || {
        c.other_rows.set(true);
        (c.row.get() + 1) as INT
    });
    let c = Rc::clone(ctx);
    engine.register_fn("n_rows", move || {
        c.other_rows.set(true);
        c.n_rows as INT
    });
    let c = Rc::clone(ctx);
    engine.register_fn("col", move |name: &str| c.value_at(name, Some(c.row.get())));
    let c = Rc::clone(ctx);
    engine.register_fn("col", move |name: &str, row: Dynamic| -> Fallible<Dynamic> {
        let r = num(&row)?;
        let r = (r.is_finite() && r >= 1.0 && r.fract() == 0.0).then(|| r as usize - 1);
        c.value_at(name, r)
    });
    for (name, sign) in [("lag", -1i64), ("lead", 1)] {
        let c = Rc::clone(ctx);
        engine.register_fn(name, move |col: &str, k: INT| {
            let target = c.row.get() as i64 + sign * k;
            c.value_at(col, usize::try_from(target).ok())
        });
        let c = Rc::clone(ctx);
        engine.register_fn(name, move |col: &str| {
            let target = c.row.get() as i64 + sign;
            c.value_at(col, usize::try_from(target).ok())
        });
    }

    // Column summaries, cumulative sums and ranks (optionally by group).
    for kind in ["col_mean", "col_sd", "col_min", "col_max", "col_sum", "col_count", "cum_sum", "rank"] {
        let c = Rc::clone(ctx);
        engine.register_fn(kind, move |name: &str| c.summary(kind, name, None, 0.0));
        let c = Rc::clone(ctx);
        engine.register_fn(kind, move |name: &str, by: &str| c.summary(kind, name, Some(by), 0.0));
    }
    // Ranks with a tie rule: "average", "min", "max", "dense" or "first".
    let tie_kind = |rule: &str| -> Fallible<String> {
        match TieRule::from_key(rule) {
            Some(r) => Ok(format!("rank:{}", r.key())),
            None => {
                fail(format!("unknown tie rule {rule:?}: use \"average\", \"min\", \"max\", \"dense\" or \"first\""))
            }
        }
    };
    let c = Rc::clone(ctx);
    engine.register_fn("rank_ties", move |name: &str, rule: &str| c.summary(&tie_kind(rule)?, name, None, 0.0));
    let c = Rc::clone(ctx);
    engine.register_fn("rank_ties", move |name: &str, rule: &str, by: &str| {
        c.summary(&tie_kind(rule)?, name, Some(by), 0.0)
    });
    // p-values adjusted for multiple testing over the whole column (or
    // within groups); see `stat_inference::pvalues`.
    let c = Rc::clone(ctx);
    engine.register_fn("p_adjust", move |name: &str, method: &str| c.summary(&adjust_kind(method)?, name, None, 0.0));
    let c = Rc::clone(ctx);
    engine.register_fn("p_adjust", move |name: &str, method: &str, by: &str| {
        c.summary(&adjust_kind(method)?, name, Some(by), 0.0)
    });
    // Curve transforms (see `area_analysis`), optionally within groups.
    for kind in ["percent_of_range", "fraction_of_total"] {
        let c = Rc::clone(ctx);
        engine.register_fn(kind, move |name: &str| c.summary(kind, name, None, 0.0));
        let c = Rc::clone(ctx);
        engine.register_fn(kind, move |name: &str, by: &str| c.summary(kind, name, Some(by), 0.0));
    }
    let baseline_kind = |k: rhai::INT| -> Fallible<String> {
        if k < 1 {
            return fail("subtract_baseline needs at least one point for the baseline");
        }
        Ok(format!("subtract_baseline:{k}"))
    };
    let c = Rc::clone(ctx);
    engine.register_fn("subtract_baseline", move |name: &str, k: rhai::INT| {
        c.summary(&baseline_kind(k)?, name, None, 0.0)
    });
    let c = Rc::clone(ctx);
    engine.register_fn("subtract_baseline", move |name: &str, k: rhai::INT, by: &str| {
        c.summary(&baseline_kind(k)?, name, Some(by), 0.0)
    });
    // Signal smoothing, derivatives, integrals and interpolation
    // (`stat_signal`), optionally within groups.
    let window = |name: &str, w: INT| -> Fallible<usize> {
        usize::try_from(w).ok().filter(|&w| w >= 1).map_or_else(|| fail(format!("{name}: the window must be a positive whole number")), Ok)
    };
    let savgol_kind = move |w: INT, o: INT| -> Fallible<String> { Ok(format!("signal:savgol:{}:{}", window("savgol", w)?, o.max(0))) };
    let c = Rc::clone(ctx);
    engine.register_fn("savgol", move |col: &str, w: INT, o: INT| c.summary(&savgol_kind(w, o)?, col, None, 0.0));
    let c = Rc::clone(ctx);
    engine.register_fn("savgol", move |col: &str, w: INT, o: INT, g: &str| c.summary(&savgol_kind(w, o)?, col, Some(g), 0.0));
    for name in ["moving_average", "running_median"] {
        let c = Rc::clone(ctx);
        engine.register_fn(name, move |col: &str, w: INT| c.summary(&format!("signal:{name}:{}", window(name, w)?), col, None, 0.0));
        let c = Rc::clone(ctx);
        engine.register_fn(name, move |col: &str, w: INT, g: &str| {
            c.summary(&format!("signal:{name}:{}", window(name, w)?), col, Some(g), 0.0)
        });
    }
    for name in ["exp_smooth", "gaussian_smooth"] {
        let c = Rc::clone(ctx);
        engine.register_fn(name, move |col: &str, a: Dynamic| c.summary(&format!("signal:{name}:{}", num(&a)?), col, None, 0.0));
        let c = Rc::clone(ctx);
        engine.register_fn(name, move |col: &str, a: Dynamic, g: &str| {
            c.summary(&format!("signal:{name}:{}", num(&a)?), col, Some(g), 0.0)
        });
    }
    for name in ["derivative", "cum_integral"] {
        let c = Rc::clone(ctx);
        engine.register_fn(name, move |y: &str, x: &str| c.summary(&format!("xy:{name}\u{3}{x}"), y, None, 0.0));
        let c = Rc::clone(ctx);
        engine.register_fn(name, move |y: &str, x: &str, g: &str| c.summary(&format!("xy:{name}\u{3}{x}"), y, Some(g), 0.0));
    }
    for (name, method) in [
        ("interpolate", Interpolation::Linear),
        ("interpolate_spline", Interpolation::CubicSpline),
        ("interpolate_pchip", Interpolation::Pchip),
    ] {
        let c = Rc::clone(ctx);
        engine.register_fn(name, move |y: &str, x: &str, at: Dynamic| c.interpolate(method, y, x, num(&at)?));
    }
    // Signal processing down the column in row order (see `signal_analysis`):
    // zero-phase Butterworth filters with frequencies in cycles per row, the
    // Hilbert envelope and phase, and wavelet denoising; optionally by group.
    let edge = |f: &Dynamic| -> Fallible<f64> {
        let f = num(f)?;
        if f > 0.0 && f < 0.5 { Ok(f) } else { fail("frequencies are in cycles per row, between 0 and 0.5") }
    };
    for kind in ["lowpass", "highpass"] {
        let c = Rc::clone(ctx);
        let name_fn = format!("{kind}_filter");
        engine.register_fn(&name_fn, move |name: &str, f: Dynamic| {
            c.summary(&format!("sigproc:{kind}:{}", edge(&f)?), name, None, 0.0)
        });
        let c = Rc::clone(ctx);
        engine.register_fn(&name_fn, move |name: &str, f: Dynamic, by: &str| {
            c.summary(&format!("sigproc:{kind}:{}", edge(&f)?), name, Some(by), 0.0)
        });
    }
    for kind in ["bandpass", "bandstop"] {
        let band = move |a: &Dynamic, b: &Dynamic| -> Fallible<String> {
            let (a, b) = (edge(a)?, edge(b)?);
            if a < b { Ok(format!("sigproc:{kind}:{a}:{b}")) } else { fail("the first frequency must be the lower") }
        };
        let c = Rc::clone(ctx);
        let name_fn = format!("{kind}_filter");
        engine.register_fn(&name_fn, move |name: &str, a: Dynamic, b: Dynamic| {
            c.summary(&band(&a, &b)?, name, None, 0.0)
        });
        let c = Rc::clone(ctx);
        engine.register_fn(&name_fn, move |name: &str, a: Dynamic, b: Dynamic, by: &str| {
            c.summary(&band(&a, &b)?, name, Some(by), 0.0)
        });
    }
    for kind in ["envelope", "instant_phase"] {
        let c = Rc::clone(ctx);
        engine.register_fn(kind, move |name: &str| c.summary(&format!("sigproc:{kind}"), name, None, 0.0));
        let c = Rc::clone(ctx);
        engine.register_fn(kind, move |name: &str, by: &str| c.summary(&format!("sigproc:{kind}"), name, Some(by), 0.0));
    }
    let c = Rc::clone(ctx);
    engine.register_fn("wavelet_denoise", move |name: &str| c.summary("sigproc:wavelet_denoise:db4", name, None, 0.0));
    let c = Rc::clone(ctx);
    engine.register_fn("wavelet_denoise", move |name: &str, w: &str| {
        c.summary(&format!("sigproc:wavelet_denoise:{w}"), name, None, 0.0)
    });
    let c = Rc::clone(ctx);
    engine.register_fn("wavelet_denoise", move |name: &str, w: &str, by: &str| {
        c.summary(&format!("sigproc:wavelet_denoise:{w}"), name, Some(by), 0.0)
    });
    let c = Rc::clone(ctx);
    engine.register_fn("col_quantile", move |name: &str, p: Dynamic| -> Fallible<FLOAT> {
        let p = num(&p)?;
        if !(0.0..=1.0).contains(&p) {
            return fail("the probability must be between 0 and 1");
        }
        c.summary("col_quantile", name, None, p)
    });
    let c = Rc::clone(ctx);
    engine.register_fn("col_quantile", move |name: &str, p: Dynamic, by: &str| -> Fallible<FLOAT> {
        let p = num(&p)?;
        if !(0.0..=1.0).contains(&p) {
            return fail("the probability must be between 0 and 1");
        }
        c.summary("col_quantile", name, Some(by), p)
    });

    // Random numbers: reproducible, from a seed and the row.
    let c = Rc::clone(ctx);
    engine.register_fn("random_seed", move |seed: INT| {
        c.other_rows.set(true);
        c.seed.set(seed as u64);
        *c.rng.borrow_mut() = None;
    });
    let c = Rc::clone(ctx);
    engine.register_fn("random_uniform", move || c.random());
    let c = Rc::clone(ctx);
    engine.register_fn("random_uniform", move |lo: Dynamic, hi: Dynamic| -> Fallible<FLOAT> {
        let (lo, hi) = (num(&lo)?, num(&hi)?);
        Ok(lo + (hi - lo) * c.random())
    });
    let normal_draw = |c: &Context| {
        // Box–Muller.
        let u = 1.0 - c.random();
        let v = c.random();
        (-2.0 * u.ln()).sqrt() * (std::f64::consts::TAU * v).cos()
    };
    let c = Rc::clone(ctx);
    engine.register_fn("random_normal", move || normal_draw(&c));
    let c = Rc::clone(ctx);
    engine.register_fn("random_normal", move |m: Dynamic, s: Dynamic| -> Fallible<FLOAT> {
        Ok(num(&m)? + num(&s)? * normal_draw(&c))
    });
    let c = Rc::clone(ctx);
    engine.register_fn("random_integer", move |lo: INT, hi: INT| -> Fallible<INT> {
        if hi < lo {
            return fail("the upper limit is below the lower one");
        }
        let span = (hi - lo) as f64 + 1.0;
        Ok(lo + (c.random() * span).floor() as INT)
    });
    engine
}

fn unit_error(unit: &str) -> String {
    format!("unknown unit {unit:?} (use seconds, minutes, hours, days, weeks, months or years)")
}

/// The formula's identifier columns as a scope (values unset).
fn scope_for<'a>(table: &DataTable, names: impl Iterator<Item = &'a str>) -> Scope<'static> {
    let mut scope = Scope::new();
    for name in names {
        if let Some(j) = table.find(name) {
            let missing = match table.columns()[j].data {
                ColumnData::Text(_) => Dynamic::from(ImmutableString::new()),
                _ => Dynamic::from_float(f64::NAN),
            };
            scope.push_dynamic(name.to_owned(), missing);
        }
    }
    scope
}

/// A formula ready to evaluate on one table.
struct Compiled {
    engine: Engine,
    ctx: Rc<Context>,
    ast: rhai::AST,
    /// `(name, column index)` of each identifier column used.
    variables: Vec<(String, usize)>,
}

/// Compile `formula` for `table`; `target` is the column the formula is for
/// (which it may not use).
fn compile(table: &DataTable, formula: &str, target: Option<usize>) -> std::result::Result<Compiled, FormulaError> {
    compile_with(table, formula, target, &[])
}

/// [`compile`] with extra variables `extra` (model parameters) in scope.
fn compile_with(
    table: &DataTable,
    formula: &str,
    target: Option<usize>,
    extra: &[String],
) -> std::result::Result<Compiled, FormulaError> {
    if formula.trim().is_empty() {
        return Err(plain_error("the formula is empty"));
    }
    let used = references(table, formula);
    if let Some(t) = target.filter(|t| used.contains(t)) {
        return Err(plain_error(format!("a formula cannot use its own column ({:?})", table.columns()[t].name)));
    }
    let ctx = Rc::new(Context {
        n_rows: table.n_rows(),
        columns: used.iter().map(|&j| (table.columns()[j].name.clone(), table.columns()[j].clone())).collect(),
        row: Cell::new(0),
        summaries: RefCell::default(),
        regexes: RefCell::default(),
        interpolators: RefCell::default(),
        seed: Cell::new(0),
        rng: RefCell::new(None),
        other_rows: Cell::new(false),
    });
    let engine = engine(&ctx);
    // Check against every identifier column, so a misspelt name is reported
    // as an unknown variable at its place in the text.
    let mut all = scope_for(table, table.columns().iter().map(|c| c.name.as_str()).filter(|n| is_identifier(n)));
    for name in extra {
        all.push_dynamic(name.clone(), Dynamic::from_float(1.0));
    }
    engine
        .compile_with_scope(&all, formula)
        .map_err(|e| FormulaError { message: e.err_type().to_string(), position: position_of(e.position()) })?;
    let idents: Vec<&str> =
        used.iter().map(|&j| table.columns()[j].name.as_str()).filter(|n| is_identifier(n)).collect();
    let mut scope = scope_for(table, idents.iter().copied());
    for name in extra {
        scope.push_dynamic(name.clone(), Dynamic::from_float(1.0));
    }
    let ast = engine
        .compile_with_scope(&scope, formula)
        .map_err(|e| FormulaError { message: e.err_type().to_string(), position: position_of(e.position()) })?;
    let variables = idents.iter().filter_map(|n| table.find(n).map(|j| ((*n).to_owned(), j))).collect();
    Ok(Compiled { engine, ctx, ast, variables })
}

/// A runtime error as one line, without Rhai's internal wrapping.
fn runtime_message(e: &EvalAltResult) -> String {
    let message = match e {
        EvalAltResult::ErrorInFunctionCall(_, _, inner, _) => return runtime_message(inner),
        EvalAltResult::ErrorRuntime(value, _) => value.to_string(),
        EvalAltResult::ErrorTooManyOperations(_) => "the formula took too long on this row".to_owned(),
        other => other.to_string(),
    };
    match position_of(e.position()) {
        Some((line, col)) if !message.contains("(line") => format!("{message} (line {line}, column {col})"),
        _ => message,
    }
}

fn to_value(x: &Dynamic) -> std::result::Result<FormulaValue, String> {
    if x.is_unit() {
        Ok(FormulaValue::Missing)
    } else if let Ok(f) = x.as_float() {
        Ok(if f.is_finite() { FormulaValue::Number(f) } else { FormulaValue::Missing })
    } else if let Ok(i) = x.as_int() {
        Ok(FormulaValue::Number(i as f64))
    } else if let Ok(b) = x.as_bool() {
        Ok(FormulaValue::Number(if b { 1.0 } else { 0.0 }))
    } else if x.is_string() || x.is_char() {
        Ok(FormulaValue::Text(x.to_string()))
    } else {
        Err(format!("the formula gave {} instead of a number or text", x.type_name()))
    }
}

impl Compiled {
    fn run(&mut self, table: &DataTable, rows: impl ExactSizeIterator<Item = usize>) -> Evaluation {
        let mut scope = Scope::new();
        let mut values = Vec::with_capacity(rows.len());
        let mut failures = Vec::new();
        for r in rows {
            self.ctx.row.set(r);
            self.ctx.seed.set(0);
            *self.ctx.rng.borrow_mut() = None;
            scope.rewind(0);
            for (name, j) in &self.variables {
                scope.push_dynamic(name.as_str(), cell(&table.columns()[*j], r));
            }
            match self.engine.eval_ast_with_scope::<Dynamic>(&mut scope, &self.ast) {
                Ok(x) => match to_value(&x) {
                    Ok(v) => values.push(v),
                    Err(message) => {
                        failures.push((r, message));
                        values.push(FormulaValue::Missing);
                    }
                },
                Err(e) => {
                    failures.push((r, runtime_message(&e)));
                    values.push(FormulaValue::Missing);
                }
            }
        }
        Evaluation { values, failures }
    }
}

/// Check `formula` for column `target` of `table` (or a new column, `None`):
/// it compiles, names only existing columns, and does not make the formulas
/// form a cycle.
///
/// # Errors
/// A [`FormulaError`] describing the first problem.
pub fn check(table: &DataTable, target: Option<usize>, formula: &str) -> std::result::Result<(), FormulaError> {
    compile(table, formula, target)?;
    if let Some(t) = target {
        let mut trial = table.clone();
        trial.set_formula(t, Some(formula.to_owned())).map_err(|e| plain_error(e.to_string()))?;
        // Only a cycle through this column is its formula's fault (a table
        // read from a file may hold an unrelated one).
        if let Some(cycle) = evaluation_plan(&trial).1.into_iter().find(|c| c.contains(&t)) {
            let names: Vec<&str> = cycle.iter().map(|&c| trial.columns()[c].name.as_str()).collect();
            return Err(plain_error(format!(
                "the formulas would depend on each other in a circle: {}",
                names.join(" → ")
            )));
        }
    }
    Ok(())
}

/// Evaluate `formula` on `rows` of `table` (for previews), as for column
/// `target` (`None` for a new column).
///
/// # Errors
/// A [`FormulaError`] if the formula does not compile.
pub fn evaluate(
    table: &DataTable,
    target: Option<usize>,
    formula: &str,
    rows: std::ops::Range<usize>,
) -> std::result::Result<Evaluation, FormulaError> {
    let rows = rows.start.min(table.n_rows())..rows.end.min(table.n_rows());
    Ok(compile(table, formula, target)?.run(table, rows))
}

/// Store `eval` in a column like `column`: numeric results keep a numeric
/// or date-time column's type (and become text in a text column); any text
/// result makes the column text.
#[must_use]
pub fn fill(column: &Column, eval: &Evaluation) -> Column {
    let text = column.kind() == ColumnKind::Text || eval.natural_kind() == ColumnKind::Text;
    let data = if text {
        ColumnData::Text(eval.values.iter().map(FormulaValue::text).collect())
    } else {
        let numbers = eval
            .values
            .iter()
            .map(|v| match v {
                FormulaValue::Number(x) => *x,
                _ => f64::NAN,
            })
            .collect();
        match column.temporal() {
            Some(display) => ColumnData::DateTime { values: numbers, display },
            None => ColumnData::Numeric(numbers),
        }
    };
    let level =
        if text && column.level == MeasurementLevel::Continuous { MeasurementLevel::Nominal } else { column.level };
    Column {
        name: column.name.clone(),
        data,
        level,
        formula: column.formula.clone(),
        value_labels: column.value_labels.clone(),
        hover: column.hover,
    }
}

/// What an edit changed, for deciding which formulas to recompute.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Changed {
    /// Nothing that formulas read.
    #[default]
    Nothing,
    /// The values (or formulas) of these columns, by name.
    Columns(Vec<String>),
    /// The rows themselves, in a way not described more closely: every
    /// formula, in full.
    Rows,
    /// Values of some rows only, and rows added, removed or put in a new
    /// order (cell edits, pastes, new rows, deleted rows, sorting). Row-wise
    /// formulas (see [`is_row_wise`]) recompute only the rows concerned;
    /// formulas that read other rows recompute in full.
    Partial(RowChanges),
}

/// The rows an edit touched, as positions in the table after it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RowChanges {
    /// Columns (by name) whose values changed at [`rows`](Self::rows).
    pub columns: Vec<String>,
    /// Where those columns changed, ascending.
    pub rows: Vec<usize>,
    /// Rows added, ascending: every column is new there.
    pub new_rows: Vec<usize>,
    /// Rows were removed or put in a new order, so every formula that reads
    /// other rows (or the row's position) recomputes.
    pub moved: bool,
}

impl RowChanges {
    /// New values of column `name` at `row`.
    #[must_use]
    pub fn cell(name: String, row: usize) -> Self {
        Self { columns: vec![name], rows: vec![row], ..Self::default() }
    }

    /// Rows added at `positions` (ascending, positions after the edit).
    #[must_use]
    pub fn inserted(positions: Vec<usize>) -> Self {
        Self { new_rows: positions, ..Self::default() }
    }

    /// Rows removed or reordered.
    #[must_use]
    pub fn moved() -> Self {
        Self { moved: true, ..Self::default() }
    }

    /// These changes followed by `later`, as positions after both; `None`
    /// when they cannot be told apart row by row (rows of the first moved by
    /// the second).
    fn then(mut self, later: Self) -> Option<Self> {
        if later.moved && !(self.rows.is_empty() && self.new_rows.is_empty()) {
            return None;
        }
        // Rows the later edit inserted push the earlier positions down.
        let shift = |r: usize| later.new_rows.iter().fold(r, |at, &p| if p <= at { at + 1 } else { at });
        for r in self.rows.iter_mut().chain(&mut self.new_rows) {
            *r = shift(*r);
        }
        self.columns.extend(later.columns);
        self.columns.sort();
        self.columns.dedup();
        self.rows.extend(later.rows);
        self.rows.sort_unstable();
        self.rows.dedup();
        self.new_rows.extend(later.new_rows);
        self.new_rows.sort_unstable();
        self.new_rows.dedup();
        self.moved |= later.moved;
        Some(self)
    }
}

impl Changed {
    /// Both changes, `other` made after `self`.
    #[must_use]
    pub fn and(self, other: Self) -> Self {
        match (self, other) {
            (Self::Rows, _) | (_, Self::Rows) => Self::Rows,
            (Self::Nothing, x) | (x, Self::Nothing) => x,
            (Self::Columns(mut a), Self::Columns(b)) => {
                a.extend(b);
                Self::Columns(a)
            }
            (Self::Partial(a), Self::Partial(b)) => a.then(b).map_or(Self::Rows, Self::Partial),
            // Whole columns and some rows: the columns in full.
            (Self::Columns(mut a), Self::Partial(p)) | (Self::Partial(p), Self::Columns(mut a)) => {
                if p.moved || !p.new_rows.is_empty() {
                    return Self::Rows;
                }
                a.extend(p.columns);
                Self::Columns(a)
            }
        }
    }
}

/// Functions besides the [`COLUMN_FUNCTIONS`] (other than `col` with one
/// argument, which reads the current row) whose value depends on more than
/// the current row's values: its position, the number of rows, or random
/// draws seeded by the row. Also Rhai's ways of calling a function by name.
const WHOLE_TABLE_FUNCTIONS: &[&str] = &[
    "row",
    "n_rows",
    "random_seed",
    "random_uniform",
    "random_normal",
    "random_integer",
    "Fn",
    "call",
    "curry",
];

/// Whether `formula` is row-wise: each row's value depends only on that
/// row's values, so a change to some rows (or rows added, removed or
/// reordered) needs only those rows recomputed. Decided from the text: the
/// formula names none of the functions that read other rows
/// ([`COLUMN_FUNCTIONS`] and the like), except `col("name")` with a single
/// quoted name. A column whose name is one of those functions counts
/// against it too, which only costs a full recomputation.
#[must_use]
pub fn is_row_wise(formula: &str) -> bool {
    let tokens = scan(formula);
    let is = |i: usize, c: char| matches!(tokens.get(i), Some((Token::Other(o), _)) if *o == c);
    let reads_other_rows = |name: &str| COLUMN_FUNCTIONS.contains(&name) || WHOLE_TABLE_FUNCTIONS.contains(&name);
    tokens.iter().enumerate().all(|(i, (token, _))| match token {
        Token::Ident(name) if name == "col" => {
            i.checked_sub(1).is_none_or(|p| !is(p, '.'))
                && is(i + 1, '(')
                && matches!(tokens.get(i + 2), Some((Token::Str(_), _)))
                && is(i + 3, ')')
        }
        Token::Ident(name) | Token::Str(name) => !reads_other_rows(name),
        Token::Other(_) => true,
    })
}

/// Recompute the formula columns affected by `changed`, in dependency order.
/// Formulas that no longer compile, or that sit on a cycle, keep their
/// values (the other formula columns still recompute); rows that fail become
/// missing. See [`refresh_noting`] for a list of the columns left as they
/// were.
pub fn refresh(table: &mut DataTable, changed: &Changed) {
    let _ = refresh_noting(table, changed);
}

/// [`refresh`], returning one note per formula column that could not be
/// computed (a formula on a cycle, or one that no longer compiles), naming
/// the column and why; those columns keep their values. Empty when every
/// affected formula column recomputed.
pub fn refresh_noting(table: &mut DataTable, changed: &Changed) -> Vec<String> {
    if matches!(changed, Changed::Nothing) || !table.columns().iter().any(|c| c.formula.is_some()) {
        return Vec::new();
    }
    let (order, cycles) = evaluation_plan(table);
    let mut notes: Vec<String> = cycles
        .iter()
        .flat_map(|cycle| {
            let names: Vec<&str> = cycle.iter().map(|&c| table.columns()[c].name.as_str()).collect();
            let path = names.join(" → ");
            names
                .iter()
                .map(|name| {
                    format!(
                        "Formula column {name} could not be computed: the formulas depend on each other in a \
                         circle ({path}); it keeps its saved values."
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect();
    // Columns changed in full, and columns changed at `partial.rows` only.
    let (mut dirty, mut partly): (HashSet<String>, HashSet<String>) = match changed {
        Changed::Columns(names) => (names.iter().cloned().collect(), HashSet::new()),
        Changed::Partial(p) => (HashSet::new(), p.columns.iter().cloned().collect()),
        _ => (HashSet::new(), HashSet::new()),
    };
    for j in order {
        let column = &table.columns()[j];
        let Some(formula) = column.formula.clone() else { continue };
        let used: Vec<String> = references(table, &formula).iter().map(|&u| table.columns()[u].name.clone()).collect();
        // The rows to recompute: `None` for all of them.
        let rows: Option<Vec<usize>> = match changed {
            Changed::Partial(p) => {
                let reads_dirty = dirty.contains(&column.name) || used.iter().any(|u| dirty.contains(u));
                let reads_partly = used.iter().any(|u| partly.contains(u));
                if reads_dirty || (!is_row_wise(&formula) && (reads_partly || p.moved || !p.new_rows.is_empty())) {
                    None
                } else if reads_partly {
                    let mut rows: Vec<usize> = p.rows.iter().chain(&p.new_rows).copied().collect();
                    rows.sort_unstable();
                    rows.dedup();
                    Some(rows)
                } else if p.new_rows.is_empty() {
                    continue;
                } else {
                    Some(p.new_rows.clone())
                }
            }
            Changed::Rows => None,
            _ if dirty.contains(&column.name) || used.iter().any(|u| dirty.contains(u)) => None,
            _ => continue,
        };
        let name = column.name.clone();
        let mut compiled = match compile(table, &formula, Some(j)) {
            Ok(compiled) => compiled,
            Err(e) => {
                notes.push(format!(
                    "Formula column {name} could not be computed: {}; it keeps its saved values.",
                    e.message
                ));
                match rows {
                    None => dirty.insert(name),
                    Some(_) => partly.insert(name),
                };
                continue;
            }
        };
        if let Some(rows) = &rows
            && recompute_rows(&mut compiled, table, j, rows)
        {
            if !rows.is_empty() {
                partly.insert(name);
            }
            continue;
        }
        let column = &table.columns()[j];
        let eval = compiled.run(table, 0..table.n_rows());
        let new = fill(column, &eval);
        if new != *column {
            let _ = table.replace_column(j, new);
        }
        dirty.insert(name);
    }
    notes
}

/// Recompute formula column `j` at `rows` alone with `compiled`, its
/// formula. Returns `false`, changing nothing, when that is not enough: the
/// evaluation read another row after all, or a result would change the
/// column's type (a text value in a numeric column).
fn recompute_rows(compiled: &mut Compiled, table: &mut DataTable, j: usize, rows: &[usize]) -> bool {
    compiled.ctx.other_rows.set(false);
    let eval = compiled.run(table, rows.iter().copied());
    if compiled.ctx.other_rows.get() {
        return false;
    }
    let text = table.columns()[j].kind() == ColumnKind::Text;
    let values: Option<Vec<CellValue>> = eval
        .values
        .iter()
        .map(|v| match (v, text) {
            (_, true) => Some(CellValue::Text(v.text())),
            (FormulaValue::Number(x), false) => Some(CellValue::Number(*x)),
            (FormulaValue::Missing, false) => Some(CellValue::Number(f64::NAN)),
            (FormulaValue::Text(_), false) => None,
        })
        .collect();
    let Some(values) = values else { return false };
    for (&r, value) in rows.iter().zip(values) {
        let same = match (table.columns()[j].value(r), &value) {
            (CellValue::Number(a), CellValue::Number(b)) => a == *b || (a.is_nan() && b.is_nan()),
            (old, new) => old == *new,
        };
        if !same {
            let _ = table.set_value(r, j, value);
        }
    }
    true
}

/// Recompute every formula column.
pub fn refresh_all(table: &mut DataTable) {
    let _ = refresh_all_noting(table);
}

/// [`refresh_all`], returning the notes of [`refresh_noting`].
pub fn refresh_all_noting(table: &mut DataTable) -> Vec<String> {
    let names = table.columns().iter().map(|c| c.name.clone()).collect();
    refresh_noting(table, &Changed::Columns(names))
}

/// The formula columns that use column `index`, by name.
#[must_use]
pub fn users(table: &DataTable, index: usize) -> Vec<String> {
    table
        .columns()
        .iter()
        .enumerate()
        .filter(|(j, c)| *j != index && c.formula.as_deref().is_some_and(|f| references(table, f).contains(&index)))
        .map(|(_, c)| c.name.clone())
        .collect()
}

// ---------------------------------------------------------------------------
// Model formulas with named parameters (nonlinear regression)
// ---------------------------------------------------------------------------

/// A model formula with named parameters, such as `Vmax * Conc / (Km + Conc)`
/// with parameters `Vmax` and `Km`: compiled once for a table and evaluated
/// on chosen rows for any parameter values (see
/// [`crate::nonlinear_analysis`]). Parameters are variables like identifier
/// columns; everything else is as for formula columns.
pub struct ParameterFormula {
    compiled: Compiled,
    parameters: Vec<String>,
}

impl fmt::Debug for ParameterFormula {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ParameterFormula").field("parameters", &self.parameters).finish_non_exhaustive()
    }
}

/// Compile model formula `formula` with parameters `parameters` for `table`.
///
/// # Errors
/// A [`FormulaError`] if a parameter name is not an identifier, repeats, or is
/// also a column name, or if the formula does not compile.
pub fn compile_with_parameters(
    table: &DataTable,
    formula: &str,
    parameters: &[String],
) -> std::result::Result<ParameterFormula, FormulaError> {
    for (i, name) in parameters.iter().enumerate() {
        if !is_identifier(name) {
            return Err(plain_error(format!(
                "parameter name {name:?} must be letters, digits and _ (not starting with a digit)"
            )));
        }
        if parameters[..i].contains(name) {
            return Err(plain_error(format!("parameter {name:?} is named twice")));
        }
        if table.find(name).is_some() {
            return Err(plain_error(format!("parameter {name:?} has the same name as a column")));
        }
    }
    Ok(ParameterFormula { compiled: compile_with(table, formula, None, parameters)?, parameters: parameters.to_vec() })
}

impl ParameterFormula {
    /// The table columns the formula uses (ascending indices).
    #[must_use]
    pub fn columns(&self, table: &DataTable) -> Vec<usize> {
        let mut used: Vec<usize> = self.compiled.ctx.columns.keys().filter_map(|n| table.find(n)).collect();
        used.sort_unstable();
        used
    }

    /// The formula's value on each of `rows` of `table` (the table it was
    /// compiled for) with parameter values `theta`, into `out`.
    ///
    /// # Errors
    /// The first row whose value is not a finite number, with the reason.
    pub fn evaluate(
        &mut self,
        table: &DataTable,
        rows: &[usize],
        theta: &[f64],
        out: &mut [f64],
    ) -> std::result::Result<(), (usize, String)> {
        let mut scope = Scope::new();
        for (&r, o) in rows.iter().zip(out.iter_mut()) {
            self.compiled.ctx.row.set(r);
            self.compiled.ctx.seed.set(0);
            *self.compiled.ctx.rng.borrow_mut() = None;
            scope.rewind(0);
            for (name, j) in &self.compiled.variables {
                scope.push_dynamic(name.as_str(), cell(&table.columns()[*j], r));
            }
            for (name, &v) in self.parameters.iter().zip(theta) {
                scope.push_dynamic(name.as_str(), Dynamic::from_float(v));
            }
            let value = self
                .compiled
                .engine
                .eval_ast_with_scope::<Dynamic>(&mut scope, &self.compiled.ast)
                .map_err(|e| (r, runtime_message(&e)))?;
            *o = match to_value(&value).map_err(|m| (r, m))? {
                FormulaValue::Number(v) => v,
                FormulaValue::Missing => return Err((r, "the formula gave a missing value".to_owned())),
                FormulaValue::Text(_) => return Err((r, "the formula gave text instead of a number".to_owned())),
            };
        }
        Ok(())
    }

    /// The formula's value at row `row` of `table` with parameter values
    /// `theta`, except that column `j` reads `value_of(j)` wherever that is
    /// `Some` (used to evaluate a model at points that are not table rows,
    /// such as the steps of a differential equation or the points near a
    /// curve).
    ///
    /// # Errors
    /// Why the value is not a finite number.
    pub fn evaluate_with(
        &mut self,
        table: &DataTable,
        row: usize,
        value_of: &dyn Fn(usize) -> Option<f64>,
        theta: &[f64],
    ) -> std::result::Result<f64, String> {
        let mut scope = Scope::new();
        self.compiled.ctx.row.set(row);
        self.compiled.ctx.seed.set(0);
        *self.compiled.ctx.rng.borrow_mut() = None;
        for (name, j) in &self.compiled.variables {
            match value_of(*j) {
                Some(v) => scope.push_dynamic(name.as_str(), Dynamic::from_float(v)),
                None => scope.push_dynamic(name.as_str(), cell(&table.columns()[*j], row)),
            };
        }
        for (name, &v) in self.parameters.iter().zip(theta) {
            scope.push_dynamic(name.as_str(), Dynamic::from_float(v));
        }
        let value = self
            .compiled
            .engine
            .eval_ast_with_scope::<Dynamic>(&mut scope, &self.compiled.ast)
            .map_err(|e| runtime_message(&e))?;
        match to_value(&value)? {
            FormulaValue::Number(v) if v.is_finite() => Ok(v),
            FormulaValue::Number(_) => Err("the formula gave an infinite or undefined number".to_owned()),
            FormulaValue::Missing => Err("the formula gave a missing value".to_owned()),
            FormulaValue::Text(_) => Err("the formula gave text instead of a number".to_owned()),
        }
    }

    /// Whether every column the formula reads is read by name as a variable
    /// (so [`evaluate_with`](Self::evaluate_with) can replace its value).
    #[must_use]
    pub fn reads_columns_only_by_name(&self, table: &DataTable) -> bool {
        self.columns(table).iter().all(|j| self.compiled.variables.iter().any(|(_, k)| k == j))
    }
}

// ---------------------------------------------------------------------------
// The function library, for the formula editor's browser
// ---------------------------------------------------------------------------

/// One library function as the formula editor lists it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FunctionInfo {
    /// Group in the browser.
    pub category: &'static str,
    /// Text inserted at the cursor (arguments as placeholders).
    pub template: &'static str,
    /// What it does.
    pub help: &'static str,
}

const fn f(category: &'static str, template: &'static str, help: &'static str) -> FunctionInfo {
    FunctionInfo { category, template, help }
}

/// Every library function, by category.
pub const FUNCTIONS: &[FunctionInfo] = &[
    f("Conditions", "if x > 0 { \"positive\" } else { \"not positive\" }", "Choose a value by a condition."),
    f(
        "Conditions",
        "switch x { 1 => \"one\", 2 | 3 => \"two or three\", _ => \"other\" }",
        "Choose a value by matching cases.",
    ),
    f("Conditions", "x > 0 && y > 0", "Both conditions hold (|| for either, ! for not)."),
    f("Missing values", "is_missing(x)", "true when x is missing."),
    f("Missing values", "if_missing(x, 0)", "x, or the second value when x is missing."),
    f("Missing values", "missing()", "A missing value."),
    f("Mathematics", "sqrt(x)", "Square root."),
    f("Mathematics", "x ** 2", "Power (also pow(x, y))."),
    f("Mathematics", "exp(x)", "e to the power x."),
    f("Mathematics", "ln(x)", "Natural logarithm."),
    f("Mathematics", "log10(x)", "Base-10 logarithm (also log2)."),
    f("Mathematics", "abs(x)", "Absolute value."),
    f("Mathematics", "round(x, 2)", "Round to a number of decimals (round(x) to a whole number)."),
    f("Mathematics", "floor(x)", "Largest whole number not above x (ceiling for the smallest not below)."),
    f("Mathematics", "min(x, y)", "Smaller of two values (max for the larger)."),
    f("Mathematics", "modulo(x, y)", "Remainder after division, never negative."),
    f("Mathematics", "sign(x)", "−1, 0 or 1."),
    f("Mathematics", "logit(p)", "Log odds ln(p / (1 − p)); logistic(x) is its inverse."),
    f("Mathematics", "sin(x)", "Sine of an angle in radians (also cos, tan, asin, acos, atan, atan2)."),
    f("Distributions", "normal_cdf(x)", "Standard normal probability P(Z ≤ x); normal_cdf(x, mean, sd)."),
    f("Distributions", "normal_pdf(x)", "Standard normal density; normal_pdf(x, mean, sd)."),
    f("Distributions", "normal_quantile(p)", "Standard normal quantile; normal_quantile(p, mean, sd)."),
    f("Distributions", "t_cdf(x, df)", "Student's t probability P(T ≤ x)."),
    f("Distributions", "t_quantile(p, df)", "Student's t quantile."),
    f("Distributions", "chisq_cdf(x, df)", "Chi-square probability P(X ≤ x)."),
    f("Distributions", "chisq_quantile(p, df)", "Chi-square quantile."),
    f("Distributions", "f_cdf(x, df1, df2)", "F probability P(F ≤ x)."),
    f("Distributions", "f_quantile(p, df1, df2)", "F quantile."),
    f("Distributions", "binomial_pmf(k, n, p)", "Binomial probability of k successes."),
    f("Distributions", "binomial_cdf(k, n, p)", "Binomial probability of at most k successes."),
    f("Distributions", "poisson_pmf(k, mean)", "Poisson probability of k events."),
    f("Distributions", "poisson_cdf(k, mean)", "Poisson probability of at most k events."),
    f("Text", "concat(a, \" \", b)", "Join values as text (up to six)."),
    f("Text", "text(x)", "A number as text, written as in the data view."),
    f("Text", "number(s)", "Text read as a number (missing if it is not one)."),
    f("Text", "upper(s)", "Capital letters (lower for small letters)."),
    f("Text", "trim(s)", "Without spaces at either end."),
    f("Text", "length(s)", "Number of characters."),
    f("Text", "substr(s, 0, 3)", "Characters from a position (from 0), of a length."),
    f("Text", "find(s, \"x\")", "Position of the first match (from 0), or −1."),
    f("Text", "replace(s, \"old\", \"new\")", "Replace every match."),
    f("Text", "word(s, 0, \",\")", "The nth piece (from 0) between separators; word(s, n) splits at spaces."),
    f(
        "Text",
        "has_item(s, \"Email\", \";\")",
        "true if a check-all-that-apply list holds the item (separator ; when left out).",
    ),
    f("Text", "item_count(s, \";\")", "How many different items a check-all-that-apply list holds."),
    f("Text", "s.contains(\"x\")", "true if the text contains x (also starts_with, ends_with)."),
    f("Text", "regex_match(s, \"^[A-Z]\\\\d+$\")", "true if a regular expression matches."),
    f("Text", "regex_find(s, \"\\\\d+\")", "The first match of a regular expression."),
    f("Text", "regex_replace(s, \"\\\\s+\", \" \")", "Replace every match of a regular expression ($1 for groups)."),
    f("Dates", "year(d)", "Year of a date (also month, day, hour, minute, second)."),
    f("Dates", "weekday(d)", "Day of the week, 1 (Monday) to 7 (Sunday)."),
    f("Dates", "day_of_year(d)", "Day of the year, from 1."),
    f("Dates", "date_part(d)", "The date at midnight, without the time of day."),
    f("Dates", "make_date(2026, 1, 31)", "A date from year, month and day (make_datetime adds h, m, s)."),
    f("Dates", "date_diff(start, end, \"days\")", "Time from start to end in a unit (whole months or years)."),
    f("Dates", "date_add(d, 1, \"months\")", "A date moved by a number of units."),
    f("Dates", "format_date(d, \"%d %b %Y\")", "A date as text (%Y %m %d %H %M %S %j %B %b %A %a)."),
    f("Rows", "row()", "This row's number (from 1)."),
    f("Rows", "n_rows()", "Number of rows."),
    f("Rows", "lag(\"column\", 1)", "The value a number of rows earlier (missing before the first row)."),
    f("Rows", "lead(\"column\", 1)", "The value a number of rows later."),
    f("Rows", "col(\"column\")", "This row's value of any column, by name."),
    f("Rows", "col(\"column\", 1)", "The value in a given row (from 1)."),
    f(
        "Column summaries",
        "col_mean(\"column\")",
        "Mean of a column; col_mean(\"column\", \"group\") within each group.",
    ),
    f("Column summaries", "col_sd(\"column\")", "Standard deviation (n − 1), optionally by a group column."),
    f("Column summaries", "col_min(\"column\")", "Smallest value (col_max for the largest), optionally by group."),
    f("Column summaries", "col_sum(\"column\")", "Sum, optionally by group."),
    f("Column summaries", "col_count(\"column\")", "Number of non-missing values, optionally by group."),
    f(
        "Column summaries",
        "col_quantile(\"column\", 0.5)",
        "Quantile (Hyndman–Fan type 8), optionally by group as a third argument.",
    ),
    f("Column summaries", "cum_sum(\"column\")", "Running total down to this row, optionally by group."),
    f("Column summaries", "rank(\"column\")", "Rank from 1 (ties averaged), optionally by group."),
    f(
        "Column summaries",
        "rank_ties(\"column\", \"min\")",
        "Rank from 1 with a tie rule: \"average\", \"min\" (1, 2, 2, 4), \"max\" (1, 3, 3, 4), \"dense\" \
         (1, 2, 2, 3) or \"first\" (row order); optionally by group as a third argument.",
    ),
    f(
        "Column summaries",
        "p_adjust(\"column\", \"bh\")",
        "A column of p-values adjusted for multiple testing: \"bonferroni\", \"sidak\", \"holm\", \"hochberg\", \
         \"bh\", \"by\" or \"q\" (Storey q-values, λ = 0.5); optionally by group.",
    ),
    f(
        "Column summaries",
        "percent_of_range(\"column\")",
        "The value as a percentage of the column's range: 0 at the smallest, 100 at the largest; optionally by group.",
    ),
    f(
        "Column summaries",
        "fraction_of_total(\"column\")",
        "The value divided by the column's total; optionally by group.",
    ),
    f(
        "Column summaries",
        "subtract_baseline(\"column\", 3)",
        "The value minus the mean of the column's first 3 values (in row order); optionally by group as a third \
         argument.",
    ),
    f(
        "Signals",
        "savgol(\"column\", 11, 3)",
        "Savitzky–Golay smoothing: a cubic (order 3) fitted over 11 points around each row, in row order; \
         optionally by group as a last argument (each group smoothed on its own; missing values skipped).",
    ),
    f("Signals", "moving_average(\"column\", 5)", "Centred moving average of 5 points; optionally by group."),
    f("Signals", "running_median(\"column\", 5)", "Running median of 5 points; optionally by group."),
    f("Signals", "exp_smooth(\"column\", 0.3)", "Exponential smoothing with weight 0.3 on the newest value; optionally by group."),
    f("Signals", "gaussian_smooth(\"column\", 2)", "Gaussian kernel smoothing with σ = 2 points; optionally by group."),
    f(
        "Signals",
        "derivative(\"y\", \"x\")",
        "dy/dx by central differences, with the rows sorted by x (duplicate x averaged); optionally by group.",
    ),
    f("Signals", "cum_integral(\"y\", \"x\")", "Running trapezoid integral of y over x from the smallest x; optionally by group."),
    f(
        "Signals",
        "interpolate(\"y\", \"x\", 2.5)",
        "y read off at x = 2.5 by straight lines between points (missing outside the data); interpolate_spline \
         uses a cubic spline, interpolate_pchip a shape-preserving cubic.",
    ),
    f(
        "Signal processing",
        "lowpass_filter(\"column\", 0.1)",
        "The column (in row order) through a fourth-order Butterworth low-pass filter applied forwards and \
         backwards (zero phase); the cut-off is in cycles per row, between 0 and 0.5. highpass_filter is the \
         same above the cut-off. Optionally by group as a third argument.",
    ),
    f(
        "Signal processing",
        "bandpass_filter(\"column\", 0.05, 0.2)",
        "Keeps the frequencies between two edges (cycles per row); bandstop_filter removes them. Optionally by group.",
    ),
    f(
        "Signal processing",
        "envelope(\"column\")",
        "The envelope (magnitude of the analytic signal from the Hilbert transform); instant_phase gives its \
         unwrapped phase in radians. Optionally by group.",
    ),
    f(
        "Signal processing",
        "wavelet_denoise(\"column\", \"db4\")",
        "The column denoised by soft-thresholding its wavelet coefficients (universal threshold, noise from the \
         finest level); the wavelet defaults to db4. Optionally by group as a third argument.",
    ),
    f("Random numbers", "random_uniform()", "Uniform on [0, 1); random_uniform(low, high)."),
    f("Random numbers", "random_normal()", "Standard normal; random_normal(mean, sd)."),
    f("Random numbers", "random_integer(1, 6)", "A whole number between two limits."),
    f("Random numbers", "random_seed(42); random_normal()", "Choose the seed (the default is 0)."),
];

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::{TemporalDisplay, TemporalFormat};

    fn table() -> DataTable {
        DataTable::new(
            "t",
            vec![
                Column::numeric("x", vec![1.0, 2.0, f64::NAN, 4.0, 5.0]),
                Column::text("g", vec!["a".into(), "b".into(), "a".into(), "b".into(), "a".into()]),
                Column::numeric("Body mass", vec![10.0, 20.0, 30.0, 40.0, 50.0]),
                Column::date_time(
                    "when",
                    vec![
                        seconds_from_civil(2024, 1, 31, 6, 30, 0.0),
                        seconds_from_civil(2024, 2, 29, 0, 0, 0.0),
                        f64::NAN,
                        seconds_from_civil(2025, 12, 31, 23, 59, 59.0),
                        seconds_from_civil(1969, 12, 31, 0, 0, 0.0),
                    ],
                    TemporalDisplay::new(TemporalFormat::DateTime),
                ),
            ],
        )
        .unwrap()
    }

    fn values(formula: &str) -> Vec<FormulaValue> {
        let t = table();
        let eval = evaluate(&t, None, formula, 0..t.n_rows()).unwrap_or_else(|e| panic!("{formula}: {e}"));
        assert!(eval.failures.is_empty(), "{formula}: {:?}", eval.failures);
        eval.values
    }

    fn numbers(formula: &str) -> Vec<f64> {
        values(formula)
            .into_iter()
            .map(|v| match v {
                FormulaValue::Number(x) => x,
                FormulaValue::Missing => f64::NAN,
                FormulaValue::Text(s) => panic!("{formula} gave text {s:?}"),
            })
            .collect()
    }

    fn texts(formula: &str) -> Vec<String> {
        values(formula).iter().map(FormulaValue::text).collect()
    }

    fn close(a: &[f64], b: &[f64]) {
        assert_eq!(a.len(), b.len());
        for (x, y) in a.iter().zip(b) {
            assert!((x.is_nan() && y.is_nan()) || (x - y).abs() <= 1e-12 * y.abs().max(1.0), "{a:?} vs {b:?}");
        }
    }

    #[test]
    fn arithmetic_columns_and_missing_values() {
        close(&numbers("x * 2 + 1"), &[3.0, 5.0, f64::NAN, 9.0, 11.0]);
        close(&numbers("col(\"Body mass\") / x"), &[10.0, 10.0, f64::NAN, 10.0, 10.0]);
        close(&numbers("1 / 2"), &[0.5; 5]);
        close(&numbers("row() / 2"), &[0.5, 1.0, 1.5, 2.0, 2.5]);
        close(&numbers("7 % 4 + 2 * 3 - 1"), &[8.0; 5]);
        close(&numbers("sqrt(16) + ln(1) + abs(-2)"), &[6.0; 5]);
        close(&numbers("round(2.345, 2)"), &[2.35; 5]);
        close(&numbers("if_missing(x, 0)"), &[1.0, 2.0, 0.0, 4.0, 5.0]);
        close(&numbers("is_missing(x)"), &[0.0, 0.0, 1.0, 0.0, 0.0]);
        close(&numbers("if x > 2 { 1 } else { 0 }"), &[0.0, 0.0, 0.0, 1.0, 1.0]);
        close(&numbers("let y = x + 1; y * y"), &[4.0, 9.0, f64::NAN, 25.0, 36.0]);
        assert_eq!(values("if x > 4 { 1 }")[0], FormulaValue::Missing);
        assert_eq!(texts("if g == \"a\" { \"first\" } else { \"second\" }")[..2], ["first", "second"]);
    }

    #[test]
    fn distributions_match_known_values() {
        close(&numbers("normal_cdf(1.96)")[..1], &[0.975_002_104_851_780_1]);
        close(&numbers("normal_quantile(0.975, 10, 2)")[..1], &[10.0 + 2.0 * 1.959_963_984_540_054]);
        close(&numbers("t_quantile(0.975, 10)")[..1], &[2.228_138_851_986_274]);
        close(&numbers("chisq_cdf(3.841458820694124, 1)")[..1], &[0.95]);
        close(&numbers("binomial_pmf(2, 4, 0.5)")[..1], &[0.375]);
        close(&numbers("poisson_cdf(0, 2)")[..1], &[(-2.0f64).exp()]);
    }

    #[test]
    fn text_functions() {
        assert_eq!(texts("upper(g) + text(x)")[..2], ["A1", "B2"]);
        assert_eq!(texts("concat(g, \"-\", x)")[2], "a-");
        assert_eq!(texts("substr(\"statistics\", 4, 3)")[0], "ist");
        assert_eq!(numbers("find(\"statistics\", \"tis\")")[0], 3.0);
        assert_eq!(texts("replace(\"a-b-c\", \"-\", \"+\")")[0], "a+b+c");
        assert_eq!(texts("word(\"red, green, blue\", 1, \", \")")[0], "green");
        assert_eq!(texts("if has_item(\"Email; Phone\", \"Phone\") { \"y\" } else { \"n\" }")[0], "y");
        assert_eq!(texts("if has_item(\"Email, Phone\", \"Mail\", \",\") { \"y\" } else { \"n\" }")[0], "n");
        close(&numbers("item_count(\"a; b; a; ;\")")[..1], &[2.0]);
        assert_eq!(texts("trim(\"  x \")")[0], "x");
        assert_eq!(numbers("number(\" 2.5 \") * 2")[0], 5.0);
        assert_eq!(numbers("regex_match(\"A12\", \"^[A-Z]\\\\d+$\")")[0], 1.0);
        assert_eq!(texts("regex_find(\"lot 42b\", \"\\\\d+\")")[0], "42");
        assert_eq!(texts("regex_replace(\"a   b\", \"\\\\s+\", \"_\")")[0], "a_b");
        let t = table();
        let bad = evaluate(&t, None, "regex_match(g, \"(\")", 0..1).unwrap();
        assert!(bad.failures[0].1.contains("regular expression"), "{:?}", bad.failures);
    }

    #[test]
    fn date_functions() {
        close(&numbers("year(when)"), &[2024.0, 2024.0, f64::NAN, 2025.0, 1969.0]);
        close(&numbers("month(when) * 100 + day(when)"), &[131.0, 229.0, f64::NAN, 1231.0, 1231.0]);
        close(&numbers("hour(when) * 60 + minute(when)")[..1], &[390.0]);
        // 2024-01-31 was a Wednesday; 1969-12-31 a Wednesday.
        close(&numbers("weekday(when)"), &[3.0, 4.0, f64::NAN, 3.0, 3.0]);
        close(&numbers("day_of_year(when)"), &[31.0, 60.0, f64::NAN, 365.0, 365.0]);
        let jan31 = seconds_from_civil(2024, 1, 31, 6, 30, 0.0);
        // Adding a month clamps to the end of the month and keeps the time.
        close(&numbers("date_add(when, 1, \"month\")")[..1], &[seconds_from_civil(2024, 2, 29, 6, 30, 0.0)]);
        close(&numbers("date_add(when, 2, \"days\")")[..1], &[jan31 + 2.0 * DAY]);
        close(&numbers("date_diff(make_date(2024, 1, 31), make_date(2024, 2, 29), \"months\")")[..1], &[0.0]);
        close(&numbers("date_diff(make_date(2024, 1, 31), make_date(2024, 3, 31), \"months\")")[..1], &[2.0]);
        close(&numbers("date_diff(make_date(2020, 2, 29), make_date(2024, 2, 28), \"years\")")[..1], &[3.0]);
        close(&numbers("date_diff(make_date(2024, 1, 1), make_date(2024, 1, 8), \"weeks\")")[..1], &[1.0]);
        assert_eq!(texts("format_date(when, \"%a %d %b %Y %H:%M:%S (%j)\")")[0], "Wed 31 Jan 2024 06:30:00 (031)");
        assert_eq!(texts("format_date(when, \"%Y\")")[2], "");
        let t = table();
        let bad = evaluate(&t, None, "make_date(2023, 2, 29)", 0..1).unwrap();
        assert!(bad.failures[0].1.contains("not a date"), "{:?}", bad.failures);
    }

    #[test]
    fn rows_summaries_and_ranks() {
        close(&numbers("row()"), &[1.0, 2.0, 3.0, 4.0, 5.0]);
        close(&numbers("lag(\"x\")"), &[f64::NAN, 1.0, 2.0, f64::NAN, 4.0]);
        close(&numbers("lead(\"x\", 2)"), &[f64::NAN, 4.0, 5.0, f64::NAN, f64::NAN]);
        close(&numbers("col(\"x\", n_rows())"), &[5.0; 5]);
        close(&numbers("col_mean(\"x\")"), &[3.0; 5]);
        close(&numbers("col_mean(\"x\", \"g\")"), &[3.0, 3.0, 3.0, 3.0, 3.0]);
        close(&numbers("col_sum(\"Body mass\", \"g\")"), &[90.0, 60.0, 90.0, 60.0, 90.0]);
        close(&numbers("col_count(\"x\", \"g\")"), &[2.0, 2.0, 2.0, 2.0, 2.0]);
        close(&numbers("col_sd(\"Body mass\")"), &[250f64.sqrt(); 5]);
        close(&numbers("col_min(\"x\") + col_max(\"x\")"), &[6.0; 5]);
        // Type 8 median of 1, 2, 4, 5.
        close(&numbers("col_quantile(\"x\", 0.5)"), &[3.0; 5]);
        close(&numbers("cum_sum(\"x\")"), &[1.0, 3.0, 3.0, 7.0, 12.0]);
        close(&numbers("cum_sum(\"x\", \"g\")"), &[1.0, 2.0, 1.0, 6.0, 6.0]);
        close(&numbers("rank(\"Body mass\") + 0 * x"), &[1.0, 2.0, f64::NAN, 4.0, 5.0]);
        close(&numbers("rank(\"x\", \"g\")"), &[1.0, 1.0, f64::NAN, 2.0, 2.0]);
        // p-values adjusted over the column (R p.adjust).
        let t = DataTable::new(
            "t",
            vec![
                Column::numeric("p", vec![0.01, 0.04, f64::NAN, 0.03]),
                Column::text("g", ["a", "a", "b", "b"].map(String::from).to_vec()),
            ],
        )
        .unwrap();
        let eval = |f: &str| -> Vec<f64> {
            let values = evaluate(&t, None, f, 0..4).unwrap().values;
            values.into_iter().map(|x| if let FormulaValue::Number(n) = x { n } else { f64::NAN }).collect()
        };
        close(&eval("p_adjust(\"p\", \"bonferroni\")"), &[0.03, 0.12, f64::NAN, 0.09]);
        close(&eval("p_adjust(\"p\", \"BH\")"), &[0.03, 0.04, f64::NAN, 0.04]);
        close(&eval("p_adjust(\"p\", \"holm\", \"g\")"), &[0.02, 0.04, f64::NAN, 0.03]);
        assert!(evaluate(&t, None, "p_adjust(\"p\", \"nope\")", 0..4).unwrap().failure_summary().is_some());
        // Curve transforms: p is 0.01, 0.04, missing, 0.03.
        close(&eval("percent_of_range(\"p\")"), &[0.0, 100.0, f64::NAN, 200.0 / 3.0]);
        close(&eval("fraction_of_total(\"p\", \"g\")"), &[0.2, 0.8, f64::NAN, 1.0]);
        close(&eval("subtract_baseline(\"p\", 2)"), &[-0.015, 0.015, f64::NAN, 0.005]);
        assert!(evaluate(&t, None, "subtract_baseline(\"p\", 0)", 0..4).unwrap().failure_summary().is_some());
        // Signal functions keep missing rows missing and work within groups.
        let wave: Vec<f64> = (0..64).map(|i| (i as f64 * 0.3).sin() + if i % 2 == 0 { 0.2 } else { -0.2 }).collect();
        let g: Vec<String> = (0..64).map(|i| if i < 32 { "a" } else { "b" }.to_owned()).collect();
        let t = DataTable::new("t", vec![Column::numeric("w", wave), Column::text("g", g)]).unwrap();
        let eval = |f: &str| -> Vec<f64> {
            let values = evaluate(&t, None, f, 0..64).unwrap().values;
            values.into_iter().map(|x| if let FormulaValue::Number(n) = x { n } else { f64::NAN }).collect()
        };
        // The alternating ±0.2 sits at 0.5 cycles per row: a low-pass removes it.
        let low = eval("lowpass_filter(\"w\", 0.2)");
        assert!((10..54).all(|i| (low[i] - (i as f64 * 0.3).sin()).abs() < 0.05));
        assert!(eval("highpass_filter(\"w\", 0.3, \"g\")").iter().all(|v| v.is_finite()));
        assert!(eval("bandpass_filter(\"w\", 0.02, 0.1)").iter().all(|v| v.is_finite()));
        assert!(eval("envelope(\"w\")").iter().all(|v| v.is_finite() && *v >= 0.0));
        assert!(eval("wavelet_denoise(\"w\", \"haar\", \"g\")").iter().all(|v| v.is_finite()));
        assert!(evaluate(&t, None, "lowpass_filter(\"w\", 0.6)", 0..64).unwrap().failure_summary().is_some());
        assert!(evaluate(&t, None, "bandpass_filter(\"w\", 0.2, 0.1)", 0..64).unwrap().failure_summary().is_some());
        // Ties share their average rank.
        let t = DataTable::new("t", vec![Column::numeric("v", vec![3.0, 1.0, 3.0, 2.0])]).unwrap();
        let eval = evaluate(&t, None, "rank(\"v\")", 0..4).unwrap();
        assert_eq!(eval.values, [3.5, 1.0, 3.5, 2.0].map(FormulaValue::Number));
        // Tie rules (pandas `rank(method=…)`), whole column and by group.
        for (rule, want) in [
            ("average", [3.5, 1.0, 3.5, 2.0]),
            ("min", [3.0, 1.0, 3.0, 2.0]),
            ("max", [4.0, 1.0, 4.0, 2.0]),
            ("dense", [3.0, 1.0, 3.0, 2.0]),
            ("first", [3.0, 1.0, 4.0, 2.0]),
        ] {
            let eval = evaluate(&t, None, &format!("rank_ties(\"v\", \"{rule}\")"), 0..4).unwrap();
            assert_eq!(eval.values, want.map(FormulaValue::Number), "{rule}");
        }
        let t = DataTable::new(
            "t",
            vec![
                Column::numeric("v", vec![3.0, 1.0, 3.0, 2.0, 2.0]),
                Column::text("g", vec!["a".into(), "a".into(), "a".into(), "b".into(), "b".into()]),
            ],
        )
        .unwrap();
        let eval = evaluate(&t, None, "rank_ties(\"v\", \"min\", \"g\")", 0..5).unwrap();
        assert_eq!(eval.values, [2.0, 1.0, 2.0, 1.0, 1.0].map(FormulaValue::Number));
        assert!(evaluate(&t, None, "rank_ties(\"v\", \"random\")", 0..5).unwrap().failure_summary().is_some());
    }

    #[test]
    fn signal_functions() {
        let x: Vec<f64> = (0..12).map(|i| i as f64 * 0.5).collect();
        let y: Vec<f64> = x.iter().map(|v| v * v).collect();
        let g: Vec<String> = (0..12).map(|i| if i < 6 { "a" } else { "b" }.to_string()).collect();
        let t = DataTable::new(
            "t",
            vec![Column::numeric("x", x.clone()), Column::numeric("y", y.clone()), Column::text("g", g)],
        )
        .unwrap();
        let eval = |f: &str| -> Vec<f64> {
            let e = evaluate(&t, None, f, 0..12).unwrap_or_else(|e| panic!("{f}: {e}"));
            assert!(e.failures.is_empty(), "{f}: {:?}", e.failures);
            e.values.into_iter().map(|v| if let FormulaValue::Number(n) = v { n } else { f64::NAN }).collect()
        };
        // A quadratic passes through a cubic Savitzky–Golay filter unchanged.
        close(&eval("savgol(\"y\", 5, 3)"), &y);
        close(&eval("savgol(\"y\", 5, 2, \"g\")"), &y);
        let ma = eval("moving_average(\"x\", 3)");
        close(&ma[1..11], &x[1..11]);
        close(&eval("running_median(\"x\", 3)")[1..11], &x[1..11]);
        assert!(eval("exp_smooth(\"x\", 0.5)").iter().all(|v| v.is_finite()));
        assert!(eval("gaussian_smooth(\"x\", 1)").iter().all(|v| v.is_finite()));
        // Central differences are exact for x² inside; the ends are one-sided.
        let d = eval("derivative(\"y\", \"x\")");
        close(&d[1..11], &x[1..11].iter().map(|v| 2.0 * v).collect::<Vec<_>>());
        let d = eval("derivative(\"y\", \"x\", \"g\")");
        close(&d[7..11], &x[7..11].iter().map(|v| 2.0 * v).collect::<Vec<_>>());
        // Trapezoid integral of x² from 0 to 5.5 at step 0.5: exact minus h²/12·(b − a)·2.
        let i = eval("cum_integral(\"y\", \"x\")");
        assert!((i[11] - (5.5f64.powi(3) / 3.0 + 0.25 / 12.0 * 5.5 * 2.0)).abs() < 1e-12, "{}", i[11]);
        let i = eval("cum_integral(\"y\", \"x\", \"g\")");
        assert_eq!(i[6], 0.0, "each group starts from zero");
        close(&eval("interpolate(\"y\", \"x\", 0.25)"), &[0.125; 12]);
        close(&eval("interpolate_spline(\"y\", \"x\", 0.25)"), &[0.0625; 12]);
        assert!(eval("interpolate_pchip(\"y\", \"x\", 9)").iter().all(|v| v.is_nan()), "missing outside the data");
        assert!(evaluate(&t, None, "moving_average(\"x\", 0)", 0..12).unwrap().failure_summary().is_some());
    }

    #[test]
    fn random_numbers_are_reproducible() {
        let a = numbers("random_normal()");
        assert_eq!(a, numbers("random_normal()"), "the same every time");
        assert_ne!(a[0], a[1], "different in each row");
        assert_ne!(a, numbers("random_seed(7); random_normal()"), "the seed matters");
        let u = numbers("random_uniform(10, 20)");
        assert!(u.iter().all(|x| (10.0..20.0).contains(x)));
        let k = numbers("random_integer(1, 6)");
        assert!(k.iter().all(|x| (1.0..=6.0).contains(x) && x.fract() == 0.0));
    }

    #[test]
    fn errors_point_at_the_formula() {
        let t = table();
        let e = evaluate(&t, None, "x +", 0..1).unwrap_err();
        assert!(e.position.is_some(), "{e}");
        let e = evaluate(&t, None, "y * 2", 0..1).unwrap_err();
        assert!(e.message.contains('y') && e.position == Some((1, 1)), "{e:?}");
        assert!(evaluate(&t, None, "  ", 0..1).unwrap_err().message.contains("empty"));
        let e = evaluate(&t, Some(0), "x + 1", 0..1).unwrap_err();
        assert!(e.message.contains("its own column"), "{e}");
        let eval = evaluate(&t, None, "col(\"nope\")", 0..2).unwrap();
        assert_eq!(eval.failures.len(), 2);
        assert!(eval.failure_summary().unwrap().starts_with("2 rows could not be computed"), "{eval:?}");
        let eval = evaluate(&t, None, "[1, 2]", 0..1).unwrap();
        assert!(eval.failures[0].1.contains("instead of a number"), "{eval:?}");
        let eval = evaluate(&t, None, "loop { }", 0..1).unwrap();
        assert!(eval.failures[0].1.contains("too long"), "{eval:?}");
    }

    #[test]
    fn references_and_renames() {
        let t = table();
        let f = "x * col(\"Body mass\") + lag(\"x\") + col_mean(\"x\", \"g\") + \"g\".len() + y.x + when()";
        assert_eq!(references(&t, f), [0, 1, 2]);
        assert_eq!(references(&t, "let x = 1; x"), Vec::<usize>::new(), "a local named like a column");
        assert_eq!(
            renamed("x + col(\"x\") + lag(\"x\", 1) + \"x\" // x", "x", "Mass (kg)"),
            "col(\"Mass (kg)\") + col(\"Mass (kg)\") + lag(\"Mass (kg)\", 1) + \"x\" // x"
        );
        assert_eq!(renamed("col(\"Body mass\") * 2", "Body mass", "mass"), "mass * 2");
        assert_eq!(renamed("col_mean(\"Body mass\")", "Body mass", "mass"), "col_mean(\"mass\")");
        assert_eq!(renamed("x.abs() + max_x", "x", "z"), "z.abs() + max_x");
        assert_eq!(column_reference("Body mass"), "col(\"Body mass\")");
        assert_eq!(column_reference("if"), "col(\"if\")");
    }

    /// Renaming a column to the name of a variable the formula declares
    /// keeps the formula's meaning (and its values), and undo restores it.
    #[test]
    fn renaming_to_a_local_variable_name_keeps_the_meaning() {
        assert_eq!(renamed("let b = 10; a + b", "a", "b"), "let b = 10; col(\"b\") + b");
        assert_eq!(renamed("let b = 10; col(\"a\") + b", "a", "b"), "let b = 10; col(\"b\") + b");
        assert_eq!(renamed("let b = 10; col(\"b\") + b", "b", "a"), "let b = 10; a + b");

        let mut f = Column::numeric("f", vec![0.0; 2]);
        f.formula = Some("let b = 10; a + b".into());
        let mut t = DataTable::new("t", vec![Column::numeric("a", vec![1.0, 2.0]), f]).unwrap();
        let mut rows = stat_data::RowStateManager::new(2);
        refresh_all(&mut t);
        let values = |t: &DataTable| t.column(1).unwrap().as_numeric().unwrap().to_vec();
        let close = |got: Vec<f64>, want: [f64; 2]| got.iter().zip(want).all(|(g, w)| (g - w).abs() < 1e-12);
        assert!(close(values(&t), [11.0, 12.0]));
        let applied =
            crate::editing::Edit::RenameColumn { index: 0, name: "b".into() }.apply(&mut t, &mut rows).unwrap();
        assert_eq!(references(&t, t.column(1).unwrap().formula.as_deref().unwrap()), [0]);
        t.set_value(0, 0, stat_data::CellValue::Number(5.0)).unwrap();
        refresh(&mut t, &Changed::Columns(vec!["b".into()]));
        assert!(close(values(&t), [15.0, 12.0]), "{:?}", values(&t));
        applied.inverse.apply(&mut t, &mut rows).unwrap();
        assert_eq!(t.column(1).unwrap().formula.as_deref(), Some("let b = 10; a + b"));
    }

    /// F-20: a `for` loop variable that shares a column's name is the loop's
    /// own variable inside the loop body (not a dependency), and renaming
    /// that column leaves the loop alone. After the loop the name means the
    /// column again.
    #[test]
    fn loop_variables_are_not_column_references() {
        let t = DataTable::new(
            "t",
            vec![
                Column::numeric("a", vec![1.0, 2.0]),
                Column::numeric("i", vec![3.0, 4.0]),
                Column::numeric("x", vec![5.0, 6.0]),
            ],
        )
        .unwrap();
        let single = "let s = 0; for a in 0..5 { s += a; } s + x";
        assert_eq!(references(&t, single), [2]);
        assert!(check(&t, Some(0), "let s = 0; for a in 0..5 { s += a; } s").is_ok(), "not a self-reference");
        let tuple = "let s = 0; for (v, i) in [10, 20] { s += v * i; } s";
        assert_eq!(references(&t, tuple), Vec::<usize>::new());
        assert_eq!(references(&t, "let s = 0; for a in 0..2 { s += a; } s + a"), [0], "after the loop: the column");
        // Renaming the column does not touch the loop variable.
        assert_eq!(renamed(single, "a", "a new"), single);
        assert_eq!(renamed(tuple, "i", "index"), tuple);
        assert_eq!(
            renamed("let s = 0; for a in 0..2 { s += a; } s + a", "a", "b"),
            "let s = 0; for a in 0..2 { s += a; } s + b"
        );
        // Renaming another column to the loop variable's name quotes it.
        assert_eq!(renamed("for a in 0..2 { } x", "x", "a"), "for a in 0..2 { } col(\"a\")");
        let eval = evaluate(&t, Some(0), "let s = 0; for a in 0..5 { s += a; } s", 0..2).unwrap();
        assert_eq!(eval.values[0], FormulaValue::Number(10.0));
        // The forms docs/formulas.md shows.
        let eval = evaluate(&t, None, "let s = 0; for k in 1..4 { s += k * x } s", 0..2).unwrap();
        assert_eq!(eval.values[0], FormulaValue::Number(30.0), "{:?}", eval.failures);
        let eval = evaluate(&t, None, "let s = 0; for (v, k) in [a, i, x] { s += v * k } s", 0..2).unwrap();
        assert_eq!(eval.values[0], FormulaValue::Number(13.0), "{:?}", eval.failures);
    }

    /// Formulas on a cycle keep their values without stopping the others,
    /// and the refresh names them.
    #[test]
    fn a_cycle_does_not_freeze_other_formula_columns() {
        let mut a = Column::numeric("a", vec![-1.0; 2]);
        a.formula = Some("b + 1".into());
        let mut b = Column::numeric("b", vec![-2.0; 2]);
        b.formula = Some("a + 1".into());
        let mut c = Column::numeric("c", vec![0.0; 2]);
        c.formula = Some("x * 2".into());
        let mut bad = Column::numeric("d", vec![7.0; 2]);
        bad.formula = Some("x +".into());
        let mut t = DataTable::new("t", vec![Column::numeric("x", vec![1.0, 2.0]), a, b, c, bad]).unwrap();
        let notes = refresh_all_noting(&mut t);
        assert_eq!(t.column(3).unwrap().as_numeric().unwrap(), &[2.0, 4.0], "c recomputes");
        assert_eq!(t.column(1).unwrap().as_numeric().unwrap(), &[-1.0, -1.0], "a keeps its values");
        assert_eq!(t.column(4).unwrap().as_numeric().unwrap(), &[7.0, 7.0], "d keeps its values");
        assert_eq!(notes.len(), 3, "{notes:?}");
        assert!(notes.iter().any(|n| n.contains("column a ") && n.contains("circle")), "{notes:?}");
        assert!(notes.iter().any(|n| n.contains("column b ") && n.contains("circle")), "{notes:?}");
        assert!(notes.iter().any(|n| n.contains("column d ")), "{notes:?}");
        assert!(!notes.iter().any(|n| n.contains("column c ")), "{notes:?}");
        assert!(check(&t, Some(3), "x * 3").is_ok());
    }

    #[test]
    fn refresh_follows_dependencies_and_rejects_cycles() {
        let mut t = table();
        let mut a = Column::empty("a", ColumnKind::Numeric, 5);
        a.formula = Some("x * 10".into());
        let mut b = Column::empty("b", ColumnKind::Numeric, 5);
        b.formula = Some("a + 1".into());
        // b comes before a: order follows the dependencies, not the columns.
        t.insert_column(4, b).unwrap();
        t.insert_column(5, a).unwrap();
        refresh_all(&mut t);
        assert_eq!(t.column(4).unwrap().as_numeric().unwrap()[..2], [11.0, 21.0]);
        t.set_value(0, 0, stat_data::CellValue::Number(7.0)).unwrap();
        refresh(&mut t, &Changed::Columns(vec!["x".into()]));
        assert_eq!(t.column(4).unwrap().as_numeric().unwrap()[0], 71.0);
        assert_eq!(users(&t, 0), ["a"]);
        assert_eq!(users(&t, 5), ["b"]);
        let e = check(&t, Some(5), "b * 2").unwrap_err();
        assert!(e.message.contains("circle") && e.message.contains("a"), "{e}");
        assert!(check(&t, Some(5), "x * 3").is_ok());
        // Text results turn the column into text.
        let mut c = Column::empty("c", ColumnKind::Numeric, 5);
        c.formula = Some("g + \"!\"".into());
        t.insert_column(6, c).unwrap();
        refresh_all(&mut t);
        assert_eq!(t.column(6).unwrap().cell_text(0), "a!");
        assert_eq!(t.column(6).unwrap().level, MeasurementLevel::Nominal);
    }

    #[test]
    fn a_million_rows_evaluate_quickly() {
        let n = if cfg!(debug_assertions) { 20_000 } else { 1_000_000 };
        let t = DataTable::new("big", vec![Column::numeric("x", (0..n).map(f64::from).collect())]).unwrap();
        let start = std::time::Instant::now();
        let eval = evaluate(&t, None, "x * 2 + 1", 0..t.n_rows()).unwrap();
        let elapsed = start.elapsed();
        assert_eq!(eval.values[n as usize - 1], FormulaValue::Number(2.0 * f64::from(n - 1) + 1.0));
        eprintln!("{n} rows in {elapsed:?}");
    }

    /// Which formulas count as row-wise.
    #[test]
    fn row_wise_formulas_are_told_from_the_rest() {
        for f in ["x * 2", "if is_missing(x) { 0 } else { sqrt(x) }", "col(\"Body mass\") / 2", "upper(g)"] {
            assert!(is_row_wise(f), "{f}");
        }
        for f in [
            "x - col_mean(\"x\")",
            "lag(\"x\")",
            "col(\"x\", 1)",
            "row()",
            "random_uniform()",
            "\"x\".col()",
            "Fn(\"col_mean\").call(\"x\")",
        ] {
            assert!(!is_row_wise(f), "{f}");
        }
    }

    /// A partial change recomputes a row-wise formula at the rows concerned
    /// only (rows elsewhere keep whatever they held), and a formula reading
    /// other rows in full.
    #[test]
    fn partial_changes_recompute_only_their_rows() {
        let mut t = table();
        let mut double = Column::numeric("double", vec![-1.0; 5]);
        double.formula = Some("x * 2".into());
        let mut centred = Column::numeric("centred", vec![-1.0; 5]);
        centred.formula = Some("x - col_mean(\"x\")".into());
        let mut chained = Column::numeric("chained", vec![-1.0; 5]);
        chained.formula = Some("double + 1".into());
        t.insert_column(4, double).unwrap();
        t.insert_column(5, centred).unwrap();
        t.insert_column(6, chained).unwrap();
        refresh(&mut t, &Changed::Partial(RowChanges::cell("x".into(), 1)));
        let numbers = |t: &DataTable, j: usize| t.column(j).unwrap().as_numeric().unwrap().to_vec();
        assert_eq!(numbers(&t, 4), [-1.0, 4.0, -1.0, -1.0, -1.0], "only row 2 of the row-wise formula");
        assert_eq!(numbers(&t, 6), [-1.0, 5.0, -1.0, -1.0, -1.0], "and of the formula using it");
        assert!((numbers(&t, 5)[0] - (1.0 - 3.0)).abs() < 1e-12, "the column mean in full");
        // Moving rows leaves row-wise formulas alone.
        let before = numbers(&t, 4);
        refresh(&mut t, &Changed::Partial(RowChanges::moved()));
        assert_eq!(numbers(&t, 4), before);
        // A change to a column the formulas do not read recomputes nothing.
        let rev = t.revision();
        refresh(&mut t, &Changed::Partial(RowChanges::cell("g".into(), 0)));
        assert_eq!(t.revision(), rev);
    }

    /// Changes in a row: positions of earlier changes follow rows the later
    /// change inserted; a move after row changes falls back to every row.
    #[test]
    fn row_changes_combine_in_order() {
        let a = Changed::Partial(RowChanges::cell("x".into(), 3));
        let b = Changed::Partial(RowChanges::inserted(vec![0, 5]));
        let Changed::Partial(both) = a.clone().and(b) else { panic!() };
        assert_eq!(both.rows, [4], "row 3 moved down past the row added at 0");
        assert_eq!(both.new_rows, [0, 5]);
        assert_eq!(a.and(Changed::Partial(RowChanges::moved())), Changed::Rows);
        let Changed::Partial(p) =
            Changed::Partial(RowChanges::moved()).and(Changed::Partial(RowChanges::cell("x".into(), 0)))
        else {
            panic!()
        };
        assert!(p.moved && p.rows == [0]);
    }

    /// Every function that reads the evaluation context is classified: a
    /// tripwire for new library functions, which must be added to
    /// [`COLUMN_FUNCTIONS`] or [`WHOLE_TABLE_FUNCTIONS`] when they read more
    /// than the current row (then the count here goes up by their number of
    /// registrations), or left out when, like the regular expressions, they
    /// use the context only as a cache.
    #[test]
    fn functions_reading_the_context_are_classified() {
        let source = include_str!("formula.rs");
        let engine_body = &source[source.find("fn engine(ctx").unwrap()..source.find("fn unit_error").unwrap()];
        assert_eq!(engine_body.matches("Rc::clone(ctx)").count(), 45);
    }
}
