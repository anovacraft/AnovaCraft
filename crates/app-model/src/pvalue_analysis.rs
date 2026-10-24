//! Adjusting a column of p-values for multiple testing and combining them
//! ([`stat_inference::pvalues`]).
//!
//! Each usable row (not excluded, with a p-value) is one test. The report
//! lists every row's p-value with its Bonferroni, Šidák, Holm, Hochberg,
//! Benjamini–Hochberg and Benjamini–Yekutieli adjustments and Storey
//! q-value, counts the rejections of each method at the chosen `α`, plots
//! the ordered p-values against the Benjamini–Hochberg line, and combines
//! them by Fisher's and Stouffer's (optionally weighted) methods. Every
//! adjustment can be saved as a new column.

use crate::compare::{DIGITS, section};
use crate::format::{format_number, format_p_value};
use crate::project::{invalid, member, obj, s};
use crate::report::{PlotSpec, ReportSection, RowPoint, SectionContent, SectionKind, TextTable};
use crate::report_columns::ReportColumn;
use stat_core::Result;
use stat_data::{DataTable, JsonValue, RowStateManager};
use stat_inference::pvalues::{
    AdjustMethod, CombinedTest, QValues, adjust_p_values, fisher_combined, storey_q_values, stouffer_combined,
};

/// Settings of a p-value adjustment report.
#[derive(Debug, Clone, PartialEq)]
pub struct PValueRequest {
    /// Column of p-values.
    pub p_values: usize,
    /// Optional column naming each test.
    pub label: Option<usize>,
    /// Optional column of weights for Stouffer's combined test.
    pub weight: Option<usize>,
    /// Error rate at which rejections are counted.
    pub alpha: f64,
    /// Storey's tuning value `λ` for the proportion of true nulls.
    pub lambda: f64,
}

impl PValueRequest {
    /// Defaults: `α = 0.05`, `λ = 0.5`, no labels or weights.
    #[must_use]
    pub fn new(p_values: usize) -> Self {
        Self { p_values, label: None, weight: None, alpha: 0.05, lambda: 0.5 }
    }

    /// Every column the request uses.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        std::iter::once(self.p_values).chain(self.label).chain(self.weight).collect()
    }

    /// The request with its columns renumbered by `f`.
    #[must_use]
    pub fn map_columns(&self, f: &dyn Fn(usize) -> usize) -> Self {
        Self { p_values: f(self.p_values), label: self.label.map(f), weight: self.weight.map(f), ..self.clone() }
    }

    /// Project-file form.
    #[must_use]
    pub fn to_json(&self, table: &DataTable) -> JsonValue {
        let col = |i: usize| s(table.column(i).map_or("", |c| c.name.as_str()));
        obj([
            ("analysis", s("adjust-p-values")),
            ("p_values", col(self.p_values)),
            ("label", self.label.map_or(JsonValue::Null, col)),
            ("weight", self.weight.map_or(JsonValue::Null, col)),
            ("alpha", JsonValue::number(self.alpha)),
            ("lambda", JsonValue::number(self.lambda)),
        ])
    }

    /// Read the project-file form.
    ///
    /// # Errors
    /// Missing columns or settings out of range.
    pub fn from_json(v: &JsonValue, table: &DataTable) -> Result<Self> {
        let find =
            |name: &str| table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")));
        let opt = |key: &str| -> Result<Option<usize>> {
            match v.get(key) {
                None | Some(JsonValue::Null) => Ok(None),
                Some(x) => find(x.as_str().ok_or_else(|| invalid(format!("{key}: expected a column name")))?).map(Some),
            }
        };
        let p = member(v, "p_values")?.as_str().ok_or_else(|| invalid("p_values: expected a column name"))?;
        let r = Self {
            p_values: find(p)?,
            label: opt("label")?,
            weight: opt("weight")?,
            alpha: v.get("alpha").and_then(JsonValue::as_f64).unwrap_or(0.05),
            lambda: v.get("lambda").and_then(JsonValue::as_f64).unwrap_or(0.5),
        };
        if !(r.alpha > 0.0 && r.alpha < 1.0) {
            return Err(invalid("adjust p-values: alpha must lie between 0 and 1"));
        }
        if !(0.0..1.0).contains(&r.lambda) {
            return Err(invalid("adjust p-values: lambda must lie in [0, 1)"));
        }
        Ok(r)
    }
}

/// A computed p-value adjustment report.
#[derive(Debug, Clone)]
pub struct PValueAnalysis {
    request: PValueRequest,
    p_name: String,
    n_rows: usize,
    /// Rows used, in table order.
    rows: Vec<usize>,
    labels: Vec<String>,
    p: Vec<f64>,
    /// Adjusted p-values by method (in [`AdjustMethod::ALL`] order).
    adjusted: Vec<Vec<f64>>,
    q: QValues,
    fisher: Result<CombinedTest>,
    stouffer: Result<CombinedTest>,
    weighted: bool,
}

impl PValueAnalysis {
    /// Gather the p-values and adjust them.
    ///
    /// # Errors
    /// A non-numeric p-value or weight column, no usable rows, or a value
    /// outside `[0, 1]`.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &PValueRequest) -> Result<Self> {
        let pc = table.column(request.p_values).ok_or_else(|| invalid("no such column"))?;
        let pv = pc.as_numeric().ok_or_else(|| invalid(format!("{} must hold numbers (p-values)", pc.name)))?;
        let weights = match request.weight {
            Some(w) => {
                let c = table.column(w).ok_or_else(|| invalid("no such column"))?;
                Some(c.as_numeric().ok_or_else(|| invalid(format!("the weight column {} must be numeric", c.name)))?)
            }
            None => None,
        };
        let rows: Vec<usize> = (0..table.n_rows()).filter(|&r| !states.is_excluded(r) && !pv[r].is_nan()).collect();
        if rows.is_empty() {
            return Err(invalid(format!("{} has no p-values in rows that are not excluded", pc.name)));
        }
        let p: Vec<f64> = rows.iter().map(|&r| pv[r]).collect();
        if let Some(bad) = rows.iter().find(|&&r| !(0.0..=1.0).contains(&pv[r])) {
            return Err(invalid(format!(
                "{}: row {} holds {}, but p-values lie between 0 and 1",
                pc.name,
                bad + 1,
                pv[*bad]
            )));
        }
        let labels: Vec<String> = match request.label.and_then(|c| table.column(c)) {
            Some(c) => rows.iter().map(|&r| c.level_text(r)).collect(),
            None => rows.iter().map(|&r| format!("Row {}", r + 1)).collect(),
        };
        let adjusted = AdjustMethod::ALL.iter().map(|&m| adjust_p_values(&p, m)).collect::<Result<Vec<_>>>()?;
        let q = storey_q_values(&p, request.lambda)?;
        let w: Option<Vec<f64>> = weights.map(|w| rows.iter().map(|&r| w[r]).collect());
        Ok(Self {
            request: request.clone(),
            p_name: pc.name.clone(),
            n_rows: table.n_rows(),
            labels,
            fisher: fisher_combined(&p),
            stouffer: stouffer_combined(&p, w.as_deref()),
            weighted: w.is_some(),
            rows,
            p,
            adjusted,
            q,
        })
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        format!("Adjusted p-values: {}", self.p_name)
    }

    /// Adjusted p-values of `method` in the order of the rows used.
    #[must_use]
    pub fn adjusted(&self, method: AdjustMethod) -> &[f64] {
        let i = AdjustMethod::ALL.iter().position(|&m| m == method).unwrap_or(0);
        &self.adjusted[i]
    }

    /// Storey's q-values and `π̂₀`.
    #[must_use]
    pub fn q_values(&self) -> &QValues {
        &self.q
    }

    /// One column per adjustment (keys as [`AdjustMethod::key`], and `"q"`
    /// for q-values); NaN in rows not used.
    #[must_use]
    pub fn saveable_columns(&self) -> Vec<ReportColumn> {
        let spread = |v: &[f64]| {
            let mut out = vec![f64::NAN; self.n_rows];
            for (&r, &x) in self.rows.iter().zip(v) {
                out[r] = x;
            }
            out
        };
        let mut out: Vec<ReportColumn> = AdjustMethod::ALL
            .iter()
            .zip(&self.adjusted)
            .map(|(m, v)| {
                ReportColumn::continuous(
                    m.key(),
                    &format!("Save {} p-values", m.label()),
                    format!("{} {}", m.label(), self.p_name),
                    spread(v),
                )
            })
            .collect();
        out.push(ReportColumn::continuous(
            "q",
            "Save Storey q-values",
            format!("q-value {}", self.p_name),
            spread(&self.q.q_values),
        ));
        out
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let alpha = self.request.alpha;
        let m = self.p.len();
        let mut out = Vec::new();
        // Summary: rejections by each method at α.
        let mut summary = TextTable::new(["Method", "Controls", "Rejected at α", "Largest p-value rejected"]);
        let rejected = |adj: &[f64]| -> (usize, f64) {
            let mut n = 0;
            let mut largest = f64::NAN;
            for (p, a) in self.p.iter().zip(adj) {
                if *a <= alpha {
                    n += 1;
                    largest = if largest.is_nan() { *p } else { largest.max(*p) };
                }
            }
            (n, largest)
        };
        let unadjusted = rejected(&self.p);
        summary.push([
            "None (unadjusted)".to_owned(),
            "per-test error rate".to_owned(),
            unadjusted.0.to_string(),
            format_number(unadjusted.1, DIGITS),
        ]);
        for (method, adj) in AdjustMethod::ALL.iter().zip(&self.adjusted) {
            let (n, largest) = rejected(adj);
            summary.push([
                method.label().to_owned(),
                method.controls().to_owned(),
                n.to_string(),
                format_number(largest, DIGITS),
            ]);
        }
        let (nq, largest_q) = rejected(&self.q.q_values);
        summary.push([
            format!("Storey q-value (π̂₀ = {})", format_number(self.q.pi0, 4)),
            "false discovery rate (positive)".to_owned(),
            nq.to_string(),
            format_number(largest_q, DIGITS),
        ]);
        out.push(section(
            SectionKind::PValueSummary,
            format!(
                "Multiple Testing Summary ({m} tests; rejected when the adjusted p-value is at most α = {}; \
                 Storey's λ = {})",
                format_number(alpha, 4),
                format_number(self.q.lambda, 4)
            ),
            SectionContent::Table(summary),
        ));
        // The table, one row per test.
        let mut columns = vec!["Test".to_owned(), "p-value".to_owned()];
        columns.extend(AdjustMethod::ALL.iter().map(|m| m.label().to_owned()));
        columns.push("q-value".to_owned());
        let mut t = TextTable::new(columns);
        for i in 0..m {
            let mut row = vec![self.labels[i].clone(), format_p_value(self.p[i])];
            row.extend(self.adjusted.iter().map(|a| format_p_value(a[i])));
            row.push(format_p_value(self.q.q_values[i]));
            t.push(row);
        }
        out.push(section(SectionKind::PValueAdjustments, "Adjusted p-values", SectionContent::Table(t)));
        out.push(self.combined_section());
        out.push(self.plot());
        out
    }

    fn combined_section(&self) -> ReportSection {
        let mut t = TextTable::new(["Method", "Statistic", "DF", "Combined p-value", "Tests"]);
        match &self.fisher {
            Ok(f) => t.push([
                "Fisher (−2 Σ ln p)".to_owned(),
                format!("χ² = {}", format_number(f.statistic, DIGITS)),
                format_number(f.df, 4),
                format_p_value(f.p_value),
                f.k.to_string(),
            ]),
            Err(e) => t.push([
                "Fisher".to_owned(),
                format!("not available: {e}"),
                String::new(),
                String::new(),
                String::new(),
            ]),
        }
        let name = if self.weighted { "Stouffer (weighted Z)" } else { "Stouffer (Z)" };
        match &self.stouffer {
            Ok(z) => t.push([
                name.to_owned(),
                format!("Z = {}", format_number(z.statistic, DIGITS)),
                String::new(),
                format_p_value(z.p_value),
                z.k.to_string(),
            ]),
            Err(e) => {
                t.push([name.to_owned(), format!("not available: {e}"), String::new(), String::new(), String::new()])
            }
        }
        section(
            SectionKind::CombinedPValues,
            "Combined Tests (one overall null hypothesis; the p-values are taken as independent and one-sided in \
             the same direction)",
            SectionContent::Table(t),
        )
    }

    /// Ordered p-values against their rank, with the Benjamini–Hochberg line
    /// `α·i/m`: the largest p-value below the line and every smaller one are
    /// rejected (Benjamini & Hochberg 1995).
    fn plot(&self) -> ReportSection {
        let m = self.p.len();
        let mut order: Vec<usize> = (0..m).collect();
        order.sort_by(|&a, &b| self.p[a].total_cmp(&self.p[b]));
        let points = order
            .iter()
            .enumerate()
            .map(|(rank, &i)| RowPoint { row: self.rows[i], x: (rank + 1) as f64, y: self.p[i] })
            .collect();
        let alpha = self.request.alpha;
        let spec = PlotSpec {
            x_label: "Rank".to_owned(),
            y_label: format!("p-value ({})", self.p_name),
            points,
            line: Some(vec![[0.0, 0.0], [m as f64, alpha]]),
            line_name: Some(format!("Benjamini–Hochberg line α·i/m (α = {})", format_number(alpha, 4))),
            point_name: Some("Tests (rows)".to_owned()),
            ..PlotSpec::default()
        };
        section(
            SectionKind::PValuePlot,
            "Ordered p-values (points below the line up to the last one under it are discoveries)",
            SectionContent::Plot(Box::new(spec)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    fn table() -> DataTable {
        let p = vec![0.01, 0.04, 0.03, 0.005, 0.5, 0.2, 0.04, 0.8, 0.001, 0.07, f64::NAN];
        let names: Vec<String> = (0..11).map(|i| format!("gene{i}")).collect();
        let w: Vec<f64> = (1..=11).map(f64::from).collect();
        DataTable::new("t", vec![Column::numeric("p", p), Column::text("name", names), Column::numeric("w", w)])
            .unwrap()
    }

    /// The reference values of `stat_inference::pvalues` (R `p.adjust`)
    /// reach the report.
    #[test]
    fn report_sections_and_columns() {
        let t = table();
        let rows = RowStateManager::new(11);
        let req = PValueRequest { label: Some(1), weight: Some(2), ..PValueRequest::new(0) };
        let a = PValueAnalysis::run(&t, &rows, &req).unwrap();
        let s = a.sections();
        let kinds: Vec<SectionKind> = s.iter().map(|x| x.kind).collect();
        assert_eq!(
            kinds,
            [
                SectionKind::PValueSummary,
                SectionKind::PValueAdjustments,
                SectionKind::CombinedPValues,
                SectionKind::PValuePlot
            ]
        );
        let SectionContent::Table(summary) = &s[0].content else { panic!() };
        // Unadjusted: 6 at or below 0.05; Bonferroni: 0.001 and 0.005 (0.05 counts).
        assert_eq!(summary.rows[0][2], "6");
        assert_eq!(summary.rows[1][2], "2");
        // BH: 0.033, 0.067, 0.067, 0.025, …, 0.01 → three at or below 0.05.
        assert_eq!(summary.rows[5][2], "3");
        let SectionContent::Table(tab) = &s[1].content else { panic!() };
        assert_eq!(tab.rows.len(), 10, "the missing p-value is left out");
        assert_eq!(tab.rows[0][0], "gene0");
        assert_eq!(tab.rows[0][2], "0.1000");
        let SectionContent::Table(comb) = &s[2].content else { panic!() };
        assert!(comb.rows[1][0].contains("weighted"));
        let cols = a.saveable_columns();
        assert_eq!(cols.len(), 7);
        assert_eq!(cols[4].key, "bh");
        assert!((cols[4].values[0] - 1.0 / 30.0).abs() < 1e-12);
        assert!(cols[4].values[10].is_nan());
        let SectionContent::Plot(plot) = &s[3].content else { panic!() };
        assert_eq!(plot.points[0].row, 8, "the smallest p-value comes first");
    }

    #[test]
    fn json_round_trip_and_errors() {
        let t = table();
        let req = PValueRequest { label: Some(1), alpha: 0.1, ..PValueRequest::new(0) };
        let back = PValueRequest::from_json(&req.to_json(&t), &t).unwrap();
        assert_eq!(back, req);
        let mut bad = t.clone();
        bad.replace_column(0, Column::numeric("p", vec![1.5; 11])).unwrap();
        assert!(PValueAnalysis::run(&bad, &RowStateManager::new(11), &PValueRequest::new(0)).is_err());
        assert!(PValueAnalysis::run(&t, &RowStateManager::new(11), &PValueRequest::new(1)).is_err());
    }
}
