//! Life–stress models for accelerated life tests: a log-location-scale life
//! distribution (Weibull, lognormal, log-logistic) or the exponential whose
//! location depends on one or two stresses through a physical acceleration
//! relationship, fitted to censored failure times with
//! [`crate::life_regression`].
//!
//! # Relationships
//! The location of log life is `μ = β₀ + Σ βⱼ xⱼ + o`, where each stress `s`
//! enters through its transform `x`:
//!
//! | Relationship | `x` | Offset `o` | Life proportional to |
//! |---|---|---|---|
//! | Arrhenius | `1/(k T)` = `11 604.518 / T[K]` | – | `exp(Eₐ/(kT))` |
//! | Eyring | `1/(k T)` | `−ln T[K]` | `(1/T) exp(ΔH/(kT))` |
//! | Inverse power | `ln s` | – | `s^(−n)`, `n = −β` |
//! | Exponential | `s` | – | `exp(β s)` |
//!
//! with `k = 8.617 333 262 × 10⁻⁵ eV/K` (Boltzmann's constant), temperatures
//! in °C (`T[K] = °C + 273.15`) or kelvin, so the Arrhenius (or Eyring)
//! coefficient is the activation energy in electron-volts (Nelson,
//! *Accelerated Testing*, 1990, ch. 2; Meeker & Escobar, *Statistical Methods
//! for Reliability Data*, 1998, ch. 18–19; Escobar & Meeker 2006, "A review
//! of accelerated test models", *Statistical Science* 21, 552–577). Two
//! stresses give, for example, the temperature–humidity (Peck) model
//! (Arrhenius in temperature, inverse power in relative humidity) or the
//! generalized Eyring model (Eyring in temperature, an exponential or inverse
//! power second stress, optionally with the interaction `x₁x₂`).
//!
//! # Scale
//! The scale `σ` (Weibull shape `β = 1/σ`) is constant across stress by
//! default; optionally `ln σ` is linear in the transformed stresses
//! (Meeker & Escobar §17.5). The likelihood-ratio statistic
//! `2(ℓ_varying − ℓ_constant)` on as many degrees of freedom as stresses
//! tests a constant scale.
//!
//! # Acceleration and use conditions
//! The acceleration factor of stress `a` relative to `b` is
//! `AF = exp(μ(b) − μ(a))`, the ratio of the lives at `b` and `a` (with a
//! constant scale, of every quantile). The life distribution at use
//! conditions is the fitted distribution at the use stresses, with Wald
//! and likelihood-ratio limits for its quantiles and failure probabilities.
//!
//! # Checking the model
//! Following Nelson (1990, ch. 5 §4) and Meeker & Escobar (§19.2), the
//! fit is compared with models for the distinct stress levels:
//! *individual* fits (a location and scale per level), and a *common scale*
//! model (a location per level, one scale). Likelihood-ratio tests compare
//! individual with common scale (equal scales, `L − 1` df), common scale
//! with the life–stress model (the relationship, `L − 1 − m` df for `m`
//! stress terms) and individual with the life–stress model. Levels where an
//! individual fit is impossible (e.g. no failures) are left out of these
//! comparisons, and the models compared are refitted to the rest.

use stat_core::{ChiSquared, ContinuousDistribution, Result, StatError};

use crate::censored_fit::{CensoredFit, IntervalEstimate, fit_censored};
use crate::life_data::{CensorKind, LifeData, LifeObservation};
use crate::life_regression::{CovariatePoint, RegressionData, RegressionFamily, RegressionFit, fit_regression};

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// `1/k` in kelvin per electron-volt (`k` = Boltzmann's constant,
/// 8.617 333 262 × 10⁻⁵ eV/K).
pub const INVERSE_BOLTZMANN: f64 = 1.0 / 8.617_333_262e-5;

/// How one stress enters the location of log life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StressTransform {
    /// Arrhenius: `x = 1/(kT)` (temperature).
    Arrhenius,
    /// Eyring: `x = 1/(kT)` with offset `−ln T` (temperature).
    Eyring,
    /// Inverse power: `x = ln s`.
    InversePower,
    /// Exponential (linear): `x = s`.
    Exponential,
}

impl StressTransform {
    /// Every transform, in menu order.
    pub const ALL: [Self; 4] = [Self::Arrhenius, Self::InversePower, Self::Exponential, Self::Eyring];

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Arrhenius => "Arrhenius (temperature)",
            Self::Eyring => "Eyring (temperature)",
            Self::InversePower => "Inverse power (log stress)",
            Self::Exponential => "Exponential (linear stress)",
        }
    }

    /// Stable key.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Arrhenius => "arrhenius",
            Self::Eyring => "eyring",
            Self::InversePower => "inverse-power",
            Self::Exponential => "exponential",
        }
    }

    /// The transform with this key (`"linear"` is the exponential).
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        if key == "linear" {
            return Some(Self::Exponential);
        }
        Self::ALL.into_iter().find(|t| t.key() == key)
    }

    /// `true` for the temperature relationships.
    #[must_use]
    pub fn is_temperature(self) -> bool {
        matches!(self, Self::Arrhenius | Self::Eyring)
    }
}

/// Units of a temperature stress.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TemperatureUnit {
    /// Degrees Celsius.
    #[default]
    Celsius,
    /// Kelvin.
    Kelvin,
}

/// One stress variable and its relationship.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StressVariable {
    /// The relationship.
    pub transform: StressTransform,
    /// Temperature units (temperature relationships only).
    pub unit: TemperatureUnit,
}

impl StressVariable {
    /// A stress with this relationship, temperatures in °C.
    #[must_use]
    pub fn new(transform: StressTransform) -> Self {
        Self { transform, unit: TemperatureUnit::Celsius }
    }

    /// Absolute temperature in kelvin.
    fn kelvin(&self, s: f64) -> f64 {
        match self.unit {
            TemperatureUnit::Celsius => s + 273.15,
            TemperatureUnit::Kelvin => s,
        }
    }

    /// `true` when `s` is a valid stress for the relationship.
    #[must_use]
    pub fn valid(&self, s: f64) -> bool {
        s.is_finite()
            && match self.transform {
                StressTransform::Arrhenius | StressTransform::Eyring => self.kelvin(s) > 0.0,
                StressTransform::InversePower => s > 0.0,
                StressTransform::Exponential => true,
            }
    }

    /// The transformed stress `x` (the relationship's scale).
    #[must_use]
    pub fn transformed(&self, s: f64) -> f64 {
        match self.transform {
            StressTransform::Arrhenius | StressTransform::Eyring => INVERSE_BOLTZMANN / self.kelvin(s),
            StressTransform::InversePower => s.ln(),
            StressTransform::Exponential => s,
        }
    }

    /// The stress with transformed value `x`.
    #[must_use]
    pub fn untransformed(&self, x: f64) -> f64 {
        match self.transform {
            StressTransform::Arrhenius | StressTransform::Eyring => {
                let k = INVERSE_BOLTZMANN / x;
                match self.unit {
                    TemperatureUnit::Celsius => k - 273.15,
                    TemperatureUnit::Kelvin => k,
                }
            }
            StressTransform::InversePower => x.exp(),
            StressTransform::Exponential => x,
        }
    }

    /// The offset added to the location (`−ln T` for Eyring).
    #[must_use]
    pub fn offset(&self, s: f64) -> f64 {
        if self.transform == StressTransform::Eyring { -self.kelvin(s).ln() } else { 0.0 }
    }
}

/// The model: a life distribution, its stresses and scale form.
#[derive(Debug, Clone, PartialEq)]
pub struct LifeStressModel {
    /// Life distribution (one of [`RegressionFamily::LIFE`]).
    pub family: RegressionFamily,
    /// One or two stresses.
    pub stresses: Vec<StressVariable>,
    /// Include the interaction `x₁x₂` of two stresses.
    pub interaction: bool,
    /// `ln σ` linear in the transformed stresses (otherwise constant).
    pub stress_dependent_scale: bool,
}

impl LifeStressModel {
    /// Location covariates `(1, x₁, [x₂], [x₁x₂])` at stresses `s`.
    fn location(&self, s: &[f64]) -> Vec<f64> {
        let x: Vec<f64> = self.stresses.iter().zip(s).map(|(v, s)| v.transformed(*s)).collect();
        let mut out = vec![1.0];
        out.extend(&x);
        if self.interaction && x.len() == 2 {
            out.push(x[0] * x[1]);
        }
        out
    }

    /// The covariate point at stresses `s` under this model.
    #[must_use]
    pub fn point(&self, s: &[f64]) -> CovariatePoint {
        let x: Vec<f64> = self.stresses.iter().zip(s).map(|(v, s)| v.transformed(*s)).collect();
        let scale = if self.stress_dependent_scale {
            std::iter::once(1.0).chain(x.iter().copied()).collect()
        } else {
            vec![1.0]
        };
        CovariatePoint {
            location: self.location(s),
            scale,
            offset: self.stresses.iter().zip(s).map(|(v, s)| v.offset(*s)).sum(),
        }
    }

    /// Number of stress terms in the location.
    #[must_use]
    pub fn n_stress_terms(&self) -> usize {
        self.location(&vec![1.0; self.stresses.len()]).len() - 1
    }
}

/// Failure times with the stresses each unit was tested at.
#[derive(Debug, Clone, PartialEq)]
pub struct LifeStressData {
    /// The observations.
    pub observations: Vec<LifeObservation>,
    /// Stresses of each observation (one value per stress variable).
    pub stresses: Vec<Vec<f64>>,
}

/// A likelihood-ratio test between two nested models.
#[derive(Debug, Clone, PartialEq)]
pub struct LikelihoodRatioTest {
    /// What is tested.
    pub name: &'static str,
    /// Log-likelihood of the simpler model.
    pub reduced: f64,
    /// Log-likelihood of the larger model.
    pub full: f64,
    /// `2(ℓ_full − ℓ_reduced)`.
    pub statistic: f64,
    /// Degrees of freedom.
    pub df: usize,
    /// Upper-tail χ² probability.
    pub p_value: f64,
}

impl LikelihoodRatioTest {
    fn new(name: &'static str, reduced: f64, full: f64, df: usize) -> Self {
        let statistic = (2.0 * (full - reduced)).max(0.0);
        let p_value = if df == 0 {
            f64::NAN
        } else {
            ChiSquared::new(df as f64).and_then(|c| c.sf(statistic)).unwrap_or(f64::NAN)
        };
        Self { name, reduced, full, statistic, df, p_value }
    }
}

/// One distinct combination of stresses.
#[derive(Debug, Clone)]
pub struct StressLevel {
    /// The stresses.
    pub stresses: Vec<f64>,
    /// Indices of the observations tested there.
    pub members: Vec<usize>,
    /// The observations.
    pub data: LifeData,
    /// The individual (per-level) fit, or why it is impossible.
    pub individual: std::result::Result<CensoredFit, String>,
}

impl StressLevel {
    /// Total frequency.
    #[must_use]
    pub fn units(&self) -> f64 {
        self.data.total_weight()
    }

    /// Frequency of units with a failure time known (not right-censored).
    #[must_use]
    pub fn failures(&self) -> f64 {
        self.data.total_weight() - self.data.weight_of(CensorKind::Right)
    }
}

/// Comparison of the life–stress model with per-level models.
#[derive(Debug, Clone)]
pub struct LevelComparison {
    /// Indices (into [`LifeStressFit::levels`]) of the levels compared.
    pub used: Vec<usize>,
    /// Log-likelihood of the individual fits (sum over the used levels).
    pub individual: f64,
    /// Log-likelihood of the common-scale model.
    pub common_scale: Option<f64>,
    /// Log-likelihood of the life–stress model refitted to the used levels.
    pub life_stress: f64,
    /// The tests.
    pub tests: Vec<LikelihoodRatioTest>,
}

/// A fitted life–stress model.
#[derive(Debug, Clone)]
pub struct LifeStressFit {
    /// The model.
    pub model: LifeStressModel,
    /// The regression fit.
    pub fit: RegressionFit,
    /// The data.
    pub data: LifeStressData,
    /// Distinct stress levels in increasing order of the first stress.
    pub levels: Vec<StressLevel>,
    /// Constant against stress-dependent scale (`None` for the exponential,
    /// or when the alternative could not be fitted).
    pub scale_test: Option<LikelihoodRatioTest>,
    /// Comparison with per-level models (`None` with fewer than two levels
    /// that can be fitted individually).
    pub level_comparison: Option<LevelComparison>,
}

fn regression_data(model: &LifeStressModel, data: &LifeStressData, rows: &[usize]) -> RegressionData {
    RegressionData {
        observations: rows.iter().map(|&i| data.observations[i]).collect(),
        points: rows.iter().map(|&i| model.point(&data.stresses[i])).collect(),
    }
}

/// Fit a life–stress model (see the module docs).
///
/// # Errors
/// [`StatError::InvalidArgument`] for a family that is not a life
/// distribution, no or more than two stresses, an interaction without two
/// stresses, mismatched lengths, a stress outside its relationship's domain
/// (temperatures at or below absolute zero, non-positive inverse-power
/// stresses), fewer distinct stress levels than the relationship needs, or
/// a likelihood without a maximum.
pub fn fit_life_stress(data: &LifeStressData, model: &LifeStressModel) -> Result<LifeStressFit> {
    if !model.family.is_log() {
        return Err(invalid("a life–stress model needs a life distribution (Weibull, lognormal, …)"));
    }
    if model.stresses.is_empty() || model.stresses.len() > 2 {
        return Err(invalid("a life–stress model has one or two stresses"));
    }
    if model.interaction && model.stresses.len() != 2 {
        return Err(invalid("an interaction needs two stresses"));
    }
    if data.observations.len() != data.stresses.len() {
        return Err(invalid("every observation needs its stresses"));
    }
    for s in &data.stresses {
        if s.len() != model.stresses.len() || !s.iter().zip(&model.stresses).all(|(v, m)| m.valid(*v)) {
            return Err(invalid(
                "every stress must suit its relationship (temperatures above absolute zero, positive \
                 inverse-power stresses)",
            ));
        }
    }
    // Distinct levels.
    let mut levels: Vec<(Vec<f64>, Vec<usize>)> = Vec::new();
    for (i, s) in data.stresses.iter().enumerate() {
        match levels.iter_mut().find(|(l, _)| l == s) {
            Some((_, m)) => m.push(i),
            None => levels.push((s.clone(), vec![i])),
        }
    }
    levels.sort_by(|a, b| {
        a.0.iter().zip(&b.0).map(|(x, y)| x.total_cmp(y)).find(|o| o.is_ne()).unwrap_or(std::cmp::Ordering::Equal)
    });
    for j in 0..model.stresses.len() {
        let mut distinct: Vec<f64> = levels.iter().map(|l| l.0[j]).collect();
        distinct.dedup();
        distinct.sort_by(f64::total_cmp);
        distinct.dedup();
        if distinct.len() < 2 {
            return Err(invalid("each stress needs at least two different levels"));
        }
    }
    let all: Vec<usize> = (0..data.observations.len()).collect();
    let fit = fit_regression(&regression_data(model, data, &all), model.family)?;
    let single = model.family.single_sample_family();
    let levels: Vec<StressLevel> = levels
        .into_iter()
        .map(|(stresses, members)| {
            let obs: Vec<LifeObservation> = members.iter().map(|&i| data.observations[i]).collect();
            let level_data = LifeData::new(obs)?;
            let individual = fit_censored(&level_data, single).map_err(|e| e.to_string());
            Ok(StressLevel { stresses, members, data: level_data, individual })
        })
        .collect::<Result<_>>()?;
    let scale_test = (!model.family.fixed_scale())
        .then(|| {
            let other = LifeStressModel { stress_dependent_scale: !model.stress_dependent_scale, ..model.clone() };
            let alt = fit_regression(&regression_data(&other, data, &all), model.family).ok()?;
            let (reduced, full) = if model.stress_dependent_scale {
                (alt.log_likelihood, fit.log_likelihood)
            } else {
                (fit.log_likelihood, alt.log_likelihood)
            };
            Some(LikelihoodRatioTest::new("Constant scale (shape) across stress", reduced, full, model.stresses.len()))
        })
        .flatten();
    let level_comparison = compare_levels(model, data, &levels, &fit);
    Ok(LifeStressFit { model: model.clone(), fit, data: data.clone(), levels, scale_test, level_comparison })
}

fn compare_levels(
    model: &LifeStressModel,
    data: &LifeStressData,
    levels: &[StressLevel],
    fit: &RegressionFit,
) -> Option<LevelComparison> {
    let used: Vec<usize> = (0..levels.len()).filter(|&i| levels[i].individual.is_ok()).collect();
    if used.len() < 2 {
        return None;
    }
    let rows: Vec<usize> = used.iter().flat_map(|&i| levels[i].members.iter().copied()).collect();
    let individual: f64 = used.iter().map(|&i| levels[i].individual.as_ref().map_or(0.0, |f| f.log_likelihood())).sum();
    // The life–stress model with a constant scale, on the used levels.
    let life_stress = if used.len() == levels.len() && !model.stress_dependent_scale {
        fit.log_likelihood
    } else {
        let m = LifeStressModel { stress_dependent_scale: false, ..model.clone() };
        fit_regression(&regression_data(&m, data, &rows), model.family).ok()?.log_likelihood
    };
    let l = used.len();
    let k = 1 + model.n_stress_terms();
    let per_level = if model.family.fixed_scale() { 1 } else { 2 };
    // A location per level, one common scale.
    let common_scale = (!model.family.fixed_scale())
        .then(|| {
            let mut obs = Vec::new();
            let mut points = Vec::new();
            for (j, &i) in used.iter().enumerate() {
                for &r in &levels[i].members {
                    obs.push(data.observations[r]);
                    let mut x = vec![0.0; l];
                    x[0] = 1.0;
                    if j > 0 {
                        x[j] = 1.0;
                    }
                    points.push(CovariatePoint { location: x, scale: vec![1.0], offset: 0.0 });
                }
            }
            fit_regression(&RegressionData { observations: obs, points }, model.family).ok().map(|f| f.log_likelihood)
        })
        .flatten();
    let mut tests = Vec::new();
    let stress_params = k + 1 - usize::from(model.family.fixed_scale());
    if let Some(c) = common_scale {
        tests.push(LikelihoodRatioTest::new("Equal scales (common shape) across levels", c, individual, l - 1));
        if l > k {
            tests.push(LikelihoodRatioTest::new("Life–stress relationship (common scale)", life_stress, c, l - k));
        }
    }
    if per_level * l > stress_params {
        tests.push(LikelihoodRatioTest::new(
            "Life–stress model against individual fits",
            life_stress,
            individual,
            per_level * l - stress_params,
        ));
    }
    Some(LevelComparison { used, individual, common_scale, life_stress, tests })
}

/// Names of the location coefficients of a model.
#[must_use]
pub fn coefficient_names(model: &LifeStressModel, stress_names: &[String]) -> Vec<String> {
    let mut out = vec!["Intercept β₀".to_owned()];
    let name = |j: usize| stress_names.get(j).cloned().unwrap_or_else(|| format!("Stress {}", j + 1));
    for (j, s) in model.stresses.iter().enumerate() {
        let what = match s.transform {
            StressTransform::Arrhenius | StressTransform::Eyring => "1/kT".to_owned(),
            StressTransform::InversePower => format!("ln {}", name(j)),
            StressTransform::Exponential => name(j),
        };
        out.push(format!("β{} ({what})", subscript(j + 1)));
    }
    if model.interaction && model.stresses.len() == 2 {
        out.push(format!("β{} (interaction)", subscript(3)));
    }
    out
}

fn subscript(n: usize) -> &'static str {
    ["₀", "₁", "₂", "₃", "₄"].get(n).copied().unwrap_or("")
}

/// A stress-related quantity derived from a coefficient.
#[derive(Debug, Clone, PartialEq)]
pub struct StressParameter {
    /// Name, e.g. "Activation energy Eₐ (eV)".
    pub name: String,
    /// Estimate with intervals.
    pub estimate: IntervalEstimate,
}

impl LifeStressFit {
    /// The covariate point at stresses `s`.
    #[must_use]
    pub fn point(&self, s: &[f64]) -> CovariatePoint {
        self.model.point(s)
    }

    /// Activation energies (temperature stresses) and power exponents
    /// (inverse-power stresses) with Wald and, when `likelihood_ratio`,
    /// profile-likelihood limits.
    #[must_use]
    pub fn stress_parameters(
        &self,
        stress_names: &[String],
        level: f64,
        likelihood_ratio: bool,
    ) -> Vec<StressParameter> {
        let mut out = Vec::new();
        if self.model.interaction {
            return out; // the coefficients no longer have these meanings alone
        }
        for (j, s) in self.model.stresses.iter().enumerate() {
            let mut a = vec![0.0; j + 2];
            let name = stress_names.get(j).cloned().unwrap_or_else(|| format!("stress {}", j + 1));
            match s.transform {
                StressTransform::Arrhenius | StressTransform::Eyring => {
                    a[j + 1] = 1.0;
                    out.push(StressParameter {
                        name: format!("Activation energy Eₐ (eV), {name}"),
                        estimate: self.fit.linear_function(&a, 0.0, level, likelihood_ratio),
                    });
                }
                StressTransform::InversePower => {
                    a[j + 1] = -1.0;
                    out.push(StressParameter {
                        name: format!("Power exponent n, {name} (life ∝ stress^−n)"),
                        estimate: self.fit.linear_function(&a, 0.0, level, likelihood_ratio),
                    });
                }
                StressTransform::Exponential => {}
            }
        }
        out
    }

    /// The acceleration factor of stresses `at` relative to `reference`
    /// (`life(reference) / life(at)`), with Wald and, when
    /// `likelihood_ratio`, profile-likelihood limits.
    #[must_use]
    pub fn acceleration_factor(
        &self,
        at: &[f64],
        reference: &[f64],
        level: f64,
        likelihood_ratio: bool,
    ) -> IntervalEstimate {
        let (pa, pr) = (self.point(at), self.point(reference));
        let a: Vec<f64> = pr.location.iter().zip(&pa.location).map(|(r, x)| r - x).collect();
        let lin = self.fit.linear_function(&a, pr.offset - pa.offset, level, likelihood_ratio);
        let exp_pair = |(a, b): (f64, f64)| (a.exp(), b.exp());
        let estimate = lin.estimate.exp();
        IntervalEstimate {
            name: "Acceleration factor",
            estimate,
            std_error: estimate * lin.std_error,
            wald: exp_pair(lin.wald),
            likelihood_ratio: lin.likelihood_ratio.map(exp_pair),
        }
    }

    /// The `p` quantile of life at stresses `s`.
    #[must_use]
    pub fn quantile(&self, s: &[f64], p: f64, level: f64, likelihood_ratio: bool) -> IntervalEstimate {
        self.fit.quantile(&self.point(s), p, level, likelihood_ratio)
    }

    /// The probability of failing by `t` at stresses `s`.
    #[must_use]
    pub fn failure_probability(&self, s: &[f64], t: f64, level: f64, likelihood_ratio: bool) -> IntervalEstimate {
        self.fit.failure_probability(&self.point(s), t, level, likelihood_ratio)
    }

    /// Scale `σ` at stresses `s` (the Weibull shape is `1/σ`).
    #[must_use]
    pub fn sigma(&self, s: &[f64]) -> f64 {
        self.fit.sigma(&self.point(s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The likelihood-ratio p-value is the χ² survival function, so strong
    /// evidence keeps its magnitude instead of rounding to zero: statistic 100
    /// on 1 df has p = 1.5239706048320995e-23 (SciPy `chi2.sf`).
    #[test]
    fn likelihood_ratio_p_value_keeps_far_tail() {
        let t = LikelihoodRatioTest::new("x", 0.0, 50.0, 1);
        assert!(t.p_value > 0.0);
        assert!((t.p_value / 1.523_970_604_832_099_5e-23 - 1.0).abs() < 1e-6, "{}", t.p_value);
    }

    #[test]
    fn transforms_and_keys() {
        let v = StressVariable::new(StressTransform::Arrhenius);
        assert!((v.transformed(0.0) - INVERSE_BOLTZMANN / 273.15).abs() < 1e-9);
        assert!(!v.valid(-300.0) && v.valid(-200.0));
        let k = StressVariable { transform: StressTransform::Arrhenius, unit: TemperatureUnit::Kelvin };
        assert!((k.transformed(300.0) - INVERSE_BOLTZMANN / 300.0).abs() < 1e-9);
        let ip = StressVariable::new(StressTransform::InversePower);
        assert!(!ip.valid(0.0) && (ip.transformed(std::f64::consts::E) - 1.0).abs() < 1e-12);
        for t in StressTransform::ALL {
            assert_eq!(StressTransform::from_key(t.key()), Some(t));
        }
        assert_eq!(StressTransform::from_key("linear"), Some(StressTransform::Exponential));
    }

    /// Two levels and two location parameters make the exponential model
    /// saturated: with complete data each level's fitted mean life is its
    /// sample mean, so the acceleration factor is the ratio of the means.
    #[test]
    fn saturated_exponential_model_reproduces_level_means() {
        let low = [120.0, 340.0, 560.0, 800.0, 950.0];
        let high = [30.0, 45.0, 80.0, 150.0, 210.0];
        let mut observations = Vec::new();
        let mut stresses = Vec::new();
        for (times, s) in [(&low, 60.0), (&high, 100.0)] {
            for &t in times {
                observations.push(LifeObservation::exact(t, 1.0));
                stresses.push(vec![s]);
            }
        }
        let model = LifeStressModel {
            family: RegressionFamily::Exponential,
            stresses: vec![StressVariable::new(StressTransform::Arrhenius)],
            interaction: false,
            stress_dependent_scale: false,
        };
        let fit = fit_life_stress(&LifeStressData { observations, stresses }, &model).unwrap();
        let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
        let mu_low = fit.fit.location(&fit.point(&[60.0]));
        let mu_high = fit.fit.location(&fit.point(&[100.0]));
        assert!((mu_low - mean(&low).ln()).abs() < 1e-6);
        assert!((mu_high - mean(&high).ln()).abs() < 1e-6);
        let af = fit.acceleration_factor(&[100.0], &[60.0], 0.95, false);
        assert!((af.estimate - mean(&low) / mean(&high)).abs() < 1e-4 * af.estimate);
        let one = fit.acceleration_factor(&[60.0], &[60.0], 0.95, false);
        assert!((one.estimate - 1.0).abs() < 1e-12);
    }
}
