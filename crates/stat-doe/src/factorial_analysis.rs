//! Analysis of two-level factorial experiments in coded units.
//!
//! # Effects
//! With every factor coded −1 (low) and +1 (high), the model
//! `y = β₀ + Σ βₜ xₜ + ε` over product terms `xₜ = Πⱼ∈ₜ xⱼ` (main effects and
//! interactions, [`ModelFormula::interactions_up_to`]) is fitted by least
//! squares (pivoted Householder QR). The **effect** of a term is `2βₜ`: the
//! change in the mean response from its low to its high setting (for an
//! orthogonal design, the difference of the two half-means). The fit uses the
//! coded settings actually run, so a run that missed its target (a coded
//! value that is not exactly −1, 0 or +1) keeps the estimates unbiased; the
//! design is then no longer exactly orthogonal and the effects' standard
//! errors differ slightly ([`OffTarget`]).
//!
//! # Normal and half-normal plots
//! Daniel (1959), "Use of half-normal plots in interpreting factorial two-level
//! experiments", *Technometrics* 1(4): the `m` ordered effects plotted against
//! normal scores `Φ⁻¹((i − ½)/m)`, or their absolute values against
//! half-normal scores `Φ⁻¹(½ + ½(i − ½)/m)` ([`normal_scores`],
//! [`half_normal_scores`]). Inert effects fall on a line through the origin
//! with slope equal to their standard error; active effects stand off it.
//!
//! # Lenth's method
//! Lenth (1989), "Quick and easy analysis of unreplicated factorials",
//! *Technometrics* 31(4): with effects `c₁ … c_m` and no error degrees of
//! freedom, `s₀ = 1.5 · median|cᵢ|` and the pseudo standard error
//! `PSE = 1.5 · median{|cᵢ| : |cᵢ| < 2.5 s₀}`. Treating `PSE` like a standard
//! error on `d = m/3` degrees of freedom, the margin of error is
//! `ME = t_{1−α/2; d} · PSE` and the simultaneous margin of error
//! `SME = t_{γ; d} · PSE` with `γ = (1 + (1 − α)^{1/m})/2`. Effects beyond `ME`
//! are individually active; beyond `SME`, active allowing for all `m`
//! comparisons ([`lenth`]). The method assumes effects of equal variance (an
//! orthogonal design) and effect sparsity (most effects inert); with fewer
//! than about seven effects it has little power.
//!
//! When the model leaves error degrees of freedom (replicates, centre runs or
//! terms left out), each effect gets an ordinary `t` test instead:
//! `se(2β̂) = 2 √(s² [(XᵀX)⁻¹]ₜₜ)` with `s² = RSS/(n − p)`.
//!
//! # Dispersion effects
//! * **Replicated designs** — Box & Meyer (1986), "Dispersion effects from
//!   fractional designs", *Technometrics* 28(1); Box, Hunter & Hunter,
//!   *Statistics for Experimenters* (2nd ed.): the log standard
//!   deviation `ln sᵢ` of the replicates at each distinct setting is modelled
//!   on the factors like a response, and its effects judged with the same
//!   normal plot and Lenth machinery ([`replicated_dispersion`]).
//! * **Unreplicated designs** — Bergman & Hynén (1997), "Dispersion effects
//!   from unreplicated designs in the 2^(k−p) series", *Technometrics* 39(2):
//!   for factor `j`, the location model is fitted separately within the runs
//!   at `xⱼ = +1` and at `xⱼ = −1` (equivalently, `xⱼ`'s interactions with every
//!   location term are added), and `Dⱼ = (RSS₊/df₊)/(RSS₋/df₋)` is compared with
//!   `F(df₊, df₋)` (two-sided). Box & Meyer's statistic
//!   `ln(Σr²₊ / Σr²₋)` from the location model's residuals is reported
//!   alongside ([`unreplicated_dispersion`]). Both need a location model that
//!   leaves residual degrees of freedom: usually the active effects only.
//!
//! # Means
//! [`level_means`], [`interaction_means`] and [`cube_means`] average the
//! response over the runs at each sign of the coded settings (centre runs,
//! coded 0, are left out); [`FactorialFit::predict`] gives the model's
//! prediction instead, with factors not shown held at their centre.

use crate::optimal::ModelFormula;
use stat_core::distributions::{ContinuousDistribution, FisherSnedecor, Normal, StudentsT};
use stat_core::{Matrix, QrDecomposition, Result, StatError};

/// Tolerance for a coded value to count as on target (−1, 0 or +1).
pub const TARGET_TOLERANCE: f64 = 1e-6;

/// How strongly an effect stands out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Activity {
    /// Within the margin of error (or `p ≥ α`).
    Inactive,
    /// Beyond Lenth's margin of error (or `p < α` in a `t` or `z` test).
    Active,
    /// Beyond Lenth's simultaneous margin of error.
    SimultaneouslyActive,
}

impl Activity {
    /// Short text for tables.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Inactive => "",
            Self::Active => "Active",
            Self::SimultaneouslyActive => "Active (simultaneous)",
        }
    }
}

/// Lenth's pseudo standard error and margins of error for `m` effects.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LenthResult {
    /// Number of effects.
    pub m: usize,
    /// Significance level α.
    pub alpha: f64,
    /// Initial scale `1.5 · median|c|`.
    pub s0: f64,
    /// Pseudo standard error.
    pub pse: f64,
    /// Degrees of freedom `m/3`.
    pub df: f64,
    /// Margin of error `t_{1−α/2; m/3} · PSE`.
    pub me: f64,
    /// Simultaneous margin of error `t_{γ; m/3} · PSE`.
    pub sme: f64,
    /// `γ = (1 + (1 − α)^{1/m})/2`.
    pub gamma: f64,
}

impl LenthResult {
    /// Lenth's `t` ratio `c / PSE`.
    #[must_use]
    pub fn t_ratio(&self, effect: f64) -> f64 {
        effect / self.pse
    }

    /// Two-sided p-value of `c / PSE` against `t` on `m/3` df (individual).
    #[must_use]
    pub fn p_value(&self, effect: f64) -> f64 {
        StudentsT::new(self.df).and_then(|t| t.two_sided_p_value(self.t_ratio(effect))).unwrap_or(f64::NAN)
    }

    /// Whether `effect` lies beyond ME or SME.
    #[must_use]
    pub fn activity(&self, effect: f64) -> Activity {
        let a = effect.abs();
        if a > self.sme {
            Activity::SimultaneouslyActive
        } else if a > self.me {
            Activity::Active
        } else {
            Activity::Inactive
        }
    }
}

fn median(v: &mut [f64]) -> f64 {
    v.sort_by(f64::total_cmp);
    let n = v.len();
    if n % 2 == 1 { v[n / 2] } else { 0.5 * (v[n / 2 - 1] + v[n / 2]) }
}

/// Lenth's method for effects `c` at significance level `alpha`.
///
/// # Errors
/// Fewer than two effects, a non-finite effect, `alpha ∉ (0, 1)`, or a pseudo
/// standard error of zero (more than half the effects exactly zero).
pub fn lenth(effects: &[f64], alpha: f64) -> Result<LenthResult> {
    let m = effects.len();
    if m < 2 {
        return Err(invalid("Lenth's method needs at least two effects"));
    }
    if effects.iter().any(|e| !e.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "effects for Lenth's method" });
    }
    if !(alpha > 0.0 && alpha < 1.0) {
        return Err(invalid(format!("α must lie between 0 and 1 (got {alpha})")));
    }
    let mut abs: Vec<f64> = effects.iter().map(|e| e.abs()).collect();
    let s0 = 1.5 * median(&mut abs);
    let mut trimmed: Vec<f64> = abs.iter().copied().filter(|&a| a < 2.5 * s0).collect();
    if trimmed.is_empty() || s0 == 0.0 {
        return Err(invalid("Lenth's pseudo standard error is zero: most effects are exactly zero"));
    }
    let pse = 1.5 * median(&mut trimmed);
    if pse <= 0.0 {
        return Err(invalid("Lenth's pseudo standard error is zero: most effects are exactly zero"));
    }
    let df = m as f64 / 3.0;
    let t = StudentsT::new(df)?;
    let gamma = 0.5 * (1.0 + (1.0 - alpha).powf(1.0 / m as f64));
    Ok(LenthResult {
        m,
        alpha,
        s0,
        pse,
        df,
        me: t.quantile(1.0 - alpha / 2.0)? * pse,
        sme: t.quantile(gamma)? * pse,
        gamma,
    })
}

/// Normal scores `Φ⁻¹((i − ½)/m)`, `i = 1 … m` (increasing).
#[must_use]
pub fn normal_scores(m: usize) -> Vec<f64> {
    let z = Normal::standard();
    (1..=m).map(|i| z.quantile((i as f64 - 0.5) / m as f64).unwrap_or(f64::NAN)).collect()
}

/// Half-normal scores `Φ⁻¹(½ + ½(i − ½)/m)`, `i = 1 … m` (increasing).
#[must_use]
pub fn half_normal_scores(m: usize) -> Vec<f64> {
    let z = Normal::standard();
    (1..=m).map(|i| z.quantile(0.5 + 0.5 * (i as f64 - 0.5) / m as f64).unwrap_or(f64::NAN)).collect()
}

/// The normal score of each effect, in the effects' own order (ties keep
/// their order).
#[must_use]
pub fn normal_plot_positions(effects: &[f64]) -> Vec<f64> {
    plot_positions(effects, false)
}

/// The half-normal score of each effect's absolute value, in the effects'
/// own order.
#[must_use]
pub fn half_normal_plot_positions(effects: &[f64]) -> Vec<f64> {
    plot_positions(effects, true)
}

fn plot_positions(effects: &[f64], half: bool) -> Vec<f64> {
    let key = |e: f64| if half { e.abs() } else { e };
    let mut order: Vec<usize> = (0..effects.len()).collect();
    order.sort_by(|&a, &b| key(effects[a]).total_cmp(&key(effects[b])));
    let scores = if half { half_normal_scores(effects.len()) } else { normal_scores(effects.len()) };
    let mut out = vec![f64::NAN; effects.len()];
    for (rank, &i) in order.iter().enumerate() {
        out[i] = scores[rank];
    }
    out
}

/// The reference distribution of the effects' tests.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EffectTest {
    /// `t` tests on the given error degrees of freedom.
    T(f64),
    /// Wald `z` tests (fixed dispersion).
    Z,
}

/// One estimated effect.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectEstimate {
    /// Term label, e.g. `"A × B"`.
    pub term: String,
    /// Number of factors in the term.
    pub order: usize,
    /// Coefficient `β` in coded units.
    pub coefficient: f64,
    /// Effect `2β`.
    pub effect: f64,
    /// Standard error of the effect (NaN without error df).
    pub std_error: f64,
    /// `t` or `z` ratio (NaN without error df).
    pub statistic: f64,
    /// Two-sided p-value of the `t` or `z` test (NaN without error df).
    pub p_value: f64,
    /// Lenth's `t` ratio (NaN when Lenth's method is not used).
    pub lenth_t: f64,
    /// Lenth's individual p-value on `m/3` df (NaN when not used).
    pub lenth_p: f64,
    /// Whether the effect stands out: by Lenth's margins when Lenth's method
    /// is used, otherwise `p < α`.
    pub activity: Activity,
}

/// A set of effects with the inference that applies to them.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectSet {
    /// Effects, in model order.
    pub effects: Vec<EffectEstimate>,
    /// The tests behind the standard errors (`None`: no error df).
    pub test: Option<EffectTest>,
    /// Lenth's method (used when there is no test, and there are enough effects).
    pub lenth: Option<LenthResult>,
    /// Significance level α.
    pub alpha: f64,
}

impl EffectSet {
    /// Classify effects. `std_errors` (of the effects, not the coefficients)
    /// with `test` give ordinary tests; without them Lenth's method is applied
    /// when there are at least two effects and its PSE is positive.
    /// `use_lenth` forces Lenth's method as well (e.g. a saturated GLM, whose
    /// Wald tests rest on large-sample theory).
    ///
    /// # Errors
    /// Mismatched lengths or `alpha ∉ (0, 1)`.
    pub fn new(
        terms: &[(String, usize)],
        coefficients: &[f64],
        std_errors: Option<(&[f64], EffectTest)>,
        use_lenth: bool,
        alpha: f64,
    ) -> Result<Self> {
        if terms.len() != coefficients.len() || std_errors.is_some_and(|(s, _)| s.len() != terms.len()) {
            return Err(StatError::DimensionMismatch {
                context: "effect terms vs. coefficients",
                expected: terms.len(),
                found: coefficients.len(),
            });
        }
        if !(alpha > 0.0 && alpha < 1.0) {
            return Err(invalid(format!("α must lie between 0 and 1 (got {alpha})")));
        }
        let effects: Vec<f64> = coefficients.iter().map(|b| 2.0 * b).collect();
        let lenth = if std_errors.is_none() || use_lenth { lenth(&effects, alpha).ok() } else { None };
        let test = std_errors.map(|(_, t)| t);
        let p_of = |stat: f64| -> f64 {
            match test {
                Some(EffectTest::T(df)) => {
                    StudentsT::new(df).and_then(|t| t.two_sided_p_value(stat)).unwrap_or(f64::NAN)
                }
                Some(EffectTest::Z) => Normal::standard().sf(stat.abs()).map_or(f64::NAN, |p| 2.0 * p),
                None => f64::NAN,
            }
        };
        let out = terms
            .iter()
            .zip(coefficients)
            .enumerate()
            .map(|(i, ((term, order), &b))| {
                let effect = 2.0 * b;
                let (std_error, statistic, p_value) = match std_errors {
                    Some((se, _)) if se[i] > 0.0 => {
                        let z = effect / se[i];
                        (se[i], z, p_of(z))
                    }
                    _ => (f64::NAN, f64::NAN, f64::NAN),
                };
                let activity = match &lenth {
                    Some(l) => l.activity(effect),
                    _ if p_value < alpha => Activity::Active,
                    _ => Activity::Inactive,
                };
                EffectEstimate {
                    term: term.clone(),
                    order: *order,
                    coefficient: b,
                    effect,
                    std_error,
                    statistic,
                    p_value,
                    lenth_t: lenth.map_or(f64::NAN, |l| l.t_ratio(effect)),
                    lenth_p: lenth.map_or(f64::NAN, |l| l.p_value(effect)),
                    activity,
                }
            })
            .collect();
        Ok(Self { effects: out, test, lenth, alpha })
    }

    /// The effects' values in model order.
    #[must_use]
    pub fn values(&self) -> Vec<f64> {
        self.effects.iter().map(|e| e.effect).collect()
    }

    /// The slope of the normal-plot reference line: the standard error of an
    /// inert effect (Lenth's PSE, or the median standard error of the tests).
    #[must_use]
    pub fn reference_scale(&self) -> f64 {
        if let Some(l) = &self.lenth {
            return l.pse;
        }
        let mut se: Vec<f64> = self.effects.iter().map(|e| e.std_error).filter(|s| s.is_finite()).collect();
        if se.is_empty() { f64::NAN } else { median(&mut se) }
    }
}

/// Runs whose coded settings missed −1, 0 and +1.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct OffTarget {
    /// Number of runs with at least one off-target setting.
    pub runs: usize,
    /// Largest distance of a coded setting from the nearest of −1, 0, +1.
    pub max_deviation: f64,
}

/// Check coded runs against the targets −1, 0 and +1.
#[must_use]
pub fn off_target(runs: &[Vec<f64>]) -> OffTarget {
    let mut out = OffTarget::default();
    for r in runs {
        let dev = r.iter().map(|&x| [-1.0, 0.0, 1.0].iter().map(|t| (x - t).abs()).fold(f64::INFINITY, f64::min));
        let worst = dev.fold(0.0, f64::max);
        if worst > TARGET_TOLERANCE {
            out.runs += 1;
        }
        out.max_deviation = out.max_deviation.max(worst);
    }
    out
}

/// Check that `formula` holds only products of distinct factors.
///
/// # Errors
/// A squared or cubed factor (in a two-level design `x² = 1`, aliased with
/// the intercept).
pub fn check_two_level(formula: &ModelFormula, names: &[String]) -> Result<()> {
    if let Some(t) = formula.terms().iter().find(|t| t.powers.iter().any(|&p| p > 1)) {
        return Err(invalid(format!(
            "{} is a power: two-level factors have no curvature terms (x² = 1 at both levels)",
            t.label(names)
        )));
    }
    Ok(())
}

/// The model matrix of `runs` (one row per run).
fn model_matrix(formula: &ModelFormula, runs: &[Vec<f64>]) -> Result<Matrix> {
    let rows: Vec<Vec<f64>> = runs.iter().map(|r| formula.expand(r)).collect();
    Matrix::from_rows(&rows)
}

/// A least-squares fit: coefficients, residuals, RSS, inverse Gram matrix.
struct LsFit {
    coefficients: Vec<f64>,
    residuals: Vec<f64>,
    rss: f64,
    inverse_gram: Matrix,
}

fn least_squares(x: Matrix, y: &[f64], what: &str) -> Result<LsFit> {
    let qr = QrDecomposition::new(x.clone())?;
    let p = x.ncols();
    if qr.rank(qr.default_rank_tolerance()) < p {
        return Err(invalid(format!(
            "{what}: some terms cannot be estimated separately from the runs (they are aliased); choose a \
             lower interaction order or fewer terms"
        )));
    }
    let coefficients = qr.solve_least_squares(y)?;
    let fitted = x.mul_vec(&coefficients)?;
    let residuals: Vec<f64> = y.iter().zip(&fitted).map(|(a, b)| a - b).collect();
    let rss = stat_core::summation::sum_iter(residuals.iter().map(|r| r * r));
    Ok(LsFit { coefficients, residuals, rss, inverse_gram: qr.inverse_gram()? })
}

/// A two-level factorial model fitted by least squares.
#[derive(Debug, Clone, PartialEq)]
pub struct FactorialFit {
    /// The model.
    pub formula: ModelFormula,
    /// Factor names.
    pub names: Vec<String>,
    /// Coefficients, intercept first.
    pub coefficients: Vec<f64>,
    /// Effects of the non-intercept terms with their tests or Lenth's method.
    pub effects: EffectSet,
    /// Residuals, in run order.
    pub residuals: Vec<f64>,
    /// Residual sum of squares.
    pub rss: f64,
    /// Residual degrees of freedom `n − p`.
    pub error_df: usize,
    /// Off-target settings among the runs.
    pub off_target: OffTarget,
}

impl FactorialFit {
    /// Predicted mean at coded point `x`.
    #[must_use]
    pub fn predict(&self, x: &[f64]) -> f64 {
        self.formula.expand(x).iter().zip(&self.coefficients).map(|(f, b)| f * b).sum()
    }

    /// Root mean squared error `√(RSS/(n − p))` (NaN without error df).
    #[must_use]
    pub fn rmse(&self) -> f64 {
        if self.error_df == 0 { f64::NAN } else { (self.rss / self.error_df as f64).sqrt() }
    }

    /// The model of the active effects only (intercept only when none is).
    #[must_use]
    pub fn active_formula(&self) -> ModelFormula {
        let k = self.formula.n_factors();
        let terms: Vec<Vec<u8>> = self.formula.terms()[1..]
            .iter()
            .zip(&self.effects.effects)
            .filter(|(_, e)| e.activity != Activity::Inactive)
            .map(|(t, _)| t.powers.clone())
            .collect();
        ModelFormula::from_powers(k, terms)
    }
}

/// Fit `formula` to coded `runs` and response `y`, with effects judged at level
/// `alpha` (`t` tests with error df, Lenth's method without).
///
/// # Errors
/// Mismatched lengths, non-finite values, a power term ([`check_two_level`]),
/// fewer runs than terms, aliased terms, or a bad `alpha`.
pub fn fit_factorial(
    runs: &[Vec<f64>],
    y: &[f64],
    formula: &ModelFormula,
    names: &[String],
    alpha: f64,
) -> Result<FactorialFit> {
    check_inputs(runs, y, formula, names)?;
    let p = formula.n_terms();
    if runs.len() < p {
        return Err(invalid(format!(
            "the model has {p} terms but there are only {} runs; choose a lower interaction order",
            runs.len()
        )));
    }
    let fit = least_squares(model_matrix(formula, runs)?, y, "the model")?;
    let error_df = runs.len() - p;
    let terms = term_labels(formula, names);
    let effects = if error_df > 0 {
        let s2 = fit.rss / error_df as f64;
        let se: Vec<f64> = (1..p).map(|i| 2.0 * (s2 * fit.inverse_gram[(i, i)]).sqrt()).collect();
        EffectSet::new(&terms, &fit.coefficients[1..], Some((&se, EffectTest::T(error_df as f64))), false, alpha)?
    } else {
        EffectSet::new(&terms, &fit.coefficients[1..], None, true, alpha)?
    };
    Ok(FactorialFit {
        formula: formula.clone(),
        names: names.to_vec(),
        coefficients: fit.coefficients,
        effects,
        residuals: fit.residuals,
        rss: fit.rss,
        error_df,
        off_target: off_target(runs),
    })
}

/// `(label, order)` of each non-intercept term.
#[must_use]
pub fn term_labels(formula: &ModelFormula, names: &[String]) -> Vec<(String, usize)> {
    formula.terms()[1..].iter().map(|t| (t.label(names), t.degree() as usize)).collect()
}

fn check_inputs(runs: &[Vec<f64>], y: &[f64], formula: &ModelFormula, names: &[String]) -> Result<()> {
    let k = formula.n_factors();
    if names.len() != k {
        return Err(StatError::DimensionMismatch {
            context: "factor names vs. model",
            expected: k,
            found: names.len(),
        });
    }
    if runs.len() != y.len() {
        return Err(StatError::DimensionMismatch {
            context: "runs vs. responses",
            expected: runs.len(),
            found: y.len(),
        });
    }
    if runs.iter().any(|r| r.len() != k) {
        return Err(invalid("every run needs a setting for each factor"));
    }
    if runs.iter().flatten().chain(y).any(|v| !v.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "factorial runs or responses" });
    }
    check_two_level(formula, names)
}

/// Mean and count of the runs at one setting.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellMean {
    /// Mean response (NaN when there are no runs).
    pub mean: f64,
    /// Number of runs.
    pub n: usize,
}

fn cell_mean(values: impl Iterator<Item = f64>) -> CellMean {
    let (sum, n) = values.fold((0.0, 0usize), |(s, n), v| (s + v, n + 1));
    CellMean { mean: if n == 0 { f64::NAN } else { sum / n as f64 }, n }
}

/// Mean response at the low (`[0]`) and high (`[1]`) setting of each factor,
/// by the sign of the coded value (centre runs are left out).
#[must_use]
pub fn level_means(runs: &[Vec<f64>], y: &[f64]) -> Vec<[CellMean; 2]> {
    let k = runs.first().map_or(0, Vec::len);
    (0..k)
        .map(|j| {
            let at = |s: f64| cell_mean(runs.iter().zip(y).filter(|(r, _)| r[j] * s > 0.5).map(|(_, &v)| v));
            [at(-1.0), at(1.0)]
        })
        .collect()
}

/// Mean response in the four cells of factors `a` and `b`: `[level of a][level of b]`.
#[must_use]
pub fn interaction_means(runs: &[Vec<f64>], y: &[f64], a: usize, b: usize) -> [[CellMean; 2]; 2] {
    let at = |sa: f64, sb: f64| {
        cell_mean(runs.iter().zip(y).filter(|(r, _)| r[a] * sa > 0.5 && r[b] * sb > 0.5).map(|(_, &v)| v))
    };
    [[at(-1.0, -1.0), at(-1.0, 1.0)], [at(1.0, -1.0), at(1.0, 1.0)]]
}

/// Mean response at each corner of the factors `factors` (two or three):
/// `(corner signs, mean)` in standard order (first factor fastest).
#[must_use]
pub fn cube_means(runs: &[Vec<f64>], y: &[f64], factors: &[usize]) -> Vec<(Vec<f64>, CellMean)> {
    corners(factors.len())
        .into_iter()
        .map(|c| {
            let m = cell_mean(
                runs.iter()
                    .zip(y)
                    .filter(|(r, _)| factors.iter().zip(&c).all(|(&j, s)| r[j] * s > 0.5))
                    .map(|(_, &v)| v),
            );
            (c, m)
        })
        .collect()
}

/// The `2^d` corners of `[−1, 1]^d` in standard order.
#[must_use]
pub fn corners(d: usize) -> Vec<Vec<f64>> {
    (0..1usize << d).map(|i| (0..d).map(|j| if (i >> j) & 1 == 1 { 1.0 } else { -1.0 }).collect()).collect()
}

/// One distinct setting of a replicated design.
#[derive(Debug, Clone, PartialEq)]
pub struct ReplicatedSetting {
    /// Coded setting.
    pub setting: Vec<f64>,
    /// Number of runs.
    pub n: usize,
    /// Mean response.
    pub mean: f64,
    /// Standard deviation (n − 1 divisor).
    pub sd: f64,
}

/// Dispersion effects of a replicated design: effects of `ln s`.
#[derive(Debug, Clone, PartialEq)]
pub struct ReplicatedDispersion {
    /// Settings with at least two runs and a positive standard deviation.
    pub settings: Vec<ReplicatedSetting>,
    /// The model of `ln s`.
    pub formula: ModelFormula,
    /// Effects on `ln s`.
    pub effects: EffectSet,
    /// Residual df of the `ln s` model.
    pub error_df: usize,
}

/// Group runs by their coded setting (rounded to 1e-6).
fn distinct_settings(runs: &[Vec<f64>]) -> Vec<(Vec<f64>, Vec<usize>)> {
    let mut groups: Vec<(Vec<i64>, Vec<f64>, Vec<usize>)> = Vec::new();
    for (i, r) in runs.iter().enumerate() {
        let key: Vec<i64> = r.iter().map(|x| (x * 1e6).round() as i64).collect();
        match groups.iter_mut().find(|g| g.0 == key) {
            Some(g) => g.2.push(i),
            None => groups.push((key, r.clone(), vec![i])),
        }
    }
    groups.into_iter().map(|(_, s, idx)| (s, idx)).collect()
}

/// Box & Meyer's dispersion analysis of a replicated design: `ln s` at each
/// distinct setting with two or more runs, modelled by `formula` when the
/// settings can estimate it, else by the main effects.
///
/// # Errors
/// Fewer than three replicated settings, a setting whose replicates are
/// identical (`s = 0`, so `ln s = −∞`), or neither model estimable.
pub fn replicated_dispersion(
    runs: &[Vec<f64>],
    y: &[f64],
    formula: &ModelFormula,
    names: &[String],
    alpha: f64,
) -> Result<ReplicatedDispersion> {
    check_inputs(runs, y, formula, names)?;
    let mut settings = Vec::new();
    for (setting, idx) in distinct_settings(runs) {
        if idx.len() < 2 {
            continue;
        }
        let v: Vec<f64> = idx.iter().map(|&i| y[i]).collect();
        let n = v.len() as f64;
        let mean = v.iter().sum::<f64>() / n;
        let sd = (v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1.0)).sqrt();
        if sd <= 0.0 {
            return Err(invalid(
                "the replicates at one setting are identical (standard deviation 0), so ln s cannot be modelled",
            ));
        }
        settings.push(ReplicatedSetting { setting, n: idx.len(), mean, sd });
    }
    if settings.len() < 3 {
        return Err(invalid("dispersion effects need at least three settings run more than once"));
    }
    let points: Vec<Vec<f64>> = settings.iter().map(|s| s.setting.clone()).collect();
    let ln_s: Vec<f64> = settings.iter().map(|s| s.sd.ln()).collect();
    let k = formula.n_factors();
    let candidates = [formula.clone(), ModelFormula::main_effects(k)];
    let mut last = invalid("dispersion model not estimable");
    for f in candidates {
        if f.n_terms() > points.len() {
            continue;
        }
        match least_squares(model_matrix(&f, &points)?, &ln_s, "the ln s model") {
            Ok(fit) => {
                let p = f.n_terms();
                let error_df = points.len() - p;
                let terms = term_labels(&f, names);
                let effects = if error_df > 0 {
                    let s2 = fit.rss / error_df as f64;
                    let se: Vec<f64> = (1..p).map(|i| 2.0 * (s2 * fit.inverse_gram[(i, i)]).sqrt()).collect();
                    EffectSet::new(
                        &terms,
                        &fit.coefficients[1..],
                        Some((&se, EffectTest::T(error_df as f64))),
                        false,
                        alpha,
                    )?
                } else {
                    EffectSet::new(&terms, &fit.coefficients[1..], None, true, alpha)?
                };
                return Ok(ReplicatedDispersion { settings, formula: f, effects, error_df });
            }
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// Bergman–Hynén and Box–Meyer statistics for one factor.
#[derive(Debug, Clone, PartialEq)]
pub struct DispersionTest {
    /// Factor name.
    pub factor: String,
    /// Residual sum of squares of the location model within the high runs.
    pub rss_high: f64,
    /// Its degrees of freedom.
    pub df_high: usize,
    /// Residual sum of squares within the low runs.
    pub rss_low: f64,
    /// Its degrees of freedom.
    pub df_low: usize,
    /// `D = (RSS₊/df₊)/(RSS₋/df₋)` (NaN when a side has no df or RSS₋ = 0).
    pub statistic: f64,
    /// Two-sided p-value of `D` against `F(df₊, df₋)`.
    pub p_value: f64,
    /// Box–Meyer `ln(Σr²₊/Σr²₋)` from the location model's residuals.
    pub box_meyer: f64,
}

/// Dispersion tests for an unreplicated design (one per factor) given the
/// location model `location`.
///
/// # Errors
/// Bad inputs, the location model not estimable, or no residual df.
pub fn unreplicated_dispersion(
    runs: &[Vec<f64>],
    y: &[f64],
    location: &ModelFormula,
    names: &[String],
) -> Result<Vec<DispersionTest>> {
    check_inputs(runs, y, location, names)?;
    if runs.len() <= location.n_terms() {
        return Err(invalid("the location model leaves no residual degrees of freedom for dispersion tests"));
    }
    let fit = least_squares(model_matrix(location, runs)?, y, "the location model")?;
    let k = location.n_factors();
    let mut out = Vec::with_capacity(k);
    for j in 0..k {
        // Each location term with factor j removed (x_j is constant within a side).
        let reduced: Vec<Vec<u8>> = location.terms()[1..]
            .iter()
            .map(|t| {
                let mut p = t.powers.clone();
                p[j] = 0;
                p
            })
            .filter(|p| p.iter().any(|&v| v > 0))
            .collect();
        let side_formula = ModelFormula::from_powers(k, reduced);
        let side = |sign: f64| -> (f64, usize) {
            let idx: Vec<usize> = (0..runs.len()).filter(|&i| runs[i][j] * sign > 0.5).collect();
            let sub: Vec<Vec<f64>> = idx.iter().map(|&i| runs[i].clone()).collect();
            let ys: Vec<f64> = idx.iter().map(|&i| y[i]).collect();
            if sub.len() <= side_formula.n_terms() {
                return (f64::NAN, 0);
            }
            match model_matrix(&side_formula, &sub).and_then(|x| least_squares(x, &ys, "half")) {
                Ok(f) => (f.rss, sub.len() - side_formula.n_terms()),
                Err(_) => (f64::NAN, 0),
            }
        };
        let (rss_high, df_high) = side(1.0);
        let (rss_low, df_low) = side(-1.0);
        let (statistic, p_value) = if df_high > 0 && df_low > 0 && rss_low > 0.0 && rss_high.is_finite() {
            let d = (rss_high / df_high as f64) / (rss_low / df_low as f64);
            let p = FisherSnedecor::new(df_high as f64, df_low as f64)
                .and_then(|f| Ok((f.sf(d)?, f.cdf(d)?)))
                .map_or(f64::NAN, |(u, l)| (2.0 * u.min(l)).min(1.0));
            (d, p)
        } else {
            (f64::NAN, f64::NAN)
        };
        let sq = |s: f64| -> f64 {
            fit.residuals.iter().zip(runs).filter(|(_, r)| r[j] * s > 0.5).map(|(e, _)| e * e).sum()
        };
        let (plus, minus) = (sq(1.0), sq(-1.0));
        let box_meyer = if plus > 0.0 && minus > 0.0 { (plus / minus).ln() } else { f64::NAN };
        out.push(DispersionTest {
            factor: names[j].clone(),
            rss_high,
            df_high,
            rss_low,
            df_low,
            statistic,
            p_value,
            box_meyer,
        });
    }
    Ok(out)
}

/// The terms of `formula` the runs can estimate, chosen in model order: a
/// term whose column lies in the span of the columns already kept (aliased
/// with lower-order or earlier terms, as in a fractional factorial) is left
/// out, and so is every term once the model is saturated (as many terms as
/// runs). Returns the reduced model and the labels of the terms left out.
#[must_use]
pub fn estimable_terms(runs: &[Vec<f64>], formula: &ModelFormula, names: &[String]) -> (ModelFormula, Vec<String>) {
    let n = runs.len();
    // Modified Gram–Schmidt over the model columns.
    let mut basis: Vec<Vec<f64>> = Vec::new();
    let mut kept = Vec::new();
    let mut dropped = Vec::new();
    for t in formula.terms() {
        let col: Vec<f64> = runs.iter().map(|r| t.eval(r)).collect();
        let norm0 = col.iter().map(|v| v * v).sum::<f64>().sqrt();
        let mut v = col;
        for b in &basis {
            let d: f64 = v.iter().zip(b).map(|(a, c)| a * c).sum();
            v.iter_mut().zip(b).for_each(|(a, c)| *a -= d * c);
        }
        let norm = v.iter().map(|x| x * x).sum::<f64>().sqrt();
        if basis.len() < n && norm0 > 0.0 && norm > 1e-8 * norm0 {
            v.iter_mut().for_each(|x| *x /= norm);
            basis.push(v);
            kept.push(t.powers.clone());
        } else if t.degree() > 0 {
            dropped.push(t.label(names));
        }
    }
    let k = formula.n_factors();
    (ModelFormula::from_powers(k, kept), dropped)
}

/// A saturated Poisson log-linear model (as many terms as runs): the fit is
/// exact, `η = ln y`, so the coefficients solve `Xβ = ln y`; Wald standard
/// errors come from `Cov(β̂) = (XᵀWX)⁻¹` with `W = diag(μ̂) = diag(y)`
/// (McCullagh & Nelder, §6.2). Effects (on the log scale) get Wald `z` tests
/// and Lenth's method.
///
/// # Errors
/// A count of zero (its log-mean is −∞ in the saturated model), a negative
/// count, a model that is not saturated or not estimable, or bad inputs.
pub fn saturated_poisson(
    runs: &[Vec<f64>],
    y: &[f64],
    formula: &ModelFormula,
    names: &[String],
    alpha: f64,
) -> Result<(Vec<f64>, EffectSet)> {
    check_inputs(runs, y, formula, names)?;
    if formula.n_terms() != runs.len() {
        return Err(invalid("the saturated Poisson fit needs exactly as many terms as runs"));
    }
    if y.iter().any(|&v| v <= 0.0) {
        return Err(invalid(
            "a count of zero has an infinite log mean in the saturated model; choose a lower interaction order",
        ));
    }
    let ln_y: Vec<f64> = y.iter().map(|v| v.ln()).collect();
    let fit = least_squares(model_matrix(formula, runs)?, &ln_y, "the model")?;
    let weighted: Vec<Vec<f64>> =
        runs.iter().zip(y).map(|(r, w)| formula.expand(r).into_iter().map(|f| f * w.sqrt()).collect()).collect();
    let inv = QrDecomposition::new(Matrix::from_rows(&weighted)?)?.inverse_gram()?;
    let p = formula.n_terms();
    let se: Vec<f64> = (1..p).map(|i| 2.0 * inv[(i, i)].sqrt()).collect();
    let effects =
        EffectSet::new(&term_labels(formula, names), &fit.coefficients[1..], Some((&se, EffectTest::Z)), true, alpha)?;
    Ok((fit.coefficients, effects))
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn standard_order(k: usize) -> Vec<Vec<f64>> {
        corners(k)
    }

    fn names(s: &str) -> Vec<String> {
        s.chars().map(String::from).collect()
    }

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol * b.abs().max(1.0)
    }

    fn assert_all(got: &[f64], want: &[f64], tol: f64) {
        assert_eq!(got.len(), want.len());
        for (i, (g, w)) in got.iter().zip(want).enumerate() {
            assert!(close(*g, *w, tol), "item {i}: {g} vs {w}");
        }
    }

    const FILTRATION: [f64; 16] =
        [45.0, 71.0, 48.0, 65.0, 68.0, 60.0, 80.0, 65.0, 43.0, 100.0, 45.0, 104.0, 75.0, 86.0, 70.0, 96.0];

    /// Box, Hunter & Hunter's 2³ pilot plant experiment: the published
    /// effects T = 23, C = −5, K = 1.5, TC = 1.5, TK = 10, CK = 0, TCK = 0.5.
    #[test]
    fn pilot_plant_effects() {
        let y = [60.0, 72.0, 54.0, 68.0, 52.0, 83.0, 45.0, 80.0];
        let f = ModelFormula::interactions_up_to(3, 3);
        let fit = fit_factorial(&standard_order(3), &y, &f, &names("TCK"), 0.05).unwrap();
        let labels: Vec<&str> = fit.effects.effects.iter().map(|e| e.term.as_str()).collect();
        assert_eq!(labels, ["T", "C", "K", "T × C", "T × K", "C × K", "T × C × K"]);
        assert_all(&fit.effects.values(), &[23.0, -5.0, 1.5, 1.5, 10.0, 0.0, 0.5], 1e-12);
        assert_eq!(fit.error_df, 0);
        assert!(fit.effects.lenth.is_some() && fit.effects.test.is_none());
        assert!(close(fit.predict(&[1.0, 1.0, 1.0]), 80.0, 1e-12));
        assert_eq!(fit.off_target.runs, 0);
    }

    /// Montgomery Example 6.2: all 15 published effects, and Lenth's PSE, ME
    /// and SME, scores and the dispersion statistics from
    /// `tests/data/factorial_reference.py` (SciPy).
    #[test]
    fn filtration_effects_lenth_and_dispersion() {
        let f = ModelFormula::interactions_up_to(4, 4);
        let n = names("ABCD");
        let fit = fit_factorial(&standard_order(4), &FILTRATION, &f, &n, 0.05).unwrap();
        let published = [
            21.625, 3.125, 9.875, 14.625, 0.125, -18.125, 16.625, 2.375, -0.375, -1.125, 1.875, 4.125, -1.625, -2.625,
            1.375,
        ];
        assert_all(&fit.effects.values(), &published, 1e-12);
        let l = fit.effects.lenth.unwrap();
        assert!(close(l.s0, 3.9375, 1e-12));
        assert!(close(l.pse, 2.625, 1e-12));
        assert!(close(l.df, 5.0, 1e-12));
        assert!(close(l.me, 6.7477773185453165, 1e-10), "{}", l.me);
        assert!(close(l.sme, 13.698959562802386, 1e-10), "{}", l.sme);
        assert!(close(l.gamma, 0.9982931435267048, 1e-12));
        let active: Vec<(&str, Activity)> = fit
            .effects
            .effects
            .iter()
            .filter(|e| e.activity != Activity::Inactive)
            .map(|e| (e.term.as_str(), e.activity))
            .collect();
        use Activity::{Active, SimultaneouslyActive as Sim};
        assert_eq!(active, [("A", Sim), ("C", Active), ("D", Sim), ("A × C", Sim), ("A × D", Sim)]);
        assert!(close(fit.effects.effects[0].lenth_t, 8.238095238095243, 1e-12));
        assert!(close(fit.effects.effects[0].lenth_p, 0.0004294763936910598, 1e-9));
        assert!(close(fit.effects.effects[2].lenth_p, 0.013131762950767152, 1e-9));
        assert!(close(fit.effects.reference_scale(), 2.625, 1e-12));

        assert_all(
            &normal_scores(15),
            &[
                -1.8339146358159146,
                -1.2815515655446004,
                -0.967421566101701,
                -0.7279132908816442,
                -0.5244005127080409,
                -0.3406948270877956,
                -0.16789400478810546,
                0.0,
                0.16789400478810546,
                0.3406948270877954,
                0.5244005127080407,
                0.7279132908816444,
                0.967421566101701,
                1.2815515655446004,
                1.8339146358159146,
            ],
            1e-10,
        );
        assert_all(
            &half_normal_scores(15),
            &[
                0.04178929781645395,
                0.12566134685507416,
                0.21042839424792484,
                0.29673783825989813,
                0.38532046640756773,
                0.47704042848944356,
                0.5729675484954635,
                0.6744897501960817,
                0.7835003753897741,
                0.9027347916438648,
                1.0364333894937898,
                1.191816171681394,
                1.3829941271006385,
                1.6448536269514722,
                2.1280452341849863,
            ],
            1e-10,
        );
        // A × C is the most negative effect, A the largest |effect|.
        let pos = normal_plot_positions(&fit.effects.values());
        assert!(close(pos[5], -1.8339146358159146, 1e-10));
        let half = half_normal_plot_positions(&fit.effects.values());
        assert!(close(half[0], 2.1280452341849863, 1e-10));

        // Location model of the active effects: A, C, D, AC, AD.
        let location = fit.active_formula();
        assert_eq!(location.describe(&n), "A + C + D + A × C + A × D");
        let tests = unreplicated_dispersion(&standard_order(4), &FILTRATION, &location, &n).unwrap();
        let want = [
            (1.1325136612021856, 5, 5, 0.894711998037824, 0.12443964117398418),
            (1.2555555555555562, 2, 2, 0.8866995073891623, 0.058953647794023165),
            (4.523076923076923, 4, 4, 0.17295110544310502, 1.3455531126935858),
            (0.677570093457944, 4, 4, 0.7152492099392762, -0.35737508788580724),
        ];
        for (t, w) in tests.iter().zip(want) {
            assert!(close(t.statistic, w.0, 1e-10), "{} D {}", t.factor, t.statistic);
            assert_eq!((t.df_high, t.df_low), (w.1, w.2), "{}", t.factor);
            assert!(close(t.p_value, w.3, 1e-9), "{} p {}", t.factor, t.p_value);
            assert!(close(t.box_meyer, w.4, 1e-10), "{} BM {}", t.factor, t.box_meyer);
        }
    }

    /// Two runs that missed their targets: the fit at the actual coded values
    /// (main effects and two-factor interactions, 5 error df) against NumPy.
    #[test]
    fn off_target_settings() {
        let mut runs = standard_order(4);
        runs[2][0] = -0.9;
        runs[9][2] = 1.1;
        let f = ModelFormula::interactions_up_to(4, 2);
        let fit = fit_factorial(&runs, &FILTRATION, &f, &names("ABCD"), 0.05).unwrap();
        assert_eq!(fit.off_target.runs, 2);
        assert!(close(fit.off_target.max_deviation, 0.1, 1e-9));
        assert_eq!(fit.error_df, 5);
        assert_all(
            &fit.effects.values(),
            &[
                23.331810129110423,
                1.4264802683528273,
                10.052582251479047,
                16.323519731647153,
                -1.2872155730575678,
                -18.233721907110507,
                18.03721557305759,
                2.475431509647246,
                -1.7955059705208536,
                -1.2254315096472173,
            ],
            1e-10,
        );
        let se: Vec<f64> = fit.effects.effects.iter().map(|e| e.std_error).collect();
        assert_all(
            &se,
            &[
                3.277207124981257,
                3.2528415306839964,
                3.3471144337104644,
                3.2528415306839964,
                3.2604744863798767,
                3.3660723622488633,
                3.260474486379877,
                3.345741674795813,
                3.248476214377039,
                3.3457416747958124,
            ],
            1e-10,
        );
        assert!(fit.effects.lenth.is_none());
        assert_eq!(fit.effects.test, Some(EffectTest::T(5.0)));
    }

    const REPLICATED: [[f64; 3]; 8] = [
        [48.9, 50.8, 50.0],
        [54.4, 55.0, 55.9],
        [39.3, 39.1, 39.3],
        [52.9, 53.2, 55.8],
        [50.5, 49.0, 47.5],
        [52.0, 48.2, 55.2],
        [38.5, 46.0, 40.1],
        [51.3, 51.6, 59.2],
    ];

    fn replicated() -> (Vec<Vec<f64>>, Vec<f64>) {
        let base = standard_order(3);
        let runs = (0..24).map(|i| base[i / 3].clone()).collect();
        let y = REPLICATED.iter().flatten().copied().collect();
        (runs, y)
    }

    /// A replicated 2³ (simulated): t tests on 16 df and the ln s effects
    /// with Lenth's margins, against `factorial_reference.py`.
    #[test]
    fn replicated_location_and_dispersion() {
        let (runs, y) = replicated();
        let f = ModelFormula::interactions_up_to(3, 3);
        let n = names("ABC");
        let fit = fit_factorial(&runs, &y, &f, &n, 0.05).unwrap();
        assert_eq!(fit.error_df, 16);
        assert_all(
            &fit.effects.values(),
            &[
                8.808333333333323,
                -4.258333333333344,
                -0.4583333333333382,
                4.80833333333334,
                -1.1583333333333279,
                1.6416666666666733,
                0.04166666666666041,
            ],
            1e-10,
        );
        for e in &fit.effects.effects {
            assert!(close(e.std_error, 1.062753237795324, 1e-10));
        }
        let p: Vec<f64> = fit.effects.effects.iter().map(|e| e.p_value).collect();
        assert_all(
            &p,
            &[
                3.493106927706987e-07,
                0.001017188383325638,
                0.6720266460337433,
                0.00034565379269699904,
                0.2918848705683927,
                0.14195903009420668,
                0.969210918739283,
            ],
            1e-8,
        );
        let d = replicated_dispersion(&runs, &y, &f, &n, 0.05).unwrap();
        assert_eq!(d.settings.len(), 8);
        assert_eq!(d.error_df, 0);
        assert_all(
            &d.effects.values(),
            &[
                0.8413261459655598,
                -0.03763799189892525,
                1.6380906820619308,
                0.5340177790104671,
                -0.3544627119050173,
                0.6442577479219036,
                -0.8956751982012597,
            ],
            1e-10,
        );
        let l = d.effects.lenth.unwrap();
        assert!(close(l.pse, 0.9663866218828554, 1e-10));
        assert!(close(l.me, 3.637598179981206, 1e-10));
        assert!(close(l.sme, 8.705507480728595, 1e-10));
    }

    #[test]
    fn means_and_cube() {
        let y = [60.0, 72.0, 54.0, 68.0, 52.0, 83.0, 45.0, 80.0];
        let runs = standard_order(3);
        let lm = level_means(&runs, &y);
        // T: low (60+54+52+45)/4 = 52.75, high (72+68+83+80)/4 = 75.75 — the effect 23.
        assert!(close(lm[0][0].mean, 52.75, 1e-12) && close(lm[0][1].mean, 75.75, 1e-12));
        let im = interaction_means(&runs, &y, 0, 2);
        // T × K cells: T−K− (60+54)/2 = 57, T+K+ (83+80)/2 = 81.5.
        assert!(close(im[0][0].mean, 57.0, 1e-12) && close(im[1][1].mean, 81.5, 1e-12));
        assert_eq!(im[1][0].n, 2);
        let cube = cube_means(&runs, &y, &[0, 1, 2]);
        assert_eq!(cube.len(), 8);
        assert!(cube.iter().zip(y).all(|((_, m), v)| close(m.mean, v, 1e-12) && m.n == 1));
        // Centre runs are left out of level means.
        let mut with_centre = runs.clone();
        with_centre.push(vec![0.0, 0.0, 0.0]);
        let mut y2 = y.to_vec();
        y2.push(1000.0);
        assert!(close(level_means(&with_centre, &y2)[0][0].mean, 52.75, 1e-12));
        assert_eq!(off_target(&with_centre).runs, 0);
    }

    #[test]
    fn errors_and_edge_cases() {
        let n = names("AB");
        let runs = standard_order(2);
        let y = [1.0, 2.0, 3.0, 5.0];
        let quad = ModelFormula::quadratic(2);
        assert!(fit_factorial(&runs, &y, &quad, &n, 0.05).unwrap_err().to_string().contains("power"));
        let three = ModelFormula::interactions_up_to(3, 3);
        assert!(fit_factorial(&standard_order(3)[..4], &y, &three, &names("ABC"), 0.05).is_err());
        // Aliased: C = A × B in a half fraction.
        let half: Vec<Vec<f64>> = runs.iter().chain(&runs).map(|r| vec![r[0], r[1], r[0] * r[1]]).collect();
        let y8 = [1.0, 2.0, 3.0, 5.0, 1.5, 2.5, 2.0, 6.0];
        let aliased = ModelFormula::from_powers(3, vec![vec![1, 0, 0], vec![0, 1, 0], vec![0, 0, 1], vec![1, 1, 0]]);
        let e = fit_factorial(&half, &y8, &aliased, &names("ABC"), 0.05).unwrap_err();
        assert!(e.to_string().contains("aliased"), "{e}");
        assert!(lenth(&[1.0], 0.05).is_err());
        assert!(lenth(&[0.0, 0.0, 0.0, 1.0], 0.05).is_err());
        assert!(lenth(&[1.0, 2.0, 3.0], 1.5).is_err());
        assert!(replicated_dispersion(&runs, &y, &ModelFormula::main_effects(2), &n, 0.05).is_err());
        assert!(unreplicated_dispersion(&runs, &y, &ModelFormula::interactions_up_to(2, 2), &n).is_err());
        let f = ModelFormula::interactions_up_to(4, 2);
        assert_eq!(f.n_terms(), 11);
        assert_eq!(ModelFormula::interactions_up_to(5, 9).n_terms(), 32);
    }

    /// A saturated Poisson 2² against `factorial_reference.py`.
    #[test]
    fn saturated_poisson_counts() {
        let y = [4.0, 9.0, 12.0, 30.0];
        let f = ModelFormula::interactions_up_to(2, 2);
        let (b, e) = saturated_poisson(&standard_order(2), &y, &f, &names("AB"), 0.05).unwrap();
        assert!(close(b[0], y.iter().map(|v| v.ln()).sum::<f64>() / 4.0, 1e-12));
        assert_all(&e.values(), &[0.8636104740452422, 1.151292546497023, 0.05268025782891317], 1e-10);
        let se: Vec<f64> = e.effects.iter().map(|x| x.std_error).collect();
        assert_all(&se, &[0.3456073558887953, 0.3456073558887954, 0.3456073558887953], 1e-10);
        assert_eq!(e.test, Some(EffectTest::Z));
        assert!(e.lenth.is_some());
        assert!(saturated_poisson(&standard_order(2), &[0.0, 1.0, 2.0, 3.0], &f, &names("AB"), 0.05).is_err());
    }

    /// In the half fraction 2^(4−1) with D = ABC, every two-factor interaction
    /// is aliased with another (AB = CD, AC = BD, AD = BC), and the main
    /// effects with three-factor interactions: of the 16 terms, the
    /// intercept, four main effects and three interactions remain.
    #[test]
    fn estimable_terms_of_a_half_fraction() {
        let runs: Vec<Vec<f64>> =
            standard_order(3).into_iter().map(|r| vec![r[0], r[1], r[2], r[0] * r[1] * r[2]]).collect();
        let n = names("ABCD");
        let (f, dropped) = estimable_terms(&runs, &ModelFormula::interactions_up_to(4, 4), &n);
        assert_eq!(f.describe(&n), "A + B + C + D + A × B + A × C + A × D");
        assert_eq!(dropped.len(), 8);
        assert!(dropped.contains(&"C × D".to_owned()) && dropped.contains(&"A × B × C × D".to_owned()));
    }
}
