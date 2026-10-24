//! Posterior distributions of one quantity and their summaries.
//!
//! Every closed-form posterior of the conjugate analyses implements
//! [`Univariate`]: beta, gamma, a location–scale Student t, the standard
//! deviation of a normal under a scaled inverse-χ² posterior, a ratio of
//! gammas (a scaled F), and the difference, ratio and odds ratio of two
//! independent posteriors, whose density and distribution function are
//! one-dimensional integrals (the convolution `∫ f₁(x) f₂(x − d) dx` and
//! `P(X₁ − X₂ ≤ d) = ∫ f₁(x) S₂(x − d) dx`, and the analogues for ratios),
//! computed by adaptive quadrature rather than simulation.
//!
//! [`summarize`] gives the posterior mean, standard deviation, median, the
//! equal-tailed credible interval and the highest-posterior-density (HPD)
//! interval: for a unimodal density, the shortest interval of the given
//! probability, found by minimizing `Q(p + level) − Q(p)` over `p` (Box &
//! Tiao, *Bayesian Inference in Statistical Analysis*, 1973, §2.8).
//! [`hpd_from_draws`] is the shortest interval containing the given share of
//! sorted draws (Chen & Shao 1999, *J. Comput. Graph. Stat.* 8).

use crate::integrate::integrate;
use stat_core::catalog::{Distribution, Family};
use stat_core::special::ln_gamma;
use stat_core::{ContinuousDistribution, Result, StatError, StudentsT};

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// A continuous distribution of one quantity.
pub trait Univariate: Send + Sync {
    /// Density at `x`.
    fn pdf(&self, x: f64) -> f64;
    /// `P(X ≤ x)`.
    fn cdf(&self, x: f64) -> f64;
    /// Mean (`NaN` when it does not exist).
    fn mean(&self) -> f64;
    /// Standard deviation (`NaN` when it does not exist).
    fn sd(&self) -> f64;
    /// Support `(lower, upper)` (possibly infinite).
    fn support(&self) -> (f64, f64);
    /// Quantile: the `x` with `P(X ≤ x) = p`. The default inverts
    /// [`Univariate::cdf`] by bracketing and bisection.
    fn quantile(&self, p: f64) -> f64 {
        invert_cdf(self, p)
    }
    /// `P(X > x)`.
    fn sf(&self, x: f64) -> f64 {
        1.0 - self.cdf(x)
    }
    /// `a` when the support starts at 0 and the density behaves like
    /// `x^(a−1)` there (beta first shape, gamma shape), so that `E[X⁻ᵏ]`
    /// exists exactly when `a > k`; `None` when that does not apply.
    fn shape_at_zero(&self) -> Option<f64> {
        None
    }
    /// `b` when the support ends at 1 with density like `(1 − x)^(b−1)`
    /// there (beta second shape); `None` otherwise.
    fn shape_at_one(&self) -> Option<f64> {
        None
    }
}

/// Invert a distribution function by bracketing from the mean and
/// standard deviation (or the support) and bisecting to 1e-13 relative.
fn invert_cdf<D: Univariate + ?Sized>(d: &D, p: f64) -> f64 {
    let (lo_s, hi_s) = d.support();
    if p <= 0.0 {
        return lo_s;
    }
    if p >= 1.0 {
        return hi_s;
    }
    let (m, s) = (d.mean(), d.sd());
    let centre = if m.is_finite() { m } else { 0.5 * (lo_s.max(-1.0) + hi_s.min(1.0)) };
    let width = if s.is_finite() && s > 0.0 { s } else { 1.0 };
    let mut lo = if lo_s.is_finite() { lo_s } else { centre - width };
    let mut hi = if hi_s.is_finite() { hi_s } else { centre + width };
    let mut step = width;
    while !lo_s.is_finite() && d.cdf(lo) > p && step < 1e300 {
        lo -= step;
        step *= 2.0;
    }
    step = width;
    while !hi_s.is_finite() && d.cdf(hi) < p && step < 1e300 {
        hi += step;
        step *= 2.0;
    }
    for _ in 0..200 {
        let mid = 0.5 * (lo + hi);
        if d.cdf(mid) < p {
            lo = mid;
        } else {
            hi = mid;
        }
        if (hi - lo) <= 1e-13 * mid.abs().max(1e-300) {
            break;
        }
    }
    0.5 * (lo + hi)
}

/// Beta(`a`, `b`) (a proportion's posterior).
#[derive(Debug, Clone)]
pub struct BetaDist {
    /// First shape.
    pub a: f64,
    /// Second shape.
    pub b: f64,
    inner: Distribution,
}

impl BetaDist {
    /// Beta(`a`, `b`), `a, b > 0`.
    ///
    /// # Errors
    /// Non-positive shapes.
    pub fn new(a: f64, b: f64) -> Result<Self> {
        if !(a > 0.0 && b > 0.0 && a.is_finite() && b.is_finite()) {
            return Err(invalid(format!("beta shapes must be positive (got {a}, {b})")));
        }
        Ok(Self { a, b, inner: Distribution::new(Family::Beta, &[a, b])? })
    }
}

impl Univariate for BetaDist {
    fn pdf(&self, x: f64) -> f64 {
        self.inner.density(x)
    }
    fn cdf(&self, x: f64) -> f64 {
        self.inner.cdf(x)
    }
    fn sf(&self, x: f64) -> f64 {
        self.inner.sf(x)
    }
    fn quantile(&self, p: f64) -> f64 {
        if p <= 0.0 {
            return 0.0;
        }
        if p >= 1.0 {
            return 1.0;
        }
        self.inner.quantile(p).unwrap_or_else(|_| invert_cdf(self, p))
    }
    fn mean(&self) -> f64 {
        self.a / (self.a + self.b)
    }
    fn shape_at_zero(&self) -> Option<f64> {
        Some(self.a)
    }
    fn shape_at_one(&self) -> Option<f64> {
        Some(self.b)
    }
    fn sd(&self) -> f64 {
        let s = self.a + self.b;
        (self.a * self.b / (s * s * (s + 1.0))).sqrt()
    }
    fn support(&self) -> (f64, f64) {
        (0.0, 1.0)
    }
}

/// Gamma with `shape` and `rate` (an event rate's posterior).
#[derive(Debug, Clone)]
pub struct GammaDist {
    /// Shape.
    pub shape: f64,
    /// Rate (inverse scale).
    pub rate: f64,
    inner: Distribution,
}

impl GammaDist {
    /// Gamma(`shape`, `rate`), both positive.
    ///
    /// # Errors
    /// Non-positive parameters.
    pub fn new(shape: f64, rate: f64) -> Result<Self> {
        if !(shape > 0.0 && rate > 0.0 && shape.is_finite() && rate.is_finite()) {
            return Err(invalid(format!("gamma shape and rate must be positive (got {shape}, {rate})")));
        }
        Ok(Self { shape, rate, inner: Distribution::new(Family::Gamma, &[shape, 1.0 / rate, 0.0])? })
    }
}

impl Univariate for GammaDist {
    fn pdf(&self, x: f64) -> f64 {
        self.inner.density(x)
    }
    fn cdf(&self, x: f64) -> f64 {
        self.inner.cdf(x)
    }
    fn sf(&self, x: f64) -> f64 {
        self.inner.sf(x)
    }
    fn quantile(&self, p: f64) -> f64 {
        if p <= 0.0 {
            return 0.0;
        }
        self.inner.quantile(p).unwrap_or_else(|_| invert_cdf(self, p))
    }
    fn mean(&self) -> f64 {
        self.shape / self.rate
    }
    fn shape_at_zero(&self) -> Option<f64> {
        Some(self.shape)
    }
    fn sd(&self) -> f64 {
        self.shape.sqrt() / self.rate
    }
    fn support(&self) -> (f64, f64) {
        (0.0, f64::INFINITY)
    }
}

/// Location–scale Student t: `location + scale · T_ν` (a normal mean's
/// posterior).
#[derive(Debug, Clone)]
pub struct ScaledT {
    /// Location.
    pub location: f64,
    /// Scale.
    pub scale: f64,
    /// Degrees of freedom.
    pub df: f64,
    t: StudentsT,
}

impl ScaledT {
    /// `location + scale · T_df`.
    ///
    /// # Errors
    /// A non-positive scale or degrees of freedom.
    pub fn new(location: f64, scale: f64, df: f64) -> Result<Self> {
        if !(scale > 0.0 && scale.is_finite() && location.is_finite()) {
            return Err(invalid(format!("the t scale must be positive (got {scale})")));
        }
        Ok(Self { location, scale, df, t: StudentsT::new(df)? })
    }
}

impl Univariate for ScaledT {
    fn pdf(&self, x: f64) -> f64 {
        self.t.pdf((x - self.location) / self.scale) / self.scale
    }
    fn cdf(&self, x: f64) -> f64 {
        self.t.cdf((x - self.location) / self.scale).unwrap_or(f64::NAN)
    }
    fn sf(&self, x: f64) -> f64 {
        self.t.sf((x - self.location) / self.scale).unwrap_or(f64::NAN)
    }
    fn quantile(&self, p: f64) -> f64 {
        if p <= 0.0 {
            return f64::NEG_INFINITY;
        }
        if p >= 1.0 {
            return f64::INFINITY;
        }
        self.location + self.scale * self.t.quantile(p).unwrap_or(f64::NAN)
    }
    fn mean(&self) -> f64 {
        if self.df > 1.0 { self.location } else { f64::NAN }
    }
    fn sd(&self) -> f64 {
        if self.df > 2.0 { self.scale * (self.df / (self.df - 2.0)).sqrt() } else { f64::NAN }
    }
    fn support(&self) -> (f64, f64) {
        (f64::NEG_INFINITY, f64::INFINITY)
    }
}

/// The standard deviation `σ` when `σ²` has a scaled inverse-χ²(`ν`, `s²`)
/// posterior, i.e. `σ² = ν s² / X` with `X ~ χ²_ν`.
#[derive(Debug, Clone)]
pub struct SigmaDist {
    /// Degrees of freedom `ν`.
    pub df: f64,
    /// Scale `s²`.
    pub scale2: f64,
    chi: Distribution,
}

impl SigmaDist {
    /// `σ` with `σ² ~ Scaled-Inv-χ²(df, scale2)`.
    ///
    /// # Errors
    /// Non-positive parameters.
    pub fn new(df: f64, scale2: f64) -> Result<Self> {
        if !(df > 0.0 && scale2 > 0.0 && scale2.is_finite()) {
            return Err(invalid("the scaled inverse chi-square needs positive degrees of freedom and scale"));
        }
        Ok(Self { df, scale2, chi: Distribution::new(Family::ChiSquared, &[df])? })
    }
}

impl Univariate for SigmaDist {
    fn pdf(&self, s: f64) -> f64 {
        if s <= 0.0 {
            return 0.0;
        }
        // σ = √(νs²/X): f(σ) = f_X(νs²/σ²) · 2νs²/σ³.
        let x = self.df * self.scale2 / (s * s);
        self.chi.density(x) * 2.0 * x / s
    }
    fn cdf(&self, s: f64) -> f64 {
        if s <= 0.0 {
            return 0.0;
        }
        self.chi.sf(self.df * self.scale2 / (s * s))
    }
    fn sf(&self, s: f64) -> f64 {
        if s <= 0.0 {
            return 1.0;
        }
        self.chi.cdf(self.df * self.scale2 / (s * s))
    }
    fn quantile(&self, p: f64) -> f64 {
        if p <= 0.0 {
            return 0.0;
        }
        if p >= 1.0 {
            return f64::INFINITY;
        }
        self.chi.quantile(1.0 - p).map_or(f64::NAN, |x| (self.df * self.scale2 / x).sqrt())
    }
    fn mean(&self) -> f64 {
        if self.df <= 1.0 {
            return f64::NAN;
        }
        let h = 0.5 * self.df;
        (self.scale2 * h).sqrt() * (ln_gamma(h - 0.5) - ln_gamma(h)).exp()
    }
    fn sd(&self) -> f64 {
        if self.df <= 2.0 {
            return f64::NAN;
        }
        let m = self.mean();
        (self.df * self.scale2 / (self.df - 2.0) - m * m).max(0.0).sqrt()
    }
    fn support(&self) -> (f64, f64) {
        (0.0, f64::INFINITY)
    }
}

/// How two independent posteriors are combined.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Contrast {
    /// `X₁ − X₂`.
    Difference,
    /// `X₁ / X₂` (positive quantities).
    Ratio,
    /// `[X₁/(1 − X₁)] / [X₂/(1 − X₂)]` (proportions).
    OddsRatio,
}

/// The distribution of a contrast of two independent posteriors, by
/// one-dimensional quadrature over the second component.
pub struct Combined<'a> {
    /// First posterior.
    pub first: &'a dyn Univariate,
    /// Second posterior.
    pub second: &'a dyn Univariate,
    /// The contrast.
    pub contrast: Contrast,
    /// Effective range of the second posterior (its 1e-13 and 1 − 1e-13
    /// quantiles), over which the integrals run.
    range: (f64, f64),
}

impl std::fmt::Debug for Combined<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Combined").field("contrast", &self.contrast).finish_non_exhaustive()
    }
}

impl<'a> Combined<'a> {
    /// The contrast of `first` and `second`.
    #[must_use]
    pub fn new(first: &'a dyn Univariate, second: &'a dyn Univariate, contrast: Contrast) -> Self {
        let range = (second.quantile(1e-13), second.quantile(1.0 - 1e-13));
        Self { first, second, contrast, range }
    }

    /// For a value `y` of the second component, the value of the first at
    /// which the contrast equals `c`, and `d(x)/d(c)`.
    fn first_at(&self, c: f64, y: f64) -> (f64, f64) {
        match self.contrast {
            Contrast::Difference => (y + c, 1.0),
            Contrast::Ratio => (c * y, y),
            Contrast::OddsRatio => {
                let o = c * y / (1.0 - y);
                let x = o / (1.0 + o);
                (x, (y / (1.0 - y)) / ((1.0 + o) * (1.0 + o)))
            }
        }
    }

    fn over_second(&self, g: impl Fn(f64) -> f64) -> f64 {
        let (a, b) = self.range;
        integrate(|y| self.second.pdf(y) * g(y), a, b, 1e-10, 24)
    }
}

impl Univariate for Combined<'_> {
    fn pdf(&self, c: f64) -> f64 {
        if matches!(self.contrast, Contrast::Ratio | Contrast::OddsRatio) && c <= 0.0 {
            return 0.0;
        }
        self.over_second(|y| {
            let (x, j) = self.first_at(c, y);
            self.first.pdf(x) * j
        })
    }
    fn cdf(&self, c: f64) -> f64 {
        if matches!(self.contrast, Contrast::Ratio | Contrast::OddsRatio) && c <= 0.0 {
            return 0.0;
        }
        self.over_second(|y| self.first.cdf(self.first_at(c, y).0)).clamp(0.0, 1.0)
    }
    fn mean(&self) -> f64 {
        match self.contrast {
            Contrast::Difference => self.first.mean() - self.second.mean(),
            Contrast::Ratio | Contrast::OddsRatio => self.moment(1),
        }
    }
    fn sd(&self) -> f64 {
        match self.contrast {
            Contrast::Difference => self.first.sd().hypot(self.second.sd()),
            Contrast::Ratio | Contrast::OddsRatio => {
                let m = self.moment(1);
                let v = self.moment(2) - m * m;
                // `f64::max` would turn an undefined (NaN) variance into 0.
                if v.is_nan() { f64::NAN } else { v.max(0.0).sqrt() }
            }
        }
    }
    fn support(&self) -> (f64, f64) {
        match self.contrast {
            Contrast::Difference => {
                let (a1, b1) = self.first.support();
                let (a2, b2) = self.second.support();
                (a1 - b2, b1 - a2)
            }
            Contrast::Ratio | Contrast::OddsRatio => (0.0, f64::INFINITY),
        }
    }
}

impl Combined<'_> {
    /// `E[cᵏ]` of a ratio or odds ratio, by quadrature of the first
    /// component's moments over the second (`NaN` when it diverges).
    ///
    /// Divergence is decided from the shapes, not from the size of a
    /// truncated integral: `E[X₂⁻ᵏ]` (and `E[((1 − X₂)/X₂)ᵏ]`) exist only when
    /// the second component's shape at zero exceeds `k` (e.g. not for a
    /// Beta(1, n + 1) posterior after zero events), and for an odds ratio
    /// `E[(X₁/(1 − X₁))ᵏ]` only when the first one's shape at one does.
    fn moment(&self, k: i32) -> f64 {
        let kf = f64::from(k);
        let second_diverges = self.second.shape_at_zero().is_some_and(|a| a <= kf);
        let first_diverges = self.contrast == Contrast::OddsRatio && self.first.shape_at_one().is_some_and(|b| b <= kf);
        if second_diverges || first_diverges {
            return f64::NAN;
        }
        // Where a moment's integrand may be singular at an end of the support
        // (but integrable, as just checked), integrate up to that end: the
        // truncated range would drop a share of order 1e-13^((a − k)/a).
        let mut a = self.first.quantile(1e-13);
        let mut b = self.first.quantile(1.0 - 1e-13);
        if self.contrast == Contrast::OddsRatio && self.first.shape_at_one().is_some() {
            b = 1.0;
        }
        if self.first.shape_at_zero().is_some() {
            a = 0.0;
        }
        let m1 = integrate(
            |x| {
                let v = if self.contrast == Contrast::OddsRatio { x / (1.0 - x) } else { x };
                self.first.pdf(x) * v.powi(k)
            },
            a,
            b,
            1e-10,
            24,
        );
        let lower = if self.second.shape_at_zero().is_some() { 0.0 } else { self.range.0 };
        let m2 = integrate(
            |y| {
                let v = if self.contrast == Contrast::OddsRatio { y / (1.0 - y) } else { y };
                self.second.pdf(y) * v.powi(-k)
            },
            lower,
            self.range.1,
            1e-10,
            24,
        );
        let v = m1 * m2;
        // Backstop for posteriors without a known shape.
        if v.is_finite() && v < 1e15 { v } else { f64::NAN }
    }
}

/// Summary of a posterior.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Summary {
    /// Posterior mean (`NaN` if it does not exist).
    pub mean: f64,
    /// Posterior standard deviation (`NaN` if it does not exist).
    pub sd: f64,
    /// Posterior median.
    pub median: f64,
    /// Credible level of the intervals.
    pub level: f64,
    /// Equal-tailed interval `(Q((1 − level)/2), Q((1 + level)/2))`.
    pub equal_tailed: (f64, f64),
    /// Highest-posterior-density interval.
    pub hpd: (f64, f64),
}

/// Mean, SD, median, equal-tailed and HPD intervals of `d` at `level`.
#[must_use]
pub fn summarize(d: &dyn Univariate, level: f64) -> Summary {
    let tail = 0.5 * (1.0 - level);
    Summary {
        mean: d.mean(),
        sd: d.sd(),
        median: d.quantile(0.5),
        level,
        equal_tailed: (d.quantile(tail), d.quantile(1.0 - tail)),
        hpd: hpd_interval(d, level),
    }
}

/// The highest-posterior-density interval of a unimodal distribution: the
/// shortest `[Q(p), Q(p + level)]`, by golden-section search on `p`.
#[must_use]
pub fn hpd_interval(d: &dyn Univariate, level: f64) -> (f64, f64) {
    let width = |p: f64| d.quantile(p + level) - d.quantile(p);
    let (mut lo, mut hi) = (0.0_f64, 1.0 - level);
    let phi = 0.5 * (5f64.sqrt() - 1.0);
    let (mut x1, mut x2) = (hi - phi * (hi - lo), lo + phi * (hi - lo));
    let (mut w1, mut w2) = (width(x1), width(x2));
    for _ in 0..80 {
        if w1 <= w2 {
            hi = x2;
            x2 = x1;
            w2 = w1;
            x1 = hi - phi * (hi - lo);
            w1 = width(x1);
        } else {
            lo = x1;
            x1 = x2;
            w1 = w2;
            x2 = lo + phi * (hi - lo);
            w2 = width(x2);
        }
        if hi - lo < 1e-12 {
            break;
        }
    }
    // The search can end at a boundary (a J-shaped density).
    let mut p = 0.5 * (lo + hi);
    for edge in [0.0, 1.0 - level] {
        if width(edge) < width(p) {
            p = edge;
        }
    }
    (d.quantile(p), d.quantile(p + level))
}

/// The shortest interval containing `level` of the draws (sorted copy).
#[must_use]
pub fn hpd_from_draws(draws: &[f64], level: f64) -> (f64, f64) {
    let mut s: Vec<f64> = draws.iter().copied().filter(|v| v.is_finite()).collect();
    if s.is_empty() {
        return (f64::NAN, f64::NAN);
    }
    s.sort_by(f64::total_cmp);
    let n = s.len();
    let k = ((level * n as f64).ceil() as usize).clamp(1, n);
    let mut best = (s[0], s[n - 1]);
    let mut width = f64::INFINITY;
    for i in 0..=(n - k) {
        let w = s[i + k - 1] - s[i];
        if w < width {
            width = w;
            best = (s[i], s[i + k - 1]);
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beta_summary_and_hpd() {
        let d = BetaDist::new(8.0, 14.0).unwrap();
        let s = summarize(&d, 0.95);
        assert!((s.mean - 8.0 / 22.0).abs() < 1e-14);
        // HPD is shorter than the equal-tailed interval and has equal density at both ends.
        assert!(s.hpd.1 - s.hpd.0 < s.equal_tailed.1 - s.equal_tailed.0);
        assert!((d.pdf(s.hpd.0) / d.pdf(s.hpd.1) - 1.0).abs() < 1e-6);
        assert!((d.cdf(s.hpd.1) - d.cdf(s.hpd.0) - 0.95).abs() < 1e-10);
        // J-shaped: the interval starts at zero.
        let j = BetaDist::new(0.5, 3.0).unwrap();
        assert_eq!(hpd_interval(&j, 0.9).0, 0.0);
    }

    #[test]
    fn combined_contrasts_agree_with_closed_forms() {
        // Difference of two normals-as-t with huge df ≈ normal convolution.
        let a = ScaledT::new(1.0, 1.0, 1e7).unwrap();
        let b = ScaledT::new(0.0, 2.0, 1e7).unwrap();
        let d = Combined::new(&a, &b, Contrast::Difference);
        let s5 = 5f64.sqrt();
        let n = stat_core::Normal::new(1.0, s5).unwrap();
        for x in [-3.0, 0.0, 1.0, 4.0] {
            assert!((d.cdf(x) - n.cdf(x).unwrap()).abs() < 1e-7);
            assert!((d.pdf(x) - n.pdf(x)).abs() < 1e-7);
        }
        // Ratio of gammas with equal rates: X₁/X₂ ~ (a₁/a₂)·F(2a₁, 2a₂).
        let g1 = GammaDist::new(3.0, 2.0).unwrap();
        let g2 = GammaDist::new(5.0, 2.0).unwrap();
        let r = Combined::new(&g1, &g2, Contrast::Ratio);
        let f = stat_core::FisherSnedecor::new(6.0, 10.0).unwrap();
        for c in [0.2, 0.6, 1.5] {
            assert!((r.cdf(c) - f.cdf(c * 5.0 / 3.0).unwrap()).abs() < 1e-9);
        }
        // E[X₁/X₂] = (a₁/b)·b/(a₂ − 1).
        assert!((r.mean() - 3.0 / 4.0).abs() < 1e-8);
    }

    #[test]
    fn sigma_distribution_is_consistent() {
        let s = SigmaDist::new(9.0, 4.0).unwrap();
        let q = s.quantile(0.3);
        assert!((s.cdf(q) - 0.3).abs() < 1e-12);
        let m = integrate(|x| x * s.pdf(x), 0.0, 40.0, 1e-12, 8);
        assert!((m - s.mean()).abs() < 1e-8);
        let total = integrate(|x| s.pdf(x), 0.0, 40.0, 1e-12, 8);
        assert!((total - 1.0).abs() < 1e-9);
    }

    #[test]
    fn hpd_of_draws() {
        let draws: Vec<f64> = (0..1000).map(|i| f64::from(i) / 999.0).collect();
        let (a, b) = hpd_from_draws(&draws, 0.5);
        assert!((b - a - 0.4995).abs() < 0.002);
    }
}
