//! The studentized range distribution, used for Tukey–Kramer and Games–Howell
//! multiple comparisons.
//!
//! For `k` independent standard normal variables with range `R` and an
//! independent `S = √(χ²_ν / ν)`, `Q = R / S`. The range distribution is
//!
//! ```text
//! W(w; k) = P(R ≤ w) = k ∫ φ(z) [Φ(z) − Φ(z − w)]^{k−1} dz,
//! ```
//!
//! and `P(Q ≤ q) = ∫₀^∞ f_S(s) W(qs; k) ds` with
//! `f_S(s) = 2 (ν/2)^{ν/2} s^{ν−1} e^{−νs²/2} / Γ(ν/2)` (Hartley 1942;
//! Copenhaver & Holland 1988, *J. Statist. Comput. Simul.* 30). Both integrals
//! are evaluated by composite Gauss–Legendre quadrature; the outer one over the
//! central `1 − 2·10⁻¹⁴` of `S`, in the variable `u = ln s` (density
//! `s·f_S(s)`), so that for few degrees of freedom the far tail of `Q`, which
//! comes from very small `s`, is resolved as well as the centre. For
//! `ν > 10⁶` the factor `S` is taken as 1. Inside the outer integral the
//! range distribution `W` is evaluated by barycentric interpolation at
//! Chebyshev–Lobatto points on `[0, w₁]`, where `2k·Φ(−w₁/2) = 10⁻¹⁷` bounds
//! `1 − W(w₁)`; `W` is entire, so the interpolant is accurate to rounding
//! and each CDF costs a few hundred interpolations instead of tens of
//! thousands of normal probabilities.
//! Quantiles are found by safeguarded secant iteration on the CDF.

use crate::distributions::{ChiSquared, ContinuousDistribution, Normal};
use crate::error::{Result, StatError, invalid};
use crate::quadrature::gauss_legendre;
use crate::special::{erfc, ln_gamma};

fn phi_cdf(x: f64) -> f64 {
    0.5 * erfc(-x * std::f64::consts::FRAC_1_SQRT_2)
}

/// Studentized range distribution with `k` means and `ν` error degrees of freedom.
#[derive(Debug, Clone)]
pub struct StudentizedRange {
    k: f64,
    /// Outer quadrature `(s, weight · s · f_S(s))` over `u = ln s`, or `None`
    /// for `ν > 10⁶` (`S = 1`).
    outer: Option<Vec<(f64, f64)>>,
    /// Inner quadrature: `(z, weight · φ(z), Φ(z))` at each node.
    inner: Vec<(f64, f64, f64)>,
    /// Upper end `w₁` of the interpolation interval of `W`.
    w_max: f64,
    /// Chebyshev–Lobatto nodes on `[0, w₁]` with `W` at each.
    table: Vec<(f64, f64)>,
}

/// Degree of the Chebyshev interpolant of the range distribution `W`
/// (doubled beyond 100 means, where `W` rises more steeply).
fn range_degree(k: usize) -> usize {
    if k <= 100 { 128 } else { 256 }
}

/// Smallest degrees of freedom accepted.
const MIN_DF: f64 = 0.05;

/// Width (in `ln s`) of each 16-point panel of the outer quadrature.
const OUTER_PANEL_WIDTH: f64 = 1.4;

impl StudentizedRange {
    /// `k ≥ 2` groups and `ν ≥ 0.05` degrees of freedom (use `f64::INFINITY`
    /// for a known variance).
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] for `k < 2` or `ν < 0.05` (below that the
    /// distribution's mass spreads over thousands of orders of magnitude and
    /// the quadrature cannot cover it).
    pub fn new(k: usize, df: f64) -> Result<Self> {
        if k < 2 {
            return Err(invalid("the studentized range needs at least two means"));
        }
        if df.is_nan() || df <= 0.0 {
            return Err(invalid(format!("degrees of freedom must be positive (got {df})")));
        }
        if df < MIN_DF {
            return Err(invalid(format!(
                "the studentized range needs at least {MIN_DF} degrees of freedom (got {df})"
            )));
        }
        let outer = if df > 1e6 {
            None
        } else {
            let chi = ChiSquared::new(df)?;
            let lo = (chi.quantile(1e-14)? / df).sqrt();
            let hi = (chi.quantile(1.0 - 1e-14)? / df).sqrt();
            // For ν below about 0.1 the lower quantile underflows; take its
            // logarithm from the leading term of the lower tail instead,
            // P(χ² ≤ x) ≈ (x/2)^{ν/2} / Γ(ν/2 + 1).
            let ln_lo = if lo.is_normal() {
                lo.ln()
            } else {
                let ln_x = std::f64::consts::LN_2 + (2.0 / df) * (1e-14_f64.ln() + ln_gamma(0.5 * df + 1.0));
                0.5 * (ln_x - df.ln())
            };
            // ∫ f_S(s) g(s) ds = ∫ s f_S(s) g(s) du with u = ln s.
            let ln_c = std::f64::consts::LN_2 + 0.5 * df * (0.5 * df).ln() - ln_gamma(0.5 * df);
            let (base, base_w) = gauss_legendre(16);
            let (a, b) = (ln_lo, hi.ln());
            // About 34 units of ln s for ν = 1, a couple for ν = 20; at least
            // eight panels.
            let panels = (((b - a) / OUTER_PANEL_WIDTH).ceil() as usize).max(8);
            let half = 0.5 * (b - a) / panels as f64;
            let mut nodes = Vec::with_capacity(16 * panels);
            for p in 0..panels {
                let mid = a + half * (2 * p + 1) as f64;
                for (x, w) in base.iter().zip(&base_w) {
                    let u = mid + half * x;
                    let s = u.exp();
                    let weight = half * w * (ln_c + df * u - 0.5 * df * s * s).exp();
                    // Nodes in the outermost 10⁻¹⁴ tails carry no weight worth summing.
                    if weight > 1e-20 {
                        nodes.push((s, weight));
                    }
                }
            }
            // Normalise so that the weights sum to one: the far upper tail is
            // then `Σ w (1 − W)` with no floor from the quadrature's own error
            // in `∫ f_S = 1` (about 2·10⁻¹⁰ near ν = 10⁶).
            let total: f64 = nodes.iter().map(|&(_, w)| w).sum();
            for node in &mut nodes {
                node.1 /= total;
            }
            Some(nodes)
        };
        // Inner nodes over [−8.5, 8.5], with φ and Φ precomputed (they do not depend on w).
        let (nodes, weights) = gauss_legendre(16);
        let panels = 12;
        let h = 17.0 / panels as f64;
        let norm = 1.0 / (2.0 * std::f64::consts::PI).sqrt();
        let mut inner = Vec::with_capacity(16 * panels);
        for p in 0..panels {
            let mid = -8.5 + h * (p as f64 + 0.5);
            for (x, w) in nodes.iter().zip(&weights) {
                let z = mid + 0.5 * h * x;
                inner.push((z, 0.5 * h * w * norm * (-0.5 * z * z).exp(), phi_cdf(z)));
            }
        }
        let w_max = -2.0 * Normal::standard().quantile(1e-17 / (2.0 * k as f64))?;
        let mut dist = Self { k: k as f64, outer, inner, w_max, table: Vec::new() };
        if dist.outer.is_some() {
            let degree = range_degree(k);
            let d = degree as f64;
            dist.table = (0..=degree)
                .map(|j| {
                    let w = 0.5 * w_max * (1.0 - (std::f64::consts::PI * j as f64 / d).cos());
                    (w, dist.range_cdf(w))
                })
                .collect();
        }
        Ok(dist)
    }

    /// `P(R ≤ w)` for the range of `k` standard normals.
    fn range_cdf(&self, w: f64) -> f64 {
        if w <= 0.0 {
            return 0.0;
        }
        let k = self.k;
        // P(R > w) ≤ 2k·Φ(−w/2): negligible beyond this.
        if w > 16.0 && 2.0 * k * phi_cdf(-0.5 * w) < 1e-17 {
            return 1.0;
        }
        let km1 = (k - 1.0).round() as i32;
        let mut v = 0.0;
        for &(z, weight, big_phi) in &self.inner {
            let d = (big_phi - phi_cdf(z - w)).max(0.0);
            if d > 0.0 {
                v += weight * d.powi(km1);
            }
        }
        (k * v).clamp(0.0, 1.0)
    }

    /// `W(w)` from the Chebyshev interpolant (barycentric formula for points
    /// of the second kind).
    fn range_cdf_interpolated(&self, w: f64) -> f64 {
        if w <= 0.0 {
            return 0.0;
        }
        if w >= self.w_max {
            return 1.0;
        }
        let last = self.table.len() - 1;
        let (mut num, mut den) = (0.0, 0.0);
        for (j, &(wj, fj)) in self.table.iter().enumerate() {
            let diff = w - wj;
            if diff == 0.0 {
                return fj;
            }
            let mut c = if j % 2 == 0 { 1.0 } else { -1.0 };
            if j == 0 || j == last {
                c *= 0.5;
            }
            let t = c / diff;
            num += t * fj;
            den += t;
        }
        (num / den).clamp(0.0, 1.0)
    }

    /// `P(Q ≤ q)`.
    ///
    /// # Errors
    /// [`StatError::NonFiniteInput`] for NaN.
    pub fn cdf(&self, q: f64) -> Result<f64> {
        if q.is_nan() {
            return Err(StatError::NonFiniteInput { context: "studentized range value" });
        }
        if q <= 0.0 {
            return Ok(0.0);
        }
        if q.is_infinite() {
            return Ok(1.0);
        }
        let Some(outer) = &self.outer else { return Ok(self.range_cdf(q)) };
        let v: f64 = outer.iter().map(|&(s, w)| w * self.range_cdf_interpolated(q * s)).sum();
        Ok(v.clamp(0.0, 1.0))
    }

    /// `P(Q > q)`.
    ///
    /// # Errors
    /// As [`cdf`](Self::cdf).
    pub fn sf(&self, q: f64) -> Result<f64> {
        if q.is_nan() {
            return Err(StatError::NonFiniteInput { context: "studentized range value" });
        }
        if q <= 0.0 {
            return Ok(1.0);
        }
        if q.is_infinite() {
            return Ok(0.0);
        }
        let Some(outer) = &self.outer else { return Ok((1.0 - self.range_cdf(q)).max(0.0)) };
        // The weights sum to one, so the tail is summed directly.
        let v: f64 = outer.iter().map(|&(s, w)| w * (1.0 - self.range_cdf_interpolated(q * s))).sum();
        Ok(v.clamp(0.0, 1.0))
    }

    /// The `q` with `P(Q ≤ q) = p`.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] unless `0 < p < 1`;
    /// [`StatError::ConvergenceFailure`] if the iteration fails.
    pub fn quantile(&self, p: f64) -> Result<f64> {
        if !(p > 0.0 && p < 1.0) {
            return Err(invalid(format!("probability must lie strictly between 0 and 1 (got {p})")));
        }
        // Bracket, then regula falsi (Illinois variant), on the CDF below the
        // median and on the upper tail above it, so that extreme upper
        // quantiles are not limited by the rounding of `1 − p`.
        let upper = p > 0.5;
        let target = if upper { 1.0 - p } else { p };
        let f = |q: f64| -> Result<f64> { Ok(if upper { target - self.sf(q)? } else { self.cdf(q)? - p }) };
        let (mut a, mut b) = (0.0, 2.0);
        let mut fa = -p;
        let mut fb = f(b)?;
        let mut grow = 0;
        while fb < 0.0 {
            a = b;
            fa = fb;
            b *= 2.0;
            fb = f(b)?;
            grow += 1;
            if grow > 60 {
                return Err(StatError::ConvergenceFailure { routine: "studentized range quantile", iterations: grow });
            }
        }
        let mut side = 0;
        for _ in 0..200 {
            let c = (a * fb - b * fa) / (fb - fa);
            let fc = f(c)?;
            if fc.abs() < 1e-14 * target.min(1.0) || (b - a).abs() < 1e-12 * c.abs() {
                return Ok(c);
            }
            if fc * fb > 0.0 {
                b = c;
                fb = fc;
                if side == -1 {
                    fa *= 0.5;
                }
                side = -1;
            } else {
                a = c;
                fa = fc;
                if side == 1 {
                    fb *= 0.5;
                }
                side = 1;
            }
        }
        Err(StatError::ConvergenceFailure { routine: "studentized range quantile", iterations: 200 })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) {
        assert!((a - b).abs() <= tol * b.abs(), "{a} vs {b}");
    }

    /// Reference values: SciPy 1.18 `stats.studentized_range`; the 95% points
    /// agree with published tables (Harter 1960): q(3, 12) = 3.77,
    /// q(5, 20) = 4.23, q(4, 60) = 3.74, q(2, 10) = 3.15.
    #[test]
    fn matches_reference() {
        let cases = [
            (3.5, 3, 12.0, 0.930_004_514_724_816_4),
            (4.0, 5, 20.0, 0.930_412_856_076_954_1),
            (2.0, 2, 5.0, 0.783_562_770_730_314_7),
            (5.5, 10, 30.0, 0.984_178_770_750_614_7),
            (3.31, 3, 1e9, 0.949_596_627_852_897_2),
            (6.0, 4, 2.0, 0.875_510_781_158_128_7),
        ];
        for (q, k, df, cdf) in cases {
            let d = StudentizedRange::new(k, df).unwrap();
            close(d.cdf(q).unwrap(), cdf, 1e-9);
            close(d.sf(q).unwrap(), 1.0 - cdf, 1e-7);
        }
        for (k, df, q) in [
            (3, 12.0, 3.772_928_965_726_967),
            (5, 20.0, 4.231_856_748_997_479),
            (4, 60.0, 3.737_089_225_635_308_8),
            (2, 10.0, 3.151_064_183_329_372),
        ] {
            close(StudentizedRange::new(k, df).unwrap().quantile(0.95).unwrap(), q, 1e-9);
        }
        // Two means: Q = √2 |t|.
        let t = crate::StudentsT::new(10.0).unwrap();
        let q = 2.7;
        close(
            StudentizedRange::new(2, 10.0).unwrap().sf(q).unwrap(),
            t.two_sided_p_value(q / 2f64.sqrt()).unwrap(),
            1e-8,
        );
    }

    /// Few error degrees of freedom and high levels, where the tail of `Q`
    /// comes from very small `s`. Two means with `ν = 1`: `Q = √2|t₁|`, so
    /// `q₀.₉₉₉ = √2·t₀.₉₉₉₅,₁ = 900.3155756762` exactly. The others are
    /// SciPy 1.18.1 `stats.studentized_range` (the 95% points agree with
    /// Harter's 1960 tables: q(20, 1) = 59.56, q(100, 1) = 79.98).
    #[test]
    fn small_df_tails() {
        let exact = 2f64.sqrt() * crate::StudentsT::new(1.0).unwrap().quantile(0.9995).unwrap();
        close(exact, 900.315_575_676_2, 1e-9);
        close(StudentizedRange::new(2, 1.0).unwrap().quantile(0.999).unwrap(), exact, 1e-9);
        for (k, df, p, q) in [
            (3, 1.0, 0.95, 26.975_529_869_5),
            (3, 1.0, 0.99, 135.040_659_518_5),
            (3, 1.0, 0.999, 1_350.473_795_462),
            (10, 2.0, 0.99, 31.689_352_369_39),
            (10, 2.0, 0.999, 100.498_350_669_9),
            (20, 3.0, 0.999, 43.042_871_096_43),
            (50, 2.0, 0.95, 20.046_456_4),
            (20, 1.0, 0.95, 59.557_57),
            (100, 1.0, 0.95, 79.976_05),
        ] {
            close(StudentizedRange::new(k, df).unwrap().quantile(p).unwrap(), q, 1e-6);
        }
        close(StudentizedRange::new(10, 1.0).unwrap().cdf(40.0).unwrap(), 0.938_685_718_2, 1e-8);
        close(StudentizedRange::new(5, 5.0).unwrap().sf(30.0).unwrap(), 2.390_477_227e-5, 1e-6);
        close(StudentizedRange::new(20, 3.0).unwrap().sf(30.0).unwrap(), 2.926_553_186e-3, 1e-7);
    }

    /// A quantile is used in interactive reports, so it must stay fast.
    #[test]
    fn quantile_is_fast() {
        let start = std::time::Instant::now();
        for (k, df) in [(2, 1.0), (10, 2.0), (5, 20.0), (100, 30.0)] {
            StudentizedRange::new(k, df).unwrap().quantile(0.99).unwrap();
        }
        // Generous for unoptimized test builds (a release build takes ~5 ms each).
        assert!(start.elapsed().as_secs_f64() < 5.0);
    }

    /// Small degrees of freedom used to panic (the lower quantile of χ²_ν
    /// underflowed to 0); references from SciPy 1.18 `studentized_range.cdf`.
    #[test]
    fn small_degrees_of_freedom() {
        for (df, at3, at50) in [
            (0.05, 0.108_719_642_709_396_32, 0.225_482_628_878_222_87),
            (0.09, 0.165_631_398_393_151_9, 0.351_747_501_478_635_04),
            (0.1, 0.178_013_006_948_733_45, 0.378_966_544_115_215_46),
        ] {
            let d = StudentizedRange::new(3, df).unwrap();
            close(d.cdf(3.0).unwrap(), at3, 1e-10);
            close(d.cdf(50.0).unwrap(), at50, 1e-10);
        }
        assert!(StudentizedRange::new(3, 0.01).is_err());
    }
}
