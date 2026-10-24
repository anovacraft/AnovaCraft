//! Factorial repeated-measures ANOVA bound to a dataset
//! ([`stat_inference::repeated::factorial_repeated_measures`]): one to three
//! within-subject factors, up to three between-subject factors (four factors
//! in all), univariate tests with Greenhouse–Geisser and Huynh–Feldt
//! corrections per within-subject effect, Mauchly's tests, cell means, simple
//! effects of one factor at each level of another, and a profile plot.
//!
//! The data are stacked (long): one row per measurement, with a response
//! column, a subject column and one column per factor. A subject's
//! between-subject levels must be the same on every row; several rows in one
//! within-subject cell are averaged (and counted in the report); subjects
//! missing a cell are left out. Wide data (one column per condition) can be
//! stacked with Data ▸ Reshape ▸ Stack columns first.
//!
//! Simple effects use a separate analysis at each level of the second factor
//! (the data of that level only, so each test has its own error term and
//! sphericity correction), as Maxwell & Delaney (2nd ed., ch. 12) recommend
//! for within-subject factors.

use crate::compare::section;
use crate::format::{format_number, format_p_value};
use crate::project::{invalid, member, obj, s};
use crate::report::{PlotSpec, PointGroups, ReportSection, RowPoint, SectionContent, SectionKind, TextTable};
use stat_core::Result;
use stat_data::{DataTable, JsonValue, RowStateManager};
use stat_inference::repeated::{BetweenFactor, FactorialRepeatedMeasures, factorial_repeated_measures};
use stat_linear::natural_order;
use std::collections::HashMap;

const DIGITS: usize = 5;
/// Most factors in one analysis.
pub const MAX_FACTORS: usize = 4;

/// Settings of a factorial repeated-measures report.
#[derive(Debug, Clone, PartialEq)]
pub struct FactorialRepeatedRequest {
    /// Response column (numeric).
    pub response: usize,
    /// Subject identifier column.
    pub subject: usize,
    /// Within-subject factor columns (one to three).
    pub within: Vec<usize>,
    /// Between-subject factor columns (up to three).
    pub between: Vec<usize>,
    /// Simple effects: `(factor, by)`, the effect of `factor` at each level
    /// of `by` (both among the factors).
    pub simple: Option<(usize, usize)>,
}

impl FactorialRepeatedRequest {
    /// Every column the request uses.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        let mut c = vec![self.response, self.subject];
        c.extend(&self.within);
        c.extend(&self.between);
        c
    }

    /// The request with its columns renumbered by `f`.
    #[must_use]
    pub fn map_columns(&self, f: &dyn Fn(usize) -> usize) -> Self {
        Self {
            response: f(self.response),
            subject: f(self.subject),
            within: self.within.iter().map(|&c| f(c)).collect(),
            between: self.between.iter().map(|&c| f(c)).collect(),
            simple: self.simple.map(|(a, b)| (f(a), f(b))),
        }
    }

    /// Project-file form.
    #[must_use]
    pub fn to_json(&self, table: &DataTable) -> JsonValue {
        let name = |i: usize| table.column(i).map_or(String::new(), |c| c.name.clone());
        let list = |v: &[usize]| JsonValue::Array(v.iter().map(|&c| s(&name(c))).collect());
        obj([
            ("analysis", s("factorial-repeated-measures")),
            ("response", s(&name(self.response))),
            ("subject", s(&name(self.subject))),
            ("within", list(&self.within)),
            ("between", list(&self.between)),
            (
                "simple_effects",
                self.simple.map_or(JsonValue::Null, |(a, b)| obj([("factor", s(&name(a))), ("by", s(&name(b)))])),
            ),
        ])
    }

    /// Read the project-file form.
    ///
    /// # Errors
    /// Missing columns or malformed lists.
    pub fn from_json(v: &JsonValue, table: &DataTable) -> Result<Self> {
        let find =
            |name: &str| table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")));
        let col = |key: &str| -> Result<usize> {
            find(member(v, key)?.as_str().ok_or_else(|| invalid(format!("{key}: expected a column name")))?)
        };
        let list = |key: &str| -> Result<Vec<usize>> {
            match v.get(key) {
                None | Some(JsonValue::Null) => Ok(Vec::new()),
                Some(JsonValue::Array(items)) => items
                    .iter()
                    .map(|i| find(i.as_str().ok_or_else(|| invalid(format!("{key}: expected column names")))?))
                    .collect(),
                Some(_) => Err(invalid(format!("{key}: expected a list of column names"))),
            }
        };
        let simple = match v.get("simple_effects") {
            None | Some(JsonValue::Null) => None,
            Some(m) => {
                let name = |k: &str| {
                    m.get(k).and_then(JsonValue::as_str).ok_or_else(|| invalid(format!("simple_effects: {k} missing")))
                };
                Some((find(name("factor")?)?, find(name("by")?)?))
            }
        };
        Ok(Self {
            response: col("response")?,
            subject: col("subject")?,
            within: list("within")?,
            between: list("between")?,
            simple,
        })
    }
}

/// One factor as used.
#[derive(Debug, Clone)]
struct Factor {
    name: String,
    levels: Vec<String>,
    within: bool,
}

/// One complete subject.
#[derive(Debug, Clone)]
struct Subject {
    /// Level of each between-subject factor.
    between: Vec<usize>,
    /// Mean response in each within-subject cell (last factor fastest).
    cells: Vec<f64>,
}

/// A computed factorial repeated-measures report.
#[derive(Debug, Clone)]
pub struct FactorialRepeatedReport {
    request: FactorialRepeatedRequest,
    response_name: String,
    /// Within-subject factors, then between-subject ones.
    factors: Vec<Factor>,
    n_within: usize,
    subjects: Vec<Subject>,
    /// `(row, subject index, cell index)` of every measurement used.
    rows: Vec<(usize, usize, usize, f64)>,
    dropped: usize,
    averaged_cells: usize,
    /// The analysis.
    pub analysis: FactorialRepeatedMeasures,
}

impl FactorialRepeatedReport {
    /// Gather complete subjects and run the analysis.
    ///
    /// # Errors
    /// Repeated or invalid columns, no within-subject factor, too many
    /// factors, a factor with one level, a subject in two between-subject
    /// groups, or the engine's errors.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &FactorialRepeatedRequest) -> Result<Self> {
        let cols = request.columns();
        if (1..cols.len()).any(|i| cols[..i].contains(&cols[i])) {
            return Err(invalid("the response, subject and factor columns must all differ"));
        }
        if request.within.is_empty() {
            return Err(invalid("choose at least one within-subject factor"));
        }
        if request.within.len() + request.between.len() > MAX_FACTORS {
            return Err(invalid(format!("use at most {MAX_FACTORS} factors in all")));
        }
        let response = table.column(request.response).ok_or_else(|| invalid("no such column"))?;
        let y = response.as_numeric().ok_or_else(|| invalid(format!("{} must be numeric", response.name)))?;
        let text = |c: usize, r: usize| table.column(c).map(|c| c.level_text(r)).unwrap_or_default();
        let factor_cols: Vec<usize> = request.within.iter().chain(&request.between).copied().collect();
        let used: Vec<usize> = (0..table.n_rows())
            .filter(|&r| {
                !states.is_excluded(r)
                    && y[r].is_finite()
                    && !text(request.subject, r).is_empty()
                    && factor_cols.iter().all(|&c| !text(c, r).is_empty())
            })
            .collect();
        let mut factors = Vec::new();
        for (k, &c) in factor_cols.iter().enumerate() {
            let column = table.column(c).ok_or_else(|| invalid("no such column"))?;
            let mut levels: Vec<String> = Vec::new();
            for &r in &used {
                let v = column.level_text(r);
                if !levels.contains(&v) {
                    levels.push(v);
                }
            }
            levels.sort_by(|a, b| natural_order(a, b));
            if levels.len() < 2 {
                return Err(invalid(format!("the factor {} needs at least two levels", column.name)));
            }
            factors.push(Factor { name: column.name.clone(), levels, within: k < request.within.len() });
        }
        let n_within = request.within.len();
        let within_levels: Vec<usize> = factors[..n_within].iter().map(|f| f.levels.len()).collect();
        let n_cells: usize = within_levels.iter().product();
        let level_of = |k: usize, r: usize| {
            let v = text(factor_cols[k], r);
            factors[k].levels.iter().position(|l| *l == v).unwrap_or(0)
        };
        // Subject → between levels, cell sums and counts, rows.
        let mut ids: Vec<String> = Vec::new();
        let mut index: HashMap<String, usize> = HashMap::new();
        let mut between_of: Vec<Vec<usize>> = Vec::new();
        let mut sums: Vec<Vec<(f64, usize)>> = Vec::new();
        let mut members: Vec<Vec<(usize, usize, f64)>> = Vec::new();
        for &r in &used {
            let id = text(request.subject, r);
            let b: Vec<usize> = (n_within..factors.len()).map(|k| level_of(k, r)).collect();
            let i = *index.entry(id.clone()).or_insert_with(|| {
                ids.push(id.clone());
                between_of.push(b.clone());
                sums.push(vec![(0.0, 0); n_cells]);
                members.push(Vec::new());
                ids.len() - 1
            });
            if between_of[i] != b {
                return Err(invalid(format!(
                    "subject {id:?} has more than one level of a between-subject factor; a factor that changes \
                     within a subject is a within-subject factor"
                )));
            }
            let cell = (0..n_within).fold(0, |acc, k| acc * within_levels[k] + level_of(k, r));
            sums[i][cell].0 += y[r];
            sums[i][cell].1 += 1;
            members[i].push((r, cell, y[r]));
        }
        let mut order: Vec<usize> = (0..ids.len()).collect();
        order.sort_by(|&a, &b| natural_order(&ids[a], &ids[b]));
        let mut subjects = Vec::new();
        let mut rows = Vec::new();
        let (mut dropped, mut averaged_cells) = (0, 0);
        for i in order {
            if sums[i].iter().any(|c| c.1 == 0) {
                dropped += 1;
                continue;
            }
            averaged_cells += sums[i].iter().filter(|c| c.1 > 1).count();
            let k = subjects.len();
            rows.extend(members[i].iter().map(|&(r, cell, v)| (r, k, cell, v)));
            subjects.push(Subject {
                between: between_of[i].clone(),
                cells: sums[i].iter().map(|c| c.0 / c.1 as f64).collect(),
            });
        }
        if subjects.len() < 2 {
            return Err(invalid("fewer than two subjects have a value in every within-subject cell"));
        }
        let mut report = Self {
            request: request.clone(),
            response_name: response.name.clone(),
            factors,
            n_within,
            subjects,
            rows,
            dropped,
            averaged_cells,
            analysis: FactorialRepeatedMeasures { n_subjects: 0, residual_df: 0.0, effects: Vec::new() },
        };
        report.analysis = report.analyse(None)?;
        Ok(report)
    }

    /// Run the engine on all subjects and cells, or on those at level `at.1`
    /// of factor `at.0` (that factor then left out of the model).
    fn analyse(&self, at: Option<(usize, usize)>) -> Result<FactorialRepeatedMeasures> {
        let within_levels: Vec<usize> = self.factors[..self.n_within].iter().map(|f| f.levels.len()).collect();
        let keep_factor = |k: usize| at.is_none_or(|(f, _)| f != k);
        let w_kept: Vec<usize> = (0..self.n_within).filter(|&k| keep_factor(k)).collect();
        let b_kept: Vec<usize> = (self.n_within..self.factors.len()).filter(|&k| keep_factor(k)).collect();
        // Cells kept: all, or those at the chosen within level.
        let cells: Vec<usize> = (0..within_levels.iter().product())
            .filter(|&cell| match at {
                Some((f, level)) if f < self.n_within => {
                    let stride: usize = within_levels[f + 1..].iter().product();
                    (cell / stride) % within_levels[f] == level
                }
                _ => true,
            })
            .collect();
        let subjects: Vec<&Subject> = self
            .subjects
            .iter()
            .filter(|s| match at {
                Some((f, level)) if f >= self.n_within => s.between[f - self.n_within] == level,
                _ => true,
            })
            .collect();
        let data: Vec<Vec<f64>> = subjects.iter().map(|s| cells.iter().map(|&c| s.cells[c]).collect()).collect();
        let names: Vec<String> = w_kept.iter().map(|&k| self.factors[k].name.clone()).collect();
        let levels: Vec<usize> = w_kept.iter().map(|&k| within_levels[k]).collect();
        let between: Vec<BetweenFactor> = b_kept
            .iter()
            .map(|&k| BetweenFactor {
                name: self.factors[k].name.clone(),
                n_levels: self.factors[k].levels.len(),
                codes: subjects.iter().map(|s| s.between[k - self.n_within]).collect(),
            })
            .collect();
        // Renumber the levels present in this subset; drop a between
        // factor with a single level left.
        let between: Vec<BetweenFactor> = between
            .into_iter()
            .filter_map(|mut b| {
                let mut seen: Vec<usize> = b.codes.clone();
                seen.sort_unstable();
                seen.dedup();
                if seen.len() < 2 {
                    return None;
                }
                for c in &mut b.codes {
                    *c = seen.binary_search(c).unwrap_or(0);
                }
                b.n_levels = seen.len();
                Some(b)
            })
            .collect();
        factorial_repeated_measures(&data, &names, &levels, &between)
    }

    /// Number of complete subjects.
    #[must_use]
    pub fn n_subjects(&self) -> usize {
        self.subjects.len()
    }

    /// Names of the factors (within-subject first).
    #[must_use]
    pub fn factor_names(&self) -> Vec<String> {
        self.factors.iter().map(|f| f.name.clone()).collect()
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        let names = |w: bool| -> Vec<String> {
            self.factors.iter().filter(|f| f.within == w).map(|f| f.name.clone()).collect()
        };
        let within = names(true).join(" × ");
        let between = names(false);
        if between.is_empty() {
            format!("Repeated Measures: {} by {within}", self.response_name)
        } else {
            format!("Repeated Measures: {} by {within} (within), {} (between)", self.response_name, between.join(" × "))
        }
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let mut out = vec![self.design_section(), self.cell_means_section(), self.tests_section()];
        if let Some(s) = self.sphericity_section() {
            out.push(s);
        }
        if let Some((a, b)) = self.request.simple {
            out.push(self.simple_effects_section(a, b));
        }
        out.push(self.profile_section());
        out
    }

    fn design_section(&self) -> ReportSection {
        let mut t = TextTable::new(["Factor", "Type", "Levels"]);
        for f in &self.factors {
            t.push([
                f.name.clone(),
                if f.within { "Within subjects" } else { "Between subjects" }.to_owned(),
                f.levels.join(", "),
            ]);
        }
        let mut notes = Vec::new();
        if self.dropped > 0 {
            notes.push(format!("{} subjects missing a within-subject cell left out", self.dropped));
        }
        if self.averaged_cells > 0 {
            notes.push(format!("{} subject cells with several rows averaged", self.averaged_cells));
        }
        let extra = if notes.is_empty() { String::new() } else { format!("; {}", notes.join("; ")) };
        section(
            SectionKind::RepeatedDesign,
            format!("Design ({} complete subjects{extra})", self.subjects.len()),
            SectionContent::Table(t),
        )
    }

    /// Combinations of every factor's levels with the subjects' values in each.
    fn cells(&self) -> Vec<(Vec<usize>, Vec<f64>)> {
        let within_levels: Vec<usize> = self.factors[..self.n_within].iter().map(|f| f.levels.len()).collect();
        let n_cells: usize = within_levels.iter().product();
        let n_between: usize = self.factors[self.n_within..].iter().map(|f| f.levels.len()).product();
        let between_levels: Vec<usize> = self.factors[self.n_within..].iter().map(|f| f.levels.len()).collect();
        let mut out = Vec::new();
        for b in 0..n_between {
            let mut bcodes = vec![0; between_levels.len()];
            let mut rest = b;
            for k in (0..between_levels.len()).rev() {
                bcodes[k] = rest % between_levels[k];
                rest /= between_levels[k];
            }
            for cell in 0..n_cells {
                let mut wcodes = vec![0; within_levels.len()];
                let mut rest = cell;
                for k in (0..within_levels.len()).rev() {
                    wcodes[k] = rest % within_levels[k];
                    rest /= within_levels[k];
                }
                let values: Vec<f64> =
                    self.subjects.iter().filter(|s| s.between == bcodes).map(|s| s.cells[cell]).collect();
                let mut codes = wcodes;
                codes.extend(&bcodes);
                out.push((codes, values));
            }
        }
        out
    }

    fn cell_means_section(&self) -> ReportSection {
        let mut heads: Vec<String> = self.factors.iter().map(|f| f.name.clone()).collect();
        heads.extend(["Subjects", "Mean", "Std dev", "Std error"].map(str::to_owned));
        let mut t = TextTable::new(heads);
        for (codes, v) in self.cells() {
            let n = v.len() as f64;
            let mean = v.iter().sum::<f64>() / n;
            let sd = if v.len() > 1 {
                (v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1.0)).sqrt()
            } else {
                f64::NAN
            };
            let mut row: Vec<String> =
                codes.iter().enumerate().map(|(k, &c)| self.factors[k].levels[c].clone()).collect();
            row.extend([
                v.len().to_string(),
                format_number(mean, DIGITS),
                format_number(sd, DIGITS),
                format_number(sd / n.sqrt(), DIGITS),
            ]);
            t.push(row);
        }
        section(
            SectionKind::RepeatedCellMeans,
            format!("Cell Means of {} (each subject's cell value counts once)", self.response_name),
            SectionContent::Table(t),
        )
    }

    fn tests_section(&self) -> ReportSection {
        let mut t = TextTable::new(
            [
                "Effect",
                "Sum of squares",
                "DF",
                "Error SS",
                "Error DF",
                "F ratio",
                "p-value",
                "GG ε",
                "p (GG)",
                "HF ε",
                "p (HF)",
                "Partial η²",
                "Generalized η²",
            ]
            .map(str::to_owned),
        );
        for e in &self.analysis.effects {
            let (gg, hf) = match &e.sphericity {
                Some(s) => (format_number(s.greenhouse_geisser, 4), format_number(s.huynh_feldt_capped(), 4)),
                None => (String::new(), String::new()),
            };
            let corrected = |p: f64| if e.sphericity.is_some() { format_p_value(p) } else { String::new() };
            t.push([
                e.name.clone(),
                format_number(e.ss, DIGITS),
                format_number(e.df, 4),
                format_number(e.ss_error, DIGITS),
                format_number(e.df_error, 4),
                format_number(e.f_statistic, DIGITS),
                format_p_value(e.p_value),
                gg,
                corrected(e.p_value_gg),
                hf,
                corrected(e.p_value_hf),
                format_number(e.partial_eta_squared, 4),
                format_number(e.generalized_eta_squared, 4),
            ]);
        }
        let unequal = self.unequal_groups();
        let note = if unequal { "; between-subject groups of unequal size: Type III sums of squares" } else { "" };
        section(
            SectionKind::RepeatedTests,
            format!(
                "Univariate Tests (each within-subject effect against its own Effect × Subjects error; GG and HF: \
                 Greenhouse–Geisser and Huynh–Feldt corrected degrees of freedom{note})"
            ),
            SectionContent::Table(t),
        )
    }

    fn unequal_groups(&self) -> bool {
        let mut counts: HashMap<&[usize], usize> = HashMap::new();
        for s in &self.subjects {
            *counts.entry(&s.between).or_default() += 1;
        }
        let first = counts.values().next().copied().unwrap_or(0);
        counts.values().any(|&c| c != first)
    }

    fn sphericity_section(&self) -> Option<ReportSection> {
        let mut t = TextTable::new(["Within-subject effect", "Mauchly's W", "χ²", "DF", "p-value", "GG ε", "HF ε"]);
        for e in self.analysis.effects.iter().filter(|e| e.between.is_empty()) {
            if let Some(s) = &e.sphericity {
                let ok = s.mauchly_w.is_finite();
                t.push([
                    e.name.clone(),
                    if ok { format_number(s.mauchly_w, 4) } else { "not available".to_owned() },
                    if ok { format_number(s.chi_square, DIGITS) } else { String::new() },
                    format_number(s.df, 4),
                    if ok { format_p_value(s.p_value) } else { String::new() },
                    format_number(s.greenhouse_geisser, 4),
                    format_number(s.huynh_feldt, 4),
                ]);
            }
        }
        (!t.rows.is_empty()).then(|| {
            section(
                SectionKind::Sphericity,
                "Sphericity (Mauchly's test; a small p-value means use a corrected test; HF ε above 1 is used as 1; \
                 effects with two levels always meet sphericity)",
                SectionContent::Table(t),
            )
        })
    }

    fn simple_effects_section(&self, factor: usize, by: usize) -> ReportSection {
        let cols: Vec<usize> = self.request.within.iter().chain(&self.request.between).copied().collect();
        let (Some(a), Some(b)) = (cols.iter().position(|&c| c == factor), cols.iter().position(|&c| c == by)) else {
            return section(
                SectionKind::SimpleEffects,
                "Simple Effects",
                SectionContent::Note("Not available: choose two of the analysis's factors.".to_owned()),
            );
        };
        if a == b {
            return section(
                SectionKind::SimpleEffects,
                "Simple Effects",
                SectionContent::Note("Not available: choose two different factors.".to_owned()),
            );
        }
        let (fa, fb) = (&self.factors[a], &self.factors[b]);
        let mut t = TextTable::new(
            [
                fb.name.as_str(),
                "DF",
                "Error DF",
                "F ratio",
                "p-value",
                "GG ε",
                "p (GG)",
                "HF ε",
                "p (HF)",
                "Partial η²",
            ]
            .map(str::to_owned),
        );
        for (level, label) in fb.levels.iter().enumerate() {
            let row = self.analyse(Some((b, level))).map_err(|e| e.to_string()).and_then(|r| {
                r.effects
                    .into_iter()
                    .find(|e| {
                        let mine = if fa.within { &e.within } else { &e.between };
                        let other = if fa.within { &e.between } else { &e.within };
                        // The pure effect of `a` in the subset's model.
                        other.is_empty() && mine.len() == 1 && e.name == fa.name
                    })
                    .ok_or_else(|| "the factor has no test at this level".to_owned())
            });
            match row {
                Ok(e) => {
                    let (gg, hf) = match &e.sphericity {
                        Some(s) => (format_number(s.greenhouse_geisser, 4), format_number(s.huynh_feldt_capped(), 4)),
                        None => (String::new(), String::new()),
                    };
                    let corrected = |p: f64| if e.sphericity.is_some() { format_p_value(p) } else { String::new() };
                    t.push([
                        label.clone(),
                        format_number(e.df, 4),
                        format_number(e.df_error, 4),
                        format_number(e.f_statistic, DIGITS),
                        format_p_value(e.p_value),
                        gg,
                        corrected(e.p_value_gg),
                        hf,
                        corrected(e.p_value_hf),
                        format_number(e.partial_eta_squared, 4),
                    ]);
                }
                Err(err) => {
                    let mut row = vec![label.clone(), format!("not available: {err}")];
                    row.extend(std::iter::repeat_n(String::new(), 8));
                    t.push(row);
                }
            }
        }
        section(
            SectionKind::SimpleEffects,
            format!(
                "Simple Effects of {} at Each Level of {} (a separate analysis of each level's data, with its own \
                 error term; unadjusted p-values)",
                fa.name, fb.name
            ),
            SectionContent::Table(t),
        )
    }

    /// Means across the first within-subject factor, one trace per
    /// combination of the other factors; every measurement is a point.
    fn profile_section(&self) -> ReportSection {
        let first = &self.factors[0];
        let others: Vec<usize> = (1..self.factors.len()).collect();
        let trace_of = |codes: &[usize]| -> usize {
            others.iter().fold(0, |acc, &k| acc * self.factors[k].levels.len() + codes[k])
        };
        let n_traces: usize = others.iter().map(|&k| self.factors[k].levels.len()).product();
        let mut lines: Vec<Vec<[f64; 2]>> = vec![Vec::new(); n_traces];
        let mut labels = vec![String::new(); n_traces];
        let mut by_trace: Vec<Vec<(usize, f64)>> = vec![Vec::new(); n_traces];
        for (codes, v) in self.cells() {
            let tr = trace_of(&codes);
            by_trace[tr].push((codes[0], v.iter().sum::<f64>() / v.len() as f64));
            labels[tr] =
                others.iter().map(|&k| self.factors[k].levels[codes[k]].clone()).collect::<Vec<_>>().join(", ");
        }
        for (tr, pts) in by_trace.iter_mut().enumerate() {
            pts.sort_by_key(|p| p.0);
            lines[tr] = pts.iter().map(|&(x, m)| [(x + 1) as f64, m]).collect();
        }
        let within_levels: Vec<usize> = self.factors[..self.n_within].iter().map(|f| f.levels.len()).collect();
        let mut points = Vec::new();
        let mut membership = Vec::new();
        for &(row, subj, cell, v) in &self.rows {
            let mut codes = vec![0; self.factors.len()];
            let mut rest = cell;
            for k in (0..self.n_within).rev() {
                codes[k] = rest % within_levels[k];
                rest /= within_levels[k];
            }
            for (j, &b) in self.subjects[subj].between.iter().enumerate() {
                codes[self.n_within + j] = b;
            }
            points.push(RowPoint { row, x: (codes[0] + 1) as f64, y: v });
            membership.push(trace_of(&codes));
        }
        let ticks = Some(first.levels.iter().enumerate().map(|(i, l)| ((i + 1) as f64, l.clone())).collect());
        let mut spec = PlotSpec {
            x_label: first.name.clone(),
            y_label: self.response_name.clone(),
            points,
            x_ticks: ticks,
            ..PlotSpec::default()
        };
        if others.is_empty() {
            spec.line = lines.into_iter().next();
            spec.line_name = Some("Means".to_owned());
        } else {
            let title = others.iter().map(|&k| self.factors[k].name.clone()).collect::<Vec<_>>().join(", ");
            spec.groups = Some(PointGroups { title, labels, membership, lines });
        }
        section(
            SectionKind::RepeatedProfile,
            format!("Profile Plot (points: measurements; lines join the cell means across {})", first.name),
            SectionContent::Plot(Box::new(spec)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    /// The O'Brien & Kaiser data stacked: 16 subjects × phase (3) × hour (5),
    /// between treatment (3) × gender (2) (see `stat_inference::repeated`).
    fn obrien_kaiser() -> DataTable {
        let raw: [(&str, &str, [f64; 15]); 16] = [
            ("control", "M", [1., 2., 4., 2., 1., 3., 2., 5., 3., 2., 2., 3., 2., 4., 4.]),
            ("control", "M", [4., 4., 5., 3., 4., 2., 2., 3., 5., 3., 4., 5., 6., 4., 1.]),
            ("control", "M", [5., 6., 5., 7., 7., 4., 5., 7., 5., 4., 7., 6., 9., 7., 6.]),
            ("control", "F", [5., 4., 7., 5., 4., 2., 2., 3., 5., 3., 4., 4., 5., 3., 4.]),
            ("control", "F", [3., 4., 6., 4., 3., 6., 7., 8., 6., 3., 4., 3., 6., 4., 3.]),
            ("A", "M", [7., 8., 7., 9., 9., 9., 9., 10., 8., 9., 9., 10., 11., 9., 6.]),
            ("A", "M", [5., 5., 6., 4., 5., 7., 7., 8., 10., 8., 8., 9., 11., 9., 8.]),
            ("A", "F", [2., 3., 5., 3., 2., 2., 4., 8., 6., 5., 6., 6., 7., 5., 6.]),
            ("A", "F", [3., 3., 4., 6., 4., 4., 5., 6., 4., 1., 5., 4., 7., 5., 4.]),
            ("B", "M", [4., 4., 5., 3., 4., 6., 7., 6., 8., 8., 8., 8., 9., 7., 8.]),
            ("B", "M", [3., 3., 4., 2., 3., 5., 4., 7., 5., 4., 5., 6., 8., 6., 5.]),
            ("B", "M", [6., 7., 8., 6., 3., 9., 10., 11., 9., 6., 8., 7., 10., 8., 7.]),
            ("B", "F", [5., 5., 6., 8., 6., 4., 6., 6., 8., 6., 7., 7., 8., 10., 8.]),
            ("B", "F", [2., 2., 3., 1., 2., 5., 6., 7., 5., 2., 6., 7., 8., 6., 3.]),
            ("B", "F", [2., 2., 3., 4., 4., 6., 6., 7., 9., 7., 7., 7., 8., 6., 7.]),
            ("B", "F", [4., 5., 7., 5., 4., 7., 7., 8., 6., 7., 7., 8., 10., 8., 7.]),
        ];
        let phases = ["1 pre", "2 post", "3 follow-up"];
        let (mut y, mut subj, mut treat, mut gender, mut phase, mut hour) =
            (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for (i, (t, g, v)) in raw.iter().enumerate() {
            for (c, &value) in v.iter().enumerate() {
                y.push(value);
                subj.push(format!("S{}", i + 1));
                treat.push((*t).to_owned());
                gender.push((*g).to_owned());
                phase.push(phases[c / 5].to_owned());
                hour.push(format!("{}", c % 5 + 1));
            }
        }
        DataTable::new(
            "ok",
            vec![
                Column::numeric("Score", y),
                Column::text("Subject", subj),
                Column::text("Treatment", treat),
                Column::text("Gender", gender),
                Column::text("Phase", phase),
                Column::text("Hour", hour),
            ],
        )
        .unwrap()
    }

    fn request() -> FactorialRepeatedRequest {
        FactorialRepeatedRequest { response: 0, subject: 1, within: vec![4, 5], between: vec![2, 3], simple: None }
    }

    /// Stacked data reproduce the engine's (R-verified) results; treatment
    /// levels sort A, B, control here, which does not change Type III tests.
    #[test]
    fn stacked_data_match_engine() {
        let t = obrien_kaiser();
        let r = FactorialRepeatedReport::run(&t, &RowStateManager::new(t.n_rows()), &request()).unwrap();
        assert_eq!(r.n_subjects(), 16);
        assert_eq!(r.title(), "Repeated Measures: Score by Phase × Hour (within), Treatment × Gender (between)");
        let e = r.analysis.effects.iter().find(|e| e.name == "Treatment × Phase").unwrap();
        assert!((e.ss - 77.885_239_254).abs() < 1e-8);
        assert!((e.p_value_hf - 8.438_775_501_929_79e-3).abs() < 1e-10);
        let s = r.sections();
        let kinds: Vec<_> = s.iter().map(|x| x.kind).collect();
        assert_eq!(
            kinds,
            [
                SectionKind::RepeatedDesign,
                SectionKind::RepeatedCellMeans,
                SectionKind::RepeatedTests,
                SectionKind::Sphericity,
                SectionKind::RepeatedProfile
            ]
        );
        let SectionContent::Table(cells) = &s[1].content else { panic!() };
        assert_eq!(cells.rows.len(), 3 * 5 * 3 * 2);
        let SectionContent::Table(tests) = &s[2].content else { panic!() };
        assert_eq!(tests.rows.len(), 15);
        assert!(s[2].title.contains("unequal size"));
        let SectionContent::Table(sph) = &s[3].content else { panic!() };
        assert_eq!(sph.rows.iter().map(|r| r[0].as_str()).collect::<Vec<_>>(), ["Phase", "Hour", "Phase × Hour"]);
        let SectionContent::Plot(p) = &s[4].content else { panic!() };
        assert_eq!(p.points.len(), 240);
        assert_eq!(p.groups.as_ref().unwrap().lines.len(), 5 * 3 * 2);
    }

    /// Simple effects of Phase at each level of Treatment: R
    /// `car::Anova` on each treatment's subjects (phase × hour within,
    /// gender between) gives the Phase rows (`tests/data/factorial_rm_reference.R`).
    #[test]
    fn simple_effects_by_between_factor() {
        let t = obrien_kaiser();
        let req = FactorialRepeatedRequest { simple: Some((4, 2)), ..request() };
        let r = FactorialRepeatedReport::run(&t, &RowStateManager::new(t.n_rows()), &req).unwrap();
        let s = r.sections();
        let simple = s.iter().find(|x| x.kind == SectionKind::SimpleEffects).unwrap();
        assert!(simple.title.starts_with("Simple Effects of Phase at Each Level of Treatment"));
        let SectionContent::Table(tab) = &simple.content else { panic!() };
        assert_eq!(tab.rows.iter().map(|r| r[0].as_str()).collect::<Vec<_>>(), ["A", "B", "control"]);
        for (row, (f, gg)) in tab.rows.iter().zip(SIMPLE) {
            let fv: f64 = row[3].parse().unwrap();
            assert!((fv - f).abs() < 5e-5 * f, "{} vs {f}", row[3]);
            assert_eq!(row[5], gg);
        }
        assert_eq!((tab.rows[0][1].as_str(), tab.rows[0][2].as_str()), ("2", "4"));
        assert_eq!(tab.rows[1][4], "6.3e-5", "R: p = 6.31e-5");
    }

    /// `(F, GG ε)` of Phase within treatments A, B and control (R car).
    const SIMPLE: [(f64, &str); 3] =
        [(5.727_272_727_272_726, "0.5261"), (29.589_127_686_472_8, "0.8"), (0.076_363_636_363_636, "0.5526")];

    #[test]
    fn invalid_requests_and_dropped_subjects() {
        let t = obrien_kaiser();
        let rows = RowStateManager::new(t.n_rows());
        assert!(
            FactorialRepeatedReport::run(&t, &rows, &FactorialRepeatedRequest { within: vec![], ..request() }).is_err()
        );
        assert!(
            FactorialRepeatedReport::run(&t, &rows, &FactorialRepeatedRequest { within: vec![4, 5, 3], ..request() })
                .is_err(),
            "five factors"
        );
        // Gender as a within factor: subjects have one gender only.
        let bad = FactorialRepeatedRequest { within: vec![4, 3], between: vec![2], ..request() };
        assert!(FactorialRepeatedReport::run(&t, &rows, &bad).is_err());
        // Treatment as a within factor would put a subject in two levels.
        let mut ex = RowStateManager::new(t.n_rows());
        ex.set_excluded(0..1, true);
        let r = FactorialRepeatedReport::run(&t, &ex, &request()).unwrap();
        assert_eq!(r.n_subjects(), 15, "the subject missing a cell is left out");
        assert!(r.sections()[0].title.contains("1 subjects missing a within-subject cell left out"));
    }
}
