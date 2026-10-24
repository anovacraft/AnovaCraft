//! Between/within capability of one numeric column whose rows fall into
//! subgroups — batches in time order or parallel streams — given by a
//! subgroup column or by consecutive rows of a fixed size
//! ([`stat_quality::between_within`]).
//!
//! The report gives three sets of indices (within, between/within and
//! overall), the variance components that separate between- from
//! within-subgroup variation, expected and observed nonconforming rates, a
//! row-linked plot of the values by subgroup with the subgroup means and the
//! specification limits, and a histogram with the limits.

use crate::distribution::histogram_bars;
use crate::format::format_number;
use crate::project::{invalid, member, s};
use crate::report::{
    Annotations, BarChartSpec, PlotSpec, ReportSection, RowPoint, SectionContent, SectionKind, TextTable,
};
use stat_core::Result;
use stat_data::{DataTable, JsonValue, RowStateManager};
use stat_quality::{
    BetweenEstimate, BetweenWithin, BetweenWithinOptions, CapabilityIndices, SpecLimits, WithinEstimate,
    between_within_capability,
};

/// Significant digits in report tables.
const DIGITS: usize = 5;
/// Confidence level of the intervals.
pub const BETWEEN_WITHIN_LEVEL: f64 = 0.95;

/// How rows are grouped into subgroups.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubgroupBy {
    /// Rows with the same value of this column form a subgroup (in order of
    /// first appearance).
    Column(usize),
    /// Consecutive rows, this many at a time (a shorter last subgroup is kept).
    Size(usize),
}

/// What to analyse.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BetweenWithinRequest {
    /// Measurement column.
    pub column: usize,
    /// Subgroups.
    pub subgroup: SubgroupBy,
    /// Specification limits and target.
    pub limits: SpecLimits,
    /// Within-subgroup σ estimator.
    pub within: WithinEstimate,
    /// Between-subgroup σ estimator.
    pub between: BetweenEstimate,
}

impl BetweenWithinRequest {
    /// Columns used.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        match self.subgroup {
            SubgroupBy::Column(c) => vec![self.column, c],
            SubgroupBy::Size(_) => vec![self.column],
        }
    }

    /// The same request with columns renumbered by `f`.
    #[must_use]
    pub fn map_columns(mut self, f: impl Fn(usize) -> usize) -> Self {
        self.column = f(self.column);
        if let SubgroupBy::Column(c) = &mut self.subgroup {
            *c = f(*c);
        }
        self
    }

    /// The project-file form (`"analysis": "between-within-capability"`).
    #[must_use]
    pub fn to_json(&self, table: &DataTable) -> JsonValue {
        let col = |i: usize| s(table.column(i).map_or("", |c| c.name.as_str()));
        let mut m: Vec<(String, JsonValue)> =
            vec![("analysis".into(), s("between-within-capability")), ("column".into(), col(self.column))];
        match self.subgroup {
            SubgroupBy::Column(c) => m.push(("subgroup".into(), col(c))),
            SubgroupBy::Size(n) => m.push(("subgroup_size".into(), JsonValue::Number(n as f64))),
        }
        let l = &self.limits;
        for (key, v) in [("lower", l.lower), ("upper", l.upper), ("target", l.target)] {
            if let Some(v) = v {
                m.push((key.into(), JsonValue::number(v)));
            }
        }
        let within = match self.within {
            WithinEstimate::PooledStdDev => "pooled-sd",
            WithinEstimate::AverageRange => "average-range",
        };
        let between = match self.between {
            BetweenEstimate::MovingRange => "moving-range",
            BetweenEstimate::VarianceComponents => "variance-components",
        };
        m.push(("within".into(), s(within)));
        m.push(("between".into(), s(between)));
        JsonValue::Object(m)
    }

    /// Read the project-file form; missing estimators keep their defaults.
    ///
    /// # Errors
    /// Missing or unknown columns, no subgroups, unknown estimators.
    pub fn from_json(v: &JsonValue, table: &DataTable) -> Result<Self> {
        let col = |key: &str| -> Result<usize> {
            let name = member(v, key)?.as_str().ok_or_else(|| invalid(format!("{key}: expected a column name")))?;
            table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")))
        };
        let subgroup = match (v.get("subgroup"), v.get("subgroup_size").and_then(JsonValue::as_usize)) {
            (Some(JsonValue::String(_)), _) => SubgroupBy::Column(col("subgroup")?),
            (_, Some(n)) => SubgroupBy::Size(n),
            _ => return Err(invalid("between/within capability: missing subgroup column or size")),
        };
        let num = |key: &str| v.get(key).and_then(JsonValue::as_f64);
        let within = match v.get("within").and_then(JsonValue::as_str) {
            None | Some("pooled-sd") => WithinEstimate::PooledStdDev,
            Some("average-range") => WithinEstimate::AverageRange,
            Some(other) => return Err(invalid(format!("between/within capability: unknown within σ {other:?}"))),
        };
        let between = match v.get("between").and_then(JsonValue::as_str) {
            None | Some("moving-range") => BetweenEstimate::MovingRange,
            Some("variance-components") => BetweenEstimate::VarianceComponents,
            Some(other) => return Err(invalid(format!("between/within capability: unknown between σ {other:?}"))),
        };
        Ok(Self {
            column: col("column")?,
            subgroup,
            limits: SpecLimits { lower: num("lower"), upper: num("upper"), target: num("target") },
            within,
            between,
        })
    }
}

/// A computed between/within capability study.
#[derive(Debug, Clone)]
pub struct BetweenWithinAnalysis {
    request: BetweenWithinRequest,
    name: String,
    /// Subgroup labels, in subgroup order.
    labels: Vec<String>,
    /// `(row, value)` of each observation, by subgroup.
    groups: Vec<Vec<(usize, f64)>>,
    result: BetweenWithin,
    n_excluded: usize,
    n_missing: usize,
}

fn section(kind: SectionKind, title: impl Into<String>, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.into(), content }
}

/// How a `Cpk` reads against the usual 1 and 1.33 thresholds.
fn verdict(cpk: f64) -> &'static str {
    match cpk {
        k if k >= 1.33 => "capable (Cpk B/W ≥ 1.33)",
        k if k >= 1.0 => "marginal (1 ≤ Cpk B/W < 1.33)",
        _ => "not capable (Cpk B/W < 1)",
    }
}

impl BetweenWithinAnalysis {
    /// Compute the study from rows with a value (and subgroup) that are not
    /// excluded, in row order.
    ///
    /// # Errors
    /// Missing or non-numeric measurement column, a subgroup column equal to
    /// it, a subgroup size below 1, and any error of
    /// [`between_within_capability`].
    pub fn run(table: &DataTable, states: &RowStateManager, request: &BetweenWithinRequest) -> Result<Self> {
        let col = table.column(request.column).ok_or_else(|| invalid(format!("no column {}", request.column)))?;
        let values = col.as_numeric().ok_or_else(|| invalid(format!("{} must be numeric", col.name)))?;
        let key_column = match request.subgroup {
            SubgroupBy::Column(c) if c == request.column => {
                return Err(invalid("the subgroup column must differ from the measurement column"));
            }
            SubgroupBy::Column(c) => Some(table.column(c).ok_or_else(|| invalid(format!("no column {c}")))?),
            SubgroupBy::Size(0) => return Err(invalid("subgroups need at least one row")),
            SubgroupBy::Size(_) => None,
        };
        let key = |r: usize| key_column.map(|c| c.level_text(r));
        let present: Vec<usize> =
            (0..table.n_rows()).filter(|&r| values[r].is_finite() && key(r).is_none_or(|k| !k.is_empty())).collect();
        let used: Vec<usize> = present.iter().copied().filter(|&r| !states.is_excluded(r)).collect();
        let mut labels: Vec<String> = Vec::new();
        let mut groups: Vec<Vec<(usize, f64)>> = Vec::new();
        match request.subgroup {
            SubgroupBy::Column(_) => {
                for &r in &used {
                    let k = key(r).unwrap_or_default();
                    match labels.iter().position(|l| *l == k) {
                        Some(i) => groups[i].push((r, values[r])),
                        None => {
                            labels.push(k);
                            groups.push(vec![(r, values[r])]);
                        }
                    }
                }
            }
            SubgroupBy::Size(m) => {
                for (i, chunk) in used.chunks(m).enumerate() {
                    labels.push((i + 1).to_string());
                    groups.push(chunk.iter().map(|&r| (r, values[r])).collect());
                }
            }
        }
        let data: Vec<Vec<f64>> = groups.iter().map(|g| g.iter().map(|p| p.1).collect()).collect();
        let options = BetweenWithinOptions {
            within: request.within,
            between: request.between,
            confidence_level: BETWEEN_WITHIN_LEVEL,
        };
        let result = between_within_capability(&data, &request.limits, &options)?;
        Ok(Self {
            request: *request,
            name: col.name.clone(),
            labels,
            groups,
            result,
            n_excluded: present.len() - used.len(),
            n_missing: table.n_rows() - present.len(),
        })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &BetweenWithinRequest {
        &self.request
    }

    /// The computed study.
    #[must_use]
    pub fn result(&self) -> &BetweenWithin {
        &self.result
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        format!("Between/Within Capability of {}", self.name)
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        vec![
            self.indices(),
            self.components(),
            self.summary(),
            self.nonconforming(),
            self.by_subgroup(),
            self.histogram(),
        ]
    }

    fn indices(&self) -> ReportSection {
        let r = &self.result;
        let level = format!("{:.0}%", BETWEEN_WITHIN_LEVEL * 100.0);
        let mut t = TextTable::new([
            "Index".to_owned(),
            "Estimate".to_owned(),
            format!("Lower {level}"),
            format!("Upper {level}"),
        ]);
        let f = |v: f64| format_number(v, 4);
        let mut add = |set: &str, c: &CapabilityIndices, cp: &str, cpk: &str| {
            if let Some(v) = c.cp {
                let (lo, hi) = c.cp_interval.map_or((String::new(), String::new()), |(a, b)| (f(a), f(b)));
                t.push([format!("{cp} ({set})"), f(v), lo, hi]);
            }
            t.push([format!("{cpk} ({set})"), f(c.cpk), f(c.cpk_interval.0), f(c.cpk_interval.1)]);
        };
        add("within", &r.within, "Cp", "Cpk");
        add("between/within", &r.between_within, "Cp", "Cpk");
        add("overall", &r.overall, "Pp", "Ppk");
        section(
            SectionKind::BetweenWithinIndices,
            format!("Capability Indices ({})", verdict(r.between_within.cpk)),
            SectionContent::Table(t),
        )
    }

    fn components(&self) -> ReportSection {
        let r = &self.result;
        let f = |v: f64| format_number(v, DIGITS);
        let total = r.sigma_between_within * r.sigma_between_within;
        let share = |s: f64| format_number(100.0 * s * s / total, 4);
        let mut t = TextTable::new(["Source", "Standard deviation", "Variance", "% of between/within variance"]);
        t.push([
            "Between subgroups".to_owned(),
            f(r.sigma_between),
            f(r.sigma_between.powi(2)),
            share(r.sigma_between),
        ]);
        t.push(["Within subgroups".to_owned(), f(r.sigma_within), f(r.sigma_within.powi(2)), share(r.sigma_within)]);
        t.push(["Between/within".to_owned(), f(r.sigma_between_within), f(total), "100".to_owned()]);
        t.push(["Overall (sample SD)".to_owned(), f(r.sigma_overall), f(r.sigma_overall.powi(2)), String::new()]);
        section(SectionKind::BetweenWithinComponents, "Variance Components", SectionContent::Table(t))
    }

    fn summary(&self) -> ReportSection {
        let r = &self.result;
        let f = |v: f64| format_number(v, DIGITS);
        let opt = |v: Option<f64>| v.map_or_else(|| "—".to_owned(), f);
        let l = &self.request.limits;
        let mut t = TextTable::new(["Statistic", "Value"]);
        t.push(["Lower specification limit".to_owned(), opt(l.lower)]);
        t.push(["Target".to_owned(), opt(l.target)]);
        t.push(["Upper specification limit".to_owned(), opt(l.upper)]);
        t.push(["Observations".to_owned(), r.n.to_string()]);
        t.push(["Subgroups".to_owned(), r.subgroups.to_string()]);
        if self.n_excluded > 0 {
            t.push(["Rows excluded".to_owned(), self.n_excluded.to_string()]);
        }
        if self.n_missing > 0 {
            t.push(["Rows without a value".to_owned(), self.n_missing.to_string()]);
        }
        t.push(["Mean".to_owned(), f(r.mean)]);
        let (within, between) = match self.request.between {
            BetweenEstimate::MovingRange => (
                match self.request.within {
                    WithinEstimate::PooledStdDev => "pooled SD ÷ c₄",
                    WithinEstimate::AverageRange => "average of Rᵢ ÷ d₂(nᵢ)",
                },
                "moving range of subgroup means ÷ d₂(2)",
            ),
            BetweenEstimate::VarianceComponents => ("√(mean square within)", "one-way variance components"),
        };
        t.push(["Within σ from".to_owned(), within.to_owned()]);
        t.push(["Between σ from".to_owned(), between.to_owned()]);
        let (size_name, sd_name) = match self.request.between {
            BetweenEstimate::MovingRange => ("Harmonic mean subgroup size ñ", "σ of subgroup means (MR̄ ÷ d₂(2))"),
            BetweenEstimate::VarianceComponents => ("Effective subgroup size n₀", "SD of subgroup means"),
        };
        t.push([sd_name.to_owned(), f(r.sigma_means)]);
        t.push([size_name.to_owned(), f(r.effective_size)]);
        section(SectionKind::BetweenWithinSummary, "Process Summary", SectionContent::Table(t))
    }

    fn nonconforming(&self) -> ReportSection {
        let r = &self.result;
        let mut t = TextTable::new([
            "Portion",
            "Observed (ppm)",
            "Expected within (ppm)",
            "Expected between/within (ppm)",
            "Expected overall (ppm)",
        ]);
        let f = |v: f64| format_number(v, 4);
        let sum = |p: (f64, f64)| p.0 + p.1;
        let (w, b, o) = (r.within.expected_ppm, r.between_within.expected_ppm, r.overall.expected_ppm);
        t.push(["Below LSL".to_owned(), f(r.observed_ppm.0), f(w.0), f(b.0), f(o.0)]);
        t.push(["Above USL".to_owned(), f(r.observed_ppm.1), f(w.1), f(b.1), f(o.1)]);
        t.push(["Total".to_owned(), f(sum(r.observed_ppm)), f(sum(w)), f(sum(b)), f(sum(o))]);
        section(
            SectionKind::CapabilityNonconforming,
            "Nonconforming Parts (expected rates assume a normal distribution)",
            SectionContent::Table(t),
        )
    }

    fn by_subgroup(&self) -> ReportSection {
        let points: Vec<RowPoint> = self
            .groups
            .iter()
            .enumerate()
            .flat_map(|(i, g)| g.iter().map(move |&(row, y)| RowPoint { row, x: (i + 1) as f64, y }))
            .collect();
        let means: Vec<[f64; 2]> = self.result.means.iter().enumerate().map(|(i, &m)| [(i + 1) as f64, m]).collect();
        let k = self.groups.len() as f64;
        let l = &self.request.limits;
        let mut segments = vec![vec![[0.5, self.result.mean], [k + 0.5, self.result.mean]]];
        let mut labels = vec![(k + 0.5, self.result.mean, format!("Mean {}", format_number(self.result.mean, 4)))];
        for (v, name) in [(l.lower, "LSL"), (l.target, "Target"), (l.upper, "USL")] {
            if let Some(v) = v {
                segments.push(vec![[0.5, v], [k + 0.5, v]]);
                labels.push((k + 0.5, v, name.to_owned()));
            }
        }
        // Label subgroups when there are few enough to read.
        let x_ticks = (self.labels.len() <= 30)
            .then(|| self.labels.iter().enumerate().map(|(i, l)| ((i + 1) as f64, l.clone())).collect::<Vec<_>>());
        let x_label = match self.request.subgroup {
            SubgroupBy::Column(_) => "Subgroup".to_owned(),
            SubgroupBy::Size(m) => format!("Subgroup (consecutive rows, {m} at a time)"),
        };
        let spec = PlotSpec {
            x_label,
            y_label: self.name.clone(),
            points,
            line: Some(means),
            line_name: Some("Subgroup means".to_owned()),
            band: None,
            groups: None,
            x_ticks,
            panes: Vec::new(),
            annotations: Some(Annotations { groups: Vec::new(), items: Vec::new(), labels }),
            x_time: None,
            y_time: None,
            segments,
            ..PlotSpec::default()
        };
        section(
            SectionKind::BetweenWithinPlot,
            "Values by Subgroup (line: subgroup means)",
            SectionContent::Plot(Box::new(spec)),
        )
    }

    fn histogram(&self) -> ReportSection {
        let data: Vec<(usize, f64)> = self.groups.iter().flatten().copied().collect();
        let l = &self.request.limits;
        let references = [(l.lower, "LSL"), (l.target, "Target"), (l.upper, "USL")]
            .into_iter()
            .filter_map(|(v, n)| v.map(|v| (v, n.to_owned())))
            .collect();
        let spec = BarChartSpec {
            x_time: None,
            x_label: self.name.clone(),
            y_label: "Count".to_owned(),
            bars: histogram_bars(&data),
            x_ticks: None,
            curve: None,
            references,
        };
        section(SectionKind::CapabilityHistogram, "Histogram with Specification Limits", SectionContent::Bars(spec))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    fn table() -> DataTable {
        let v = [
            10.2, 10.4, 10.1, 10.3, 10.8, 10.6, 10.9, 10.7, 9.9, 10.1, 10.0, 9.8, 10.5, 10.4, 10.7, 10.1, 10.3, 10.2,
            10.0, 10.9, 11.1, 10.8, 11.0,
        ];
        let batch: Vec<String> = [4, 4, 4, 3, 4, 4]
            .iter()
            .enumerate()
            .flat_map(|(i, &n)| std::iter::repeat_n(format!("B{}", i + 1), n))
            .collect();
        DataTable::new("t", vec![Column::numeric("Thickness", v.to_vec()), Column::text("Batch", batch)]).unwrap()
    }

    const LIMITS: SpecLimits = SpecLimits { lower: Some(9.0), upper: Some(12.0), target: None };

    #[test]
    fn report_by_subgroup_column() {
        let t = table();
        let rows = RowStateManager::new(23);
        let request = BetweenWithinRequest {
            column: 0,
            subgroup: SubgroupBy::Column(1),
            limits: LIMITS,
            within: WithinEstimate::PooledStdDev,
            between: BetweenEstimate::MovingRange,
        };
        let a = BetweenWithinAnalysis::run(&t, &rows, &request).unwrap();
        assert_eq!(a.title(), "Between/Within Capability of Thickness");
        assert_eq!(a.result().subgroups, 6);
        assert!((a.result().sigma_between - 0.539_172_365_627_604_3).abs() < 1e-10);
        let s = a.sections();
        assert_eq!(s.len(), 6);
        let SectionContent::Table(idx) = &s[0].content else { panic!() };
        assert_eq!(idx.rows.len(), 6);
        assert!(idx.rows[2][0] == "Cp (between/within)");
        let SectionContent::Plot(p) = &s[4].content else { panic!() };
        assert_eq!(p.points.len(), 23);
        assert_eq!(p.x_ticks.as_ref().unwrap()[3].1, "B4");
        // Excluding a batch's rows removes that subgroup.
        let mut ex = RowStateManager::new(23);
        ex.set_excluded([12, 13, 14], true);
        assert_eq!(BetweenWithinAnalysis::run(&t, &ex, &request).unwrap().result().subgroups, 5);
        // Consecutive rows of four: the last subgroup holds three.
        let by_size = BetweenWithinRequest { subgroup: SubgroupBy::Size(4), ..request };
        let b = BetweenWithinAnalysis::run(&t, &rows, &by_size).unwrap();
        assert_eq!(b.result().subgroups, 6);
        assert!(
            BetweenWithinAnalysis::run(&t, &rows, &BetweenWithinRequest { subgroup: SubgroupBy::Column(0), ..request })
                .is_err()
        );
    }

    #[test]
    fn json_round_trip() {
        let t = table();
        for request in [
            BetweenWithinRequest {
                column: 0,
                subgroup: SubgroupBy::Column(1),
                limits: LIMITS,
                within: WithinEstimate::AverageRange,
                between: BetweenEstimate::VarianceComponents,
            },
            BetweenWithinRequest {
                column: 0,
                subgroup: SubgroupBy::Size(5),
                limits: SpecLimits { lower: None, upper: Some(11.5), target: Some(10.5) },
                within: WithinEstimate::PooledStdDev,
                between: BetweenEstimate::MovingRange,
            },
        ] {
            assert_eq!(BetweenWithinRequest::from_json(&request.to_json(&t), &t).unwrap(), request);
        }
    }
}
