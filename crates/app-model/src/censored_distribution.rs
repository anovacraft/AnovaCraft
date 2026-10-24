//! Censored values and detection limits in the Distribution report
//! (Helsel, *Statistics for Censored Environmental Data Using Minitab and R*,
//! 2nd ed., 2012).
//!
//! A value can be known exactly, known only to lie below a limit
//! (left-censored: a non-detect below a detection or reporting limit),
//! known only to lie above a limit (right-censored), or known to lie in an
//! interval. Two layouts say which:
//!
//! * **Indicator:** a second column flags censored values (nonzero = censored);
//!   the analysed column holds the value or, for a censored row, its limit,
//!   and every flagged value is censored in one chosen direction.
//! * **Interval:** the analysed column holds the lower end and a second column
//!   the upper end; equal ends are an exact value, a missing lower end a
//!   left-censored value (below the upper end), a missing upper end a
//!   right-censored value (above the lower end).
//!
//! The report then gives:
//!
//! * a summary of the exact and censored values and the detection limits;
//! * the nonparametric estimate of the distribution function — Turnbull's
//!   (1976) self-consistent estimate ([`stat_reliability::turnbull`]), which
//!   is the Kaplan–Meier estimate for right-censored data and the
//!   Kaplan–Meier estimate of the flipped data ("reversed" Kaplan–Meier,
//!   Helsel ch. 6) for left-censored data — with its percentiles (shown as
//!   "< limit" when they fall below the lowest detection limit) and mean
//!   (the mass below the lowest limit placed at that limit, so a high-side
//!   estimate, as Helsel notes);
//! * maximum-likelihood fits of the normal, lognormal, Weibull, gamma,
//!   exponential, logistic and log-logistic distributions to the censored
//!   data ([`stat_reliability::fit_censored`]), ranked by AIC, with their
//!   parameters, mean and median and Wald and likelihood-ratio limits;
//! * a plot of the nonparametric estimate with the fitted distribution
//!   functions;
//! * when every finite end is a whole number ≥ 0, Poisson and negative
//!   binomial fits to the counts with their censoring
//!   ([`stat_reliability::censored_count`]): a count below a detection limit
//!   `d` lies in `0 … ⌈d⌉ − 1`, one above `d` is at least `⌊d⌋ + 1`, and an
//!   interval `[l, u]` includes both ends.

use crate::format::format_number;
use crate::report::{PlotSpec, PointGroups, Polyline, ReportSection, RowPoint, SectionContent, SectionKind, TextTable};
use stat_core::distribution_fit::{FitFamily, FittedDistribution};
use stat_core::{ContinuousDistribution, Normal, Result, StatError};
use stat_data::{DataTable, JsonValue};
use stat_reliability::censored_count::{CountFamily, CountObservation, fit_censored_counts};
use stat_reliability::{
    CensorKind, CensoredFit, LifeData, LifeObservation, NonparametricEstimate, fit_censored, turnbull,
};

const DIGITS: usize = 6;
/// Confidence level of the intervals.
const LEVEL: f64 = 0.95;
/// Points on each fitted curve.
const CURVE_POINTS: usize = 160;

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

fn num(v: f64) -> String {
    format_number(v, DIGITS)
}

fn section(kind: SectionKind, title: impl Into<String>, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.into(), content }
}

/// Which way an indicated value is censored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CensorDirection {
    /// Below the recorded limit (a non-detect).
    #[default]
    Left,
    /// Above the recorded limit.
    Right,
}

impl CensorDirection {
    /// Both directions, in menu order.
    pub const ALL: [Self; 2] = [Self::Left, Self::Right];

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Left => "Below the value (left-censored; non-detects)",
            Self::Right => "Above the value (right-censored)",
        }
    }

    /// Stable key used in project files and scripts.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
        }
    }

    /// The direction with this key.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|d| d.key() == key)
    }
}

/// How censored values are marked (see the module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Censoring {
    /// A column whose nonzero values flag censored rows, all censored in
    /// `direction`.
    Indicator {
        /// The indicator column.
        column: usize,
        /// Which way flagged values are censored.
        direction: CensorDirection,
    },
    /// The analysed column is the lower end, `upper` the upper end.
    Interval {
        /// The upper-end column.
        upper: usize,
    },
}

impl Censoring {
    /// The extra column the censoring reads.
    #[must_use]
    pub fn column(&self) -> usize {
        match *self {
            Self::Indicator { column, .. } => column,
            Self::Interval { upper } => upper,
        }
    }

    /// Pass the column index through `f`.
    #[must_use]
    pub fn map_column(self, f: &impl Fn(usize) -> usize) -> Self {
        match self {
            Self::Indicator { column, direction } => Self::Indicator { column: f(column), direction },
            Self::Interval { upper } => Self::Interval { upper: f(upper) },
        }
    }

    /// JSON members of a distribution report with this censoring.
    pub(crate) fn json_members(&self, table: &DataTable, out: &mut Vec<(&'static str, JsonValue)>) {
        let name = |c: usize| JsonValue::String(table.column(c).map_or_else(String::new, |c| c.name.clone()));
        match *self {
            Self::Indicator { column, direction } => {
                out.push(("censor", name(column)));
                out.push(("censor_direction", JsonValue::String(direction.key().to_owned())));
            }
            Self::Interval { upper } => out.push(("upper", name(upper))),
        }
    }

    /// Read the censoring of a distribution report from its JSON form
    /// (`None` when it has none).
    pub(crate) fn from_json(v: &JsonValue, table: &DataTable) -> Result<Option<Self>> {
        let col = |key: &str| -> Result<Option<usize>> {
            match v.get(key) {
                None | Some(JsonValue::Null) => Ok(None),
                Some(x) => {
                    let name = x.as_str().ok_or_else(|| invalid(format!("report {key}: expected a column name")))?;
                    table
                        .find(name)
                        .map(Some)
                        .ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")))
                }
            }
        };
        if let Some(upper) = col("upper")? {
            return Ok(Some(Self::Interval { upper }));
        }
        let Some(column) = col("censor")? else { return Ok(None) };
        let direction = match v.get("censor_direction").and_then(JsonValue::as_str) {
            None => CensorDirection::Left,
            Some(k) => {
                CensorDirection::from_key(k).ok_or_else(|| invalid(format!("unknown censor direction {k:?}")))?
            }
        };
        Ok(Some(Self::Indicator { column, direction }))
    }
}

/// The observation of row `r`, or `None` when it is incomplete.
fn observation(value: f64, extra: f64, censoring: &Censoring) -> Option<LifeObservation> {
    match *censoring {
        Censoring::Indicator { direction, .. } => {
            if !value.is_finite() || !extra.is_finite() {
                return None;
            }
            Some(match (extra != 0.0, direction) {
                (false, _) => LifeObservation::exact(value, 1.0),
                (true, CensorDirection::Left) => LifeObservation::left(value, 1.0),
                (true, CensorDirection::Right) => LifeObservation::right(value, 1.0),
            })
        }
        Censoring::Interval { .. } => {
            let lo = if value.is_finite() { value } else { f64::NEG_INFINITY };
            let hi = if extra.is_finite() { extra } else { f64::INFINITY };
            if (!lo.is_finite() && !hi.is_finite()) || lo > hi {
                return None;
            }
            Some(LifeObservation::interval(lo, hi, 1.0))
        }
    }
}

/// Rows read from the table: `(row, observation)` plus counts of rows left out.
pub(crate) struct CensoredRows {
    pub rows: Vec<(usize, LifeObservation)>,
    pub n_missing: usize,
    pub n_excluded: usize,
}

/// Read the censored observations of `column`.
pub(crate) fn read_rows(
    table: &DataTable,
    states: &stat_data::RowStateManager,
    column: usize,
    censoring: &Censoring,
) -> Result<CensoredRows> {
    let numeric = |c: usize| -> Result<&[f64]> {
        let col = table.column(c).ok_or_else(|| invalid(format!("no column {c}")))?;
        col.as_numeric().ok_or_else(|| invalid(format!("{} must be numeric to hold censored values", col.name)))
    };
    let values = numeric(column)?;
    let extra = numeric(censoring.column())?;
    if censoring.column() == column {
        return Err(invalid("choose a different column for the censoring"));
    }
    let (mut rows, mut n_missing, mut n_excluded) = (Vec::new(), 0, 0);
    for r in 0..table.n_rows() {
        match observation(values[r], extra[r], censoring) {
            None => n_missing += 1,
            Some(_) if states.is_excluded(r) => n_excluded += 1,
            Some(o) => rows.push((r, o)),
        }
    }
    if rows.is_empty() {
        return Err(StatError::InsufficientData { required: 1, found: 0 });
    }
    Ok(CensoredRows { rows, n_missing, n_excluded })
}

/// Families fitted, in the order they are tried.
const FAMILIES: [FitFamily; 7] = [
    FitFamily::Normal,
    FitFamily::Lognormal,
    FitFamily::Weibull,
    FitFamily::Gamma,
    FitFamily::Exponential,
    FitFamily::Logistic,
    FitFamily::LogLogistic,
];

/// Mean of a fitted distribution, `∫₀¹ Q(p) dp`, by the trapezoid rule on
/// `p = Φ(z)`, `z ∈ [−8.5, 8.5]` (NaN when it does not exist).
fn fitted_mean(d: &FittedDistribution) -> f64 {
    let n01 = Normal::standard();
    let steps = 4000;
    let (a, b) = (-8.5, 8.5);
    let h = (b - a) / f64::from(steps);
    let mut sum = 0.0;
    for i in 0..=steps {
        let z = a + h * f64::from(i);
        let p = n01.cdf(z).unwrap_or(f64::NAN);
        let w = if i == 0 || i == steps { 0.5 } else { 1.0 };
        let q = d.quantile(p);
        if !q.is_finite() {
            return f64::NAN;
        }
        sum += w * q * n01.pdf(z);
    }
    let mean = sum * h;
    // A log-logistic with shape ≤ 1 has no mean: the integral then keeps growing.
    if d.family == FitFamily::LogLogistic && d.scale >= 1.0 { f64::NAN } else { mean }
}

/// The censored-data part of a Distribution report.
#[derive(Debug, Clone)]
pub struct CensoredDistribution {
    name: String,
    rows: Vec<(usize, LifeObservation)>,
    n_missing: usize,
    n_excluded: usize,
    nonparametric: std::result::Result<NonparametricEstimate, String>,
    fits: Vec<(FitFamily, std::result::Result<CensoredFit, String>)>,
    counts: Option<Vec<(CountFamily, std::result::Result<stat_reliability::censored_count::CountFit, String>)>>,
}

impl CensoredDistribution {
    /// Analyse the censored values of `rows`.
    pub(crate) fn new(name: String, read: CensoredRows) -> Result<Self> {
        let data = LifeData::new(read.rows.iter().map(|r| r.1).collect())?;
        // A non-detect "< d" means a value below d, never d itself, so for the
        // nonparametric estimate a left-censored upper end stops just short
        // of d: a value detected at d then does not share the non-detect's
        // interval, and the estimate is the reversed Kaplan–Meier (Helsel 2012).
        let open: Vec<LifeObservation> = read
            .rows
            .iter()
            .map(|r| {
                let o = r.1;
                if o.lower == f64::NEG_INFINITY && o.upper.is_finite() {
                    LifeObservation { upper: o.upper - o.upper.abs().max(1.0) * 1e-12, ..o }
                } else {
                    o
                }
            })
            .collect();
        let nonparametric = LifeData::new(open).and_then(|d| turnbull(&d)).map_err(|e| e.to_string());
        let fits = FAMILIES.into_iter().map(|f| (f, fit_censored(&data, f).map_err(|e| e.to_string()))).collect();
        let is_count = |v: f64| !v.is_finite() || (v >= 0.0 && v.fract() == 0.0);
        let counts = read.rows.iter().all(|(_, o)| is_count(o.lower) && is_count(o.upper)).then(|| {
            let obs: Vec<CountObservation> = read.rows.iter().map(|(_, o)| count_observation(o)).collect();
            [CountFamily::Poisson, CountFamily::NegativeBinomial]
                .into_iter()
                .map(|f| (f, fit_censored_counts(&obs, f, LEVEL).map_err(|e| e.to_string())))
                .collect()
        });
        Ok(Self {
            name,
            rows: read.rows,
            n_missing: read.n_missing,
            n_excluded: read.n_excluded,
            nonparametric,
            fits,
            counts,
        })
    }

    /// The maximum-likelihood fit of `family`, when it succeeded.
    #[must_use]
    pub fn fit(&self, family: FitFamily) -> Option<&CensoredFit> {
        self.fits.iter().find(|(f, _)| *f == family).and_then(|(_, r)| r.as_ref().ok())
    }

    /// The nonparametric (Turnbull / Kaplan–Meier) estimate, when it exists.
    #[must_use]
    pub fn nonparametric(&self) -> Option<&NonparametricEstimate> {
        self.nonparametric.as_ref().ok()
    }

    fn weight(&self, kind: CensorKind) -> usize {
        self.rows.iter().filter(|(_, o)| o.kind() == kind).count()
    }

    /// Report sections; `show_plot` adds the distribution-function plot.
    #[must_use]
    pub fn sections(&self, show_plot: bool) -> Vec<ReportSection> {
        let mut out = vec![self.summary()];
        out.push(self.nonparametric_section());
        out.push(self.fit_comparison());
        out.push(self.parameter_table());
        if show_plot {
            out.push(self.plot());
        }
        if let Some(counts) = &self.counts {
            out.push(Self::count_section(counts));
        }
        out
    }

    fn summary(&self) -> ReportSection {
        let mut t = TextTable::new(["Statistic", "Value"]);
        let n = self.rows.len();
        t.push(["N".to_owned(), n.to_string()]);
        if self.n_missing > 0 {
            t.push(["Missing or incomplete".to_owned(), self.n_missing.to_string()]);
        }
        if self.n_excluded > 0 {
            t.push(["Excluded".to_owned(), self.n_excluded.to_string()]);
        }
        let exact = self.weight(CensorKind::Exact);
        let left = self.weight(CensorKind::Left);
        let right = self.weight(CensorKind::Right);
        let interval = self.weight(CensorKind::Interval);
        t.push(["Exact values".to_owned(), exact.to_string()]);
        t.push(["Left-censored (below a limit)".to_owned(), left.to_string()]);
        t.push(["Right-censored (above a limit)".to_owned(), right.to_string()]);
        t.push(["Interval-censored".to_owned(), interval.to_string()]);
        t.push(["Percent censored".to_owned(), format_number(100.0 * (n - exact) as f64 / n as f64, 4)]);
        let mut limits: Vec<f64> =
            self.rows.iter().filter(|(_, o)| o.kind() == CensorKind::Left).map(|(_, o)| o.upper).collect();
        limits.sort_by(f64::total_cmp);
        limits.dedup();
        if !limits.is_empty() {
            let shown: Vec<String> = limits.iter().take(12).map(|v| num(*v)).collect();
            let more = if limits.len() > 12 { format!(" and {} more", limits.len() - 12) } else { String::new() };
            t.push(["Detection limits".to_owned(), format!("{}{more}", shown.join(", "))]);
        }
        let detected: Vec<f64> =
            self.rows.iter().filter(|(_, o)| o.kind() == CensorKind::Exact).map(|(_, o)| o.lower).collect();
        if let (Some(lo), Some(hi)) =
            (detected.iter().copied().reduce(f64::min), detected.iter().copied().reduce(f64::max))
        {
            t.push(["Smallest exact value".to_owned(), num(lo)]);
            t.push(["Largest exact value".to_owned(), num(hi)]);
        }
        section(SectionKind::CensoredSummary, format!("Censored Values: {}", self.name), SectionContent::Table(t))
    }

    fn nonparametric_title(&self) -> &'static str {
        let (left, right, interval) =
            (self.weight(CensorKind::Left), self.weight(CensorKind::Right), self.weight(CensorKind::Interval));
        match (left > 0, right > 0 || interval > 0) {
            (true, false) => "Nonparametric Estimate (reversed Kaplan–Meier for left-censored values)",
            (false, true) if interval == 0 => "Nonparametric Estimate (Kaplan–Meier)",
            (false, false) => "Nonparametric Estimate (empirical distribution)",
            _ => "Nonparametric Estimate (Turnbull)",
        }
    }

    fn nonparametric_section(&self) -> ReportSection {
        let title = self.nonparametric_title();
        let np = match &self.nonparametric {
            Ok(np) => np,
            Err(e) => return section(SectionKind::CensoredNonparametric, title, SectionContent::Note(e.clone())),
        };
        let mut t = TextTable::new(["Statistic", "Estimate"]);
        for (p, label) in [
            (0.1, "10%"),
            (0.25, "25% (lower quartile)"),
            (0.5, "50% (median)"),
            (0.75, "75% (upper quartile)"),
            (0.9, "90%"),
        ] {
            t.push([label.to_owned(), percentile_text(np, p)]);
        }
        let (mean, note) = nonparametric_mean(np);
        t.push([format!("Mean{note}"), num(mean)]);
        section(SectionKind::CensoredNonparametric, title, SectionContent::Table(t))
    }

    fn ranked(&self) -> Vec<&CensoredFit> {
        let mut ok: Vec<&CensoredFit> = self.fits.iter().filter_map(|(_, r)| r.as_ref().ok()).collect();
        ok.sort_by(|a, b| a.aic().total_cmp(&b.aic()));
        ok
    }

    fn fit_comparison(&self) -> ReportSection {
        let mut t = TextTable::new(["Distribution", "Parameters", "−2 log-likelihood", "AIC", "BIC", "ΔAIC"]);
        let ranked = self.ranked();
        let best = ranked.first().map_or(f64::NAN, |f| f.aic());
        for f in &ranked {
            let d = &f.distribution;
            let params: Vec<String> = d.parameters().iter().map(|p| format!("{} = {}", p.name, num(p.value))).collect();
            t.push([
                f.family().label().to_owned(),
                params.join("; "),
                num(-2.0 * f.log_likelihood()),
                num(f.aic()),
                num(f.bic()),
                format_number(f.aic() - best, 4),
            ]);
        }
        for (family, r) in &self.fits {
            if let Err(e) = r {
                t.push([
                    family.label().to_owned(),
                    format!("not fitted: {e}"),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                ]);
            }
        }
        section(
            SectionKind::CensoredFits,
            "Censored Maximum-Likelihood Fits (ranked by AIC; smaller is better)",
            SectionContent::Table(t),
        )
    }

    fn parameter_table(&self) -> ReportSection {
        let level = format!("{}%", format_number(LEVEL * 100.0, 3));
        let mut t = TextTable::new(vec![
            "Distribution".to_owned(),
            "Quantity".to_owned(),
            "Estimate".to_owned(),
            "Std error".to_owned(),
            format!("Wald lower {level}"),
            format!("Wald upper {level}"),
            format!("Likelihood-ratio lower {level}"),
            format!("Likelihood-ratio upper {level}"),
        ]);
        let z = Normal::standard().quantile(0.5 + LEVEL / 2.0).unwrap_or(f64::NAN);
        for f in self.ranked() {
            let name = f.family().label().to_owned();
            let mut push = |q: &str, e: f64, se: f64, wald: (f64, f64), lr: Option<(f64, f64)>| {
                let lr = lr.unwrap_or((f64::NAN, f64::NAN));
                t.push([
                    name.clone(),
                    q.to_owned(),
                    num(e),
                    format_number(se, 4),
                    num(wald.0),
                    num(wald.1),
                    num(lr.0),
                    num(lr.1),
                ]);
            };
            for p in f.parameters(LEVEL, true) {
                push(p.name, p.estimate, p.std_error, p.wald, p.likelihood_ratio);
            }
            let median = f.quantile(0.5, LEVEL, true);
            push("Median", median.estimate, median.std_error, median.wald, median.likelihood_ratio);
            let mean = fitted_mean(&f.distribution);
            if mean.is_finite() {
                let se = f.delta_std_error(&fitted_mean);
                let positive = f.family().needs_positive();
                let wald = if positive && mean > 0.0 {
                    let s = se / mean;
                    (mean * (-z * s).exp(), mean * (z * s).exp())
                } else {
                    (mean - z * se, mean + z * se)
                };
                push("Mean", mean, se, wald, None);
            }
        }
        section(
            SectionKind::CensoredParameters,
            "Fitted Parameters, Median and Mean (Wald limits on the log scale for positive quantities)",
            SectionContent::Table(t),
        )
    }

    fn plot(&self) -> ReportSection {
        let mut labels = Vec::new();
        let mut lines: Vec<Polyline> = Vec::new();
        let mut points = Vec::new();
        let mut membership = Vec::new();
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for (_, o) in &self.rows {
            for v in [o.lower, o.upper] {
                if v.is_finite() {
                    (lo, hi) = (lo.min(v), hi.max(v));
                }
            }
        }
        if !lo.is_finite() {
            (lo, hi) = (0.0, 1.0);
        }
        let pad = 0.05 * (hi - lo).max(1e-9 * hi.abs().max(1.0));
        let (x0, x1) = (lo - pad, hi + pad);
        let kinds = [
            (CensorKind::Exact, "Exact values"),
            (CensorKind::Left, "Detection limits (left-censored)"),
            (CensorKind::Right, "Right-censored limits"),
            (CensorKind::Interval, "Interval-censored (midpoints)"),
        ];
        if let Ok(np) = &self.nonparametric {
            for (kind, label) in kinds {
                let g = labels.len();
                let mut any = false;
                for (row, o) in self.rows.iter().filter(|(_, o)| o.kind() == kind) {
                    let x = match kind {
                        CensorKind::Exact | CensorKind::Right => o.lower,
                        CensorKind::Left => o.upper,
                        CensorKind::Interval => 0.5 * (o.lower + o.upper),
                    };
                    if let Some(y) = crate::life_stress::plotting_position(np, x).or_else(|| np.cdf_at(x)) {
                        points.push(RowPoint { row: *row, x, y });
                        membership.push(g);
                        any = true;
                    }
                }
                if any {
                    labels.push(label.to_owned());
                    lines.push(Vec::new());
                }
            }
            let step: Polyline = np.survival_polyline(x0).into_iter().map(|[x, s]| [x, 1.0 - s]).collect();
            labels.push(self.nonparametric_title().replace("Nonparametric Estimate (", "Nonparametric (").to_owned());
            lines.push(step);
        }
        for f in self.ranked().into_iter().take(3) {
            let d = &f.distribution;
            let line: Polyline = (0..=CURVE_POINTS)
                .map(|i| x0 + (x1 - x0) * i as f64 / CURVE_POINTS as f64)
                .map(|x| [x, d.cdf(x)])
                .filter(|p| p[1].is_finite())
                .collect();
            labels.push(format!("{} fit", f.family().label()));
            lines.push(line);
        }
        let spec = PlotSpec {
            x_label: self.name.clone(),
            y_label: "Cumulative probability".to_owned(),
            points,
            groups: Some(PointGroups { title: String::new(), labels, membership, lines }),
            ..PlotSpec::default()
        };
        section(
            SectionKind::CensoredPlot,
            "Distribution Function (nonparametric estimate with the three best fits)",
            SectionContent::Plot(Box::new(spec)),
        )
    }

    fn count_section(
        counts: &[(CountFamily, std::result::Result<stat_reliability::censored_count::CountFit, String>)],
    ) -> ReportSection {
        let level = format!("{}%", format_number(LEVEL * 100.0, 3));
        let mut t = TextTable::new(vec![
            "Distribution".to_owned(),
            "Parameter".to_owned(),
            "Estimate".to_owned(),
            "Std error".to_owned(),
            format!("Lower {level}"),
            format!("Upper {level}"),
            "−2 log-likelihood".to_owned(),
            "AIC".to_owned(),
        ]);
        for (family, r) in counts {
            match r {
                Ok(fit) => {
                    for (i, p) in fit.parameters.iter().enumerate() {
                        let (ll, aic) = if i == 0 {
                            (num(-2.0 * fit.log_likelihood), num(fit.aic))
                        } else {
                            (String::new(), String::new())
                        };
                        t.push(vec![
                            family.label().to_owned(),
                            p.name.to_owned(),
                            num(p.estimate),
                            format_number(p.std_error, 4),
                            num(p.wald.0),
                            num(p.wald.1),
                            ll,
                            aic,
                        ]);
                    }
                }
                Err(e) => t.push(vec![
                    family.label().to_owned(),
                    format!("not fitted: {e}"),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                ]),
            }
        }
        section(
            SectionKind::CensoredCountFits,
            "Count Fits with Detection Limits (a count below a limit d lies in 0 … d − 1)",
            SectionContent::Table(t),
        )
    }
}

/// The count observation of a censored value.
fn count_observation(o: &LifeObservation) -> CountObservation {
    match o.kind() {
        CensorKind::Exact => CountObservation { lower: o.lower, upper: o.lower, weight: o.weight },
        CensorKind::Left => CountObservation { lower: 0.0, upper: (o.upper.ceil() - 1.0).max(0.0), weight: o.weight },
        CensorKind::Right => CountObservation { lower: o.lower.floor() + 1.0, upper: f64::INFINITY, weight: o.weight },
        CensorKind::Interval => CountObservation { lower: o.lower.max(0.0), upper: o.upper, weight: o.weight },
    }
}

/// The `p` percentile of the nonparametric estimate as text: a value, or
/// "< limit" / "> limit" when it falls where the estimate only bounds it.
fn percentile_text(np: &NonparametricEstimate, p: f64) -> String {
    let cum = np.cumulative();
    for (j, iv) in np.intervals.iter().enumerate() {
        if cum[j] + 1e-12 >= p {
            if iv.lower == iv.upper {
                return num(iv.upper);
            }
            if !iv.lower.is_finite() {
                return format!("< {}", num(iv.upper));
            }
            if !iv.upper.is_finite() {
                return format!("> {}", num(iv.lower));
            }
            let before = if j == 0 { 0.0 } else { cum[j - 1] };
            if p <= before + 1e-12 {
                return num(iv.lower);
            }
            return format!("{} to {}", num(iv.lower), num(iv.upper));
        }
    }
    "—".to_owned()
}

/// The mean of the nonparametric estimate with a note on how mass on
/// unbounded or proper intervals was placed.
fn nonparametric_mean(np: &NonparametricEstimate) -> (f64, &'static str) {
    let mut note = "";
    let mut sum = 0.0;
    for iv in &np.intervals {
        let x = if iv.lower == iv.upper {
            iv.lower
        } else if !iv.lower.is_finite() {
            note = " (mass below the lowest detection limit placed at that limit: an upper-side estimate)";
            iv.upper
        } else if !iv.upper.is_finite() {
            note = " (mass above the largest value placed at it: a lower-side estimate)";
            iv.lower
        } else {
            0.5 * (iv.lower + iv.upper)
        };
        sum += iv.probability * x;
    }
    (sum, note)
}

#[cfg(test)]
mod tests;
