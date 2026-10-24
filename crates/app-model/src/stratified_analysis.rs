//! The stratified contingency-table report (Compare ▸ Stratified tables and
//! exact tests…): a row × column table of counts in each stratum (the
//! combinations of one or more stratum columns), tested per stratum
//! (Pearson χ² and Fisher's exact test for r × c tables) and across strata
//! (generalized Cochran–Mantel–Haenszel statistics; for 2 × 2 tables the
//! Mantel–Haenszel common odds ratio and the Breslow–Day test of
//! homogeneity). Without a stratum column it is a single table with its
//! exact test. Engines: [`stat_discrete::stratified`],
//! [`stat_discrete::exact_rxc`].
//!
//! The report runs through [`crate::ComparisonReport`], so front ends edit
//! its settings in place.

use std::collections::BTreeMap;

use crate::format::{format_number, format_p_value};
use crate::project::{invalid, member, obj, s};
use crate::report::{ReportSection, SectionContent, SectionKind, TextTable};
use stat_core::Result;
use stat_data::{DataTable, JsonValue, RowStateManager};
use stat_discrete::exact_rxc::{ExactMethod, ExactOptions, ExactTest, fisher_exact_rxc};
use stat_discrete::stratified::{ChiSquareTest, GeneralizedCmh, MantelHaenszel, generalized_cmh, mantel_haenszel};
use stat_discrete::{ContingencyTable, test_independence};

/// Significant digits in report tables.
const DIGITS: usize = 5;

/// Settings of a stratified contingency analysis.
#[derive(Debug, Clone, PartialEq)]
pub struct StratifiedRequest {
    /// Row column (for 2 × 2 tables, the exposure or treatment).
    pub row: usize,
    /// Column column (the response).
    pub column: usize,
    /// Stratum columns (their level combinations are the strata; empty: one table).
    pub strata: Vec<usize>,
    /// Frequency (count) column: each row stands for this many observations.
    pub frequency: Option<usize>,
    /// Confidence level of the common odds ratio's interval.
    pub confidence: f64,
    /// Run Fisher's exact test in each stratum and on the pooled table.
    pub exact: bool,
    /// Random tables of the Monte Carlo estimate used when a table is too
    /// large for the exact computation.
    pub monte_carlo_tables: usize,
    /// Seed of the Monte Carlo estimate.
    pub seed: u64,
    /// Adjust the per-stratum p-values across the strata for the false
    /// discovery rate (`None`: off).
    pub fdr: Option<crate::FdrMethod>,
}

impl StratifiedRequest {
    /// Defaults: 95% interval, exact tests on, 10 000 Monte Carlo tables.
    #[must_use]
    pub fn new(row: usize, column: usize, strata: Vec<usize>) -> Self {
        Self {
            row,
            column,
            strata,
            frequency: None,
            confidence: 0.95,
            exact: true,
            monte_carlo_tables: 10_000,
            seed: 1,
            fdr: None,
        }
    }

    /// Every column used.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        [self.row, self.column].into_iter().chain(self.strata.iter().copied()).chain(self.frequency).collect()
    }

    /// The same request with columns renumbered by `f`.
    #[must_use]
    pub fn map_columns(mut self, f: impl Fn(usize) -> usize) -> Self {
        self.row = f(self.row);
        self.column = f(self.column);
        self.strata.iter_mut().for_each(|c| *c = f(*c));
        self.frequency = self.frequency.map(&f);
        self
    }

    /// The project-file form (`"analysis": "stratified-tables"`).
    #[must_use]
    pub fn to_json(&self, table: &DataTable) -> JsonValue {
        let name = |i: usize| s(table.column(i).map_or("", |c| c.name.as_str()));
        obj([
            ("analysis", s("stratified-tables")),
            ("row", name(self.row)),
            ("column", name(self.column)),
            ("strata", JsonValue::Array(self.strata.iter().map(|&i| name(i)).collect())),
            ("frequency", self.frequency.map_or(JsonValue::Null, name)),
            ("confidence", JsonValue::number(self.confidence)),
            ("exact", JsonValue::Bool(self.exact)),
            ("monte_carlo_tables", JsonValue::number(self.monte_carlo_tables as f64)),
            ("seed", JsonValue::number(self.seed as f64)),
            ("fdr", self.fdr.map_or(JsonValue::Null, |m| s(m.key()))),
        ])
    }

    /// Read the project-file form.
    ///
    /// # Errors
    /// Missing or unknown columns.
    pub fn from_json(v: &JsonValue, table: &DataTable) -> Result<Self> {
        let find = |n: &JsonValue, key: &str| -> Result<usize> {
            let name = n.as_str().ok_or_else(|| invalid(format!("{key}: expected a column name")))?;
            table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")))
        };
        let strata = match v.get("strata") {
            None | Some(JsonValue::Null) => Vec::new(),
            Some(list) => list
                .as_array()
                .ok_or_else(|| invalid("strata: expected a list of column names"))?
                .iter()
                .map(|n| find(n, "strata"))
                .collect::<Result<_>>()?,
        };
        let frequency = match v.get("frequency") {
            None | Some(JsonValue::Null) => None,
            Some(n) => Some(find(n, "frequency")?),
        };
        Ok(Self {
            row: find(member(v, "row")?, "row")?,
            column: find(member(v, "column")?, "column")?,
            strata,
            frequency,
            confidence: v.get("confidence").and_then(JsonValue::as_f64).unwrap_or(0.95),
            exact: v.get("exact").and_then(JsonValue::as_bool).unwrap_or(true),
            monte_carlo_tables: v.get("monte_carlo_tables").and_then(JsonValue::as_usize).unwrap_or(10_000),
            seed: v.get("seed").and_then(crate::seed::seed_from_json).unwrap_or(1),
            fdr: match v.get("fdr").and_then(JsonValue::as_str) {
                None => None,
                Some(key) => Some(
                    crate::FdrMethod::from_key(key)
                        .ok_or_else(|| invalid(format!("unknown false-discovery-rate method {key:?}")))?,
                ),
            },
        })
    }
}

/// One stratum's table and tests.
#[derive(Debug, Clone)]
struct Stratum {
    label: String,
    counts: Vec<Vec<u64>>,
    pearson: Option<ChiSquareTest>,
    /// Fisher's exact test when asked for: the test, or why this table
    /// cannot have one.
    exact: Option<std::result::Result<ExactTest, String>>,
}

/// A computed stratified analysis.
#[derive(Debug, Clone)]
pub struct StratifiedAnalysis {
    request: StratifiedRequest,
    row_name: String,
    column_name: String,
    stratum_names: Vec<String>,
    row_levels: Vec<String>,
    column_levels: Vec<String>,
    row_scores: Vec<f64>,
    column_scores: Vec<f64>,
    strata: Vec<Stratum>,
    pooled: Stratum,
    cmh: Option<GeneralizedCmh>,
    mantel_haenszel: Option<MantelHaenszel>,
    skipped: usize,
    excluded: usize,
}

/// Scores of ordered levels: their values when every label is a number,
/// otherwise 1, 2, ….
fn scores(levels: &[String]) -> Vec<f64> {
    let parsed: Option<Vec<f64>> =
        levels.iter().map(|l| l.trim().parse::<f64>().ok().filter(|v| v.is_finite())).collect();
    parsed.unwrap_or_else(|| (1..=levels.len()).map(|i| i as f64).collect())
}

fn tests_of(label: String, counts: Vec<Vec<u64>>, exact: Option<&ExactOptions>) -> Stratum {
    let t = ContingencyTable {
        row_labels: (0..counts.len()).map(|i| i.to_string()).collect(),
        column_labels: (0..counts[0].len()).map(|j| j.to_string()).collect(),
        counts: counts.clone(),
    };
    let pearson = test_independence(&t).ok().map(|r| ChiSquareTest {
        statistic: r.pearson_chi_square,
        df: r.df,
        p_value: r.pearson_p_value,
    });
    // A table the exact test cannot handle (a total beyond 2⁵³) says so in
    // its own row instead of failing the whole report.
    let exact = exact.map(|o| fisher_exact_rxc(&counts, o).map_err(|e| e.to_string()));
    Stratum { label, counts, pearson, exact }
}

impl StratifiedAnalysis {
    /// Count and test the non-excluded rows that have a row, column and
    /// stratum label (and a frequency, when one is used).
    ///
    /// # Errors
    /// A column in two roles, a frequency column that is not numeric or has
    /// negative or fractional counts, fewer than two row or column levels,
    /// or a confidence level outside (0, 1).
    pub fn run(table: &DataTable, states: &RowStateManager, request: &StratifiedRequest) -> Result<Self> {
        let used = request.columns();
        if (1..used.len()).any(|i| used[..i].contains(&used[i])) {
            return Err(invalid("the row, column, stratum and frequency columns must all be different"));
        }
        if !(request.confidence > 0.0 && request.confidence < 1.0) {
            return Err(invalid("the confidence level must be between 0 and 1"));
        }
        let col = |i: usize| table.column(i).ok_or_else(|| invalid(format!("no column {i}")));
        let (row_col, column_col) = (col(request.row)?, col(request.column)?);
        let stratum_cols = request.strata.iter().map(|&i| col(i)).collect::<Result<Vec<_>>>()?;
        let freq = match request.frequency {
            Some(i) => {
                let c = col(i)?;
                Some(c.as_numeric().ok_or_else(|| invalid(format!("{} must be a numeric column", c.name)))?)
            }
            None => None,
        };
        // The rows counted: included, with every label and a positive
        // frequency. Levels come from these rows only, so a label found only
        // in excluded or incomplete rows adds no empty row, column or stratum.
        let (mut skipped, mut excluded) = (0, 0);
        let mut counted: Vec<(usize, u64)> = Vec::new();
        for k in 0..table.n_rows() {
            if states.is_excluded(k) {
                excluded += 1;
                continue;
            }
            let w = match freq {
                Some(f) => {
                    let w = f[k];
                    if w.is_nan() {
                        skipped += 1;
                        continue;
                    }
                    if w < 0.0 || (w - w.round()).abs() > 1e-9 {
                        return Err(invalid("frequencies must be whole numbers of zero or more"));
                    }
                    // Keeps every total far from overflowing.
                    if w > 1e12 {
                        return Err(invalid("a frequency is too large (above 10¹²)"));
                    }
                    w.round() as u64
                }
                None => 1,
            };
            let labelled = std::iter::once(row_col)
                .chain(std::iter::once(column_col))
                .chain(stratum_cols.iter().copied())
                .all(|c| !c.level_text(k).is_empty());
            if !labelled {
                skipped += 1;
            } else if w > 0 {
                counted.push((k, w));
            }
        }
        let levels_of = |c: &stat_data::Column| {
            let mut levels: Vec<String> = Vec::new();
            for &(k, _) in &counted {
                let t = c.level_text(k);
                if !levels.contains(&t) {
                    levels.push(t);
                }
            }
            levels.sort_by(|a, b| stat_linear::natural_order(a, b));
            levels
        };
        let row_levels = levels_of(row_col);
        let column_levels_ = levels_of(column_col);
        let stratum_levels: Vec<Vec<String>> = stratum_cols.iter().map(|c| levels_of(c)).collect();
        let (r, c) = (row_levels.len(), column_levels_.len());
        if counted.is_empty() {
            return Err(invalid("no row has a row, column and stratum label"));
        }
        if r < 2 || c < 2 {
            return Err(invalid("the row and column variables each need at least two levels among the rows used"));
        }
        let mut by_stratum: BTreeMap<Vec<usize>, Vec<Vec<u64>>> = BTreeMap::new();
        let pos = |levels: &[String], text: String| levels.iter().position(|l| *l == text).expect("level listed");
        for &(k, w) in &counted {
            let i = pos(&row_levels, row_col.level_text(k));
            let j = pos(&column_levels_, column_col.level_text(k));
            let key: Vec<usize> =
                stratum_cols.iter().zip(&stratum_levels).map(|(sc, l)| pos(l, sc.level_text(k))).collect();
            by_stratum.entry(key).or_insert_with(|| vec![vec![0; c]; r])[i][j] += w;
        }
        let options = ExactOptions {
            monte_carlo_tables: request.monte_carlo_tables.max(100),
            seed: request.seed,
            ..ExactOptions::default()
        };
        let exact = request.exact.then_some(&options);
        let mut pooled_counts = vec![vec![0u64; c]; r];
        let mut strata = Vec::new();
        for (key, counts) in &by_stratum {
            for i in 0..r {
                for j in 0..c {
                    pooled_counts[i][j] += counts[i][j];
                }
            }
            if !request.strata.is_empty() {
                let label =
                    key.iter().zip(&stratum_levels).map(|(&k, l)| l[k].as_str()).collect::<Vec<_>>().join(" / ");
                strata.push(tests_of(label, counts.clone(), exact));
            }
        }
        let pooled_label = if request.strata.is_empty() { "All rows".to_owned() } else { "Strata pooled".to_owned() };
        let pooled = tests_of(pooled_label, pooled_counts, exact);
        let row_scores = scores(&row_levels);
        let column_scores = scores(&column_levels_);
        let (cmh, mh) = if request.strata.is_empty() {
            (None, None)
        } else {
            let tables: Vec<Vec<Vec<u64>>> = strata.iter().map(|s| s.counts.clone()).collect();
            let cmh = generalized_cmh(&tables, &row_scores, &column_scores).ok();
            let mh = if r == 2 && c == 2 {
                let two: Vec<[[u64; 2]; 2]> = tables.iter().map(|t| [[t[0][0], t[0][1]], [t[1][0], t[1][1]]]).collect();
                mantel_haenszel(&two, request.confidence).ok()
            } else {
                None
            };
            (cmh, mh)
        };
        let name = |i: usize| table.column(i).map_or(String::new(), |c| c.name.clone());
        Ok(Self {
            request: request.clone(),
            row_name: name(request.row),
            column_name: name(request.column),
            stratum_names: request.strata.iter().map(|&i| name(i)).collect(),
            row_levels,
            column_levels: column_levels_,
            row_scores,
            column_scores,
            strata,
            pooled,
            cmh,
            mantel_haenszel: mh,
            skipped,
            excluded,
        })
    }

    /// The request this analysis answers.
    #[must_use]
    pub fn request(&self) -> &StratifiedRequest {
        &self.request
    }

    /// Generalized CMH statistics (`None` without strata or when they
    /// cannot be computed).
    #[must_use]
    pub fn cmh(&self) -> Option<&GeneralizedCmh> {
        self.cmh.as_ref()
    }

    /// Mantel–Haenszel analysis (2 × 2 tables with strata only).
    #[must_use]
    pub fn mantel_haenszel(&self) -> Option<&MantelHaenszel> {
        self.mantel_haenszel.as_ref()
    }

    /// Fisher's exact test of the table pooled over strata.
    #[must_use]
    pub fn pooled_exact(&self) -> Option<&ExactTest> {
        self.pooled.exact.as_ref().and_then(|e| e.as_ref().ok())
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        if self.stratum_names.is_empty() {
            format!("Contingency Table and Exact Test: {} by {}", self.column_name, self.row_name)
        } else {
            format!(
                "Stratified Analysis: {} by {}, Strata {}",
                self.column_name,
                self.row_name,
                self.stratum_names.join(" / ")
            )
        }
    }

    fn counts_section(&self) -> ReportSection {
        let stratified = !self.stratum_names.is_empty();
        let mut headings: Vec<String> = Vec::new();
        if stratified {
            headings.push(self.stratum_names.join(" / "));
        }
        headings.push(self.row_name.clone());
        headings.extend(self.column_levels.iter().cloned());
        headings.push("Total".into());
        let mut t = TextTable::new(headings);
        let list: Vec<&Stratum> = if stratified { self.strata.iter().collect() } else { vec![&self.pooled] };
        for s in list {
            let c = self.column_levels.len();
            let mut col_tot = vec![0u64; c];
            for (i, row) in s.counts.iter().enumerate() {
                let mut cells: Vec<String> = Vec::new();
                if stratified {
                    cells.push(if i == 0 { s.label.clone() } else { String::new() });
                }
                cells.push(self.row_levels[i].clone());
                cells.extend(row.iter().map(u64::to_string));
                cells.push(row.iter().sum::<u64>().to_string());
                t.push(cells);
                for j in 0..c {
                    col_tot[j] += row[j];
                }
            }
            let mut cells: Vec<String> = Vec::new();
            if stratified {
                cells.push(String::new());
            }
            cells.push("Total".into());
            cells.extend(col_tot.iter().map(u64::to_string));
            cells.push(col_tot.iter().sum::<u64>().to_string());
            t.push(cells);
        }
        ReportSection {
            kind: SectionKind::StratifiedCounts,
            title: if stratified { "Counts by Stratum".into() } else { "Counts".into() },
            content: SectionContent::Table(t),
        }
    }

    fn two_by_two(&self) -> bool {
        self.row_levels.len() == 2 && self.column_levels.len() == 2
    }

    fn tests_section(&self) -> ReportSection {
        let stratified = !self.stratum_names.is_empty();
        let mut headings: Vec<String> = vec![if stratified { "Stratum".into() } else { "Table".into() }, "N".into()];
        if self.two_by_two() {
            headings.push("Odds ratio".into());
        }
        headings.extend(["Pearson χ²".into(), "DF".into(), "p-value".into()]);
        if self.request.exact {
            headings.extend(["Fisher exact p".into(), "Exact method".into()]);
        }
        let mut t = TextTable::new(headings);
        let num = |v: f64| {
            if v.is_finite() {
                format_number(v, DIGITS)
            } else if v.is_nan() {
                "—".into()
            } else {
                "∞".into()
            }
        };
        for s in self.strata.iter().chain(std::iter::once(&self.pooled)) {
            let n: u64 = s.counts.iter().flatten().sum();
            let mut cells = vec![s.label.clone(), n.to_string()];
            if self.two_by_two() {
                let [[a, b], [c, d]] =
                    [[s.counts[0][0], s.counts[0][1]], [s.counts[1][0], s.counts[1][1]]].map(|r| r.map(|x| x as f64));
                cells.push(if a * d == 0.0 && b * c == 0.0 { "—".into() } else { num(a * d / (b * c)) });
            }
            match &s.pearson {
                Some(p) => {
                    cells.extend([format_number(p.statistic, DIGITS), p.df.to_string(), format_p_value(p.p_value)])
                }
                None => cells.extend(["—".to_owned(), "—".to_owned(), "—".to_owned()]),
            }
            if self.request.exact {
                match &s.exact {
                    Some(Ok(e)) => cells.extend([format_p_value(e.p_value), method_text(e)]),
                    Some(Err(_)) => cells.extend(["—".to_owned(), "exact test not available".to_owned()]),
                    None => cells.extend(["—".to_owned(), String::new()]),
                }
            }
            t.push(cells);
        }
        ReportSection {
            kind: SectionKind::StratifiedTests,
            title: if stratified { "Tests by Stratum".into() } else { "Tests of Independence".into() },
            content: SectionContent::Table(t),
        }
    }

    /// Each stratum's Pearson (and exact) p-value with its false-discovery-
    /// rate adjustment across the strata.
    fn fdr_section(&self) -> Option<ReportSection> {
        let method = self.request.fdr?;
        if self.strata.is_empty() {
            return None;
        }
        let pearson: Vec<f64> =
            self.strata.iter().map(|s| s.pearson.as_ref().map_or(f64::NAN, |p| p.p_value)).collect();
        let exact: Vec<f64> = self
            .strata
            .iter()
            .map(|s| match &s.exact {
                Some(Ok(e)) => e.p_value,
                _ => f64::NAN,
            })
            .collect();
        let (qp, pi_p) = method.adjust(&pearson).ok()?;
        let mut head: Vec<String> = ["Stratum", "Pearson p", "Pearson q"].map(String::from).to_vec();
        let with_exact = self.request.exact;
        let (qe, pi_e) = if with_exact { method.adjust(&exact).ok()? } else { (Vec::new(), None) };
        if with_exact {
            head.extend(["Fisher exact p".to_owned(), "Fisher exact q".to_owned()]);
        }
        let mut t = TextTable::new(head);
        for (i, s) in self.strata.iter().enumerate() {
            let mut row = vec![s.label.clone(), format_p_value(pearson[i]), format_p_value(qp[i])];
            if with_exact {
                row.extend([format_p_value(exact[i]), format_p_value(qe[i])]);
            }
            t.push(row);
        }
        let found = |q: &[f64]| q.iter().filter(|&&v| v <= 0.05).count();
        let mut note = format!("{} of {} strata with Pearson q ≤ 0.05", found(&qp), self.strata.len());
        if with_exact {
            note.push_str(&format!("; {} with exact q ≤ 0.05", found(&qe)));
        }
        for (name, pi) in [("Pearson", pi_p), ("exact", pi_e)] {
            if let Some(pi) = pi {
                note.push_str(&format!("; π̂₀ ({name}) = {}", format_number(pi, 4)));
            }
        }
        Some(ReportSection {
            kind: SectionKind::AdjustedTableTests,
            title: format!("Tests by Stratum Adjusted for the False Discovery Rate ({}; {note})", method.label()),
            content: SectionContent::Table(t),
        })
    }

    fn cmh_section(&self) -> Option<ReportSection> {
        let g = self.cmh.as_ref()?;
        let mut t = TextTable::new(["Statistic", "Alternative hypothesis", "Value", "DF", "p-value"]);
        let mut push = |name: &str, alt: &str, test: Option<ChiSquareTest>| match test {
            Some(x) => t.push([
                name.to_owned(),
                alt.to_owned(),
                format_number(x.statistic, DIGITS),
                x.df.to_string(),
                format_p_value(x.p_value),
            ]),
            None => t.push([name.to_owned(), alt.to_owned(), "—".into(), "—".into(), "singular".into()]),
        };
        if let Some(mh) = &self.mantel_haenszel {
            push("Mantel–Haenszel, continuity corrected", "Common odds ratio ≠ 1", Some(mh.cmh_corrected));
        }
        push("Nonzero correlation", "Linear association of row and column scores", g.correlation);
        push("Row mean scores differ", "Mean column score differs between rows", g.row_mean_scores);
        push("General association", "Rows and columns associated in some stratum", g.general_association);
        Some(ReportSection {
            kind: SectionKind::CmhTests,
            title: "Cochran–Mantel–Haenszel Tests".into(),
            content: SectionContent::Table(t),
        })
    }

    fn odds_ratio_sections(&self) -> Vec<ReportSection> {
        let Some(mh) = &self.mantel_haenszel else { return Vec::new() };
        let level = format!("{}%", format_number(100.0 * mh.confidence, 4));
        let num = |v: f64| {
            if v.is_finite() {
                format_number(v, DIGITS)
            } else if v.is_nan() {
                "—".into()
            } else {
                "∞".into()
            }
        };
        let mut or = TextTable::new([
            "Estimate".to_owned(),
            "Odds ratio".to_owned(),
            format!("Lower {level}"),
            format!("Upper {level}"),
            "SE of ln(odds ratio)".to_owned(),
        ]);
        let (lo, hi) = mh.interval.map_or(("—".into(), "—".into()), |(l, h)| (num(l), num(h)));
        or.push([
            "Mantel–Haenszel common odds ratio".to_owned(),
            num(mh.odds_ratio),
            lo,
            hi,
            mh.log_odds_ratio_se.map_or("—".into(), |s| format_number(s, DIGITS)),
        ]);
        or.push(["Crude (strata pooled)".to_owned(), num(mh.crude_odds_ratio), "—".into(), "—".into(), "—".into()]);
        let mut homog = TextTable::new(["Test", "χ²", "DF", "p-value"]);
        for (name, test) in
            [("Breslow–Day", mh.breslow_day), ("Breslow–Day with Tarone's correction", mh.breslow_day_tarone)]
        {
            match test {
                Some(x) => homog.push([
                    name.to_owned(),
                    format_number(x.statistic, DIGITS),
                    x.df.to_string(),
                    format_p_value(x.p_value),
                ]),
                None => homog.push([name.to_owned(), "—".into(), "—".into(), "—".into()]),
            }
        }
        vec![
            ReportSection {
                kind: SectionKind::CommonOddsRatio,
                title: "Common Odds Ratio".into(),
                content: SectionContent::Table(or),
            },
            ReportSection {
                kind: SectionKind::OddsRatioHomogeneity,
                title: "Homogeneity of Odds Ratios".into(),
                content: SectionContent::Table(homog),
            },
        ]
    }

    fn note(&self) -> ReportSection {
        let mut note = String::new();
        if self.two_by_two() {
            note.push_str(&format!(
                "Odds ratios compare the odds of {col} = {c0} for {row} = {r0} with those for {row} = {r1}. ",
                col = self.column_name,
                c0 = self.column_levels[0],
                row = self.row_name,
                r0 = self.row_levels[0],
                r1 = self.row_levels[1]
            ));
            if self.mantel_haenszel.is_some() {
                note.push_str(
                    "The interval is Robins, Breslow & Greenland's; Breslow–Day leaves out strata with an empty row \
                     or column. ",
                );
            }
        }
        if self.cmh.is_some() {
            let show = |levels: &[String], s: &[f64]| {
                levels
                    .iter()
                    .zip(s)
                    .map(|(l, v)| format!("{l} = {}", format_number(*v, 6)))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            note.push_str(&format!(
                "CMH scores: {} ({}); {} ({}). Strata with fewer than two observations add nothing. ",
                self.row_name,
                show(&self.row_levels, &self.row_scores),
                self.column_name,
                show(&self.column_levels, &self.column_scores)
            ));
        }
        if self.request.exact {
            note.push_str(
                "Fisher's exact p is the probability, given the table's margins, of a table no more probable than \
                 the observed one (network algorithm).",
            );
            let mc = self
                .strata
                .iter()
                .chain(std::iter::once(&self.pooled))
                .any(|s| matches!(&s.exact, Some(Ok(e)) if e.is_estimate()));
            if mc {
                note.push_str(&format!(
                    " ⚠ Tables marked Monte Carlo were too large for the exact computation: their p-values are \
                     estimates from {} random tables with the observed margins (seed {}).",
                    self.request.monte_carlo_tables.max(100),
                    self.request.seed
                ));
            }
            for s in self.strata.iter().chain(std::iter::once(&self.pooled)) {
                if let Some(Err(reason)) = &s.exact {
                    note.push_str(&format!(" ⚠ {}: exact test not available ({reason}).", s.label));
                }
            }
            note.push(' ');
        }
        if self.skipped > 0 {
            note.push_str(&format!("{} rows with a missing label or frequency were left out. ", self.skipped));
        }
        if self.excluded > 0 {
            note.push_str(&format!("{} excluded rows not used.", self.excluded));
        }
        ReportSection {
            kind: SectionKind::StratifiedNote,
            title: "About These Tests".into(),
            content: SectionContent::Note(note.trim_end().to_owned()),
        }
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let mut out = vec![self.counts_section(), self.tests_section()];
        out.extend(self.fdr_section());
        out.extend(self.cmh_section());
        out.extend(self.odds_ratio_sections());
        out.push(self.note());
        out
    }
}

fn method_text(e: &ExactTest) -> String {
    match e.method {
        ExactMethod::Network => "exact".into(),
        ExactMethod::MonteCarlo { standard_error, .. } => {
            format!("Monte Carlo (± {})", format_number(standard_error, 2))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    /// The penicillin-and-rabbits strata (see stat-discrete's reference
    /// tests) as one row per rabbit.
    fn rabbits() -> DataTable {
        let strata = [[[0, 6], [0, 5]], [[3, 3], [0, 6]], [[6, 0], [2, 4]], [[5, 1], [6, 0]], [[2, 0], [5, 0]]];
        let (mut level, mut delay, mut response) = (Vec::new(), Vec::new(), Vec::new());
        for (k, t) in strata.iter().enumerate() {
            for (i, row) in t.iter().enumerate() {
                for (j, &n) in row.iter().enumerate() {
                    for _ in 0..n {
                        level.push(format!("L{k}"));
                        delay.push(["Immediate", "Delayed"][i].to_owned());
                        response.push(["Cured", "Died"][j].to_owned());
                    }
                }
            }
        }
        DataTable::new(
            "rabbits",
            vec![Column::text("Level", level), Column::text("Delay", delay), Column::text("Response", response)],
        )
        .unwrap()
    }

    /// Per-stratum p-values adjusted across strata; R 4.3.3: `fisher.test`,
    /// `chisq.test(correct = FALSE)` and `p.adjust` (BY for Pearson over the
    /// three strata where χ² is defined, BH for the exact tests).
    #[test]
    #[allow(clippy::excessive_precision)]
    fn strata_adjusted_for_the_false_discovery_rate() {
        let t = rabbits();
        let rows = RowStateManager::new(t.n_rows());
        let req = StratifiedRequest {
            fdr: Some(crate::FdrMethod::BenjaminiYekutieli),
            ..StratifiedRequest::new(1, 2, vec![0])
        };
        let s = StratifiedAnalysis::run(&t, &rows, &req).unwrap().sections();
        let fdr = s.iter().find(|x| x.kind == SectionKind::AdjustedTableTests).unwrap();
        let SectionContent::Table(tab) = &fdr.content else { panic!() };
        let pearson_q =
            [f64::NAN, 0.125_125_725_714_985_81, 0.078_682_331_394_863_053, 0.543_161_431_054_525_08, f64::NAN];
        for (row, q) in tab.rows.iter().zip(pearson_q) {
            assert_eq!(row[2], format_p_value(q), "{row:?}");
        }
        let bh = StratifiedRequest { fdr: Some(crate::FdrMethod::BenjaminiHochberg), ..req.clone() };
        let s = StratifiedAnalysis::run(&t, &rows, &bh).unwrap().sections();
        let SectionContent::Table(tab) = &s.iter().find(|x| x.kind == SectionKind::AdjustedTableTests).unwrap().content
        else {
            panic!()
        };
        let exact_q = [1.0, 0.454_545_454_545_454_92, 0.303_030_303_030_303_37, 1.0, 1.0];
        for (row, q) in tab.rows.iter().zip(exact_q) {
            assert_eq!(row[4], format_p_value(q), "{row:?}");
        }
        // Off by default, and never for a single table.
        let plain = StratifiedAnalysis::run(&t, &rows, &StratifiedRequest::new(1, 2, vec![0])).unwrap();
        assert!(!plain.sections().iter().any(|x| x.kind == SectionKind::AdjustedTableTests));
        let single = StratifiedRequest { strata: Vec::new(), ..bh };
        let one = StratifiedAnalysis::run(&t, &rows, &single).unwrap();
        assert!(!one.sections().iter().any(|x| x.kind == SectionKind::AdjustedTableTests));
    }

    #[test]
    fn rabbits_report_matches_the_engine_reference() {
        let t = rabbits();
        // Natural order puts "Delayed" before "Immediate": the odds ratio is inverted.
        let req = StratifiedRequest::new(1, 2, vec![0]);
        let a = StratifiedAnalysis::run(&t, &RowStateManager::new(t.n_rows()), &req).unwrap();
        let mh = a.mantel_haenszel().unwrap();
        assert!((mh.odds_ratio - 1.0 / 7.0).abs() < 1e-12);
        assert!((mh.cmh_corrected.statistic - 3.928_571_428_571_429).abs() < 1e-10);
        assert!((a.cmh().unwrap().general_association.unwrap().statistic - 5.657_142_857_142_857_5).abs() < 1e-9);
        let s = a.sections();
        let kinds: Vec<SectionKind> = s.iter().map(|x| x.kind).collect();
        assert_eq!(
            kinds,
            [
                SectionKind::StratifiedCounts,
                SectionKind::StratifiedTests,
                SectionKind::CmhTests,
                SectionKind::CommonOddsRatio,
                SectionKind::OddsRatioHomogeneity,
                SectionKind::StratifiedNote
            ]
        );
        let SectionContent::Table(tests) = &s[1].content else { panic!() };
        assert_eq!(tests.rows.len(), 6, "five strata and the pooled table");
        assert_eq!(tests.rows[5][0], "Strata pooled");
        assert_eq!(a.title(), "Stratified Analysis: Response by Delay, Strata Level");
        let back = StratifiedRequest::from_json(&req.to_json(&t), &t).unwrap();
        assert_eq!(back, req);
    }

    /// A label found only in excluded or incomplete rows (or rows of zero
    /// frequency) is not a level: the analysis is the same as without them.
    #[test]
    fn levels_come_from_the_rows_used() {
        let base = rabbits();
        let reference = StratifiedAnalysis::run(
            &base,
            &RowStateManager::new(base.n_rows()),
            &StratifiedRequest::new(1, 2, vec![0]),
        )
        .unwrap();
        let mut cols: Vec<stat_data::Column> = base.columns().to_vec();
        let push = |cols: &mut Vec<stat_data::Column>, cells: [&str; 3]| {
            for (c, v) in cols.iter_mut().zip(cells) {
                let stat_data::ColumnData::Text(t) = &mut c.data else { panic!() };
                t.push(v.to_owned());
            }
        };
        push(&mut cols, ["L0", "Unknown", "Cured"]); // excluded below
        push(&mut cols, ["L9", "", "Died"]); // incomplete
        push(&mut cols, ["L1", "Delayed", "Lost"]); // excluded below
        let n = base.n_rows();
        let t = DataTable::new("rabbits", cols).unwrap();
        let mut rows = RowStateManager::new(t.n_rows());
        rows.set_excluded([n, n + 2], true);
        let a = StratifiedAnalysis::run(&t, &rows, &StratifiedRequest::new(1, 2, vec![0])).unwrap();
        let (mh, mh0) = (a.mantel_haenszel().expect("still a 2 × 2 analysis"), reference.mantel_haenszel().unwrap());
        assert!((mh.odds_ratio - mh0.odds_ratio).abs() < 1e-12);
        let (g, g0) = (a.cmh().unwrap(), reference.cmh().unwrap());
        assert_eq!(g.general_association.unwrap().statistic, g0.general_association.unwrap().statistic);
        assert_eq!(a.strata.len(), 5, "no stratum from the incomplete row");

        // Zero frequencies count nothing.
        let job = DataTable::new(
            "f",
            vec![
                Column::text("r", ["a", "a", "b", "b", "c"].map(String::from).to_vec()),
                Column::text("c", ["x", "y", "x", "y", "x"].map(String::from).to_vec()),
                Column::numeric("n", vec![3.0, 5.0, 4.0, 1.0, 0.0]),
            ],
        )
        .unwrap();
        let mut req = StratifiedRequest::new(0, 1, vec![]);
        req.frequency = Some(2);
        let a = StratifiedAnalysis::run(&job, &RowStateManager::new(5), &req).unwrap();
        assert_eq!(a.row_levels, ["a", "b"]);
        assert!(a.pooled.pearson.is_some());
    }

    #[test]
    fn single_table_with_frequency_column() {
        // R's fisher.test job satisfaction example as a frequency table.
        let job = [[1, 2, 1, 0], [3, 3, 6, 1], [10, 10, 14, 9], [6, 7, 12, 11]];
        let (mut inc, mut sat, mut n) = (Vec::new(), Vec::new(), Vec::new());
        for (i, row) in job.iter().enumerate() {
            for (j, &x) in row.iter().enumerate() {
                inc.push(format!("{}", i + 1));
                sat.push(format!("S{}", j + 1));
                n.push(f64::from(x));
            }
        }
        let t = DataTable::new(
            "job",
            vec![Column::text("Income", inc), Column::text("Satisfaction", sat), Column::numeric("n", n)],
        )
        .unwrap();
        let mut req = StratifiedRequest::new(0, 1, vec![]);
        req.frequency = Some(2);
        let a = StratifiedAnalysis::run(&t, &RowStateManager::new(16), &req).unwrap();
        assert!((a.pooled_exact().unwrap().p_value - 0.7827).abs() < 5e-5);
        assert!(a.cmh().is_none() && a.mantel_haenszel().is_none());
        assert_eq!(a.sections().len(), 3);
        assert!(StratifiedAnalysis::run(&t, &RowStateManager::new(16), &StratifiedRequest::new(0, 0, vec![])).is_err());
    }

    #[test]
    fn stratum_too_large_for_the_exact_test_is_reported_not_fatal() {
        // Stratum "big" totals 9 100 rows × 10¹² = 9.1·10¹⁵ > 2⁵³; stratum
        // "huge" totals 4·10¹⁰ (beyond u32, which used to fail) and gets a
        // Monte Carlo test; "small" is exact.
        let (mut s, mut r, mut c, mut n) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let mut push = |stratum: &str, i: usize, j: usize, w: f64| {
            s.push(stratum.to_owned());
            r.push(["a", "b"][i].to_owned());
            c.push(["x", "y"][j].to_owned());
            n.push(w);
        };
        for (i, j, w) in [(0, 0, 3.0), (0, 1, 1.0), (1, 0, 1.0), (1, 1, 4.0)] {
            push("small", i, j, w);
        }
        for (i, j, w) in [(0, 0, 1.2e10), (0, 1, 0.8e10), (1, 0, 0.9e10), (1, 1, 1.1e10)] {
            push("huge", i, j, w);
        }
        for k in 0..9100 {
            push("big", k % 2, (k / 2) % 2, 1e12);
        }
        let rows = n.len();
        let t = DataTable::new(
            "big",
            vec![Column::text("S", s), Column::text("R", r), Column::text("C", c), Column::numeric("n", n)],
        )
        .unwrap();
        let mut req = StratifiedRequest::new(1, 2, vec![0]);
        req.frequency = Some(3);
        req.monte_carlo_tables = 200;
        let a = StratifiedAnalysis::run(&t, &RowStateManager::new(rows), &req).unwrap();
        assert!(a.pooled_exact().is_none(), "the pooled table is too large too");
        let by_label = |l: &str| a.strata.iter().find(|x| x.label == l).unwrap();
        assert!(matches!(by_label("small").exact, Some(Ok(ref e)) if !e.is_estimate()));
        assert!(matches!(by_label("huge").exact, Some(Ok(ref e)) if e.is_estimate()));
        assert!(matches!(by_label("big").exact, Some(Err(_))));
        let sections = a.sections();
        let SectionContent::Table(tests) = &sections[1].content else { panic!("tests table") };
        let big = tests.rows.iter().find(|row| row[0] == "big").unwrap();
        assert_eq!(big.last().unwrap(), "exact test not available");
        let SectionContent::Note(note) = &sections.last().unwrap().content else { panic!("note") };
        assert!(note.contains("big: exact test not available (") && note.contains("2⁵³"), "{note}");
    }
}
