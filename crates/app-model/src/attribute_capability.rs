//! Attribute capability: the fraction of defective units (binomial) or the
//! defects per unit (Poisson) of a process, from a count column and a sample
//! size column (or a fixed size), one sample per row
//! ([`stat_quality::attribute_capability`]).
//!
//! The report gives the estimate with its exact interval (and, for the
//! binomial, parts per million and the process Z), the p or u chart that
//! shows whether the process is stable enough for the estimate to mean
//! anything, the cumulative estimate over the samples (which should settle),
//! the rate of each sample against its size (which should show no trend), and
//! a table of the samples. Every plotted point is a dataset row, so brushing
//! links them.

use crate::attribute_chart::{AttributeChartAnalysis, AttributeChartRequest, SubgroupSize};
use crate::format::format_number;
use crate::project::{invalid, member, s};
use crate::report::{Annotations, PlotSpec, ReportSection, RowPoint, SectionContent, SectionKind, TextTable};
use stat_core::Result;
use stat_data::{DataTable, JsonValue, RowStateManager};
use stat_quality::{
    AttributeChartKind, BinomialCapability, PoissonCapability, binomial_capability, poisson_capability,
};

/// Significant digits in report tables.
const DIGITS: usize = 5;

/// Which count model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AttributeCapabilityKind {
    /// Defective units out of units inspected (binomial).
    #[default]
    Binomial,
    /// Defects over units inspected (Poisson).
    Poisson,
}

impl AttributeCapabilityKind {
    /// Both kinds, in menu order.
    pub const ALL: [Self; 2] = [Self::Binomial, Self::Poisson];

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Binomial => "Binomial: defective units (percent defective)",
            Self::Poisson => "Poisson: defects (defects per unit)",
        }
    }

    /// Key in project files.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Binomial => "binomial",
            Self::Poisson => "poisson",
        }
    }
}

/// What to analyse.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AttributeCapabilityRequest {
    /// Binomial or Poisson.
    pub kind: AttributeCapabilityKind,
    /// Column of defective units (binomial) or defects (Poisson) per sample.
    pub count: usize,
    /// Units inspected per sample: a column or a constant (Poisson also
    /// accepts none: one unit per row).
    pub size: SubgroupSize,
    /// Confidence level of the intervals.
    pub level: f64,
}

impl AttributeCapabilityRequest {
    /// A binomial study of `count` with sizes from column `size`, 95% intervals.
    #[must_use]
    pub fn new(kind: AttributeCapabilityKind, count: usize, size: SubgroupSize) -> Self {
        Self { kind, count, size, level: 0.95 }
    }

    /// Columns used.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        match self.size {
            SubgroupSize::Column(c) => vec![self.count, c],
            _ => vec![self.count],
        }
    }

    /// The same request with columns renumbered by `f`.
    #[must_use]
    pub fn map_columns(mut self, f: impl Fn(usize) -> usize) -> Self {
        self.count = f(self.count);
        if let SubgroupSize::Column(c) = &mut self.size {
            *c = f(*c);
        }
        self
    }

    /// The project-file form (`"analysis": "attribute-capability"`).
    #[must_use]
    pub fn to_json(&self, table: &DataTable) -> JsonValue {
        let col = |i: usize| s(table.column(i).map_or("", |c| c.name.as_str()));
        let size = match self.size {
            SubgroupSize::Column(c) => col(c),
            SubgroupSize::Constant(n) => JsonValue::number(n),
            SubgroupSize::None => JsonValue::Null,
        };
        crate::project::obj([
            ("analysis", s("attribute-capability")),
            ("model", s(self.kind.key())),
            ("count", col(self.count)),
            ("size", size),
            ("level", JsonValue::number(self.level)),
        ])
    }

    /// Read the project-file form; a missing level is 95%.
    ///
    /// # Errors
    /// Missing or unknown columns, an unknown model.
    pub fn from_json(v: &JsonValue, table: &DataTable) -> Result<Self> {
        let col = |key: &str| -> Result<usize> {
            let name = member(v, key)?.as_str().ok_or_else(|| invalid(format!("{key}: expected a column name")))?;
            table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")))
        };
        let kind = match v.get("model").and_then(JsonValue::as_str) {
            None | Some("binomial") => AttributeCapabilityKind::Binomial,
            Some("poisson") => AttributeCapabilityKind::Poisson,
            Some(other) => return Err(invalid(format!("attribute capability: unknown model {other:?}"))),
        };
        let size = match v.get("size") {
            None | Some(JsonValue::Null) => SubgroupSize::None,
            Some(JsonValue::String(_)) => SubgroupSize::Column(col("size")?),
            Some(other) => SubgroupSize::Constant(
                other.as_f64().ok_or_else(|| invalid("attribute capability: size must be a column or a number"))?,
            ),
        };
        let level = v.get("level").and_then(JsonValue::as_f64).unwrap_or(0.95);
        Ok(Self { kind, count: col("count")?, size, level })
    }
}

#[derive(Debug, Clone)]
enum Estimate {
    Binomial(BinomialCapability),
    Poisson(PoissonCapability),
}

/// A computed attribute capability study.
#[derive(Debug, Clone)]
pub struct AttributeCapabilityAnalysis {
    request: AttributeCapabilityRequest,
    name: String,
    /// Dataset row of each sample.
    rows: Vec<usize>,
    counts: Vec<f64>,
    sizes: Vec<f64>,
    estimate: Estimate,
    /// The p or u chart (`Err` with the reason when it cannot be drawn).
    chart: std::result::Result<AttributeChartAnalysis, String>,
    n_excluded: usize,
    n_missing: usize,
}

fn section(kind: SectionKind, title: impl Into<String>, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.into(), content }
}

impl AttributeCapabilityAnalysis {
    /// Compute the study from the rows that have a count (and size) and are
    /// not excluded.
    ///
    /// # Errors
    /// Missing or non-numeric columns, a binomial study without sizes, the
    /// size column equal to the count column, and any error of
    /// [`binomial_capability`] or [`poisson_capability`].
    pub fn run(table: &DataTable, states: &RowStateManager, request: &AttributeCapabilityRequest) -> Result<Self> {
        let numeric = |i: usize| -> Result<(String, &[f64])> {
            let c = table.column(i).ok_or_else(|| invalid(format!("no column {i}")))?;
            let v = c.as_numeric().ok_or_else(|| invalid(format!("{} must be numeric", c.name)))?;
            Ok((c.name.clone(), v))
        };
        let (name, counts_all) = numeric(request.count)?;
        let size_column = match request.size {
            SubgroupSize::Column(i) if i == request.count => {
                return Err(invalid("the size column must differ from the count column"));
            }
            SubgroupSize::Column(i) => Some(numeric(i)?.1),
            _ => None,
        };
        if request.kind == AttributeCapabilityKind::Binomial && request.size == SubgroupSize::None {
            return Err(invalid("a binomial study needs the number of units inspected in each sample"));
        }
        let present: Vec<usize> = (0..table.n_rows())
            .filter(|&r| counts_all[r].is_finite() && size_column.is_none_or(|s| s[r].is_finite()))
            .collect();
        let rows: Vec<usize> = present.iter().copied().filter(|&r| !states.is_excluded(r)).collect();
        let counts: Vec<f64> = rows.iter().map(|&r| counts_all[r]).collect();
        let sizes: Vec<f64> = match (request.size, size_column) {
            (SubgroupSize::Column(_), Some(s)) => rows.iter().map(|&r| s[r]).collect(),
            (SubgroupSize::Constant(n), _) => vec![n; rows.len()],
            _ => vec![1.0; rows.len()],
        };
        let estimate = match request.kind {
            AttributeCapabilityKind::Binomial => {
                Estimate::Binomial(binomial_capability(&counts, &sizes, request.level)?)
            }
            AttributeCapabilityKind::Poisson => Estimate::Poisson(poisson_capability(&counts, &sizes, request.level)?),
        };
        let chart_kind = match (request.kind, request.size) {
            (AttributeCapabilityKind::Binomial, _) => AttributeChartKind::P,
            (AttributeCapabilityKind::Poisson, SubgroupSize::None) => AttributeChartKind::C,
            (AttributeCapabilityKind::Poisson, _) => AttributeChartKind::U,
        };
        let chart_request = AttributeChartRequest::new(chart_kind, request.count, request.size);
        let chart = AttributeChartAnalysis::run(table, states, &chart_request).map_err(|e| e.to_string());
        Ok(Self {
            request: *request,
            name,
            n_excluded: present.len() - rows.len(),
            n_missing: table.n_rows() - present.len(),
            rows,
            counts,
            sizes,
            estimate,
            chart,
        })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &AttributeCapabilityRequest {
        &self.request
    }

    /// The binomial estimate, if this is a binomial study.
    #[must_use]
    pub fn binomial(&self) -> Option<&BinomialCapability> {
        match &self.estimate {
            Estimate::Binomial(b) => Some(b),
            Estimate::Poisson(_) => None,
        }
    }

    /// The Poisson estimate, if this is a Poisson study.
    #[must_use]
    pub fn poisson(&self) -> Option<&PoissonCapability> {
        match &self.estimate {
            Estimate::Poisson(p) => Some(p),
            Estimate::Binomial(_) => None,
        }
    }

    /// Dataset rows of the samples outside the chart's limits.
    #[must_use]
    pub fn signal_rows(&self) -> Vec<usize> {
        self.chart.as_ref().map(AttributeChartAnalysis::signal_rows).unwrap_or_default()
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        match self.request.kind {
            AttributeCapabilityKind::Binomial => format!("Binomial Capability of {}", self.name),
            AttributeCapabilityKind::Poisson => format!("Poisson Capability of {}", self.name),
        }
    }

    /// Report sections: summary, chart, cumulative estimate, rate by sample
    /// size, samples.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        vec![self.summary(), self.chart(), self.cumulative(), self.by_size(), self.samples()]
    }

    /// `(scale, unit)`: binomial rates are shown as percentages.
    fn scale(&self) -> (f64, &'static str) {
        match self.request.kind {
            AttributeCapabilityKind::Binomial => (100.0, "% defective"),
            AttributeCapabilityKind::Poisson => (1.0, "Defects per unit"),
        }
    }

    fn summary(&self) -> ReportSection {
        let f = |v: f64| format_number(v, DIGITS);
        let level = format!("{}%", format_number(self.request.level * 100.0, 4));
        let mut t =
            TextTable::new(["Statistic", "Estimate", format!("Lower {level}").as_str(), &format!("Upper {level}")]);
        let blank = String::new;
        let mut row = |name: &str, v: String, lo: String, hi: String| t.push([name.to_owned(), v, lo, hi]);
        row("Samples", self.rows.len().to_string(), blank(), blank());
        if self.n_excluded > 0 {
            row("Rows excluded", self.n_excluded.to_string(), blank(), blank());
        }
        if self.n_missing > 0 {
            row("Rows without a value", self.n_missing.to_string(), blank(), blank());
        }
        let title = match &self.estimate {
            Estimate::Binomial(b) => {
                row("Defective units", f(b.defective), blank(), blank());
                row("Units inspected", f(b.inspected), blank(), blank());
                row("% defective", f(100.0 * b.fraction), f(100.0 * b.interval.0), f(100.0 * b.interval.1));
                row("Defective PPM", f(1e6 * b.fraction), f(1e6 * b.interval.0), f(1e6 * b.interval.1));
                let z = |v: f64| if v.is_finite() { format_number(v, 4) } else { "∞".to_owned() };
                row("Process Z", z(b.process_z), z(b.z_interval.0), z(b.z_interval.1));
                let (lo, hi) = min_max(&b.by_sample);
                row(
                    "Smallest / largest % defective",
                    format!("{} / {}", f(100.0 * lo), f(100.0 * hi)),
                    blank(),
                    blank(),
                );
                "Percent Defective (exact Clopper–Pearson interval)"
            }
            Estimate::Poisson(p) => {
                row("Defects", f(p.defects), blank(), blank());
                row("Units inspected", f(p.units), blank(), blank());
                row("Defects per unit", f(p.dpu), f(p.interval.0), f(p.interval.1));
                let (lo, hi) = min_max(&p.by_sample);
                row("Smallest / largest defects per unit", format!("{} / {}", f(lo), f(hi)), blank(), blank());
                "Defects per Unit (exact Garwood interval)"
            }
        };
        if self.chart.is_ok() {
            let signals = self.signal_rows().len();
            let note = if signals == 0 {
                "none: the process looks stable".to_owned()
            } else {
                format!("{signals}: the process is not stable, so the estimate describes no single process")
            };
            row("Samples outside the chart limits", note, blank(), blank());
        }
        section(SectionKind::AttributeCapabilitySummary, title, SectionContent::Table(t))
    }

    fn chart(&self) -> ReportSection {
        match &self.chart {
            Ok(a) => {
                let mut s = a.sections().swap_remove(1);
                s.title = format!("Stability: {}", s.title);
                s
            }
            Err(e) => section(
                SectionKind::AttributeChart,
                "Stability Chart",
                SectionContent::Note(format!("The control chart could not be drawn: {e}.")),
            ),
        }
    }

    fn estimate_and_interval(&self) -> (f64, (f64, f64), &[f64]) {
        match &self.estimate {
            Estimate::Binomial(b) => (b.fraction, b.interval, &b.cumulative),
            Estimate::Poisson(p) => (p.dpu, p.interval, &p.cumulative),
        }
    }

    fn cumulative(&self) -> ReportSection {
        let (scale, unit) = self.scale();
        let (est, (lo, hi), cumulative) = self.estimate_and_interval();
        let points: Vec<RowPoint> = cumulative
            .iter()
            .zip(&self.rows)
            .enumerate()
            .map(|(i, (&c, &row))| RowPoint { row, x: (i + 1) as f64, y: scale * c })
            .collect();
        let end = self.rows.len() as f64 + 0.5;
        let level = format_number(self.request.level * 100.0, 4);
        let flat = |y: f64| vec![[0.5, scale * y], [end, scale * y]];
        let labels = vec![
            (end, scale * est, format!("Estimate {}", format_number(scale * est, 4))),
            (end, scale * hi, format!("Upper {level}%")),
            (end, scale * lo, format!("Lower {level}%")),
        ];
        let spec = PlotSpec {
            x_label: "Sample".to_owned(),
            y_label: format!("Cumulative {}", unit.to_lowercase()),
            points: points.clone(),
            line: Some(points.iter().map(|p| [p.x, p.y]).collect()),
            line_name: Some(format!("Cumulative {}", unit.to_lowercase())),
            band: None,
            groups: None,
            x_ticks: None,
            panes: Vec::new(),
            annotations: Some(Annotations { groups: Vec::new(), items: Vec::new(), labels }),
            x_time: None,
            y_time: None,
            segments: vec![flat(est), flat(lo), flat(hi)],
            ..PlotSpec::default()
        };
        section(
            SectionKind::AttributeCapabilityCumulative,
            format!("Cumulative {} (should settle near the estimate)", unit.to_lowercase()),
            SectionContent::Plot(Box::new(spec)),
        )
    }

    fn by_size(&self) -> ReportSection {
        let (scale, unit) = self.scale();
        let (est, _, _) = self.estimate_and_interval();
        let points: Vec<RowPoint> = self
            .counts
            .iter()
            .zip(&self.sizes)
            .zip(&self.rows)
            .map(|((&c, &n), &row)| RowPoint { row, x: n, y: scale * c / n })
            .collect();
        let (lo, hi) = min_max(&self.sizes);
        let pad = if hi > lo { 0.05 * (hi - lo) } else { 0.5 };
        let spec = PlotSpec {
            x_label: "Units inspected".to_owned(),
            y_label: unit.to_owned(),
            points,
            line: None,
            band: None,
            groups: None,
            x_ticks: None,
            panes: Vec::new(),
            annotations: None,
            x_time: None,
            y_time: None,
            segments: vec![vec![[lo - pad, scale * est], [hi + pad, scale * est]]],
            ..PlotSpec::default()
        };
        section(
            SectionKind::AttributeCapabilityBySize,
            format!("{unit} by Sample Size (no trend expected; line: overall estimate)"),
            SectionContent::Plot(Box::new(spec)),
        )
    }

    fn samples(&self) -> ReportSection {
        let (scale, unit) = self.scale();
        let count = match self.request.kind {
            AttributeCapabilityKind::Binomial => "Defective units",
            AttributeCapabilityKind::Poisson => "Defects",
        };
        let mut t = TextTable::new(["Sample", "Row", count, "Units inspected", unit]);
        for (i, ((&c, &n), &row)) in self.counts.iter().zip(&self.sizes).zip(&self.rows).enumerate() {
            t.push([
                (i + 1).to_string(),
                (row + 1).to_string(),
                format_number(c, DIGITS),
                format_number(n, DIGITS),
                format_number(scale * c / n, DIGITS),
            ]);
        }
        section(SectionKind::AttributeCapabilitySamples, format!("{count} by Sample"), SectionContent::Table(t))
    }
}

fn min_max(v: &[f64]) -> (f64, f64) {
    v.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &x| (a.min(x), b.max(x)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    fn table() -> DataTable {
        DataTable::new(
            "lots",
            vec![
                Column::numeric("Defective", vec![3.0, 1.0, 0.0, 2.0, f64::NAN]),
                Column::numeric("Inspected", vec![50.0, 50.0, 40.0, 60.0, 50.0]),
                Column::numeric("Defects", vec![4.0, 7.0, 2.0, 5.0, 1.0]),
            ],
        )
        .unwrap()
    }

    #[test]
    fn binomial_report() {
        let t = table();
        let rows = RowStateManager::new(5);
        let request = AttributeCapabilityRequest::new(AttributeCapabilityKind::Binomial, 0, SubgroupSize::Column(1));
        let a = AttributeCapabilityAnalysis::run(&t, &rows, &request).unwrap();
        assert_eq!(a.title(), "Binomial Capability of Defective");
        let s = a.sections();
        let kinds: Vec<_> = s.iter().map(|x| x.kind).collect();
        assert_eq!(
            kinds,
            [
                SectionKind::AttributeCapabilitySummary,
                SectionKind::AttributeChart,
                SectionKind::AttributeCapabilityCumulative,
                SectionKind::AttributeCapabilityBySize,
                SectionKind::AttributeCapabilitySamples,
            ]
        );
        let SectionContent::Table(summary) = &s[0].content else { panic!() };
        let row = |name: &str| summary.rows.iter().find(|r| r[0] == name).unwrap().clone();
        assert_eq!(row("% defective")[1..], ["3", "1.1087", "6.4151"]);
        assert_eq!(row("Process Z")[1..], ["1.881", "1.521", "2.287"]);
        assert_eq!(row("Rows without a value")[1], "1");
        let SectionContent::Plot(p) = &s[2].content else { panic!() };
        assert_eq!(p.points.iter().map(|q| q.row).collect::<Vec<_>>(), [0, 1, 2, 3]);
        assert!((p.points[1].y - 4.0).abs() < 1e-12, "cumulative 4/100 = 4%");
        let SectionContent::Plot(p) = &s[3].content else { panic!() };
        assert!((p.points[3].x - 60.0).abs() < 1e-12);
        // Excluding a row drops it; a binomial study needs sizes.
        let mut ex = RowStateManager::new(5);
        ex.set_excluded([0], true);
        let b = AttributeCapabilityAnalysis::run(&t, &ex, &request).unwrap();
        assert!((b.binomial().unwrap().fraction - 3.0 / 150.0).abs() < 1e-15);
        let none = AttributeCapabilityRequest { size: SubgroupSize::None, ..request };
        assert!(AttributeCapabilityAnalysis::run(&t, &rows, &none).is_err());
    }

    #[test]
    fn poisson_report_and_json() {
        let t = table();
        let rows = RowStateManager::new(5);
        let request =
            AttributeCapabilityRequest::new(AttributeCapabilityKind::Poisson, 2, SubgroupSize::Constant(10.0));
        let a = AttributeCapabilityAnalysis::run(&t, &rows, &request).unwrap();
        assert!((a.poisson().unwrap().dpu - 19.0 / 50.0).abs() < 1e-15);
        assert!(a.title().starts_with("Poisson Capability"));
        let SectionContent::Table(summary) = &a.sections()[0].content else { panic!() };
        assert!(summary.rows.iter().any(|r| r[0] == "Defects per unit"));
        let back = AttributeCapabilityRequest::from_json(&request.to_json(&t), &t).unwrap();
        assert_eq!(back, request);
        let binomial = AttributeCapabilityRequest {
            level: 0.9,
            ..AttributeCapabilityRequest::new(AttributeCapabilityKind::Binomial, 0, SubgroupSize::Column(1))
        };
        assert_eq!(AttributeCapabilityRequest::from_json(&binomial.to_json(&t), &t).unwrap(), binomial);
        assert_eq!(binomial.map_columns(|c| c + 1).columns(), [1, 2]);
    }
}
