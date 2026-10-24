//! Fitting a sum of peaks plus a baseline (deconvolution of overlapping
//! peaks) by nonlinear least squares.
//!
//! # Model
//! ```text
//! f(x) = Σₖ Pₖ(x; hₖ, cₖ, widthsₖ) + b(x),   b(x) = 0, a, or a + b·x,
//! ```
//!
//! with every peak of one [`PeakShape`]. Parameters may be shared: a
//! **common width** gives every peak the same width parameter(s), and the
//! centres may be **equally spaced** (`cₖ = c₁ + (k − 1)d`, `d` estimated) or
//! held at a **fixed spacing** `d`. Peaks are ordered by their starting
//! centres. Heights are kept non-negative, centres inside the data's x range
//! (or a window around their starts), widths between a millionth of the
//! range and twice the range, and the pseudo-Voigt's η in [0, 1].
//!
//! The sum of squares is minimised by Levenberg–Marquardt
//! ([`stat_core::optimize::least_squares`]) from the starting values (usually
//! from [`crate::find_peaks`]). With `k` free parameters and `n` points, the
//! residual variance is `s² = SSE/(n − k)` and the estimates' covariance
//! `s²(JᵀJ)⁻¹` (`J` the Jacobian at the solution; Bates & Watts, *Nonlinear
//! Regression Analysis and Its Applications*, Wiley 1988, §2.3). Shared and
//! spaced parameters are linear in the free ones, so each peak's height,
//! centre and widths get standard errors through that linear map, and its
//! area and FWHM by the delta method (central-difference gradients).
//!
//! # Comparing numbers of peaks
//! [`compare_peak_counts`] fits 1, 2, … peaks (the first `m` starts, most
//! prominent first) and reports for each the corrected Akaike criterion
//! `AICc = n ln(SSE/n) + 2K + 2K(K + 1)/(n − K − 1)` with `K = k + 1`
//! (counting σ; Hurvich & Tsai, *Biometrika* 76, 1989) and the extra sum of
//! squares F test against one peak fewer,
//! `F = ((SSE₀ − SSE₁)/(k₁ − k₀)) / (SSE₁/(n − k₁))` on `(k₁ − k₀, n − k₁)`
//! degrees of freedom (approximate for nonlinear models; Bates & Watts §3.10).

use stat_core::distributions::{ContinuousDistribution, FisherSnedecor};
use stat_core::matrix::Matrix;
use stat_core::optimize::least_squares::{
    BoundState, LeastSquaresOptions, LeastSquaresProblem, Termination, levenberg_marquardt,
};
use stat_core::qr::QrDecomposition;

use crate::profiles::PeakShape;
use crate::{Result, invalid};

/// Baseline term added to the peaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaselineTerm {
    /// None (data already baseline-corrected).
    None,
    /// A constant offset.
    Constant,
    /// A straight line `a + b·x`.
    Linear,
}

impl BaselineTerm {
    /// Every choice.
    pub const ALL: [Self; 3] = [Self::None, Self::Constant, Self::Linear];

    /// Number of parameters.
    #[must_use]
    pub fn n_params(self) -> usize {
        match self {
            Self::None => 0,
            Self::Constant => 1,
            Self::Linear => 2,
        }
    }

    /// Name for dialogs and reports.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Constant => "Constant",
            Self::Linear => "Straight line",
        }
    }

    /// Identifier in project files and scripts.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Constant => "constant",
            Self::Linear => "linear",
        }
    }

    /// The term with identifier `key`.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.key() == key)
    }
}

/// How peak centres relate to each other.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Spacing {
    /// Every centre estimated on its own.
    Free,
    /// Equally spaced, the spacing estimated.
    Equal,
    /// Spaced by this fixed distance.
    Fixed(f64),
}

/// Starting values of one peak.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PeakStart {
    /// Centre.
    pub centre: f64,
    /// Height above the baseline.
    pub height: f64,
    /// Full width at half maximum.
    pub fwhm: f64,
}

/// What to fit.
#[derive(Debug, Clone, PartialEq)]
pub struct PeakFitSpec {
    /// Peak shape.
    pub shape: PeakShape,
    /// Baseline term.
    pub baseline: BaselineTerm,
    /// One start per peak.
    pub starts: Vec<PeakStart>,
    /// All peaks share their width parameter(s).
    pub common_width: bool,
    /// Relation between centres.
    pub spacing: Spacing,
    /// Each centre stays within this distance of its start (`None`: anywhere
    /// in the data's x range).
    pub centre_window: Option<f64>,
    /// Iteration limit.
    pub max_iterations: usize,
}

impl PeakFitSpec {
    /// Fit `starts` with `shape`, a straight-line baseline and no sharing.
    #[must_use]
    pub fn new(shape: PeakShape, starts: Vec<PeakStart>) -> Self {
        Self {
            shape,
            baseline: BaselineTerm::Linear,
            starts,
            common_width: false,
            spacing: Spacing::Free,
            centre_window: None,
            max_iterations: 500,
        }
    }
}

/// One fitted peak's summary values with standard errors.
#[derive(Debug, Clone, PartialEq)]
pub struct PeakEstimate {
    /// The peak's parameters (shape order: height, centre, widths…).
    pub params: Vec<f64>,
    /// Their standard errors (`NaN` where undefined).
    pub param_se: Vec<f64>,
    /// Centre and its standard error.
    pub centre: (f64, f64),
    /// Height and its standard error.
    pub height: (f64, f64),
    /// FWHM and its standard error.
    pub fwhm: (f64, f64),
    /// Area and its standard error.
    pub area: (f64, f64),
}

/// The result of [`fit_peaks`].
#[derive(Debug, Clone, PartialEq)]
pub struct PeakFit {
    /// What was fitted (starts sorted by centre).
    pub spec: PeakFitSpec,
    /// The peaks, in order of starting centre.
    pub peaks: Vec<PeakEstimate>,
    /// Baseline parameters (`[a]` or `[a, b]`) and standard errors.
    pub baseline: Vec<(f64, f64)>,
    /// Points used.
    pub n: usize,
    /// Free parameters estimated.
    pub n_free: usize,
    /// Residual sum of squares.
    pub sse: f64,
    /// Residual standard deviation `√(SSE/(n − k))`.
    pub sigma: f64,
    /// R² against the mean.
    pub r_squared: f64,
    /// Corrected Akaike information criterion.
    pub aicc: f64,
    /// Bayesian information criterion `n ln(SSE/n) + K ln n`.
    pub bic: f64,
    /// Fitted values at the data's x.
    pub fitted: Vec<f64>,
    /// Why the iterations stopped.
    pub termination: Termination,
    /// Iterations used.
    pub iterations: usize,
    /// Names of free parameters that ended on a bound.
    pub at_bounds: Vec<String>,
    full: Vec<f64>,
}

impl PeakFit {
    /// The fitted model at `x`.
    #[must_use]
    pub fn eval(&self, x: f64) -> f64 {
        eval_full(&self.spec, &self.full, x)
    }

    /// Peak `k` alone at `x` (without the baseline).
    #[must_use]
    pub fn component(&self, k: usize, x: f64) -> f64 {
        let np = self.spec.shape.n_params();
        self.spec.shape.eval(x, &self.full[k * np..(k + 1) * np])
    }

    /// The baseline term at `x`.
    #[must_use]
    pub fn baseline_at(&self, x: f64) -> f64 {
        let np = self.spec.shape.n_params();
        baseline_value(self.spec.baseline, &self.full[self.spec.starts.len() * np..], x)
    }

    /// Whether the iterations converged.
    #[must_use]
    pub fn converged(&self) -> bool {
        self.termination.converged()
    }
}

fn baseline_value(term: BaselineTerm, b: &[f64], x: f64) -> f64 {
    match term {
        BaselineTerm::None => 0.0,
        BaselineTerm::Constant => b[0],
        BaselineTerm::Linear => b[0] + b[1] * x,
    }
}

fn eval_full(spec: &PeakFitSpec, full: &[f64], x: f64) -> f64 {
    let np = spec.shape.n_params();
    let k = spec.starts.len();
    let peaks: f64 = (0..k).map(|j| spec.shape.eval(x, &full[j * np..(j + 1) * np])).sum();
    peaks + baseline_value(spec.baseline, &full[k * np..], x)
}

/// The linear map from free parameters to the full parameter vector:
/// `full = M·free + offset`, with names, starts and bounds of the free ones.
struct Layout {
    /// `m[i]` lists `(free index, coefficient)` for full parameter `i`.
    m: Vec<Vec<(usize, f64)>>,
    offset: Vec<f64>,
    names: Vec<String>,
    start: Vec<f64>,
    lower: Vec<f64>,
    upper: Vec<f64>,
}

impl Layout {
    fn full(&self, free: &[f64]) -> Vec<f64> {
        self.m.iter().zip(&self.offset).map(|(row, o)| o + row.iter().map(|&(j, c)| c * free[j]).sum::<f64>()).collect()
    }

    fn add(&mut self, name: String, start: f64, lower: f64, upper: f64) -> usize {
        self.names.push(name);
        self.start.push(start.clamp(lower, upper));
        self.lower.push(lower);
        self.upper.push(upper);
        self.names.len() - 1
    }
}

fn layout(spec: &PeakFitSpec, x_lo: f64, x_hi: f64) -> Layout {
    let shape = spec.shape;
    let np = shape.n_params();
    let k = spec.starts.len();
    let range = (x_hi - x_lo).max(f64::MIN_POSITIVE);
    let (w_lo, w_hi) = (1e-6 * range, 2.0 * range);
    let n_full = k * np + spec.baseline.n_params();
    let mut l = Layout {
        m: vec![Vec::new(); n_full],
        offset: vec![0.0; n_full],
        names: Vec::new(),
        start: Vec::new(),
        lower: Vec::new(),
        upper: Vec::new(),
    };
    let names = shape.param_names();
    let label = |j: usize, what: &str| format!("Peak {} {}", j + 1, what.to_lowercase());
    // Heights.
    for (j, s) in spec.starts.iter().enumerate() {
        let f = l.add(label(j, names[0]), s.height.max(0.0), 0.0, f64::INFINITY);
        l.m[j * np].push((f, 1.0));
    }
    // Centres.
    let centre_bounds = |s: &PeakStart| match spec.centre_window {
        Some(w) => ((s.centre - w).max(x_lo), (s.centre + w).min(x_hi)),
        None => (x_lo, x_hi),
    };
    match spec.spacing {
        Spacing::Free => {
            for (j, s) in spec.starts.iter().enumerate() {
                let (lo, hi) = centre_bounds(s);
                let f = l.add(label(j, names[1]), s.centre, lo, hi);
                l.m[j * np + 1].push((f, 1.0));
            }
        }
        Spacing::Equal | Spacing::Fixed(_) => {
            let first = spec.starts[0];
            let (lo, hi) = centre_bounds(&first);
            let c1 = l.add(label(0, names[1]), first.centre, lo, hi);
            let spacing = match spec.spacing {
                Spacing::Equal => {
                    let d0 = if k > 1 { (spec.starts[k - 1].centre - first.centre) / (k - 1) as f64 } else { 0.0 };
                    Some(l.add("Peak spacing".to_owned(), d0, -range, range))
                }
                _ => None,
            };
            for j in 0..k {
                let row = &mut l.m[j * np + 1];
                row.push((c1, 1.0));
                match (spec.spacing, spacing) {
                    (_, Some(d)) if j > 0 => row.push((d, j as f64)),
                    (Spacing::Fixed(d), _) => l.offset[j * np + 1] = d * j as f64,
                    _ => {}
                }
            }
        }
    }
    // Widths (FWHM-type parameters).
    for &w in shape.width_params() {
        // A Voigt's width starts split between its Gaussian and Lorentzian parts.
        let share = if shape == PeakShape::Voigt { 0.5 } else { 1.0 };
        if spec.common_width {
            let mean = spec.starts.iter().map(|s| s.fwhm).sum::<f64>() / k as f64;
            let f = l.add(format!("Common {}", names[w].to_lowercase()), share * mean, w_lo, w_hi);
            for j in 0..k {
                l.m[j * np + w].push((f, 1.0));
            }
        } else {
            for (j, s) in spec.starts.iter().enumerate() {
                let f = l.add(label(j, names[w]), share * s.fwhm, w_lo, w_hi);
                l.m[j * np + w].push((f, 1.0));
            }
        }
    }
    // The pseudo-Voigt's Lorentzian share.
    if shape == PeakShape::PseudoVoigt {
        for j in 0..k {
            let f = l.add(label(j, names[3]), 0.5, 0.0, 1.0);
            l.m[j * np + 3].push((f, 1.0));
        }
    }
    // Baseline.
    let base = k * np;
    match spec.baseline {
        BaselineTerm::None => {}
        BaselineTerm::Constant => {
            let f = l.add("Baseline level".to_owned(), 0.0, f64::NEG_INFINITY, f64::INFINITY);
            l.m[base].push((f, 1.0));
        }
        BaselineTerm::Linear => {
            let f = l.add("Baseline intercept".to_owned(), 0.0, f64::NEG_INFINITY, f64::INFINITY);
            l.m[base].push((f, 1.0));
            let g = l.add("Baseline slope".to_owned(), 0.0, f64::NEG_INFINITY, f64::INFINITY);
            l.m[base + 1].push((g, 1.0));
        }
    }
    l
}

fn derived_se(f: impl Fn(&[f64]) -> f64, p: &[f64], cov: &[Vec<f64>]) -> f64 {
    let np = p.len();
    let mut g = vec![0.0; np];
    let mut q = p.to_vec();
    for i in 0..np {
        let h = f64::EPSILON.cbrt() * p[i].abs().max(1e-8);
        q[i] = p[i] + h;
        let up = f(&q);
        q[i] = p[i] - h;
        let down = f(&q);
        q[i] = p[i];
        g[i] = (up - down) / (2.0 * h);
    }
    let mut v = 0.0;
    for i in 0..np {
        for j in 0..np {
            v += g[i] * cov[i][j] * g[j];
        }
    }
    if v >= 0.0 { v.sqrt() } else { f64::NAN }
}

/// Fit `spec` to the points `(x, y)` (see the module documentation).
///
/// # Errors
/// [`crate::StatError::InvalidArgument`] without peaks, with missing values,
/// lengths that differ, or invalid starts (non-positive widths, a fixed
/// spacing that is not finite); [`crate::StatError::InsufficientData`] with
/// no more points than free parameters; whatever the least-squares solver
/// reports.
pub fn fit_peaks(x: &[f64], y: &[f64], spec: &PeakFitSpec) -> Result<PeakFit> {
    if spec.starts.is_empty() {
        return Err(invalid("the model needs at least one peak"));
    }
    if x.len() != y.len() {
        return Err(invalid(format!("x has {} values and y has {}", x.len(), y.len())));
    }
    if x.iter().chain(y).any(|v| !v.is_finite()) {
        return Err(invalid("x and y must not contain missing or infinite values"));
    }
    if spec.starts.iter().any(|s| !(s.fwhm.is_finite() && s.fwhm > 0.0 && s.centre.is_finite() && s.height.is_finite())) {
        return Err(invalid("every starting peak needs a finite centre and height and a positive width"));
    }
    if let Spacing::Fixed(d) = spec.spacing
        && !d.is_finite()
    {
        return Err(invalid("the fixed spacing must be a number"));
    }
    let mut spec = spec.clone();
    spec.starts.sort_by(|a, b| a.centre.total_cmp(&b.centre));
    let n = x.len();
    let (x_lo, x_hi) = x.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &v| (a.min(v), b.max(v)));
    let y_lo = y.iter().copied().fold(f64::INFINITY, f64::min);
    let mut lay = layout(&spec, x_lo, x_hi);
    // Baseline starts: the lower envelope as a line through the ends' minima.
    let base_free: Vec<usize> =
        lay.names.iter().enumerate().filter(|(_, nme)| nme.starts_with("Baseline")).map(|(i, _)| i).collect();
    match spec.baseline {
        BaselineTerm::None => {}
        BaselineTerm::Constant => lay.start[base_free[0]] = y_lo,
        BaselineTerm::Linear => {
            let tenth = (n / 10).max(1);
            let mut order: Vec<usize> = (0..n).collect();
            order.sort_by(|&a, &b| x[a].total_cmp(&x[b]));
            let left = order[..tenth].iter().map(|&i| y[i]).fold(f64::INFINITY, f64::min);
            let right = order[n - tenth..].iter().map(|&i| y[i]).fold(f64::INFINITY, f64::min);
            let slope = if x_hi > x_lo { (right - left) / (x_hi - x_lo) } else { 0.0 };
            lay.start[base_free[0]] = left - slope * x_lo;
            lay.start[base_free[1]] = slope;
        }
    }
    let k = lay.start.len();
    if n <= k {
        return Err(crate::StatError::InsufficientData { required: k + 1, found: n });
    }
    let options = LeastSquaresOptions {
        lower: lay.lower.clone(),
        upper: lay.upper.clone(),
        max_iterations: spec.max_iterations,
        ..LeastSquaresOptions::default()
    };
    // Difference steps scale with each parameter's typical size: its start,
    // its value, or the width of its bounded range, so a share at 0 or a
    // baseline near 0 still gets a step well above rounding error.
    let typical: Vec<f64> = (0..k)
        .map(|j| {
            let range = lay.upper[j] - lay.lower[j];
            let range = if range.is_finite() { range } else { 0.0 };
            lay.start[j].abs().max(range).max(1e-8)
        })
        .collect();
    let solve = |start: &[f64], options: &LeastSquaresOptions| {
        let mut problem = PeakProblem {
            n,
            residuals: |theta: &[f64], out: &mut [f64]| {
                let full = lay.full(theta);
                for i in 0..n {
                    out[i] = eval_full(&spec, &full, x[i]) - y[i];
                }
            },
            typical: &typical,
            lower: &options.lower,
            upper: &options.upper,
        };
        levenberg_marquardt(&mut problem, start, options)
    };
    let mut result = solve(&lay.start, &options)?;
    // A parameter that ends on its bound (a pseudo-Voigt share of 0 or 1)
    // can stall the iterations short of the convergence test; finish with
    // those parameters held at their bounds.
    if !result.termination.converged() && result.at_bound.iter().any(|b| *b != BoundState::Free) {
        let mut held = options.clone();
        for (j, b) in result.at_bound.iter().enumerate() {
            if *b != BoundState::Free {
                held.lower[j] = result.theta[j];
                held.upper[j] = result.theta[j];
            }
        }
        if let Ok(again) = solve(&result.theta, &held)
            && again.termination.converged()
        {
            let (at_bound, iterations) = (result.at_bound.clone(), result.iterations);
            result = again;
            result.at_bound = at_bound;
            result.iterations += iterations;
        }
    }
    let full = lay.full(&result.theta);
    let sse = result.sse;
    let df = (n - k) as f64;
    let s2 = sse / df;
    // Covariance of the free parameters.
    let cov_free: Option<Vec<Vec<f64>>> = QrDecomposition::new(result.jacobian.clone())
        .ok()
        .filter(|qr| qr.rank(qr.default_rank_tolerance()) == k)
        .and_then(|qr| qr.inverse_gram().ok())
        .map(|g| (0..k).map(|i| (0..k).map(|j| s2 * g[(i, j)]).collect()).collect());
    let n_full = full.len();
    let cov_full: Vec<Vec<f64>> = match &cov_free {
        Some(c) => (0..n_full)
            .map(|a| {
                (0..n_full)
                    .map(|b| {
                        let mut v = 0.0;
                        for &(i, ci) in &lay.m[a] {
                            for &(j, cj) in &lay.m[b] {
                                v += ci * cj * c[i][j];
                            }
                        }
                        v
                    })
                    .collect()
            })
            .collect(),
        None => vec![vec![f64::NAN; n_full]; n_full],
    };
    let np = spec.shape.n_params();
    let shape = spec.shape;
    let peaks = (0..spec.starts.len())
        .map(|j| {
            let p = full[j * np..(j + 1) * np].to_vec();
            let block: Vec<Vec<f64>> =
                (0..np).map(|a| (0..np).map(|b| cov_full[j * np + a][j * np + b]).collect()).collect();
            let param_se: Vec<f64> = (0..np).map(|a| block[a][a].max(0.0).sqrt()).collect();
            let (fwhm_se, area_se) = if cov_free.is_some() {
                (derived_se(|q| shape.fwhm(q), &p, &block), derived_se(|q| shape.area(q), &p, &block))
            } else {
                (f64::NAN, f64::NAN)
            };
            PeakEstimate {
                centre: (p[1], param_se[1]),
                height: (p[0], param_se[0]),
                fwhm: (shape.fwhm(&p), fwhm_se),
                area: (shape.area(&p), area_se),
                param_se,
                params: p,
            }
        })
        .collect();
    let base = spec.starts.len() * np;
    let baseline = (base..n_full).map(|i| (full[i], cov_full[i][i].max(0.0).sqrt())).collect();
    let fitted: Vec<f64> = x.iter().map(|&v| eval_full(&spec, &full, v)).collect();
    let mean = y.iter().sum::<f64>() / n as f64;
    let sst: f64 = y.iter().map(|v| (v - mean).powi(2)).sum();
    let scale = y.iter().fold(0.0f64, |m, v| m.max(v.abs()));
    let big_k = (k + 1) as f64;
    let nf = n as f64;
    let loglik_term = nf * (sse.max(f64::MIN_POSITIVE) / nf).ln();
    let aicc = if nf - big_k - 1.0 > 0.0 {
        loglik_term + 2.0 * big_k + 2.0 * big_k * (big_k + 1.0) / (nf - big_k - 1.0)
    } else {
        f64::NAN
    };
    let at_bounds = result
        .at_bound
        .iter()
        .enumerate()
        .filter(|(i, b)| **b != BoundState::Free && lay.lower[*i] != lay.upper[*i])
        .map(|(i, _)| lay.names[i].clone())
        .collect();
    Ok(PeakFit {
        peaks,
        baseline,
        n,
        n_free: k,
        sse,
        sigma: s2.sqrt(),
        r_squared: if sst > 0.0 { 1.0 - sse / sst } else { f64::NAN },
        aicc,
        bic: loglik_term + big_k * nf.ln(),
        fitted,
        // Data the model reproduces to rounding error (simulated curves)
        // leave the relative-offset criterion undefined, so no step looks
        // like progress; such a fit has converged.
        termination: if result.termination == Termination::NoProgress && sse <= nf * (1e-12 * scale).powi(2) {
            Termination::ZeroResidual
        } else {
            result.termination
        },
        iterations: result.iterations,
        at_bounds,
        full,
        spec,
    })
}

/// One row of [`compare_peak_counts`].
#[derive(Debug, Clone, PartialEq)]
pub struct PeakCountComparison {
    /// Number of peaks.
    pub peaks: usize,
    /// The fit (or why it failed).
    pub fit: std::result::Result<PeakFit, String>,
    /// `AICc − min AICc` over the successful fits.
    pub delta_aicc: f64,
    /// F statistic against one peak fewer (`NaN` for the first row or after
    /// a failed fit).
    pub f: f64,
    /// Its p-value.
    pub p_value: f64,
}

/// Fit `template` with its first 1, 2, …, `max_peaks` starts (most
/// prominent first) and compare the fits (see the module documentation).
///
/// # Errors
/// [`crate::StatError::InvalidArgument`] when `template` has fewer starts than
/// `max_peaks` or `max_peaks` is 0.
pub fn compare_peak_counts(
    x: &[f64],
    y: &[f64],
    template: &PeakFitSpec,
    max_peaks: usize,
) -> Result<Vec<PeakCountComparison>> {
    if max_peaks == 0 || template.starts.len() < max_peaks {
        return Err(invalid(format!(
            "comparing up to {max_peaks} peaks needs that many starting peaks (have {})",
            template.starts.len()
        )));
    }
    let mut rows: Vec<PeakCountComparison> = (1..=max_peaks)
        .map(|m| {
            let spec = PeakFitSpec { starts: template.starts[..m].to_vec(), ..template.clone() };
            PeakCountComparison {
                peaks: m,
                fit: fit_peaks(x, y, &spec).map_err(|e| e.to_string()),
                delta_aicc: f64::NAN,
                f: f64::NAN,
                p_value: f64::NAN,
            }
        })
        .collect();
    let best = rows.iter().filter_map(|r| r.fit.as_ref().ok()).map(|f| f.aicc).fold(f64::INFINITY, f64::min);
    for i in 0..rows.len() {
        if let Ok(f) = &rows[i].fit {
            rows[i].delta_aicc = f.aicc - best;
        }
        if i == 0 {
            continue;
        }
        if let (Ok(small), Ok(big)) = (&rows[i - 1].fit, &rows[i].fit) {
            let df1 = big.n_free as f64 - small.n_free as f64;
            let df2 = (big.n - big.n_free) as f64;
            if df1 > 0.0 && df2 > 0.0 && big.sse > 0.0 {
                let f = ((small.sse - big.sse) / df1) / (big.sse / df2);
                rows[i].f = f;
                rows[i].p_value = if f > 0.0 {
                    FisherSnedecor::new(df1, df2).and_then(|d| d.sf(f)).unwrap_or(f64::NAN)
                } else {
                    1.0
                };
            }
        }
    }
    Ok(rows)
}

/// The peak-fit residuals with a Jacobian by differences whose steps scale
/// with each parameter's typical size rather than its current value (which
/// can be 0 for a Lorentzian share on its bound), central where the bounds
/// allow and one-sided second-order beside a bound.
struct PeakProblem<'a, F> {
    n: usize,
    residuals: F,
    typical: &'a [f64],
    lower: &'a [f64],
    upper: &'a [f64],
}

impl<F: FnMut(&[f64], &mut [f64])> LeastSquaresProblem for PeakProblem<'_, F> {
    fn n_residuals(&self) -> usize {
        self.n
    }

    fn residuals(&mut self, theta: &[f64], out: &mut [f64]) -> bool {
        (self.residuals)(theta, out);
        out.iter().all(|v| v.is_finite())
    }

    fn jacobian(&mut self, theta: &[f64], out: &mut Matrix) -> Option<bool> {
        let n = self.n;
        let (mut a, mut b) = (vec![0.0; n], vec![0.0; n]);
        let mut t = theta.to_vec();
        for j in 0..theta.len() {
            let (lo, hi) = (self.lower[j], self.upper[j]);
            if lo == hi {
                (0..n).for_each(|i| out[(i, j)] = 0.0);
                continue;
            }
            let h = f64::EPSILON.cbrt() * theta[j].abs().max(self.typical[j]);
            let h = h.min(0.25 * (hi - lo));
            let mut at = |v: f64, buf: &mut [f64]| {
                t[j] = v;
                (self.residuals)(&t, buf);
                buf.iter().all(|r| r.is_finite())
            };
            if theta[j] - h >= lo && theta[j] + h <= hi {
                if !(at(theta[j] + h, &mut a) && at(theta[j] - h, &mut b)) {
                    return Some(false);
                }
                (0..n).for_each(|i| out[(i, j)] = (a[i] - b[i]) / (2.0 * h));
            } else {
                // (−3 f(θ) + 4 f(θ + s) − f(θ + 2s)) / 2s, stepping away from the bound.
                let s = if theta[j] + 2.0 * h <= hi { h } else { -h };
                let mut r0 = vec![0.0; n];
                if !(at(theta[j], &mut r0) && at(theta[j] + s, &mut a) && at(theta[j] + 2.0 * s, &mut b)) {
                    return Some(false);
                }
                (0..n).for_each(|i| out[(i, j)] = (-3.0 * r0[i] + 4.0 * a[i] - b[i]) / (2.0 * s));
            }
            t[j] = theta[j];
        }
        Some(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(shape: PeakShape, truth: &[Vec<f64>]) -> (Vec<f64>, Vec<f64>) {
        let x: Vec<f64> = (0..300).map(|i| 400.0 + i as f64).collect();
        let mut s = 12345u64;
        let y = x
            .iter()
            .map(|&t| {
                s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                let u = (s >> 11) as f64 / (1u64 << 53) as f64 - 0.5;
                truth.iter().map(|p| shape.eval(t, p)).sum::<f64>() + 2.0 + 0.002 * t + 0.05 * u
            })
            .collect();
        (x, y)
    }

    #[test]
    fn recovers_overlapping_peaks() {
        for shape in PeakShape::ALL {
            let truth: Vec<Vec<f64>> = match shape.n_params() {
                3 => vec![vec![10.0, 520.0, 30.0], vec![6.0, 555.0, 25.0]],
                _ => match shape {
                    PeakShape::PseudoVoigt => vec![vec![10.0, 520.0, 30.0, 0.4], vec![6.0, 555.0, 25.0, 0.4]],
                    PeakShape::Voigt => vec![vec![10.0, 520.0, 20.0, 12.0], vec![6.0, 555.0, 15.0, 10.0]],
                    _ => vec![vec![10.0, 520.0, 24.0, 36.0], vec![6.0, 555.0, 20.0, 30.0]],
                },
            };
            let (x, y) = data(shape, &truth);
            let starts = vec![
                PeakStart { centre: 515.0, height: 9.0, fwhm: 25.0 },
                PeakStart { centre: 560.0, height: 5.0, fwhm: 25.0 },
            ];
            let fit = fit_peaks(&x, &y, &PeakFitSpec::new(shape, starts)).unwrap();
            assert!(fit.converged(), "{shape:?}: {:?}", fit.termination);
            for (est, t) in fit.peaks.iter().zip(&truth) {
                assert!((est.centre.0 - t[1]).abs() < 0.5, "{shape:?} centre {} vs {}", est.centre.0, t[1]);
                assert!((est.height.0 - t[0]).abs() < 0.1, "{shape:?} height {} vs {}", est.height.0, t[0]);
                assert!(est.area.1 > 0.0 && est.area.1.is_finite());
            }
            assert!(fit.sigma < 0.05);
        }
    }

    #[test]
    fn shared_widths_and_spacing() {
        let truth = vec![vec![8.0, 500.0, 20.0], vec![5.0, 530.0, 20.0], vec![3.0, 560.0, 20.0]];
        let (x, y) = data(PeakShape::Gaussian, &truth);
        let starts: Vec<PeakStart> =
            [497.0, 533.0, 562.0].iter().map(|&c| PeakStart { centre: c, height: 5.0, fwhm: 15.0 }).collect();
        let spec = PeakFitSpec {
            common_width: true,
            spacing: Spacing::Equal,
            ..PeakFitSpec::new(PeakShape::Gaussian, starts.clone())
        };
        let fit = fit_peaks(&x, &y, &spec).unwrap();
        assert_eq!(fit.n_free, 3 + 2 + 1 + 2);
        assert!((fit.peaks[2].centre.0 - 560.0).abs() < 0.2);
        assert_eq!(fit.peaks[0].fwhm.0, fit.peaks[2].fwhm.0);
        let fixed = PeakFitSpec { spacing: Spacing::Fixed(30.0), ..spec };
        let fit = fit_peaks(&x, &y, &fixed).unwrap();
        assert!((fit.peaks[1].centre.0 - fit.peaks[0].centre.0 - 30.0).abs() < 1e-9);
        assert!(fit.peaks[1].centre.1 > 0.0);
        // Comparing counts prefers three peaks.
        let cmp = compare_peak_counts(&x, &y, &PeakFitSpec::new(PeakShape::Gaussian, starts), 3).unwrap();
        let best = cmp.iter().min_by(|a, b| a.delta_aicc.total_cmp(&b.delta_aicc)).unwrap();
        assert_eq!(best.peaks, 3);
        assert!(cmp[2].p_value < 1e-6);
    }
}
