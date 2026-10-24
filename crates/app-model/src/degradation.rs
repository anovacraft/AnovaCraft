//! Degradation analysis bound to a dataset
//! ([`stat_reliability::degradation`]): life information from measurements
//! of a characteristic that drifts towards a failure threshold.
//!
//! **Layouts:**
//! * *Repeated measures* — unit, time and response columns, several
//!   measurements per unit. Each unit's path (linear on transformed scales,
//!   asymptotic or power) is fitted by least squares and solved for its
//!   *pseudo failure time*, and a life distribution (or, with a stress column
//!   constant within each unit, a life–stress model) is fitted to those times
//!   with censoring for units whose path never reaches the threshold; or the
//!   linear path is fitted as a random-effects model and the implied
//!   distribution of the threshold-crossing time is computed, with parametric
//!   bootstrap intervals.
//! * *Destructive* — time and response columns, one measurement per unit
//!   (optionally a stress column): a location–scale regression of the
//!   transformed response on transformed time gives the time-to-threshold
//!   distribution.
//!
//! Excluded rows and rows with missing values are left out.

use crate::format::{format_number, format_p_value};
use crate::life_stress::{StressColumn, log_ticks, numbers, numbers_from, plotting_position, probability_ticks};
use crate::project::{SavedReport, invalid as json_invalid, member, s};
use crate::report::{PlotSpec, PointGroups, Polyline, ReportSection, RowPoint, SectionContent, SectionKind, TextTable};
use stat_core::{Result, StatError};
use stat_data::{DataTable, JsonValue, RowStateManager};
use stat_linear::natural_order;
use stat_reliability::degradation::{
    BootstrapBands, Crossing, DestructiveData, DestructiveFit, DestructiveModel, FailureDirection, PathModel,
    RandomPathFit, ScaleTransform, UnitPath, UnitSeries, fit_destructive, fit_random_paths, fit_unit_path,
};
use stat_reliability::life_regression::RegressionFamily;
use stat_reliability::life_stress::{LifeStressData, LifeStressFit, LifeStressModel, fit_life_stress};
use stat_reliability::{CensoredFit, IntervalEstimate, LifeData, fit_censored, turnbull};

const DIGITS: usize = 5;
const CURVE_POINTS: usize = 120;
/// Seed of the parametric bootstrap (fixed, so a report reproduces exactly).
pub const BOOTSTRAP_SEED: u64 = 20_260_101;

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

fn section(kind: SectionKind, title: impl Into<String>, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.into(), content }
}

fn num(v: f64) -> String {
    format_number(v, DIGITS)
}

/// Where the measurements come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DegradationLayout {
    /// Several measurements per unit.
    RepeatedMeasures {
        /// Unit identifier.
        unit: usize,
        /// Measurement time.
        time: usize,
        /// Measured response.
        response: usize,
    },
    /// One measurement per unit (the measurement destroys it).
    Destructive {
        /// Measurement time.
        time: usize,
        /// Measured response.
        response: usize,
    },
}

/// How repeated measures are turned into life information.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DegradationMethod {
    /// Pseudo failure times from each unit's path, then a life distribution.
    #[default]
    PseudoFailureTimes,
    /// Random intercepts and slopes; the implied life distribution.
    RandomEffects,
}

impl DegradationMethod {
    /// Stable key.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::PseudoFailureTimes => "pseudo-failure-times",
            Self::RandomEffects => "random-effects",
        }
    }
}

/// Columns and settings of a degradation analysis.
#[derive(Debug, Clone, PartialEq)]
pub struct DegradationRequest {
    /// Data layout.
    pub layout: DegradationLayout,
    /// Path form (destructive degradation uses the linear form's transforms).
    pub path: PathModel,
    /// Failure threshold on the response's scale.
    pub threshold: f64,
    /// Which way the response moves towards failure.
    pub direction: FailureDirection,
    /// Repeated measures: pseudo failure times or random effects.
    pub method: DegradationMethod,
    /// Life distribution of the pseudo failure times.
    pub life_family: RegressionFamily,
    /// Destructive: distribution of the transformed response.
    pub response_family: RegressionFamily,
    /// Optional stress column (accelerated degradation).
    pub stress: Option<StressColumn>,
    /// Use condition of the stress.
    pub use_stress: Option<f64>,
    /// Confidence level.
    pub confidence: f64,
    /// Fractions failing whose times are estimated.
    pub probabilities: Vec<f64>,
    /// Times whose failure probabilities are estimated.
    pub times: Vec<f64>,
    /// Random effects: bootstrap replicates (0 for none).
    pub bootstrap: usize,
}

impl DegradationRequest {
    /// Default confidence level.
    pub const DEFAULT_CONFIDENCE: f64 = 0.95;
    /// Default percentiles.
    pub const DEFAULT_PROBABILITIES: [f64; 3] = [0.1, 0.5, 0.9];
    /// Default bootstrap replicates.
    pub const DEFAULT_BOOTSTRAP: usize = 200;

    /// A linear-path, pseudo-failure-time analysis with a Weibull life.
    #[must_use]
    pub fn new(layout: DegradationLayout, threshold: f64, direction: FailureDirection) -> Self {
        Self {
            layout,
            path: PathModel::Linear { response: ScaleTransform::Identity, time: ScaleTransform::Identity },
            threshold,
            direction,
            method: DegradationMethod::PseudoFailureTimes,
            life_family: RegressionFamily::Weibull,
            response_family: RegressionFamily::Normal,
            stress: None,
            use_stress: None,
            confidence: Self::DEFAULT_CONFIDENCE,
            probabilities: Self::DEFAULT_PROBABILITIES.to_vec(),
            times: Vec::new(),
            bootstrap: Self::DEFAULT_BOOTSTRAP,
        }
    }

    /// Every column the request reads.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        let mut out = match self.layout {
            DegradationLayout::RepeatedMeasures { unit, time, response } => vec![unit, time, response],
            DegradationLayout::Destructive { time, response } => vec![time, response],
        };
        out.extend(self.stress.map(|s| s.column));
        out
    }

    /// Pass every column index through `f`.
    pub fn map_columns(&mut self, f: &impl Fn(usize) -> usize) {
        match &mut self.layout {
            DegradationLayout::RepeatedMeasures { unit, time, response } => {
                *unit = f(*unit);
                *time = f(*time);
                *response = f(*response);
            }
            DegradationLayout::Destructive { time, response } => {
                *time = f(*time);
                *response = f(*response);
            }
        }
        if let Some(s) = &mut self.stress {
            s.column = f(s.column);
        }
    }

    fn transforms(&self) -> (ScaleTransform, ScaleTransform) {
        match self.path {
            PathModel::Linear { response, time } => (response, time),
            _ => (ScaleTransform::Identity, ScaleTransform::Identity),
        }
    }
}

/// One unit of a repeated-measures analysis.
#[derive(Debug, Clone)]
pub struct DegradationUnit {
    /// Unit label.
    pub label: String,
    /// Rows of its measurements.
    pub rows: Vec<usize>,
    /// Times.
    pub times: Vec<f64>,
    /// Values.
    pub values: Vec<f64>,
    /// Stress (when a stress column is used).
    pub stress: Option<f64>,
    /// Its path, or why it could not be fitted.
    pub path: std::result::Result<UnitPath, String>,
}

/// The life information derived from the paths.
#[derive(Debug, Clone)]
enum LifeResult {
    Distribution { fit: CensoredFit, data: LifeData },
    Stress { fit: Box<LifeStressFit> },
    Random { fit: Box<RandomPathFit>, bands: Option<BootstrapBands>, horizon: f64 },
    Destructive { fit: Box<DestructiveFit>, rows: Vec<usize>, data: DestructiveData },
}

/// A computed degradation analysis.
#[derive(Debug, Clone)]
pub struct DegradationAnalysis {
    request: DegradationRequest,
    response_name: String,
    time_name: String,
    stress_name: Option<String>,
    units: Vec<DegradationUnit>,
    life: LifeResult,
    n_skipped: usize,
}

impl DegradationAnalysis {
    /// Fit the paths and the life model to the included rows.
    ///
    /// # Errors
    /// Missing or non-numeric columns, a column used twice, settings that do
    /// not fit together (a nonlinear path with random effects or destructive
    /// data, a stress with random effects, a stress without a use condition),
    /// a confidence level outside (0.5, 1), too few usable units or rows, or
    /// errors of the fits.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &DegradationRequest) -> Result<Self> {
        let cols = request.columns();
        if (1..cols.len()).any(|i| cols[..i].contains(&cols[i])) {
            return Err(invalid("choose a different column for each role"));
        }
        let unit_col = match request.layout {
            DegradationLayout::RepeatedMeasures { unit, .. } => Some(unit),
            DegradationLayout::Destructive { .. } => None,
        };
        for &c in &cols {
            let col = table.column(c).ok_or_else(|| invalid(format!("no column {c}")))?;
            if Some(c) != unit_col && col.as_numeric().is_none() {
                return Err(invalid(format!("{} must be numeric", col.name)));
            }
        }
        if !(request.confidence > 0.5 && request.confidence < 1.0) {
            return Err(invalid("the confidence level must be between 0.5 and 1"));
        }
        if !request.threshold.is_finite() {
            return Err(invalid("give the failure threshold"));
        }
        if let Some(st) = request.stress {
            match request.use_stress {
                Some(u) if st.variable().valid(u) => {}
                _ => return Err(invalid("give a use condition valid for the stress relationship")),
            }
        }
        let linear = matches!(request.path, PathModel::Linear { .. });
        let value = |c: usize, r: usize| crate::life_stress::value(table, c, r);
        let name = |i: usize| table.column(i).map_or_else(String::new, |c| c.name.clone());
        let (time, response) = match request.layout {
            DegradationLayout::RepeatedMeasures { time, response, .. }
            | DegradationLayout::Destructive { time, response } => (time, response),
        };
        let level = request.confidence;
        let mut used = 0;
        let mut units = Vec::new();
        let life = match request.layout {
            DegradationLayout::RepeatedMeasures { unit, .. } => {
                let unit_column = table.column(unit).ok_or_else(|| invalid("no unit column"))?;
                let mut labels: Vec<String> = Vec::new();
                let mut members: Vec<Vec<usize>> = Vec::new();
                for r in 0..table.n_rows() {
                    if states.is_excluded(r) {
                        continue;
                    }
                    let label = unit_column.level_text(r);
                    let s_ok = request.stress.is_none_or(|st| value(st.column, r).is_finite());
                    if label.is_empty() || !value(time, r).is_finite() || !value(response, r).is_finite() || !s_ok {
                        continue;
                    }
                    let g = labels.iter().position(|l| *l == label).unwrap_or_else(|| {
                        labels.push(label);
                        members.push(Vec::new());
                        labels.len() - 1
                    });
                    members[g].push(r);
                    used += 1;
                }
                let mut order: Vec<usize> = (0..labels.len()).collect();
                order.sort_by(|&a, &b| natural_order(&labels[a], &labels[b]));
                for g in order {
                    let mut rows = std::mem::take(&mut members[g]);
                    rows.sort_by(|&a, &b| value(time, a).total_cmp(&value(time, b)));
                    let times: Vec<f64> = rows.iter().map(|&r| value(time, r)).collect();
                    let values: Vec<f64> = rows.iter().map(|&r| value(response, r)).collect();
                    let stress = match request.stress {
                        Some(st) => {
                            let first = value(st.column, rows[0]);
                            if rows.iter().any(|&r| value(st.column, r) != first) {
                                return Err(invalid(format!(
                                    "unit {}: the stress must be the same for every measurement of a unit",
                                    labels[g]
                                )));
                            }
                            Some(first)
                        }
                        None => None,
                    };
                    let path = fit_unit_path(&times, &values, request.path, request.threshold, request.direction)
                        .map_err(|e| e.to_string());
                    units.push(DegradationUnit { label: labels[g].clone(), rows, times, values, stress, path });
                }
                if units.len() < 2 {
                    return Err(invalid("a degradation analysis needs at least two units"));
                }
                match request.method {
                    DegradationMethod::PseudoFailureTimes => {
                        let fitted: Vec<&DegradationUnit> = units.iter().filter(|u| u.path.is_ok()).collect();
                        let observations: Vec<_> = fitted
                            .iter()
                            .filter_map(|u| u.path.as_ref().ok().map(|p| p.crossing.observation()))
                            .collect();
                        if observations.is_empty() {
                            return Err(invalid("no unit's path could be fitted (each needs three measurements)"));
                        }
                        if !request.life_family.is_log() {
                            return Err(invalid("choose a life distribution for the pseudo failure times"));
                        }
                        match (request.stress, request.use_stress) {
                            (Some(st), Some(_)) => {
                                let data = LifeStressData {
                                    observations,
                                    stresses: fitted.iter().map(|u| vec![u.stress.unwrap_or(f64::NAN)]).collect(),
                                };
                                let model = LifeStressModel {
                                    family: request.life_family,
                                    stresses: vec![st.variable()],
                                    interaction: false,
                                    stress_dependent_scale: false,
                                };
                                LifeResult::Stress { fit: Box::new(fit_life_stress(&data, &model)?) }
                            }
                            _ => {
                                let data = LifeData::new(observations)?;
                                let fit = fit_censored(&data, request.life_family.single_sample_family())?;
                                LifeResult::Distribution { fit, data }
                            }
                        }
                    }
                    DegradationMethod::RandomEffects => {
                        if !linear {
                            return Err(invalid("the random-effects path model needs the linear path"));
                        }
                        if request.stress.is_some() {
                            return Err(invalid("the random-effects path model does not take a stress"));
                        }
                        let (g, h) = request.transforms();
                        let series: Vec<UnitSeries> = units
                            .iter()
                            .map(|u| UnitSeries { times: u.times.clone(), values: u.values.clone() })
                            .collect();
                        let fit = fit_random_paths(&series, g, h, request.threshold, request.direction, true)?;
                        let t_max = units.iter().flat_map(|u| u.times.iter().copied()).fold(0.0, f64::max);
                        let horizon = 100.0 * t_max.max(1e-9);
                        let bands = (request.bootstrap > 0).then(|| {
                            let mut times = curve_times(&fit, &request.times, t_max, horizon);
                            times.extend(&request.times);
                            fit.bootstrap(
                                &times,
                                &request.probabilities,
                                horizon,
                                request.bootstrap,
                                BOOTSTRAP_SEED,
                                level,
                            )
                        });
                        LifeResult::Random { fit: Box::new(fit), bands, horizon }
                    }
                }
            }
            DegradationLayout::Destructive { .. } => {
                if !linear {
                    return Err(invalid("destructive degradation uses the linear path (with transforms)"));
                }
                if request.response_family.is_log() {
                    return Err(invalid("choose a response distribution (normal, logistic or extreme value)"));
                }
                let mut rows = Vec::new();
                let mut data = DestructiveData { times: Vec::new(), values: Vec::new(), stresses: Vec::new() };
                for r in 0..table.n_rows() {
                    if states.is_excluded(r) {
                        continue;
                    }
                    let (t, y) = (value(time, r), value(response, r));
                    let st = request.stress.map(|st| value(st.column, r));
                    if !t.is_finite() || !y.is_finite() || st.is_some_and(|v| !v.is_finite()) {
                        continue;
                    }
                    rows.push(r);
                    data.times.push(t);
                    data.values.push(y);
                    data.stresses.extend(st);
                }
                used = rows.len();
                if rows.is_empty() {
                    return Err(invalid("no rows with usable times and responses"));
                }
                let (g, h) = request.transforms();
                let model = DestructiveModel {
                    family: request.response_family,
                    response: g,
                    time: h,
                    stress: request.stress.map(|s| s.variable()),
                    use_stress: request.use_stress,
                    threshold: request.threshold,
                    direction: request.direction,
                };
                let fit = fit_destructive(&data, &model)?;
                LifeResult::Destructive { fit: Box::new(fit), rows, data }
            }
        };
        Ok(Self {
            request: request.clone(),
            response_name: name(response),
            time_name: name(time),
            stress_name: request.stress.map(|s| name(s.column)),
            units,
            life,
            n_skipped: table.n_rows() - used,
        })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &DegradationRequest {
        &self.request
    }

    /// The units (repeated measures).
    #[must_use]
    pub fn units(&self) -> &[DegradationUnit] {
        &self.units
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        let kind = match self.request.layout {
            DegradationLayout::RepeatedMeasures { .. } => "Degradation",
            DegradationLayout::Destructive { .. } => "Destructive Degradation",
        };
        match &self.stress_name {
            Some(s) => format!("{kind}: {} over {} by {s}", self.response_name, self.time_name),
            None => format!("{kind}: {} over {}", self.response_name, self.time_name),
        }
    }

    fn level_text(&self) -> String {
        format!("{}%", format_number(self.request.confidence * 100.0, 4))
    }

    fn threshold_text(&self) -> String {
        let dir = match self.request.direction {
            FailureDirection::Rising => "rises to",
            FailureDirection::Falling => "falls to",
        };
        format!("failure when {} {dir} {}", self.response_name, format_number(self.request.threshold, 6))
    }

    fn use_text(&self) -> String {
        match (&self.stress_name, self.request.use_stress) {
            (Some(n), Some(u)) => format!(" at use conditions ({n} {})", format_number(u, 6)),
            _ => String::new(),
        }
    }

    fn interval_headings(&self, first: &[&str], lr: bool) -> Vec<String> {
        let level = self.level_text();
        let mut cols: Vec<String> = first.iter().map(|s| (*s).to_owned()).collect();
        cols.extend(["Std error".to_owned(), format!("Wald lower {level}"), format!("Wald upper {level}")]);
        if lr {
            cols.extend([format!("Likelihood-ratio lower {level}"), format!("Likelihood-ratio upper {level}")]);
        }
        cols
    }

    fn interval_cells(e: &IntervalEstimate, lr: bool) -> Vec<String> {
        let mut out = vec![num(e.estimate), format_number(e.std_error, 4), num(e.wald.0), num(e.wald.1)];
        if lr {
            let l = e.likelihood_ratio.unwrap_or((f64::NAN, f64::NAN));
            out.extend([num(l.0), num(l.1)]);
        }
        out
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let mut out = vec![self.summary(), self.path_plot()];
        match &self.life {
            LifeResult::Distribution { fit, data } => {
                out.push(self.unit_table());
                out.push(self.distribution_parameters(fit));
                out.push(self.distribution_probability_plot(fit, data));
                out.extend(self.distribution_estimates(fit));
            }
            LifeResult::Stress { fit } => {
                out.push(self.unit_table());
                out.extend(self.stress_sections(fit));
            }
            LifeResult::Random { fit, bands, horizon } => {
                out.push(self.unit_table());
                out.push(self.random_effects_table(fit));
                out.push(self.random_life_curve(fit, bands.as_ref()));
                out.extend(self.random_estimates(fit, bands.as_ref(), *horizon));
            }
            LifeResult::Destructive { fit, .. } => {
                out.push(self.destructive_parameters(fit));
                out.push(self.destructive_life_curve(fit));
                out.extend(self.destructive_estimates(fit));
            }
        }
        out
    }

    fn summary(&self) -> ReportSection {
        let skipped = if self.n_skipped > 0 {
            format!("; {} rows excluded or incomplete", self.n_skipped)
        } else {
            String::new()
        };
        let mut t = TextTable::new(["Quantity", "Value"]);
        match &self.life {
            LifeResult::Destructive { data, .. } => {
                let mut times = data.times.clone();
                times.sort_by(f64::total_cmp);
                times.dedup();
                t.push(vec!["Units (one measurement each)".to_owned(), data.times.len().to_string()]);
                t.push(vec!["Measurement times".to_owned(), times.len().to_string()]);
                if let Some(n) = &self.stress_name {
                    let mut st = data.stresses.clone();
                    st.sort_by(f64::total_cmp);
                    st.dedup();
                    t.push(vec![format!("{n} levels"), st.len().to_string()]);
                }
            }
            _ => {
                let measurements: usize = self.units.iter().map(|u| u.rows.len()).sum();
                let count = |f: &dyn Fn(&Crossing) -> bool| {
                    self.units.iter().filter(|u| u.path.as_ref().is_ok_and(|p| f(&p.crossing))).count()
                };
                let t_max = self.units.iter().flat_map(|u| u.times.iter().copied()).fold(0.0, f64::max);
                t.push(vec!["Units".to_owned(), self.units.len().to_string()]);
                t.push(vec!["Measurements".to_owned(), measurements.to_string()]);
                t.push(vec![
                    "Reached the threshold during the test".to_owned(),
                    count(&|c| matches!(c, Crossing::At(x) if *x <= t_max)).to_string(),
                ]);
                t.push(vec![
                    "Projected to reach it later".to_owned(),
                    count(&|c| matches!(c, Crossing::At(x) if *x > t_max)).to_string(),
                ]);
                t.push(vec![
                    "Never reach it (censored at the last measurement)".to_owned(),
                    count(&|c| matches!(c, Crossing::Never(_))).to_string(),
                ]);
                let failed_paths = self.units.iter().filter(|u| u.path.is_err()).count();
                if failed_paths > 0 {
                    t.push(vec!["Paths that could not be fitted".to_owned(), failed_paths.to_string()]);
                }
            }
        }
        t.push(vec!["Threshold".to_owned(), self.threshold_text()]);
        section(SectionKind::DegradationSummary, format!("Degradation Data{skipped}"), SectionContent::Table(t))
    }

    fn path_plot(&self) -> ReportSection {
        let mut points = Vec::new();
        let mut segments = Vec::new();
        let (mut t_hi, mut y_lo, mut y_hi) = (0.0f64, f64::INFINITY, f64::NEG_INFINITY);
        let mut groups = None;
        match &self.life {
            LifeResult::Destructive { fit, rows, data } => {
                let mut levels: Vec<f64> = data.stresses.clone();
                levels.sort_by(f64::total_cmp);
                levels.dedup();
                let mut membership = Vec::new();
                for (k, &r) in rows.iter().enumerate() {
                    points.push(RowPoint { row: r, x: data.times[k], y: data.values[k] });
                    membership.push(levels.iter().position(|l| Some(*l) == data.stresses.get(k).copied()).unwrap_or(0));
                    t_hi = t_hi.max(data.times[k]);
                    (y_lo, y_hi) = (y_lo.min(data.values[k]), y_hi.max(data.values[k]));
                }
                let curve = |s: Option<f64>| -> Polyline {
                    (0..=CURVE_POINTS)
                        .map(|i| t_hi * i as f64 / CURVE_POINTS as f64)
                        .map(|t| [t, fit.location_at(t, s)])
                        .filter(|p| p[1].is_finite())
                        .collect()
                };
                if self.stress_name.is_some() {
                    let lines: Vec<Polyline> = levels.iter().map(|&s| curve(Some(s))).collect();
                    groups = Some(PointGroups {
                        title: self.stress_name.clone().unwrap_or_default(),
                        labels: levels.iter().map(|v| format_number(*v, 6)).collect(),
                        membership,
                        lines,
                    });
                } else {
                    segments.push(curve(None));
                }
            }
            _ => {
                for u in &self.units {
                    for (k, &r) in u.rows.iter().enumerate() {
                        points.push(RowPoint { row: r, x: u.times[k], y: u.values[k] });
                        t_hi = t_hi.max(u.times[k]);
                        (y_lo, y_hi) = (y_lo.min(u.values[k]), y_hi.max(u.values[k]));
                    }
                }
                // Extend the fitted paths to the threshold when that is not far beyond the test.
                let reach = self
                    .units
                    .iter()
                    .filter_map(|u| match u.path.as_ref().ok()?.crossing {
                        Crossing::At(t) if t <= 2.0 * t_hi => Some(t),
                        _ => None,
                    })
                    .fold(t_hi, f64::max);
                for u in &self.units {
                    if let Ok(p) = &u.path {
                        let end = match p.crossing {
                            Crossing::At(t) if t <= reach => t.max(u.times[u.times.len() - 1]),
                            _ => reach,
                        };
                        let line: Polyline = (0..=CURVE_POINTS / 2)
                            .map(|i| end * i as f64 / (CURVE_POINTS / 2) as f64)
                            .map(|t| [t, self.request.path.value(p.parameters, t)])
                            .filter(|q| q[1].is_finite())
                            .collect();
                        segments.push(line);
                    }
                }
                t_hi = reach;
            }
        }
        let threshold = vec![[0.0, self.request.threshold], [t_hi, self.request.threshold]];
        let spec = PlotSpec {
            x_label: self.time_name.clone(),
            y_label: self.response_name.clone(),
            points,
            line: Some(threshold),
            line_name: Some("Failure threshold".to_owned()),
            point_name: Some("Measurements".to_owned()),
            groups,
            segments,
            ..PlotSpec::default()
        };
        let what = match self.request.layout {
            DegradationLayout::RepeatedMeasures { .. } => "thin lines: each unit's fitted path",
            DegradationLayout::Destructive { .. } if self.stress_name.is_some() => {
                "lines: fitted median response at each stress level"
            }
            DegradationLayout::Destructive { .. } => "thin line: fitted median response",
        };
        let _ = (y_lo, y_hi);
        section(
            SectionKind::DegradationPathPlot,
            format!("Degradation Paths ({what}; horizontal line: {})", self.threshold_text()),
            SectionContent::Plot(Box::new(spec)),
        )
    }

    fn unit_table(&self) -> ReportSection {
        let names = self.request.path.parameter_names();
        let mut heads = vec!["Unit".to_owned()];
        if let Some(s) = &self.stress_name {
            heads.push(s.clone());
        }
        heads.extend([
            "Measurements".to_owned(),
            names[0].to_owned(),
            names[1].to_owned(),
            "Residual SD".to_owned(),
            "Pseudo failure time".to_owned(),
            "Status".to_owned(),
        ]);
        let t_max = self.units.iter().flat_map(|u| u.times.iter().copied()).fold(0.0, f64::max);
        let mut t = TextTable::new(heads);
        for u in &self.units {
            let mut row = vec![u.label.clone()];
            if self.stress_name.is_some() {
                row.push(u.stress.map_or_else(String::new, |v| format_number(v, 6)));
            }
            row.push(u.rows.len().to_string());
            match &u.path {
                Ok(p) => {
                    let (time, status) = match p.crossing {
                        Crossing::At(x) if x <= t_max => (num(x), "Reached during the test (interpolated)".to_owned()),
                        Crossing::At(x) => (num(x), "Extrapolated".to_owned()),
                        Crossing::Never(x) => {
                            (format!("> {}", num(x)), "Never reaches the threshold: censored at last time".to_owned())
                        }
                        Crossing::Before(x) => {
                            (format!("< {}", num(x)), "Past the threshold at the first measurement".to_owned())
                        }
                    };
                    row.extend([
                        num(p.parameters[0]),
                        num(p.parameters[1]),
                        format_number(p.residual_sd, 4),
                        time,
                        status,
                    ]);
                }
                Err(e) => row.extend([String::new(), String::new(), String::new(), String::new(), e.clone()]),
            }
            t.push(row);
        }
        let form = match self.request.path {
            PathModel::Linear { response, time } => {
                format!("{} = a + b·{}", response.describe(&self.response_name), time.describe(&self.time_name))
            }
            PathModel::Asymptotic => format!("{} = D∞(1 − e^(−k·{}))", self.response_name, self.time_name),
            PathModel::Power => format!("{} = a·{}^b", self.response_name, self.time_name),
        };
        section(
            SectionKind::DegradationUnitPaths,
            format!("Unit Paths and Pseudo Failure Times (least squares, {form})"),
            SectionContent::Table(t),
        )
    }

    fn distribution_parameters(&self, fit: &CensoredFit) -> ReportSection {
        let level = self.request.confidence;
        let mut t = TextTable::new(self.interval_headings(&["Parameter", "Estimate"], true));
        for p in fit.parameters(level, true) {
            let mut row = vec![p.name.to_owned()];
            row.extend(Self::interval_cells(
                &IntervalEstimate {
                    name: p.name,
                    estimate: p.estimate,
                    std_error: p.std_error,
                    wald: p.wald,
                    likelihood_ratio: p.likelihood_ratio,
                },
                true,
            ));
            t.push(row);
        }
        t.push(vec![
            "Log-likelihood".to_owned(),
            num(fit.log_likelihood()),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
        ]);
        section(
            SectionKind::DegradationLifeParameters,
            format!(
                "{} Fit to the Pseudo Failure Times (maximum likelihood; never-crossing units right-censored)",
                self.request.life_family.label()
            ),
            SectionContent::Table(t),
        )
    }

    fn distribution_probability_plot(&self, fit: &CensoredFit, data: &LifeData) -> ReportSection {
        let d = &fit.distribution;
        let level = self.request.confidence;
        let mut points = Vec::new();
        let (mut y_lo, mut y_hi, mut t_lo, mut t_hi) = (f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, 0.0f64);
        if let Ok(np) = turnbull(data) {
            let fitted: Vec<&DegradationUnit> = self.units.iter().filter(|u| u.path.is_ok()).collect();
            for (u, o) in fitted.iter().zip(data.observations()) {
                if !o.upper.is_finite() || o.upper <= 0.0 {
                    continue;
                }
                let Some(p) = plotting_position(&np, o.upper).filter(|p| *p > 0.0 && *p < 1.0) else { continue };
                let y = d.standard_quantile(p);
                points.push(RowPoint { row: u.rows[u.rows.len() - 1], x: o.upper.ln(), y });
                (y_lo, y_hi, t_lo, t_hi) = (y_lo.min(y), y_hi.max(y), t_lo.min(o.upper), t_hi.max(o.upper));
            }
        }
        if !t_lo.is_finite() {
            (t_lo, t_hi, y_lo, y_hi) = (d.quantile(0.1), d.quantile(0.9), -2.0, 1.0);
        }
        let grid: Vec<f64> = (0..=CURVE_POINTS)
            .map(|i| ((t_lo / 1.5).ln() + ((t_hi * 1.5) / (t_lo / 1.5)).ln() * i as f64 / CURVE_POINTS as f64).exp())
            .collect();
        let line: Polyline = grid.iter().map(|&t| [t.ln(), d.standardize(t)]).filter(|p| p[1].is_finite()).collect();
        let (mut lower, mut upper) = (Vec::new(), Vec::new());
        for &t in &grid {
            let (lo, hi) = fit.probability_band(t, level);
            let (a, b) = (d.standard_quantile(lo), d.standard_quantile(hi));
            if a.is_finite() && b.is_finite() {
                lower.push([t.ln(), a]);
                upper.push([t.ln(), b]);
            }
        }
        let spec = PlotSpec {
            x_label: format!("Pseudo failure time, {} (log scale)", self.time_name),
            y_label: "Fraction failing".to_owned(),
            points,
            line: Some(line),
            line_name: Some(format!("{} fit", self.request.life_family.label())),
            band: Some((lower, upper)),
            band_name: Some(format!("Pointwise {} limits", self.level_text())),
            point_name: Some("Nonparametric estimate".to_owned()),
            x_ticks: Some(log_ticks(t_lo, t_hi).into_iter().map(|v| (v.ln(), format_number(v, 4))).collect()),
            y_ticks: Some(probability_ticks(&|p| d.standard_quantile(p), y_lo, y_hi)),
            ..PlotSpec::default()
        };
        section(
            SectionKind::DegradationProbabilityPlot,
            format!(
                "{} Probability Plot of the Pseudo Failure Times (dashed: pointwise {} limits)",
                self.request.life_family.label(),
                self.level_text()
            ),
            SectionContent::Plot(Box::new(spec)),
        )
    }

    fn distribution_estimates(&self, fit: &CensoredFit) -> Vec<ReportSection> {
        let level = self.request.confidence;
        let mut out = Vec::new();
        if !self.request.probabilities.is_empty() {
            let mut t = TextTable::new(self.interval_headings(&["Fraction failing", "Time"], true));
            for &p in &self.request.probabilities {
                let mut row = vec![format!("{}%", format_number(p * 100.0, 4))];
                row.extend(Self::interval_cells(&fit.quantile(p, level, true), true));
                t.push(row);
            }
            out.push(section(
                SectionKind::DegradationPercentiles,
                "Percentiles of the Time to Reach the Threshold",
                SectionContent::Table(t),
            ));
        }
        if !self.request.times.is_empty() {
            let mut t = TextTable::new(self.interval_headings(&["Time", "Failure probability"], true));
            for &time in &self.request.times {
                let mut row = vec![num(time)];
                row.extend(Self::interval_cells(&fit.failure_probability(time, level, true), true));
                t.push(row);
            }
            out.push(section(
                SectionKind::DegradationProbabilities,
                "Probability of Reaching the Threshold by Each Time",
                SectionContent::Table(t),
            ));
        }
        out
    }

    fn stress_sections(&self, fit: &LifeStressFit) -> Vec<ReportSection> {
        let level = self.request.confidence;
        let stress_names = vec![self.stress_name.clone().unwrap_or_default()];
        let use_s = vec![self.request.use_stress.unwrap_or(f64::NAN)];
        let mut out = Vec::new();
        let mut t = TextTable::new(self.interval_headings(&["Parameter", "Estimate"], true));
        let names = stat_reliability::life_stress::coefficient_names(&fit.model, &stress_names);
        let p = fit.fit.location_coefficients.len();
        for (j, name) in names.iter().enumerate() {
            let mut a = vec![0.0; j + 1];
            a[j] = 1.0;
            let mut row = vec![name.clone()];
            row.extend(Self::interval_cells(&fit.fit.linear_function(&a, 0.0, level, true), true));
            t.push(row);
        }
        if !fit.model.family.fixed_scale() {
            let mut a = vec![0.0; p + 1];
            a[p] = 1.0;
            let e = fit.fit.linear_function(&a, 0.0, level, true);
            let ex = |(a, b): (f64, f64)| (a.exp(), b.exp());
            let est = e.estimate.exp();
            let sigma = IntervalEstimate {
                name: "σ",
                estimate: est,
                std_error: est * e.std_error,
                wald: ex(e.wald),
                likelihood_ratio: e.likelihood_ratio.map(ex),
            };
            let mut row = vec!["Scale σ (of log life)".to_owned()];
            row.extend(Self::interval_cells(&sigma, true));
            t.push(row);
        }
        for d in fit.stress_parameters(&stress_names, level, true) {
            let mut row = vec![d.name];
            row.extend(Self::interval_cells(&d.estimate, true));
            t.push(row);
        }
        out.push(section(
            SectionKind::DegradationLifeParameters,
            format!(
                "{} Life–Stress Model of the Pseudo Failure Times (log-likelihood {})",
                self.request.life_family.label(),
                num(fit.fit.log_likelihood)
            ),
            SectionContent::Table(t),
        ));
        if !self.request.probabilities.is_empty() {
            let mut t = TextTable::new(self.interval_headings(&["Fraction failing", "Time"], true));
            for &q in &self.request.probabilities {
                let mut row = vec![format!("{}%", format_number(q * 100.0, 4))];
                row.extend(Self::interval_cells(&fit.quantile(&use_s, q, level, true), true));
                t.push(row);
            }
            out.push(section(
                SectionKind::DegradationPercentiles,
                format!("Percentiles of the Time to Reach the Threshold{}", self.use_text()),
                SectionContent::Table(t),
            ));
        }
        if !self.request.times.is_empty() {
            let mut t = TextTable::new(self.interval_headings(&["Time", "Failure probability"], true));
            for &time in &self.request.times {
                let mut row = vec![num(time)];
                row.extend(Self::interval_cells(&fit.failure_probability(&use_s, time, level, true), true));
                t.push(row);
            }
            out.push(section(
                SectionKind::DegradationProbabilities,
                format!("Probability of Reaching the Threshold by Each Time{}", self.use_text()),
                SectionContent::Table(t),
            ));
        }
        out
    }

    fn random_effects_table(&self, fit: &RandomPathFit) -> ReportSection {
        let mut t = TextTable::new(["Quantity", "Estimate", "Std error"]);
        for fe in &fit.mixed.fixed {
            let name = if fe.name == "Intercept" { "Mean intercept β₀" } else { "Mean slope β₁" };
            t.push(vec![name.to_owned(), num(fe.estimate), format_number(fe.std_error, 4)]);
        }
        let [s00, s01, s11] = fit.sigma;
        t.push(vec!["Intercept SD between units".to_owned(), num(s00.sqrt()), String::new()]);
        t.push(vec!["Slope SD between units".to_owned(), num(s11.sqrt()), String::new()]);
        let corr = if s00 > 0.0 && s11 > 0.0 { s01 / (s00 * s11).sqrt() } else { f64::NAN };
        t.push(vec!["Intercept–slope correlation".to_owned(), format_number(corr, 4), String::new()]);
        t.push(vec!["Residual SD".to_owned(), num(fit.residual_sd), String::new()]);
        t.push(vec!["−2 log-likelihood (REML)".to_owned(), num(fit.mixed.minus_two_log_likelihood), String::new()]);
        for w in &fit.mixed.warnings {
            t.push(vec!["Note".to_owned(), w.clone(), String::new()]);
        }
        let (g, h) = self.request.transforms();
        section(
            SectionKind::DegradationRandomEffects,
            format!(
                "Random-Effects Path Model ({} = (β₀ + b₀) + (β₁ + b₁)·{} + ε, unit effects b correlated normal)",
                g.describe(&self.response_name),
                h.describe(&self.time_name)
            ),
            SectionContent::Table(t),
        )
    }

    fn random_life_curve(&self, fit: &RandomPathFit, bands: Option<&BootstrapBands>) -> ReportSection {
        let t_max = self.units.iter().flat_map(|u| u.times.iter().copied()).fold(0.0, f64::max);
        let horizon = 100.0 * t_max;
        let times = curve_times(fit, &self.request.times, t_max, horizon);
        let line: Polyline =
            times.iter().map(|&t| [t, fit.failure_probability(t)]).filter(|p| p[1].is_finite()).collect();
        let band = bands.map(|b| {
            let n = times.len().min(b.times.len());
            let lower: Polyline = (0..n).map(|k| [b.times[k], b.lower[k]]).filter(|p| p[1].is_finite()).collect();
            let upper: Polyline = (0..n).map(|k| [b.times[k], b.upper[k]]).filter(|p| p[1].is_finite()).collect();
            (lower, upper)
        });
        let spec = PlotSpec {
            x_label: self.time_name.clone(),
            y_label: "Fraction past the threshold F(t)".to_owned(),
            line: Some(line),
            line_name: Some("Implied life distribution".to_owned()),
            band_name: bands.map(|b| format!("Bootstrap {} limits ({} replicates)", self.level_text(), b.replicates)),
            band,
            ..PlotSpec::default()
        };
        section(
            SectionKind::DegradationLifeCurve,
            "Implied Life Distribution (fraction of units whose path is past the threshold)",
            SectionContent::Plot(Box::new(spec)),
        )
    }

    fn random_estimates(
        &self,
        fit: &RandomPathFit,
        bands: Option<&BootstrapBands>,
        horizon: f64,
    ) -> Vec<ReportSection> {
        let level = self.level_text();
        let mut out = Vec::new();
        let heads = |first: &str, second: &str| {
            let mut h = vec![first.to_owned(), second.to_owned()];
            if bands.is_some() {
                h.extend([format!("Bootstrap lower {level}"), format!("Bootstrap upper {level}")]);
            }
            h
        };
        if !self.request.probabilities.is_empty() {
            let mut t = TextTable::new(heads("Fraction failing", "Time"));
            for (k, &p) in self.request.probabilities.iter().enumerate() {
                let mut row = vec![format!("{}%", format_number(p * 100.0, 4)), num(fit.quantile(p, horizon))];
                if let Some(b) = bands {
                    let (_, lo, hi) = b.quantiles[k];
                    row.extend([num(lo), if hi.is_finite() { num(hi) } else { "not reached".to_owned() }]);
                }
                t.push(row);
            }
            out.push(section(
                SectionKind::DegradationPercentiles,
                "Percentiles of the Time to Reach the Threshold (random-effects model)",
                SectionContent::Table(t),
            ));
        }
        if !self.request.times.is_empty() {
            let mut t = TextTable::new(heads("Time", "Failure probability"));
            let offset = bands.map_or(0, |b| b.times.len() - self.request.times.len());
            for (k, &time) in self.request.times.iter().enumerate() {
                let mut row = vec![num(time), num(fit.failure_probability(time))];
                if let Some(b) = bands {
                    row.extend([num(b.lower[offset + k]), num(b.upper[offset + k])]);
                }
                t.push(row);
            }
            out.push(section(
                SectionKind::DegradationProbabilities,
                "Probability of Reaching the Threshold by Each Time (random-effects model)",
                SectionContent::Table(t),
            ));
        }
        out
    }

    fn destructive_parameters(&self, fit: &DestructiveFit) -> ReportSection {
        use stat_reliability::life_stress::StressTransform;
        let level = self.request.confidence;
        let (g, h) = self.request.transforms();
        let stress = self.stress_name.clone().unwrap_or_default();
        let at_use = if self.request.stress.is_some() { " at use conditions" } else { "" };
        let mut names =
            vec!["Intercept β₀".to_owned(), format!("Rate β₁ (per unit of {}{at_use})", h.describe(&self.time_name))];
        if let Some(st) = &self.request.stress {
            names.push(match st.transform {
                StressTransform::Arrhenius | StressTransform::Eyring => {
                    "Rate change β₂ (per unit of 1/kT, eV⁻¹)".to_owned()
                }
                StressTransform::InversePower => format!("Rate change β₂ (per unit of ln {stress})"),
                StressTransform::Exponential => format!("Rate change β₂ (per unit of {stress})"),
            });
        }
        names.push("Scale σ".to_owned());
        let estimates = fit.parameter_estimates(level);
        let mut t = TextTable::new(self.interval_headings(&["Parameter", "Estimate"], false));
        for (name, e) in names.iter().zip(&estimates) {
            let mut row = vec![name.clone()];
            row.extend(Self::interval_cells(e, false));
            t.push(row);
        }
        if let Some(st) = &self.request.stress {
            let b2 = &estimates[2];
            let (name, e) = match st.transform {
                StressTransform::Arrhenius | StressTransform::Eyring => (
                    "Activation energy of the rate (eV) = −β₂".to_owned(),
                    IntervalEstimate {
                        name: "Ea",
                        estimate: -b2.estimate,
                        std_error: b2.std_error,
                        wald: (-b2.wald.1, -b2.wald.0),
                        likelihood_ratio: None,
                    },
                ),
                StressTransform::InversePower => (format!("Power of {stress} in the rate = β₂"), b2.clone()),
                StressTransform::Exponential => (format!("Rate multiplier per unit of {stress} = exp(β₂)"), {
                    IntervalEstimate {
                        name: "exp(β₂)",
                        estimate: b2.estimate.exp(),
                        std_error: b2.estimate.exp() * b2.std_error,
                        wald: (b2.wald.0.exp(), b2.wald.1.exp()),
                        likelihood_ratio: None,
                    }
                }),
            };
            let mut row = vec![name];
            row.extend(Self::interval_cells(&e, false));
            t.push(row);
        }
        let rate = if self.request.stress.is_some() { "β₁·exp(β₂(x − x_use))" } else { "β₁" };
        section(
            SectionKind::DegradationLifeParameters,
            format!(
                "{} Regression of {} (location μ = β₀ + {rate}·{}; log-likelihood {})",
                self.request.response_family.label(),
                g.describe(&self.response_name),
                h.describe(&self.time_name),
                num(fit.log_likelihood)
            ),
            SectionContent::Table(t),
        )
    }

    fn destructive_life_curve(&self, fit: &DestructiveFit) -> ReportSection {
        let level = self.request.confidence;
        let use_s = self.request.use_stress;
        let median = fit.quantile(0.5, use_s, level).estimate;
        let t_max = if median.is_finite() && median > 0.0 { median * 3.0 } else { 1.0 };
        let grid: Vec<f64> = (1..=CURVE_POINTS).map(|i| t_max * i as f64 / CURVE_POINTS as f64).collect();
        let est: Vec<IntervalEstimate> = grid.iter().map(|&t| fit.failure_probability(t, use_s, level)).collect();
        let line: Polyline =
            grid.iter().zip(&est).map(|(&t, e)| [t, e.estimate]).filter(|p| p[1].is_finite()).collect();
        let lower: Polyline = grid.iter().zip(&est).map(|(&t, e)| [t, e.wald.0]).filter(|p| p[1].is_finite()).collect();
        let upper: Polyline = grid.iter().zip(&est).map(|(&t, e)| [t, e.wald.1]).filter(|p| p[1].is_finite()).collect();
        let spec = PlotSpec {
            x_label: self.time_name.clone(),
            y_label: "Fraction past the threshold F(t)".to_owned(),
            line: Some(line),
            line_name: Some("Time-to-threshold distribution".to_owned()),
            band: Some((lower, upper)),
            band_name: Some(format!("Pointwise Wald {} limits", self.level_text())),
            ..PlotSpec::default()
        };
        section(
            SectionKind::DegradationLifeCurve,
            format!("Time-to-Threshold Distribution{}", self.use_text()),
            SectionContent::Plot(Box::new(spec)),
        )
    }

    fn destructive_estimates(&self, fit: &DestructiveFit) -> Vec<ReportSection> {
        let level = self.request.confidence;
        let use_s = self.request.use_stress;
        let mut out = Vec::new();
        if !self.request.probabilities.is_empty() {
            let mut t = TextTable::new(self.interval_headings(&["Fraction failing", "Time"], false));
            for &p in &self.request.probabilities {
                let mut row = vec![format!("{}%", format_number(p * 100.0, 4))];
                row.extend(Self::interval_cells(&fit.quantile(p, use_s, level), false));
                t.push(row);
            }
            out.push(section(
                SectionKind::DegradationPercentiles,
                format!("Percentiles of the Time to Reach the Threshold{}", self.use_text()),
                SectionContent::Table(t),
            ));
        }
        if !self.request.times.is_empty() {
            let mut t = TextTable::new(self.interval_headings(&["Time", "Failure probability"], false));
            for &time in &self.request.times {
                let mut row = vec![num(time)];
                row.extend(Self::interval_cells(&fit.failure_probability(time, use_s, level), false));
                t.push(row);
            }
            out.push(section(
                SectionKind::DegradationProbabilities,
                format!("Probability of Reaching the Threshold by Each Time{}", self.use_text()),
                SectionContent::Table(t),
            ));
        }
        let _ = format_p_value;
        out
    }
}

/// Times at which the random-effects life curve is drawn: from zero to
/// beyond the 99th percentile (or the requested times).
fn curve_times(fit: &RandomPathFit, extra: &[f64], t_max: f64, horizon: f64) -> Vec<f64> {
    let q99 = fit.quantile(0.99, horizon);
    let end =
        [q99 * 1.1, t_max * 1.5].into_iter().chain(extra.iter().copied()).filter(|v| v.is_finite()).fold(0.0, f64::max);
    let end = if end > 0.0 { end } else { 1.0 };
    (1..=CURVE_POINTS).map(|i| end * i as f64 / CURVE_POINTS as f64).collect()
}

// ---------------------------------------------------------------------------
// Project-file (JSON) form
// ---------------------------------------------------------------------------

/// The JSON form of a degradation report.
pub(crate) fn to_json(r: &DegradationRequest, table: &DataTable) -> JsonValue {
    let col = |i: usize| s(table.column(i).map_or("", |c| c.name.as_str()));
    let mut m: Vec<(String, JsonValue)> = vec![("analysis".into(), s("degradation"))];
    match r.layout {
        DegradationLayout::RepeatedMeasures { unit, time, response } => {
            m.push(("layout".into(), s("repeated")));
            m.push(("unit".into(), col(unit)));
            m.push(("time".into(), col(time)));
            m.push(("response".into(), col(response)));
        }
        DegradationLayout::Destructive { time, response } => {
            m.push(("layout".into(), s("destructive")));
            m.push(("time".into(), col(time)));
            m.push(("response".into(), col(response)));
        }
    }
    let path = match r.path {
        PathModel::Linear { response, time } => {
            m.push(("response_transform".into(), s(response.key())));
            m.push(("time_transform".into(), s(time.key())));
            "linear"
        }
        PathModel::Asymptotic => "asymptotic",
        PathModel::Power => "power",
    };
    m.push(("path".into(), s(path)));
    m.push(("threshold".into(), JsonValue::number(r.threshold)));
    let dir = match r.direction {
        FailureDirection::Rising => "rising",
        FailureDirection::Falling => "falling",
    };
    m.push(("direction".into(), s(dir)));
    m.push(("method".into(), s(r.method.key())));
    m.push(("distribution".into(), s(r.life_family.key())));
    m.push(("response_distribution".into(), s(r.response_family.key())));
    if let Some(st) = &r.stress {
        m.push(("stress".into(), crate::life_stress::stress_json(st, &col)));
    }
    if let Some(u) = r.use_stress {
        m.push(("use".into(), JsonValue::number(u)));
    }
    m.push(("confidence".into(), JsonValue::number(r.confidence)));
    m.push(("probabilities".into(), numbers(&r.probabilities)));
    m.push(("times".into(), numbers(&r.times)));
    m.push(("bootstrap".into(), JsonValue::number(r.bootstrap as f64)));
    JsonValue::Object(m)
}

/// Read a degradation report from its JSON form.
pub(crate) fn from_json(v: &JsonValue, table: &DataTable) -> Result<SavedReport> {
    let col = |key: &str| -> Result<usize> {
        let name =
            member(v, key)?.as_str().ok_or_else(|| json_invalid(format!("report {key}: expected a column name")))?;
        table.find(name).ok_or_else(|| json_invalid(format!("report refers to a missing column {name:?}")))
    };
    let text = |key: &str| v.get(key).and_then(JsonValue::as_str);
    let layout = match text("layout") {
        None | Some("repeated") => {
            DegradationLayout::RepeatedMeasures { unit: col("unit")?, time: col("time")?, response: col("response")? }
        }
        Some("destructive") => DegradationLayout::Destructive { time: col("time")?, response: col("response")? },
        Some(other) => return Err(json_invalid(format!("degradation: unknown layout {other:?}"))),
    };
    let transform = |key: &str| -> Result<ScaleTransform> {
        match text(key) {
            None => Ok(ScaleTransform::Identity),
            Some(k) => {
                ScaleTransform::from_key(k).ok_or_else(|| json_invalid(format!("degradation: unknown transform {k:?}")))
            }
        }
    };
    let path = match text("path") {
        None | Some("linear") => {
            PathModel::Linear { response: transform("response_transform")?, time: transform("time_transform")? }
        }
        Some("asymptotic") => PathModel::Asymptotic,
        Some("power") => PathModel::Power,
        Some(other) => return Err(json_invalid(format!("degradation: unknown path {other:?}"))),
    };
    let direction = match text("direction") {
        None | Some("rising") => FailureDirection::Rising,
        Some("falling") => FailureDirection::Falling,
        Some(other) => return Err(json_invalid(format!("degradation: unknown direction {other:?}"))),
    };
    let method = match text("method") {
        None | Some("pseudo-failure-times") => DegradationMethod::PseudoFailureTimes,
        Some("random-effects") => DegradationMethod::RandomEffects,
        Some(other) => return Err(json_invalid(format!("degradation: unknown method {other:?}"))),
    };
    let family = |key: &str, default: RegressionFamily| -> Result<RegressionFamily> {
        match text(key) {
            None => Ok(default),
            Some(k) => RegressionFamily::from_key(k)
                .ok_or_else(|| json_invalid(format!("degradation: unknown distribution {k:?}"))),
        }
    };
    let threshold =
        member(v, "threshold")?.as_f64().ok_or_else(|| json_invalid("degradation: threshold must be a number"))?;
    let stress = match v.get("stress") {
        None | Some(JsonValue::Null) => None,
        Some(x) => Some(crate::life_stress::stress_from_json(x, table)?),
    };
    let d = DegradationRequest::new(layout, threshold, direction);
    Ok(SavedReport::Degradation {
        request: DegradationRequest {
            path,
            method,
            life_family: family("distribution", d.life_family)?,
            response_family: family("response_distribution", d.response_family)?,
            stress,
            use_stress: v.get("use").and_then(JsonValue::as_f64),
            confidence: v.get("confidence").and_then(JsonValue::as_f64).unwrap_or(d.confidence),
            probabilities: numbers_from(v, "probabilities").unwrap_or_else(|| d.probabilities.clone()),
            times: numbers_from(v, "times").unwrap_or_default(),
            bootstrap: v.get("bootstrap").and_then(JsonValue::as_usize).unwrap_or(d.bootstrap),
            ..d
        },
    })
}

#[cfg(test)]
mod tests;
