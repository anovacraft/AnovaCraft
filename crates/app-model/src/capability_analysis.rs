//! Process capability of one numeric column against specification limits
//! ([`stat_quality::capability`]), bound to the dataset's row state.
//!
//! Values are taken in row order from rows with a value that are not excluded
//! (order matters for the moving-range and subgroup estimates of the
//! short-term σ). The report gives the indices with confidence intervals, a
//! summary of the process, observed and expected nonconforming rates, and a
//! row-linked histogram with the limits and the fitted normal curve.
//!
//! With a non-normal model ([`CapabilityModel`]) the report leads with the
//! percentile-based indices of [`stat_quality::nonnormal`] and the expected
//! rates under the fitted distribution, shows the distribution with its
//! reference percentiles, draws its density over the histogram and a
//! quantile plot against it, and keeps the normal-theory indices at the end
//! for comparison. Bootstrap percentile intervals for those indices can be
//! added ([`stat_quality::bootstrap_percentile_intervals`]).
//!
//! The **summary page** layout gathers one study on one page, in this order:
//! the indices, the process summary, the location chart (X̄ or individuals,
//! following the σ-within method's subgrouping) and its range, standard
//! deviation or moving-range chart, the last subgroups' individual values
//! against the specification limits, the histogram, and a probability plot
//! against the model.
//!
//! With a **by** column the study is also run on each group's rows (in row
//! order within the group), giving a Capability by Group table (N, mean,
//! overall SD, Cp, Cpk, Pp, Ppk and the observed share out of
//! specification) and a Ppk by Group chart with reference lines at 1 and
//! 1.33, its groups in natural order or sorted by Ppk ([`GroupOrder`]). The
//! standard layout can add the probability plot, which carries the
//! specification limits as horizontal lines. The report offers an
//! in-specification indicator column (a formula: 1 inside the limits, 0
//! outside, missing without a value).

use crate::control_chart::{RuleSet, limit_steps};
use crate::distribution::histogram_bars;
use crate::distribution_fit_analysis::identity_line;
use crate::format::format_number;
use crate::report::{
    BarChartSpec, ChartPoint, ControlChartSpec, LimitStep, PlotSpec, Polyline, ReportSection, RowPoint, SectionContent,
    SectionKind, TextTable,
};
use stat_core::distributions::{ContinuousDistribution, Normal};
use stat_core::{Result, StatError};
use stat_data::{DataTable, RowStateManager};
use stat_quality::{
    BootstrapIntervals, Capability, CapabilityModel, CapabilityOptions, ChartKind, PercentileCapability,
    PercentileSource, REFERENCE_PROBABILITIES, RunRule, SpecLimits, VaryingChart, WithinSigma,
    bootstrap_percentile_intervals, percentile_capability, phased_chart, process_capability,
};

/// Significant digits in report tables.
const DIGITS: usize = 5;
/// Confidence level of the intervals.
pub const CAPABILITY_LEVEL: f64 = 0.95;
/// Subgroups (or individual values) shown in the summary page's last-subgroups plot.
pub const LAST_SUBGROUPS: usize = 25;

/// Resampling settings of bootstrap intervals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BootstrapSettings {
    /// Number of resamples.
    pub resamples: usize,
    /// Seed of the random resampling.
    pub seed: u64,
}

impl Default for BootstrapSettings {
    fn default() -> Self {
        Self { resamples: 1000, seed: 1 }
    }
}

/// Order of the groups in the Ppk by Group chart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GroupOrder {
    /// Natural order of the group labels (as the table).
    #[default]
    Labels,
    /// Smallest Ppk first.
    Ascending,
    /// Largest Ppk first.
    Descending,
}

impl GroupOrder {
    /// Every order.
    pub const ALL: [Self; 3] = [Self::Labels, Self::Ascending, Self::Descending];

    /// Key in project files and scripts.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Labels => "labels",
            Self::Ascending => "ascending",
            Self::Descending => "descending",
        }
    }

    /// The order named `key`.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|o| o.key() == key)
    }

    /// Menu label.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Labels => "By group label",
            Self::Ascending => "Smallest Ppk first",
            Self::Descending => "Largest Ppk first",
        }
    }
}

/// What to analyse.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CapabilityRequest {
    /// Measurement column.
    pub column: usize,
    /// Lower specification limit, upper limit and target.
    pub limits: SpecLimits,
    /// Short-term σ estimator.
    pub within: WithinSigma,
    /// Box–Cox power for non-normal data (`None`: none).
    pub box_cox: Option<f64>,
    /// Distribution model: normal theory, or a non-normal model with
    /// percentile-based indices (not combined with a Box–Cox power).
    pub model: CapabilityModel,
    /// Lay the report out as a one-page summary (see the module documentation).
    pub summary: bool,
    /// Bootstrap percentile intervals for the percentile-based indices of a
    /// non-normal model (`None`: none).
    pub bootstrap: Option<BootstrapSettings>,
    /// Column whose groups are also studied one by one (`None`: none).
    pub by: Option<usize>,
    /// Order of the groups in the Ppk by Group chart.
    pub group_order: GroupOrder,
    /// Add the probability plot to the standard layout (the summary page
    /// always has it).
    pub probability_plot: bool,
}

impl CapabilityRequest {
    /// A standard (not summary) normal-theory study with the moving-range σ.
    #[must_use]
    pub fn new(column: usize, limits: SpecLimits) -> Self {
        Self {
            column,
            limits,
            within: WithinSigma::MovingRange,
            box_cox: None,
            model: CapabilityModel::Normal,
            summary: false,
            bootstrap: None,
            by: None,
            group_order: GroupOrder::Labels,
            probability_plot: false,
        }
    }

    /// Columns the study reads.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        std::iter::once(self.column).chain(self.by).collect()
    }
}

/// The study of one group of a by column.
#[derive(Debug, Clone)]
struct GroupStudy {
    label: String,
    n: usize,
    /// Normal-theory indices, or why they could not be computed.
    result: std::result::Result<Capability, String>,
    /// Percentile-based `Ppk` of a non-normal model.
    percentile_ppk: Option<std::result::Result<f64, String>>,
    /// Share of values outside the limits (%).
    out_of_spec: f64,
}

impl GroupStudy {
    /// The Ppk the chart shows (the model's for a non-normal model).
    fn ppk(&self) -> Option<f64> {
        match &self.percentile_ppk {
            Some(p) => p.as_ref().ok().copied(),
            None => self.result.as_ref().ok().map(|c| c.overall.cpk),
        }
    }
}

/// A computed capability study.
#[derive(Debug, Clone)]
pub struct CapabilityAnalysis {
    request: CapabilityRequest,
    name: String,
    /// (row, value) of each observation used, in row order.
    data: Vec<(usize, f64)>,
    result: Capability,
    /// Percentile-based indices of a non-normal model.
    percentile: Option<PercentileCapability>,
    /// Bootstrap intervals of the percentile-based indices, or why they could
    /// not be computed.
    bootstrap: Option<std::result::Result<BootstrapIntervals, String>>,
    n_missing: usize,
    n_excluded: usize,
    exclusion_revision: u64,
    /// Name of the by column and its groups' studies.
    groups: Option<(String, Vec<GroupStudy>)>,
    n_rows: usize,
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// How a `Ppk` reads against the usual 1 and 1.33 thresholds.
fn verdict(ppk: f64) -> &'static str {
    match ppk {
        k if k >= 1.33 => "capable (Ppk ≥ 1.33)",
        k if k >= 1.0 => "marginal (1 ≤ Ppk < 1.33)",
        _ => "not capable (Ppk < 1)",
    }
}

fn section(kind: SectionKind, title: impl Into<String>, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.into(), content }
}

impl CapabilityAnalysis {
    /// Compute the study.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] for a missing or non-numeric column and
    /// any error from [`process_capability`] (no limits, too few values, …)
    /// or [`percentile_capability`] (a family that cannot be fitted, …); a
    /// Box–Cox power together with a non-normal model.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &CapabilityRequest) -> Result<Self> {
        let col = table.column(request.column).ok_or_else(|| invalid(format!("no column {}", request.column)))?;
        let values = col.as_numeric().ok_or_else(|| invalid(format!("{} must be numeric", col.name)))?;
        let present: Vec<usize> = (0..table.n_rows()).filter(|&r| values[r].is_finite()).collect();
        let data: Vec<(usize, f64)> =
            present.iter().filter(|&&r| !states.is_excluded(r)).map(|&r| (r, values[r])).collect();
        let x: Vec<f64> = data.iter().map(|d| d.1).collect();
        let options =
            CapabilityOptions { within: request.within, confidence_level: CAPABILITY_LEVEL, box_cox: request.box_cox };
        if request.box_cox.is_some() && request.model != CapabilityModel::Normal {
            return Err(invalid("choose either a Box–Cox transformation or a non-normal distribution, not both"));
        }
        let result = process_capability(&x, &request.limits, &options)?;
        let percentile = match request.model {
            CapabilityModel::Normal => None,
            model => Some(percentile_capability(&x, &request.limits, model)?),
        };
        let bootstrap = match (&percentile, request.bootstrap) {
            (Some(p), Some(b)) => Some(
                bootstrap_percentile_intervals(&x, &request.limits, p, b.resamples, b.seed, CAPABILITY_LEVEL)
                    .map_err(|e| e.to_string()),
            ),
            _ => None,
        };
        let groups = match request.by {
            None => None,
            Some(by) => {
                let by_name = table.column(by).ok_or_else(|| invalid(format!("no column {by}")))?.name.clone();
                let (labels, members) = crate::compare::split_by_group(table, states, request.column, Some(by))?;
                let studies = labels
                    .into_iter()
                    .zip(members)
                    .map(|(label, m)| {
                        let x: Vec<f64> = m.iter().map(|d| d.1).collect();
                        let l = &request.limits;
                        let out = x
                            .iter()
                            .filter(|&&v| l.lower.is_some_and(|a| v < a) || l.upper.is_some_and(|b| v > b))
                            .count();
                        let percentile_ppk = match request.model {
                            CapabilityModel::Normal => None,
                            model => Some(
                                percentile_capability(&x, &request.limits, model)
                                    .map(|p| p.ppk)
                                    .map_err(|e| e.to_string()),
                            ),
                        };
                        GroupStudy {
                            label,
                            n: x.len(),
                            result: process_capability(&x, &request.limits, &options).map_err(|e| e.to_string()),
                            percentile_ppk,
                            out_of_spec: if x.is_empty() { f64::NAN } else { 100.0 * out as f64 / x.len() as f64 },
                        }
                    })
                    .collect();
                Some((by_name, studies))
            }
        };
        Ok(Self {
            groups,
            n_rows: table.n_rows(),
            percentile,
            bootstrap,
            request: *request,
            name: col.name.clone(),
            n_excluded: present.len() - data.len(),
            data,
            result,
            n_missing: table.n_rows() - present.len(),
            exclusion_revision: states.exclusion_revision(),
        })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &CapabilityRequest {
        &self.request
    }

    /// The computed indices.
    #[must_use]
    pub fn result(&self) -> &Capability {
        &self.result
    }

    /// The percentile-based indices of a non-normal model.
    #[must_use]
    pub fn percentile(&self) -> Option<&PercentileCapability> {
        self.percentile.as_ref()
    }

    /// `true` if row exclusion has changed since the study was computed.
    #[must_use]
    pub fn is_stale(&self, states: &RowStateManager) -> bool {
        states.exclusion_revision() != self.exclusion_revision
    }

    /// Bootstrap intervals of the percentile-based indices, or why they could
    /// not be computed (`None` when not requested or for a normal model).
    #[must_use]
    pub fn bootstrap(&self) -> Option<&std::result::Result<BootstrapIntervals, String>> {
        self.bootstrap.as_ref()
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        if self.request.summary {
            format!("Capability Summary of {}", self.name)
        } else {
            format!("Process Capability of {}", self.name)
        }
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let mut out = if self.request.summary {
            self.summary_page()
        } else if let Some(p) = &self.percentile {
            let mut out = vec![self.percentile_indices(p), self.distribution(p), self.summary()];
            out.push(self.percentile_nonconforming(p));
            out.push(self.histogram());
            if self.request.probability_plot {
                out.push(self.probability_plot());
            } else {
                out.extend(self.quantile_plot(p));
            }
            let mut normal = self.indices();
            normal.kind = SectionKind::CapabilityNormalComparison;
            normal.title = normal.title.replace("Capability Indices", "Normal-Theory Indices for Comparison");
            out.push(normal);
            out
        } else {
            let mut out = vec![self.indices(), self.summary(), self.nonconforming(), self.histogram()];
            if self.request.probability_plot {
                out.push(self.probability_plot());
            }
            out
        };
        if let Some((by, groups)) = &self.groups {
            out.splice(1..1, [self.group_table(by, groups), self.group_chart(by, groups)]);
        }
        out
    }

    /// The in-specification indicator column: a formula giving 1 inside the
    /// limits, 0 outside and missing without a value.
    #[must_use]
    pub fn saveable_columns(&self) -> Vec<crate::ReportColumn> {
        let x = crate::formula::column_reference(&self.name);
        let l = &self.request.limits;
        let lit = |v: f64| {
            let t = crate::script_text::number(v);
            if t.chars().all(|c| c.is_ascii_digit() || c == '-') { format!("{t}.0") } else { t }
        };
        let mut tests = Vec::new();
        if let Some(a) = l.lower {
            tests.push(format!("{x} >= {}", lit(a)));
        }
        if let Some(b) = l.upper {
            tests.push(format!("{x} <= {}", lit(b)));
        }
        if tests.is_empty() {
            return Vec::new();
        }
        let formula = format!("if is_missing({x}) {{ missing() }} else if {} {{ 1 }} else {{ 0 }}", tests.join(" && "));
        let mut values = vec![f64::NAN; self.n_rows];
        let inside = |v: f64| l.lower.is_none_or(|a| v >= a) && l.upper.is_none_or(|b| v <= b);
        for &(r, v) in &self.data {
            values[r] = if inside(v) { 1.0 } else { 0.0 };
        }
        vec![crate::ReportColumn {
            formula: Some(formula),
            level: stat_data::MeasurementLevel::Nominal,
            ..crate::ReportColumn::continuous(
                "in_spec",
                "Save in-specification indicator",
                format!("{} in spec", self.name),
                values,
            )
        }]
    }

    /// Indices of each group of the by column.
    fn group_table(&self, by: &str, groups: &[GroupStudy]) -> ReportSection {
        let model = self.percentile.as_ref().map(Self::model_name);
        let mut header: Vec<String> =
            [by, "N", "Mean", "SD (overall)", "Cp", "Cpk", "Pp", "Ppk", "Out of spec (%)"].map(str::to_owned).into();
        if let Some(m) = &model {
            header.push(format!("Ppk ({m})"));
        }
        let failed = groups.iter().any(|g| g.result.is_err() || matches!(g.percentile_ppk, Some(Err(_))));
        if failed {
            header.push("Note".to_owned());
        }
        let mut t = TextTable::new(header);
        let f = |v: Option<f64>| v.map_or_else(|| "—".to_owned(), |v| format_number(v, 4));
        for g in groups {
            let mut cells = vec![g.label.clone(), g.n.to_string()];
            let mut notes = Vec::new();
            match &g.result {
                Ok(c) => cells.extend([
                    format_number(c.mean, DIGITS),
                    format_number(c.overall.sigma, DIGITS),
                    f(c.within.cp),
                    f(Some(c.within.cpk)),
                    f(c.overall.cp),
                    f(Some(c.overall.cpk)),
                ]),
                Err(e) => {
                    cells.extend((0..6).map(|_| "—".to_owned()));
                    notes.push(e.clone());
                }
            }
            cells.push(f(g.out_of_spec.is_finite().then_some(g.out_of_spec)));
            match &g.percentile_ppk {
                Some(Ok(v)) => cells.push(f(Some(*v))),
                Some(Err(e)) => {
                    cells.push("—".to_owned());
                    notes.push(e.clone());
                }
                None => {}
            }
            if failed {
                cells.push(notes.join("; "));
            }
            t.push(cells);
        }
        section(SectionKind::CapabilityByGroup, format!("Capability by {by}"), SectionContent::Table(t))
    }

    /// Ppk of each group as a dot on a stem from zero, with reference lines
    /// at 1 and 1.33, in the requested order.
    fn group_chart(&self, by: &str, groups: &[GroupStudy]) -> ReportSection {
        let mut shown: Vec<(&str, f64)> = groups.iter().filter_map(|g| Some((g.label.as_str(), g.ppk()?))).collect();
        match self.request.group_order {
            GroupOrder::Labels => {}
            GroupOrder::Ascending => shown.sort_by(|a, b| a.1.total_cmp(&b.1)),
            GroupOrder::Descending => shown.sort_by(|a, b| b.1.total_cmp(&a.1)),
        }
        if shown.is_empty() {
            return section(
                SectionKind::CapabilityByGroupChart,
                format!("Ppk by {by}"),
                SectionContent::Note("No group has enough values for a Ppk.".to_owned()),
            );
        }
        let x = |i: usize| (i + 1) as f64;
        let mut segments: Vec<Polyline> =
            shown.iter().enumerate().map(|(i, s)| vec![[x(i), 0.0], [x(i), s.1]]).collect();
        let (lo, hi) = (0.5, shown.len() as f64 + 0.5);
        segments.extend([1.0, 1.33].map(|v| vec![[lo, v], [hi, v]]));
        let ppk = |v: f64| format_number(v, 3);
        let items = shown.iter().enumerate().map(|(i, s)| (x(i), s.1, ppk(s.1), 0)).collect();
        let labels = vec![(hi, 1.0, "1".to_owned()), (hi, 1.33, "1.33".to_owned())];
        let name = match &self.percentile {
            Some(p) => format!("Ppk ({})", Self::model_name(p)),
            None => "Ppk".to_owned(),
        };
        section(
            SectionKind::CapabilityByGroupChart,
            format!("{name} by {by}"),
            SectionContent::Plot(Box::new(PlotSpec {
                x_label: by.to_owned(),
                y_label: name.clone(),
                x_ticks: Some(shown.iter().enumerate().map(|(i, s)| (x(i), s.0.to_owned())).collect()),
                annotations: Some(crate::report::Annotations { groups: vec![name], items, labels }),
                segments,
                ..PlotSpec::default()
            })),
        )
    }

    /// Name of the model as fitted (the chosen family for best fit).
    fn model_name(p: &PercentileCapability) -> String {
        match (&p.source, p.model) {
            (PercentileSource::Fitted(f), CapabilityModel::BestFit) => format!("{}, best by AIC", f.family.label()),
            (PercentileSource::Fitted(f), _) => f.family.label().to_owned(),
            (PercentileSource::Johnson(j), _) => format!("Johnson {}", j.family.label()),
            (PercentileSource::Sample, _) => "sample percentiles".to_owned(),
        }
    }

    fn percentile_indices(&self, p: &PercentileCapability) -> ReportSection {
        let level = format!("{:.0}%", CAPABILITY_LEVEL * 100.0);
        let boot = self.bootstrap.as_ref().and_then(|b| b.as_ref().ok());
        let mut header = vec!["Index".to_owned(), "Estimate".to_owned()];
        if self.bootstrap.is_some() {
            header.extend([format!("Lower {level} (bootstrap)"), format!("Upper {level} (bootstrap)")]);
        }
        header.push("Definition".to_owned());
        let mut t = TextTable::new(header);
        let f = |v: f64| format_number(v, 4);
        let with_ci = self.bootstrap.is_some();
        let mut row = |name: &str, v: Option<f64>, ci: Option<(f64, f64)>, how: &str| {
            if let Some(v) = v {
                let mut cells = vec![name.to_owned(), f(v)];
                if with_ci {
                    let (lo, hi) = ci.map_or_else(|| (String::new(), String::new()), |(a, b)| (f(a), f(b)));
                    cells.extend([lo, hi]);
                }
                cells.push(how.to_owned());
                t.push(cells);
            }
        };
        row("Pp", p.pp, boot.and_then(|b| b.pp), "(USL − LSL) / (X99.865 − X0.135)");
        row("Ppl", p.ppl, boot.and_then(|b| b.ppl), "(X50 − LSL) / (X50 − X0.135)");
        row("Ppu", p.ppu, boot.and_then(|b| b.ppu), "(USL − X50) / (X99.865 − X50)");
        row("Ppk", Some(p.ppk), boot.map(|b| b.ppk), "min(Ppl, Ppu)");
        if p.source != PercentileSource::Sample {
            row("Ppk (equivalent)", p.ppk_equivalent, None, "−Φ⁻¹(worse-side expected fraction) / 3");
        }
        match &self.bootstrap {
            Some(Ok(b)) => {
                let fitted = if b.used < b.resamples { format!(", {} fitted", b.used) } else { String::new() };
                let note = format!("{} resamples (seed {}){fitted}; percentile intervals", b.resamples, b.seed);
                let mut cells = vec!["Bootstrap".to_owned(), note, String::new(), String::new()];
                let how = if matches!(p.model, CapabilityModel::BestFit) {
                    "the family chosen on the full sample refitted to each resample"
                } else {
                    "the model refitted to each resample"
                };
                cells.push(how.to_owned());
                t.push(cells);
            }
            Some(Err(message)) => {
                t.push([
                    "Bootstrap".to_owned(),
                    "not available".to_owned(),
                    String::new(),
                    String::new(),
                    message.clone(),
                ]);
            }
            None => {}
        }
        let verdict = verdict(p.ppk);
        section(
            SectionKind::CapabilityIndices,
            format!("Capability Indices: {} ({verdict})", Self::model_name(p)),
            SectionContent::Table(t),
        )
    }

    fn distribution(&self, p: &PercentileCapability) -> ReportSection {
        let mut t = TextTable::new(["Item", "Value"]);
        let f = |v: f64| format_number(v, DIGITS);
        t.push(["Model".to_owned(), Self::model_name(p)]);
        match &p.source {
            PercentileSource::Fitted(fit) => {
                for q in fit.parameters() {
                    t.push([q.name.to_owned(), f(q.value)]);
                }
                t.push(["AIC".to_owned(), f(fit.aic())]);
            }
            PercentileSource::Johnson(j) => {
                t.push(["Shape γ".to_owned(), f(j.gamma)]);
                t.push(["Shape η".to_owned(), f(j.eta)]);
                t.push(["Location ε".to_owned(), f(j.epsilon)]);
                if j.family != stat_core::distribution_fit::JohnsonFamily::Sl {
                    t.push(["Scale λ".to_owned(), f(j.lambda)]);
                }
                t.push([
                    "Normality of transformed values (Anderson–Darling p)".to_owned(),
                    format_number(j.normality.p_value, 4),
                ]);
            }
            PercentileSource::Sample => {
                if self.data.len() < 740 {
                    t.push([
                        "Note".to_owned(),
                        format!(
                            "with {} values the outer percentiles are near the sample extremes, so these indices are rough",
                            self.data.len()
                        ),
                    ]);
                }
            }
        }
        for (pr, v) in REFERENCE_PROBABILITIES.iter().zip(p.percentiles) {
            t.push([format!("{}% point", format_number(pr * 100.0, 5)), f(v)]);
        }
        for (i, (family, aic)) in p.candidates.iter().enumerate() {
            let mark = if i == 0 { " ◀ chosen" } else { "" };
            t.push([format!("AIC: {}{mark}", family.label()), f(*aic)]);
        }
        section(SectionKind::CapabilityDistribution, "Process Distribution", SectionContent::Table(t))
    }

    fn percentile_nonconforming(&self, p: &PercentileCapability) -> ReportSection {
        let c = &self.result;
        let expected = if p.source == PercentileSource::Sample {
            "Expected (sample) (ppm)".to_owned()
        } else {
            format!("Expected {} (ppm)", Self::model_name(p))
        };
        let mut t = TextTable::new([
            "Portion".to_owned(),
            "Observed (ppm)".to_owned(),
            expected,
            "Expected normal (ppm)".to_owned(),
        ]);
        let f = |v: f64| format_number(v, 4);
        let rows = [
            ("Below LSL", c.observed_ppm.0, p.expected_ppm.0, c.overall.expected_ppm.0),
            ("Above USL", c.observed_ppm.1, p.expected_ppm.1, c.overall.expected_ppm.1),
            (
                "Total",
                c.observed_ppm.0 + c.observed_ppm.1,
                p.expected_ppm.0 + p.expected_ppm.1,
                c.overall.expected_ppm.0 + c.overall.expected_ppm.1,
            ),
        ];
        for (name, o, e, n) in rows {
            t.push([name.to_owned(), f(o), f(e), f(n)]);
        }
        section(SectionKind::CapabilityNonconforming, "Nonconforming Parts", SectionContent::Table(t))
    }

    /// Ordered values against the model's quantiles at the plotting positions
    /// `(i − 0.3)/(n + 0.4)` (none for sample percentiles).
    fn quantile_plot(&self, p: &PercentileCapability) -> Option<ReportSection> {
        if p.source == PercentileSource::Sample {
            return None;
        }
        let mut sorted = self.data.clone();
        sorted.sort_by(|a, b| a.1.total_cmp(&b.1));
        let n = sorted.len() as f64;
        let points: Vec<RowPoint> = sorted
            .iter()
            .enumerate()
            .map(|(i, &(row, y))| RowPoint { row, x: p.quantile((i as f64 + 0.7) / (n + 0.4), &[]), y })
            .filter(|q| q.x.is_finite())
            .collect();
        let line = identity_line(&points);
        let name = Self::model_name(p);
        Some(section(
            SectionKind::ProbabilityPlot,
            format!("Quantile Plot: {name}"),
            SectionContent::Plot(Box::new(PlotSpec {
                x_label: format!("{name} quantile (fitted)"),
                y_label: self.name.clone(),
                points,
                line,
                line_name: Some("Reference line y = x".to_owned()),
                band: None,
                groups: None,
                x_ticks: None,
                panes: Vec::new(),
                annotations: None,
                x_time: None,
                y_time: None,
                segments: Vec::new(),
                ..PlotSpec::default()
            })),
        ))
    }

    /// The one-page summary (see the module documentation).
    fn summary_page(&self) -> Vec<ReportSection> {
        let mut out = vec![match &self.percentile {
            Some(p) => self.percentile_indices(p),
            None => self.indices(),
        }];
        out.push(self.summary());
        out.extend(self.control_charts());
        out.push(self.last_subgroups());
        out.push(self.histogram());
        out.push(self.probability_plot());
        out
    }

    /// The observations grouped as the σ-within method groups them:
    /// single values for the moving range, consecutive subgroups otherwise
    /// (trailing values that do not fill a subgroup are left out).
    fn subgroups(&self) -> (ChartKind, Vec<&[(usize, f64)]>) {
        match self.request.within {
            WithinSigma::MovingRange => (ChartKind::IndividualsAndMovingRange, self.data.chunks(1).collect()),
            WithinSigma::AverageRange { subgroup_size } => {
                (ChartKind::MeanAndRange, self.data.chunks_exact(subgroup_size.max(1)).collect())
            }
            WithinSigma::PooledStdDev { subgroup_size } => {
                (ChartKind::MeanAndStdDev, self.data.chunks_exact(subgroup_size.max(1)).collect())
            }
        }
    }

    /// Location and dispersion control charts of the summary page, with all
    /// eight run rules on the location chart and rule 1 on the other.
    fn control_charts(&self) -> Vec<ReportSection> {
        let (kind, groups) = self.subgroups();
        let values: Vec<Vec<f64>> = groups.iter().map(|g| g.iter().map(|d| d.1).collect()).collect();
        let chart = match phased_chart(kind, &values, &vec![0; values.len()]) {
            Ok(c) => c,
            Err(e) => {
                return vec![section(
                    SectionKind::CapabilityLocationChart,
                    "Control Chart",
                    SectionContent::Note(format!("No control chart: {e}")),
                )];
            }
        };
        let rows: Vec<Vec<usize>> = groups.iter().map(|g| g.iter().map(|d| d.0).collect()).collect();
        let spec = |c: &VaryingChart, rules: RuleSet, moving: bool| -> ControlChartSpec {
            let violations = c.violations(&rules.rules(), &chart.phases);
            let points = c
                .values
                .iter()
                .enumerate()
                .filter_map(|(i, v)| {
                    let value = (*v)?;
                    let first = if moving { i.saturating_sub(1) } else { i };
                    let mut numbers: Vec<u8> =
                        violations.iter().filter(|x| x.index == i).map(|x| x.rule.number()).collect();
                    numbers.dedup();
                    Some(ChartPoint {
                        sample: i + 1,
                        value,
                        rows: (first..=i).flat_map(|k| rows[k].iter().copied()).collect(),
                        rules: numbers,
                        label: None,
                        note: None,
                        signal: false,
                    })
                })
                .collect();
            let steps = limit_steps(&c.limits);
            let first: LimitStep = steps[0];
            ControlChartSpec {
                x_label: "Sample".to_owned(),
                x_ticks: None,
                flag_label: None,
                limit_label: None,
                y_label: c.statistic.label().to_owned(),
                points,
                center: first.center,
                lower: first.lower,
                upper: first.upper,
                zone_sigma: first.zone_sigma,
                steps: if steps.len() > 1 { steps } else { Vec::new() },
                phases: Vec::new(),
            }
        };
        let moving = kind == ChartKind::IndividualsAndMovingRange;
        vec![
            section(
                SectionKind::CapabilityLocationChart,
                format!("{} Chart", chart.location.statistic.label()),
                SectionContent::ControlChart(spec(&chart.location, RuleSet::ALL, false)),
            ),
            section(
                SectionKind::CapabilityDispersionChart,
                format!("{} Chart", chart.dispersion.statistic.label()),
                SectionContent::ControlChart(spec(
                    &chart.dispersion,
                    RuleSet::from_numbers([RunRule::BeyondLimits.number()]),
                    moving,
                )),
            ),
        ]
    }

    /// The last subgroups' individual values (or the last individual values)
    /// against the specification limits, with the subgroup means joined.
    fn last_subgroups(&self) -> ReportSection {
        let (kind, groups) = self.subgroups();
        let individual = kind == ChartKind::IndividualsAndMovingRange;
        let start = groups.len().saturating_sub(LAST_SUBGROUPS);
        let shown = &groups[start..];
        let x = |k: usize| (start + k + 1) as f64;
        let points: Vec<RowPoint> = shown
            .iter()
            .enumerate()
            .flat_map(|(k, g)| g.iter().map(move |&(row, y)| RowPoint { row, x: x(k), y }))
            .collect();
        let line = (!individual && !shown.is_empty()).then(|| {
            shown
                .iter()
                .enumerate()
                .map(|(k, g)| [x(k), g.iter().map(|d| d.1).sum::<f64>() / g.len() as f64])
                .collect::<Vec<[f64; 2]>>()
        });
        let (lo, hi) = (x(0) - 0.5, x(shown.len().saturating_sub(1)) + 0.5);
        let r = &self.request.limits;
        let marks: Vec<(f64, &str)> = [(r.lower, "LSL"), (r.target, "Target"), (r.upper, "USL")]
            .into_iter()
            .filter_map(|(v, n)| Some((v?, n)))
            .collect();
        let segments = marks.iter().map(|&(v, _)| vec![[lo, v], [hi, v]]).collect();
        let labels = marks.iter().map(|&(v, n)| (hi, v, n.to_owned())).collect();
        let (noun, title) = if individual {
            ("Observation", format!("Last {} Values", shown.len()))
        } else {
            ("Subgroup", format!("Last {} Subgroups", shown.len()))
        };
        section(
            SectionKind::CapabilityLastSubgroups,
            format!("{title} with Specification Limits"),
            SectionContent::Plot(Box::new(PlotSpec {
                x_label: noun.to_owned(),
                y_label: self.name.clone(),
                points,
                line,
                line_name: Some("Subgroup means".to_owned()),
                annotations: Some(crate::report::Annotations { groups: Vec::new(), items: Vec::new(), labels }),
                segments,
                ..PlotSpec::default()
            })),
        )
    }

    /// Probability plot against the model: the fitted distribution's
    /// quantiles for a fitted or Johnson model, normal quantiles (from the
    /// sample mean and standard deviation) otherwise.
    fn probability_plot(&self) -> ReportSection {
        if let Some(mut s) = self.percentile.as_ref().and_then(|p| self.quantile_plot(p)) {
            s.kind = SectionKind::CapabilityProbabilityPlot;
            if let SectionContent::Plot(plot) = &mut s.content {
                let (segments, labels) = self.limit_lines(&plot.points);
                plot.segments = segments;
                plot.annotations = Some(crate::report::Annotations { groups: Vec::new(), items: Vec::new(), labels });
            }
            return s;
        }
        let n = self.data.len() as f64;
        let mean = self.data.iter().map(|d| d.1).sum::<f64>() / n;
        let sd = (self.data.iter().map(|d| (d.1 - mean).powi(2)).sum::<f64>() / (n - 1.0)).sqrt();
        let mut sorted = self.data.clone();
        sorted.sort_by(|a, b| a.1.total_cmp(&b.1));
        let normal = Normal::standard();
        let points: Vec<RowPoint> = sorted
            .iter()
            .enumerate()
            .filter_map(|(i, &(row, y))| {
                let z = normal.quantile((i as f64 + 0.7) / (n + 0.4)).ok()?;
                Some(RowPoint { row, x: mean + sd * z, y })
            })
            .collect();
        let line = identity_line(&points);
        let (segments, labels) = self.limit_lines(&points);
        section(
            SectionKind::CapabilityProbabilityPlot,
            "Normal Probability Plot",
            SectionContent::Plot(Box::new(PlotSpec {
                x_label: "Normal quantile (sample mean and SD)".to_owned(),
                y_label: self.name.clone(),
                points,
                line,
                line_name: Some("Reference line y = x".to_owned()),
                segments,
                annotations: Some(crate::report::Annotations { groups: Vec::new(), items: Vec::new(), labels }),
                ..PlotSpec::default()
            })),
        )
    }

    /// Horizontal LSL, target and USL lines across the plotted quantiles,
    /// with their labels at the right end.
    fn limit_lines(&self, points: &[RowPoint]) -> (Vec<Polyline>, Vec<(f64, f64, String)>) {
        let (lo, hi) = points.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(p.x), b.max(p.x)));
        if !lo.is_finite() {
            return (Vec::new(), Vec::new());
        }
        let r = &self.request.limits;
        let marks: Vec<(f64, &str)> = [(r.lower, "LSL"), (r.target, "Target"), (r.upper, "USL")]
            .into_iter()
            .filter_map(|(v, n)| Some((v?, n)))
            .collect();
        (
            marks.iter().map(|&(v, _)| vec![[lo, v], [hi, v]]).collect(),
            marks.iter().map(|&(v, n)| (hi, v, n.to_owned())).collect(),
        )
    }

    fn indices(&self) -> ReportSection {
        let c = &self.result;
        let level = format!("{:.0}%", CAPABILITY_LEVEL * 100.0);
        let mut t = TextTable::new([
            "Index".to_owned(),
            "Estimate".to_owned(),
            format!("Lower {level}"),
            format!("Upper {level}"),
        ]);
        let f = |v: f64| format_number(v, 4);
        let blank = String::new;
        let mut row = |name: &str, v: Option<f64>, ci: Option<(f64, f64)>| {
            if let Some(v) = v {
                let (lo, hi) = ci.map_or_else(|| (blank(), blank()), |(a, b)| (f(a), f(b)));
                t.push([name.to_owned(), f(v), lo, hi]);
            }
        };
        row("Cp (within)", c.within.cp, c.within.cp_interval);
        row("Cpl (within)", c.within.cpl, None);
        row("Cpu (within)", c.within.cpu, None);
        row("Cpk (within)", Some(c.within.cpk), Some(c.within.cpk_interval));
        row("Pp (overall)", c.overall.cp, c.overall.cp_interval);
        row("Ppl (overall)", c.overall.cpl, None);
        row("Ppu (overall)", c.overall.cpu, None);
        row("Ppk (overall)", Some(c.overall.cpk), Some(c.overall.cpk_interval));
        row("Cpm (target)", c.cpm, None);
        let verdict = verdict(c.overall.cpk);
        section(SectionKind::CapabilityIndices, format!("Capability Indices ({verdict})"), SectionContent::Table(t))
    }

    fn summary(&self) -> ReportSection {
        let c = &self.result;
        let mut t = TextTable::new(["Statistic", "Value"]);
        let f = |v: f64| format_number(v, DIGITS);
        let opt = |v: Option<f64>| v.map_or_else(|| "—".to_owned(), f);
        let r = &self.request.limits;
        t.push(["Lower specification limit".to_owned(), opt(r.lower)]);
        t.push(["Target".to_owned(), opt(r.target)]);
        t.push(["Upper specification limit".to_owned(), opt(r.upper)]);
        t.push(["Observations".to_owned(), c.n.to_string()]);
        if self.n_excluded > 0 {
            t.push(["Rows excluded".to_owned(), self.n_excluded.to_string()]);
        }
        if self.n_missing > 0 {
            t.push(["Rows without a value".to_owned(), self.n_missing.to_string()]);
        }
        let scale = if c.box_cox.is_some() { " (transformed scale)" } else { "" };
        t.push([format!("Mean{scale}"), f(c.mean)]);
        let method = match self.request.within {
            WithinSigma::MovingRange => "moving range".to_owned(),
            WithinSigma::AverageRange { subgroup_size } => format!("average range, subgroups of {subgroup_size}"),
            WithinSigma::PooledStdDev { subgroup_size } => format!("pooled SD, subgroups of {subgroup_size}"),
        };
        t.push([format!("σ within ({method}){scale}"), f(c.within.sigma)]);
        t.push([format!("σ overall (sample SD){scale}"), f(c.overall.sigma)]);
        if let Some(l) = c.box_cox {
            t.push(["Box–Cox power λ".to_owned(), format_number(l, 4)]);
        }
        section(SectionKind::CapabilitySummary, "Process Summary", SectionContent::Table(t))
    }

    fn nonconforming(&self) -> ReportSection {
        let c = &self.result;
        let mut t = TextTable::new(["Portion", "Observed (ppm)", "Expected within (ppm)", "Expected overall (ppm)"]);
        let f = |v: f64| format_number(v, 4);
        let rows = [
            ("Below LSL", c.observed_ppm.0, c.within.expected_ppm.0, c.overall.expected_ppm.0),
            ("Above USL", c.observed_ppm.1, c.within.expected_ppm.1, c.overall.expected_ppm.1),
            (
                "Total",
                c.observed_ppm.0 + c.observed_ppm.1,
                c.within.expected_ppm.0 + c.within.expected_ppm.1,
                c.overall.expected_ppm.0 + c.overall.expected_ppm.1,
            ),
        ];
        for (name, o, w, a) in rows {
            t.push([name.to_owned(), f(o), f(w), f(a)]);
        }
        let title = if c.box_cox.is_some() {
            "Nonconforming Parts (expected rates assume normality on the transformed scale)"
        } else {
            "Nonconforming Parts (expected rates assume a normal distribution)"
        };
        section(SectionKind::CapabilityNonconforming, title, SectionContent::Table(t))
    }

    fn histogram(&self) -> ReportSection {
        let bars = histogram_bars(&self.data);
        let r = &self.request.limits;
        let mut references = Vec::new();
        if let Some(l) = r.lower {
            references.push((l, "LSL".to_owned()));
        }
        if let Some(t) = r.target {
            references.push((t, "Target".to_owned()));
        }
        if let Some(u) = r.upper {
            references.push((u, "USL".to_owned()));
        }
        // The non-normal model's density scaled to counts, spanning the data,
        // the limits, the reference percentiles and the model's support (its
        // threshold or bound, else its 0.01% and 99.99% points), so a curve
        // that falls to zero at a threshold is drawn down to it instead of
        // starting in mid-air at the smallest value.
        let width = bars.first().map_or(1.0, |b| b.width);
        let model_curve = self.percentile.as_ref().filter(|p| p.source != PercentileSource::Sample).map(|p| {
            let (mut lo, mut hi) = (p.percentiles[0], p.percentiles[2]);
            let tails = [p.lower_bound().unwrap_or_else(|| p.quantile(1e-4, &[])), p.quantile(1.0 - 1e-4, &[])];
            for x in references.iter().map(|r| r.0).chain(self.data.iter().map(|d| d.1)).chain(tails) {
                if !x.is_finite() {
                    continue;
                }
                lo = lo.min(x);
                hi = hi.max(x);
            }
            let scale = self.data.len() as f64 * width;
            let points: Vec<[f64; 2]> = (0..=200)
                .filter_map(|i| {
                    let x = lo + (hi - lo) * f64::from(i) / 200.0;
                    let y = scale * p.pdf(x)?;
                    y.is_finite().then_some([x, y])
                })
                .collect();
            (format!("{} fit", Self::model_name(p)), points)
        });
        // Overall normal density scaled to counts (original scale only).
        let curve = if self.percentile.is_some() {
            model_curve
        } else if self.result.box_cox.is_none() {
            let (mean, s) = (self.result.mean, self.result.overall.sigma);
            let mut lo = mean - 3.5 * s;
            let mut hi = mean + 3.5 * s;
            for (x, _) in &references {
                lo = lo.min(*x);
                hi = hi.max(*x);
            }
            Normal::new(mean, s).ok().map(|d| {
                let scale = self.data.len() as f64 * width;
                let points = (0..=160).map(|i| {
                    let x = lo + (hi - lo) * f64::from(i) / 160.0;
                    [x, scale * d.pdf(x)]
                });
                ("Normal (overall σ)".to_owned(), points.collect())
            })
        } else {
            None
        };
        let spec = BarChartSpec {
            x_time: None,
            x_label: self.name.clone(),
            y_label: "Count".to_owned(),
            bars,
            x_ticks: None,
            curve,
            references,
        };
        section(SectionKind::CapabilityHistogram, "Histogram with Specification Limits", SectionContent::Bars(spec))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    fn rings() -> DataTable {
        let mut v = vec![
            74.030, 74.002, 74.019, 73.992, 74.008, 73.995, 73.992, 74.001, 74.011, 74.004, 73.988, 74.024, 74.021,
            74.005, 74.002, 74.002, 73.996, 73.993, 74.015, 74.009,
        ];
        v.push(f64::NAN);
        DataTable::new("rings", vec![Column::numeric("Diameter", v)]).unwrap()
    }

    const LIMITS: SpecLimits = SpecLimits { lower: Some(73.95), upper: Some(74.05), target: Some(74.0) };

    #[test]
    fn report_sections() {
        let t = rings();
        let rows = RowStateManager::new(t.n_rows());
        let request = CapabilityRequest::new(0, LIMITS);
        let a = CapabilityAnalysis::run(&t, &rows, &request).unwrap();
        assert_eq!(a.title(), "Process Capability of Diameter");
        let s = a.sections();
        assert_eq!(s.len(), 4);
        assert!(s[0].title.contains("marginal"), "Ppk = 1.27: {}", s[0].title);
        let SectionContent::Table(idx) = &s[0].content else { panic!() };
        assert_eq!(idx.cell(0, "Estimate"), Some("1.483"));
        assert_eq!(idx.rows.iter().find(|r| r[0] == "Ppk (overall)").unwrap()[1], "1.272");
        assert_eq!(idx.rows.len(), 9);
        let SectionContent::Table(summary) = &s[1].content else { panic!() };
        assert!(summary.rows.iter().any(|r| r[0] == "Rows without a value" && r[1] == "1"));
        let SectionContent::Table(nc) = &s[2].content else { panic!() };
        assert_eq!(nc.cell(2, "Observed (ppm)"), Some("0"));
        let SectionContent::Bars(h) = &s[3].content else { panic!() };
        assert_eq!(h.references.len(), 3);
        assert!(h.curve.is_some());
        assert_eq!(h.bars.iter().map(|b| b.count()).sum::<usize>(), 20);
    }

    #[test]
    fn exclusion_transform_and_errors() {
        let t = rings();
        let mut rows = RowStateManager::new(t.n_rows());
        rows.set_excluded([0], true);
        let request = CapabilityRequest {
            within: WithinSigma::PooledStdDev { subgroup_size: 4 },
            box_cox: Some(0.0),
            ..CapabilityRequest::new(0, LIMITS)
        };
        let a = CapabilityAnalysis::run(&t, &rows, &request).unwrap();
        assert_eq!(a.result().n, 19);
        let SectionContent::Bars(h) = &a.sections()[3].content else { panic!() };
        assert!(h.curve.is_none(), "no normal curve on the original scale after a transform");
        let none = CapabilityRequest { limits: SpecLimits::default(), ..request };
        assert!(CapabilityAnalysis::run(&t, &rows, &none).is_err());
        assert!(CapabilityAnalysis::run(&t, &rows, &CapabilityRequest { column: 4, ..request }).is_err());
        let both = CapabilityRequest { model: CapabilityModel::Johnson, ..request };
        assert!(CapabilityAnalysis::run(&t, &rows, &both).unwrap_err().to_string().contains("not both"));
    }

    #[test]
    fn non_normal_sections() {
        // Right-skewed values: a Weibull with shape 1.5 and scale 4 at its 60 plotting midpoints.
        let v: Vec<f64> = (1..=60).map(|i| 4.0 * (-(-(f64::from(i) - 0.5) / 60.0).ln_1p()).powf(1.0 / 1.5)).collect();
        let t = DataTable::new("t", vec![Column::numeric("Flatness", v)]).unwrap();
        let rows = RowStateManager::new(60);
        let limits = SpecLimits { lower: None, upper: Some(16.0), target: None };
        let request = CapabilityRequest {
            model: CapabilityModel::Distribution(stat_core::distribution_fit::FitFamily::Weibull),
            ..CapabilityRequest::new(0, limits)
        };
        let a = CapabilityAnalysis::run(&t, &rows, &request).unwrap();
        let s = a.sections();
        let kinds: Vec<SectionKind> = s.iter().map(|x| x.kind).collect();
        assert_eq!(
            kinds,
            [
                SectionKind::CapabilityIndices,
                SectionKind::CapabilityDistribution,
                SectionKind::CapabilitySummary,
                SectionKind::CapabilityNonconforming,
                SectionKind::CapabilityHistogram,
                SectionKind::ProbabilityPlot,
                SectionKind::CapabilityNormalComparison,
            ]
        );
        assert!(s[0].title.starts_with("Capability Indices: Weibull ("), "{}", s[0].title);
        let SectionContent::Table(idx) = &s[0].content else { panic!() };
        let p = a.percentile().unwrap();
        assert_eq!(idx.rows.iter().find(|r| r[0] == "Ppk").unwrap()[1], format_number(p.ppk, 4));
        assert!(idx.rows.iter().all(|r| r[0] != "Pp"), "one limit: no Pp");
        let SectionContent::Table(dist) = &s[1].content else { panic!() };
        assert!(dist.rows.iter().any(|r| r[0] == "99.865% point"), "{:?}", dist.rows);
        let SectionContent::Table(nc) = &s[3].content else { panic!() };
        assert_eq!(nc.columns[2], "Expected Weibull (ppm)");
        let SectionContent::Bars(h) = &s[4].content else { panic!() };
        assert_eq!(h.curve.as_ref().unwrap().0, "Weibull fit");
        assert!(s[6].title.starts_with("Normal-Theory Indices for Comparison"));

        // Best fit names the chosen family and lists the comparison; sample percentiles draw no curve or plot.
        let best =
            CapabilityAnalysis::run(&t, &rows, &CapabilityRequest { model: CapabilityModel::BestFit, ..request })
                .unwrap();
        let s = best.sections();
        assert!(s[0].title.contains("best by AIC"), "{}", s[0].title);
        let SectionContent::Table(dist) = &s[1].content else { panic!() };
        assert!(dist.rows.iter().any(|r| r[0].ends_with("◀ chosen")));
        let np =
            CapabilityAnalysis::run(&t, &rows, &CapabilityRequest { model: CapabilityModel::Nonparametric, ..request })
                .unwrap();
        let s = np.sections();
        assert_eq!(s.len(), 6);
        let SectionContent::Bars(h) = &s[4].content else { panic!() };
        assert!(h.curve.is_none());
        let j = CapabilityAnalysis::run(&t, &rows, &CapabilityRequest { model: CapabilityModel::Johnson, ..request })
            .unwrap();
        assert!(j.sections()[0].title.contains("Johnson"));
    }

    #[test]
    fn summary_page_and_bootstrap() {
        // 60 rows in subgroups of 4: a normal-theory summary with X̄ and S charts.
        let v: Vec<f64> = (0..60).map(|i| 74.0 + 0.01 * f64::from((i * 7) % 11) - 0.05).collect();
        let t = DataTable::new("t", vec![Column::numeric("Diameter", v)]).unwrap();
        let rows = RowStateManager::new(60);
        let request = CapabilityRequest {
            within: WithinSigma::PooledStdDev { subgroup_size: 4 },
            summary: true,
            ..CapabilityRequest::new(0, LIMITS)
        };
        let a = CapabilityAnalysis::run(&t, &rows, &request).unwrap();
        assert_eq!(a.title(), "Capability Summary of Diameter");
        let s = a.sections();
        let kinds: Vec<SectionKind> = s.iter().map(|x| x.kind).collect();
        assert_eq!(
            kinds,
            [
                SectionKind::CapabilityIndices,
                SectionKind::CapabilitySummary,
                SectionKind::CapabilityLocationChart,
                SectionKind::CapabilityDispersionChart,
                SectionKind::CapabilityLastSubgroups,
                SectionKind::CapabilityHistogram,
                SectionKind::CapabilityProbabilityPlot,
            ]
        );
        let SectionContent::ControlChart(loc) = &s[2].content else { panic!() };
        assert_eq!(loc.points.len(), 15);
        assert_eq!(loc.points[1].rows, vec![4, 5, 6, 7]);
        assert_eq!(s[3].title, "Standard deviation Chart");
        let SectionContent::Plot(last) = &s[4].content else { panic!() };
        assert_eq!(s[4].title, "Last 15 Subgroups with Specification Limits");
        assert_eq!(last.points.len(), 60);
        assert_eq!(last.line.as_ref().unwrap().len(), 15);
        assert_eq!(last.segments.len(), 3, "LSL, target and USL");
        let SectionContent::Plot(pp) = &s[6].content else { panic!() };
        assert_eq!(pp.points.len(), 60);
        assert_eq!(s[6].title, "Normal Probability Plot");
        // Bootstrap intervals are ignored for a normal model…
        assert!(a.bootstrap().is_none());
        // …and added to the percentile indices of a non-normal one, reproducibly.
        let boot = CapabilityRequest {
            model: CapabilityModel::Distribution(stat_core::distribution_fit::FitFamily::Normal),
            bootstrap: Some(BootstrapSettings { resamples: 200, seed: 3 }),
            ..request
        };
        let b = CapabilityAnalysis::run(&t, &rows, &boot).unwrap();
        let s = b.sections();
        let SectionContent::Table(idx) = &s[0].content else { panic!() };
        assert_eq!(idx.columns[2], "Lower 95% (bootstrap)");
        let ppk = idx.rows.iter().find(|r| r[0] == "Ppk").unwrap();
        let (lo, est, hi): (f64, f64, f64) =
            (ppk[2].parse().unwrap(), ppk[1].parse().unwrap(), ppk[3].parse().unwrap());
        assert!(lo < est && est < hi, "{lo} {est} {hi}");
        assert!(idx.rows.iter().any(|r| r[0] == "Bootstrap" && r[1].starts_with("200 resamples (seed 3)")));
        assert_eq!(s[6].kind, SectionKind::CapabilityProbabilityPlot);
        let again = CapabilityAnalysis::run(&t, &rows, &boot).unwrap();
        assert_eq!(again.bootstrap(), b.bootstrap());
        // Individuals: the last 25 values and a moving range chart.
        let imr = CapabilityRequest { within: WithinSigma::MovingRange, ..request };
        let c = CapabilityAnalysis::run(&t, &rows, &imr).unwrap();
        let s = c.sections();
        assert_eq!(s[3].title, "Moving range Chart");
        let SectionContent::ControlChart(mr) = &s[3].content else { panic!() };
        assert_eq!(mr.points[0].rows, vec![0, 1]);
        assert_eq!(s[4].title, "Last 25 Values with Specification Limits");
    }

    #[test]
    fn capability_by_group_and_in_spec_column() {
        // Two lines: A centred, B shifted towards the USL; a missing value and an excluded row.
        let mut v: Vec<f64> = (0..40).map(|i| 74.0 + 0.004 * f64::from((i * 7) % 9) - 0.016).collect();
        for x in &mut v[20..] {
            *x += 0.03;
        }
        v[5] = f64::NAN;
        let line: Vec<String> = (0..40).map(|i| if i < 20 { "A" } else { "B" }.to_owned()).collect();
        let t = DataTable::new("t", vec![Column::numeric("Diameter", v.clone()), Column::text("Line", line)]).unwrap();
        let mut rows = RowStateManager::new(40);
        rows.set_excluded([30], true);
        let request = CapabilityRequest {
            by: Some(1),
            group_order: GroupOrder::Ascending,
            probability_plot: true,
            ..CapabilityRequest::new(0, LIMITS)
        };
        let a = CapabilityAnalysis::run(&t, &rows, &request).unwrap();
        let s = a.sections();
        let kinds: Vec<SectionKind> = s.iter().map(|x| x.kind).collect();
        assert_eq!(
            kinds,
            [
                SectionKind::CapabilityIndices,
                SectionKind::CapabilityByGroup,
                SectionKind::CapabilityByGroupChart,
                SectionKind::CapabilitySummary,
                SectionKind::CapabilityNonconforming,
                SectionKind::CapabilityHistogram,
                SectionKind::CapabilityProbabilityPlot,
            ]
        );
        // Each group's row equals the study run on that group's values alone.
        let SectionContent::Table(g) = &s[1].content else { panic!() };
        assert_eq!(g.columns[0], "Line");
        let options = CapabilityOptions { within: request.within, confidence_level: CAPABILITY_LEVEL, box_cox: None };
        let a_vals: Vec<f64> = v[..20].iter().copied().filter(|x| x.is_finite()).collect();
        let b_vals: Vec<f64> = (20..40).filter(|&r| r != 30).map(|r| v[r]).collect();
        let direct_a = process_capability(&a_vals, &LIMITS, &options).unwrap();
        let direct_b = process_capability(&b_vals, &LIMITS, &options).unwrap();
        assert_eq!(g.rows[0][..2], ["A".to_owned(), "19".to_owned()]);
        assert_eq!(g.rows[1][1], "19");
        assert_eq!(g.cell(0, "Ppk"), Some(format_number(direct_a.overall.cpk, 4).as_str()));
        assert_eq!(g.cell(1, "Cpk"), Some(format_number(direct_b.within.cpk, 4).as_str()));
        let out_b = b_vals.iter().filter(|&&x| x > 74.05).count() as f64 * 100.0 / 19.0;
        assert_eq!(g.cell(1, "Out of spec (%)"), Some(format_number(out_b, 4).as_str()));
        // The chart puts the smaller Ppk (line B) first, with reference lines at 1 and 1.33.
        let SectionContent::Plot(c) = &s[2].content else { panic!() };
        assert!(direct_b.overall.cpk < direct_a.overall.cpk);
        assert_eq!(c.x_ticks.as_ref().unwrap()[0].1, "B");
        let items = &c.annotations.as_ref().unwrap().items;
        assert!((items[0].1 - direct_b.overall.cpk).abs() < 1e-12);
        assert_eq!(c.segments.len(), 4, "two stems and two reference lines");
        // The probability plot carries LSL, target and USL.
        let SectionContent::Plot(pp) = &s[6].content else { panic!() };
        assert_eq!(pp.segments.len(), 3);
        // The in-spec indicator: a formula that reproduces its values.
        let cols = a.saveable_columns();
        assert_eq!(cols.len(), 1);
        let col = &cols[0];
        assert_eq!(col.key, "in_spec");
        let eval = crate::formula::evaluate(&t, None, col.formula.as_deref().unwrap(), 0..40).unwrap();
        assert!(eval.failures.is_empty(), "{:?}", eval.failures);
        for (r, value) in eval.values.iter().enumerate() {
            let expected =
                if v[r].is_finite() { f64::from(u8::from((73.95..=74.05).contains(&v[r]))) } else { f64::NAN };
            match value {
                crate::formula::FormulaValue::Number(x) => assert_eq!(*x, expected, "row {r}"),
                crate::formula::FormulaValue::Missing => assert!(expected.is_nan(), "row {r}"),
                crate::formula::FormulaValue::Text(_) => panic!(),
            }
            if r != 30 && v[r].is_finite() {
                assert_eq!(col.values[r], expected);
            }
        }
        assert!(col.values[30].is_nan(), "excluded rows were not studied");
        // The by column must differ from the measurement.
        assert!(CapabilityAnalysis::run(&t, &rows, &CapabilityRequest { by: Some(0), ..request }).is_err());
    }
}
