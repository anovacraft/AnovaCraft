//! Functional data analysis bound to a dataset ([`stat_functional`]): curves
//! from long data (a curve identifier, a time and a value column), smoothed
//! in a basis, optionally registered (aligned in time), summarized by
//! functional principal components, and the component scores modelled on
//! curve-level factors so the predicted *curve* can be profiled as the
//! factors change.
//!
//! **Rows.** Excluded rows and rows missing the curve, time or value are
//! left out. A curve needs at least three observations; curves with fewer
//! are skipped and named in the summary. The curves' common range is from
//! the earliest to the latest time used.
//!
//! **Basis and smoothing.** B-splines (cubic by default) or a Fourier series
//! with `n` functions (automatic: enough to follow the data, see
//! [`FunctionalRequest::basis_size`]), and a roughness penalty on the second
//! derivative (or harmonic acceleration for a Fourier basis) with one
//! smoothing parameter for all curves, chosen by GCV, fixed, or none
//! (plain least squares in the basis: a regression spline).
//!
//! **Registration.** Landmarks (each curve's maximum or minimum inside a
//! window) moved to their mean times by monotone warping; then, optionally,
//! continuous registration of every curve to the mean curve. Principal
//! components and the score model use the registered curves.
//!
//! **Score model.** Factors are columns constant within each curve. The
//! scores of the retained components are fitted jointly by a multivariate
//! linear model ([`stat_linear::manova`]) with main effects (and, if asked,
//! two-factor interactions); each effect gets Wilks' test across the
//! components and a univariate F-test per component. The predicted curve at
//! factor settings `x` is `x̄(t) + Σₖ ŝₖ(x) ξₖ(t)` with pointwise confidence
//! limits from `Var = x₀ᵀ(XᵀX)⁻¹x₀ · ξ(t)ᵀ(E/ν)ξ(t)` on `ν` degrees of
//! freedom, `E` the error SSCP matrix of the scores.

use crate::format::{format_number, format_p_value};
use crate::project::{invalid as json_invalid, s};
use crate::report::{
    EffectPareto, PlotSpec, PointGroups, Polyline, ReportSection, RowPoint, SectionContent, SectionKind, TextTable,
};
use crate::report_columns::ReportColumn;
use crate::surface_profiler::{Limited, Observation, ProfileFactor, ProfileFactorKind, SurfaceModel};
use stat_core::distributions::{ContinuousDistribution, StudentsT};
use stat_core::{Matrix, Result, StatError};
use stat_data::{DataTable, JsonValue, MeasurementLevel, RowStateManager};
use stat_functional::{
    BSplineBasis, Basis, ContinuousOptions, CurveData, FourierBasis, Fpca, FunctionalData, LandmarkKind, LandmarkSpec,
    Roughness, SmoothOptions, SmoothResult, Smoothing, Warp, apply_warps, find_landmarks, functional_pca,
    landmark_warps, register_continuous, smooth_curves,
};
use stat_linear::manova::{ManovaFit, MultivariateStatistic, fit_manova};
use stat_linear::{ModelDesign, Predictor, PredictorValues, TermKind, TermValue, natural_order};

const DIGITS: usize = 5;
/// Points at which curves are drawn.
const PLOT_POINTS: usize = 201;
/// Up to this many curves are drawn in their own colors with a key;
/// more are drawn as thin lines.
const KEYED_CURVES: usize = 10;
/// Most components chosen automatically.
const MAX_AUTO_COMPONENTS: usize = 6;
/// Share of variation the automatic number of components explains.
const AUTO_SHARE: f64 = 0.9;

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

fn section(kind: SectionKind, title: impl Into<String>, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.into(), content }
}

fn num(v: f64) -> String {
    format_number(v, DIGITS)
}

/// The basis system curves are expressed in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FunctionalBasis {
    /// B-splines with equally spaced knots.
    BSpline {
        /// Number of functions (`None`: automatic).
        functions: Option<usize>,
        /// Order (4: cubic).
        order: usize,
    },
    /// Fourier series (for periodic curves).
    Fourier {
        /// Number of functions, odd (`None`: automatic).
        functions: Option<usize>,
        /// Period (`None`: the length of the time range).
        period: Option<f64>,
    },
}

impl Default for FunctionalBasis {
    fn default() -> Self {
        Self::BSpline { functions: None, order: 4 }
    }
}

/// How much the curves are smoothed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FunctionalSmoothing {
    /// Roughness penalty chosen by generalized cross-validation.
    Gcv,
    /// This smoothing parameter λ.
    Fixed(f64),
    /// No penalty: least squares in the basis.
    None,
}

/// A landmark searched for in each curve.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FunctionalLandmark {
    /// `true` for the curve's maximum, `false` for its minimum.
    pub maximum: bool,
    /// Window `[from, to]` in time units (`None`: the whole range).
    pub window: Option<(f64, f64)>,
}

/// Columns and settings of a functional data analysis.
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionalRequest {
    /// Curve identifier column (any kind).
    pub curve: usize,
    /// Time (argument) column, numeric.
    pub time: usize,
    /// Measured value column, numeric.
    pub value: usize,
    /// Curve-level factors for modelling the component scores.
    pub factors: Vec<usize>,
    /// Basis system.
    pub basis: FunctionalBasis,
    /// Roughness penalty.
    pub smoothing: FunctionalSmoothing,
    /// Landmarks (empty: no landmark registration).
    pub landmarks: Vec<FunctionalLandmark>,
    /// Continuous registration to the mean curve after any landmarks.
    pub continuous: bool,
    /// Basis functions of the continuous warping functions.
    pub warp_functions: usize,
    /// Roughness penalty of the continuous warping functions.
    pub warp_penalty: f64,
    /// Components retained (`None`: enough to explain 90% of the variation,
    /// at most six).
    pub components: Option<usize>,
    /// Two-factor interactions in the score model.
    pub interactions: bool,
    /// Confidence level of the predicted-curve limits.
    pub confidence: f64,
}

impl FunctionalRequest {
    /// Default settings: cubic B-splines, GCV, no registration, automatic
    /// components, main effects.
    #[must_use]
    pub fn new(curve: usize, time: usize, value: usize) -> Self {
        Self {
            curve,
            time,
            value,
            factors: Vec::new(),
            basis: FunctionalBasis::default(),
            smoothing: FunctionalSmoothing::Gcv,
            landmarks: Vec::new(),
            continuous: false,
            warp_functions: ContinuousOptions::default().n_basis,
            warp_penalty: ContinuousOptions::default().penalty,
            components: None,
            interactions: false,
            confidence: 0.95,
        }
    }

    /// Columns read.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        let mut c = vec![self.curve, self.time, self.value];
        c.extend(&self.factors);
        c
    }

    /// The same request with column indices passed through `f`.
    #[must_use]
    pub fn map_columns(mut self, f: impl Fn(usize) -> usize) -> Self {
        self.curve = f(self.curve);
        self.time = f(self.time);
        self.value = f(self.value);
        self.factors.iter_mut().for_each(|c| *c = f(*c));
        self
    }

    /// Number of basis functions used for curves observed at
    /// `distinct_times` distinct times overall and at least `fewest` times
    /// each: as given, or automatically one function per distinct time
    /// (plus two for cubic splines) up to 40 when penalized, and at most
    /// the fewest observations of a curve (up to 10) when not.
    #[must_use]
    pub fn basis_size(&self, distinct_times: usize, fewest: usize) -> usize {
        let penalized = self.smoothing != FunctionalSmoothing::None;
        match self.basis {
            FunctionalBasis::BSpline { functions: Some(n), .. }
            | FunctionalBasis::Fourier { functions: Some(n), .. } => n,
            FunctionalBasis::BSpline { functions: None, order } => {
                if penalized {
                    (distinct_times + order - 2).clamp(order, 40)
                } else {
                    fewest.min(10).max(order)
                }
            }
            FunctionalBasis::Fourier { functions: None, .. } => {
                let n = if penalized { distinct_times.min(25) } else { fewest.min(9) };
                // Odd, at least three.
                (if n % 2 == 0 { n.saturating_sub(1) } else { n }).max(3)
            }
        }
    }
}

/// How a factor's values are held, one per curve.
#[derive(Debug, Clone)]
enum FactorValues {
    Continuous(Vec<f64>),
    Categorical(Vec<String>),
}

/// The fitted score model.
#[derive(Debug, Clone)]
struct ScoreModel {
    design: ModelDesign,
    fit: ManovaFit,
    /// Components modelled (the first `k`).
    k: usize,
    /// Curves in the model.
    curves: Vec<usize>,
    defaults: Vec<TermValue>,
    factors: Vec<ProfileFactor>,
}

/// A functional data analysis.
#[derive(Debug, Clone)]
pub struct FunctionalAnalysis {
    request: FunctionalRequest,
    names: (String, String, String),
    factor_names: Vec<String>,
    /// Curve labels, in natural order.
    labels: Vec<String>,
    /// Dataset rows of each curve's observations.
    rows: Vec<Vec<usize>>,
    times: Vec<Vec<f64>>,
    values: Vec<Vec<f64>>,
    skipped: Vec<String>,
    range: (f64, f64),
    smooth: SmoothResult,
    landmarks: Option<(Vec<Vec<f64>>, Vec<f64>)>,
    continuous: Option<stat_functional::ContinuousRegistration>,
    warps: Option<Vec<Warp>>,
    /// The curves analysed: registered if registration was asked for.
    curves: FunctionalData,
    fpca: Fpca,
    auto_components: bool,
    factor_values: Vec<FactorValues>,
    model: Option<std::result::Result<ScoreModel, String>>,
    n_table_rows: usize,
}

fn column_name(table: &DataTable, c: usize) -> Result<String> {
    table.column(c).map(|c| c.name.clone()).ok_or_else(|| invalid(format!("no column {c}")))
}

impl FunctionalAnalysis {
    /// Smooth, register and analyse the curves.
    ///
    /// # Errors
    /// Missing or non-numeric time or value columns, fewer than two curves
    /// with three observations, an invalid basis, smoothing, landmark or
    /// component setting, landmarks out of order in a curve, or a factor
    /// that varies within a curve.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &FunctionalRequest) -> Result<Self> {
        let names =
            (column_name(table, request.curve)?, column_name(table, request.time)?, column_name(table, request.value)?);
        let numeric = |c: usize, role: &str| -> Result<Vec<f64>> {
            let col = table.column(c).ok_or_else(|| invalid(format!("no column {c}")))?;
            col.as_numeric()
                .map(<[f64]>::to_vec)
                .ok_or_else(|| invalid(format!("the {role} {:?} must be numeric", col.name)))
        };
        let t_all = numeric(request.time, "time column")?;
        let y_all = numeric(request.value, "value column")?;
        let id_col = table.column(request.curve).ok_or_else(|| invalid("no curve column"))?;
        if request.factors.iter().any(|f| [request.curve, request.time, request.value].contains(f)) {
            return Err(invalid("a factor cannot also be the curve, time or value column"));
        }
        let mut labels: Vec<String> = Vec::new();
        let mut by_label: std::collections::HashMap<String, Vec<usize>> = std::collections::HashMap::new();
        for r in 0..table.n_rows() {
            if states.is_excluded(r) || !t_all[r].is_finite() || !y_all[r].is_finite() {
                continue;
            }
            let label = id_col.level_text(r);
            if label.trim().is_empty() {
                continue;
            }
            by_label.entry(label.clone()).or_insert_with(|| {
                labels.push(label);
                Vec::new()
            });
        }
        for r in 0..table.n_rows() {
            if states.is_excluded(r) || !t_all[r].is_finite() || !y_all[r].is_finite() {
                continue;
            }
            if let Some(v) = by_label.get_mut(&id_col.level_text(r)) {
                v.push(r);
            }
        }
        labels.sort_by(|a, b| natural_order(a, b));
        let mut skipped = Vec::new();
        let mut kept_labels = Vec::new();
        let mut rows = Vec::new();
        for l in labels {
            let mut rs = by_label.remove(&l).unwrap_or_default();
            if rs.len() < 3 {
                skipped.push(l);
                continue;
            }
            rs.sort_by(|&a, &b| t_all[a].total_cmp(&t_all[b]).then(a.cmp(&b)));
            kept_labels.push(l);
            rows.push(rs);
        }
        if rows.len() < 2 {
            return Err(invalid(format!(
                "functional data analysis needs at least two curves with three or more observations each; {:?} gives {}",
                names.0,
                rows.len()
            )));
        }
        let times: Vec<Vec<f64>> = rows.iter().map(|rs| rs.iter().map(|&r| t_all[r]).collect()).collect();
        let values: Vec<Vec<f64>> = rows.iter().map(|rs| rs.iter().map(|&r| y_all[r]).collect()).collect();
        let lo = times.iter().flatten().copied().fold(f64::INFINITY, f64::min);
        let hi = times.iter().flatten().copied().fold(f64::NEG_INFINITY, f64::max);
        if hi <= lo {
            return Err(invalid("the times must span an interval"));
        }
        let range = (lo, hi);
        let mut distinct: Vec<f64> = times.iter().flatten().copied().collect();
        distinct.sort_by(f64::total_cmp);
        distinct.dedup();
        let fewest = times
            .iter()
            .map(|t| {
                let mut d = t.clone();
                d.dedup();
                d.len()
            })
            .min()
            .unwrap_or(0);
        let k = request.basis_size(distinct.len(), fewest);
        let (basis, roughness) = match request.basis {
            FunctionalBasis::BSpline { order, .. } => {
                if !(1..=6).contains(&order) {
                    return Err(invalid("the B-spline order must be between 1 and 6"));
                }
                let deriv = if order >= 3 { 2 } else { order - 1 };
                (Basis::BSpline(BSplineBasis::equally_spaced(range, k, order)?), Roughness::Derivative(deriv))
            }
            FunctionalBasis::Fourier { period, .. } => {
                let period = period.unwrap_or(hi - lo);
                (Basis::Fourier(FourierBasis::new(range, k, period)?), Roughness::HarmonicAcceleration { period })
            }
        };
        if roughness == Roughness::Derivative(0) && request.smoothing != FunctionalSmoothing::None {
            return Err(invalid("piecewise-constant curves cannot carry a roughness penalty; choose no smoothing"));
        }
        let smoothing = match request.smoothing {
            FunctionalSmoothing::Gcv => Smoothing::Gcv,
            FunctionalSmoothing::Fixed(l) => Smoothing::Fixed(l),
            FunctionalSmoothing::None => Smoothing::None,
        };
        let curves_in: Vec<CurveData> =
            times.iter().zip(&values).map(|(t, y)| CurveData { t: t.clone(), y: y.clone() }).collect();
        let smooth = smooth_curves(&basis, &curves_in, &SmoothOptions { operator: roughness.operator(), smoothing })?;

        // Registration.
        let mut current = smooth.data.clone();
        let mut warps: Option<Vec<Warp>> = None;
        let mut landmarks = None;
        if !request.landmarks.is_empty() {
            let specs: Vec<LandmarkSpec> = request
                .landmarks
                .iter()
                .map(|l| LandmarkSpec {
                    kind: if l.maximum { LandmarkKind::Maximum } else { LandmarkKind::Minimum },
                    window: l.window.unwrap_or(range),
                })
                .collect();
            let found = find_landmarks(&current, &specs)?;
            for (label, lm) in kept_labels.iter().zip(&found) {
                let mut pts = vec![lo];
                pts.extend(lm);
                pts.push(hi);
                if pts.windows(2).any(|w| w[1] <= w[0]) {
                    return Err(invalid(format!(
                        "curve {label}: its landmarks ({}) are not in increasing order strictly inside the time range; \
                         narrow the landmark windows",
                        lm.iter().map(|v| num(*v)).collect::<Vec<_>>().join(", ")
                    )));
                }
            }
            let (w, location) = landmark_warps(range, &found, None)?;
            current = apply_warps(&smooth.data, &w)?;
            warps = Some(w);
            landmarks = Some((found, location));
        }
        let mut continuous = None;
        if request.continuous {
            let options = ContinuousOptions {
                n_basis: request.warp_functions,
                penalty: request.warp_penalty,
                ..ContinuousOptions::default()
            };
            let reg = register_continuous(&current, &options)?;
            let total: Vec<Warp> = match &warps {
                Some(outer) => outer.iter().zip(&reg.warps).map(|(o, i)| o.compose(i)).collect(),
                None => reg.warps.clone(),
            };
            current = apply_warps(&smooth.data, &total)?;
            warps = Some(total);
            continuous = Some(reg);
        }

        // Principal components.
        let n = current.n_curves();
        let most = (n - 1).min(k);
        let (fpca, auto_components) = match request.components {
            Some(c) => {
                if c == 0 || c > most {
                    return Err(invalid(format!("choose between 1 and {most} components for {n} curves")));
                }
                (functional_pca(&current, c)?, false)
            }
            None => {
                let all = functional_pca(&current, most)?;
                let total = all.total_variance();
                let mut c = 1;
                let mut share = all.proportion(0);
                while c < most.min(MAX_AUTO_COMPONENTS) && share < AUTO_SHARE && total > 0.0 {
                    share += all.proportion(c);
                    c += 1;
                }
                let mut p = all;
                p.harmonics.truncate(c);
                p.scores.iter_mut().for_each(|s| s.truncate(c));
                (p, true)
            }
        };

        // Curve-level factors.
        let mut factor_names = Vec::new();
        let mut factor_values = Vec::new();
        for &f in &request.factors {
            let col = table.column(f).ok_or_else(|| invalid(format!("no column {f}")))?;
            factor_names.push(col.name.clone());
            let continuous = col.level == MeasurementLevel::Continuous && col.as_numeric().is_some();
            let mut texts = Vec::with_capacity(rows.len());
            let mut nums = Vec::with_capacity(rows.len());
            for (label, rs) in kept_labels.iter().zip(&rows) {
                let mut seen: Option<String> = None;
                for &r in rs {
                    let text = col.level_text(r);
                    if text.trim().is_empty() {
                        continue;
                    }
                    match &seen {
                        None => seen = Some(text),
                        Some(prev) if *prev != text => {
                            return Err(invalid(format!(
                                "the factor {:?} must be constant within each curve; curve {label} has {prev} and {text}",
                                col.name
                            )));
                        }
                        Some(_) => {}
                    }
                }
                let first = rs.iter().find(|&&r| !col.level_text(r).trim().is_empty());
                nums.push(first.and_then(|&r| col.as_numeric().map(|v| v[r])).unwrap_or(f64::NAN));
                texts.push(seen.unwrap_or_default());
            }
            factor_values.push(if continuous {
                FactorValues::Continuous(nums)
            } else {
                FactorValues::Categorical(texts)
            });
        }
        let model = (!request.factors.is_empty()).then(|| {
            fit_score_model(&factor_names, &factor_values, &fpca, request.interactions).map_err(|e| e.to_string())
        });
        Ok(Self {
            request: request.clone(),
            names,
            factor_names,
            labels: kept_labels,
            rows,
            times,
            values,
            skipped,
            range,
            smooth,
            landmarks,
            continuous,
            warps,
            curves: current,
            fpca,
            auto_components,
            factor_values,
            model,
            n_table_rows: table.n_rows(),
        })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &FunctionalRequest {
        &self.request
    }

    /// The smoothed (unregistered) curves.
    #[must_use]
    pub fn smoothed(&self) -> &SmoothResult {
        &self.smooth
    }

    /// The analysed curves (registered when asked for).
    #[must_use]
    pub fn curves(&self) -> &FunctionalData {
        &self.curves
    }

    /// The principal components.
    #[must_use]
    pub fn fpca(&self) -> &Fpca {
        &self.fpca
    }

    /// Curve labels, in the order of the curves.
    #[must_use]
    pub fn labels(&self) -> &[String] {
        &self.labels
    }

    /// The warping functions (when the curves were registered).
    #[must_use]
    pub fn warps(&self) -> Option<&[Warp]> {
        self.warps.as_deref()
    }

    /// `true` when the score model was fitted and can be profiled.
    #[must_use]
    pub fn has_model(&self) -> bool {
        matches!(self.model, Some(Ok(_)))
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        format!(
            "Functional Data: {} by {} ({} curves of {}{})",
            self.names.2,
            self.names.1,
            self.labels.len(),
            self.names.0,
            if self.warps.is_some() { ", registered" } else { "" }
        )
    }

    fn grid(&self) -> Vec<f64> {
        let (a, b) = self.range;
        (0..PLOT_POINTS).map(|i| a + (b - a) * i as f64 / (PLOT_POINTS - 1) as f64).collect()
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let mut out = vec![self.summary_section(), self.curves_plot()];
        if !self.smooth.gcv_path.is_empty() {
            out.push(self.gcv_plot());
        }
        if self.warps.is_some() {
            if let Some(s) = self.landmark_table() {
                out.push(s);
            }
            out.push(self.registered_plot());
            out.push(self.warp_plot());
        }
        out.push(self.mean_plot());
        out.push(self.eigenvalue_table());
        out.push(self.eigenfunction_plot());
        for k in 0..self.fpca.n_components() {
            out.push(self.component_plot(k));
        }
        out.push(self.score_plot());
        out.push(self.score_table());
        out.push(self.reconstruction_plot());
        out.push(self.fit_table());
        match &self.model {
            Some(Ok(m)) => {
                out.extend(self.model_sections(m));
            }
            Some(Err(e)) => out.push(section(
                SectionKind::FunctionalScoreModel,
                "Score Model",
                SectionContent::Note(format!("The component scores could not be modelled on the factors: {e}.")),
            )),
            None => {}
        }
        out
    }

    fn summary_section(&self) -> ReportSection {
        let mut t = TextTable::new(["Statistic", "Value"]);
        let mut row = |a: &str, b: String| t.push([a.to_owned(), b]);
        row("Curves", self.labels.len().to_string());
        row("Observations", self.rows.iter().map(Vec::len).sum::<usize>().to_string());
        if !self.skipped.is_empty() {
            row("Curves skipped (fewer than 3 observations)", self.skipped.join(", "));
        }
        row(&format!("Range of {}", self.names.1), format!("{} to {}", num(self.range.0), num(self.range.1)));
        row("Basis", self.smooth.data.basis.describe());
        let penalty = match self.request.basis {
            FunctionalBasis::Fourier { .. } => "harmonic acceleration",
            FunctionalBasis::BSpline { order: 1 | 2, .. } => "first derivative",
            FunctionalBasis::BSpline { .. } => "second derivative",
        };
        row(
            "Smoothing",
            match self.request.smoothing {
                FunctionalSmoothing::Gcv => format!("{penalty} penalty, λ chosen by GCV"),
                FunctionalSmoothing::Fixed(_) => format!("{penalty} penalty, λ fixed"),
                FunctionalSmoothing::None => "none (least squares in the basis)".to_owned(),
            },
        );
        if self.request.smoothing != FunctionalSmoothing::None {
            row("λ", format_number(self.smooth.lambda, 4));
            row("log₁₀ λ", format_number(self.smooth.lambda.log10(), 4));
        }
        let mean_df = self.smooth.df.iter().sum::<f64>() / self.smooth.df.len() as f64;
        row("Degrees of freedom per curve (mean)", format_number(mean_df, 4));
        row("GCV (summed over curves)", num(self.smooth.total_gcv()));
        let n_obs: usize = self.rows.iter().map(Vec::len).sum();
        let df_total: f64 = self.smooth.df.iter().sum();
        let sse: f64 = self.smooth.sse.iter().sum();
        if n_obs as f64 > df_total {
            row("Residual standard deviation", num((sse / (n_obs as f64 - df_total)).sqrt()));
        }
        let mut reg = Vec::new();
        if let Some((_, location)) = &self.landmarks {
            reg.push(format!(
                "landmarks at mean times {}",
                location.iter().map(|v| num(*v)).collect::<Vec<_>>().join(", ")
            ));
        }
        if let Some(c) = &self.continuous {
            reg.push(format!(
                "continuous (minimum eigenvalue, {} warping functions, penalty {}, {} pass{})",
                self.request.warp_functions,
                format_number(self.request.warp_penalty, 3),
                c.passes,
                if c.passes == 1 { "" } else { "es" }
            ));
        }
        row("Registration", if reg.is_empty() { "none".to_owned() } else { reg.join("; then ") });
        let k = self.fpca.n_components();
        let explained: f64 = (0..k).map(|j| self.fpca.proportion(j)).sum();
        row(
            "Components retained",
            format!(
                "{k}{} explaining {}%",
                if self.auto_components { " (automatic)" } else { "" },
                format_number(100.0 * explained, 4)
            ),
        );
        section(SectionKind::FunctionalSummary, "Functional Data Summary", SectionContent::Table(t))
    }

    /// Curves drawn either keyed by color (few) or as thin lines (many).
    fn curve_lines(
        &self,
        title: &str,
        lines: Vec<Polyline>,
        points: Vec<RowPoint>,
        membership: Vec<usize>,
    ) -> PlotSpec {
        let mut spec = PlotSpec {
            x_label: self.names.1.clone(),
            y_label: self.names.2.clone(),
            points,
            point_name: Some("Observations".to_owned()),
            ..PlotSpec::default()
        };
        if lines.len() <= KEYED_CURVES {
            spec.groups = Some(PointGroups { title: title.to_owned(), labels: self.labels.clone(), membership, lines });
        } else {
            spec.segments = lines;
        }
        spec
    }

    fn observation_points(&self, warped: bool) -> (Vec<RowPoint>, Vec<usize>) {
        let mut points = Vec::new();
        let mut membership = Vec::new();
        for (i, rs) in self.rows.iter().enumerate() {
            for (j, &r) in rs.iter().enumerate() {
                let t = self.times[i][j];
                let x = match (&self.warps, warped) {
                    (Some(w), true) => w[i].inverse(t),
                    _ => t,
                };
                points.push(RowPoint { row: r, x, y: self.values[i][j] });
                membership.push(i);
            }
        }
        (points, membership)
    }

    fn curves_plot(&self) -> ReportSection {
        let grid = self.grid();
        let values = self.smooth.data.eval_all(&grid, 0);
        let lines = values.iter().map(|v| grid.iter().zip(v).map(|(t, y)| [*t, *y]).collect()).collect();
        let (points, membership) = self.observation_points(false);
        let spec = self.curve_lines(&self.names.0, lines, points, membership);
        section(SectionKind::FunctionalCurves, "Smoothed Curves", SectionContent::Plot(Box::new(spec)))
    }

    fn gcv_plot(&self) -> ReportSection {
        // GCV grows by orders of magnitude at large λ, so plot its logarithm.
        let path: Polyline =
            self.smooth.gcv_path.iter().filter(|p| p.1.is_finite() && p.1 > 0.0).map(|p| [p.0, p.1.log10()]).collect();
        let lambda = self.smooth.lambda.log10();
        let best = self.smooth.total_gcv().log10();
        let spec = PlotSpec {
            x_label: "log₁₀ λ".to_owned(),
            y_label: "log₁₀ GCV".to_owned(),
            line: Some(path),
            line_name: Some("log₁₀ GCV".to_owned()),
            annotations: Some(crate::report::Annotations {
                groups: vec!["λ chosen".to_owned()],
                items: vec![(lambda, best, String::new(), 0)],
                labels: Vec::new(),
            }),
            ..PlotSpec::default()
        };
        section(SectionKind::FunctionalGcv, "Smoothing Parameter (GCV)", SectionContent::Plot(Box::new(spec)))
    }

    fn landmark_table(&self) -> Option<ReportSection> {
        let (found, location) = self.landmarks.as_ref()?;
        let mut headings = vec![self.names.0.clone()];
        for (j, l) in self.request.landmarks.iter().enumerate() {
            headings.push(format!("Landmark {} ({})", j + 1, if l.maximum { "maximum" } else { "minimum" }));
        }
        let mut t = TextTable::new(headings);
        for (label, lm) in self.labels.iter().zip(found) {
            let mut row = vec![label.clone()];
            row.extend(lm.iter().map(|v| num(*v)));
            t.push(row);
        }
        let mut mean = vec!["Mean (registered time)".to_owned()];
        mean.extend(location.iter().map(|v| num(*v)));
        t.push(mean);
        Some(section(SectionKind::FunctionalLandmarks, "Landmarks", SectionContent::Table(t)))
    }

    fn registered_plot(&self) -> ReportSection {
        let grid = self.grid();
        let values = self.curves.eval_all(&grid, 0);
        let lines = values.iter().map(|v| grid.iter().zip(v).map(|(t, y)| [*t, *y]).collect()).collect();
        let (points, membership) = self.observation_points(true);
        let mut spec = self.curve_lines(&self.names.0, lines, points, membership);
        spec.x_label = format!("Registered {}", self.names.1);
        section(SectionKind::FunctionalRegistered, "Registered Curves", SectionContent::Plot(Box::new(spec)))
    }

    fn warp_plot(&self) -> ReportSection {
        let warps = self.warps.as_deref().unwrap_or_default();
        let lines: Vec<Polyline> =
            warps.iter().map(|w| w.grid.iter().zip(&w.values).step_by(2).map(|(t, h)| [*t, *h]).collect()).collect();
        let mut spec = self.curve_lines(&self.names.0, lines, Vec::new(), Vec::new());
        spec.x_label = format!("Registered {}", self.names.1);
        spec.y_label = format!("{} of the curve", self.names.1);
        spec.line = Some(vec![[self.range.0, self.range.0], [self.range.1, self.range.1]]);
        spec.line_name = Some("No warping".to_owned());
        section(SectionKind::FunctionalWarps, "Warping Functions", SectionContent::Plot(Box::new(spec)))
    }

    fn mean_plot(&self) -> ReportSection {
        let grid = self.grid();
        let mean = self.curves.eval_coefs(&self.fpca.mean, &grid, 0);
        let sd = self.curves.std_dev(&grid);
        let values = self.curves.eval_all(&grid, 0);
        let spec = PlotSpec {
            x_label: self.axis_time(),
            y_label: self.names.2.clone(),
            line: Some(grid.iter().zip(&mean).map(|(t, m)| [*t, *m]).collect()),
            line_name: Some("Mean curve".to_owned()),
            band: Some((
                grid.iter().zip(&mean).zip(&sd).map(|((t, m), s)| [*t, m - 2.0 * s]).collect(),
                grid.iter().zip(&mean).zip(&sd).map(|((t, m), s)| [*t, m + 2.0 * s]).collect(),
            )),
            band_name: Some("Mean ± 2 SD".to_owned()),
            segments: values.iter().map(|v| grid.iter().zip(v).map(|(t, y)| [*t, *y]).collect()).collect(),
            ..PlotSpec::default()
        };
        section(SectionKind::FunctionalMean, "Mean Curve", SectionContent::Plot(Box::new(spec)))
    }

    fn axis_time(&self) -> String {
        if self.warps.is_some() { format!("Registered {}", self.names.1) } else { self.names.1.clone() }
    }

    fn eigenvalue_table(&self) -> ReportSection {
        let mut t = TextTable::new(["Component", "Eigenvalue", "Percent", "Cumulative percent"]);
        let shown = self.fpca.eigenvalues.len().min(self.labels.len() - 1).min(10.max(self.fpca.n_components()));
        let mut cum = 0.0;
        for k in 0..shown {
            let p = 100.0 * self.fpca.proportion(k);
            cum += p;
            let name = if k < self.fpca.n_components() {
                format!("FPC {} (retained)", k + 1)
            } else {
                format!("FPC {}", k + 1)
            };
            t.push([name, num(self.fpca.eigenvalues[k].max(0.0)), format_number(p, 4), format_number(cum, 4)]);
        }
        section(SectionKind::FunctionalEigenvalues, "Functional Principal Components", SectionContent::Table(t))
    }

    fn eigenfunction_plot(&self) -> ReportSection {
        let grid = self.grid();
        let lines: Vec<Polyline> = self
            .fpca
            .harmonics
            .iter()
            .map(|h| grid.iter().zip(self.curves.eval_coefs(h, &grid, 0)).map(|(t, v)| [*t, v]).collect())
            .collect();
        let labels = (1..=lines.len()).map(|k| format!("FPC {k}")).collect();
        // Number each eigenfunction at its largest value, so the curves are
        // told apart without colour.
        let numbers = lines
            .iter()
            .enumerate()
            .filter_map(|(k, l)| {
                let p = l.iter().max_by(|a, b| a[1].abs().total_cmp(&b[1].abs()))?;
                Some((p[0], p[1], (k + 1).to_string()))
            })
            .collect();
        let spec = PlotSpec {
            x_label: self.axis_time(),
            y_label: "Eigenfunction".to_owned(),
            annotations: Some(crate::report::Annotations { groups: Vec::new(), items: Vec::new(), labels: numbers }),
            groups: Some(PointGroups { title: "Component".to_owned(), labels, membership: Vec::new(), lines }),
            line: Some(vec![[self.range.0, 0.0], [self.range.1, 0.0]]),
            line_name: Some("Zero".to_owned()),
            ..PlotSpec::default()
        };
        section(SectionKind::FunctionalEigenfunctions, "Eigenfunctions", SectionContent::Plot(Box::new(spec)))
    }

    /// The mean ± 2√ρₖ ξₖ: how the k-th component changes the curve.
    fn component_plot(&self, k: usize) -> ReportSection {
        let grid = self.grid();
        let mean = self.curves.eval_coefs(&self.fpca.mean, &grid, 0);
        let xi = self.curves.eval_coefs(&self.fpca.harmonics[k], &grid, 0);
        let c = 2.0 * self.fpca.eigenvalues[k].max(0.0).sqrt();
        let plus: Polyline = grid.iter().zip(&mean).zip(&xi).map(|((t, m), x)| [*t, m + c * x]).collect();
        let minus: Polyline = grid.iter().zip(&mean).zip(&xi).map(|((t, m), x)| [*t, m - c * x]).collect();
        // "+" and "−" along the two curves, so they read without colour.
        let step = (grid.len() / 12).max(1);
        let signs = (step / 2..grid.len())
            .step_by(step)
            .flat_map(|i| [(plus[i][0], plus[i][1], "+".to_owned()), (minus[i][0], minus[i][1], "−".to_owned())])
            .collect();
        let spec = PlotSpec {
            annotations: Some(crate::report::Annotations { groups: Vec::new(), items: Vec::new(), labels: signs }),
            x_label: self.axis_time(),
            y_label: self.names.2.clone(),
            line: Some(grid.iter().zip(&mean).map(|(t, m)| [*t, *m]).collect()),
            line_name: Some("Mean curve".to_owned()),
            groups: Some(PointGroups {
                title: "Score".to_owned(),
                labels: vec!["Mean + 2 SD of score".to_owned(), "Mean − 2 SD of score".to_owned()],
                membership: Vec::new(),
                lines: vec![plus, minus],
            }),
            ..PlotSpec::default()
        };
        section(
            SectionKind::FunctionalComponent(k),
            format!("FPC {} Effect ({}% of variation)", k + 1, format_number(100.0 * self.fpca.proportion(k), 3)),
            SectionContent::Plot(Box::new(spec)),
        )
    }

    fn score_plot(&self) -> ReportSection {
        let two = self.fpca.n_components() >= 2;
        let points: Vec<RowPoint> = self
            .rows
            .iter()
            .zip(&self.fpca.scores)
            .enumerate()
            .map(|(i, (rs, s))| RowPoint {
                row: rs[0],
                x: if two { s[0] } else { (i + 1) as f64 },
                y: if two { s[1] } else { s[0] },
            })
            .collect();
        // Colour by the first categorical factor, if any.
        let groups = self.factor_values.iter().zip(&self.factor_names).find_map(|(v, name)| match v {
            FactorValues::Categorical(labels) => {
                let mut levels: Vec<String> = labels.iter().filter(|l| !l.is_empty()).cloned().collect();
                levels.sort_by(|a, b| natural_order(a, b));
                levels.dedup();
                if levels.is_empty() || levels.len() > 12 || labels.iter().any(String::is_empty) {
                    return None;
                }
                let membership = labels.iter().map(|l| levels.iter().position(|x| x == l).unwrap_or(0)).collect();
                Some(PointGroups { title: name.clone(), labels: levels, membership, lines: Vec::new() })
            }
            FactorValues::Continuous(_) => None,
        });
        let spec = PlotSpec {
            x_label: if two { "FPC 1 score".to_owned() } else { self.names.0.clone() },
            y_label: if two { "FPC 2 score".to_owned() } else { "FPC 1 score".to_owned() },
            points,
            point_name: Some("Curves (first row)".to_owned()),
            groups,
            ..PlotSpec::default()
        };
        section(SectionKind::FunctionalScorePlot, "Component Scores", SectionContent::Plot(Box::new(spec)))
    }

    fn score_table(&self) -> ReportSection {
        let mut headings = vec![self.names.0.clone()];
        headings.extend(self.factor_names.iter().cloned());
        headings.extend((1..=self.fpca.n_components()).map(|k| format!("FPC {k}")));
        let mut t = TextTable::new(headings);
        for (i, label) in self.labels.iter().enumerate() {
            let mut row = vec![label.clone()];
            for v in &self.factor_values {
                row.push(match v {
                    FactorValues::Continuous(x) => num(x[i]),
                    FactorValues::Categorical(x) => x[i].clone(),
                });
            }
            row.extend(self.fpca.scores[i].iter().map(|s| num(*s)));
            t.push(row);
        }
        section(SectionKind::FunctionalScores, "Scores by Curve", SectionContent::Table(t))
    }

    fn reconstruction_plot(&self) -> ReportSection {
        let grid = self.grid();
        let k = self.fpca.n_components();
        let lines: Vec<Polyline> = (0..self.labels.len())
            .map(|i| {
                let c = self.fpca.reconstruct(i, k);
                grid.iter().zip(self.curves.eval_coefs(&c, &grid, 0)).map(|(t, v)| [*t, v]).collect()
            })
            .collect();
        let (points, membership) = self.observation_points(true);
        let mut spec = self.curve_lines(&self.names.0, lines, points, membership);
        spec.x_label = self.axis_time();
        section(
            SectionKind::FunctionalReconstruction,
            format!("Reconstruction from {k} Component{}", if k == 1 { "" } else { "s" }),
            SectionContent::Plot(Box::new(spec)),
        )
    }

    fn fit_table(&self) -> ReportSection {
        let k = self.fpca.n_components();
        let gram = self.curves.basis.gram();
        let (a, b) = self.range;
        let mut t = TextTable::new([
            self.names.0.clone(),
            "Observations".to_owned(),
            "Smoothing df".to_owned(),
            "Residual SD".to_owned(),
            format!("Reconstruction RMS ({k} components)"),
        ]);
        for i in 0..self.labels.len() {
            let n = self.rows[i].len() as f64;
            let df = self.smooth.df[i];
            let sd = if n > df { (self.smooth.sse[i] / (n - df)).sqrt() } else { f64::NAN };
            let rec = self.fpca.reconstruct(i, k);
            let diff: Vec<f64> = self.curves.coefs[i].iter().zip(&rec).map(|(x, r)| x - r).collect();
            let rms = (self.curves.squared_norm(&diff, &gram).max(0.0) / (b - a)).sqrt();
            t.push([self.labels[i].clone(), self.rows[i].len().to_string(), format_number(df, 4), num(sd), num(rms)]);
        }
        section(SectionKind::FunctionalCurveFit, "Fit by Curve", SectionContent::Table(t))
    }

    fn model_sections(&self, m: &ScoreModel) -> Vec<ReportSection> {
        let mut headings =
            vec!["Effect".to_owned(), "DF".to_owned(), "Wilks' Λ".to_owned(), "F".to_owned(), "p-value".to_owned()];
        headings.extend((1..=m.k).map(|k| format!("p (FPC {k})")));
        let mut t = TextTable::new(headings);
        let mut pareto = Vec::new();
        for e in m.fit.effect_tests() {
            let w = e.test(MultivariateStatistic::Wilks);
            let p = w.p_value.unwrap_or(f64::NAN);
            pareto.push((e.name.clone(), p));
            let mut row = vec![
                e.name.clone(),
                e.df.to_string(),
                format_number(w.value, 4),
                w.f.map_or("—".to_owned(), |f| format_number(f, 4)),
                format_p_value(p),
            ];
            row.extend(e.univariate.iter().map(|u| format_p_value(u.p_value.unwrap_or(f64::NAN))));
            t.push(row);
        }
        let mut fits = TextTable::new(["Component", "R²", "RMSE", "Curves"]);
        let e = m.fit.error_sscp();
        let nu = m.fit.df_error() as f64;
        for k in 0..m.k {
            let scores: Vec<f64> = m.curves.iter().map(|&i| self.fpca.scores[i][k]).collect();
            let mean = scores.iter().sum::<f64>() / scores.len() as f64;
            let sst: f64 = scores.iter().map(|s| (s - mean).powi(2)).sum();
            let r2 = if sst > 0.0 { 1.0 - e[k][k] / sst } else { f64::NAN };
            fits.push([
                format!("FPC {}", k + 1),
                format_number(r2, 4),
                num((e[k][k] / nu).sqrt()),
                m.curves.len().to_string(),
            ]);
        }
        let mut out = vec![
            section(
                SectionKind::FunctionalScoreModel,
                format!(
                    "Score Model: FPC 1{} ~ {}",
                    if m.k > 1 { format!("–{}", m.k) } else { String::new() },
                    m.fit.effect_tests().iter().map(|e| e.name.as_str()).collect::<Vec<_>>().join(" + ")
                ),
                SectionContent::Table(t),
            ),
            section(SectionKind::FunctionalScoreFits, "Score Fits", SectionContent::Table(fits)),
        ];
        if m.k < self.fpca.n_components() {
            out.push(section(
                SectionKind::FunctionalScoreFits,
                "Score Model Note",
                SectionContent::Note(format!(
                    "Only the first {} components are modelled: the model leaves {} error degrees of freedom.",
                    m.k,
                    m.fit.df_error()
                )),
            ));
        }
        out.extend(EffectPareto::new(pareto, "Wilks' Λ over the component scores").section());
        out
    }

    /// Columns the report can save: each row's curve scores, smoothed value
    /// and (if registered) registered time.
    #[must_use]
    pub fn saveable_columns(&self) -> Vec<ReportColumn> {
        let n = self.n_table_rows;
        let mut out = Vec::new();
        for k in 0..self.fpca.n_components() {
            let mut v = vec![f64::NAN; n];
            for (i, rs) in self.rows.iter().enumerate() {
                for &r in rs {
                    v[r] = self.fpca.scores[i][k];
                }
            }
            out.push(ReportColumn::continuous(
                &format!("fpc{}", k + 1),
                &format!("Save FPC {} scores", k + 1),
                format!("FPC {} {}", k + 1, self.names.2),
                v,
            ));
        }
        let mut smooth = vec![f64::NAN; n];
        for (i, rs) in self.rows.iter().enumerate() {
            for (j, &r) in rs.iter().enumerate() {
                smooth[r] = self.smooth.fitted[i][j];
            }
        }
        out.push(ReportColumn::continuous(
            "smoothed",
            "Save smoothed values",
            format!("Smoothed {}", self.names.2),
            smooth,
        ));
        if let Some(w) = &self.warps {
            let mut reg = vec![f64::NAN; n];
            for (i, rs) in self.rows.iter().enumerate() {
                for (j, &r) in rs.iter().enumerate() {
                    reg[r] = w[i].inverse(self.times[i][j]);
                }
            }
            out.push(ReportColumn::continuous(
                "registered_time",
                "Save registered times",
                format!("Registered {}", self.names.1),
                reg,
            ));
        }
        out
    }

    fn model(&self) -> Option<&ScoreModel> {
        match &self.model {
            Some(Ok(m)) => Some(m),
            _ => None,
        }
    }

    /// The predicted curve value and its standard error at `settings`
    /// (factors, then time).
    fn predict_one(&self, m: &ScoreModel, settings: &[TermValue]) -> Result<(f64, f64)> {
        let nf = m.factors.len();
        if settings.len() != nf + 1 {
            return Err(invalid("the settings do not match the factors"));
        }
        let TermValue::Number(t) = settings[nf] else { return Err(invalid("time must be a number")) };
        let mut x0 = vec![1.0];
        x0.extend(m.design.encode(&settings[..nf])?);
        let coefs = m.fit.coefficients();
        let s_hat: Vec<f64> = (0..m.k).map(|k| x0.iter().zip(coefs).map(|(x, c)| x * c[k]).sum()).collect();
        let phi = self.curves.basis.evaluate(t, 0);
        let mean: f64 = self.fpca.mean.iter().zip(&phi).map(|(c, p)| c * p).sum();
        let xi: Vec<f64> =
            (0..m.k).map(|k| self.fpca.harmonics[k].iter().zip(&phi).map(|(c, p)| c * p).sum()).collect();
        let value = mean + s_hat.iter().zip(&xi).map(|(s, x)| s * x).sum::<f64>();
        let g = m.fit.inverse_gram();
        let h: f64 = (0..x0.len()).map(|a| (0..x0.len()).map(|b| x0[a] * g[(a, b)] * x0[b]).sum::<f64>()).sum();
        let e = m.fit.error_sscp();
        let nu = m.fit.df_error() as f64;
        let q: f64 = (0..m.k).map(|a| (0..m.k).map(|b| xi[a] * e[a][b] * xi[b]).sum::<f64>()).sum::<f64>() / nu;
        Ok((value, (h * q).max(0.0).sqrt()))
    }
}

fn fit_score_model(names: &[String], values: &[FactorValues], fpca: &Fpca, interactions: bool) -> Result<ScoreModel> {
    let preds: Vec<Predictor<'_>> = names
        .iter()
        .zip(values)
        .map(|(name, v)| Predictor {
            name,
            values: match v {
                FactorValues::Continuous(x) => PredictorValues::Continuous(x),
                FactorValues::Categorical(x) => PredictorValues::Categorical(x),
            },
        })
        .collect();
    let n = fpca.scores.len();
    let curves: Vec<usize> = (0..n)
        .filter(|&i| {
            values.iter().all(|v| match v {
                FactorValues::Continuous(x) => x[i].is_finite(),
                FactorValues::Categorical(x) => !x[i].trim().is_empty(),
            })
        })
        .collect();
    let mut effects: Vec<Vec<(usize, u8)>> = (0..names.len()).map(|i| vec![(i, 1)]).collect();
    if interactions {
        for i in 0..names.len() {
            for j in i + 1..names.len() {
                effects.push(vec![(i, 1), (j, 1)]);
            }
        }
    }
    let design = ModelDesign::with_effects(&preds, &curves, &effects)?;
    let x: Matrix = design.matrix(&preds, &curves)?;
    let p = x.ncols() + 1;
    if curves.len() <= p {
        return Err(invalid(format!("{} curves are too few for a model with {p} parameters", curves.len())));
    }
    let k = fpca.n_components().min(curves.len() - p);
    let y: Vec<Vec<f64>> = (0..k).map(|c| curves.iter().map(|&i| fpca.scores[i][c]).collect()).collect();
    let named: Vec<(String, std::ops::Range<usize>)> =
        design.effects().iter().map(|e| (e.name.clone(), e.columns.clone())).collect();
    let fit = fit_manova(&x, &y, &named)?;
    let factors: Vec<ProfileFactor> = design
        .terms()
        .iter()
        .zip(values)
        .map(|(term, v)| ProfileFactor {
            name: term.name.clone(),
            kind: match (&term.kind, v) {
                (TermKind::Categorical { levels }, _) => ProfileFactorKind::Categorical { levels: levels.clone() },
                (_, FactorValues::Continuous(x)) => {
                    let lo = curves.iter().map(|&i| x[i]).fold(f64::INFINITY, f64::min);
                    let hi = curves.iter().map(|&i| x[i]).fold(f64::NEG_INFINITY, f64::max);
                    ProfileFactorKind::Continuous { range: if lo < hi { (lo, hi) } else { (lo - 1.0, lo + 1.0) } }
                }
                (_, FactorValues::Categorical(_)) => ProfileFactorKind::Continuous { range: (0.0, 1.0) },
            },
        })
        .collect();
    let defaults = design
        .terms()
        .iter()
        .zip(values)
        .map(|(term, v)| match (&term.kind, v) {
            (TermKind::Categorical { levels }, FactorValues::Categorical(labels)) => {
                let counts: Vec<usize> =
                    levels.iter().map(|l| curves.iter().filter(|&&i| labels[i] == *l).count()).collect();
                let best = counts.iter().copied().max().unwrap_or(0);
                TermValue::Level(counts.iter().position(|&c| c == best).unwrap_or(0))
            }
            (TermKind::Categorical { .. }, _) => TermValue::Level(0),
            (_, FactorValues::Continuous(x)) => {
                TermValue::Number(curves.iter().map(|&i| x[i]).sum::<f64>() / curves.len() as f64)
            }
            (_, FactorValues::Categorical(_)) => TermValue::Number(0.0),
        })
        .collect();
    Ok(ScoreModel { design, fit, k, curves, defaults, factors })
}

impl SurfaceModel for FunctionalAnalysis {
    fn response_name(&self) -> &str {
        &self.names.2
    }

    fn profile_factors(&self) -> Vec<ProfileFactor> {
        let mut f = self.model().map(|m| m.factors.clone()).unwrap_or_default();
        f.push(ProfileFactor { name: self.axis_time(), kind: ProfileFactorKind::Continuous { range: self.range } });
        f
    }

    fn default_profile_settings(&self) -> Vec<TermValue> {
        let mut s = self.model().map(|m| m.defaults.clone()).unwrap_or_default();
        s.push(TermValue::Number(0.5 * (self.range.0 + self.range.1)));
        s
    }

    fn predict_settings(&self, points: &[Vec<TermValue>]) -> Result<Vec<f64>> {
        Ok(self.predict_with_limits(points)?.into_iter().map(|p| p.0).collect())
    }

    fn predict_with_limits(&self, points: &[Vec<TermValue>]) -> Result<Vec<Limited>> {
        let m = self.model().ok_or_else(|| invalid("there is no score model to profile"))?;
        let nu = m.fit.df_error() as f64;
        let q = StudentsT::new(nu)?.quantile(0.5 + 0.5 * self.request.confidence)?;
        points
            .iter()
            .map(|s| {
                let (v, se) = self.predict_one(m, s)?;
                Ok((v, Some((v - q * se, v + q * se))))
            })
            .collect()
    }

    fn confidence_level(&self) -> Option<f64> {
        Some(self.request.confidence)
    }

    fn profile_note(&self) -> Option<String> {
        self.model().map(|m| {
            format!(
                "The predicted curve is the mean curve plus the predicted scores of {} component{} times their \
                 eigenfunctions; drag {} to move along the curve.",
                m.k,
                if m.k == 1 { "" } else { "s" },
                self.axis_time()
            )
        })
    }

    fn observations(&self) -> Vec<Observation> {
        let Some(m) = self.model() else { return Vec::new() };
        let mut out = Vec::new();
        for &i in &m.curves {
            let settings: Vec<TermValue> = m
                .design
                .terms()
                .iter()
                .zip(&self.factor_values)
                .map(|(term, v)| match (&term.kind, v) {
                    (TermKind::Categorical { levels }, FactorValues::Categorical(l)) => {
                        TermValue::Level(levels.iter().position(|x| *x == l[i]).unwrap_or(0))
                    }
                    (_, FactorValues::Continuous(x)) => TermValue::Number(x[i]),
                    _ => TermValue::Number(f64::NAN),
                })
                .collect();
            for (j, &r) in self.rows[i].iter().enumerate() {
                let t = match &self.warps {
                    Some(w) => w[i].inverse(self.times[i][j]),
                    None => self.times[i][j],
                };
                let mut s = settings.clone();
                s.push(TermValue::Number(t));
                out.push(Observation { row: r, settings: s, response: self.values[i][j] });
            }
        }
        out
    }
}

// ---- Project files and scripts ------------------------------------------------

impl FunctionalRequest {
    /// The request as JSON members after `"analysis": "functional-data"`
    /// (columns by name).
    pub(crate) fn to_json(&self, table: &DataTable, profilers: &crate::ProfilerOptions) -> JsonValue {
        let n = JsonValue::number;
        let col = |c: usize| table.column(c).map_or(JsonValue::Null, |c| s(&c.name));
        let mut m: Vec<(String, JsonValue)> = vec![
            ("analysis".into(), s("functional-data")),
            ("curve".into(), col(self.curve)),
            ("time".into(), col(self.time)),
            ("value".into(), col(self.value)),
            ("factors".into(), JsonValue::Array(self.factors.iter().map(|&c| col(c)).collect())),
        ];
        match self.basis {
            FunctionalBasis::BSpline { functions, order } => {
                m.push(("basis".into(), s("b-spline")));
                m.push(("order".into(), n(order as f64)));
                if let Some(f) = functions {
                    m.push(("functions".into(), n(f as f64)));
                }
            }
            FunctionalBasis::Fourier { functions, period } => {
                m.push(("basis".into(), s("fourier")));
                if let Some(f) = functions {
                    m.push(("functions".into(), n(f as f64)));
                }
                if let Some(p) = period {
                    m.push(("period".into(), n(p)));
                }
            }
        }
        match self.smoothing {
            FunctionalSmoothing::Gcv => m.push(("smoothing".into(), s("gcv"))),
            FunctionalSmoothing::None => m.push(("smoothing".into(), s("none"))),
            FunctionalSmoothing::Fixed(l) => {
                m.push(("smoothing".into(), s("fixed")));
                m.push(("lambda".into(), n(l)));
            }
        }
        if !self.landmarks.is_empty() {
            let items = self
                .landmarks
                .iter()
                .map(|l| {
                    let mut o = vec![("feature".to_owned(), s(if l.maximum { "maximum" } else { "minimum" }))];
                    if let Some((a, b)) = l.window {
                        o.push(("from".to_owned(), n(a)));
                        o.push(("to".to_owned(), n(b)));
                    }
                    JsonValue::Object(o)
                })
                .collect();
            m.push(("landmarks".into(), JsonValue::Array(items)));
        }
        if self.continuous {
            m.push(("continuous_registration".into(), JsonValue::Bool(true)));
            m.push(("warp_functions".into(), n(self.warp_functions as f64)));
            m.push(("warp_penalty".into(), n(self.warp_penalty)));
        }
        if let Some(c) = self.components {
            m.push(("components".into(), n(c as f64)));
        }
        if self.interactions {
            m.push(("interactions".into(), JsonValue::Bool(true)));
        }
        m.push(("confidence".into(), n(self.confidence)));
        m.push(("show_profiler".into(), JsonValue::Bool(profilers.show_profiler)));
        m.push(("show_surface_profiler".into(), JsonValue::Bool(profilers.show_surface_profiler)));
        m.push(("show_observed".into(), JsonValue::Bool(profilers.show_observed)));
        JsonValue::Object(m)
    }

    /// Read the JSON form (see [`to_json`](Self::to_json)).
    pub(crate) fn from_json(v: &JsonValue, table: &DataTable) -> Result<(Self, crate::ProfilerOptions)> {
        let find = |c: &JsonValue| -> Result<usize> {
            let name = c.as_str().ok_or_else(|| json_invalid("functional data: expected a column name"))?;
            table.find(name).ok_or_else(|| json_invalid(format!("report refers to a missing column {name:?}")))
        };
        let col = |key: &str| -> Result<usize> {
            find(v.get(key).ok_or_else(|| json_invalid(format!("functional data: missing {key:?}")))?)
        };
        let number = |key: &str| v.get(key).and_then(JsonValue::as_f64);
        let count = |key: &str| v.get(key).and_then(JsonValue::as_usize);
        let mut r = Self::new(col("curve")?, col("time")?, col("value")?);
        if let Some(list) = v.get("factors").and_then(JsonValue::as_array) {
            r.factors = list.iter().map(find).collect::<Result<_>>()?;
        }
        r.basis = match v.get("basis").and_then(JsonValue::as_str).unwrap_or("b-spline") {
            "b-spline" => {
                FunctionalBasis::BSpline { functions: count("functions"), order: count("order").unwrap_or(4) }
            }
            "fourier" => FunctionalBasis::Fourier { functions: count("functions"), period: number("period") },
            other => return Err(json_invalid(format!("functional data: unknown basis {other:?}"))),
        };
        r.smoothing = match v.get("smoothing").and_then(JsonValue::as_str).unwrap_or("gcv") {
            "gcv" => FunctionalSmoothing::Gcv,
            "none" => FunctionalSmoothing::None,
            "fixed" => FunctionalSmoothing::Fixed(
                number("lambda").ok_or_else(|| json_invalid("functional data: a fixed smoothing needs \"lambda\""))?,
            ),
            other => return Err(json_invalid(format!("functional data: unknown smoothing {other:?}"))),
        };
        if let Some(list) = v.get("landmarks").and_then(JsonValue::as_array) {
            r.landmarks = list
                .iter()
                .map(|l| {
                    let maximum = match l.get("feature").and_then(JsonValue::as_str).unwrap_or("maximum") {
                        "maximum" => true,
                        "minimum" => false,
                        other => return Err(json_invalid(format!("functional data: unknown landmark {other:?}"))),
                    };
                    let window =
                        match (l.get("from").and_then(JsonValue::as_f64), l.get("to").and_then(JsonValue::as_f64)) {
                            (Some(a), Some(b)) => Some((a, b)),
                            _ => None,
                        };
                    Ok(FunctionalLandmark { maximum, window })
                })
                .collect::<Result<_>>()?;
        }
        r.continuous = v.get("continuous_registration").and_then(JsonValue::as_bool).unwrap_or(false);
        if let Some(f) = count("warp_functions") {
            r.warp_functions = f;
        }
        if let Some(p) = number("warp_penalty") {
            r.warp_penalty = p;
        }
        r.components = count("components");
        r.interactions = v.get("interactions").and_then(JsonValue::as_bool).unwrap_or(false);
        if let Some(c) = number("confidence") {
            r.confidence = c;
        }
        let d = crate::ProfilerOptions::default();
        let flag = |key: &str, default: bool| v.get(key).and_then(JsonValue::as_bool).unwrap_or(default);
        let profilers = crate::ProfilerOptions {
            show_profiler: flag("show_profiler", d.show_profiler),
            show_surface_profiler: flag("show_surface_profiler", d.show_surface_profiler),
            show_observed: flag("show_observed", d.show_observed),
        };
        Ok((r, profilers))
    }
}

#[cfg(test)]
mod tests;
