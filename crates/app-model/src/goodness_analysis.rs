//! Goodness of fit for counts ([`stat_inference::goodness`]): whether the
//! categories of a column occur in given proportions, or whether a column of
//! counts follows a Poisson distribution with its mean estimated.
//!
//! Each row is one observation, or carries a frequency from an optional
//! frequency column. Excluded rows and missing values are left out. The
//! report gives Pearson's χ² and the likelihood-ratio G² with their degrees
//! of freedom and p-values, a table of observed and expected counts with
//! each cell's contribution to χ² and its Pearson residual, and a chart of
//! observed against expected counts per cell.

use crate::compare::section;
use crate::format::{format_number, format_p_value};
use crate::project::{invalid, member, s};
use crate::report::{Annotations, PlotSpec, ReportSection, SectionContent, SectionKind, TextTable};
use stat_core::Result;
use stat_data::{DataTable, JsonValue, RowStateManager};
use stat_inference::goodness::{GoodnessOfFit, goodness_of_fit_poisson, goodness_of_fit_proportions};
use stat_linear::natural_order;

const DIGITS: usize = 5;

/// What the counts are compared with.
#[derive(Debug, Clone, PartialEq)]
pub enum GoodnessModel {
    /// Given proportions (or ratios) per category label; empty means equal
    /// proportions.
    Proportions(Vec<(String, f64)>),
    /// A Poisson distribution with the mean estimated from the data; cells
    /// are pooled until each expected count is at least `min_expected`.
    Poisson {
        /// Smallest expected count per cell (Cochran's rule: 5).
        min_expected: f64,
    },
}

/// Settings of a goodness-of-fit report.
#[derive(Debug, Clone, PartialEq)]
pub struct GoodnessRequest {
    /// Category column (proportions) or numeric column of counts (Poisson).
    pub column: usize,
    /// Optional frequency column: how many observations each row stands for.
    pub frequency: Option<usize>,
    /// The hypothesized distribution.
    pub model: GoodnessModel,
}

impl GoodnessRequest {
    /// Equal proportions for the categories of `column`.
    #[must_use]
    pub fn new(column: usize) -> Self {
        Self { column, frequency: None, model: GoodnessModel::Proportions(Vec::new()) }
    }

    /// Every column the request uses.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        std::iter::once(self.column).chain(self.frequency).collect()
    }

    /// The request with its columns renumbered by `f`.
    #[must_use]
    pub fn map_columns(&self, f: &dyn Fn(usize) -> usize) -> Self {
        Self { column: f(self.column), frequency: self.frequency.map(f), model: self.model.clone() }
    }

    /// Project-file form.
    #[must_use]
    pub fn to_json(&self, table: &DataTable) -> JsonValue {
        let col = |i: usize| s(table.column(i).map_or("", |c| c.name.as_str()));
        let (model, extra) = match &self.model {
            GoodnessModel::Proportions(p) => (
                "proportions",
                (
                    "proportions",
                    JsonValue::Array(
                        p.iter()
                            .map(|(l, w)| crate::project::obj([("level", s(l)), ("proportion", JsonValue::number(*w))]))
                            .collect(),
                    ),
                ),
            ),
            GoodnessModel::Poisson { min_expected } => ("poisson", ("min_expected", JsonValue::number(*min_expected))),
        };
        crate::project::obj([
            ("analysis", s("goodness-of-fit")),
            ("column", col(self.column)),
            ("frequency", self.frequency.map_or(JsonValue::Null, col)),
            ("model", s(model)),
            extra,
        ])
    }

    /// Read the project-file form.
    ///
    /// # Errors
    /// Missing or unknown columns, an unknown model, malformed proportions.
    pub fn from_json(v: &JsonValue, table: &DataTable) -> Result<Self> {
        let find =
            |name: &str| table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")));
        let column = find(member(v, "column")?.as_str().ok_or_else(|| invalid("column: expected a column name"))?)?;
        let frequency = match v.get("frequency") {
            None | Some(JsonValue::Null) => None,
            Some(f) => Some(find(f.as_str().ok_or_else(|| invalid("frequency: expected a column name"))?)?),
        };
        let model = match v.get("model").and_then(JsonValue::as_str) {
            None | Some("proportions") => {
                let mut list = Vec::new();
                for p in v.get("proportions").and_then(JsonValue::as_array).unwrap_or(&[]) {
                    let level = p.get("level").and_then(JsonValue::as_str);
                    let weight = p.get("proportion").and_then(JsonValue::as_f64);
                    match (level, weight) {
                        (Some(l), Some(w)) => list.push((l.to_owned(), w)),
                        _ => return Err(invalid("goodness of fit: each proportion needs a level and a value")),
                    }
                }
                GoodnessModel::Proportions(list)
            }
            Some("poisson") => GoodnessModel::Poisson {
                min_expected: v.get("min_expected").and_then(JsonValue::as_f64).unwrap_or(5.0),
            },
            Some(other) => return Err(invalid(format!("goodness of fit: unknown model {other:?}"))),
        };
        Ok(Self { column, frequency, model })
    }
}

/// A computed goodness-of-fit report.
#[derive(Debug, Clone)]
pub struct GoodnessAnalysis {
    request: GoodnessRequest,
    column_name: String,
    /// Label of each cell.
    labels: Vec<String>,
    /// The test.
    pub fit: GoodnessOfFit,
}

impl GoodnessAnalysis {
    /// Count the observations and run the test.
    ///
    /// # Errors
    /// Invalid columns, negative or non-numeric frequencies, non-integer
    /// Poisson counts, categories without a given proportion, or too few
    /// cells.
    pub fn run(table: &DataTable, rows: &RowStateManager, request: &GoodnessRequest) -> Result<Self> {
        let column = table.column(request.column).ok_or_else(|| invalid("no such column"))?;
        let freq = match request.frequency {
            Some(f) if f == request.column => {
                return Err(invalid("the frequency column must differ from the data column"));
            }
            Some(f) => Some(
                table
                    .column(f)
                    .and_then(|c| c.as_numeric())
                    .ok_or_else(|| invalid("the frequency column must be numeric"))?,
            ),
            None => None,
        };
        let weight = |r: usize| -> Result<Option<f64>> {
            match freq {
                None => Ok(Some(1.0)),
                Some(f) if f[r].is_nan() => Ok(None),
                Some(f) if f[r] < 0.0 || !f[r].is_finite() => {
                    Err(invalid(format!("row {}: frequencies cannot be negative", r + 1)))
                }
                Some(f) => Ok(Some(f[r])),
            }
        };
        let included = (0..table.n_rows()).filter(|&r| !rows.is_excluded(r));
        match &request.model {
            GoodnessModel::Proportions(given) => {
                let mut labels: Vec<String> = Vec::new();
                let mut counts: Vec<f64> = Vec::new();
                for r in included {
                    let label = column.level_text(r);
                    let Some(w) = weight(r)? else { continue };
                    if label.is_empty() {
                        continue;
                    }
                    match labels.iter().position(|l| *l == label) {
                        Some(i) => counts[i] += w,
                        None => {
                            labels.push(label);
                            counts.push(w);
                        }
                    }
                }
                for (l, _) in given {
                    if !labels.contains(l) {
                        labels.push(l.clone());
                        counts.push(0.0);
                    }
                }
                let mut order: Vec<usize> = (0..labels.len()).collect();
                order.sort_by(|&a, &b| natural_order(&labels[a], &labels[b]));
                let labels: Vec<String> = order.iter().map(|&i| labels[i].clone()).collect();
                let counts: Vec<f64> = order.iter().map(|&i| counts[i]).collect();
                let proportions: Vec<f64> = if given.is_empty() {
                    vec![1.0; labels.len()]
                } else {
                    labels
                        .iter()
                        .map(|l| {
                            given.iter().find(|(g, _)| g == l).map(|(_, w)| *w).ok_or_else(|| {
                                invalid(format!("no hypothesized proportion is given for the category {l:?}"))
                            })
                        })
                        .collect::<Result<_>>()?
                };
                let fit = goodness_of_fit_proportions(&counts, &proportions)?;
                Ok(Self { request: request.clone(), column_name: column.name.clone(), labels, fit })
            }
            GoodnessModel::Poisson { min_expected } => {
                let values = column.as_numeric().ok_or_else(|| invalid(format!("{} must be numeric", column.name)))?;
                let mut data = Vec::new();
                for r in included {
                    if values[r].is_nan() {
                        continue;
                    }
                    if let Some(w) = weight(r)? {
                        data.push((values[r], w));
                    }
                }
                let fit = goodness_of_fit_poisson(&data, *min_expected)?;
                let labels = fit
                    .cells
                    .iter()
                    .map(|c| match c.high {
                        None => format!("≥ {}", c.low),
                        Some(h) if h == c.low => c.low.to_string(),
                        Some(h) => format!("{}–{}", c.low, h),
                    })
                    .collect();
                Ok(Self { request: request.clone(), column_name: column.name.clone(), labels, fit })
            }
        }
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &GoodnessRequest {
        &self.request
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        match self.request.model {
            GoodnessModel::Proportions(_) => format!("Goodness of Fit: {}", self.column_name),
            GoodnessModel::Poisson { .. } => format!("Goodness of Fit to a Poisson: {}", self.column_name),
        }
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let f = &self.fit;
        let mut tests = TextTable::new(["Test", "Statistic", "DF", "p-value"]);
        tests.push([
            "Pearson χ²".to_owned(),
            format_number(f.pearson, DIGITS),
            format_number(f.df, 4),
            format_p_value(f.pearson_p),
        ]);
        tests.push([
            "Likelihood ratio G²".to_owned(),
            format_number(f.likelihood_ratio, DIGITS),
            format_number(f.df, 4),
            format_p_value(f.likelihood_ratio_p),
        ]);
        let against = match &self.request.model {
            GoodnessModel::Proportions(p) if p.is_empty() => "equal proportions".to_owned(),
            GoodnessModel::Proportions(_) => "the given proportions".to_owned(),
            GoodnessModel::Poisson { .. } => {
                format!("a Poisson with the estimated mean {} (1 DF used)", format_number(f.mean, DIGITS))
            }
        };
        let warning = if f.small_expected > 0 {
            format!(
                "; {} of {} cells expect fewer than 5, so the χ² approximation may be poor",
                f.small_expected,
                f.cells.len()
            )
        } else {
            String::new()
        };
        let mut out = vec![section(
            SectionKind::GoodnessOfFitTests,
            format!("Goodness of Fit to {against}: N = {}{warning}", format_number(f.total, 8)),
            SectionContent::Table(tests),
        )];
        let mut cells = TextTable::new([
            if matches!(self.request.model, GoodnessModel::Poisson { .. }) { "Count" } else { "Category" },
            "Observed",
            "Hypothesized proportion",
            "Expected",
            "Contribution to χ²",
            "Pearson residual",
        ]);
        for (c, label) in f.cells.iter().zip(&self.labels) {
            cells.push([
                label.clone(),
                format_number(c.observed, 8),
                format_number(c.probability, DIGITS),
                format_number(c.expected, DIGITS),
                format_number(c.contribution, DIGITS),
                format_number(c.residual, 4),
            ]);
        }
        let pooled = matches!(self.request.model, GoodnessModel::Poisson { min_expected } if min_expected > 0.0);
        out.push(section(
            SectionKind::GoodnessOfFitCells,
            if pooled {
                "Observed and Expected Counts (neighbouring counts pooled until each cell expects at least the minimum)"
                    .to_owned()
            } else {
                "Observed and Expected Counts".to_owned()
            },
            SectionContent::Table(cells),
        ));
        let mut items = Vec::new();
        let mut segments = Vec::new();
        for (i, c) in f.cells.iter().enumerate() {
            let x = (i + 1) as f64;
            items.push((x, c.observed, String::new(), 0));
            items.push((x, c.expected, String::new(), 1));
            segments.push(vec![[x, c.expected], [x, c.observed]]);
        }
        let spec = PlotSpec {
            x_time: None,
            y_time: None,
            x_label: self.column_name.clone(),
            y_label: "Count".to_owned(),
            points: Vec::new(),
            line: None,
            band: None,
            groups: None,
            x_ticks: Some(self.labels.iter().enumerate().map(|(i, l)| ((i + 1) as f64, l.clone())).collect()),
            panes: Vec::new(),
            annotations: Some(Annotations {
                groups: vec!["Observed".to_owned(), "Expected".to_owned()],
                items,
                labels: Vec::new(),
            }),
            segments,
            ..PlotSpec::default()
        };
        out.push(section(
            SectionKind::GoodnessOfFitPlot,
            "Observed and Expected Counts by Cell (line: the gap between them)",
            SectionContent::Plot(Box::new(spec)),
        ));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    fn table_of(s: &[ReportSection], kind: SectionKind) -> &TextTable {
        match &s.iter().find(|x| x.kind == kind).unwrap().content {
            SectionContent::Table(t) => t,
            _ => panic!(),
        }
    }

    /// Mendel's peas as a frequency table (engine values are checked in
    /// `stat_inference::goodness`).
    #[test]
    fn proportions_from_a_frequency_column() {
        let t = DataTable::new(
            "peas",
            vec![
                Column::text(
                    "Seed",
                    ["round yellow", "round green", "wrinkled yellow", "wrinkled green"].map(String::from).to_vec(),
                ),
                Column::numeric("Count", vec![315.0, 108.0, 101.0, 32.0]),
            ],
        )
        .unwrap();
        let req = GoodnessRequest {
            column: 0,
            frequency: Some(1),
            model: GoodnessModel::Proportions(vec![
                ("round yellow".into(), 9.0),
                ("round green".into(), 3.0),
                ("wrinkled yellow".into(), 3.0),
                ("wrinkled green".into(), 1.0),
            ]),
        };
        let a = GoodnessAnalysis::run(&t, &RowStateManager::new(4), &req).unwrap();
        assert_eq!(a.title(), "Goodness of Fit: Seed");
        let s = a.sections();
        assert_eq!(table_of(&s, SectionKind::GoodnessOfFitTests).rows[0][1], "0.47002");
        assert_eq!(table_of(&s, SectionKind::GoodnessOfFitCells).rows.len(), 4);
        // A category without a proportion is an error; excluded rows drop out.
        let missing =
            GoodnessRequest { model: GoodnessModel::Proportions(vec![("round yellow".into(), 9.0)]), ..req.clone() };
        assert!(GoodnessAnalysis::run(&t, &RowStateManager::new(4), &missing).is_err());
        let mut rows = RowStateManager::new(4);
        rows.set_excluded([3], true);
        let a = GoodnessAnalysis::run(&t, &rows, &req).unwrap();
        assert_eq!(a.fit.cells.iter().find(|c| c.observed == 0.0).map(|c| c.expected > 0.0), Some(true));
        // Round trip through the project form.
        assert_eq!(GoodnessRequest::from_json(&req.to_json(&t), &t).unwrap(), req);
    }

    #[test]
    fn poisson_counts_one_per_row() {
        let counts: Vec<f64> = [(0.0, 109), (1.0, 65), (2.0, 22), (3.0, 3), (4.0, 1)]
            .iter()
            .flat_map(|&(v, n)| std::iter::repeat_n(v, n))
            .collect();
        let t = DataTable::new("kicks", vec![Column::numeric("Deaths", counts)]).unwrap();
        let req = GoodnessRequest { column: 0, frequency: None, model: GoodnessModel::Poisson { min_expected: 5.0 } };
        let a = GoodnessAnalysis::run(&t, &RowStateManager::new(200), &req).unwrap();
        let s = a.sections();
        let cells = table_of(&s, SectionKind::GoodnessOfFitCells);
        assert_eq!(cells.rows.iter().map(|r| r[0].as_str()).collect::<Vec<_>>(), ["0", "1", "≥ 2"]);
        assert!(table_of(&s, SectionKind::GoodnessOfFitTests).rows[0][2] == "1");
        assert_eq!(GoodnessRequest::from_json(&req.to_json(&t), &t).unwrap(), req);
    }
}
