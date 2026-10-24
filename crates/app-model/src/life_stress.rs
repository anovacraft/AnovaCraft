//! Life–stress models (accelerated life tests) bound to a dataset
//! ([`stat_reliability::life_stress`]): a Weibull, lognormal, log-logistic
//! or exponential life distribution whose location depends on one or two
//! stress columns through Arrhenius, Eyring, inverse-power or exponential
//! relationships, fitted to censored failure times; activation energies and
//! power exponents, acceleration factors, the life distribution at use
//! conditions, a life–stress plot, a probability plot per stress level with
//! the fitted lines, and likelihood-ratio checks against per-level fits.
//!
//! **Layouts** are those of life distributions ([`LifeDataLayout`]): a time
//! column with an optional censor column, or start and end columns of the
//! interval in which each unit failed; an optional frequency column counts
//! units per row. Excluded rows and rows without usable times or stresses
//! are left out.

use crate::format::{format_number, format_p_value};
use crate::life_distribution::LifeDataLayout;
use crate::project::{SavedReport, invalid as json_invalid, member, s};
use crate::report::{
    Annotations, PlotSpec, PointGroups, Polyline, ReportSection, RowPoint, SectionContent, SectionKind, TextTable,
};
use crate::survival::StatusCoding;
use stat_core::{Result, StatError};
use stat_data::{DataTable, JsonValue, RowStateManager};
use stat_reliability::life_regression::RegressionFamily;
use stat_reliability::life_stress::{
    LifeStressData, LifeStressFit, LifeStressModel, StressTransform, StressVariable, TemperatureUnit,
    coefficient_names, fit_life_stress,
};
use stat_reliability::{CensorKind, IntervalEstimate, LifeObservation, NonparametricEstimate, turnbull};

const DIGITS: usize = 5;
const CURVE_POINTS: usize = 120;

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

fn section(kind: SectionKind, title: impl Into<String>, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.into(), content }
}

fn num(v: f64) -> String {
    format_number(v, DIGITS)
}

/// One stress column and how it acts on life.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StressColumn {
    /// The column.
    pub column: usize,
    /// The relationship.
    pub transform: StressTransform,
    /// Temperature units (temperature relationships).
    pub unit: TemperatureUnit,
}

impl StressColumn {
    /// The engine's stress variable.
    #[must_use]
    pub fn variable(&self) -> StressVariable {
        StressVariable { transform: self.transform, unit: self.unit }
    }
}

/// Columns and settings of a life–stress analysis.
#[derive(Debug, Clone, PartialEq)]
pub struct LifeStressRequest {
    /// Data layout.
    pub layout: LifeDataLayout,
    /// Optional frequency column.
    pub frequency: Option<usize>,
    /// One or two stresses.
    pub stresses: Vec<StressColumn>,
    /// Include the interaction of two stresses.
    pub interaction: bool,
    /// Life distribution.
    pub family: RegressionFamily,
    /// `ln σ` linear in the stresses.
    pub stress_dependent_scale: bool,
    /// Use conditions (one value per stress).
    pub use_stress: Vec<f64>,
    /// Confidence level.
    pub confidence: f64,
    /// Fractions failing whose times at use conditions are estimated.
    pub probabilities: Vec<f64>,
    /// Times whose failure probabilities at use conditions are estimated.
    pub times: Vec<f64>,
}

impl LifeStressRequest {
    /// Default confidence level.
    pub const DEFAULT_CONFIDENCE: f64 = 0.95;
    /// Default percentiles.
    pub const DEFAULT_PROBABILITIES: [f64; 3] = [0.01, 0.1, 0.5];

    /// A Weibull–Arrhenius model of `layout` against `stress` (°C), used at `use_stress`.
    #[must_use]
    pub fn new(layout: LifeDataLayout, stress: usize, use_stress: f64) -> Self {
        Self {
            layout,
            frequency: None,
            stresses: vec![StressColumn {
                column: stress,
                transform: StressTransform::Arrhenius,
                unit: TemperatureUnit::Celsius,
            }],
            interaction: false,
            family: RegressionFamily::Weibull,
            stress_dependent_scale: false,
            use_stress: vec![use_stress],
            confidence: Self::DEFAULT_CONFIDENCE,
            probabilities: Self::DEFAULT_PROBABILITIES.to_vec(),
            times: Vec::new(),
        }
    }

    /// Every column the request reads.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        let mut out = layout_columns(&self.layout);
        out.extend(self.frequency);
        out.extend(self.stresses.iter().map(|s| s.column));
        out
    }

    /// Pass every column index through `f`.
    pub fn map_columns(&mut self, f: &impl Fn(usize) -> usize) {
        map_layout(&mut self.layout, f);
        self.frequency = self.frequency.map(f);
        for s in &mut self.stresses {
            s.column = f(s.column);
        }
    }

    fn model(&self) -> LifeStressModel {
        LifeStressModel {
            family: self.family,
            stresses: self.stresses.iter().map(StressColumn::variable).collect(),
            interaction: self.interaction,
            stress_dependent_scale: self.stress_dependent_scale,
        }
    }
}

pub(crate) fn layout_columns(layout: &LifeDataLayout) -> Vec<usize> {
    match *layout {
        LifeDataLayout::Times { time, censor, .. } => std::iter::once(time).chain(censor).collect(),
        LifeDataLayout::Intervals { start, end } => vec![start, end],
    }
}

pub(crate) fn map_layout(layout: &mut LifeDataLayout, f: &impl Fn(usize) -> usize) {
    match layout {
        LifeDataLayout::Times { time, censor, .. } => {
            *time = f(*time);
            *censor = censor.map(f);
        }
        LifeDataLayout::Intervals { start, end } => {
            *start = f(*start);
            *end = f(*end);
        }
    }
}

/// A cell's number, NaN when missing or not numeric.
pub(crate) fn value(table: &DataTable, col: usize, row: usize) -> f64 {
    table.column(col).and_then(|c| c.as_numeric()).map_or(f64::NAN, |v| v[row])
}

/// The observation of row `r` under `layout` (with frequency `weight`).
pub(crate) fn observation(
    table: &DataTable,
    layout: &LifeDataLayout,
    r: usize,
    weight: f64,
) -> Option<LifeObservation> {
    match *layout {
        LifeDataLayout::Times { time, censor, coding } => {
            let t = value(table, time, r);
            if !t.is_finite() {
                return None;
            }
            let censored = match censor {
                Some(c) => {
                    let s = value(table, c, r);
                    if !s.is_finite() {
                        return None;
                    }
                    (s != 0.0) == (coding == StatusCoding::NonzeroIsCensored)
                }
                None => false,
            };
            Some(if censored { LifeObservation::right(t, weight) } else { LifeObservation::exact(t, weight) })
        }
        LifeDataLayout::Intervals { start, end } => {
            let (a, b) = (value(table, start, r), value(table, end, r));
            let lo = if a.is_finite() { a } else { f64::NEG_INFINITY };
            let hi = if b.is_finite() { b } else { f64::INFINITY };
            if (!lo.is_finite() && !hi.is_finite()) || lo > hi {
                return None;
            }
            Some(LifeObservation::interval(lo, hi, weight))
        }
    }
}

pub(crate) fn time_name(table: &DataTable, layout: &LifeDataLayout) -> String {
    let name = |i: usize| table.column(i).map_or_else(String::new, |c| c.name.clone());
    match *layout {
        LifeDataLayout::Times { time, .. } => name(time),
        LifeDataLayout::Intervals { start, end } => format!("{}–{}", name(start), name(end)),
    }
}

/// Ticks on a log scale between `lo` and `hi`.
pub(crate) fn log_ticks(lo: f64, hi: f64) -> Vec<f64> {
    let lo = lo.max(1e-300);
    let hi = hi.max(lo * 1.0001);
    let mut out = Vec::new();
    let start = lo.log10().floor() as i32 - 1;
    let stop = hi.log10().ceil() as i32 + 1;
    let decades = stop - start;
    let mantissas: &[f64] = if decades > 4 {
        &[1.0]
    } else if decades > 2 {
        &[1.0, 2.0, 5.0]
    } else {
        &[1.0, 1.5, 2.0, 3.0, 5.0, 7.0]
    };
    for k in start..=stop {
        for &m in mantissas {
            let v = m * 10f64.powi(k);
            if v >= lo / 1.6 && v <= hi * 1.6 {
                out.push(v);
            }
        }
    }
    out
}

/// Probability-axis ticks for a standardized scale.
pub(crate) fn probability_ticks(quantile: &dyn Fn(f64) -> f64, y_lo: f64, y_hi: f64) -> Vec<(f64, String)> {
    let pad = 0.15 * (y_hi - y_lo).max(1.0);
    [
        0.0001, 0.0005, 0.001, 0.002, 0.005, 0.01, 0.02, 0.05, 0.1, 0.2, 0.3, 0.5, 0.7, 0.8, 0.9, 0.95, 0.98, 0.99,
        0.995, 0.999, 0.9999,
    ]
    .iter()
    .map(|&p| (quantile(p), p))
    .filter(|(y, _)| *y >= y_lo - pad && *y <= y_hi + pad)
    .map(|(y, p)| (y, format!("{}%", format_number(p * 100.0, 3))))
    .collect()
}

/// Plotting position at time `t`: the midpoint of the nonparametric
/// estimate's jump there, or the estimate itself.
pub(crate) fn plotting_position(np: &NonparametricEstimate, t: f64) -> Option<f64> {
    let cum = np.cumulative();
    for (j, iv) in np.intervals.iter().enumerate() {
        if iv.upper == t && iv.probability > 0.0 {
            let before = if j == 0 { 0.0 } else { cum[j - 1] };
            return Some(0.5 * (before + cum[j]));
        }
    }
    np.cdf_at(t)
}

/// One analysed row.
#[derive(Debug, Clone, Copy)]
struct UsedRow {
    row: usize,
    observation: LifeObservation,
}

/// A computed life–stress analysis.
#[derive(Debug, Clone)]
pub struct LifeStressAnalysis {
    request: LifeStressRequest,
    time_name: String,
    stress_names: Vec<String>,
    rows: Vec<UsedRow>,
    fit: LifeStressFit,
    n_skipped: usize,
}

impl LifeStressAnalysis {
    /// Fit the model to the included rows.
    ///
    /// # Errors
    /// Missing or non-numeric columns, a column used twice, no or more than
    /// two stresses, use conditions that do not match the stresses or suit
    /// their relationships, a confidence level outside (0.5, 1), no usable
    /// rows, or errors of the fit.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &LifeStressRequest) -> Result<Self> {
        let cols = request.columns();
        if (1..cols.len()).any(|i| cols[..i].contains(&cols[i])) {
            return Err(invalid("choose a different column for each role"));
        }
        for &c in &cols {
            let col = table.column(c).ok_or_else(|| invalid(format!("no column {c}")))?;
            if col.as_numeric().is_none() {
                return Err(invalid(format!("{} must be numeric", col.name)));
            }
        }
        if request.stresses.is_empty() || request.stresses.len() > 2 {
            return Err(invalid("choose one or two stress columns"));
        }
        if request.use_stress.len() != request.stresses.len()
            || !request.stresses.iter().zip(&request.use_stress).all(|(s, u)| s.variable().valid(*u))
        {
            return Err(invalid(
                "give a use condition for each stress, valid for its relationship (temperatures above absolute \
                 zero, positive inverse-power stresses)",
            ));
        }
        if !(request.confidence > 0.5 && request.confidence < 1.0) {
            return Err(invalid("the confidence level must be between 0.5 and 1"));
        }
        if !request.family.is_log() {
            return Err(invalid("choose a life distribution (Weibull, lognormal, log-logistic or exponential)"));
        }
        let mut rows = Vec::new();
        let mut stresses = Vec::new();
        for r in 0..table.n_rows() {
            if states.is_excluded(r) {
                continue;
            }
            let weight = request.frequency.map_or(1.0, |c| value(table, c, r));
            if !(weight.is_finite() && weight > 0.0) {
                continue;
            }
            let Some(observation) = observation(table, &request.layout, r, weight) else { continue };
            let s: Vec<f64> = request.stresses.iter().map(|sc| value(table, sc.column, r)).collect();
            if s.iter().any(|v| !v.is_finite()) {
                continue;
            }
            rows.push(UsedRow { row: r, observation });
            stresses.push(s);
        }
        if rows.is_empty() {
            return Err(invalid("no rows with usable times and stresses"));
        }
        let data = LifeStressData { observations: rows.iter().map(|u| u.observation).collect(), stresses };
        let fit = fit_life_stress(&data, &request.model())?;
        let name = |i: usize| table.column(i).map_or_else(String::new, |c| c.name.clone());
        Ok(Self {
            request: request.clone(),
            time_name: time_name(table, &request.layout),
            stress_names: request.stresses.iter().map(|s| name(s.column)).collect(),
            n_skipped: table.n_rows() - rows.len(),
            rows,
            fit,
        })
    }

    /// The fit.
    #[must_use]
    pub fn fit(&self) -> &LifeStressFit {
        &self.fit
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &LifeStressRequest {
        &self.request
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        format!("Life–Stress Model: {} by {}", self.time_name, self.stress_names.join(" and "))
    }

    fn level_text(&self) -> String {
        format!("{}%", format_number(self.request.confidence * 100.0, 4))
    }

    fn stress_text(&self, s: &[f64]) -> String {
        self.stress_names
            .iter()
            .zip(s)
            .zip(&self.request.stresses)
            .map(|((n, v), sc)| {
                let unit = if sc.transform.is_temperature() {
                    match sc.unit {
                        TemperatureUnit::Celsius => " °C",
                        TemperatureUnit::Kelvin => " K",
                    }
                } else {
                    ""
                };
                format!("{n} {}{unit}", format_number(*v, 6))
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn interval_headings(&self, first: &[&str]) -> Vec<String> {
        let level = self.level_text();
        let mut cols: Vec<String> = first.iter().map(|s| (*s).to_owned()).collect();
        cols.extend([
            "Std error".to_owned(),
            format!("Wald lower {level}"),
            format!("Wald upper {level}"),
            format!("Likelihood-ratio lower {level}"),
            format!("Likelihood-ratio upper {level}"),
        ]);
        cols
    }

    fn interval_cells(e: &IntervalEstimate) -> Vec<String> {
        let lr = e.likelihood_ratio.unwrap_or((f64::NAN, f64::NAN));
        vec![num(e.estimate), format_number(e.std_error, 4), num(e.wald.0), num(e.wald.1), num(lr.0), num(lr.1)]
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let mut out = vec![self.summary(), self.statistics(), self.parameters()];
        let derived = self.fit.stress_parameters(&self.stress_names, self.request.confidence, true);
        if !derived.is_empty() {
            let mut t = TextTable::new(self.interval_headings(&["Quantity", "Estimate"]));
            for d in derived {
                let mut row = vec![d.name];
                row.extend(Self::interval_cells(&d.estimate));
                t.push(row);
            }
            out.push(section(
                SectionKind::LifeStressDerived,
                "Activation Energy and Power Exponents",
                SectionContent::Table(t),
            ));
        }
        out.push(self.acceleration());
        if !self.request.probabilities.is_empty() {
            out.push(self.use_percentiles());
        }
        if !self.request.times.is_empty() {
            out.push(self.use_probabilities());
        }
        out.push(self.life_stress_plot());
        out.push(self.probability_plot());
        out.push(self.checks());
        out.push(self.level_fits());
        out
    }

    fn summary(&self) -> ReportSection {
        let mut heads = self.stress_names.clone();
        heads.extend(
            ["Units", "Failures (time known)", "Still running", "Left- or interval-censored"].map(String::from),
        );
        let mut t = TextTable::new(heads);
        for l in &self.fit.levels {
            let mut row: Vec<String> = l.stresses.iter().map(|v| format_number(*v, 6)).collect();
            row.extend([
                num(l.units()),
                num(l.data.weight_of(CensorKind::Exact)),
                num(l.data.weight_of(CensorKind::Right)),
                num(l.data.weight_of(CensorKind::Left) + l.data.weight_of(CensorKind::Interval)),
            ]);
            t.push(row);
        }
        let skipped = if self.n_skipped > 0 {
            format!("; {} rows excluded or incomplete", self.n_skipped)
        } else {
            String::new()
        };
        section(SectionKind::LifeStressSummary, format!("Test Levels{skipped}"), SectionContent::Table(t))
    }

    fn statistics(&self) -> ReportSection {
        let f = &self.fit.fit;
        let relationships = self
            .request
            .stresses
            .iter()
            .zip(&self.stress_names)
            .map(|(s, n)| format!("{} in {n}", s.transform.label()))
            .collect::<Vec<_>>()
            .join("; ");
        let mut t = TextTable::new(["Quantity", "Value"]);
        t.push(vec!["Life distribution".to_owned(), self.request.family.label().to_owned()]);
        t.push(vec!["Relationship".to_owned(), relationships]);
        if self.request.interaction {
            t.push(vec!["Interaction".to_owned(), "Included".to_owned()]);
        }
        if !self.request.family.fixed_scale() {
            let scale =
                if self.request.stress_dependent_scale { "Log scale linear in the stresses" } else { "Constant" };
            t.push(vec!["Scale (shape)".to_owned(), scale.to_owned()]);
        }
        t.push(vec!["Units".to_owned(), num(f.total_weight())]);
        t.push(vec!["Parameters".to_owned(), f.n_parameters().to_string()]);
        t.push(vec!["Log-likelihood".to_owned(), num(f.log_likelihood)]);
        t.push(vec!["AIC".to_owned(), num(f.aic())]);
        section(SectionKind::LifeStressFitStatistics, "Model Fit (maximum likelihood)", SectionContent::Table(t))
    }

    fn parameters(&self) -> ReportSection {
        let level = self.request.confidence;
        let f = &self.fit.fit;
        let mut t = TextTable::new(self.interval_headings(&["Parameter", "Estimate"]));
        let names = coefficient_names(&self.fit.model, &self.stress_names);
        let p = f.location_coefficients.len();
        for (j, name) in names.iter().enumerate() {
            let mut a = vec![0.0; j + 1];
            a[j] = 1.0;
            let mut row = vec![name.clone()];
            row.extend(Self::interval_cells(&f.linear_function(&a, 0.0, level, true)));
            t.push(row);
        }
        if !self.request.family.fixed_scale() {
            if self.request.stress_dependent_scale {
                for k in 0..f.scale_coefficients.len() {
                    let mut a = vec![0.0; p + k + 1];
                    a[p + k] = 1.0;
                    let name = if k == 0 {
                        "ln σ: intercept γ₀".to_owned()
                    } else {
                        let what = match self.request.stresses[k - 1].transform {
                            StressTransform::Arrhenius | StressTransform::Eyring => "1/kT".to_owned(),
                            StressTransform::InversePower => format!("ln {}", self.stress_names[k - 1]),
                            StressTransform::Exponential => self.stress_names[k - 1].clone(),
                        };
                        format!("ln σ: slope γ{} ({what})", ["₀", "₁", "₂"][k.min(2)])
                    };
                    let mut row = vec![name];
                    row.extend(Self::interval_cells(&f.linear_function(&a, 0.0, level, true)));
                    t.push(row);
                }
            } else {
                let mut a = vec![0.0; p + 1];
                a[p] = 1.0;
                let ln_sigma = f.linear_function(&a, 0.0, level, true);
                let map = |e: &IntervalEstimate, g: &dyn Fn(f64) -> f64| {
                    let order = |(a, b): (f64, f64)| if a <= b { (a, b) } else { (b, a) };
                    let est = g(e.estimate);
                    IntervalEstimate {
                        name: e.name,
                        estimate: est,
                        std_error: est.abs() * e.std_error,
                        wald: order((g(e.wald.0), g(e.wald.1))),
                        likelihood_ratio: e.likelihood_ratio.map(|(a, b)| order((g(a), g(b)))),
                    }
                };
                let mut row = vec!["Scale σ (of log life)".to_owned()];
                row.extend(Self::interval_cells(&map(&ln_sigma, &f64::exp)));
                t.push(row);
                if matches!(self.request.family, RegressionFamily::Weibull | RegressionFamily::LogLogistic) {
                    let mut row = vec!["Shape β = 1/σ".to_owned()];
                    row.extend(Self::interval_cells(&map(&ln_sigma, &|x: f64| (-x).exp())));
                    t.push(row);
                }
            }
        }
        section(
            SectionKind::LifeStressParameters,
            format!(
                "{} Parameter Estimates (location of log life μ = β₀ + Σ βⱼ xⱼ{})",
                self.request.family.label(),
                if self.request.stresses.iter().any(|s| s.transform == StressTransform::Eyring) {
                    " − ln T"
                } else {
                    ""
                }
            ),
            SectionContent::Table(t),
        )
    }

    fn acceleration(&self) -> ReportSection {
        let level = self.request.confidence;
        let mut heads: Vec<String> = self.stress_names.clone();
        heads.extend(self.interval_headings(&["Acceleration factor"]));
        let mut t = TextTable::new(heads);
        for l in &self.fit.levels {
            let af = self.fit.acceleration_factor(&l.stresses, &self.request.use_stress, level, true);
            let mut row: Vec<String> = l.stresses.iter().map(|v| format_number(*v, 6)).collect();
            row.extend(Self::interval_cells(&af));
            t.push(row);
        }
        let note = if self.request.stress_dependent_scale {
            "; with a stress-dependent scale, the ratio of the lives at μ (the 63.2% Weibull or median lognormal life)"
        } else {
            ""
        };
        section(
            SectionKind::LifeStressAcceleration,
            format!(
                "Acceleration Factors (life at use conditions {} ÷ life at each test level{note})",
                self.stress_text(&self.request.use_stress)
            ),
            SectionContent::Table(t),
        )
    }

    fn use_percentiles(&self) -> ReportSection {
        let level = self.request.confidence;
        let mut t = TextTable::new(self.interval_headings(&["Fraction failing", "Time"]));
        for &p in &self.request.probabilities {
            let q = self.fit.quantile(&self.request.use_stress, p, level, true);
            let mut row = vec![format!("{}%", format_number(p * 100.0, 4))];
            row.extend(Self::interval_cells(&q));
            t.push(row);
        }
        section(
            SectionKind::LifeStressUsePercentiles,
            format!("Life at Use Conditions ({}): Percentiles", self.stress_text(&self.request.use_stress)),
            SectionContent::Table(t),
        )
    }

    fn use_probabilities(&self) -> ReportSection {
        let level = self.request.confidence;
        let mut t = TextTable::new(self.interval_headings(&["Time", "Failure probability"]));
        for &time in &self.request.times {
            let f = self.fit.failure_probability(&self.request.use_stress, time, level, true);
            let mut row = vec![num(time)];
            row.extend(Self::interval_cells(&f));
            t.push(row);
        }
        section(
            SectionKind::LifeStressUseProbabilities,
            format!(
                "Life at Use Conditions ({}): Failure Probabilities (reliability = 1 − failure probability)",
                self.stress_text(&self.request.use_stress)
            ),
            SectionContent::Table(t),
        )
    }

    /// Horizontal position of a first-stress value on the relationship's
    /// scale, increasing with the stress.
    fn stress_x(&self, s: f64) -> f64 {
        let v = self.request.stresses[0].variable();
        match v.transform {
            StressTransform::Arrhenius | StressTransform::Eyring => -v.transformed(s),
            _ => v.transformed(s),
        }
    }

    fn life_stress_plot(&self) -> ReportSection {
        let v0 = self.request.stresses[0].variable();
        let use0 = self.request.use_stress[0];
        let other: Vec<f64> = self.request.use_stress.iter().skip(1).copied().collect();
        let tested: Vec<f64> = self.fit.levels.iter().map(|l| l.stresses[0]).collect();
        let s_lo = tested.iter().copied().fold(use0, f64::min);
        let s_hi = tested.iter().copied().fold(use0, f64::max);
        let (x_lo, x_hi) = (self.stress_x(s_lo), self.stress_x(s_hi));
        // Points: failures, survivors and censored intervals.
        let mut points = Vec::new();
        let mut membership = Vec::new();
        let (mut t_lo, mut t_hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for (u, s) in self.rows.iter().zip(&self.fit.data.stresses) {
            let o = u.observation;
            let (t, g) = match o.kind() {
                CensorKind::Exact => (o.lower, 0),
                CensorKind::Right => (o.lower, 1),
                CensorKind::Left => (o.upper, 2),
                CensorKind::Interval => ((o.lower.max(f64::MIN_POSITIVE) * o.upper).sqrt(), 2),
            };
            if t > 0.0 && t.is_finite() {
                points.push(RowPoint { row: u.row, x: self.stress_x(s[0]), y: t.ln() });
                membership.push(g);
                (t_lo, t_hi) = (t_lo.min(t), t_hi.max(t));
            }
        }
        let mut probabilities: Vec<f64> = self.request.probabilities.iter().copied().filter(|p| *p < 1.0).collect();
        if probabilities.is_empty() {
            probabilities = vec![0.1, 0.5, 0.9];
        }
        probabilities.truncate(4);
        let mut lines: Vec<Polyline> = vec![Vec::new(), Vec::new(), Vec::new()];
        let mut labels =
            vec!["Failure".to_owned(), "Still running (censored)".to_owned(), "Left- or interval-censored".to_owned()];
        let point_at = |x: f64| -> Vec<f64> {
            let raw = match v0.transform {
                StressTransform::Arrhenius | StressTransform::Eyring => v0.untransformed(-x),
                _ => v0.untransformed(x),
            };
            std::iter::once(raw).chain(other.iter().copied()).collect()
        };
        for &p in &probabilities {
            let line: Polyline = (0..=CURVE_POINTS)
                .filter_map(|i| {
                    let x = x_lo + (x_hi - x_lo) * i as f64 / CURVE_POINTS as f64;
                    let q = self.fit.fit.distribution_at(&self.fit.point(&point_at(x))).quantile(p);
                    (q > 0.0 && q.is_finite()).then(|| [x, q.ln()])
                })
                .collect();
            for pt in &line {
                (t_lo, t_hi) = (t_lo.min(pt[1].exp()), t_hi.max(pt[1].exp()));
            }
            lines.push(line);
            labels.push(format!("{}% line", format_number(p * 100.0, 3)));
        }
        let median_use = self.fit.quantile(&self.request.use_stress, 0.5, 0.95, false).estimate;
        let annotations = (median_use > 0.0 && median_use.is_finite()).then(|| Annotations {
            groups: vec!["Use conditions".to_owned()],
            items: vec![(self.stress_x(use0), median_use.ln(), "Use: median life".to_owned(), 0)],
            labels: Vec::new(),
        });
        let x_ticks = match v0.transform {
            StressTransform::Exponential => None,
            _ => {
                let mut values: Vec<f64> = tested.clone();
                values.push(use0);
                values.sort_by(f64::total_cmp);
                values.dedup();
                Some(values.into_iter().map(|s| (self.stress_x(s), format_number(s, 5))).collect())
            }
        };
        let y_ticks = if t_lo.is_finite() {
            Some(log_ticks(t_lo, t_hi).into_iter().map(|v| (v.ln(), format_number(v, 4))).collect())
        } else {
            None
        };
        let name0 = &self.stress_names[0];
        let x_label = match v0.transform {
            StressTransform::Arrhenius | StressTransform::Eyring => {
                let unit = if v0.unit == TemperatureUnit::Kelvin { "K" } else { "°C" };
                format!("{name0} ({unit}; Arrhenius scale 1/kT)")
            }
            StressTransform::InversePower => format!("{name0} (log scale)"),
            StressTransform::Exponential => name0.clone(),
        };
        let held = if other.is_empty() {
            String::new()
        } else {
            format!("; {} held at its use value {}", self.stress_names[1], format_number(other[0], 5))
        };
        let spec = PlotSpec {
            x_label,
            y_label: format!("{} (log scale)", self.time_name),
            points,
            groups: Some(PointGroups { title: String::new(), labels, membership, lines }),
            annotations,
            x_ticks,
            y_ticks,
            ..PlotSpec::default()
        };
        section(
            SectionKind::LifeStressPlot,
            format!("Life–Stress Plot (percentile lines of the fitted model from use to the highest test level{held})"),
            SectionContent::Plot(Box::new(spec)),
        )
    }

    fn probability_plot(&self) -> ReportSection {
        let family = self.request.family;
        let std_q = |p: f64| family.standard_quantile(p);
        let mut points = Vec::new();
        let mut membership = Vec::new();
        let mut lines = Vec::new();
        let mut labels = Vec::new();
        let (mut y_lo, mut y_hi) = (f64::INFINITY, f64::NEG_INFINITY);
        let (mut t_lo, mut t_hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for (k, l) in self.fit.levels.iter().enumerate() {
            labels.push(self.stress_text(&l.stresses));
            let Ok(np) = turnbull(&l.data) else {
                lines.push(Vec::new());
                continue;
            };
            for &m in &l.members {
                let o = self.rows[m].observation;
                if !o.upper.is_finite() || o.upper <= 0.0 {
                    continue;
                }
                let Some(p) = plotting_position(&np, o.upper).filter(|p| *p > 0.0 && *p < 1.0) else { continue };
                let y = std_q(p);
                points.push(RowPoint { row: self.rows[m].row, x: o.upper.ln(), y });
                membership.push(k);
                (y_lo, y_hi) = (y_lo.min(y), y_hi.max(y));
                (t_lo, t_hi) = (t_lo.min(o.upper), t_hi.max(o.upper));
            }
            lines.push(Vec::new());
        }
        if !t_lo.is_finite() {
            (t_lo, t_hi) = (1.0, 10.0);
            (y_lo, y_hi) = (-2.0, 2.0);
        }
        let (a, b) = ((t_lo / 2.0).ln(), (t_hi * 2.0).ln());
        for (k, l) in self.fit.levels.iter().enumerate() {
            let d = self.fit.fit.distribution_at(&self.fit.point(&l.stresses));
            lines[k] = (0..=CURVE_POINTS)
                .map(|i| a + (b - a) * i as f64 / CURVE_POINTS as f64)
                .map(|x| [x, d.standardize(x.exp())])
                .filter(|p| p[1].is_finite() && p[1] >= y_lo - 1.5 && p[1] <= y_hi + 1.5)
                .collect();
        }
        let y_ticks = probability_ticks(&std_q, y_lo, y_hi);
        let x_ticks: Vec<(f64, String)> =
            log_ticks(t_lo, t_hi).into_iter().map(|v| (v.ln(), format_number(v, 4))).collect();
        let scale_name = match family {
            RegressionFamily::Exponential => "Weibull",
            f => f.label(),
        };
        let spec = PlotSpec {
            x_label: format!("{} (log scale)", self.time_name),
            y_label: "Fraction failing".to_owned(),
            points,
            groups: Some(PointGroups { title: String::new(), labels, membership, lines }),
            x_ticks: Some(x_ticks),
            y_ticks: Some(y_ticks),
            ..PlotSpec::default()
        };
        let parallel = if self.request.stress_dependent_scale || family.fixed_scale() { "" } else { " (parallel)" };
        section(
            SectionKind::LifeStressProbabilityPlot,
            format!(
                "{scale_name} Probability Plot by Test Level (points: Turnbull estimate per level; lines{parallel}: \
                 the life–stress model at each level)"
            ),
            SectionContent::Plot(Box::new(spec)),
        )
    }

    fn checks(&self) -> ReportSection {
        let mut t =
            TextTable::new(["Test", "Simpler model ℓ", "Larger model ℓ", "Likelihood-ratio χ²", "DF", "p-value"]);
        let mut push = |test: &stat_reliability::life_stress::LikelihoodRatioTest| {
            t.push(vec![
                test.name.to_owned(),
                num(test.reduced),
                num(test.full),
                format_number(test.statistic, 4),
                test.df.to_string(),
                format_p_value(test.p_value),
            ]);
        };
        if let Some(s) = &self.fit.scale_test {
            push(s);
        }
        let mut note = String::new();
        if let Some(c) = &self.fit.level_comparison {
            for test in &c.tests {
                push(test);
            }
            if c.used.len() < self.fit.levels.len() {
                let left: Vec<String> = (0..self.fit.levels.len())
                    .filter(|i| !c.used.contains(i))
                    .map(|i| self.stress_text(&self.fit.levels[i].stresses))
                    .collect();
                note = format!("; per-level comparisons leave out {} (no individual fit)", left.join(", "));
            }
        } else {
            note = "; fewer than two levels can be fitted individually".to_owned();
        }
        section(
            SectionKind::LifeStressModelChecks,
            format!("Model Checks (a small p-value favours the larger model{note})"),
            SectionContent::Table(t),
        )
    }

    fn level_fits(&self) -> ReportSection {
        let family = self.request.family;
        let mut heads = self.stress_names.clone();
        let spread = match family {
            RegressionFamily::Exponential => "Mean life",
            RegressionFamily::Weibull => "Shape β",
            _ => "Scale σ (log life)",
        };
        let loc = match family {
            RegressionFamily::Weibull => "Scale α",
            RegressionFamily::Exponential => "",
            _ => "Location μ (log life)",
        };
        if !loc.is_empty() {
            heads.push(loc.to_owned());
        }
        heads.extend([spread.to_owned(), "Log-likelihood".to_owned()]);
        let mut t = TextTable::new(heads);
        for l in &self.fit.levels {
            let mut row: Vec<String> = l.stresses.iter().map(|v| format_number(*v, 6)).collect();
            match &l.individual {
                Ok(f) => {
                    let d = &f.distribution;
                    match family {
                        RegressionFamily::Exponential => row.push(num(d.scale)),
                        RegressionFamily::Weibull => {
                            row.push(num(d.location.exp()));
                            row.push(num(1.0 / d.scale));
                        }
                        _ => {
                            row.push(num(d.location));
                            row.push(num(d.scale));
                        }
                    }
                    row.push(num(f.log_likelihood()));
                }
                Err(e) => {
                    row.push(e.clone());
                    if !loc.is_empty() {
                        row.push(String::new());
                    }
                    row.push(String::new());
                }
            }
            t.push(row);
        }
        section(
            SectionKind::LifeStressLevelFits,
            format!("Individual {} Fits by Level (maximum likelihood, each level alone)", family.label()),
            SectionContent::Table(t),
        )
    }
}

// ---------------------------------------------------------------------------
// Project-file (JSON) form
// ---------------------------------------------------------------------------

fn layout_json(layout: &LifeDataLayout, col: &dyn Fn(usize) -> JsonValue, m: &mut Vec<(String, JsonValue)>) {
    match *layout {
        LifeDataLayout::Times { time, censor, coding } => {
            m.push(("layout".into(), s("times")));
            m.push(("time".into(), col(time)));
            if let Some(c) = censor {
                m.push(("censor".into(), col(c)));
            }
            let coding = match coding {
                StatusCoding::NonzeroIsCensored => "nonzero-censored",
                StatusCoding::NonzeroIsEvent => "nonzero-event",
            };
            m.push(("censor_coding".into(), s(coding)));
        }
        LifeDataLayout::Intervals { start, end } => {
            m.push(("layout".into(), s("intervals")));
            m.push(("start".into(), col(start)));
            m.push(("end".into(), col(end)));
        }
    }
}

fn layout_from_json(v: &JsonValue, col: &dyn Fn(&str) -> Result<usize>) -> Result<LifeDataLayout> {
    let text = |key: &str| v.get(key).and_then(JsonValue::as_str);
    Ok(match text("layout") {
        Some("intervals") => LifeDataLayout::Intervals { start: col("start")?, end: col("end")? },
        None | Some("times") => {
            let coding = match text("censor_coding") {
                None | Some("nonzero-censored") => StatusCoding::NonzeroIsCensored,
                Some("nonzero-event") => StatusCoding::NonzeroIsEvent,
                Some(other) => return Err(json_invalid(format!("unknown censor coding {other:?}"))),
            };
            let censor = match v.get("censor") {
                None | Some(JsonValue::Null) => None,
                Some(_) => Some(col("censor")?),
            };
            LifeDataLayout::Times { time: col("time")?, censor, coding }
        }
        Some(other) => return Err(json_invalid(format!("unknown layout {other:?}"))),
    })
}

pub(crate) fn stress_json(st: &StressColumn, col: &dyn Fn(usize) -> JsonValue) -> JsonValue {
    let mut m = vec![("column".to_owned(), col(st.column)), ("relationship".to_owned(), s(st.transform.key()))];
    if st.transform.is_temperature() {
        let unit = match st.unit {
            TemperatureUnit::Celsius => "celsius",
            TemperatureUnit::Kelvin => "kelvin",
        };
        m.push(("unit".to_owned(), s(unit)));
    }
    JsonValue::Object(m)
}

pub(crate) fn stress_from_json(v: &JsonValue, table: &DataTable) -> Result<StressColumn> {
    let name = member(v, "column")?.as_str().ok_or_else(|| json_invalid("stress: expected a column name"))?;
    let column = table.find(name).ok_or_else(|| json_invalid(format!("report refers to a missing column {name:?}")))?;
    let transform = match v.get("relationship").and_then(JsonValue::as_str) {
        None => StressTransform::Arrhenius,
        Some(k) => StressTransform::from_key(k).ok_or_else(|| json_invalid(format!("unknown relationship {k:?}")))?,
    };
    let unit = match v.get("unit").and_then(JsonValue::as_str) {
        None | Some("celsius") => TemperatureUnit::Celsius,
        Some("kelvin") => TemperatureUnit::Kelvin,
        Some(other) => return Err(json_invalid(format!("unknown temperature unit {other:?}"))),
    };
    Ok(StressColumn { column, transform, unit })
}

pub(crate) fn numbers(values: &[f64]) -> JsonValue {
    JsonValue::Array(values.iter().map(|&v| JsonValue::number(v)).collect())
}

pub(crate) fn numbers_from(v: &JsonValue, key: &str) -> Option<Vec<f64>> {
    v.get(key).and_then(JsonValue::as_array).map(|a| a.iter().filter_map(JsonValue::as_f64).collect())
}

/// The JSON form of a life–stress report.
pub(crate) fn to_json(r: &LifeStressRequest, table: &DataTable) -> JsonValue {
    let col = |i: usize| s(table.column(i).map_or("", |c| c.name.as_str()));
    let mut m: Vec<(String, JsonValue)> = vec![("analysis".into(), s("life-stress"))];
    layout_json(&r.layout, &col, &mut m);
    if let Some(c) = r.frequency {
        m.push(("frequency".into(), col(c)));
    }
    m.push(("stresses".into(), JsonValue::Array(r.stresses.iter().map(|st| stress_json(st, &col)).collect())));
    m.push(("interaction".into(), JsonValue::Bool(r.interaction)));
    m.push(("distribution".into(), s(r.family.key())));
    m.push(("stress_dependent_scale".into(), JsonValue::Bool(r.stress_dependent_scale)));
    m.push(("use".into(), numbers(&r.use_stress)));
    m.push(("confidence".into(), JsonValue::number(r.confidence)));
    m.push(("probabilities".into(), numbers(&r.probabilities)));
    m.push(("times".into(), numbers(&r.times)));
    JsonValue::Object(m)
}

/// Read a life–stress report from its JSON form.
pub(crate) fn from_json(v: &JsonValue, table: &DataTable) -> Result<SavedReport> {
    let col = |key: &str| -> Result<usize> {
        let name =
            member(v, key)?.as_str().ok_or_else(|| json_invalid(format!("report {key}: expected a column name")))?;
        table.find(name).ok_or_else(|| json_invalid(format!("report refers to a missing column {name:?}")))
    };
    let layout = layout_from_json(v, &col)?;
    let frequency = match v.get("frequency") {
        None | Some(JsonValue::Null) => None,
        Some(_) => Some(col("frequency")?),
    };
    let stresses = member(v, "stresses")?
        .as_array()
        .ok_or_else(|| json_invalid("life–stress: stresses must be a list"))?
        .iter()
        .map(|x| stress_from_json(x, table))
        .collect::<Result<Vec<_>>>()?;
    let family = match v.get("distribution").and_then(JsonValue::as_str) {
        None => RegressionFamily::Weibull,
        Some(k) => RegressionFamily::from_key(k)
            .filter(|f| f.is_log())
            .ok_or_else(|| json_invalid(format!("life–stress: unknown distribution {k:?}")))?,
    };
    let flag = |key: &str| v.get(key).and_then(JsonValue::as_bool).unwrap_or(false);
    Ok(SavedReport::LifeStress {
        request: LifeStressRequest {
            layout,
            frequency,
            stresses,
            interaction: flag("interaction"),
            family,
            stress_dependent_scale: flag("stress_dependent_scale"),
            use_stress: numbers_from(v, "use").unwrap_or_default(),
            confidence: v
                .get("confidence")
                .and_then(JsonValue::as_f64)
                .unwrap_or(LifeStressRequest::DEFAULT_CONFIDENCE),
            probabilities: numbers_from(v, "probabilities")
                .unwrap_or_else(|| LifeStressRequest::DEFAULT_PROBABILITIES.to_vec()),
            times: numbers_from(v, "times").unwrap_or_default(),
        },
    })
}

#[cfg(test)]
mod tests;
