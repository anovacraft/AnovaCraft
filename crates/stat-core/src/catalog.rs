//! A catalogue of named probability distributions — every distribution this
//! crate implements — with one interface for the density (or probability
//! mass), the distribution function, its complement, the quantile, the
//! moments and seeded random draws. It serves the distribution calculator,
//! random-number columns and the formula functions `dist_density`,
//! `dist_cdf`, `dist_quantile` and `random_values`.
//!
//! | Family | Parameters | Density or mass |
//! |---|---|---|
//! | Normal | mean `μ`, sd `σ` | `φ((x − μ)/σ)/σ` |
//! | Lognormal | log-mean `μ`, log-sd `σ`, threshold `γ` | `φ(z)/(σ(x − γ))`, `z = (ln(x − γ) − μ)/σ` |
//! | Exponential | mean `θ`, threshold `γ` | `e^{−(x−γ)/θ}/θ` |
//! | Gamma | shape `k`, scale `θ`, threshold `γ` | `u^{k−1}e^{−u}/(θΓ(k))`, `u = (x − γ)/θ` |
//! | Weibull | shape `β`, scale `α`, threshold `γ` | `(β/α)(u)^{β−1}e^{−u^β}`, `u = (x − γ)/α` |
//! | Logistic | location `μ`, scale `s` | `e^{−z}/(s(1 + e^{−z})²)` |
//! | Log-logistic | log-location `μ`, log-scale `σ`, threshold `γ` | logistic on `ln(x − γ)` |
//! | Smallest / largest extreme value | location `μ`, scale `σ` | `e^{±z − e^{±z}}/σ` |
//! | Uniform | low `a`, high `b` | `1/(b − a)` |
//! | Triangular | low `a`, mode `c`, high `b` | piecewise linear |
//! | Beta | shapes `a`, `b` | `x^{a−1}(1 − x)^{b−1}/B(a, b)` |
//! | Student's t, χ², F | degrees of freedom | the central densities |
//! | Noncentral t, χ², F | df and noncentrality | Poisson mixtures (below) |
//! | Studentized range | means `k`, df `ν` | derivative of the distribution function |
//! | Binomial | trials `n`, probability `p` | `C(n, x)pˣ(1 − p)^{n−x}` |
//! | Poisson | mean `m` | `e^{−m}mˣ/x!` |
//! | Hypergeometric | population `N`, successes `K`, draws `n` | `C(K, x)C(N − K, n − x)/C(N, n)` |
//!
//! (Johnson, Kotz & Balakrishnan, *Continuous Univariate Distributions*,
//! vols. 1–2, and Johnson, Kemp & Kotz, *Univariate Discrete Distributions*.)
//! The threshold forms shift the two-parameter family by `γ`.
//!
//! The noncentral densities are Poisson mixtures of central ones (J, K & B
//! vol. 2, eqs. 29.4 and 30.4): `Σⱼ wⱼ f_{χ²(k+2j)}(x)` for the χ², and
//! `Σⱼ wⱼ b(y; d₁/2 + j, d₂/2)·d₁d₂/(d₂ + d₁x)²`, `y = d₁x/(d₂ + d₁x)`, for
//! the F, with `wⱼ` the Poisson(`λ/2`) weights. The noncentral t density is
//! `f(t) = (ν/t)[F_{ν+2,δ}(t√(1 + 2/ν)) − F_{ν,δ}(t)]` for `t ≠ 0` and
//! `Γ((ν+1)/2)e^{−δ²/2}/(√(νπ)Γ(ν/2))` at zero (J, K & B vol. 2, eq. 31.15).
//! The studentized range density is the central difference of its
//! distribution function (about seven significant digits).
//!
//! **Quantiles** of a discrete distribution are the smallest `x` with
//! `F(x) ≥ p`. Quantiles without a closed form are found by bracketing and
//! bisection on the distribution function.
//!
//! **Random draws** use the seedable [`Rng`]: inversion of the quantile
//! function for families with a closed form and the discrete families, the
//! Marsaglia–Tsang (2000, *ACM TOMS* 26) method for gamma variates, and the
//! defining constructions for the others (χ² = 2·Gamma(ν/2), t = Z/√(χ²/ν),
//! F as a ratio of χ²s, beta as `X/(X + Y)` of gammas, noncentral forms by
//! adding the shift or a Poisson number of extra degrees of freedom, the
//! studentized range as the range of `k` normals over `√(χ²/ν)`).

use crate::discrete::{beta_quantile, binomial_cdf, binomial_pmf, hypergeometric_pmf, poisson_cdf, poisson_pmf};
use crate::distribution_fit::{FitFamily, FittedDistribution};
use crate::distributions::{ChiSquared, ContinuousDistribution, FisherSnedecor, Normal, StudentsT};
use crate::error::{Result, invalid};
use crate::noncentral::{NoncentralChiSquared, NoncentralF, NoncentralT, poisson_mixture};
use crate::rng::{Rng, triangular_quantile};
use crate::special::{
    beta_prefactor, gamma_prefactor, ln_beta, ln_gamma, ln_gamma_difference, regularized_incomplete_beta_pair,
};
use crate::studentized_range::StudentizedRange;

const EULER: f64 = 0.577_215_664_901_532_9;

/// One parameter of a [`Family`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParameterSpec {
    /// Display name, e.g. `"Shape β"`.
    pub name: &'static str,
    /// Key for saved files and scripts, e.g. `"shape"`.
    pub key: &'static str,
    /// A typical value (the calculator's starting value).
    pub default: f64,
    /// `true` when the value must be a whole number.
    pub integer: bool,
}

const fn p(name: &'static str, key: &'static str, default: f64) -> ParameterSpec {
    ParameterSpec { name, key, default, integer: false }
}

const fn int(name: &'static str, key: &'static str, default: f64) -> ParameterSpec {
    ParameterSpec { name, key, default, integer: true }
}

/// A named family of distributions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Family {
    /// Normal (Gaussian).
    Normal,
    /// Lognormal, with an optional threshold.
    Lognormal,
    /// Exponential, with an optional threshold.
    Exponential,
    /// Gamma, with an optional threshold.
    Gamma,
    /// Weibull, with an optional threshold.
    Weibull,
    /// Logistic.
    Logistic,
    /// Log-logistic, with an optional threshold.
    LogLogistic,
    /// Smallest extreme value (Gumbel for minima).
    SmallestExtremeValue,
    /// Largest extreme value (Gumbel for maxima).
    LargestExtremeValue,
    /// Continuous uniform.
    Uniform,
    /// Triangular.
    Triangular,
    /// Beta.
    Beta,
    /// Student's t.
    StudentsT,
    /// Chi-square.
    ChiSquared,
    /// Fisher–Snedecor F.
    F,
    /// Noncentral t.
    NoncentralT,
    /// Noncentral chi-square.
    NoncentralChiSquared,
    /// Noncentral F.
    NoncentralF,
    /// Studentized range.
    StudentizedRange,
    /// Binomial.
    Binomial,
    /// Poisson.
    Poisson,
    /// Hypergeometric.
    Hypergeometric,
}

impl Family {
    /// Every family, in the order menus list them.
    pub const ALL: [Self; 22] = [
        Self::Normal,
        Self::Lognormal,
        Self::Exponential,
        Self::Gamma,
        Self::Weibull,
        Self::Logistic,
        Self::LogLogistic,
        Self::SmallestExtremeValue,
        Self::LargestExtremeValue,
        Self::Uniform,
        Self::Triangular,
        Self::Beta,
        Self::StudentsT,
        Self::ChiSquared,
        Self::F,
        Self::NoncentralT,
        Self::NoncentralChiSquared,
        Self::NoncentralF,
        Self::StudentizedRange,
        Self::Binomial,
        Self::Poisson,
        Self::Hypergeometric,
    ];

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Lognormal => "Lognormal",
            Self::Exponential => "Exponential",
            Self::Gamma => "Gamma",
            Self::Weibull => "Weibull",
            Self::Logistic => "Logistic",
            Self::LogLogistic => "Log-logistic",
            Self::SmallestExtremeValue => "Smallest extreme value",
            Self::LargestExtremeValue => "Largest extreme value",
            Self::Uniform => "Uniform",
            Self::Triangular => "Triangular",
            Self::Beta => "Beta",
            Self::StudentsT => "Student's t",
            Self::ChiSquared => "Chi-square",
            Self::F => "F",
            Self::NoncentralT => "Noncentral t",
            Self::NoncentralChiSquared => "Noncentral chi-square",
            Self::NoncentralF => "Noncentral F",
            Self::StudentizedRange => "Studentized range",
            Self::Binomial => "Binomial",
            Self::Poisson => "Poisson",
            Self::Hypergeometric => "Hypergeometric",
        }
    }

    /// Stable key for saved files and scripts, e.g. `"weibull"`.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Lognormal => "lognormal",
            Self::Exponential => "exponential",
            Self::Gamma => "gamma",
            Self::Weibull => "weibull",
            Self::Logistic => "logistic",
            Self::LogLogistic => "log-logistic",
            Self::SmallestExtremeValue => "smallest-extreme-value",
            Self::LargestExtremeValue => "largest-extreme-value",
            Self::Uniform => "uniform",
            Self::Triangular => "triangular",
            Self::Beta => "beta",
            Self::StudentsT => "t",
            Self::ChiSquared => "chi-square",
            Self::F => "f",
            Self::NoncentralT => "noncentral-t",
            Self::NoncentralChiSquared => "noncentral-chi-square",
            Self::NoncentralF => "noncentral-f",
            Self::StudentizedRange => "studentized-range",
            Self::Binomial => "binomial",
            Self::Poisson => "poisson",
            Self::Hypergeometric => "hypergeometric",
        }
    }

    /// The family with this [`key`](Self::key).
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.key() == key)
    }

    /// `true` for distributions on the whole numbers.
    #[must_use]
    pub fn is_discrete(self) -> bool {
        matches!(self, Self::Binomial | Self::Poisson | Self::Hypergeometric)
    }

    /// The parameters, in order.
    #[must_use]
    pub fn parameters(self) -> &'static [ParameterSpec] {
        const NORMAL: &[ParameterSpec] = &[p("Mean μ", "mean", 0.0), p("Standard deviation σ", "std_dev", 1.0)];
        const LOGNORMAL: &[ParameterSpec] = &[
            p("Location μ (log scale)", "location", 0.0),
            p("Scale σ (log scale)", "scale", 1.0),
            p("Threshold γ", "threshold", 0.0),
        ];
        const EXPONENTIAL: &[ParameterSpec] = &[p("Mean θ", "mean", 1.0), p("Threshold γ", "threshold", 0.0)];
        const GAMMA: &[ParameterSpec] =
            &[p("Shape k", "shape", 2.0), p("Scale θ", "scale", 1.0), p("Threshold γ", "threshold", 0.0)];
        const WEIBULL: &[ParameterSpec] =
            &[p("Shape β", "shape", 2.0), p("Scale α", "scale", 1.0), p("Threshold γ", "threshold", 0.0)];
        const LOCATION_SCALE: &[ParameterSpec] = &[p("Location μ", "location", 0.0), p("Scale σ", "scale", 1.0)];
        const LOG_LOGISTIC: &[ParameterSpec] = &[
            p("Location μ (log scale)", "location", 0.0),
            p("Scale σ (log scale)", "scale", 0.5),
            p("Threshold γ", "threshold", 0.0),
        ];
        const UNIFORM: &[ParameterSpec] = &[p("Lower a", "low", 0.0), p("Upper b", "high", 1.0)];
        const TRIANGULAR: &[ParameterSpec] =
            &[p("Lower a", "low", 0.0), p("Mode c", "mode", 0.5), p("Upper b", "high", 1.0)];
        const BETA: &[ParameterSpec] = &[p("Shape a", "a", 2.0), p("Shape b", "b", 3.0)];
        const DF: &[ParameterSpec] = &[p("Degrees of freedom ν", "df", 5.0)];
        const F: &[ParameterSpec] = &[p("Numerator DF ν₁", "df1", 3.0), p("Denominator DF ν₂", "df2", 20.0)];
        const NONCENTRAL_T: &[ParameterSpec] =
            &[p("Degrees of freedom ν", "df", 10.0), p("Noncentrality δ", "noncentrality", 1.0)];
        const NONCENTRAL_CHI: &[ParameterSpec] =
            &[p("Degrees of freedom ν", "df", 4.0), p("Noncentrality λ", "noncentrality", 2.0)];
        const NONCENTRAL_F: &[ParameterSpec] = &[
            p("Numerator DF ν₁", "df1", 3.0),
            p("Denominator DF ν₂", "df2", 20.0),
            p("Noncentrality λ", "noncentrality", 2.0),
        ];
        const RANGE: &[ParameterSpec] =
            &[int("Number of means k", "means", 3.0), p("Degrees of freedom ν", "df", 20.0)];
        const BINOMIAL: &[ParameterSpec] = &[int("Trials n", "trials", 10.0), p("Probability p", "probability", 0.5)];
        const POISSON: &[ParameterSpec] = &[p("Mean m", "mean", 3.0)];
        const HYPERGEOMETRIC: &[ParameterSpec] = &[
            int("Population N", "population", 50.0),
            int("Successes in population K", "successes", 10.0),
            int("Draws n", "draws", 8.0),
        ];
        match self {
            Self::Normal => NORMAL,
            Self::Lognormal => LOGNORMAL,
            Self::Exponential => EXPONENTIAL,
            Self::Gamma => GAMMA,
            Self::Weibull => WEIBULL,
            Self::Logistic | Self::SmallestExtremeValue | Self::LargestExtremeValue => LOCATION_SCALE,
            Self::LogLogistic => LOG_LOGISTIC,
            Self::Uniform => UNIFORM,
            Self::Triangular => TRIANGULAR,
            Self::Beta => BETA,
            Self::StudentsT | Self::ChiSquared => DF,
            Self::F => F,
            Self::NoncentralT => NONCENTRAL_T,
            Self::NoncentralChiSquared => NONCENTRAL_CHI,
            Self::NoncentralF => NONCENTRAL_F,
            Self::StudentizedRange => RANGE,
            Self::Binomial => BINOMIAL,
            Self::Poisson => POISSON,
            Self::Hypergeometric => HYPERGEOMETRIC,
        }
    }

    /// The default parameter values.
    #[must_use]
    pub fn default_parameters(self) -> Vec<f64> {
        self.parameters().iter().map(|s| s.default).collect()
    }
}

#[derive(Debug, Clone)]
enum Inner {
    Normal(Normal),
    Fit(FittedDistribution),
    Uniform(f64, f64),
    Triangular(f64, f64, f64),
    Beta(f64, f64),
    T(StudentsT),
    Chi(ChiSquared),
    F(FisherSnedecor),
    Nct(NoncentralT),
    Ncx(NoncentralChiSquared),
    Ncf(NoncentralF),
    Range(Box<StudentizedRange>),
    Binomial(u64, f64),
    Poisson(f64),
    Hyper(u64, u64, u64),
}

/// A distribution from the catalogue with its parameter values.
#[derive(Debug, Clone)]
pub struct Distribution {
    family: Family,
    params: Vec<f64>,
    inner: Inner,
}

impl PartialEq for Distribution {
    fn eq(&self, other: &Self) -> bool {
        self.family == other.family && self.params == other.params
    }
}

fn positive(v: f64, what: &str) -> Result<f64> {
    if v > 0.0 && v.is_finite() { Ok(v) } else { Err(invalid(format!("{what} must be positive (got {v})"))) }
}

fn finite(v: f64, what: &str) -> Result<f64> {
    if v.is_finite() { Ok(v) } else { Err(invalid(format!("{what} must be a finite number (got {v})"))) }
}

fn whole(v: f64, what: &str) -> Result<u64> {
    if v >= 0.0 && v.fract() == 0.0 && v < 1e15 {
        Ok(v as u64)
    } else {
        Err(invalid(format!("{what} must be a whole number of at least 0 (got {v})")))
    }
}

fn fitted(family: FitFamily, threshold: f64, location: f64, scale: f64, shape: f64) -> Inner {
    Inner::Fit(FittedDistribution { family, threshold, location, scale, shape, log_likelihood: 0.0, n: 0 })
}

impl Distribution {
    /// `family` with `params` in the order of [`Family::parameters`]; a
    /// trailing threshold may be left out (it is then 0).
    ///
    /// # Errors
    /// [`crate::StatError::InvalidArgument`] for the wrong number of
    /// parameters or values outside their range (non-positive scales or
    /// degrees of freedom, a probability outside [0, 1], `low ≥ high`, …).
    pub fn new(family: Family, params: &[f64]) -> Result<Self> {
        let specs = family.parameters();
        let mut params = params.to_vec();
        if params.len() + 1 == specs.len() && specs.last().is_some_and(|s| s.key == "threshold") {
            params.push(0.0);
        }
        if params.len() != specs.len() {
            return Err(invalid(format!(
                "the {} distribution takes {} parameters ({}); got {}",
                family.label(),
                specs.len(),
                specs.iter().map(|s| s.key).collect::<Vec<_>>().join(", "),
                params.len()
            )));
        }
        let v = &params;
        let inner = match family {
            Family::Normal => Inner::Normal(Normal::new(finite(v[0], "the mean")?, positive(v[1], "σ")?)?),
            Family::Lognormal => fitted(
                FitFamily::Lognormal3,
                finite(v[2], "the threshold")?,
                finite(v[0], "μ")?,
                positive(v[1], "σ")?,
                1.0,
            ),
            Family::Exponential => {
                fitted(FitFamily::Exponential2, finite(v[1], "the threshold")?, 0.0, positive(v[0], "θ")?, 1.0)
            }
            Family::Gamma => fitted(
                FitFamily::Gamma3,
                finite(v[2], "the threshold")?,
                0.0,
                positive(v[1], "θ")?,
                positive(v[0], "the shape")?,
            ),
            Family::Weibull => fitted(
                FitFamily::Weibull3,
                finite(v[2], "the threshold")?,
                positive(v[1], "the scale α")?.ln(),
                1.0 / positive(v[0], "the shape β")?,
                1.0,
            ),
            Family::Logistic => {
                fitted(FitFamily::Logistic, 0.0, finite(v[0], "the location")?, positive(v[1], "the scale")?, 1.0)
            }
            Family::LogLogistic => fitted(
                FitFamily::LogLogistic3,
                finite(v[2], "the threshold")?,
                finite(v[0], "μ")?,
                positive(v[1], "σ")?,
                1.0,
            ),
            Family::SmallestExtremeValue => fitted(
                FitFamily::SmallestExtremeValue,
                0.0,
                finite(v[0], "the location")?,
                positive(v[1], "the scale")?,
                1.0,
            ),
            Family::LargestExtremeValue => fitted(
                FitFamily::LargestExtremeValue,
                0.0,
                finite(v[0], "the location")?,
                positive(v[1], "the scale")?,
                1.0,
            ),
            Family::Uniform => {
                let (a, b) = (finite(v[0], "the lower limit")?, finite(v[1], "the upper limit")?);
                if a >= b {
                    return Err(invalid("the uniform distribution needs lower < upper"));
                }
                Inner::Uniform(a, b)
            }
            Family::Triangular => {
                let (a, c, b) =
                    (finite(v[0], "the lower limit")?, finite(v[1], "the mode")?, finite(v[2], "the upper limit")?);
                if !(a < b && a <= c && c <= b) {
                    return Err(invalid("the triangular distribution needs lower ≤ mode ≤ upper and lower < upper"));
                }
                Inner::Triangular(a, c, b)
            }
            Family::Beta => Inner::Beta(positive(v[0], "the shape a")?, positive(v[1], "the shape b")?),
            Family::StudentsT => Inner::T(StudentsT::new(v[0])?),
            Family::ChiSquared => Inner::Chi(ChiSquared::new(v[0])?),
            Family::F => Inner::F(FisherSnedecor::new(v[0], v[1])?),
            Family::NoncentralT => Inner::Nct(NoncentralT::new(v[0], finite(v[1], "the noncentrality")?)?),
            Family::NoncentralChiSquared => Inner::Ncx(NoncentralChiSquared::new(v[0], v[1])?),
            Family::NoncentralF => Inner::Ncf(NoncentralF::new(v[0], v[1], v[2])?),
            Family::StudentizedRange => {
                let k = whole(v[0], "the number of means")?;
                Inner::Range(Box::new(StudentizedRange::new(k as usize, positive(v[1], "the degrees of freedom")?)?))
            }
            Family::Binomial => {
                let n = whole(v[0], "the number of trials")?;
                if !(0.0..=1.0).contains(&v[1]) {
                    return Err(invalid(format!("the probability must lie in [0, 1] (got {})", v[1])));
                }
                Inner::Binomial(n, v[1])
            }
            Family::Poisson => {
                if !(v[0] >= 0.0 && v[0].is_finite()) {
                    return Err(invalid(format!("the Poisson mean must be non-negative (got {})", v[0])));
                }
                Inner::Poisson(v[0])
            }
            Family::Hypergeometric => {
                let (n, k, d) =
                    (whole(v[0], "the population")?, whole(v[1], "the successes")?, whole(v[2], "the draws")?);
                if k > n || d > n {
                    return Err(invalid("the successes and draws cannot exceed the population"));
                }
                Inner::Hyper(n, k, d)
            }
        };
        Ok(Self { family, params, inner })
    }

    /// The family.
    #[must_use]
    pub fn family(&self) -> Family {
        self.family
    }

    /// The parameter values (with the threshold, if the family has one).
    #[must_use]
    pub fn params(&self) -> &[f64] {
        &self.params
    }

    /// Smallest and largest possible values (`±∞` for unbounded support).
    #[must_use]
    pub fn support(&self) -> (f64, f64) {
        let inf = f64::INFINITY;
        match &self.inner {
            Inner::Normal(_) | Inner::T(_) | Inner::Nct(_) => (-inf, inf),
            Inner::Fit(d) => match self.family {
                Family::Logistic | Family::SmallestExtremeValue | Family::LargestExtremeValue => (-inf, inf),
                _ => (d.threshold, inf),
            },
            Inner::Uniform(a, b) | Inner::Triangular(a, _, b) => (*a, *b),
            Inner::Beta(..) => (0.0, 1.0),
            Inner::Chi(_) | Inner::F(_) | Inner::Ncx(_) | Inner::Ncf(_) | Inner::Range(_) => (0.0, inf),
            Inner::Binomial(n, _) => (0.0, *n as f64),
            Inner::Poisson(_) => (0.0, inf),
            Inner::Hyper(n, k, d) => ((d + k).saturating_sub(*n) as f64, (*k).min(*d) as f64),
        }
    }

    /// Density at `x` (probability mass for a discrete family: zero at
    /// values that are not whole numbers).
    #[must_use]
    pub fn density(&self, x: f64) -> f64 {
        if x.is_nan() {
            return f64::NAN;
        }
        let (lo, hi) = self.support();
        if x < lo || x > hi {
            return 0.0;
        }
        match &self.inner {
            Inner::Normal(d) => d.pdf(x),
            Inner::Fit(d) => d.pdf(x),
            Inner::Uniform(a, b) => 1.0 / (b - a),
            Inner::Triangular(a, c, b) => {
                if x < *c {
                    2.0 * (x - a) / ((b - a) * (c - a))
                } else if x > *c {
                    2.0 * (b - x) / ((b - a) * (b - c))
                } else {
                    2.0 / (b - a)
                }
            }
            Inner::Beta(a, b) => beta_density(x, *a, *b),
            Inner::T(d) => d.pdf(x),
            Inner::Chi(d) => d.pdf(x),
            Inner::F(d) => d.pdf(x),
            Inner::Nct(d) => {
                let (nu, delta) = (self.params[0], self.params[1]);
                if x.abs() < 1e-8 {
                    (ln_gamma_difference(0.5 * nu, 0.5) - 0.5 * (nu * std::f64::consts::PI).ln() - 0.5 * delta * delta)
                        .exp()
                } else {
                    let wider = NoncentralT::new(nu + 2.0, delta).and_then(|w| w.cdf(x * (1.0 + 2.0 / nu).sqrt()));
                    match (wider, d.cdf(x)) {
                        (Ok(a), Ok(b)) => (nu / x * (a - b)).max(0.0),
                        _ => f64::NAN,
                    }
                }
            }
            Inner::Ncx(_) => {
                let (k, lambda) = (self.params[0], self.params[1]);
                if x == 0.0 {
                    return if k < 2.0 {
                        f64::INFINITY
                    } else if k == 2.0 {
                        0.5 * (-0.5 * lambda).exp()
                    } else {
                        0.0
                    };
                }
                poisson_mixture(lambda / 2.0, |j| Ok(chi_square_density(x, k + 2.0 * j))).unwrap_or(f64::NAN)
            }
            Inner::Ncf(_) => {
                let (d1, d2, lambda) = (self.params[0], self.params[1], self.params[2]);
                if x == 0.0 {
                    return if d1 < 2.0 {
                        f64::INFINITY
                    } else if d1 == 2.0 {
                        (-0.5 * lambda).exp()
                    } else {
                        0.0
                    };
                }
                let y = d1 * x / (d2 + d1 * x);
                let jac = d1 * d2 / (d2 + d1 * x).powi(2);
                poisson_mixture(lambda / 2.0, |j| Ok(beta_density(y, 0.5 * d1 + j, 0.5 * d2) * jac)).unwrap_or(f64::NAN)
            }
            Inner::Range(r) => {
                let h = 1e-4 * x.max(1.0);
                let lo = (x - h).max(0.0);
                match (r.cdf(x + h), r.cdf(lo)) {
                    (Ok(a), Ok(b)) => ((a - b) / (x + h - lo)).max(0.0),
                    _ => f64::NAN,
                }
            }
            // Zero trials: X = 0 with certainty (the support is {0}).
            Inner::Binomial(0, _) => 1.0,
            Inner::Binomial(n, prob) => whole_value(x).map_or(0.0, |k| binomial_pmf(k, *n, *prob).unwrap_or(f64::NAN)),
            Inner::Poisson(m) => whole_value(x).map_or(0.0, |k| poisson_pmf(k, *m).unwrap_or(f64::NAN)),
            Inner::Hyper(n, k, d) => {
                whole_value(x).map_or(0.0, |j| hypergeometric_pmf(j, *n, *k, *d).unwrap_or(f64::NAN))
            }
        }
    }

    /// `P(X ≤ x)`.
    #[must_use]
    pub fn cdf(&self, x: f64) -> f64 {
        if x.is_nan() {
            return f64::NAN;
        }
        let (lo, hi) = self.support();
        if x < lo {
            return 0.0;
        }
        if x >= hi {
            return 1.0;
        }
        let v = match &self.inner {
            Inner::Normal(d) => d.cdf(x),
            Inner::Fit(d) => Ok(d.cdf(x)),
            Inner::Uniform(a, b) => Ok((x - a) / (b - a)),
            Inner::Triangular(a, c, b) => Ok(if x <= *a {
                // Also covers mode = lower, where the formula below is 0/0.
                0.0
            } else if x <= *c {
                (x - a).powi(2) / ((b - a) * (c - a))
            } else {
                1.0 - (b - x).powi(2) / ((b - a) * (b - c))
            }),
            Inner::Beta(a, b) => regularized_incomplete_beta_pair(*a, *b, x, 1.0 - x),
            Inner::T(d) => d.cdf(x),
            Inner::Chi(d) => d.cdf(x),
            Inner::F(d) => d.cdf(x),
            Inner::Nct(d) => d.cdf(x),
            Inner::Ncx(d) => d.cdf(x),
            Inner::Ncf(d) => d.cdf(x),
            Inner::Range(r) => r.cdf(x),
            Inner::Binomial(n, prob) => binomial_cdf(x.floor() as u64, *n, *prob),
            Inner::Poisson(m) => poisson_cdf(x.floor() as u64, *m),
            Inner::Hyper(n, k, d) => {
                let top = x.floor() as u64;
                let low = self.support().0 as u64;
                Ok((low..=top).map(|j| hypergeometric_pmf(j, *n, *k, *d).unwrap_or(0.0)).sum::<f64>().min(1.0))
            }
        };
        v.map_or(f64::NAN, |p| p.clamp(0.0, 1.0))
    }

    /// `P(X > x)`, computed directly where a family's upper tail is
    /// available (so small upper-tail probabilities keep their precision).
    #[must_use]
    pub fn sf(&self, x: f64) -> f64 {
        if x.is_nan() {
            return f64::NAN;
        }
        let (lo, hi) = self.support();
        if x < lo {
            return 1.0;
        }
        if x >= hi {
            return 0.0;
        }
        let v = match &self.inner {
            Inner::Normal(d) => d.sf(x),
            Inner::Fit(d) => Ok(d.ln_sf(x).exp()),
            Inner::T(d) => d.sf(x),
            Inner::Chi(d) => d.sf(x),
            Inner::F(d) => d.sf(x),
            Inner::Nct(d) => d.sf(x),
            Inner::Ncx(d) => d.sf(x),
            Inner::Ncf(d) => d.sf(x),
            Inner::Range(r) => r.sf(x),
            Inner::Beta(a, b) => regularized_incomplete_beta_pair(*b, *a, 1.0 - x, x),
            _ => Ok(1.0 - self.cdf(x)),
        };
        v.map_or(f64::NAN, |p| p.clamp(0.0, 1.0))
    }

    /// The quantile at `p ∈ (0, 1)`: the `x` with `F(x) = p` (for a discrete
    /// family, the smallest `x` with `F(x) ≥ p`). `p = 0` and `p = 1` give
    /// the ends of the support.
    ///
    /// # Errors
    /// [`crate::StatError::InvalidArgument`] for `p ∉ [0, 1]`;
    /// [`crate::StatError::ConvergenceFailure`] if root finding fails.
    pub fn quantile(&self, p: f64) -> Result<f64> {
        if !(0.0..=1.0).contains(&p) {
            return Err(invalid(format!("the probability must lie in [0, 1] (got {p})")));
        }
        let (lo, hi) = self.support();
        if p == 0.0 {
            return Ok(lo);
        }
        if p == 1.0 {
            return Ok(hi);
        }
        match &self.inner {
            Inner::Normal(d) => d.quantile(p),
            Inner::Fit(d) => Ok(d.quantile(p)),
            Inner::Uniform(a, b) => Ok(a + p * (b - a)),
            Inner::Triangular(a, c, b) => Ok(triangular_quantile(p, *a, *c, *b)),
            Inner::Beta(a, b) => beta_quantile(*a, *b, p),
            Inner::T(d) => d.quantile(p),
            Inner::Chi(d) => d.quantile(p),
            Inner::F(d) => d.quantile(p),
            Inner::Range(r) => r.quantile(p),
            Inner::Nct(_) | Inner::Ncx(_) | Inner::Ncf(_) => self.continuous_root(p),
            Inner::Binomial(..) | Inner::Poisson(_) | Inner::Hyper(..) => Ok(self.discrete_quantile(p)),
        }
    }

    /// Bracket and bisect `F(x) = p` for a continuous family.
    fn continuous_root(&self, p: f64) -> Result<f64> {
        let (lo_s, _) = self.support();
        let scale = self.std_dev().filter(|s| s.is_finite() && *s > 0.0).unwrap_or(1.0);
        let centre = self.mean().filter(|m| m.is_finite()).unwrap_or(0.0);
        let mut lo = if lo_s.is_finite() { lo_s } else { centre - scale };
        let mut hi = centre + scale;
        let mut grow = 0;
        while self.cdf(hi) < p {
            hi = centre + (hi - centre) * 2.0 + scale;
            grow += 1;
            if grow > 200 {
                return Err(crate::StatError::ConvergenceFailure {
                    routine: "distribution quantile",
                    iterations: grow,
                });
            }
        }
        while !lo_s.is_finite() && self.cdf(lo) > p {
            lo = centre - (centre - lo) * 2.0 - scale;
            grow += 1;
            if grow > 400 {
                return Err(crate::StatError::ConvergenceFailure {
                    routine: "distribution quantile",
                    iterations: grow,
                });
            }
        }
        // Bisect geometrically while the bracket spans more than a factor of
        // two on the positive axis (roots of small-df families can lie near
        // 10⁻¹⁰⁰), shrinking a bracket that starts at 0 by 10³ per step.
        for _ in 0..600 {
            let mid = if lo == 0.0 && hi > 0.0 {
                hi * 1e-3
            } else if lo > 0.0 && hi > 2.0 * lo {
                lo.sqrt() * hi.sqrt()
            } else {
                0.5 * (lo + hi)
            };
            // No double lies strictly inside the bracket: it is as narrow as it gets.
            if mid <= lo || mid >= hi {
                return Ok(0.5 * (lo + hi));
            }
            if hi - lo <= 1e-14 * mid.abs() {
                return Ok(mid);
            }
            if self.cdf(mid) < p {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        Err(crate::StatError::ConvergenceFailure { routine: "distribution quantile", iterations: 600 })
    }

    /// Smallest whole `x` with `F(x) ≥ p`, by doubling and bisection.
    fn discrete_quantile(&self, p: f64) -> f64 {
        let (lo_s, hi_s) = self.support();
        // A little slack so rounding in F does not skip a value.
        let target = p * (1.0 - 64.0 * f64::EPSILON);
        let mut lo = lo_s;
        if self.cdf(lo) >= target {
            return lo;
        }
        let mut step = 1.0;
        let mut hi = (lo + step).min(hi_s);
        while self.cdf(hi) < target && hi < hi_s {
            lo = hi;
            step *= 2.0;
            hi = (hi + step).min(hi_s);
        }
        // Invariant: F(lo) < target ≤ F(hi).
        while hi - lo > 1.0 {
            let mid = (0.5 * (lo + hi)).floor();
            if self.cdf(mid) < target {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        hi
    }

    /// Mean (`None` when it does not exist or has no closed form).
    #[must_use]
    pub fn mean(&self) -> Option<f64> {
        let v = &self.params;
        let threshold = |m: f64| m + v.last().copied().unwrap_or(0.0);
        let m = match self.family {
            Family::Normal => v[0],
            Family::Lognormal => threshold((v[0] + 0.5 * v[1] * v[1]).exp()),
            Family::Exponential => v[0] + v[1],
            Family::Gamma => threshold(v[0] * v[1]),
            Family::Weibull => threshold(v[1] * ln_gamma(1.0 + 1.0 / v[0]).exp()),
            Family::Logistic => v[0],
            Family::LogLogistic => {
                if v[1] >= 1.0 {
                    return None;
                }
                let a = std::f64::consts::PI * v[1];
                threshold(v[0].exp() * a / a.sin())
            }
            Family::SmallestExtremeValue => v[0] - EULER * v[1],
            Family::LargestExtremeValue => v[0] + EULER * v[1],
            Family::Uniform => 0.5 * (v[0] + v[1]),
            Family::Triangular => (v[0] + v[1] + v[2]) / 3.0,
            Family::Beta => v[0] / (v[0] + v[1]),
            Family::StudentsT => {
                if v[0] <= 1.0 {
                    return None;
                }
                0.0
            }
            Family::ChiSquared => v[0],
            Family::F => {
                if v[1] <= 2.0 {
                    return None;
                }
                v[1] / (v[1] - 2.0)
            }
            Family::NoncentralT => {
                if v[0] <= 1.0 {
                    return None;
                }
                v[1] * (0.5 * v[0]).sqrt() * (-ln_gamma_difference(0.5 * (v[0] - 1.0), 0.5)).exp()
            }
            Family::NoncentralChiSquared => v[0] + v[1],
            Family::NoncentralF => {
                if v[1] <= 2.0 {
                    return None;
                }
                v[1] * (v[0] + v[2]) / (v[0] * (v[1] - 2.0))
            }
            Family::StudentizedRange => return None,
            Family::Binomial => v[0] * v[1],
            Family::Poisson => v[0],
            // An empty population has no draws: X = 0.
            Family::Hypergeometric => {
                if v[0] == 0.0 {
                    0.0
                } else {
                    v[2] * v[1] / v[0]
                }
            }
        };
        Some(m)
    }

    /// Variance (`None` when it does not exist or has no closed form).
    #[must_use]
    pub fn variance(&self) -> Option<f64> {
        let v = &self.params;
        let pi2 = std::f64::consts::PI * std::f64::consts::PI;
        let var = match self.family {
            Family::Normal => v[1] * v[1],
            Family::Lognormal => (v[1] * v[1]).exp_m1() * (2.0 * v[0] + v[1] * v[1]).exp(),
            Family::Exponential => v[0] * v[0],
            Family::Gamma => v[0] * v[1] * v[1],
            Family::Weibull => {
                let (l1, l2) = (ln_gamma(1.0 + 1.0 / v[0]), ln_gamma(1.0 + 2.0 / v[0]));
                let (g1, g2) = (l1.exp(), l2.exp());
                if g2.is_finite() {
                    v[1] * v[1] * (g2 - g1 * g1)
                } else {
                    // Shapes below about 0.01: Γ(1 + 2/k) overflows, so form
                    // λ²Γ(1 + 2/k)[1 − Γ(1 + 1/k)²/Γ(1 + 2/k)] in log space
                    // (+∞ when the variance itself exceeds the largest double).
                    (2.0 * v[1].ln() + l2 + (-(2.0 * l1 - l2).exp()).ln_1p()).exp()
                }
            }
            Family::Logistic => v[1] * v[1] * pi2 / 3.0,
            Family::LogLogistic => {
                if v[1] >= 0.5 {
                    return None;
                }
                let a = std::f64::consts::PI * v[1];
                (2.0 * v[0]).exp() * log_logistic_variance_factor(a)
            }
            Family::SmallestExtremeValue | Family::LargestExtremeValue => pi2 * v[1] * v[1] / 6.0,
            Family::Uniform => (v[1] - v[0]).powi(2) / 12.0,
            Family::Triangular => {
                let (a, c, b) = (v[0], v[1], v[2]);
                (a * a + b * b + c * c - a * b - a * c - b * c) / 18.0
            }
            Family::Beta => {
                let s = v[0] + v[1];
                v[0] * v[1] / (s * s * (s + 1.0))
            }
            Family::StudentsT => {
                if v[0] <= 2.0 {
                    return None;
                }
                v[0] / (v[0] - 2.0)
            }
            Family::ChiSquared => 2.0 * v[0],
            Family::F => {
                let (d1, d2) = (v[0], v[1]);
                if d2 <= 4.0 {
                    return None;
                }
                2.0 * d2 * d2 * (d1 + d2 - 2.0) / (d1 * (d2 - 2.0).powi(2) * (d2 - 4.0))
            }
            Family::NoncentralT => {
                if v[0] <= 2.0 {
                    return None;
                }
                let m = self.mean()?;
                v[0] * (1.0 + v[1] * v[1]) / (v[0] - 2.0) - m * m
            }
            Family::NoncentralChiSquared => 2.0 * (v[0] + 2.0 * v[1]),
            Family::NoncentralF => {
                let (d1, d2, l) = (v[0], v[1], v[2]);
                if d2 <= 4.0 {
                    return None;
                }
                2.0 * (d2 / d1).powi(2) * ((d1 + l).powi(2) + (d1 + 2.0 * l) * (d2 - 2.0))
                    / ((d2 - 2.0).powi(2) * (d2 - 4.0))
            }
            Family::StudentizedRange => return None,
            Family::Binomial => v[0] * v[1] * (1.0 - v[1]),
            Family::Poisson => v[0],
            Family::Hypergeometric => {
                let (n, k, d) = (v[0], v[1], v[2]);
                if n <= 1.0 { 0.0 } else { d * (k / n) * ((n - k) / n) * (n - d) / (n - 1.0) }
            }
        };
        Some(var)
    }

    /// Standard deviation (`None` when the variance is not available).
    #[must_use]
    pub fn std_dev(&self) -> Option<f64> {
        self.variance().map(f64::sqrt)
    }

    /// One random draw.
    pub fn sample(&self, rng: &mut Rng) -> f64 {
        let v = &self.params;
        match &self.inner {
            Inner::Normal(d) => d.mean() + d.std_dev() * rng.normal(),
            Inner::Fit(d) if self.family == Family::Gamma => d.threshold + d.scale * gamma_variate(rng, d.shape),
            Inner::Fit(d) => d.quantile(open_uniform(rng)),
            Inner::Uniform(a, b) => a + (b - a) * rng.next_f64(),
            Inner::Triangular(a, c, b) => rng.triangular(*a, *c, *b),
            Inner::Beta(a, b) => {
                let lx = ln_gamma_variate(rng, *a);
                let ly = ln_gamma_variate(rng, *b);
                ratio_from_logs(lx, ly)
            }
            Inner::T(_) => rng.normal() / (chi_square_variate(rng, v[0]) / v[0]).sqrt(),
            Inner::Chi(_) => chi_square_variate(rng, v[0]),
            Inner::F(_) => (chi_square_variate(rng, v[0]) / v[0]) / (chi_square_variate(rng, v[1]) / v[1]),
            Inner::Nct(_) => (rng.normal() + v[1]) / (chi_square_variate(rng, v[0]) / v[0]).sqrt(),
            Inner::Ncx(_) => noncentral_chi_square_variate(rng, v[0], v[1]),
            Inner::Ncf(_) => {
                (noncentral_chi_square_variate(rng, v[0], v[2]) / v[0]) / (chi_square_variate(rng, v[1]) / v[1])
            }
            Inner::Range(_) => {
                let k = v[0] as usize;
                let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
                for _ in 0..k {
                    let z = rng.normal();
                    lo = lo.min(z);
                    hi = hi.max(z);
                }
                (hi - lo) / (chi_square_variate(rng, v[1]) / v[1]).sqrt()
            }
            Inner::Binomial(..) | Inner::Poisson(_) | Inner::Hyper(..) => self.discrete_quantile(open_uniform(rng)),
        }
    }

    /// `n` random draws from a generator seeded with `seed` (the same seed
    /// always gives the same values).
    #[must_use]
    pub fn draws(&self, n: usize, seed: u64) -> Vec<f64> {
        let mut rng = Rng::new(seed);
        (0..n).map(|_| self.sample(&mut rng)).collect()
    }
}

/// `x` as a whole number, if it is one.
fn whole_value(x: f64) -> Option<u64> {
    (x >= 0.0 && x.fract() == 0.0).then_some(x as u64)
}

/// A uniform value in `(0, 1)`.
fn open_uniform(rng: &mut Rng) -> f64 {
    loop {
        let u = rng.next_f64();
        if u > 0.0 {
            return u;
        }
    }
}

fn beta_density(x: f64, a: f64, b: f64) -> f64 {
    if x <= 0.0 || x >= 1.0 {
        return if (x == 0.0 && a < 1.0) || (x == 1.0 && b < 1.0) {
            f64::INFINITY
        } else if (x == 0.0 && a == 1.0) || (x == 1.0 && b == 1.0) {
            (-ln_beta(a, b)).exp()
        } else {
            0.0
        };
    }
    beta_prefactor(a, b, x, 1.0 - x) / (x * (1.0 - x))
}

/// `2a/sin 2a − (a/sin a)²`, the log-logistic variance factor at `a = πσ`.
/// The two terms cancel to `a²/3` for small `a`, so below `a = 0.1` the Taylor
/// series (computed symbolically, through `a¹⁶`) is used instead.
fn log_logistic_variance_factor(a: f64) -> f64 {
    if a < 0.1 {
        const COEFFICIENTS: [f64; 8] = [
            1.0 / 3.0,
            11.0 / 45.0,
            38.0 / 315.0,
            247.0 / 4725.0,
            2026.0 / 93555.0,
            1_880_902.0 / 212_837_625.0,
            65476.0 / 18_243_225.0,
            236_982_223.0 / 162_820_783_125.0,
        ];
        let a2 = a * a;
        return a2 * COEFFICIENTS.iter().rev().fold(0.0, |acc, &c| acc * a2 + c);
    }
    2.0 * a / (2.0 * a).sin() - (a / a.sin()).powi(2)
}

fn chi_square_density(x: f64, k: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    let h = 0.5 * k;
    if x.is_infinite() {
        return 0.0;
    }
    gamma_prefactor(h, 0.5 * x) / x
}

/// A Gamma(`shape`, 1) variate (Marsaglia & Tsang 2000; for shapes below one,
/// `G(k + 1)·U^{1/k}`, formed in log space by [`ln_gamma_variate`]).
fn gamma_variate(rng: &mut Rng, shape: f64) -> f64 {
    if shape < 1.0 {
        return ln_gamma_variate(rng, shape).exp();
    }
    marsaglia_tsang(rng, shape)
}

/// The logarithm of a Gamma(`shape`, 1) variate. For shapes below one the boost
/// `ln G(k + 1) + ln U / k` stays finite where `U^{1/k}` would underflow, so a
/// draw only rounds to 0 when its true value is below the smallest double.
fn ln_gamma_variate(rng: &mut Rng, shape: f64) -> f64 {
    if shape < 1.0 {
        let g = ln_gamma_variate(rng, shape + 1.0);
        return g + open_uniform(rng).ln() / shape;
    }
    marsaglia_tsang(rng, shape).ln()
}

/// `x / (x + y)` from `ln x` and `ln y`, without the 0/0 of two underflowed draws.
fn ratio_from_logs(lx: f64, ly: f64) -> f64 {
    let d = lx - ly;
    if d < 0.0 {
        let e = d.exp();
        e / (1.0 + e)
    } else {
        1.0 / (1.0 + (-d).exp())
    }
}

/// Marsaglia & Tsang's squeeze method for `shape ≥ 1`.
fn marsaglia_tsang(rng: &mut Rng, shape: f64) -> f64 {
    let d = shape - 1.0 / 3.0;
    let c = 1.0 / (9.0 * d).sqrt();
    loop {
        let z = rng.normal();
        let t = 1.0 + c * z;
        if t <= 0.0 {
            continue;
        }
        let v = t * t * t;
        let u = open_uniform(rng);
        if u.ln() < 0.5 * z * z + d - d * v + d * v.ln() {
            return d * v;
        }
    }
}

fn chi_square_variate(rng: &mut Rng, df: f64) -> f64 {
    2.0 * gamma_variate(rng, 0.5 * df)
}

fn poisson_variate(rng: &mut Rng, mean: f64) -> f64 {
    if mean <= 0.0 {
        return 0.0;
    }
    let u = open_uniform(rng);
    let target = u * (1.0 - 64.0 * f64::EPSILON);
    let (mut lo, mut hi) = (-1.0_f64, 1.0_f64.max(mean.ceil()));
    while poisson_cdf(hi as u64, mean).unwrap_or(1.0) < target {
        lo = hi;
        hi *= 2.0;
    }
    while hi - lo > 1.0 {
        let mid = (0.5 * (lo + hi)).floor();
        if poisson_cdf(mid as u64, mean).unwrap_or(1.0) < target {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    hi
}

/// Noncentral χ² as a central χ² with a Poisson(λ/2) number of extra pairs
/// of degrees of freedom.
fn noncentral_chi_square_variate(rng: &mut Rng, df: f64, lambda: f64) -> f64 {
    let j = poisson_variate(rng, 0.5 * lambda);
    chi_square_variate(rng, df + 2.0 * j)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) {
        assert!((a - b).abs() <= tol * b.abs().max(1e-300), "{a} vs {b} (tolerance {tol})");
    }

    fn d(f: Family, p: &[f64]) -> Distribution {
        Distribution::new(f, p).unwrap()
    }

    /// Reference values from SciPy 1.18 (`scipy.stats`): `norm`, `lognorm(s,
    /// loc=γ, scale=e^μ)`, `expon(loc, scale)`, `gamma(k, loc, scale)`,
    /// `weibull_min(β, loc, scale)`, `logistic`, `fisk(c=1/σ, loc, scale=e^μ)`,
    /// `gumbel_l`, `gumbel_r`, `uniform`, `triang`, `beta`, `t`, `chi2`, `f`,
    /// `nct`, `ncx2`, `ncf`, `studentized_range`, `binom`, `poisson`,
    /// `hypergeom` — each as (pdf or pmf, cdf, sf, quantile at 0.9).
    #[test]
    fn matches_scipy() {
        #[rustfmt::skip]
        let cases: Vec<(Family, Vec<f64>, f64, [f64; 4])> = vec![
            (Family::Normal, vec![10.0, 2.0], 12.5, [0.09132454269451096, 0.8943502263331446, 0.10564977366685535, 12.5631031310892]),
            (Family::Lognormal, vec![1.0, 0.5, 2.0], 5.0, [0.26083887270192596, 0.5781741008028732, 0.42182589919712676, 7.159170355622591]),
            (Family::Exponential, vec![3.0, 1.0], 4.0, [0.12262648039048078, 0.6321205588285577, 0.36787944117144233, 7.907755278982138]),
            (Family::Gamma, vec![2.5, 1.5, 0.0], 3.0, [0.19196788093577974, 0.4505840486472198, 0.5494159513527802, 6.927267674835843]),
            (Family::Weibull, vec![1.8, 4.0, 1.0], 5.0, [0.16554574852714907, 0.6321205588285577, 0.36787944117144233, 7.357566762218052]),
            (Family::Logistic, vec![1.0, 2.0], 0.0, [0.11750185610079725, 0.3775406687981454, 0.6224593312018546, 5.394449154672439]),
            (Family::LogLogistic, vec![0.5, 0.4, 0.0], 2.0, [0.2949694375153302, 0.6184248706469033, 0.3815751293530967, 3.9704912632473985]),
            (Family::SmallestExtremeValue, vec![2.0, 1.5], 1.0, [0.20483532941045005, 0.4015528841449275, 0.5984471158550725, 3.2510486678719337]),
            (Family::LargestExtremeValue, vec![2.0, 1.5], 3.0, [0.20483532941045005, 0.5984471158550725, 0.4015528841449275, 5.375550990968668]),
            (Family::Uniform, vec![-1.0, 3.0], 0.5, [0.25, 0.375, 0.625, 2.6]),
            (Family::Triangular, vec![0.0, 1.0, 4.0], 2.0, [0.3333333333333333, 0.6666666666666666, 0.33333333333333337, 2.904554884989668]),
            (Family::Beta, vec![2.0, 3.0], 0.3, [1.7639999999999991, 0.34829999999999994, 0.6517000000000001, 0.6795394162781817]),
            (Family::StudentsT, vec![5.0], 1.2, [0.17765861346493547, 0.8580544716469487, 0.14194552835305124, 1.4758840488244815]),
            (Family::ChiSquared, vec![4.0], 3.0, [0.1673476201113224, 0.4421745996289252, 0.5578254003710748, 7.779440339734858]),
            (Family::F, vec![3.0, 20.0], 1.5, [0.25517341781194286, 0.7549479884060327, 0.24505201159396733, 2.380087051069607]),
            (Family::NoncentralT, vec![10.0, 1.0], 1.5, [0.3247437676744484, 0.6695168482153548, 0.3304831517846451, 2.5260798970601854]),
            (Family::NoncentralChiSquared, vec![4.0, 2.0], 5.0, [0.10924297211344287, 0.48196384244277046, 0.5180361575572296, 11.391022209808877]),
            (Family::NoncentralF, vec![3.0, 20.0, 2.0], 1.5, [0.3034532000841445, 0.5236935288157636, 0.4763064711842365, 3.8527370270589234]),
            (Family::StudentizedRange, vec![3.0, 20.0], 3.0, [0.14495759021064072, 0.8892432021469359, 0.11075679785306414, 3.0774800149348143]),
            (Family::Binomial, vec![10.0, 0.3], 3.0, [0.2668279319999998, 0.6496107184000002, 0.3503892815999998, 5.0]),
            (Family::Poisson, vec![3.0], 2.0, [0.22404180765538775, 0.42319008112684364, 0.5768099188731566, 5.0]),
            (Family::Hypergeometric, vec![50.0, 10.0, 8.0], 2.0, [0.32172465789056803, 0.8122270926586482, 0.18777290734135177, 3.0]),
        ];
        for (family, params, x, [pdf, cdf, sf, q]) in cases {
            let dist = d(family, &params);
            let tol = if family == Family::StudentizedRange { 2e-6 } else { 1e-9 };
            close(dist.density(x), pdf, tol);
            close(dist.cdf(x), cdf, if family == Family::StudentizedRange { 1e-8 } else { 1e-11 });
            close(dist.sf(x), sf, if family == Family::StudentizedRange { 1e-7 } else { 1e-10 });
            close(dist.quantile(0.9).unwrap(), q, 1e-8);
        }
    }

    /// Means and variances against SciPy 1.18's `stats()` (closed forms).
    #[test]
    fn moments_match_scipy() {
        let cases: Vec<(Family, Vec<f64>, f64, f64)> = vec![
            (Family::Lognormal, vec![1.0, 0.5, 2.0], 5.080216848918031, 2.694758124344946),
            (Family::Weibull, vec![1.8, 4.0, 1.0], 4.557146929808852, 4.1816452540121585),
            (Family::LogLogistic, vec![0.5, 0.4, 0.0], 2.178465966260746, 6.877216945249797),
            (Family::NoncentralT, vec![10.0, 1.0], 1.083722307939144, 1.3255459592750551),
            (Family::NoncentralF, vec![3.0, 20.0, 2.0], 1.8518518518518519, 2.5891632373113853),
            (Family::Hypergeometric, vec![50.0, 10.0, 8.0], 1.6, 1.0971428571428572),
        ];
        for (family, params, m, v) in cases {
            let dist = d(family, &params);
            close(dist.mean().unwrap(), m, 1e-12);
            close(dist.variance().unwrap(), v, 1e-12);
        }
        assert!(d(Family::StudentsT, &[1.0]).mean().is_none());
        assert!(d(Family::StudentizedRange, &[3.0, 10.0]).mean().is_none());
    }

    /// The density integrates to the distribution function (Simpson's rule
    /// on a fine grid), and quantiles invert the distribution function.
    #[test]
    fn densities_integrate_and_quantiles_invert() {
        for family in Family::ALL {
            let dist = d(family, &family.default_parameters());
            if family.is_discrete() {
                let total: f64 = (0..=200).map(|k| dist.density(f64::from(k))).sum();
                close(total, 1.0, 1e-10);
                for p in [0.01, 0.3, 0.5, 0.77, 0.99] {
                    let q = dist.quantile(p).unwrap();
                    assert!(dist.cdf(q) >= p - 1e-12 && dist.cdf(q - 1.0) < p, "{family:?} p={p} q={q}");
                }
                continue;
            }
            let (a, b) = (dist.quantile(0.05).unwrap(), dist.quantile(0.8).unwrap());
            let n = if family == Family::StudentizedRange { 200 } else { 2000 };
            let h = (b - a) / f64::from(n);
            let mut s = dist.density(a) + dist.density(b);
            for i in 1..n {
                s += dist.density(a + h * f64::from(i)) * if i % 2 == 1 { 4.0 } else { 2.0 };
            }
            let tol = if family == Family::StudentizedRange { 1e-5 } else { 1e-7 };
            close(s * h / 3.0, 0.75, tol);
            for p in [0.001, 0.25, 0.5, 0.9, 0.999] {
                close(dist.cdf(dist.quantile(p).unwrap()), p, 1e-8);
            }
            close(dist.cdf(1.3) + dist.sf(1.3), 1.0, 1e-12);
        }
    }

    /// Seeded draws repeat, and their averages and spread agree with the
    /// moments (20 000 draws; tolerances of about five standard errors).
    #[test]
    fn draws_are_reproducible_and_follow_the_distribution() {
        for family in Family::ALL {
            let dist = d(family, &family.default_parameters());
            let x = dist.draws(20_000, 11);
            assert_eq!(x, dist.draws(20_000, 11), "{family:?}");
            assert_ne!(x, dist.draws(20_000, 12), "{family:?}");
            let (lo, hi) = dist.support();
            assert!(x.iter().all(|v| *v >= lo && *v <= hi), "{family:?}");
            // The proportion below the median is one half.
            let median = dist.quantile(0.5).unwrap();
            let below = x.iter().filter(|v| **v < median).count() as f64 / 20_000.0;
            let at_most = x.iter().filter(|v| **v <= median).count() as f64 / 20_000.0;
            assert!(below <= 0.5 + 0.02 && at_most >= 0.5 - 0.02, "{family:?}: {below} {at_most}");
            if let (Some(m), Some(v)) = (dist.mean(), dist.variance()) {
                let mean = x.iter().sum::<f64>() / 20_000.0;
                assert!((mean - m).abs() < 5.0 * (v / 20_000.0).sqrt() + 1e-12, "{family:?}: {mean} vs {m}");
            }
        }
    }

    #[test]
    fn invalid_parameters_are_rejected() {
        assert!(Distribution::new(Family::Normal, &[0.0, 0.0]).is_err());
        assert!(Distribution::new(Family::Normal, &[0.0]).is_err());
        assert!(Distribution::new(Family::Uniform, &[2.0, 1.0]).is_err());
        assert!(Distribution::new(Family::Binomial, &[2.5, 0.5]).is_err());
        assert!(Distribution::new(Family::Binomial, &[5.0, 1.5]).is_err());
        assert!(Distribution::new(Family::Hypergeometric, &[5.0, 6.0, 2.0]).is_err());
        // A threshold may be left out.
        assert_eq!(d(Family::Weibull, &[2.0, 1.0]).params(), &[2.0, 1.0, 0.0]);
        assert_eq!(Family::from_key("noncentral-f"), Some(Family::NoncentralF));
        assert!(d(Family::Normal, &[0.0, 1.0]).quantile(1.5).is_err());
        assert_eq!(d(Family::Exponential, &[1.0]).quantile(0.0).unwrap(), 0.0);
        assert_eq!(d(Family::Poisson, &[2.0]).density(1.5), 0.0);
    }

    /// Boundary cases, from the definitions: a triangular distribution with
    /// mode = lower has `F(x) = 1 − ((b − x)/(b − a))²`, so `F(a) = 0`; an
    /// exponential (or a Weibull or gamma with shape 1) has density `1/θ` at
    /// its threshold (SciPy 1.18 `expon.pdf(0, scale=2)` = 0.5); zero
    /// binomial trials or an empty hypergeometric population give `X = 0`.
    #[test]
    fn boundary_values() {
        let t = d(Family::Triangular, &[0.0, 0.0, 1.0]);
        assert_eq!((t.cdf(0.0), t.sf(0.0)), (0.0, 1.0));
        close(t.cdf(0.5), 0.75, 1e-15);
        let t = d(Family::Triangular, &[0.0, 1.0, 1.0]);
        assert_eq!((t.cdf(0.0), t.cdf(1.0)), (0.0, 1.0));
        close(t.cdf(0.5), 0.25, 1e-15);

        close(d(Family::Exponential, &[2.0, 1.0]).density(1.0), 0.5, 1e-15);
        close(d(Family::Weibull, &[1.0, 2.0]).density(0.0), 0.5, 1e-15);
        close(d(Family::Gamma, &[1.0, 2.0]).density(0.0), 0.5, 1e-15);
        assert_eq!(d(Family::Weibull, &[0.5, 2.0]).density(0.0), f64::INFINITY);
        assert_eq!(d(Family::Gamma, &[0.5, 2.0]).density(0.0), f64::INFINITY);
        assert_eq!(d(Family::Weibull, &[2.0, 2.0]).density(0.0), 0.0);
        assert_eq!(d(Family::Gamma, &[3.0, 2.0]).density(0.0), 0.0);
        assert_eq!(d(Family::Lognormal, &[0.0, 1.0]).density(0.0), 0.0);
        close(d(Family::LogLogistic, &[0.5, 1.0]).density(0.0), (-0.5f64).exp(), 1e-15);

        let b = d(Family::Binomial, &[0.0, 0.3]);
        assert_eq!(
            (b.density(0.0), b.density(1.0), b.cdf(0.0), b.mean(), b.variance()),
            (1.0, 0.0, 1.0, Some(0.0), Some(0.0))
        );
        assert_eq!(b.quantile(0.5).unwrap(), 0.0);
        let h = d(Family::Hypergeometric, &[0.0, 0.0, 0.0]);
        assert_eq!((h.mean(), h.variance(), h.cdf(0.0)), (Some(0.0), Some(0.0), 1.0));
        close(h.density(0.0), 1.0, 1e-14);
    }
}
