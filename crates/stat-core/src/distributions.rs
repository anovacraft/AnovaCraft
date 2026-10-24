//! Continuous sampling distributions used for inference.
//!
//! All functions are written from their canonical definitions in terms of the
//! regularized incomplete beta function `I_x(a, b)` (DLMF §8.17; Abramowitz &
//! Stegun 26.5.27 and 26.6.2):
//!
//! * Student's t with ν degrees of freedom, for `t ≥ 0`:
//!   `P(T > t) = ½ I_{ν/(ν+t²)}(ν/2, ½)`.
//! * Fisher–Snedecor F with (d₁, d₂) degrees of freedom:
//!   `P(F > f) = I_{d₂/(d₂+d₁f)}(d₂/2, d₁/2)`.
//!
//! * Chi-square with k degrees of freedom:
//!   `P(X > x) = Q(k/2, x/2)`, the regularized upper incomplete gamma function.
//! * Normal with mean μ and standard deviation σ:
//!   `P(X > x) = ½ erfc((x − μ)/(σ√2))` (DLMF 7.18 / A&S 26.2.29).
//!
//! Upper-tail probabilities are computed directly (never as `1 − cdf`) so that
//! small p-values keep full relative precision.

use crate::error::{Result, StatError, invalid};
use crate::special::{
    beta_prefactor, erfc, gamma_prefactor, ln_gamma_difference, regularized_gamma_p, regularized_gamma_q,
    regularized_incomplete_beta_pair,
};

/// Common interface for univariate continuous distributions.
pub trait ContinuousDistribution {
    /// Probability density at `x`.
    fn pdf(&self, x: f64) -> f64;
    /// Cumulative distribution function `P(X ≤ x)`.
    ///
    /// # Errors
    /// Propagates numerical failures from the underlying special functions.
    fn cdf(&self, x: f64) -> Result<f64>;
    /// Survival function `P(X > x)`, computed without cancellation.
    ///
    /// # Errors
    /// Propagates numerical failures from the underlying special functions.
    fn sf(&self, x: f64) -> Result<f64>;
    /// Quantile function: the `x` with `P(X ≤ x) = p`.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] if `p ∉ (0, 1)`;
    /// [`StatError::ConvergenceFailure`] if root finding fails.
    fn quantile(&self, p: f64) -> Result<f64>;
}

fn check_df(name: &str, df: f64) -> Result<()> {
    if df > 0.0 && df.is_finite() {
        Ok(())
    } else {
        Err(invalid(format!("{name} degrees of freedom must be positive and finite (got {df})")))
    }
}

fn check_probability(p: f64) -> Result<()> {
    if p > 0.0 && p < 1.0 {
        Ok(())
    } else {
        Err(invalid(format!("probability must lie strictly between 0 and 1 (got {p})")))
    }
}

/// Student's t distribution with `ν > 0` degrees of freedom.
///
/// Density: `f(t) = Γ((ν+1)/2) / (√(νπ) Γ(ν/2)) · (1 + t²/ν)^{−(ν+1)/2}`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StudentsT {
    df: f64,
}

impl StudentsT {
    /// Construct with `df` degrees of freedom.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] unless `df` is positive and finite.
    pub fn new(df: f64) -> Result<Self> {
        check_df("t", df)?;
        Ok(Self { df })
    }

    /// Degrees of freedom.
    #[must_use]
    pub fn df(&self) -> f64 {
        self.df
    }

    /// Two-sided p-value `P(|T| ≥ |t|)` for a test statistic `t`.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] if `t` is NaN.
    pub fn two_sided_p_value(&self, t: f64) -> Result<f64> {
        if t.is_nan() {
            return Err(StatError::NonFiniteInput { context: "t statistic" });
        }
        if t.is_infinite() {
            return Ok(0.0);
        }
        let (x, y) = self.beta_args(t);
        regularized_incomplete_beta_pair(self.df / 2.0, 0.5, x, y)
    }

    /// `(ν/(ν+t²), t²/(ν+t²))`, each formed without cancellation. When `t²`
    /// overflows the pair is `(ν/t², 1)` to working precision.
    fn beta_args(&self, t: f64) -> (f64, f64) {
        let t2 = t * t;
        if !t2.is_finite() {
            return (self.df / t.abs() / t.abs(), 1.0);
        }
        let denom = self.df + t2;
        (self.df / denom, t2 / denom)
    }
}

impl ContinuousDistribution for StudentsT {
    fn pdf(&self, t: f64) -> f64 {
        let v = self.df;
        let log_norm = ln_gamma_difference(0.5 * v, 0.5) - 0.5 * (v * std::f64::consts::PI).ln();
        (log_norm - 0.5 * (v + 1.0) * (t * t / v).ln_1p()).exp()
    }

    fn cdf(&self, t: f64) -> Result<f64> {
        self.sf(-t)
    }

    fn sf(&self, t: f64) -> Result<f64> {
        if t.is_nan() {
            return Err(StatError::NonFiniteInput { context: "t value" });
        }
        let half_two_sided = 0.5 * self.two_sided_p_value(t)?;
        Ok(if t >= 0.0 { half_two_sided } else { 1.0 - half_two_sided })
    }

    fn quantile(&self, p: f64) -> Result<f64> {
        check_probability(p)?;
        if p == 0.5 {
            return Ok(0.0);
        }
        // Closed forms for ν = 1 (Cauchy) and ν = 2. In the Cauchy tails
        // tan(π(p − ½)) = −1/tan(πp) avoids the cancellation in p − ½.
        if self.df == 1.0 {
            let pi = std::f64::consts::PI;
            return Ok(if p < 0.25 {
                -1.0 / (pi * p).tan()
            } else if p > 0.75 {
                1.0 / (pi * (1.0 - p)).tan()
            } else {
                (pi * (p - 0.5)).tan()
            });
        }
        if self.df == 2.0 {
            return Ok((2.0 * p - 1.0) / (2.0 * p * (1.0 - p)).sqrt());
        }
        let upper = p.min(1.0 - p);
        let t = invert_survival(self, upper, 0.0)?;
        Ok(if p > 0.5 { t } else { -t })
    }
}

/// Fisher–Snedecor F distribution with `(d₁, d₂)` degrees of freedom.
///
/// Density: `f(x) = √((d₁x)^{d₁} d₂^{d₂} / (d₁x + d₂)^{d₁+d₂}) / (x B(d₁/2, d₂/2))`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FisherSnedecor {
    df1: f64,
    df2: f64,
}

impl FisherSnedecor {
    /// Construct with numerator `df1` and denominator `df2` degrees of freedom.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] unless both are positive and finite.
    pub fn new(df1: f64, df2: f64) -> Result<Self> {
        check_df("F numerator", df1)?;
        check_df("F denominator", df2)?;
        Ok(Self { df1, df2 })
    }

    /// Numerator degrees of freedom.
    #[must_use]
    pub fn df1(&self) -> f64 {
        self.df1
    }

    /// Denominator degrees of freedom.
    #[must_use]
    pub fn df2(&self) -> f64 {
        self.df2
    }

    /// `(d₂/(d₂+d₁f), d₁f/(d₂+d₁f))`, each formed without cancellation. When
    /// `d₁f` overflows the pair is `(d₂/(d₁f), 1)` to working precision.
    fn beta_args(&self, f: f64) -> (f64, f64) {
        let d1f = self.df1 * f;
        let denom = self.df2 + d1f;
        if !denom.is_finite() {
            return (self.df2 / self.df1 / f, 1.0);
        }
        (self.df2 / denom, d1f / denom)
    }
}

impl ContinuousDistribution for FisherSnedecor {
    fn pdf(&self, x: f64) -> f64 {
        if x < 0.0 {
            return 0.0;
        }
        let (d1, d2) = (self.df1, self.df2);
        if x == 0.0 {
            return match d1.partial_cmp(&2.0) {
                Some(std::cmp::Ordering::Less) => f64::INFINITY,
                Some(std::cmp::Ordering::Equal) => 1.0,
                _ => 0.0,
            };
        }
        if x.is_infinite() {
            return 0.0;
        }
        // With u = d₁x/(d₁x + d₂), the density is the beta(d₁/2, d₂/2)
        // prefactor uᵃ(1 − u)ᵇ/B(a, b) divided by x.
        let (u_complement, u) = self.beta_args(x);
        beta_prefactor(0.5 * d1, 0.5 * d2, u, u_complement) / x
    }

    fn cdf(&self, f: f64) -> Result<f64> {
        if f.is_nan() {
            return Err(StatError::NonFiniteInput { context: "F value" });
        }
        if f <= 0.0 {
            return Ok(0.0);
        }
        if f.is_infinite() {
            return Ok(1.0);
        }
        let (x, y) = self.beta_args(f);
        regularized_incomplete_beta_pair(0.5 * self.df1, 0.5 * self.df2, y, x)
    }

    fn sf(&self, f: f64) -> Result<f64> {
        if f.is_nan() {
            return Err(StatError::NonFiniteInput { context: "F value" });
        }
        if f <= 0.0 {
            return Ok(1.0);
        }
        if f.is_infinite() {
            return Ok(0.0);
        }
        let (x, y) = self.beta_args(f);
        regularized_incomplete_beta_pair(0.5 * self.df2, 0.5 * self.df1, x, y)
    }

    fn quantile(&self, p: f64) -> Result<f64> {
        check_probability(p)?;
        if p < 0.5 { invert_cdf_positive(self, p) } else { invert_survival(self, 1.0 - p, 0.0) }
    }
}

/// Normal (Gaussian) distribution with mean `μ` and standard deviation `σ > 0`.
///
/// Density: `f(x) = exp(−(x − μ)²/(2σ²)) / (σ√(2π))`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Normal {
    mean: f64,
    std_dev: f64,
}

impl Normal {
    /// Construct with mean `mean` and standard deviation `std_dev`.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] unless `mean` is finite and `std_dev` is
    /// positive and finite.
    pub fn new(mean: f64, std_dev: f64) -> Result<Self> {
        if !mean.is_finite() || !(std_dev > 0.0 && std_dev.is_finite()) {
            return Err(invalid(format!("normal requires finite mean and positive finite sd (got {mean}, {std_dev})")));
        }
        Ok(Self { mean, std_dev })
    }

    /// The standard normal distribution, `N(0, 1)`.
    #[must_use]
    pub fn standard() -> Self {
        Self { mean: 0.0, std_dev: 1.0 }
    }

    /// Mean `μ`.
    #[must_use]
    pub fn mean(&self) -> f64 {
        self.mean
    }

    /// Standard deviation `σ`.
    #[must_use]
    pub fn std_dev(&self) -> f64 {
        self.std_dev
    }

    fn z(&self, x: f64) -> f64 {
        (x - self.mean) / self.std_dev
    }
}

impl ContinuousDistribution for Normal {
    fn pdf(&self, x: f64) -> f64 {
        let z = self.z(x);
        (-0.5 * z * z).exp() / (self.std_dev * (2.0 * std::f64::consts::PI).sqrt())
    }

    fn cdf(&self, x: f64) -> Result<f64> {
        if x.is_nan() {
            return Err(StatError::NonFiniteInput { context: "normal value" });
        }
        Ok(0.5 * erfc(-self.z(x) * std::f64::consts::FRAC_1_SQRT_2))
    }

    fn sf(&self, x: f64) -> Result<f64> {
        if x.is_nan() {
            return Err(StatError::NonFiniteInput { context: "normal value" });
        }
        Ok(0.5 * erfc(self.z(x) * std::f64::consts::FRAC_1_SQRT_2))
    }

    fn quantile(&self, p: f64) -> Result<f64> {
        check_probability(p)?;
        if p == 0.5 {
            return Ok(self.mean);
        }
        // Solve on the standard normal's upper half, then use symmetry.
        let z = invert_survival(&Self::standard(), p.min(1.0 - p), 0.0)?;
        Ok(self.mean + self.std_dev * if p > 0.5 { z } else { -z })
    }
}

/// Chi-square distribution with `k > 0` degrees of freedom.
///
/// Density: `f(x) = x^{k/2−1} e^{−x/2} / (2^{k/2} Γ(k/2))` for `x > 0`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChiSquared {
    df: f64,
}

impl ChiSquared {
    /// Construct with `df` degrees of freedom.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] unless `df` is positive and finite.
    pub fn new(df: f64) -> Result<Self> {
        check_df("chi-square", df)?;
        Ok(Self { df })
    }

    /// Degrees of freedom.
    #[must_use]
    pub fn df(&self) -> f64 {
        self.df
    }
}

impl ContinuousDistribution for ChiSquared {
    fn pdf(&self, x: f64) -> f64 {
        if x < 0.0 {
            return 0.0;
        }
        let k = self.df;
        if x == 0.0 {
            return match k.partial_cmp(&2.0) {
                Some(std::cmp::Ordering::Less) => f64::INFINITY,
                Some(std::cmp::Ordering::Equal) => 0.5,
                _ => 0.0,
            };
        }
        if x.is_infinite() {
            return 0.0;
        }
        gamma_prefactor(0.5 * k, 0.5 * x) / x
    }

    fn cdf(&self, x: f64) -> Result<f64> {
        if x.is_nan() {
            return Err(StatError::NonFiniteInput { context: "chi-square value" });
        }
        if x <= 0.0 {
            return Ok(0.0);
        }
        if x.is_infinite() {
            return Ok(1.0);
        }
        regularized_gamma_p(0.5 * self.df, 0.5 * x)
    }

    fn sf(&self, x: f64) -> Result<f64> {
        if x.is_nan() {
            return Err(StatError::NonFiniteInput { context: "chi-square value" });
        }
        if x <= 0.0 {
            return Ok(1.0);
        }
        if x.is_infinite() {
            return Ok(0.0);
        }
        regularized_gamma_q(0.5 * self.df, 0.5 * x)
    }

    fn quantile(&self, p: f64) -> Result<f64> {
        check_probability(p)?;
        if p < 0.5 { invert_cdf_positive(self, p) } else { invert_survival(self, 1.0 - p, 0.0) }
    }
}

/// Find `x > 0` with `cdf(x) = target` (`target < ½`) for a distribution on
/// `(0, ∞)` whose distribution function is strictly increasing.
///
/// Lower-tail quantiles are solved on the distribution function itself, never
/// as `sf(x) = 1 − p`: `1 − p` keeps only absolute precision (it is exactly 1
/// for `p < 1.1·10⁻¹⁶`), and the root may be far below 1 (for χ² with 0.02 df
/// the 5% point is about 10⁻¹³⁰). The bracket shrinks geometrically, and
/// Newton steps are safeguarded by bisection on `log x`. A root below the
/// smallest normal number is returned as 0.
fn invert_cdf_positive<D: ContinuousDistribution>(dist: &D, target: f64) -> Result<f64> {
    const MAX_ITER: usize = 400;
    let mut hi = 1.0_f64;
    let mut grow = 0;
    while dist.cdf(hi)? < target {
        hi *= 2.0;
        grow += 1;
        if grow > 2_000 || !hi.is_finite() {
            return Err(StatError::ConvergenceFailure { routine: "quantile bracketing", iterations: grow });
        }
    }
    let mut lo = hi;
    loop {
        if dist.cdf(lo)? <= target {
            break;
        }
        hi = lo;
        if lo <= f64::MIN_POSITIVE {
            return Ok(0.0);
        }
        lo = (lo * 1e-4).max(f64::MIN_POSITIVE);
    }
    if lo == hi {
        return Ok(lo);
    }
    // Bisect on log x while the bracket spans more than a factor of two
    // (√lo·√hi, since lo·hi can underflow), arithmetically after that.
    let midpoint = |lo: f64, hi: f64| if hi > 2.0 * lo { lo.sqrt() * hi.sqrt() } else { 0.5 * (lo + hi) };
    let mut x = midpoint(lo, hi);
    let mut width = hi - lo;
    for _ in 0..MAX_ITER {
        let g = dist.cdf(x)? - target;
        if g == 0.0 {
            return Ok(x);
        }
        if g < 0.0 {
            lo = x;
        } else {
            hi = x;
        }
        let density = dist.pdf(x);
        let newton = if density > 0.0 && density.is_finite() { x - g / density } else { f64::NAN };
        // Fall back to bisection when Newton leaves the bracket or stalls
        // (shrinks it by less than half), e.g. on rounding noise in the cdf.
        let stalled = hi - lo > 0.5 * width;
        width = hi - lo;
        let next = if !stalled && newton.is_finite() && newton > lo && newton < hi { newton } else { midpoint(lo, hi) };
        if (next - x).abs() <= 4.0 * f64::EPSILON * next || hi - lo <= 4.0 * f64::EPSILON * hi {
            return Ok(next);
        }
        x = next;
    }
    Err(StatError::ConvergenceFailure { routine: "quantile root finding", iterations: MAX_ITER })
}

/// Find `x ≥ lower` with `sf(x) = target` for a distribution whose survival
/// function is strictly decreasing on `[lower, ∞)`.
///
/// Uses Newton's method on `sf` (derivative `−pdf`), safeguarded by bisection on a
/// bracket that is first grown geometrically.
fn invert_survival<D: ContinuousDistribution>(dist: &D, target: f64, lower: f64) -> Result<f64> {
    const MAX_ITER: usize = 400;
    let mut lo = lower;
    let mut hi = lower + 1.0;
    let mut grow = 0;
    while dist.sf(hi)? > target {
        lo = hi;
        hi = lower + 2.0 * (hi - lower);
        grow += 1;
        if grow > 2_000 || !hi.is_finite() {
            return Err(StatError::ConvergenceFailure { routine: "quantile bracketing", iterations: grow });
        }
    }
    // The bisection fallback is geometric while the bracket spans more than a
    // factor of two on the positive axis, and shrinks a bracket that starts at
    // 0 by 10³ per step, so roots near 10⁻²⁰⁰ (χ² with 0.002 df) are reached.
    let midpoint = |lo: f64, hi: f64| {
        if lo == 0.0 && hi > 0.0 {
            hi * 1e-3
        } else if lo > 0.0 && hi > 2.0 * lo {
            lo.sqrt() * hi.sqrt()
        } else {
            0.5 * (lo + hi)
        }
    };
    let mut x = midpoint(lo, hi);
    for _ in 0..MAX_ITER {
        let g = dist.sf(x)? - target;
        if g == 0.0 {
            return Ok(x);
        }
        if g > 0.0 {
            lo = x;
        } else {
            hi = x;
        }
        let density = dist.pdf(x);
        let newton = if density > 0.0 { x + g / density } else { f64::NAN };
        let next = if newton.is_finite() && newton > lo && newton < hi { newton } else { midpoint(lo, hi) };
        if (next - x).abs() <= 4.0 * f64::EPSILON * next.abs().max(f64::MIN_POSITIVE)
            || hi - lo <= 4.0 * f64::EPSILON * hi.abs()
        {
            return Ok(next);
        }
        x = next;
    }
    Err(StatError::ConvergenceFailure { routine: "quantile root finding", iterations: MAX_ITER })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) -> bool {
        if b == 0.0 { a.abs() <= tol } else { ((a - b) / b).abs() <= tol }
    }

    #[test]
    fn t_closed_forms() {
        // ν = 1 (Cauchy): F(t) = ½ + atan(t)/π.  ν = 2: F(t) = ½ + t / (2√(2 + t²)).
        let t1 = StudentsT::new(1.0).unwrap();
        let t2 = StudentsT::new(2.0).unwrap();
        for &t in &[-30.0, -2.5, -0.3, 0.0, 0.7, 4.0, 1e3] {
            let c1 = 0.5 + f64::atan(t) / std::f64::consts::PI;
            assert!(close(t1.cdf(t).unwrap(), c1, 1e-13), "nu=1 t={t}");
            let c2 = 0.5 + t / (2.0 * (2.0 + t * t).sqrt());
            assert!(close(t2.cdf(t).unwrap(), c2, 1e-13), "nu=2 t={t}");
        }
        // Upper tail of ν = 2 is t-free of cancellation: sf(t) = ½ − t/(2√(2+t²)).
        let sf = t2.sf(1e4).unwrap();
        let exact = 1.0 / (2.0 * (2.0 + 1e8_f64).sqrt() * ((2.0 + 1e8_f64).sqrt() + 1e4)) * 2.0;
        assert!(close(sf, exact, 1e-10), "{sf} vs {exact}");
    }

    #[test]
    fn t_density_integrates_consistently() {
        // pdf is the derivative of cdf: check with a central difference.
        let d = StudentsT::new(7.3).unwrap();
        for &t in &[-3.0, -0.5, 0.0, 1.2, 5.0] {
            let h = 1e-5;
            let numeric = (d.cdf(t + h).unwrap() - d.cdf(t - h).unwrap()) / (2.0 * h);
            assert!(close(d.pdf(t), numeric, 1e-7));
        }
    }

    #[test]
    fn t_quantiles_match_published_critical_values() {
        // Two-sided 95% critical values (standard tables, e.g. NIST e-Handbook §1.3.6.7.2).
        let cases = [
            (1.0, 12.706_204_736_174_7),
            (2.0, 4.302_652_729_749_464),
            (5.0, 2.570_581_835_636_314),
            (10.0, 2.228_138_851_986_274),
            (30.0, 2.042_272_456_301_238),
            (100.0, 1.983_971_518_523_552),
        ];
        for (df, q) in cases {
            let d = StudentsT::new(df).unwrap();
            assert!(close(d.quantile(0.975).unwrap(), q, 1e-12), "df = {df}");
            assert!(close(d.quantile(0.025).unwrap(), -q, 1e-12));
        }
    }

    #[test]
    fn t_quantile_round_trips() {
        for &df in &[0.7, 3.0, 9.0, 41.5, 1000.0] {
            let d = StudentsT::new(df).unwrap();
            for &p in &[1e-10, 1e-4, 0.01, 0.3, 0.5, 0.8, 0.999_9] {
                let q = d.quantile(p).unwrap();
                assert!(close(d.cdf(q).unwrap(), p, 1e-11), "df={df} p={p}");
            }
        }
        assert!(StudentsT::new(3.0).unwrap().quantile(1.0).is_err());
        assert!(StudentsT::new(0.0).is_err());
    }

    #[test]
    fn f_relates_to_t_squared() {
        // If T ~ t(ν) then T² ~ F(1, ν).
        let t = StudentsT::new(12.0).unwrap();
        let f = FisherSnedecor::new(1.0, 12.0).unwrap();
        for &x in &[0.1, 1.0, 2.5, 7.0] {
            assert!(close(f.sf(x * x).unwrap(), t.two_sided_p_value(x).unwrap(), 1e-13));
        }
    }

    #[test]
    fn f_closed_form_and_quantiles() {
        // F(2, 2): cdf(x) = x / (1 + x).
        let f = FisherSnedecor::new(2.0, 2.0).unwrap();
        for &x in &[0.01, 0.5, 3.0, 100.0] {
            assert!(close(f.cdf(x).unwrap(), x / (1.0 + x), 1e-13));
            assert!(close(f.sf(x).unwrap(), 1.0 / (1.0 + x), 1e-13));
        }
        // Published upper 5% point of F(3, 10) is 3.708265 (to the 7 digits tabulated).
        let g = FisherSnedecor::new(3.0, 10.0).unwrap();
        assert!(close(g.quantile(0.95).unwrap(), 3.708_265, 2e-7));
        assert!(close(g.cdf(g.quantile(0.3).unwrap()).unwrap(), 0.3, 1e-12));
        assert_eq!(g.sf(0.0).unwrap(), 1.0);
        assert_eq!(g.sf(f64::INFINITY).unwrap(), 0.0);
    }

    #[test]
    fn tiny_p_values_keep_relative_precision() {
        // Very large F should give a tiny but non-zero, accurately computed p-value.
        let f = FisherSnedecor::new(1.0, 34.0).unwrap();
        let p = f.sf(5_436_385.540_797_85).unwrap();
        assert!(p > 0.0 && p < 1e-70);
        let t = StudentsT::new(34.0).unwrap();
        let p_t = t.two_sided_p_value(5_436_385.540_797_85_f64.sqrt()).unwrap();
        assert!(close(p, p_t, 1e-10));
    }

    #[test]
    fn normal_reference_values() {
        let n = Normal::standard();
        // Standard values (e.g. NIST e-Handbook §1.3.6.7.1).
        assert!(close(n.cdf(1.96).unwrap(), 0.975_002_104_851_779_5, 1e-14));
        assert!(close(n.cdf(-1.0).unwrap(), 0.158_655_253_931_457_05, 1e-14));
        assert!(close(n.sf(3.0).unwrap(), 0.001_349_898_031_630_094_5, 1e-13));
        assert!(close(n.sf(10.0).unwrap(), 7.619_853_024_160_527e-24, 1e-12), "far tail keeps relative precision");
        assert!(close(n.quantile(0.975).unwrap(), 1.959_963_984_540_054, 1e-13));
        assert!(close(n.quantile(0.001).unwrap(), -3.090_232_306_167_813_5, 1e-13));
        assert!(close(n.pdf(0.0), 1.0 / (2.0 * std::f64::consts::PI).sqrt(), 1e-15));
        let m = Normal::new(10.0, 2.0).unwrap();
        assert!(close(m.cdf(12.0).unwrap(), n.cdf(1.0).unwrap(), 1e-15));
        assert!(close(m.quantile(0.25).unwrap(), 10.0 + 2.0 * n.quantile(0.25).unwrap(), 1e-14));
        assert!(Normal::new(0.0, 0.0).is_err());
    }

    /// Lower-tail quantiles keep relative precision. References: closed forms
    /// χ²₂: `x = −2 ln(1 − p)`, and χ²₁ = Z²: `x = Φ⁻¹((1 + p)/2)² = πp²/2 + O(p⁴)`
    /// for small `p`; the others from SciPy 1.18 `chi2.ppf` / `f.ppf`.
    #[test]
    fn lower_tail_quantiles() {
        let c2 = ChiSquared::new(2.0).unwrap();
        for &p in &[1e-300_f64, 1e-17, 1e-15, 1e-8, 0.3, 0.499] {
            let exact = -2.0 * (-p).ln_1p();
            assert!(close(c2.quantile(p).unwrap(), exact, 1e-12), "p = {p}");
        }
        let c1 = ChiSquared::new(1.0).unwrap();
        assert!(close(c1.quantile(1e-10).unwrap(), 1.570_796_326_794_896e-20, 1e-11));
        assert!(close(c1.quantile(1e-17).unwrap(), 1.570_796_326_794_896_6e-34, 1e-11));
        let cases = [
            (5.0, 1e-12, 5.124_643_496_830_541e-5),
            (0.02, 0.05, 8.931_070_037_821_248e-131),
            (0.3, 1e-10, 2.714_571_607_792_130_4e-67),
            (10.0, 1e-8, 0.132_327_944_584_381_46),
        ];
        for (df, p, q) in cases {
            assert!(close(ChiSquared::new(df).unwrap().quantile(p).unwrap(), q, 1e-11), "df = {df}, p = {p}");
        }
        // Below the smallest normal number the root is reported as 0.
        assert_eq!(ChiSquared::new(0.01).unwrap().quantile(1e-5).unwrap(), 0.0);
        let f = FisherSnedecor::new(1.0, 1.0).unwrap();
        assert!(close(f.quantile(1e-12).unwrap(), 2.467_401_100_272_339_5e-24, 1e-11));
        let f = FisherSnedecor::new(3.0, 10.0).unwrap();
        assert!(close(f.quantile(1e-10).unwrap(), 1.656_904_427_151_904_8e-7, 1e-11));
        let f = FisherSnedecor::new(0.2, 10.0).unwrap();
        assert!(close(f.quantile(1e-5).unwrap(), 6.660_504_360_947_678e-50, 1e-10));
        // The two branches meet at the median.
        let c5 = ChiSquared::new(5.0).unwrap();
        for p in [0.499_999_9, 0.5, 0.500_000_1] {
            assert!(close(c5.cdf(c5.quantile(p).unwrap()).unwrap(), p, 1e-14));
        }
    }

    #[test]
    fn chi_squared_closed_forms_and_table_values() {
        // k = 2: sf(x) = e^{−x/2}.
        let c2 = ChiSquared::new(2.0).unwrap();
        for &x in &[0.1, 1.0, 10.745_764_652_014_651, 60.0] {
            assert!(close(c2.sf(x).unwrap(), (-x / 2.0).exp(), 1e-13), "x = {x}");
        }
        // k = 1: sf(x) = erfc(√(x/2)) = P(|Z| > √x).
        let c1 = ChiSquared::new(1.0).unwrap();
        let n = Normal::standard();
        assert!(close(c1.sf(3.841_458_820_694_124).unwrap(), 0.05, 1e-12));
        assert!(close(c1.sf(4.0).unwrap(), 2.0 * n.sf(2.0).unwrap(), 1e-13));
        // Upper 5% points (standard tables): k = 1, 5, 10.
        assert!(close(c1.quantile(0.95).unwrap(), 3.841_458_820_694_124, 1e-11));
        assert!(close(ChiSquared::new(5.0).unwrap().quantile(0.95).unwrap(), 11.070_497_693_516_35, 1e-11));
        assert!(close(ChiSquared::new(10.0).unwrap().quantile(0.95).unwrap(), 18.307_038_053_275_15, 1e-11));
        let c5 = ChiSquared::new(5.0).unwrap();
        assert!(close(c5.cdf(4.0).unwrap() + c5.sf(4.0).unwrap(), 1.0, 1e-15));
        assert!(ChiSquared::new(0.0).is_err());
    }
}
