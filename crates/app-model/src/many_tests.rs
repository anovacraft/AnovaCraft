//! Many two-group tests at once: the same test of two groups applied to each
//! of many variables (columns), with the p-values adjusted for multiple
//! testing ([`stat_inference::pvalues`]) and a volcano plot of each
//! variable's difference against `−log₁₀ p`.
//!
//! The grouping column must have exactly two values among the rows used;
//! the first in natural order is the first group, so differences are
//! first − second. Each variable uses the rows (not excluded) that have
//! both a value and a group. The tests are the pooled or Welch two-sample t
//! test ([`stat_inference::means::two_sample_t`], difference of means) or the
//! Wilcoxon rank-sum test ([`stat_inference::nonparametric::rank_sum_test`],
//! Hodges–Lehmann shift), two-sided.

use crate::compare::{DIGITS, section};
use crate::format::{format_number, format_p_value};
use crate::project::{invalid, member, obj, s};
use crate::report::{Annotations, PlotSpec, ReportSection, SectionContent, SectionKind, TextTable};
use stat_core::Result;
use stat_data::{DataTable, JsonValue, RowStateManager, natural_order};
use stat_inference::Alternative;
use stat_inference::means::two_sample_t;
use stat_inference::nonparametric::rank_sum_test;
use stat_inference::pvalues::{AdjustMethod, adjust_p_values};

/// Which test is applied to every variable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ManyTestKind {
    /// Welch's t test (unequal variances).
    WelchT,
    /// Pooled-variance t test.
    PooledT,
    /// Wilcoxon rank-sum (Mann–Whitney) test.
    RankSum,
}

impl ManyTestKind {
    /// Every test, in menu order.
    pub const ALL: [Self; 3] = [Self::WelchT, Self::PooledT, Self::RankSum];

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::WelchT => "Welch t (unequal variances)",
            Self::PooledT => "Pooled t (equal variances)",
            Self::RankSum => "Rank-sum (Wilcoxon / Mann–Whitney)",
        }
    }

    /// Identifier in project files and scripts.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::WelchT => "welch",
            Self::PooledT => "pooled",
            Self::RankSum => "rank-sum",
        }
    }
}

/// Order of the rows of the results table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ManyTestsOrder {
    /// As the variables were chosen.
    Columns,
    /// Smallest p-value first.
    PValue,
    /// Largest absolute difference first.
    Difference,
}

impl ManyTestsOrder {
    /// Every order, in menu order.
    pub const ALL: [Self; 3] = [Self::Columns, Self::PValue, Self::Difference];

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Columns => "Column order",
            Self::PValue => "Smallest p-value first",
            Self::Difference => "Largest difference first",
        }
    }

    /// Identifier in project files and scripts.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Columns => "columns",
            Self::PValue => "p-value",
            Self::Difference => "difference",
        }
    }
}

/// Settings of a many-tests report.
#[derive(Debug, Clone, PartialEq)]
pub struct ManyTestsRequest {
    /// Variables (numeric columns), one test each.
    pub columns: Vec<usize>,
    /// Grouping column with two values.
    pub group: usize,
    /// The test.
    pub test: ManyTestKind,
    /// Multiple-testing adjustment.
    pub adjust: AdjustMethod,
    /// Error rate at which adjusted p-values count as significant.
    pub alpha: f64,
    /// Order of the results table.
    pub order: ManyTestsOrder,
}

impl ManyTestsRequest {
    /// Defaults: Welch t, Benjamini–Hochberg at `α = 0.05`, column order.
    #[must_use]
    pub fn new(columns: Vec<usize>, group: usize) -> Self {
        Self {
            columns,
            group,
            test: ManyTestKind::WelchT,
            adjust: AdjustMethod::BenjaminiHochberg,
            alpha: 0.05,
            order: ManyTestsOrder::Columns,
        }
    }

    /// Every column the request uses.
    #[must_use]
    pub fn all_columns(&self) -> Vec<usize> {
        self.columns.iter().copied().chain([self.group]).collect()
    }

    /// The request with its columns renumbered by `f`.
    #[must_use]
    pub fn map_columns(&self, f: &dyn Fn(usize) -> usize) -> Self {
        Self { columns: self.columns.iter().map(|&c| f(c)).collect(), group: f(self.group), ..self.clone() }
    }

    /// Project-file form.
    #[must_use]
    pub fn to_json(&self, table: &DataTable) -> JsonValue {
        let col = |i: usize| s(table.column(i).map_or("", |c| c.name.as_str()));
        obj([
            ("analysis", s("many-tests")),
            ("columns", JsonValue::Array(self.columns.iter().map(|&c| col(c)).collect())),
            ("group", col(self.group)),
            ("test", s(self.test.key())),
            ("adjust", s(self.adjust.key())),
            ("alpha", JsonValue::number(self.alpha)),
            ("order", s(self.order.key())),
        ])
    }

    /// Read the project-file form.
    ///
    /// # Errors
    /// Missing columns, unknown names or an `α` outside `(0, 1)`.
    pub fn from_json(v: &JsonValue, table: &DataTable) -> Result<Self> {
        let find =
            |name: &str| table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")));
        let columns = member(v, "columns")?
            .as_array()
            .ok_or_else(|| invalid("columns: expected a list of column names"))?
            .iter()
            .map(|c| find(c.as_str().ok_or_else(|| invalid("columns: expected column names"))?))
            .collect::<Result<Vec<_>>>()?;
        let group = find(member(v, "group")?.as_str().ok_or_else(|| invalid("group: expected a column name"))?)?;
        let name = |key: &str, default: &str| v.get(key).and_then(JsonValue::as_str).unwrap_or(default).to_owned();
        let test_name = name("test", "welch");
        let test = ManyTestKind::ALL
            .into_iter()
            .find(|t| t.key() == test_name)
            .ok_or_else(|| invalid(format!("many tests: unknown test {test_name:?}")))?;
        let adjust_name = name("adjust", "bh");
        let adjust = AdjustMethod::from_key(&adjust_name)
            .ok_or_else(|| invalid(format!("many tests: unknown adjustment {adjust_name:?}")))?;
        let order_name = name("order", "columns");
        let order = ManyTestsOrder::ALL
            .into_iter()
            .find(|o| o.key() == order_name)
            .ok_or_else(|| invalid(format!("many tests: unknown order {order_name:?}")))?;
        let alpha = v.get("alpha").and_then(JsonValue::as_f64).unwrap_or(0.05);
        if !(alpha > 0.0 && alpha < 1.0) {
            return Err(invalid("many tests: alpha must lie between 0 and 1"));
        }
        Ok(Self { columns, group, test, adjust, alpha, order })
    }
}

/// The test of one variable.
#[derive(Debug, Clone)]
pub struct VariableTest {
    /// Variable name.
    pub name: String,
    /// Group sizes.
    pub n: (usize, usize),
    /// Group means.
    pub means: (f64, f64),
    /// Difference of means (t tests) or Hodges–Lehmann shift (rank-sum).
    pub difference: f64,
    /// Test statistic (`t` or `U`).
    pub statistic: f64,
    /// Degrees of freedom (t tests).
    pub df: Option<f64>,
    /// Two-sided p-value.
    pub p_value: f64,
    /// Adjusted p-value.
    pub adjusted: f64,
    /// Why the test could not be run.
    pub error: Option<String>,
}

/// A computed many-tests report.
#[derive(Debug, Clone)]
pub struct ManyTestsAnalysis {
    request: ManyTestsRequest,
    group_name: String,
    levels: [String; 2],
    tests: Vec<VariableTest>,
}

impl ManyTestsAnalysis {
    /// Run the test on every variable and adjust the p-values.
    ///
    /// # Errors
    /// No variables, a variable that is the grouping column or is not
    /// numeric, or a grouping column without exactly two values.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &ManyTestsRequest) -> Result<Self> {
        if request.columns.is_empty() {
            return Err(invalid("choose at least one variable to test"));
        }
        if request.columns.contains(&request.group) {
            return Err(invalid("the grouping column cannot also be a variable"));
        }
        let g = table.column(request.group).ok_or_else(|| invalid("no such column"))?;
        let group_of: Vec<String> = (0..table.n_rows()).map(|r| g.level_text(r)).collect();
        let mut levels: Vec<String> = Vec::new();
        for r in (0..table.n_rows()).filter(|&r| !states.is_excluded(r) && !group_of[r].is_empty()) {
            if !levels.contains(&group_of[r]) {
                levels.push(group_of[r].clone());
            }
        }
        levels.sort_by(|a, b| natural_order(a, b));
        let levels: [String; 2] = levels.try_into().map_err(|l: Vec<String>| {
            invalid(format!("the grouping column {} must have exactly two values (found {})", g.name, l.len()))
        })?;
        let mut tests = Vec::with_capacity(request.columns.len());
        for &c in &request.columns {
            let col = table.column(c).ok_or_else(|| invalid("no such column"))?;
            let v = col.as_numeric().ok_or_else(|| invalid(format!("{} must be numeric", col.name)))?;
            let (mut a, mut b) = (Vec::new(), Vec::new());
            for r in (0..table.n_rows()).filter(|&r| !states.is_excluded(r) && !v[r].is_nan()) {
                if group_of[r] == levels[0] {
                    a.push(v[r]);
                } else if group_of[r] == levels[1] {
                    b.push(v[r]);
                }
            }
            let mean = |x: &[f64]| if x.is_empty() { f64::NAN } else { x.iter().sum::<f64>() / x.len() as f64 };
            let r = match request.test {
                ManyTestKind::WelchT => two_sample_t(&a, &b, 0.0, false, 0.95, Alternative::TwoSided),
                ManyTestKind::PooledT => two_sample_t(&a, &b, 0.0, true, 0.95, Alternative::TwoSided),
                ManyTestKind::RankSum => rank_sum_test(&a, &b, 0.0, 0.95, Alternative::TwoSided),
            };
            let base = VariableTest {
                name: col.name.clone(),
                n: (a.len(), b.len()),
                means: (mean(&a), mean(&b)),
                difference: f64::NAN,
                statistic: f64::NAN,
                df: None,
                p_value: f64::NAN,
                adjusted: f64::NAN,
                error: None,
            };
            tests.push(match r {
                Ok(t) => VariableTest {
                    difference: t.estimate,
                    statistic: t.statistic,
                    df: t.df,
                    p_value: t.p_value,
                    ..base
                },
                Err(e) => VariableTest { error: Some(e.to_string()), ..base },
            });
        }
        let p: Vec<f64> = tests.iter().map(|t| t.p_value).collect();
        for (t, q) in tests.iter_mut().zip(adjust_p_values(&p, request.adjust)?) {
            t.adjusted = q;
        }
        Ok(Self { request: request.clone(), group_name: g.name.clone(), levels, tests })
    }

    /// The tests, in the order of the variables.
    #[must_use]
    pub fn tests(&self) -> &[VariableTest] {
        &self.tests
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        format!(
            "Many Tests: {} variables by {} ({} − {})",
            self.tests.len(),
            self.group_name,
            self.levels[0],
            self.levels[1]
        )
    }

    fn significant(&self, t: &VariableTest) -> bool {
        t.adjusted <= self.request.alpha
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let r = &self.request;
        let tested = self.tests.iter().filter(|t| t.error.is_none()).count();
        let found = self.tests.iter().filter(|t| self.significant(t)).count();
        let raw = self.tests.iter().filter(|t| t.p_value <= r.alpha).count();
        let mut summary = TextTable::new(["Setting", "Value"]);
        summary.push(["Test (two-sided)".to_owned(), r.test.label().to_owned()]);
        summary.push(["Groups".to_owned(), format!("{} − {} ({})", self.levels[0], self.levels[1], self.group_name)]);
        summary.push(["Variables tested".to_owned(), format!("{tested} of {}", self.tests.len())]);
        summary.push(["Adjustment".to_owned(), format!("{} (controls the {})", r.adjust.label(), r.adjust.controls())]);
        summary.push([format!("p ≤ {} before adjustment", format_number(r.alpha, 4)), raw.to_string()]);
        summary.push([format!("Adjusted p ≤ {}", format_number(r.alpha, 4)), found.to_string()]);
        let mut out = vec![section(SectionKind::PValueSummary, "Summary", SectionContent::Table(summary))];
        let (estimate, stat) = match r.test {
            ManyTestKind::RankSum => ("Shift (Hodges–Lehmann)", "U"),
            _ => ("Difference", "t"),
        };
        let mut t = TextTable::new([
            "Variable".to_owned(),
            format!("N {}", self.levels[0]),
            format!("N {}", self.levels[1]),
            format!("Mean {}", self.levels[0]),
            format!("Mean {}", self.levels[1]),
            estimate.to_owned(),
            stat.to_owned(),
            "DF".to_owned(),
            "p-value".to_owned(),
            format!("Adjusted p ({})", r.adjust.label()),
            "−log₁₀ p".to_owned(),
            "Significant".to_owned(),
        ]);
        let mut order: Vec<usize> = (0..self.tests.len()).collect();
        match r.order {
            ManyTestsOrder::Columns => {}
            ManyTestsOrder::PValue => {
                order.sort_by(|&a, &b| nan_last(self.tests[a].p_value, self.tests[b].p_value));
            }
            ManyTestsOrder::Difference => {
                order.sort_by(|&a, &b| nan_last(-self.tests[a].difference.abs(), -self.tests[b].difference.abs()))
            }
        }
        for &i in &order {
            let v = &self.tests[i];
            let mut row = vec![
                v.name.clone(),
                v.n.0.to_string(),
                v.n.1.to_string(),
                format_number(v.means.0, DIGITS),
                format_number(v.means.1, DIGITS),
            ];
            match &v.error {
                Some(e) => {
                    row.push(format!("not available: {e}"));
                    row.extend((0..6).map(|_| String::new()));
                }
                None => row.extend([
                    format_number(v.difference, DIGITS),
                    format_number(v.statistic, DIGITS),
                    v.df.map_or_else(String::new, |d| format_number(d, 4)),
                    format_p_value(v.p_value),
                    format_p_value(v.adjusted),
                    format_number(-v.p_value.log10(), 4),
                    if self.significant(v) { "yes".to_owned() } else { "no".to_owned() },
                ]),
            }
            t.push(row);
        }
        out.push(section(
            SectionKind::ManyTestsResults,
            format!("Tests by Variable ({})", r.order.label().to_lowercase()),
            SectionContent::Table(t),
        ));
        out.push(self.volcano());
        out
    }

    /// Difference against `−log₁₀ p`, significant variables marked and named
    /// (the 25 smallest p-values among them).
    fn volcano(&self) -> ReportSection {
        let r = &self.request;
        let ok: Vec<&VariableTest> = self.tests.iter().filter(|t| t.error.is_none()).collect();
        let mut sig: Vec<&VariableTest> = ok.iter().copied().filter(|t| self.significant(t)).collect();
        sig.sort_by(|a, b| a.p_value.total_cmp(&b.p_value));
        let named: Vec<&str> = sig.iter().take(25).map(|t| t.name.as_str()).collect();
        let y = |t: &VariableTest| -t.p_value.max(1e-300).log10();
        let items = ok
            .iter()
            .map(|t| {
                let label = if named.contains(&t.name.as_str()) { t.name.clone() } else { String::new() };
                (t.difference, y(t), label, usize::from(self.significant(t)))
            })
            .collect();
        let x_max = ok.iter().map(|t| t.difference.abs()).fold(0.0_f64, f64::max).max(f64::MIN_POSITIVE);
        let y_max = ok.iter().map(|t| y(t)).fold(0.0_f64, f64::max);
        let threshold = -r.alpha.log10();
        let estimate = match r.test {
            ManyTestKind::RankSum => "Shift (Hodges–Lehmann)",
            _ => "Difference of means",
        };
        let spec = PlotSpec {
            x_label: format!("{estimate}, {} − {}", self.levels[0], self.levels[1]),
            y_label: "−log₁₀ p".to_owned(),
            line: Some(vec![[-1.05 * x_max, threshold], [1.05 * x_max, threshold]]),
            line_name: Some(format!("p = {} before adjustment", format_number(r.alpha, 4))),
            segments: vec![vec![[0.0, 0.0], [0.0, y_max.max(threshold) * 1.05]]],
            annotations: Some(Annotations {
                groups: vec![
                    "Not significant".to_owned(),
                    format!("Adjusted p ≤ {} ({})", format_number(r.alpha, 4), r.adjust.label()),
                ],
                items,
                labels: Vec::new(),
            }),
            ..PlotSpec::default()
        };
        section(
            SectionKind::VolcanoPlot,
            "Volcano Plot (one point per variable; significant variables named)",
            SectionContent::Plot(Box::new(spec)),
        )
    }
}

/// Ascending order with NaN last.
fn nan_last(a: f64, b: f64) -> std::cmp::Ordering {
    match (a.is_nan(), b.is_nan()) {
        (true, true) => std::cmp::Ordering::Equal,
        (true, false) => std::cmp::Ordering::Greater,
        (false, true) => std::cmp::Ordering::Less,
        (false, false) => a.total_cmp(&b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    /// Three variables measured in groups "ctl" and "trt" (the R reference
    /// is `tests/data/many_tests_reference.R`: `t.test`, `wilcox.test`,
    /// `p.adjust`).
    pub(crate) fn table() -> DataTable {
        let g: Vec<String> =
            ["ctl", "ctl", "ctl", "ctl", "ctl", "trt", "trt", "trt", "trt", "trt", "trt"].map(String::from).to_vec();
        DataTable::new(
            "t",
            vec![
                Column::numeric("v1", vec![5.1, 4.9, 5.6, 5.8, 6.0, 6.9, 7.2, 6.6, 7.8, 7.0, 6.4]),
                Column::numeric("v2", vec![2.0, 2.4, 1.9, 2.2, 2.5, 2.3, 2.1, 2.6, 2.0, 2.4, 2.2]),
                Column::numeric("v3", vec![10.0, 12.5, 11.2, 9.8, 10.4, 12.9, 13.4, 11.8, 12.2, 14.0, 13.1]),
                Column::text("g", g),
            ],
        )
        .unwrap()
    }

    #[test]
    fn welch_tests_and_bh_match_r() {
        let t = table();
        let rows = RowStateManager::new(11);
        let a = ManyTestsAnalysis::run(&t, &rows, &ManyTestsRequest::new(vec![0, 1, 2], 3)).unwrap();
        let p: Vec<f64> = a.tests().iter().map(|v| v.p_value).collect();
        let q: Vec<f64> = a.tests().iter().map(|v| v.adjusted).collect();
        let (rp, rq) = (REFERENCE_WELCH_P, REFERENCE_WELCH_BH);
        for i in 0..3 {
            assert!((p[i] - rp[i]).abs() < 1e-9 * rp[i].max(1e-12), "{p:?}");
            assert!((q[i] - rq[i]).abs() < 1e-9 * rq[i].max(1e-12), "{q:?}");
        }
        assert!((a.tests()[0].difference - (5.48 - 6.983_333_333_333_333)).abs() < 1e-12);
        let s = a.sections();
        assert_eq!(s.len(), 3);
        assert_eq!(s[2].kind, SectionKind::VolcanoPlot);
        let SectionContent::Plot(plot) = &s[2].content else { panic!() };
        let notes = plot.annotations.as_ref().unwrap();
        assert_eq!(notes.items.len(), 3);
        assert_eq!(notes.items[0].3, 1, "v1 differs");
        assert_eq!(notes.items[0].2, "v1");
    }

    #[test]
    fn rank_sum_and_ordering() {
        let t = table();
        let rows = RowStateManager::new(11);
        let req = ManyTestsRequest {
            test: ManyTestKind::RankSum,
            adjust: AdjustMethod::Holm,
            order: ManyTestsOrder::PValue,
            ..ManyTestsRequest::new(vec![1, 0, 2], 3)
        };
        let a = ManyTestsAnalysis::run(&t, &rows, &req).unwrap();
        let p: Vec<f64> = a.tests().iter().map(|v| v.p_value).collect();
        for (got, want) in p.iter().zip(REFERENCE_RANK_P) {
            assert!((got - want).abs() < 1e-9 * want, "{p:?}");
        }
        let s = a.sections();
        let SectionContent::Table(tab) = &s[1].content else { panic!() };
        assert_eq!(tab.rows[2][0], "v2", "largest p-value last");
        let back = ManyTestsRequest::from_json(&req.to_json(&t), &t).unwrap();
        assert_eq!(back, req);
    }

    #[test]
    fn errors() {
        let t = table();
        let rows = RowStateManager::new(11);
        assert!(ManyTestsAnalysis::run(&t, &rows, &ManyTestsRequest::new(vec![], 3)).is_err());
        assert!(ManyTestsAnalysis::run(&t, &rows, &ManyTestsRequest::new(vec![3], 3)).is_err());
        // A numeric grouping column with many values.
        assert!(ManyTestsAnalysis::run(&t, &rows, &ManyTestsRequest::new(vec![1], 0)).is_err());
    }

    // R 4.3.3 (tests/data/many_tests_reference.R).
    const REFERENCE_WELCH_P: [f64; 3] = [0.000_607_946_620_497_935, 0.656_129_525_766_328_4, 0.008_502_313_233_559_03];
    const REFERENCE_WELCH_BH: [f64; 3] =
        [0.001_823_839_861_493_805, 0.656_129_525_766_328_4, 0.012_753_469_850_338_546];
    /// v2 (ties: normal approximation with continuity correction), v1, v3
    /// (exact).
    const REFERENCE_RANK_P: [f64; 3] = [0.713_123_954_908_715_6, 0.004_329_004_329_004_329, 0.017_316_017_316_017_316];
}
