//! Permutation tests on table columns ([`stat_inference::permutation`]): two
//! independent groups (difference of means or of medians), paired columns
//! (sign-flip test of the mean difference) and the correlation of two
//! columns (Pearson or Spearman). Exact enumeration when the arrangements are
//! few, seeded Monte Carlo otherwise.

use crate::format::{format_number, format_p_value};
use crate::project::{invalid, member, obj, s};
use crate::report::{BarChartSpec, BarItem, ReportSection, SectionContent, SectionKind, TextTable};
use stat_core::Result;
use stat_data::{DataTable, JsonValue, RowStateManager};
use stat_inference::Alternative;
pub use stat_inference::permutation::{CorrelationStatistic, GroupStatistic};
use stat_inference::permutation::{PermutationOptions, PermutationTest, correlation, paired, two_groups};

/// Significant digits in the report.
const DIGITS: usize = 6;

/// Most random arrangements a report may request.
pub const MAX_RESAMPLES: usize = 1_000_000;

/// What is permuted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermutationDesign {
    /// A response column split into two groups by a grouping column (the
    /// groups in natural order; the statistic is first − second).
    TwoGroups {
        /// Response column.
        response: usize,
        /// Grouping column with exactly two levels.
        group: usize,
        /// Difference of means or of medians.
        statistic: GroupStatistic,
    },
    /// Two columns measured on the same rows; the differences `first − second`
    /// are tested by flipping their signs.
    Paired {
        /// First column.
        first: usize,
        /// Second column.
        second: usize,
    },
    /// Two columns tested for association by permuting their pairing.
    Correlation {
        /// First column.
        x: usize,
        /// Second column.
        y: usize,
        /// Pearson or Spearman correlation.
        statistic: CorrelationStatistic,
    },
}

/// A permutation test request.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PermutationRequest {
    /// Columns and statistic.
    pub design: PermutationDesign,
    /// Alternative hypothesis about the statistic.
    pub alternative: Alternative,
    /// Random arrangements when not exact.
    pub resamples: usize,
    /// Seed of the random arrangements.
    pub seed: u64,
    /// Enumerate every arrangement when there are at most this many.
    pub exact_limit: u64,
}

impl PermutationRequest {
    /// A two-sided request with 9999 random arrangements, seed 1, exact up to
    /// 100 000 arrangements.
    #[must_use]
    pub fn new(design: PermutationDesign) -> Self {
        let d = PermutationOptions::default();
        Self { design, alternative: d.alternative, resamples: d.resamples, seed: d.seed, exact_limit: d.exact_limit }
    }

    /// Columns used.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        match self.design {
            PermutationDesign::TwoGroups { response, group, .. } => vec![response, group],
            PermutationDesign::Paired { first, second } => vec![first, second],
            PermutationDesign::Correlation { x, y, .. } => vec![x, y],
        }
    }

    /// The same request with columns renumbered by `f`.
    #[must_use]
    pub fn map_columns(mut self, f: impl Fn(usize) -> usize) -> Self {
        match &mut self.design {
            PermutationDesign::TwoGroups { response, group, .. } => {
                *response = f(*response);
                *group = f(*group);
            }
            PermutationDesign::Paired { first, second } => {
                *first = f(*first);
                *second = f(*second);
            }
            PermutationDesign::Correlation { x, y, .. } => {
                *x = f(*x);
                *y = f(*y);
            }
        }
        self
    }

    /// The project-file form (`"analysis": "permutation-test"`).
    #[must_use]
    pub fn to_json(&self, table: &DataTable) -> JsonValue {
        let col = |i: usize| s(table.column(i).map_or("", |c| c.name.as_str()));
        let (design, a, b, a_key, b_key, statistic) = match self.design {
            PermutationDesign::TwoGroups { response, group, statistic } => (
                "two-groups",
                response,
                group,
                "response",
                "group",
                match statistic {
                    GroupStatistic::MeanDifference => "mean",
                    GroupStatistic::MedianDifference => "median",
                },
            ),
            PermutationDesign::Paired { first, second } => ("paired", first, second, "first", "second", "mean"),
            PermutationDesign::Correlation { x, y, statistic } => (
                "correlation",
                x,
                y,
                "x",
                "y",
                match statistic {
                    CorrelationStatistic::Pearson => "pearson",
                    CorrelationStatistic::Spearman => "spearman",
                },
            ),
        };
        obj([
            ("analysis", s("permutation-test")),
            ("design", s(design)),
            (a_key, col(a)),
            (b_key, col(b)),
            ("statistic", s(statistic)),
            (
                "alternative",
                s(match self.alternative {
                    Alternative::TwoSided => "two-sided",
                    Alternative::Less => "less",
                    Alternative::Greater => "greater",
                }),
            ),
            ("resamples", JsonValue::Number(self.resamples as f64)),
            ("seed", JsonValue::Number(self.seed as f64)),
            ("exact_limit", JsonValue::Number(self.exact_limit as f64)),
        ])
    }

    /// Read the project-file form.
    ///
    /// # Errors
    /// A missing member, an unknown column or setting.
    pub fn from_json(v: &JsonValue, table: &DataTable) -> Result<Self> {
        let col = |key: &str| -> Result<usize> {
            let name = member(v, key)?.as_str().ok_or_else(|| invalid(format!("{key}: expected a column name")))?;
            table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")))
        };
        let statistic = v.get("statistic").and_then(JsonValue::as_str);
        let design = match member(v, "design")?.as_str() {
            Some("two-groups") => PermutationDesign::TwoGroups {
                response: col("response")?,
                group: col("group")?,
                statistic: match statistic {
                    None | Some("mean") => GroupStatistic::MeanDifference,
                    Some("median") => GroupStatistic::MedianDifference,
                    Some(other) => return Err(invalid(format!("unknown two-group statistic {other:?}"))),
                },
            },
            Some("paired") => PermutationDesign::Paired { first: col("first")?, second: col("second")? },
            Some("correlation") => PermutationDesign::Correlation {
                x: col("x")?,
                y: col("y")?,
                statistic: match statistic {
                    None | Some("pearson") => CorrelationStatistic::Pearson,
                    Some("spearman") => CorrelationStatistic::Spearman,
                    Some(other) => return Err(invalid(format!("unknown correlation {other:?}"))),
                },
            },
            other => return Err(invalid(format!("unknown permutation design {other:?}"))),
        };
        let alternative = match v.get("alternative").and_then(JsonValue::as_str) {
            None | Some("two-sided") => Alternative::TwoSided,
            Some("less") => Alternative::Less,
            Some("greater") => Alternative::Greater,
            Some(other) => return Err(invalid(format!("unknown alternative {other:?}"))),
        };
        let d = Self::new(design);
        Ok(Self {
            design,
            alternative,
            resamples: v.get("resamples").and_then(JsonValue::as_usize).unwrap_or(d.resamples),
            seed: v.get("seed").and_then(crate::seed::seed_from_json).unwrap_or(d.seed),
            exact_limit: v.get("exact_limit").and_then(JsonValue::as_f64).map_or(d.exact_limit, |x| x as u64),
        })
    }
}

fn numeric(table: &DataTable, i: usize) -> Result<(&str, &[f64])> {
    let c = table.column(i).ok_or_else(|| invalid(format!("no column {i}")))?;
    let v = c.as_numeric().ok_or_else(|| invalid(format!("{} must be numeric", c.name)))?;
    Ok((c.name.as_str(), v))
}

/// A computed permutation test.
#[derive(Debug, Clone)]
pub struct PermutationAnalysis {
    request: PermutationRequest,
    title: String,
    /// What the statistic compares, e.g. "Mean of A − mean of B".
    comparison: String,
    /// Sample sizes, labelled.
    sizes: Vec<(String, usize)>,
    test: PermutationTest,
    n_excluded: usize,
}

impl PermutationAnalysis {
    /// Run the test on the non-excluded rows with values in both columns.
    ///
    /// # Errors
    /// Repeated or non-numeric columns, a grouping column without exactly two
    /// levels, too few rows, constant columns (correlation), or settings out
    /// of range (1 to [`MAX_RESAMPLES`] arrangements).
    pub fn run(table: &DataTable, states: &RowStateManager, request: &PermutationRequest) -> Result<Self> {
        if !(1..=MAX_RESAMPLES).contains(&request.resamples) {
            return Err(invalid(format!("the number of random arrangements must be from 1 to {MAX_RESAMPLES}")));
        }
        let cols = request.columns();
        if cols[0] == cols[1] {
            return Err(invalid("choose two different columns"));
        }
        let options = PermutationOptions {
            alternative: request.alternative,
            resamples: request.resamples,
            seed: request.seed,
            exact_limit: request.exact_limit,
        };
        let n_excluded = (0..table.n_rows()).filter(|&r| states.is_excluded(r)).count();
        let (title, comparison, sizes, test) = match request.design {
            PermutationDesign::TwoGroups { response, group, statistic } => {
                let (labels, members) = crate::compare::split_by_group(table, states, response, Some(group))?;
                if labels.len() != 2 {
                    return Err(invalid(format!(
                        "the grouping column must have exactly two levels among the rows used (it has {})",
                        labels.len()
                    )));
                }
                let values = |g: usize| members[g].iter().map(|m| m.1).collect::<Vec<_>>();
                let (a, b) = (values(0), values(1));
                let name = table.column(response).map_or("", |c| c.name.as_str());
                let group_name = table.column(group).map_or("", |c| c.name.as_str());
                let what = match statistic {
                    GroupStatistic::MeanDifference => "mean",
                    GroupStatistic::MedianDifference => "median",
                };
                (
                    format!("Permutation Test: {name} by {group_name}"),
                    format!("{what} of {} − {what} of {}", labels[0], labels[1]),
                    vec![(labels[0].clone(), a.len()), (labels[1].clone(), b.len())],
                    two_groups(&a, &b, statistic, &options)?,
                )
            }
            PermutationDesign::Paired { first, second } => {
                let (fa, a) = numeric(table, first)?;
                let (fb, b) = numeric(table, second)?;
                let d: Vec<f64> = (0..table.n_rows())
                    .filter(|&r| !states.is_excluded(r) && a[r].is_finite() && b[r].is_finite())
                    .map(|r| a[r] - b[r])
                    .collect();
                (
                    format!("Permutation Test: {fa} − {fb} (paired)"),
                    format!("mean of {fa} − {fb}"),
                    vec![("Pairs".to_owned(), d.len())],
                    paired(&d, &options)?,
                )
            }
            PermutationDesign::Correlation { x, y, statistic } => {
                let (fx, xv) = numeric(table, x)?;
                let (fy, yv) = numeric(table, y)?;
                let rows: Vec<usize> = (0..table.n_rows())
                    .filter(|&r| !states.is_excluded(r) && xv[r].is_finite() && yv[r].is_finite())
                    .collect();
                let xs: Vec<f64> = rows.iter().map(|&r| xv[r]).collect();
                let ys: Vec<f64> = rows.iter().map(|&r| yv[r]).collect();
                (
                    format!("Permutation Test: {fy} with {fx}"),
                    format!("{} of {fx} and {fy}", statistic.label().to_lowercase()),
                    vec![("Pairs".to_owned(), rows.len())],
                    correlation(&xs, &ys, statistic, &options)?,
                )
            }
        };
        Ok(Self { request: *request, title, comparison, sizes, test, n_excluded })
    }

    /// The test result.
    #[must_use]
    pub fn test(&self) -> &PermutationTest {
        &self.test
    }

    /// The request this analysis answers.
    #[must_use]
    pub fn request(&self) -> &PermutationRequest {
        &self.request
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        self.title.clone()
    }

    /// Report sections: the test table, the null distribution with the
    /// observed value marked, and a note.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let t = &self.test;
        let mut table = TextTable::new(["Item", "Value"]);
        let mut row = |a: &str, b: String| table.push([a.to_owned(), b]);
        row("Statistic", format!("{}: {}", t.statistic_name, self.comparison));
        row("Observed value", format_number(t.observed, DIGITS));
        row(&format!("p-value ({})", t.alternative.label()), format_p_value(t.p_value));
        row("Share of arrangements at or below observed", format_number(t.lower_tail, DIGITS));
        row("Share of arrangements at or above observed", format_number(t.upper_tail, DIGITS));
        row(
            "Method",
            if t.exact {
                format!("exact: all {} arrangements enumerated", t.evaluated())
            } else {
                format!("Monte Carlo: {} random arrangements, seed {}", t.evaluated(), self.request.seed)
            },
        );
        row("Possible arrangements", format_number(t.arrangements, DIGITS));
        for (label, n) in &self.sizes {
            row(&format!("N ({label})"), n.to_string());
        }
        if self.n_excluded > 0 {
            row("Rows excluded", self.n_excluded.to_string());
        }
        let tagged: Vec<(usize, f64)> = t.null_values.iter().copied().enumerate().collect();
        let bars = crate::distribution::histogram_bars(&tagged)
            .into_iter()
            .map(|b| BarItem { rows: Vec::new(), unlinked: b.rows.len(), ..b })
            .collect();
        let chart = BarChartSpec {
            x_label: t.statistic_name.to_owned(),
            y_label: "Arrangements".to_owned(),
            bars,
            x_ticks: None,
            curve: None,
            references: vec![(t.observed, "Observed".to_owned())],
            x_time: None,
        };
        let note = if t.exact {
            "Every rearrangement the null hypothesis makes equally likely was evaluated, so the p-value is exact: \
             the share of arrangements at least as extreme as the one observed (two-sided: twice the smaller \
             tail, at most 1)."
                .to_owned()
        } else {
            format!(
                "There are more than {} arrangements, so {} were drawn at random (seed {}). Each tail is \
                 (b + 1)/(B + 1), b counting the random arrangements at least as extreme as the observed one, so \
                 the p-value is never zero; two-sided p is twice the smaller tail, at most 1.",
                self.request.exact_limit,
                t.evaluated(),
                self.request.seed
            )
        };
        vec![
            ReportSection {
                kind: SectionKind::PermutationTest,
                title: "Permutation Test".to_owned(),
                content: SectionContent::Table(table),
            },
            ReportSection {
                kind: SectionKind::PermutationDistribution,
                title: "Permutation Distribution".to_owned(),
                content: SectionContent::Bars(chart),
            },
            ReportSection {
                kind: SectionKind::PermutationNote,
                title: "About This Test".to_owned(),
                content: SectionContent::Note(note),
            },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    fn table() -> DataTable {
        DataTable::new(
            "t",
            vec![
                Column::numeric("y", vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, f64::NAN]),
                Column::text("g", ["a", "a", "a", "b", "b", "b", "b", "b"].map(String::from).to_vec()),
                Column::numeric("z", vec![1.5, 2.1, 2.9, 4.6, 5.2, 5.9, 7.7, 8.0]),
                Column::text("h", ["a", "b", "c", "a", "b", "c", "a", "b"].map(String::from).to_vec()),
            ],
        )
        .unwrap()
    }

    #[test]
    fn three_designs_run_and_round_trip() {
        let t = table();
        let rows = RowStateManager::new(t.n_rows());
        let designs = [
            PermutationDesign::TwoGroups { response: 0, group: 1, statistic: GroupStatistic::MeanDifference },
            PermutationDesign::TwoGroups { response: 0, group: 1, statistic: GroupStatistic::MedianDifference },
            PermutationDesign::Paired { first: 2, second: 0 },
            PermutationDesign::Correlation { x: 0, y: 2, statistic: CorrelationStatistic::Spearman },
        ];
        for d in designs {
            let mut req = PermutationRequest::new(d);
            req.alternative = Alternative::Less;
            let a = PermutationAnalysis::run(&t, &rows, &req).unwrap();
            assert!(a.test().exact);
            assert_eq!(a.sections().len(), 3);
            assert_eq!(PermutationRequest::from_json(&req.to_json(&t), &t).unwrap(), req);
        }
        // Groups a = {1, 2, 3}, b = {4, 5, 6, 7}: the observed split is the most
        // extreme of the C(7, 3) = 35, so the lower tail is 1/35.
        let a = PermutationAnalysis::run(&t, &rows, &PermutationRequest::new(designs[0])).unwrap();
        assert!((a.test().lower_tail - 1.0 / 35.0).abs() < 1e-12);
        let SectionContent::Table(tab) = &a.sections()[0].content else { panic!() };
        assert_eq!(tab.cell(1, "Value"), Some("-3.5"));
        // Monte Carlo when the limit is lowered.
        let mc = PermutationRequest { exact_limit: 10, resamples: 500, ..PermutationRequest::new(designs[0]) };
        let b = PermutationAnalysis::run(&t, &rows, &mc).unwrap();
        assert!(!b.test().exact && b.test().evaluated() == 500);
    }

    /// The largest seed a dialog accepts saves and reopens exactly, through
    /// the project file's text; a larger one in a file is clamped to it.
    #[test]
    fn seeds_save_exactly() {
        use crate::seed::MAX_SEED;
        let t = table();
        let design = PermutationDesign::Paired { first: 2, second: 0 };
        let req = PermutationRequest { seed: MAX_SEED, ..PermutationRequest::new(design) };
        let text = req.to_json(&t).to_compact();
        let back = PermutationRequest::from_json(&stat_data::parse_json(&text).unwrap(), &t).unwrap();
        assert_eq!(back, req);
        let big = PermutationRequest { seed: u64::MAX, ..req };
        assert_eq!(PermutationRequest::from_json(&big.to_json(&t), &t).unwrap().seed, MAX_SEED);
    }

    #[test]
    fn rejects_bad_requests() {
        let t = table();
        let rows = RowStateManager::new(t.n_rows());
        let three_levels =
            PermutationDesign::TwoGroups { response: 0, group: 3, statistic: GroupStatistic::MeanDifference };
        assert!(PermutationAnalysis::run(&t, &rows, &PermutationRequest::new(three_levels)).is_err());
        let same = PermutationDesign::Paired { first: 0, second: 0 };
        assert!(PermutationAnalysis::run(&t, &rows, &PermutationRequest::new(same)).is_err());
        let text = PermutationDesign::Correlation { x: 1, y: 0, statistic: CorrelationStatistic::Pearson };
        assert!(PermutationAnalysis::run(&t, &rows, &PermutationRequest::new(text)).is_err());
        let zero = PermutationRequest {
            resamples: 0,
            ..PermutationRequest::new(PermutationDesign::Paired { first: 0, second: 2 })
        };
        assert!(PermutationAnalysis::run(&t, &rows, &zero).is_err());
    }
}
