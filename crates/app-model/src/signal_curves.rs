//! Signals and spectra bound to a dataset ([`stat_signal`]): smoothing
//! filters, derivatives and integrals of x–y data, and spectrum analysis
//! (baseline correction, peak finding and multi-peak fitting).
//!
//! **Signals.** A signal is a response column ordered by an x column (or by
//! row order when there is none), one signal per level of an optional group
//! (curve) column, so many signals stored in long format are handled at
//! once. Excluded rows and rows missing a value used are left out. Rows with
//! the same x within a signal are averaged into one point (a note says so).
//! Filters that need even spacing use the mean spacing and say when the x
//! values are not evenly spaced.
//!
//! **Smoothing** saves the smoothed values (or a Savitzky–Golay derivative);
//! **derivatives and integrals** save `dy/dx` and the running integral and
//! can list the signal resampled onto an even grid (a table that can be saved
//! as a data table); **spectrum peaks** saves the baseline, the corrected and
//! smoothed spectra, and the fitted values and residuals of the peak model.
//!
//! **Many spectra.** With a curve column the same peak model is fitted to
//! every curve: starting values come from the first curve (in order of first
//! appearance), so peak *k* is the same band in every curve, and a table
//! lists each curve's peak parameters (one row per curve) for further
//! analysis as a data table.

use crate::compare::{DIGITS, section};
use crate::format::{format_number, format_p_value};
use crate::project::{invalid, member};
use crate::report::{
    Annotations, PlotSpec, PointGroups, ReportSection, RowPoint, SectionContent, SectionKind, TextTable,
};
use crate::report_columns::ReportColumn;
use stat_core::Result;
use stat_data::{DataTable, JsonValue, RowStateManager};
use stat_signal::calculus::{Integration, Interpolation, cumulative_integral, gradient, linear_grid, resample};
use stat_signal::peak_fit::{BaselineTerm, PeakFit, PeakFitSpec, PeakStart, Spacing, compare_peak_counts, fit_peaks};
use stat_signal::peaks::{Peak, PeakSearch, find_peaks, noise_level, position_at};
use stat_signal::smooth::{MovingAverage, Smoother};
use stat_signal::{BaselineMethod, PeakShape, estimate_baseline, savitzky_golay, smooth};

/// Most signals one report draws plots for.
const MAX_PLOTS: usize = 8;
/// Most peaks fitted when the number is taken from the peaks found.
pub const MAX_FIT_PEAKS: usize = 12;
/// Points of each drawn model curve.
const CURVE_POINTS: usize = 600;

fn num(v: f64) -> String {
    format_number(v, DIGITS)
}

fn pm(est: (f64, f64)) -> [String; 2] {
    [num(est.0), if est.1.is_finite() { num(est.1) } else { "—".to_owned() }]
}

// ---------------------------------------------------------------------------
// Requests
// ---------------------------------------------------------------------------

/// Smoothing a signal.
#[derive(Debug, Clone, PartialEq)]
pub struct SignalSmoothingRequest {
    /// Column ordering the signal (`None`: row order).
    pub x: Option<usize>,
    /// Signal column.
    pub y: usize,
    /// One signal per level of this column.
    pub group: Option<usize>,
    /// Filter and its settings (windows in points).
    pub method: Smoother,
}

impl SignalSmoothingRequest {
    /// Savitzky–Golay, 11 points, cubic.
    #[must_use]
    pub fn new(y: usize) -> Self {
        Self { x: None, y, group: None, method: Smoother::SavitzkyGolay { window: 11, order: 3, derivative: 0 } }
    }
}

/// How the derivative is computed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DerivativeMethod {
    /// Central differences (exact for quadratics).
    CentralDifferences,
    /// Savitzky–Golay derivative over this many points, polynomial degree.
    SavitzkyGolay {
        /// Window (odd).
        window: usize,
        /// Polynomial degree.
        order: usize,
    },
}

/// Resampling onto an even grid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResampleSettings {
    /// Interpolation method.
    pub method: Interpolation,
    /// Grid points per signal (from its first to its last x).
    pub points: usize,
}

/// Derivatives, integrals and resampling of x–y data.
#[derive(Debug, Clone, PartialEq)]
pub struct CalculusRequest {
    /// x column.
    pub x: usize,
    /// y column.
    pub y: usize,
    /// One signal per level of this column.
    pub group: Option<usize>,
    /// Derivative method.
    pub derivative: DerivativeMethod,
    /// Integration rule.
    pub integral: Integration,
    /// Resampled table (`None`: not made).
    pub resample: Option<ResampleSettings>,
}

impl CalculusRequest {
    /// Central differences, trapezoid rule, no resampling.
    #[must_use]
    pub fn new(x: usize, y: usize) -> Self {
        Self {
            x,
            y,
            group: None,
            derivative: DerivativeMethod::CentralDifferences,
            integral: Integration::Trapezoid,
            resample: None,
        }
    }
}

/// Baseline removed before peaks are found and fitted.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SpectrumBaseline {
    /// None.
    None,
    /// Estimated by a method.
    Estimated(BaselineMethod),
    /// A known baseline in this column, subtracted.
    Column(usize),
}

/// Peak detection settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PeakDetection {
    /// Savitzky–Golay smoothing before searching: (window, degree).
    pub smoothing: Option<(usize, usize)>,
    /// Smallest height, in multiples of the noise level.
    pub min_height: f64,
    /// Smallest prominence, in multiples of the noise level.
    pub min_prominence: f64,
    /// Smallest distance between peaks, in x units (0: none).
    pub min_separation: f64,
}

impl Default for PeakDetection {
    fn default() -> Self {
        Self { smoothing: Some((11, 3)), min_height: 3.0, min_prominence: 5.0, min_separation: 0.0 }
    }
}

/// Peak fitting settings.
#[derive(Debug, Clone, PartialEq)]
pub struct PeakFitSettings {
    /// Peak shape.
    pub shape: PeakShape,
    /// Baseline term fitted with the peaks.
    pub baseline: BaselineTerm,
    /// Number of peaks (`None`: every peak found, at most [`MAX_FIT_PEAKS`]),
    /// ignored when `centres` are given.
    pub count: Option<usize>,
    /// Starting centres typed by the user (empty: from the peaks found).
    pub centres: Vec<f64>,
    /// Compare fits with 1, 2, … peaks.
    pub compare: bool,
    /// One width for every peak.
    pub common_width: bool,
    /// Relation between the centres.
    pub spacing: Spacing,
}

impl Default for PeakFitSettings {
    fn default() -> Self {
        Self {
            shape: PeakShape::Gaussian,
            baseline: BaselineTerm::Constant,
            count: None,
            centres: Vec::new(),
            compare: true,
            common_width: false,
            spacing: Spacing::Free,
        }
    }
}

/// Spectrum analysis: baseline, peaks and peak fit.
#[derive(Debug, Clone, PartialEq)]
pub struct SpectrumRequest {
    /// x column (wavenumber, wavelength, time…).
    pub x: usize,
    /// Intensity column.
    pub y: usize,
    /// One spectrum per level of this column.
    pub curve: Option<usize>,
    /// Baseline.
    pub baseline: SpectrumBaseline,
    /// Peak finding.
    pub detection: PeakDetection,
    /// Peak fitting (`None`: peaks are only found).
    pub fit: Option<PeakFitSettings>,
}

impl SpectrumRequest {
    /// arPLS baseline (λ = 10⁵), default peak finding, no fit.
    #[must_use]
    pub fn new(x: usize, y: usize) -> Self {
        Self {
            x,
            y,
            curve: None,
            baseline: SpectrumBaseline::Estimated(BaselineMethod::Arpls { lambda: 1e5 }),
            detection: PeakDetection::default(),
            fit: None,
        }
    }
}

/// A signal-processing report request.
#[derive(Debug, Clone, PartialEq)]
pub enum SignalRequest {
    /// Smoothing.
    Smoothing(SignalSmoothingRequest),
    /// Derivatives, integrals and resampling.
    Calculus(CalculusRequest),
    /// Baseline, peaks and peak fit.
    Spectrum(SpectrumRequest),
}

fn col_name(table: &DataTable, i: usize) -> JsonValue {
    JsonValue::String(table.column(i).map_or_else(String::new, |c| c.name.clone()))
}

fn opt_col(table: &DataTable, i: Option<usize>) -> JsonValue {
    i.map_or(JsonValue::Null, |i| col_name(table, i))
}

fn n(v: f64) -> JsonValue {
    JsonValue::number(v)
}

fn object(members: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(members.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

fn find_col(v: &JsonValue, key: &str, table: &DataTable) -> Result<usize> {
    let name = member(v, key)?.as_str().ok_or_else(|| invalid(format!("{key}: expected a column name")))?;
    table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")))
}

fn opt_find(v: &JsonValue, key: &str, table: &DataTable) -> Result<Option<usize>> {
    match v.get(key) {
        None | Some(JsonValue::Null) => Ok(None),
        Some(_) => find_col(v, key, table).map(Some),
    }
}

fn get_f(v: &JsonValue, key: &str, default: f64) -> f64 {
    v.get(key).and_then(JsonValue::as_f64).unwrap_or(default)
}

fn get_u(v: &JsonValue, key: &str, default: usize) -> Result<usize> {
    match v.get(key) {
        None | Some(JsonValue::Null) => Ok(default),
        Some(x) => x.as_usize().ok_or_else(|| invalid(format!("{key}: expected a whole number"))),
    }
}

fn get_s<'a>(v: &'a JsonValue, key: &str, default: &'a str) -> &'a str {
    v.get(key).and_then(JsonValue::as_str).unwrap_or(default)
}

fn smoother_json(m: &Smoother) -> Vec<(&'static str, JsonValue)> {
    match *m {
        Smoother::SavitzkyGolay { window, order, derivative } => vec![
            ("method", JsonValue::String("savitzky-golay".into())),
            ("window", n(window as f64)),
            ("order", n(order as f64)),
            ("derivative", n(derivative as f64)),
        ],
        Smoother::MovingAverage { window, kind } => vec![
            (
                "method",
                JsonValue::String(
                    if kind == MovingAverage::Centred { "moving-average" } else { "trailing-average" }.into(),
                ),
            ),
            ("window", n(window as f64)),
        ],
        Smoother::Exponential { alpha } => {
            vec![("method", JsonValue::String("exponential".into())), ("alpha", n(alpha))]
        }
        Smoother::RunningMedian { window } => {
            vec![("method", JsonValue::String("running-median".into())), ("window", n(window as f64))]
        }
        Smoother::RunningPercentile { window, percentile } => vec![
            ("method", JsonValue::String("running-percentile".into())),
            ("window", n(window as f64)),
            ("percentile", n(percentile)),
        ],
        Smoother::Gaussian { sigma } => {
            vec![("method", JsonValue::String("gaussian".into())), ("sigma", n(sigma))]
        }
    }
}

/// The smoother described by `v` (`method` and its settings), as in project
/// files and scripts.
///
/// # Errors
/// An unknown method or a setting of the wrong type.
pub fn smoother_from_json(v: &JsonValue) -> Result<Smoother> {
    let window = get_u(v, "window", 11)?;
    Ok(match get_s(v, "method", "savitzky-golay").replace('_', "-").as_str() {
        "savitzky-golay" => Smoother::SavitzkyGolay {
            window,
            order: get_u(v, "order", 3)?,
            derivative: get_u(v, "derivative", 0)?,
        },
        "moving-average" => Smoother::MovingAverage { window, kind: MovingAverage::Centred },
        "trailing-average" => Smoother::MovingAverage { window, kind: MovingAverage::Trailing },
        "exponential" => Smoother::Exponential { alpha: get_f(v, "alpha", 0.3) },
        "running-median" => Smoother::RunningMedian { window },
        "running-percentile" => Smoother::RunningPercentile { window, percentile: get_f(v, "percentile", 50.0) },
        "gaussian" => Smoother::Gaussian { sigma: get_f(v, "sigma", 2.0) },
        other => return Err(invalid(format!("unknown smoothing method {other:?}"))),
    })
}

fn baseline_json(b: &SpectrumBaseline, table: &DataTable) -> Vec<(&'static str, JsonValue)> {
    let s = |t: &str| JsonValue::String(t.to_owned());
    match *b {
        SpectrumBaseline::None => vec![("baseline", s("none"))],
        SpectrumBaseline::Column(c) => vec![("baseline", s("column")), ("baseline_column", col_name(table, c))],
        SpectrumBaseline::Estimated(BaselineMethod::Asls { lambda, p }) => {
            vec![("baseline", s("asls")), ("lambda", n(lambda)), ("p", n(p))]
        }
        SpectrumBaseline::Estimated(BaselineMethod::Arpls { lambda }) => {
            vec![("baseline", s("arpls")), ("lambda", n(lambda))]
        }
        SpectrumBaseline::Estimated(BaselineMethod::Snip { half_window, decreasing, lls }) => vec![
            ("baseline", s("snip")),
            ("half_window", n(half_window as f64)),
            ("decreasing", JsonValue::Bool(decreasing)),
            ("lls", JsonValue::Bool(lls)),
        ],
    }
}

fn baseline_from_json(v: &JsonValue, table: &DataTable) -> Result<SpectrumBaseline> {
    Ok(match get_s(v, "baseline", "arpls") {
        "none" => SpectrumBaseline::None,
        "column" => SpectrumBaseline::Column(find_col(v, "baseline_column", table)?),
        "asls" => SpectrumBaseline::Estimated(BaselineMethod::Asls {
            lambda: get_f(v, "lambda", 1e5),
            p: get_f(v, "p", 0.01),
        }),
        "arpls" => SpectrumBaseline::Estimated(BaselineMethod::Arpls { lambda: get_f(v, "lambda", 1e5) }),
        "snip" => SpectrumBaseline::Estimated(BaselineMethod::Snip {
            half_window: get_u(v, "half_window", 20)?,
            decreasing: v.get("decreasing").and_then(JsonValue::as_bool).unwrap_or(true),
            lls: v.get("lls").and_then(JsonValue::as_bool).unwrap_or(false),
        }),
        other => return Err(invalid(format!("unknown baseline method {other:?}"))),
    })
}

impl SignalRequest {
    /// Identifier in project files and scripts.
    #[must_use]
    pub fn key(&self) -> &'static str {
        match self {
            Self::Smoothing(_) => "signal-smoothing",
            Self::Calculus(_) => "curve-calculus",
            Self::Spectrum(_) => "spectrum-peaks",
        }
    }

    /// Every identifier [`from_json`](Self::from_json) reads.
    pub const KEYS: [&'static str; 3] = ["signal-smoothing", "curve-calculus", "spectrum-peaks"];

    /// Report name when it could not be computed.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::Smoothing(_) => "Signal Smoothing",
            Self::Calculus(_) => "Derivative and Integral",
            Self::Spectrum(_) => "Spectrum Peaks",
        }
    }

    /// Every column the request uses.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        match self {
            Self::Smoothing(r) => r.x.into_iter().chain([r.y]).chain(r.group).collect(),
            Self::Calculus(r) => [r.x, r.y].into_iter().chain(r.group).collect(),
            Self::Spectrum(r) => {
                let known = match r.baseline {
                    SpectrumBaseline::Column(c) => Some(c),
                    _ => None,
                };
                [r.x, r.y].into_iter().chain(r.curve).chain(known).collect()
            }
        }
    }

    /// The request with its columns renumbered by `f`.
    #[must_use]
    pub fn map_columns(&self, f: &dyn Fn(usize) -> usize) -> Self {
        match self {
            Self::Smoothing(r) => {
                Self::Smoothing(SignalSmoothingRequest { x: r.x.map(f), y: f(r.y), group: r.group.map(f), ..r.clone() })
            }
            Self::Calculus(r) => {
                Self::Calculus(CalculusRequest { x: f(r.x), y: f(r.y), group: r.group.map(f), ..r.clone() })
            }
            Self::Spectrum(r) => Self::Spectrum(SpectrumRequest {
                x: f(r.x),
                y: f(r.y),
                curve: r.curve.map(f),
                baseline: match r.baseline {
                    SpectrumBaseline::Column(c) => SpectrumBaseline::Column(f(c)),
                    other => other,
                },
                ..r.clone()
            }),
        }
    }

    /// Project-file form (see `docs/scripting.md` for the members).
    #[must_use]
    pub fn to_json(&self, table: &DataTable) -> JsonValue {
        let s = |t: &str| JsonValue::String(t.to_owned());
        let mut m: Vec<(&str, JsonValue)> = vec![("analysis", s(self.key()))];
        match self {
            Self::Smoothing(r) => {
                m.extend([("x", opt_col(table, r.x)), ("y", col_name(table, r.y)), ("group", opt_col(table, r.group))]);
                m.extend(smoother_json(&r.method));
            }
            Self::Calculus(r) => {
                m.extend([("x", col_name(table, r.x)), ("y", col_name(table, r.y)), ("group", opt_col(table, r.group))]);
                match r.derivative {
                    DerivativeMethod::CentralDifferences => m.push(("derivative", s("central"))),
                    DerivativeMethod::SavitzkyGolay { window, order } => m.extend([
                        ("derivative", s("savitzky-golay")),
                        ("window", n(window as f64)),
                        ("order", n(order as f64)),
                    ]),
                }
                m.push(("integral", s(if r.integral == Integration::Simpson { "simpson" } else { "trapezoid" })));
                if let Some(rs) = r.resample {
                    m.extend([("resample", s(rs.method.key())), ("points", n(rs.points as f64))]);
                }
            }
            Self::Spectrum(r) => {
                m.extend([("x", col_name(table, r.x)), ("y", col_name(table, r.y)), ("curve", opt_col(table, r.curve))]);
                m.extend(baseline_json(&r.baseline, table));
                let d = &r.detection;
                m.extend([
                    (
                        "smooth_window",
                        d.smoothing.map_or(JsonValue::Null, |(w, _)| n(w as f64)),
                    ),
                    ("smooth_order", d.smoothing.map_or(JsonValue::Null, |(_, o)| n(o as f64))),
                    ("min_height", n(d.min_height)),
                    ("min_prominence", n(d.min_prominence)),
                    ("min_separation", n(d.min_separation)),
                ]);
                if let Some(f) = &r.fit {
                    let mut fit = vec![
                        ("shape", s(f.shape.key())),
                        ("baseline", s(f.baseline.key())),
                        ("peaks", f.count.map_or(JsonValue::Null, |c| n(c as f64))),
                        ("compare", JsonValue::Bool(f.compare)),
                        ("common_width", JsonValue::Bool(f.common_width)),
                        (
                            "spacing",
                            match f.spacing {
                                Spacing::Free => s("free"),
                                Spacing::Equal => s("equal"),
                                Spacing::Fixed(d) => n(d),
                            },
                        ),
                    ];
                    if !f.centres.is_empty() {
                        fit.push(("centres", JsonValue::Array(f.centres.iter().map(|&c| n(c)).collect())));
                    }
                    m.push(("fit", object(fit)));
                }
            }
        }
        object(m)
    }

    /// Read the project-file form whose `analysis` member is `key`.
    ///
    /// # Errors
    /// Missing columns, unknown methods or settings out of range.
    pub fn from_json(key: &str, v: &JsonValue, table: &DataTable) -> Result<Self> {
        Ok(match key {
            "signal-smoothing" => Self::Smoothing(SignalSmoothingRequest {
                x: opt_find(v, "x", table)?,
                y: find_col(v, "y", table)?,
                group: opt_find(v, "group", table)?,
                method: smoother_from_json(v)?,
            }),
            "curve-calculus" => Self::Calculus(CalculusRequest {
                x: find_col(v, "x", table)?,
                y: find_col(v, "y", table)?,
                group: opt_find(v, "group", table)?,
                derivative: match get_s(v, "derivative", "central") {
                    "central" => DerivativeMethod::CentralDifferences,
                    "savitzky-golay" | "savitzky_golay" => DerivativeMethod::SavitzkyGolay {
                        window: get_u(v, "window", 11)?,
                        order: get_u(v, "order", 3)?,
                    },
                    other => return Err(invalid(format!("unknown derivative method {other:?}"))),
                },
                integral: match get_s(v, "integral", "trapezoid") {
                    "trapezoid" => Integration::Trapezoid,
                    "simpson" => Integration::Simpson,
                    other => return Err(invalid(format!("unknown integration rule {other:?}"))),
                },
                resample: match v.get("resample") {
                    None | Some(JsonValue::Null) => None,
                    Some(m) => {
                        let key = m.as_str().ok_or_else(|| invalid("resample: expected a method name"))?;
                        Some(ResampleSettings {
                            method: Interpolation::from_key(key)
                                .ok_or_else(|| invalid(format!("unknown interpolation {key:?}")))?,
                            points: get_u(v, "points", 101)?,
                        })
                    }
                },
            }),
            "spectrum-peaks" => {
                // Absent: the dialog's default smoothing; null: none.
                let window = match v.get("smooth_window") {
                    Some(JsonValue::Null) => None,
                    _ => Some((get_u(v, "smooth_window", 11)?, get_u(v, "smooth_order", 3)?)),
                };
                let detection = PeakDetection {
                    smoothing: window,
                    min_height: get_f(v, "min_height", 3.0),
                    min_prominence: get_f(v, "min_prominence", 5.0),
                    min_separation: get_f(v, "min_separation", 0.0),
                };
                let fit = match v.get("fit") {
                    None | Some(JsonValue::Null) => None,
                    Some(f) => Some(PeakFitSettings {
                        shape: PeakShape::from_key(get_s(f, "shape", "gaussian"))
                            .ok_or_else(|| invalid(format!("unknown peak shape {:?}", get_s(f, "shape", ""))))?,
                        baseline: BaselineTerm::from_key(get_s(f, "baseline", "constant"))
                            .ok_or_else(|| invalid("fit baseline: none, constant or linear"))?,
                        count: match f.get("peaks") {
                            None | Some(JsonValue::Null) => None,
                            Some(c) => Some(c.as_usize().filter(|&c| c >= 1).ok_or_else(|| {
                                invalid("fit peaks: expected a whole number of at least 1")
                            })?),
                        },
                        centres: match f.get("centres") {
                            Some(JsonValue::Array(a)) => a
                                .iter()
                                .map(|c| c.as_f64().ok_or_else(|| invalid("fit centres: expected numbers")))
                                .collect::<Result<_>>()?,
                            _ => Vec::new(),
                        },
                        compare: f.get("compare").and_then(JsonValue::as_bool).unwrap_or(true),
                        common_width: f.get("common_width").and_then(JsonValue::as_bool).unwrap_or(false),
                        spacing: match f.get("spacing") {
                            Some(JsonValue::Number(d)) => Spacing::Fixed(*d),
                            Some(JsonValue::String(t)) if t == "equal" => Spacing::Equal,
                            _ => Spacing::Free,
                        },
                    }),
                };
                Self::Spectrum(SpectrumRequest {
                    x: find_col(v, "x", table)?,
                    y: find_col(v, "y", table)?,
                    curve: opt_find(v, "curve", table)?,
                    baseline: baseline_from_json(v, table)?,
                    detection,
                    fit,
                })
            }
            other => return Err(invalid(format!("unknown signal analysis {other:?}"))),
        })
    }
}

// ---------------------------------------------------------------------------
// Signals from the table
// ---------------------------------------------------------------------------

/// One signal: points sorted by x, each with the rows averaged into it.
#[derive(Debug, Clone)]
struct Signal {
    name: String,
    x: Vec<f64>,
    y: Vec<f64>,
    /// A second averaged column (a known baseline), if asked for.
    extra: Vec<f64>,
    rows: Vec<Vec<usize>>,
    averaged: bool,
}

impl Signal {
    fn spacing(&self) -> f64 {
        let n = self.x.len();
        if n < 2 { 1.0 } else { (self.x[n - 1] - self.x[0]) / (n - 1) as f64 }
    }

    fn uneven(&self) -> bool {
        let d = self.spacing();
        self.x.windows(2).any(|w| ((w[1] - w[0]) - d).abs() > 1e-3 * d.abs())
    }

    fn rows(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        self.rows.iter().enumerate().flat_map(|(i, rs)| rs.iter().map(move |&r| (i, r)))
    }
}

struct Signals {
    x_name: String,
    y_name: String,
    group_name: Option<String>,
    n_rows: usize,
    signals: Vec<Signal>,
}

fn numeric(table: &DataTable, i: usize) -> Result<(&str, &[f64])> {
    let c = table.column(i).ok_or_else(|| invalid("no such column"))?;
    let v = c.as_numeric().ok_or_else(|| invalid(format!("{} must be numeric", c.name)))?;
    Ok((c.name.as_str(), v))
}

fn collect_signals(
    table: &DataTable,
    states: &RowStateManager,
    x: Option<usize>,
    y: usize,
    group: Option<usize>,
    extra: Option<usize>,
) -> Result<Signals> {
    let (y_name, yv) = numeric(table, y)?;
    let (x_name, xv) = match x {
        Some(i) => {
            let (nme, v) = numeric(table, i)?;
            (nme.to_owned(), Some(v))
        }
        None => ("Position".to_owned(), None),
    };
    let ev = match extra {
        Some(i) => Some(numeric(table, i)?.1),
        None => None,
    };
    let g = match group {
        Some(i) => Some(table.column(i).ok_or_else(|| invalid("no such column"))?),
        None => None,
    };
    let usable = |r: usize| {
        !states.is_excluded(r)
            && yv[r].is_finite()
            && xv.is_none_or(|v| v[r].is_finite())
            && ev.is_none_or(|v| v[r].is_finite())
            && g.is_none_or(|c| !c.level_text(r).is_empty())
    };
    let mut names: Vec<String> = Vec::new();
    let mut members: Vec<Vec<usize>> = Vec::new();
    for r in (0..table.n_rows()).filter(|&r| usable(r)) {
        let name = g.map_or_else(String::new, |c| c.level_text(r));
        match names.iter().position(|nm| *nm == name) {
            Some(i) => members[i].push(r),
            None => {
                names.push(name);
                members.push(vec![r]);
            }
        }
    }
    if members.is_empty() {
        return Err(invalid(format!("no rows have a value of {y_name}{}", if x.is_some() { " and x" } else { "" })));
    }
    let signals = names
        .into_iter()
        .zip(members)
        .map(|(name, rows)| {
            let position = |k: usize, r: usize| xv.map_or((k + 1) as f64, |v| v[r]);
            let mut pts: Vec<(f64, usize)> = rows.iter().enumerate().map(|(k, &r)| (position(k, r), r)).collect();
            pts.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut s =
                Signal { name, x: Vec::new(), y: Vec::new(), extra: Vec::new(), rows: Vec::new(), averaged: false };
            let mut i = 0;
            while i < pts.len() {
                let mut j = i;
                while j + 1 < pts.len() && pts[j + 1].0 == pts[i].0 {
                    j += 1;
                }
                let group: Vec<usize> = pts[i..=j].iter().map(|p| p.1).collect();
                let k = group.len() as f64;
                s.x.push(pts[i].0);
                s.y.push(group.iter().map(|&r| yv[r]).sum::<f64>() / k);
                if let Some(e) = ev {
                    s.extra.push(group.iter().map(|&r| e[r]).sum::<f64>() / k);
                }
                s.averaged |= group.len() > 1;
                s.rows.push(group);
                i = j + 1;
            }
            s
        })
        .collect();
    Ok(Signals {
        x_name,
        y_name: y_name.to_owned(),
        group_name: g.map(|c| c.name.clone()),
        n_rows: table.n_rows(),
        signals,
    })
}

impl Signals {
    fn spread(&self, values: impl Fn(usize) -> Option<Vec<f64>>) -> Vec<f64> {
        let mut out = vec![f64::NAN; self.n_rows];
        for (k, s) in self.signals.iter().enumerate() {
            if let Some(v) = values(k) {
                for (i, r) in s.rows() {
                    out[r] = v[i];
                }
            }
        }
        out
    }

    fn label(&self, s: &Signal) -> String {
        match &self.group_name {
            Some(g) => format!("{g} = {}", s.name),
            None => self.y_name.clone(),
        }
    }

    fn row_points(&self, s: &Signal, values: &[f64]) -> Vec<RowPoint> {
        s.rows().map(|(i, r)| RowPoint { row: r, x: s.x[i], y: values[i] }).collect()
    }

    fn notes(&self) -> Vec<String> {
        let mut notes = Vec::new();
        let averaged: Vec<&str> = self.signals.iter().filter(|s| s.averaged).map(|s| s.name.as_str()).collect();
        if !averaged.is_empty() {
            notes.push(format!(
                "Rows with the same {} were averaged into one point{}.",
                self.x_name,
                if self.group_name.is_some() { format!(" (in {})", averaged.join(", ")) } else { String::new() }
            ));
        }
        if self.signals.len() > MAX_PLOTS {
            notes.push(format!(
                "Plots are drawn for the first {MAX_PLOTS} of {} signals; the tables list them all.",
                self.signals.len()
            ));
        }
        notes
    }
}

fn line(x: &[f64], y: &[f64]) -> Vec<[f64; 2]> {
    x.iter().zip(y).filter(|(_, b)| b.is_finite()).map(|(&a, &b)| [a, b]).collect()
}

fn note_section(kind: SectionKind, notes: Vec<String>) -> Option<ReportSection> {
    (!notes.is_empty()).then(|| section(kind, "Notes", SectionContent::Note(notes.join(" "))))
}

// ---------------------------------------------------------------------------
// Smoothing
// ---------------------------------------------------------------------------

/// A computed smoothing report.
#[derive(Debug, Clone)]
pub struct SignalSmoothingAnalysis {
    request: SignalSmoothingRequest,
    data: SignalsData,
    results: Vec<std::result::Result<Vec<f64>, String>>,
}

/// Signals kept by a computed report (cloneable copy of [`Signals`]).
#[derive(Debug, Clone)]
struct SignalsData {
    x_name: String,
    y_name: String,
    group_name: Option<String>,
    n_rows: usize,
    signals: Vec<Signal>,
}

impl From<Signals> for SignalsData {
    fn from(s: Signals) -> Self {
        Self { x_name: s.x_name, y_name: s.y_name, group_name: s.group_name, n_rows: s.n_rows, signals: s.signals }
    }
}

impl SignalsData {
    fn view(&self) -> Signals {
        Signals {
            x_name: self.x_name.clone(),
            y_name: self.y_name.clone(),
            group_name: self.group_name.clone(),
            n_rows: self.n_rows,
            signals: self.signals.clone(),
        }
    }
}

fn spacing_note(s: &Signals) -> Option<String> {
    let uneven: Vec<&str> = s.signals.iter().filter(|g| g.uneven()).map(|g| g.name.as_str()).collect();
    (!uneven.is_empty()).then(|| {
        format!(
            "{} is not evenly spaced{}: the filter treats the points as evenly spaced at the mean spacing \
             (resample the signal first for an exact result).",
            s.x_name,
            if s.group_name.is_some() { format!(" in {}", uneven.join(", ")) } else { String::new() }
        )
    })
}

impl SignalSmoothingAnalysis {
    /// Smooth every signal.
    ///
    /// # Errors
    /// Non-numeric columns or no usable rows.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &SignalSmoothingRequest) -> Result<Self> {
        let data = collect_signals(table, states, request.x, request.y, request.group, None)?;
        let results = data
            .signals
            .iter()
            .map(|s| smooth(&s.y, &request.method, s.spacing()).map_err(|e| e.to_string()))
            .collect();
        Ok(Self { request: request.clone(), data: data.into(), results })
    }

    fn is_derivative(&self) -> bool {
        matches!(self.request.method, Smoother::SavitzkyGolay { derivative, .. } if derivative > 0)
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        let what = if self.is_derivative() { "Smoothed Derivative" } else { "Smoothing" };
        match &self.data.group_name {
            Some(g) => format!("{what}: {} ({g}) — {}", self.data.y_name, self.request.method.label()),
            None => format!("{what}: {} — {}", self.data.y_name, self.request.method.label()),
        }
    }

    /// The smoothed values (or derivative) of each signal, for the rows used.
    #[must_use]
    pub fn saveable_columns(&self) -> Vec<ReportColumn> {
        let v = self.data.view();
        let values = v.spread(|k| self.results[k].as_ref().ok().cloned());
        let (key, label, name) = if self.is_derivative() {
            ("derivative", "Save smoothed derivative", format!("{} smoothed derivative", v.y_name))
        } else {
            ("smoothed", "Save smoothed values", format!("{} smoothed", v.y_name))
        };
        vec![ReportColumn::continuous(key, label, name, values)]
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let v = self.data.view();
        let deriv = self.is_derivative();
        let head = [v.group_name.clone().unwrap_or_else(|| "Signal".to_owned())]
            .into_iter()
            .chain(
                ["Points", "Spacing", if deriv { "SD of derivative" } else { "SD of residual (raw − smoothed)" }]
                    .map(str::to_owned),
            );
        let mut t = TextTable::new(head);
        for (s, r) in v.signals.iter().zip(&self.results) {
            let name = if v.group_name.is_some() { s.name.clone() } else { v.y_name.clone() };
            let stat = match r {
                Ok(z) => {
                    let d: Vec<f64> = if deriv { z.clone() } else { s.y.iter().zip(z).map(|(a, b)| a - b).collect() };
                    let m = d.iter().sum::<f64>() / d.len() as f64;
                    let sd = (d.iter().map(|e| (e - m).powi(2)).sum::<f64>() / (d.len().max(2) - 1) as f64).sqrt();
                    num(sd)
                }
                Err(e) => format!("not available: {e}"),
            };
            t.push([name, s.x.len().to_string(), num(s.spacing()), stat]);
        }
        let mut out = vec![section(
            SectionKind::SignalSummary,
            format!("Summary ({})", self.request.method.label()),
            SectionContent::Table(t),
        )];
        for (s, r) in v.signals.iter().zip(&self.results).take(MAX_PLOTS) {
            let Ok(z) = r else { continue };
            let spec = if deriv {
                PlotSpec {
                    x_label: v.x_name.clone(),
                    y_label: format!("Derivative of {}", v.y_name),
                    points: v.row_points(s, z),
                    line: Some(line(&s.x, z)),
                    line_name: Some("Savitzky–Golay derivative".to_owned()),
                    point_name: Some("Derivative at each row".to_owned()),
                    ..PlotSpec::default()
                }
            } else {
                PlotSpec {
                    x_label: v.x_name.clone(),
                    y_label: v.y_name.clone(),
                    points: v.row_points(s, &s.y),
                    line: Some(line(&s.x, z)),
                    line_name: Some(format!("Smoothed ({})", self.request.method.label())),
                    point_name: Some("Raw values (rows)".to_owned()),
                    ..PlotSpec::default()
                }
            };
            let title = if deriv { "Derivative" } else { "Raw and Smoothed" };
            let title =
                if v.group_name.is_some() { format!("{title}: {}", v.label(s)) } else { title.to_owned() };
            out.push(section(SectionKind::SignalPlot, title, SectionContent::Plot(Box::new(spec))));
        }
        let mut notes = v.notes();
        if !matches!(self.request.method, Smoother::Exponential { .. }) {
            notes.extend(spacing_note(&v));
        }
        out.extend(note_section(SectionKind::SignalNotes, notes));
        out
    }
}

// ---------------------------------------------------------------------------
// Derivatives and integrals
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct CalculusResult {
    derivative: Vec<f64>,
    integral: Vec<f64>,
    resampled: Option<(Vec<f64>, Vec<f64>)>,
}

/// A computed derivatives-and-integrals report.
#[derive(Debug, Clone)]
pub struct CalculusAnalysis {
    request: CalculusRequest,
    data: SignalsData,
    results: Vec<std::result::Result<CalculusResult, String>>,
}

impl CalculusAnalysis {
    /// Differentiate, integrate and (if asked) resample every signal.
    ///
    /// # Errors
    /// Non-numeric columns or no usable rows.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &CalculusRequest) -> Result<Self> {
        let data = collect_signals(table, states, Some(request.x), request.y, request.group, None)?;
        let results = data
            .signals
            .iter()
            .map(|s| {
                let derivative = match request.derivative {
                    DerivativeMethod::CentralDifferences => gradient(&s.x, &s.y),
                    DerivativeMethod::SavitzkyGolay { window, order } => {
                        savitzky_golay(&s.y, window, order, 1, s.spacing())
                    }
                }?;
                let integral = cumulative_integral(&s.x, &s.y, request.integral)?;
                let resampled = match request.resample {
                    Some(rs) => {
                        let grid = linear_grid(s.x[0], s.x[s.x.len() - 1], rs.points.max(2));
                        let values = resample(&s.x, &s.y, &grid, rs.method)?;
                        Some((grid, values))
                    }
                    None => None,
                };
                Ok(CalculusResult { derivative, integral, resampled })
            })
            .map(|r: Result<CalculusResult>| r.map_err(|e| e.to_string()))
            .collect();
        Ok(Self { request: request.clone(), data: data.into(), results })
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        match &self.data.group_name {
            Some(g) => format!("Derivative and Integral: {} by {} ({g})", self.data.y_name, self.data.x_name),
            None => format!("Derivative and Integral: {} by {}", self.data.y_name, self.data.x_name),
        }
    }

    /// The derivative and the running integral, for the rows used.
    #[must_use]
    pub fn saveable_columns(&self) -> Vec<ReportColumn> {
        let v = self.data.view();
        let get = |f: fn(&CalculusResult) -> &Vec<f64>| {
            v.spread(|k| self.results[k].as_ref().ok().map(|r| f(r).clone()))
        };
        vec![
            ReportColumn::continuous(
                "derivative",
                "Save derivative",
                format!("d{}/d{}", v.y_name, v.x_name),
                get(|r| &r.derivative),
            ),
            ReportColumn::continuous(
                "integral",
                "Save running integral",
                format!("Integral of {}", v.y_name),
                get(|r| &r.integral),
            ),
        ]
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let v = self.data.view();
        let r = &self.request;
        let deriv_label = match r.derivative {
            DerivativeMethod::CentralDifferences => "central differences".to_owned(),
            DerivativeMethod::SavitzkyGolay { window, order } => {
                format!("Savitzky–Golay, {window} points, degree {order}")
            }
        };
        let head = [v.group_name.clone().unwrap_or_else(|| "Signal".to_owned())].into_iter().chain(
            ["Points", "From", "To", "Total integral", "Largest slope", "at", "Smallest slope", "at"].map(str::to_owned),
        );
        let mut t = TextTable::new(head);
        for (s, res) in v.signals.iter().zip(&self.results) {
            let name = if v.group_name.is_some() { s.name.clone() } else { v.y_name.clone() };
            let mut row = vec![name, s.x.len().to_string()];
            match res {
                Ok(c) => {
                    let (imax, imin) = c.derivative.iter().enumerate().fold((0, 0), |(a, b), (i, d)| {
                        (if *d > c.derivative[a] { i } else { a }, if *d < c.derivative[b] { i } else { b })
                    });
                    row.extend([
                        num(s.x[0]),
                        num(s.x[s.x.len() - 1]),
                        num(*c.integral.last().unwrap_or(&f64::NAN)),
                        num(c.derivative[imax]),
                        num(s.x[imax]),
                        num(c.derivative[imin]),
                        num(s.x[imin]),
                    ]);
                }
                Err(e) => row.push(format!("not available: {e}")),
            }
            row.resize(t.columns.len(), String::new());
            t.push(row);
        }
        let mut out = vec![section(
            SectionKind::SignalSummary,
            format!("Summary (derivative by {deriv_label}; integral by the {} rule)", r.integral.label()),
            SectionContent::Table(t),
        )];
        for (s, res) in v.signals.iter().zip(&self.results).take(MAX_PLOTS) {
            let Ok(c) = res else { continue };
            let suffix = if v.group_name.is_some() { format!(": {}", v.label(s)) } else { String::new() };
            let plot = |y_label: String, values: &[f64], name: &str| PlotSpec {
                x_label: v.x_name.clone(),
                y_label,
                points: v.row_points(s, values),
                line: Some(line(&s.x, values)),
                line_name: Some(name.to_owned()),
                point_name: Some("Rows".to_owned()),
                ..PlotSpec::default()
            };
            out.push(section(
                SectionKind::SignalPlot,
                format!("Derivative{suffix}"),
                SectionContent::Plot(Box::new(plot(
                    format!("d{}/d{}", v.y_name, v.x_name),
                    &c.derivative,
                    "Derivative",
                ))),
            ));
            out.push(section(
                SectionKind::SignalPlot,
                format!("Running Integral{suffix}"),
                SectionContent::Plot(Box::new(plot(
                    format!("Integral of {}", v.y_name),
                    &c.integral,
                    "Running integral from the first point",
                ))),
            ));
        }
        if let Some(rs) = r.resample {
            let mut head = Vec::new();
            if let Some(g) = &v.group_name {
                head.push(g.clone());
            }
            head.extend([v.x_name.clone(), v.y_name.clone()]);
            let mut table = TextTable::new(head);
            for (s, res) in v.signals.iter().zip(&self.results) {
                let Ok(CalculusResult { resampled: Some((grid, values)), .. }) = res else { continue };
                for (g, y) in grid.iter().zip(values) {
                    let mut row = Vec::new();
                    if v.group_name.is_some() {
                        row.push(s.name.clone());
                    }
                    row.extend([format_number(*g, 10), format_number(*y, 10)]);
                    table.push(row);
                }
            }
            out.push(section(
                SectionKind::SignalResampled,
                format!(
                    "Resampled Values ({}, {} evenly spaced points per signal; save as a data table from the \
                     table's menu)",
                    rs.method.label().to_lowercase(),
                    rs.points.max(2)
                ),
                SectionContent::Table(table),
            ));
        }
        out.extend(note_section(SectionKind::SignalNotes, v.notes()));
        out
    }
}

// ---------------------------------------------------------------------------
// Spectrum peaks
// ---------------------------------------------------------------------------

/// What spectrum analysis found in one curve.
#[derive(Debug, Clone)]
pub struct SpectrumCurve {
    /// Baseline at each point (zeros without one).
    pub baseline: Vec<f64>,
    /// Intensity minus baseline.
    pub corrected: Vec<f64>,
    /// Corrected intensity smoothed for peak finding.
    pub smoothed: Vec<f64>,
    /// Noise level σ̂ of the corrected spectrum (robust, from successive
    /// differences), but never below 0.1% of its range, so that a smooth or
    /// noiseless spectrum does not turn every ripple into a peak.
    pub noise: f64,
    /// Peaks found (on the smoothed spectrum).
    pub peaks: Vec<Peak>,
    /// Peak fit (when asked for).
    pub fit: Option<std::result::Result<PeakFit, String>>,
    /// Comparison of numbers of peaks (when asked for).
    pub comparison: Option<Vec<stat_signal::PeakCountComparison>>,
}

/// A computed spectrum-peaks report.
#[derive(Debug, Clone)]
pub struct SpectrumAnalysis {
    request: SpectrumRequest,
    data: SignalsData,
    results: Vec<std::result::Result<SpectrumCurve, String>>,
}

fn starts_from_peaks(s: &Signal, curve: &SpectrumCurve, count: Option<usize>) -> Vec<PeakStart> {
    let mut found: Vec<&Peak> = curve.peaks.iter().collect();
    found.sort_by(|a, b| b.prominence.total_cmp(&a.prominence));
    let keep = count.unwrap_or(MAX_FIT_PEAKS).min(found.len());
    let min_width = 2.0 * s.spacing().abs();
    found[..keep]
        .iter()
        .map(|p| PeakStart {
            centre: s.x[p.index],
            height: p.prominence.max(curve.corrected[p.index]),
            fwhm: (position_at(&s.x, p.right_crossing) - position_at(&s.x, p.left_crossing)).abs().max(min_width),
        })
        .collect()
}

fn starts_from_centres(s: &Signal, corrected: &[f64], centres: &[f64], width: f64) -> Vec<PeakStart> {
    let f = stat_signal::Interpolator::new(&s.x, corrected, Interpolation::Linear).ok();
    centres
        .iter()
        .map(|&c| PeakStart {
            centre: c,
            height: f.as_ref().map_or(1.0, |f| f.eval(c)).max(f64::MIN_POSITIVE).max(1e-12),
            fwhm: width,
        })
        .collect()
}

impl SpectrumAnalysis {
    /// Remove the baseline, find peaks and (if asked) fit the peak model to
    /// every curve.
    ///
    /// # Errors
    /// Non-numeric columns or no usable rows.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &SpectrumRequest) -> Result<Self> {
        let known = match request.baseline {
            SpectrumBaseline::Column(c) => Some(c),
            _ => None,
        };
        let data = collect_signals(table, states, Some(request.x), request.y, request.curve, known)?;
        let mut results: Vec<std::result::Result<SpectrumCurve, String>> =
            data.signals.iter().map(|s| Self::prepare(s, request).map_err(|e| e.to_string())).collect();
        // Fit: starts from the first curve that could be prepared.
        if let Some(settings) = &request.fit {
            let template: Option<Vec<PeakStart>> =
                data.signals.iter().zip(&results).find_map(|(s, r)| {
                    let c = r.as_ref().ok()?;
                    if settings.centres.is_empty() {
                        Some(starts_from_peaks(s, c, settings.count))
                    } else {
                        let typical = c
                            .peaks
                            .first()
                            .map(|p| (position_at(&s.x, p.right_crossing) - position_at(&s.x, p.left_crossing)).abs())
                            .unwrap_or_else(|| 0.05 * (s.x[s.x.len() - 1] - s.x[0]).abs());
                        Some(starts_from_centres(s, &c.corrected, &settings.centres, typical))
                    }
                });
            for (s, r) in data.signals.iter().zip(results.iter_mut()) {
                let Ok(curve) = r else { continue };
                let Some(template) = &template else { continue };
                if template.is_empty() {
                    curve.fit = Some(Err("no peaks were found to start the fit from; lower the thresholds or \
                                          type starting centres"
                        .to_owned()));
                    continue;
                }
                // Heights from this curve at the template's centres.
                let width = template.iter().map(|p| p.fwhm).sum::<f64>() / template.len() as f64;
                let centres: Vec<f64> = template.iter().map(|p| p.centre).collect();
                let mut starts = starts_from_centres(s, &curve.corrected, &centres, width);
                for (st, t) in starts.iter_mut().zip(template) {
                    st.fwhm = t.fwhm;
                }
                let spec = PeakFitSpec {
                    baseline: settings.baseline,
                    common_width: settings.common_width,
                    spacing: settings.spacing,
                    ..PeakFitSpec::new(settings.shape, starts.clone())
                };
                curve.fit = Some(fit_peaks(&s.x, &curve.corrected, &spec).map_err(|e| e.to_string()));
                if settings.compare && starts.len() > 1 {
                    curve.comparison = compare_peak_counts(&s.x, &curve.corrected, &spec, starts.len()).ok();
                }
            }
        }
        Ok(Self { request: request.clone(), data: data.into(), results })
    }

    fn prepare(s: &Signal, request: &SpectrumRequest) -> Result<SpectrumCurve> {
        let baseline = match request.baseline {
            SpectrumBaseline::None => vec![0.0; s.y.len()],
            SpectrumBaseline::Column(_) => s.extra.clone(),
            SpectrumBaseline::Estimated(m) => estimate_baseline(&s.y, &m)?,
        };
        let corrected: Vec<f64> = s.y.iter().zip(&baseline).map(|(a, b)| a - b).collect();
        let d = &request.detection;
        let smoothed = match d.smoothing {
            Some((w, o)) if s.y.len() >= w => savitzky_golay(&corrected, w, o, 0, s.spacing())?,
            _ => corrected.clone(),
        };
        let (lo, hi) = corrected.iter().filter(|v| v.is_finite()).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &v| (a.min(v), b.max(v)));
        let floor = if hi > lo { 1e-3 * (hi - lo) } else { 0.0 };
        let noise = noise_level(&corrected).max(floor);
        let spacing = s.spacing().abs();
        let search = PeakSearch {
            min_height: d.min_height * noise,
            min_prominence: d.min_prominence * noise,
            min_distance: if spacing > 0.0 { ((d.min_separation / spacing).ceil() as usize).max(1) } else { 1 },
        };
        let peaks = find_peaks(&smoothed, &search)?;
        Ok(SpectrumCurve { baseline, corrected, smoothed, noise, peaks, fit: None, comparison: None })
    }

    /// The computed curves, in order of first appearance, with their names.
    #[must_use]
    pub fn curves(&self) -> Vec<(&str, std::result::Result<&SpectrumCurve, &str>)> {
        self.data
            .signals
            .iter()
            .zip(&self.results)
            .map(|(s, r)| (s.name.as_str(), r.as_ref().map_err(String::as_str)))
            .collect()
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        match &self.data.group_name {
            Some(g) => format!("Spectrum Peaks: {} by {} ({g})", self.data.y_name, self.data.x_name),
            None => format!("Spectrum Peaks: {} by {}", self.data.y_name, self.data.x_name),
        }
    }

    /// Baseline, corrected and smoothed spectra, fitted values and residuals
    /// for the rows used.
    #[must_use]
    pub fn saveable_columns(&self) -> Vec<ReportColumn> {
        let v = self.data.view();
        let get = |f: &dyn Fn(&Signal, &SpectrumCurve) -> Option<Vec<f64>>| {
            v.spread(|k| self.results[k].as_ref().ok().and_then(|c| f(&v.signals[k], c)))
        };
        let fitted = |s: &Signal, c: &SpectrumCurve| -> Option<Vec<f64>> {
            let fit = c.fit.as_ref()?.as_ref().ok()?;
            Some(s.x.iter().zip(&c.baseline).map(|(&x, b)| fit.eval(x) + b).collect())
        };
        let mut cols = Vec::new();
        if self.request.baseline != SpectrumBaseline::None {
            cols.push(ReportColumn::continuous(
                "baseline",
                "Save baseline",
                format!("{} baseline", v.y_name),
                get(&|_, c| Some(c.baseline.clone())),
            ));
            cols.push(ReportColumn::continuous(
                "corrected",
                "Save baseline-corrected values",
                format!("{} corrected", v.y_name),
                get(&|_, c| Some(c.corrected.clone())),
            ));
        }
        if self.request.detection.smoothing.is_some() {
            cols.push(ReportColumn::continuous(
                "smoothed",
                "Save smoothed values",
                format!("{} smoothed", v.y_name),
                get(&|_, c| Some(c.smoothed.clone())),
            ));
        }
        if self.request.fit.is_some() {
            cols.push(ReportColumn::continuous(
                "fitted",
                "Save fitted values",
                format!("{} peak fit", v.y_name),
                get(&|s, c| fitted(s, c)),
            ));
            cols.push(ReportColumn::continuous(
                "residual",
                "Save residuals",
                format!("{} peak fit residual", v.y_name),
                get(&|s, c| fitted(s, c).map(|f| s.y.iter().zip(f).map(|(a, b)| a - b).collect())),
            ));
        }
        cols
    }

    fn baseline_label(&self) -> String {
        match self.request.baseline {
            SpectrumBaseline::None => "no baseline removed".to_owned(),
            SpectrumBaseline::Column(_) => "known baseline subtracted".to_owned(),
            SpectrumBaseline::Estimated(m) => format!("baseline: {}", m.label()),
        }
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let v = self.data.view();
        let grouped = v.group_name.is_some();
        let mut out = Vec::new();
        // Summary.
        let head = [v.group_name.clone().unwrap_or_else(|| "Spectrum".to_owned())]
            .into_iter()
            .chain(["Points", "From", "To", "Noise level σ̂", "Peaks found"].map(str::to_owned))
            .chain(self.request.fit.as_ref().map(|_| "Fit".to_owned()));
        let mut t = TextTable::new(head);
        for (s, r) in v.signals.iter().zip(&self.results) {
            let name = if grouped { s.name.clone() } else { v.y_name.clone() };
            let mut row = vec![name, s.x.len().to_string(), num(s.x[0]), num(s.x[s.x.len() - 1])];
            match r {
                Ok(c) => {
                    row.extend([num(c.noise), c.peaks.len().to_string()]);
                    if let Some(fit) = &c.fit {
                        row.push(match fit {
                            Ok(f) if f.converged() => format!("R² = {}", format_number(f.r_squared, 4)),
                            Ok(f) => f.termination.label().to_owned(),
                            Err(e) => format!("failed: {e}"),
                        });
                    }
                }
                Err(e) => row.push(format!("not available: {e}")),
            }
            row.resize(t.columns.len(), String::new());
            t.push(row);
        }
        let d = &self.request.detection;
        out.push(section(
            SectionKind::SignalSummary,
            format!(
                "Summary ({}; peaks at least {}σ̂ high and {}σ̂ prominent{})",
                self.baseline_label(),
                format_number(d.min_height, 3),
                format_number(d.min_prominence, 3),
                if d.min_separation > 0.0 { format!(", {} apart", num(d.min_separation)) } else { String::new() }
            ),
            SectionContent::Table(t),
        ));
        // Peaks found, all curves in one table.
        let mut head = Vec::new();
        if grouped {
            head.push(v.group_name.clone().unwrap_or_default());
        }
        head.extend(["Peak", "Position", "Height", "Prominence", "FWHM", "Area"].map(str::to_owned));
        let mut peaks = TextTable::new(head);
        for (s, r) in v.signals.iter().zip(&self.results) {
            let Ok(c) = r else { continue };
            for (k, p) in c.peaks.iter().enumerate() {
                let mut row = Vec::new();
                if grouped {
                    row.push(s.name.clone());
                }
                let fwhm = position_at(&s.x, p.right_crossing) - position_at(&s.x, p.left_crossing);
                // Area between the bases above the straight line joining them.
                let (a, b) = (p.left_base, p.right_base);
                let mut area = 0.0;
                for i in a..b {
                    let lin = |j: usize| {
                        c.smoothed[a] + (c.smoothed[b] - c.smoothed[a]) * (s.x[j] - s.x[a]) / (s.x[b] - s.x[a])
                    };
                    let (h0, h1) = (c.smoothed[i] - lin(i), c.smoothed[i + 1] - lin(i + 1));
                    area += 0.5 * (s.x[i + 1] - s.x[i]) * (h0 + h1);
                }
                row.extend([
                    (k + 1).to_string(),
                    num(s.x[p.index]),
                    num(p.height),
                    num(p.prominence),
                    num(fwhm),
                    num(area),
                ]);
                peaks.push(row);
            }
        }
        out.push(section(
            SectionKind::SpectrumPeaks,
            "Peaks Found (on the corrected, smoothed spectrum; width at half prominence; area above the line \
             joining the peak's bases)",
            SectionContent::Table(peaks),
        ));
        // Batch table of fitted parameters.
        if self.request.fit.is_some() && grouped {
            out.extend(self.batch_table(&v));
        }
        // Per curve plots and fit details.
        for (s, r) in v.signals.iter().zip(&self.results).take(MAX_PLOTS) {
            let Ok(c) = r else { continue };
            out.extend(self.curve_sections(&v, s, c));
        }
        out.extend(note_section(SectionKind::SignalNotes, v.notes()));
        out
    }

    fn batch_table(&self, v: &Signals) -> Option<ReportSection> {
        let k = self
            .results
            .iter()
            .filter_map(|r| r.as_ref().ok()?.fit.as_ref()?.as_ref().ok().map(|f| f.peaks.len()))
            .max()?;
        let mut head = vec![v.group_name.clone().unwrap_or_default()];
        for j in 1..=k {
            for what in ["centre", "height", "FWHM", "area"] {
                head.push(format!("Peak {j} {what}"));
            }
        }
        head.extend(["Residual SD", "R²"].map(str::to_owned));
        let mut t = TextTable::new(head);
        for (s, r) in v.signals.iter().zip(&self.results) {
            let Some(Ok(f)) = r.as_ref().ok().and_then(|c| c.fit.as_ref()) else { continue };
            let mut row = vec![s.name.clone()];
            for p in &f.peaks {
                row.extend([num(p.centre.0), num(p.height.0), num(p.fwhm.0), num(p.area.0)]);
            }
            row.extend([num(f.sigma), format_number(f.r_squared, 6)]);
            row.resize(t.columns.len(), String::new());
            t.push(row);
        }
        Some(section(
            SectionKind::SpectrumBatch,
            "Peak Parameters by Curve (the same peaks fitted to every curve; save as a data table from the \
             table's menu)",
            SectionContent::Table(t),
        ))
    }

    fn curve_sections(&self, v: &Signals, s: &Signal, c: &SpectrumCurve) -> Vec<ReportSection> {
        let mut out = Vec::new();
        let suffix = if v.group_name.is_some() { format!(": {}", v.label(s)) } else { String::new() };
        if self.request.baseline != SpectrumBaseline::None {
            let spec = PlotSpec {
                x_label: v.x_name.clone(),
                y_label: v.y_name.clone(),
                points: v.row_points(s, &s.y),
                line: Some(line(&s.x, &c.baseline)),
                line_name: Some("Baseline".to_owned()),
                point_name: Some("Spectrum (rows)".to_owned()),
                ..PlotSpec::default()
            };
            out.push(section(
                SectionKind::SpectrumBaselinePlot,
                format!("Spectrum and Baseline{suffix}"),
                SectionContent::Plot(Box::new(spec)),
            ));
        }
        let markers: Vec<(f64, f64, String, usize)> =
            c.peaks.iter().enumerate().map(|(k, p)| (s.x[p.index], p.height, format!("{}", k + 1), 0)).collect();
        let corrected_label =
            if self.request.baseline == SpectrumBaseline::None { v.y_name.clone() } else { format!("{} corrected", v.y_name) };
        let spec = PlotSpec {
            x_label: v.x_name.clone(),
            y_label: corrected_label.clone(),
            points: v.row_points(s, &c.corrected),
            line: Some(line(&s.x, &c.smoothed)),
            line_name: Some(if self.request.detection.smoothing.is_some() {
                "Smoothed (Savitzky–Golay)".to_owned()
            } else {
                "Spectrum".to_owned()
            }),
            point_name: Some("Corrected values (rows)".to_owned()),
            annotations: Some(Annotations {
                groups: vec!["Peaks found".to_owned()],
                items: markers,
                labels: Vec::new(),
            }),
            ..PlotSpec::default()
        };
        out.push(section(
            SectionKind::SpectrumPeakPlot,
            format!("Peaks{suffix}"),
            SectionContent::Plot(Box::new(spec)),
        ));
        if let Some(fit) = &c.fit {
            match fit {
                Ok(f) => out.extend(self.fit_sections(v, s, c, f, &suffix, &corrected_label)),
                Err(e) => out.push(section(
                    SectionKind::SpectrumFit,
                    format!("Peak Fit{suffix}"),
                    SectionContent::Note(format!("The peak model could not be fitted: {e}")),
                )),
            }
        }
        if let Some(rows) = &c.comparison {
            out.push(comparison_section(rows, &suffix));
        }
        out
    }

    fn fit_sections(
        &self,
        v: &Signals,
        s: &Signal,
        c: &SpectrumCurve,
        f: &PeakFit,
        suffix: &str,
        y_label: &str,
    ) -> Vec<ReportSection> {
        let mut out = Vec::new();
        let (x0, x1) = (s.x[0], s.x[s.x.len() - 1]);
        let grid = linear_grid(x0, x1, CURVE_POINTS.max(4 * s.x.len()).min(4000));
        let total: Vec<[f64; 2]> = grid.iter().map(|&x| [x, f.eval(x)]).collect();
        let k = f.peaks.len();
        let mut labels = vec!["Data".to_owned()];
        let mut lines = vec![Vec::new()];
        for j in 0..k {
            labels.push(format!("Peak {}", j + 1));
            lines.push(grid.iter().map(|&x| [x, f.component(j, x) + f.baseline_at(x)]).collect());
        }
        let points = v.row_points(s, &c.corrected);
        let membership = vec![0; points.len()];
        let segments = if f.spec.baseline == BaselineTerm::None {
            Vec::new()
        } else {
            vec![grid.iter().map(|&x| [x, f.baseline_at(x)]).collect()]
        };
        let spec = PlotSpec {
            x_label: v.x_name.clone(),
            y_label: y_label.to_owned(),
            points,
            line: Some(total),
            line_name: Some("Fitted sum of peaks".to_owned()),
            groups: Some(PointGroups { title: String::new(), labels, membership, lines }),
            segments,
            ..PlotSpec::default()
        };
        out.push(section(
            SectionKind::SpectrumFitPlot,
            format!(
                "Peak Fit{suffix} ({} {} peaks{}; components drawn on the fitted baseline)",
                k,
                f.spec.shape.label(),
                match f.spec.baseline {
                    BaselineTerm::None => String::new(),
                    b => format!(" + {} baseline", b.label().to_lowercase()),
                }
            ),
            SectionContent::Plot(Box::new(spec)),
        ));
        // Fit statistics.
        let mut stats = TextTable::new(["Statistic", "Value"]);
        for (name, value) in [
            ("Points", f.n.to_string()),
            ("Parameters estimated", f.n_free.to_string()),
            ("Residual sum of squares", num(f.sse)),
            ("Residual SD", num(f.sigma)),
            ("R²", format_number(f.r_squared, 6)),
            ("AICc", num(f.aicc)),
            ("BIC", num(f.bic)),
            ("Iterations", f.iterations.to_string()),
            ("Convergence", f.termination.label().to_owned()),
        ] {
            stats.push([name.to_owned(), value]);
        }
        if !f.at_bounds.is_empty() {
            stats.push(["On a bound".to_owned(), f.at_bounds.join(", ")]);
        }
        out.push(section(SectionKind::SpectrumFit, format!("Fit Statistics{suffix}"), SectionContent::Table(stats)));
        // Peaks with standard errors.
        let mut t = TextTable::new([
            "Peak", "Centre", "Std error", "Height", "Std error", "FWHM", "Std error", "Area", "Std error", "% of area",
        ]);
        let total_area: f64 = f.peaks.iter().map(|p| p.area.0).sum();
        for (j, p) in f.peaks.iter().enumerate() {
            let mut row = vec![(j + 1).to_string()];
            row.extend(pm(p.centre));
            row.extend(pm(p.height));
            row.extend(pm(p.fwhm));
            row.extend(pm(p.area));
            row.push(if total_area > 0.0 { format_number(100.0 * p.area.0 / total_area, 4) } else { String::new() });
            t.push(row);
        }
        out.push(section(
            SectionKind::SpectrumFitPeaks,
            format!("Fitted Peaks{suffix} (standard errors from the fit's covariance; FWHM and area by the delta method)"),
            SectionContent::Table(t),
        ));
        // Every parameter.
        let names = f.spec.shape.param_names();
        let mut t = TextTable::new(["Parameter", "Estimate", "Std error"]);
        for (j, p) in f.peaks.iter().enumerate() {
            for (i, nm) in names.iter().enumerate() {
                let mut row = vec![format!("Peak {} {}", j + 1, nm.to_lowercase())];
                row.extend(pm((p.params[i], p.param_se[i])));
                t.push(row);
            }
        }
        let base_names: &[&str] = match f.spec.baseline {
            BaselineTerm::None => &[],
            BaselineTerm::Constant => &["Baseline level"],
            BaselineTerm::Linear => &["Baseline intercept", "Baseline slope"],
        };
        for (nm, est) in base_names.iter().zip(&f.baseline) {
            let mut row = vec![(*nm).to_owned()];
            row.extend(pm(*est));
            t.push(row);
        }
        out.push(section(
            SectionKind::SpectrumFitParameters,
            format!("Parameter Estimates{suffix}"),
            SectionContent::Table(t),
        ));
        // Residuals.
        let residuals: Vec<f64> = s.x.iter().zip(&c.corrected).map(|(&x, y)| y - f.eval(x)).collect();
        let spec = PlotSpec {
            x_label: v.x_name.clone(),
            y_label: "Residual".to_owned(),
            points: v.row_points(s, &residuals),
            line: Some(vec![[x0, 0.0], [x1, 0.0]]),
            line_name: Some("Zero".to_owned()),
            point_name: Some("Residuals (rows)".to_owned()),
            ..PlotSpec::default()
        };
        out.push(section(
            SectionKind::SpectrumResiduals,
            format!("Residuals{suffix}"),
            SectionContent::Plot(Box::new(spec)),
        ));
        out
    }
}

fn comparison_section(rows: &[stat_signal::PeakCountComparison], suffix: &str) -> ReportSection {
    let mut t = TextTable::new(["Peaks", "Parameters", "Residual SS", "AICc", "ΔAICc", "F vs one fewer", "p-value"]);
    for r in rows {
        match &r.fit {
            Ok(f) => t.push([
                r.peaks.to_string(),
                f.n_free.to_string(),
                num(f.sse),
                num(f.aicc),
                format_number(r.delta_aicc, 4),
                if r.f.is_finite() { num(r.f) } else { String::new() },
                if r.p_value.is_finite() { format_p_value(r.p_value) } else { String::new() },
            ]),
            Err(e) => t.push([
                r.peaks.to_string(),
                String::new(),
                format!("failed: {e}"),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
            ]),
        }
    }
    ReportSection {
        kind: SectionKind::SpectrumComparison,
        title: format!(
            "Numbers of Peaks{suffix} (most prominent first; lowest AICc preferred; extra sum of squares F test)"
        ),
        content: SectionContent::Table(t),
    }
}

/// A computed signal-processing report.
#[derive(Debug, Clone)]
pub enum SignalReport {
    /// Smoothing.
    Smoothing(SignalSmoothingAnalysis),
    /// Derivatives and integrals.
    Calculus(CalculusAnalysis),
    /// Spectrum peaks.
    Spectrum(Box<SpectrumAnalysis>),
}

impl SignalReport {
    /// Run `request`.
    ///
    /// # Errors
    /// Whatever the analysis reports.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &SignalRequest) -> Result<Self> {
        Ok(match request {
            SignalRequest::Smoothing(r) => Self::Smoothing(SignalSmoothingAnalysis::run(table, states, r)?),
            SignalRequest::Calculus(r) => Self::Calculus(CalculusAnalysis::run(table, states, r)?),
            SignalRequest::Spectrum(r) => Self::Spectrum(Box::new(SpectrumAnalysis::run(table, states, r)?)),
        })
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        match self {
            Self::Smoothing(a) => a.title(),
            Self::Calculus(a) => a.title(),
            Self::Spectrum(a) => a.title(),
        }
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        match self {
            Self::Smoothing(a) => a.sections(),
            Self::Calculus(a) => a.sections(),
            Self::Spectrum(a) => a.sections(),
        }
    }

    /// Columns the report can add to its table.
    #[must_use]
    pub fn saveable_columns(&self) -> Vec<ReportColumn> {
        match self {
            Self::Smoothing(a) => a.saveable_columns(),
            Self::Calculus(a) => a.saveable_columns(),
            Self::Spectrum(a) => a.saveable_columns(),
        }
    }
}

#[cfg(test)]
mod tests;
