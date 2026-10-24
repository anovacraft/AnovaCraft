//! Correlations of several continuous columns, bound to the row state
//! ([`stat_multivariate::correlations`]).
//!
//! Excluded rows are left out; a missing value removes the row from the
//! pairs that need it (pairwise) or from every pair (listwise). The report
//! shows each column's summary, the Pearson matrix, a heat map of it (each
//! cell's value printed in the cell), every pair's r with its Fisher-z
//! interval and p-value, and optionally Spearman's ρₛ, the partial
//! correlations (from the complete rows) and the covariance matrix.

use stat_core::{Result, StatError};
use stat_data::{DataTable, JsonValue, RowStateManager};
use stat_multivariate::{CorrelationMatrix, MissingRows, correlation_matrix};

use crate::format::{format_number, format_p_value};
use crate::heat_map::HeatMapSpec;
use crate::report::{ReportSection, SectionContent, SectionKind, TextTable};

const DIGITS: usize = 4;

/// What to correlate and which tables to show.
#[derive(Debug, Clone, PartialEq)]
pub struct CorrelationRequest {
    /// Continuous columns (two or more).
    pub columns: Vec<usize>,
    /// Rows used when values are missing.
    pub missing: MissingRows,
    /// Confidence level of the intervals.
    pub level: f64,
    /// Show Spearman's rank correlations.
    pub spearman: bool,
    /// Show partial correlations.
    pub partial: bool,
    /// Show the covariance matrix.
    pub covariance: bool,
}

impl CorrelationRequest {
    /// Default confidence level.
    pub const DEFAULT_LEVEL: f64 = 0.95;

    /// Pearson correlations of `columns` with pairwise rows, 95% intervals,
    /// and partial correlations.
    #[must_use]
    pub fn new(columns: Vec<usize>) -> Self {
        Self {
            columns,
            missing: MissingRows::Pairwise,
            level: Self::DEFAULT_LEVEL,
            spearman: false,
            partial: true,
            covariance: false,
        }
    }

    /// Every column the request reads.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        self.columns.clone()
    }

    /// Pass every column index through `f`.
    pub fn map_columns(&mut self, f: &impl Fn(usize) -> usize) {
        self.columns.iter_mut().for_each(|c| *c = f(*c));
    }

    /// The project-file form (`"analysis": "correlations"`).
    #[must_use]
    pub fn to_json(&self, table: &DataTable) -> JsonValue {
        crate::project::obj([
            ("analysis", crate::project::s("correlations")),
            ("columns", columns_json(&self.columns, table)),
            ("missing", crate::project::s(self.missing.key())),
            ("level", JsonValue::number(self.level)),
            ("spearman", JsonValue::Bool(self.spearman)),
            ("partial", JsonValue::Bool(self.partial)),
            ("covariance", JsonValue::Bool(self.covariance)),
        ])
    }

    /// Read the project-file form; missing settings take their defaults.
    ///
    /// # Errors
    /// Missing or unknown columns, an unknown missing-value choice.
    pub fn from_json(v: &JsonValue, table: &DataTable) -> Result<Self> {
        let d = Self::new(json_columns(v, "columns", table)?);
        let missing = match v.get("missing").and_then(JsonValue::as_str) {
            None => d.missing,
            Some(key) => MissingRows::from_key(key)
                .ok_or_else(|| invalid(format!("correlations: unknown missing-value choice {key:?}")))?,
        };
        let flag = |key: &str, default: bool| v.get(key).and_then(JsonValue::as_bool).unwrap_or(default);
        Ok(Self {
            missing,
            level: v.get("level").and_then(JsonValue::as_f64).unwrap_or(d.level),
            spearman: flag("spearman", d.spearman),
            partial: flag("partial", d.partial),
            covariance: flag("covariance", d.covariance),
            ..d
        })
    }
}

/// Column names as a JSON list.
pub(crate) fn columns_json(columns: &[usize], table: &DataTable) -> JsonValue {
    JsonValue::Array(
        columns.iter().map(|&i| crate::project::s(table.column(i).map_or("", |c| c.name.as_str()))).collect(),
    )
}

/// The columns named in the JSON list `v[key]` (an empty list when absent).
pub(crate) fn json_columns(v: &JsonValue, key: &str, table: &DataTable) -> Result<Vec<usize>> {
    let Some(list) = v.get(key).filter(|x| !matches!(x, JsonValue::Null)) else { return Ok(Vec::new()) };
    let list = list.as_array().ok_or_else(|| invalid(format!("{key:?} must be a list of column names")))?;
    list.iter()
        .map(|c| {
            let name = c.as_str().ok_or_else(|| invalid(format!("{key:?} must be a list of column names")))?;
            table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")))
        })
        .collect()
}

/// A computed correlation report.
#[derive(Debug, Clone)]
pub struct CorrelationAnalysis {
    request: CorrelationRequest,
    names: Vec<String>,
    matrix: CorrelationMatrix,
    n_excluded: usize,
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

fn section(kind: SectionKind, title: &str, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.to_owned(), content }
}

impl CorrelationAnalysis {
    /// Compute the correlations.
    ///
    /// # Errors
    /// Fewer than two columns, a missing or non-numeric column, and errors
    /// from [`correlation_matrix`].
    pub fn run(table: &DataTable, states: &RowStateManager, request: &CorrelationRequest) -> Result<Self> {
        if request.columns.len() < 2 {
            return Err(invalid("choose at least two columns"));
        }
        let mut names = Vec::new();
        let mut values = Vec::new();
        for &i in &request.columns {
            let c = table.column(i).ok_or_else(|| invalid(format!("no column {i}")))?;
            values.push(c.as_numeric().ok_or_else(|| invalid(format!("{} must be numeric", c.name)))?);
            names.push(c.name.clone());
        }
        let used: Vec<usize> = (0..table.n_rows()).filter(|&r| !states.is_excluded(r)).collect();
        let n_excluded = table.n_rows() - used.len();
        let columns: Vec<Vec<f64>> = values
            .iter()
            .map(|v| used.iter().map(|&r| if v[r].is_finite() { v[r] } else { f64::NAN }).collect())
            .collect();
        let matrix = correlation_matrix(&columns, request.missing, request.level)?;
        Ok(Self { request: request.clone(), names, matrix, n_excluded })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &CorrelationRequest {
        &self.request
    }

    /// The computed matrix.
    #[must_use]
    pub fn matrix(&self) -> &CorrelationMatrix {
        &self.matrix
    }

    /// Report title, e.g. `"Correlations: Height, Weight"`.
    #[must_use]
    pub fn title(&self) -> String {
        format!("Correlations: {}", self.names.join(", "))
    }

    /// Report sections: summary, matrix, heat map, pairwise table, then the
    /// optional Spearman, partial and covariance tables.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let mut out = vec![self.summary(), self.pearson_matrix(), self.heat_map(), self.pairwise()];
        if self.request.spearman {
            out.push(self.spearman());
        }
        if self.request.partial {
            out.push(self.partial());
        }
        if self.request.covariance {
            out.push(self.covariance());
        }
        out
    }

    fn summary(&self) -> ReportSection {
        let m = &self.matrix;
        let mut t = TextTable::new(["Column", "N", "Mean", "Std Dev"]);
        for (j, name) in self.names.iter().enumerate() {
            t.push([
                name.clone(),
                m.counts[j].to_string(),
                format_number(m.means[j], 6),
                format_number(m.std_devs[j], 6),
            ]);
        }
        t.push(["Rows complete in every column".to_owned(), m.complete_rows.to_string(), String::new(), String::new()]);
        if self.n_excluded > 0 {
            t.push(["Rows excluded".to_owned(), self.n_excluded.to_string(), String::new(), String::new()]);
        }
        t.push(["Missing values".to_owned(), self.request.missing.label().to_owned(), String::new(), String::new()]);
        section(SectionKind::CorrelationSummary, "Summary", SectionContent::Table(t))
    }

    fn matrix_table(&self, cell: impl Fn(usize, usize) -> String) -> TextTable {
        let mut t = TextTable::new(std::iter::once(String::new()).chain(self.names.iter().cloned()));
        for (i, name) in self.names.iter().enumerate() {
            t.push(std::iter::once(name.clone()).chain((0..self.names.len()).map(|j| cell(i, j))));
        }
        t
    }

    fn pearson_matrix(&self) -> ReportSection {
        let t = self.matrix_table(|i, j| format_number(self.matrix.pearson_r(i, j), DIGITS));
        section(SectionKind::CorrelationMatrix, "Pearson Correlations", SectionContent::Table(t))
    }

    fn heat_map(&self) -> ReportSection {
        let p = self.names.len();
        let values: Vec<Vec<f64>> = (0..p).map(|i| (0..p).map(|j| self.matrix.pearson_r(i, j)).collect()).collect();
        let text = values
            .iter()
            .map(|row| {
                row.iter()
                    .map(|v| if v.is_finite() { format!("{v:.2}").replace('-', "−") } else { String::new() })
                    .collect()
            })
            .collect();
        let spec = HeatMapSpec {
            rows: self.names.clone(),
            columns: self.names.clone(),
            values,
            text,
            measure: "r".to_owned(),
            limit: 1.0,
        };
        section(SectionKind::CorrelationHeatMap, "Correlation Heat Map", SectionContent::HeatMap(spec))
    }

    fn level_text(&self) -> String {
        format!("{}%", format_number(self.request.level * 100.0, 4))
    }

    fn pair_table(
        &self,
        title: &str,
        kind: SectionKind,
        pick: impl Fn(usize, usize) -> Option<stat_core::correlation::Correlation>,
    ) -> ReportSection {
        let level = self.level_text();
        let mut t = TextTable::new([
            "Column".to_owned(),
            "by Column".to_owned(),
            "N".to_owned(),
            "Estimate".to_owned(),
            format!("Lower {level}"),
            format!("Upper {level}"),
            "p-value".to_owned(),
        ]);
        let p = self.names.len();
        for i in 0..p {
            for j in i + 1..p {
                let n = self.matrix.pairs[i][j].map_or(0, |q| q.n);
                let c = pick(i, j);
                let (lo, hi) = c.and_then(|c| c.confidence_interval).unwrap_or((f64::NAN, f64::NAN));
                t.push([
                    self.names[i].clone(),
                    self.names[j].clone(),
                    n.to_string(),
                    format_number(c.map_or(f64::NAN, |c| c.estimate), DIGITS),
                    format_number(lo, DIGITS),
                    format_number(hi, DIGITS),
                    c.and_then(|c| c.p_value).map_or_else(|| "—".to_owned(), format_p_value),
                ]);
            }
        }
        section(kind, title, SectionContent::Table(t))
    }

    fn pairwise(&self) -> ReportSection {
        self.pair_table("Pairwise Pearson Correlations", SectionKind::PairwiseCorrelations, |i, j| {
            self.matrix.pairs[i][j].and_then(|q| q.pearson)
        })
    }

    fn spearman(&self) -> ReportSection {
        self.pair_table("Spearman's ρ", SectionKind::SpearmanCorrelations, |i, j| {
            self.matrix.pairs[i][j].and_then(|q| q.spearman)
        })
    }

    fn partial(&self) -> ReportSection {
        let title = "Partial Correlations";
        let Some(partial) = &self.matrix.partial else {
            let why = if self.names.len() < 3 {
                "With two columns there is nothing to hold fixed: the partial correlation is the ordinary one."
            } else {
                "Not available: the complete rows must outnumber the columns, and no column may be constant or an \
                 exact combination of the others."
            };
            return section(SectionKind::PartialCorrelations, title, SectionContent::Note(why.to_owned()));
        };
        let mut table = TextTable::new(["Column", "by Column", "Partial r", "p-value"]);
        let p = self.names.len();
        for i in 0..p {
            for j in i + 1..p {
                table.push([
                    self.names[i].clone(),
                    self.names[j].clone(),
                    format_number(partial.estimates[i][j], DIGITS),
                    partial.p_values[i][j].map_or_else(|| "—".to_owned(), format_p_value),
                ]);
            }
        }
        let title = format!(
            "Partial Correlations (each pair given the other columns; {} complete rows, {} df)",
            partial.n, partial.df
        );
        section(SectionKind::PartialCorrelations, &title, SectionContent::Table(table))
    }

    fn covariance(&self) -> ReportSection {
        let t = self.matrix_table(|i, j| {
            if i == j {
                let sd = self.matrix.std_devs[i];
                format_number(sd * sd, 6)
            } else {
                format_number(self.matrix.pairs[i][j].map_or(f64::NAN, |q| q.covariance), 6)
            }
        });
        section(SectionKind::CovarianceMatrix, "Covariance Matrix", SectionContent::Table(t))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample::{course_evaluation, flowers};

    fn measures(t: &DataTable) -> Vec<usize> {
        ["Sepal length", "Sepal width", "Petal length", "Petal width"].iter().map(|c| t.find(c).unwrap()).collect()
    }

    #[test]
    fn sections_heat_map_and_exclusion() {
        let t = flowers();
        let mut rows = RowStateManager::new(t.n_rows());
        let request = CorrelationRequest { spearman: true, covariance: true, ..CorrelationRequest::new(measures(&t)) };
        let a = CorrelationAnalysis::run(&t, &rows, &request).unwrap();
        let kinds: Vec<SectionKind> = a.sections().iter().map(|s| s.kind).collect();
        assert_eq!(
            kinds,
            [
                SectionKind::CorrelationSummary,
                SectionKind::CorrelationMatrix,
                SectionKind::CorrelationHeatMap,
                SectionKind::PairwiseCorrelations,
                SectionKind::SpearmanCorrelations,
                SectionKind::PartialCorrelations,
                SectionKind::CovarianceMatrix,
            ]
        );
        let s = a.sections();
        let SectionContent::HeatMap(h) = &s[2].content else { panic!("heat map") };
        assert_eq!(h.values.len(), 4);
        assert_eq!(h.values[0][0], 1.0);
        assert!((h.values[2][3] - a.matrix().pearson_r(2, 3)).abs() < 1e-15);
        assert_eq!(h.text[2][3], format!("{:.2}", a.matrix().pearson_r(2, 3)).replace('-', "−"));
        let SectionContent::Table(pairs) = &s[3].content else { panic!() };
        assert_eq!(pairs.rows.len(), 6, "every pair once");
        assert_eq!(pairs.rows[0][2], "90");
        // The Pearson matrix agrees with the bivariate routine on the same rows.
        let x = t.column(measures(&t)[0]).unwrap().as_numeric().unwrap();
        let y = t.column(measures(&t)[1]).unwrap().as_numeric().unwrap();
        let r = stat_core::correlation::pearson(x, y, 0.95).unwrap().correlation.estimate;
        assert!((a.matrix().pearson_r(0, 1) - r).abs() < 1e-14);
        // Excluding rows removes them.
        rows.set_excluded([0, 1, 2], true);
        let b = CorrelationAnalysis::run(&t, &rows, &request).unwrap();
        assert_eq!(b.matrix().counts, [87; 4]);
        assert!(a.title().starts_with("Correlations: Sepal length"));
    }

    #[test]
    fn missing_values_pairwise_or_listwise_and_errors() {
        let t = course_evaluation();
        let rows = RowStateManager::new(t.n_rows());
        let items: Vec<usize> = (2..8).collect();
        let pw = CorrelationAnalysis::run(&t, &rows, &CorrelationRequest::new(items.clone())).unwrap();
        let lw = CorrelationAnalysis::run(
            &t,
            &rows,
            &CorrelationRequest { missing: MissingRows::Listwise, ..CorrelationRequest::new(items) },
        )
        .unwrap();
        assert_eq!(pw.matrix().complete_rows, 116);
        assert_eq!(pw.matrix().pairs[0][1].unwrap().n, 119);
        assert!(lw.matrix().pairs.iter().flatten().flatten().all(|p| p.n == 116));
        assert!(CorrelationAnalysis::run(&t, &rows, &CorrelationRequest::new(vec![2])).is_err());
        assert!(CorrelationAnalysis::run(&t, &rows, &CorrelationRequest::new(vec![1, 2])).is_err(), "text column");
        // Two columns: the partial-correlation section explains itself.
        let two = CorrelationAnalysis::run(&t, &rows, &CorrelationRequest::new(vec![2, 3])).unwrap();
        let partial = two.sections().into_iter().find(|s| s.kind == SectionKind::PartialCorrelations).unwrap();
        assert!(matches!(partial.content, SectionContent::Note(_)));
    }

    #[test]
    fn json_defaults() {
        let t = flowers();
        let v = stat_data::parse_json(r#"{"analysis": "correlations", "columns": ["Sepal length", "Petal width"]}"#)
            .unwrap();
        let r = CorrelationRequest::from_json(&v, &t).unwrap();
        assert_eq!(r, CorrelationRequest::new(vec![1, 4]));
        let bad = stat_data::parse_json(r#"{"columns": ["Sepal length", "x"]}"#).unwrap();
        assert!(CorrelationRequest::from_json(&bad, &t).is_err());
        let bad = stat_data::parse_json(r#"{"columns": ["Sepal length"], "missing": "some"}"#).unwrap();
        assert!(CorrelationRequest::from_json(&bad, &t).is_err());
    }
}
