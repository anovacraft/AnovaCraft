//! Tests for outlying values in a sample from a normal population
//! (Barnett & Lewis, *Outliers in Statistical Data*, 3rd ed., 1994, ch. 6;
//! NIST/SEMATECH e-Handbook §1.3.5.17).
//!
//! * **Grubbs' test** (Grubbs 1950, 1969): `G = max |xᵢ − x̄|/s` (two-sided),
//!   `(x₍ₙ₎ − x̄)/s` (largest value) or `(x̄ − x₍₁₎)/s` (smallest). With
//!   `t² = n(n − 2)G²/[(n − 1)² − nG²]`, the p-value is `n·P(T_{n−2} > t)`
//!   one-sided and twice that two-sided (capped at 1): a Bonferroni bound
//!   that is exact for the small p-values that matter, where no two values
//!   can deviate that far at once. The critical value at level `α` is
//!   `G_crit = (n − 1)/√n · √(t²/(n − 2 + t²))`, `t = t_{1−α/(2n)}(n − 2)`
//!   (`α/n` one-sided).
//! * **Generalized ESD** (Rosner 1983) for up to `r` outliers: repeat Grubbs'
//!   two-sided statistic `Rᵢ` on the sample with the `i − 1` most extreme
//!   values removed, against
//!   `λᵢ = (n − i)t / √((n − i − 1 + t²)(n − i + 1))`,
//!   `t = t_{1−α/(2(n−i+1))}(n − i − 1)`; the number of outliers is the
//!   largest `i` with `Rᵢ > λᵢ`.
//! * **Dixon's ratio tests** (Dixon 1950, 1951): for the largest value,
//!   `r_{ij} = (x₍ₙ₎ − x₍ₙ₋ᵢ₎)/(x₍ₙ₎ − x₍₁₊ⱼ₎)` with `(i, j)` = (1, 0) `r₁₀`,
//!   (1, 1) `r₁₁`, (2, 1) `r₂₁` and (2, 2) `r₂₂` (mirror images for the
//!   smallest). Under normality, conditioning on `u = x₍₁₊ⱼ₎` and
//!   `b = x₍ₙ₎`, the other `M = n − 2 − j` values above `u` are independent
//!   and truncated to `(u, b)`, and `r > c` exactly when fewer than `i` of
//!   them exceed `t = b − c(b − u)`, so
//!
//!   ```text
//!   P(r > c) = n!/(j! M!) ∫∫_{u<b} Φ(u)ʲ φ(u) φ(b)
//!              Σ_{m<i} C(M, m) [Φ(b) − Φ(t)]ᵐ [Φ(t) − Φ(u)]^{M−m} du db,
//!   ```
//!
//!   evaluated by two-dimensional composite Gauss–Legendre quadrature.
//!   Dixon's recommended ratio is `r₁₀` for 3–7 values, `r₁₁` for 8–10, `r₂₁`
//!   for 11–13 and `r₂₂` from 14 on.

use crate::common::{Alternative, check_finite, increasing_root, invalid, mean_var};
use stat_core::distributions::{ContinuousDistribution, StudentsT};
use stat_core::quadrature::gauss_legendre;
use stat_core::special::{erfc, ln_gamma};
use stat_core::{Result, StatError};

/// Result of Grubbs' test.
#[derive(Debug, Clone, PartialEq)]
pub struct GrubbsTest {
    /// Number of values.
    pub n: usize,
    /// Sample mean.
    pub mean: f64,
    /// Sample standard deviation.
    pub std_dev: f64,
    /// Index (into the input) of the value tested.
    pub index: usize,
    /// The value tested.
    pub value: f64,
    /// `G`.
    pub statistic: f64,
    /// p-value.
    pub p_value: f64,
    /// Critical value of `G` at the level `alpha` given.
    pub critical_value: f64,
    /// Significance level of the critical value.
    pub alpha: f64,
    /// Which end was tested: `Greater` the largest value, `Less` the smallest,
    /// `TwoSided` the more extreme.
    pub alternative: Alternative,
}

fn check_alpha(alpha: f64) -> Result<()> {
    if alpha > 0.0 && alpha < 1.0 {
        Ok(())
    } else {
        Err(invalid(format!("the significance level must lie strictly between 0 and 1 (got {alpha})")))
    }
}

/// Critical value of Grubbs' statistic for `n` values at level `alpha`
/// (two-sided, or one-sided for `Less`/`Greater`).
///
/// # Errors
/// [`StatError::InsufficientData`] for `n < 3`; [`StatError::InvalidArgument`]
/// for `alpha ∉ (0, 1)`.
pub fn grubbs_critical_value(n: usize, alpha: f64, alternative: Alternative) -> Result<f64> {
    check_alpha(alpha)?;
    if n < 3 {
        return Err(StatError::InsufficientData { required: 3, found: n });
    }
    let nf = n as f64;
    let tail = match alternative {
        Alternative::TwoSided => alpha / (2.0 * nf),
        _ => alpha / nf,
    };
    let t = StudentsT::new(nf - 2.0)?.quantile(1.0 - tail)?;
    Ok((nf - 1.0) / nf.sqrt() * (t * t / (nf - 2.0 + t * t)).sqrt())
}

/// Grubbs' test for a single outlier in `x`.
///
/// # Errors
/// [`StatError::InsufficientData`] for fewer than three values;
/// [`StatError::NonFiniteInput`] for NaN/±∞; [`StatError::InvalidArgument`]
/// for values without variation or `alpha ∉ (0, 1)`.
pub fn grubbs_test(x: &[f64], alternative: Alternative, alpha: f64) -> Result<GrubbsTest> {
    check_finite(x, "sample values")?;
    check_alpha(alpha)?;
    let n = x.len();
    if n < 3 {
        return Err(StatError::InsufficientData { required: 3, found: n });
    }
    let (mean, var) = mean_var(x);
    if var <= 0.0 {
        return Err(invalid("the values have no variation"));
    }
    let s = var.sqrt();
    let (imax, imin) = extreme_indices(x);
    let index = match alternative {
        Alternative::Greater => imax,
        Alternative::Less => imin,
        Alternative::TwoSided => {
            if x[imax] - mean >= mean - x[imin] {
                imax
            } else {
                imin
            }
        }
    };
    let g = match alternative {
        Alternative::Greater => (x[imax] - mean) / s,
        Alternative::Less => (mean - x[imin]) / s,
        Alternative::TwoSided => (x[index] - mean).abs() / s,
    };
    let nf = n as f64;
    let denominator = (nf - 1.0).powi(2) - nf * g * g;
    let p_one = if g <= 0.0 {
        1.0
    } else if denominator <= 0.0 {
        0.0
    } else {
        let t = (nf * (nf - 2.0) * g * g / denominator).sqrt();
        nf * StudentsT::new(nf - 2.0)?.sf(t)?
    };
    let p_value = match alternative {
        Alternative::TwoSided => (2.0 * p_one).min(1.0),
        _ => p_one.min(1.0),
    };
    Ok(GrubbsTest {
        n,
        mean,
        std_dev: s,
        index,
        value: x[index],
        statistic: g,
        p_value,
        critical_value: grubbs_critical_value(n, alpha, alternative)?,
        alpha,
        alternative,
    })
}

fn extreme_indices(x: &[f64]) -> (usize, usize) {
    let mut imax = 0;
    let mut imin = 0;
    for (i, &v) in x.iter().enumerate() {
        if v > x[imax] {
            imax = i;
        }
        if v < x[imin] {
            imin = i;
        }
    }
    (imax, imin)
}

/// One step of the generalized ESD procedure.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EsdStep {
    /// Index (into the input) of the value removed at this step.
    pub index: usize,
    /// The value.
    pub value: f64,
    /// `Rᵢ`.
    pub statistic: f64,
    /// `λᵢ`.
    pub critical_value: f64,
}

/// Result of the generalized ESD test.
#[derive(Debug, Clone, PartialEq)]
pub struct EsdTest {
    /// Steps `i = 1, …, r`.
    pub steps: Vec<EsdStep>,
    /// Number of outliers: the largest `i` with `Rᵢ > λᵢ` (0 if none).
    pub outliers: usize,
    /// Significance level.
    pub alpha: f64,
}

impl EsdTest {
    /// Indices of the values declared outliers.
    #[must_use]
    pub fn outlier_indices(&self) -> Vec<usize> {
        self.steps[..self.outliers].iter().map(|s| s.index).collect()
    }
}

/// Rosner's generalized ESD test for up to `max_outliers` outliers.
///
/// # Errors
/// [`StatError::InsufficientData`] unless `max_outliers ≥ 1` and at least
/// `max_outliers + 3` values remain; [`StatError::NonFiniteInput`] for
/// NaN/±∞; [`StatError::InvalidArgument`] for `alpha ∉ (0, 1)` or a step
/// whose remaining values have no variation.
pub fn generalized_esd_test(x: &[f64], max_outliers: usize, alpha: f64) -> Result<EsdTest> {
    check_finite(x, "sample values")?;
    check_alpha(alpha)?;
    let n = x.len();
    if max_outliers == 0 {
        return Err(invalid("test for at least one outlier"));
    }
    if n < max_outliers + 3 {
        return Err(StatError::InsufficientData { required: max_outliers + 3, found: n });
    }
    let mut remaining: Vec<usize> = (0..n).collect();
    let mut steps = Vec::with_capacity(max_outliers);
    let nf = n as f64;
    for i in 1..=max_outliers {
        let values: Vec<f64> = remaining.iter().map(|&k| x[k]).collect();
        let (mean, var) = mean_var(&values);
        if var <= 0.0 {
            return Err(invalid("the remaining values have no variation"));
        }
        let s = var.sqrt();
        let (pos, dev) = values
            .iter()
            .enumerate()
            .map(|(p, v)| (p, (v - mean).abs()))
            .fold((0, -1.0), |best, c| if c.1 > best.1 { c } else { best });
        let fi = i as f64;
        let p = 1.0 - alpha / (2.0 * (nf - fi + 1.0));
        let t = StudentsT::new(nf - fi - 1.0)?.quantile(p)?;
        let lambda = (nf - fi) * t / ((nf - fi - 1.0 + t * t) * (nf - fi + 1.0)).sqrt();
        let index = remaining.remove(pos);
        steps.push(EsdStep { index, value: x[index], statistic: dev / s, critical_value: lambda });
    }
    let outliers = steps.iter().rposition(|s| s.statistic > s.critical_value).map_or(0, |p| p + 1);
    Ok(EsdTest { steps, outliers, alpha })
}

/// Which of Dixon's ratios.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DixonRatio {
    /// `r₁₀ = (x₍ₙ₎ − x₍ₙ₋₁₎)/(x₍ₙ₎ − x₍₁₎)`.
    R10,
    /// `r₁₁ = (x₍ₙ₎ − x₍ₙ₋₁₎)/(x₍ₙ₎ − x₍₂₎)`.
    R11,
    /// `r₂₁ = (x₍ₙ₎ − x₍ₙ₋₂₎)/(x₍ₙ₎ − x₍₂₎)`.
    R21,
    /// `r₂₂ = (x₍ₙ₎ − x₍ₙ₋₂₎)/(x₍ₙ₎ − x₍₃₎)`.
    R22,
}

impl DixonRatio {
    /// Every ratio.
    pub const ALL: [DixonRatio; 4] = [Self::R10, Self::R11, Self::R21, Self::R22];

    /// `(i, j)`: values skipped at the tested end and at the other end.
    #[must_use]
    pub fn gaps(self) -> (usize, usize) {
        match self {
            Self::R10 => (1, 0),
            Self::R11 => (1, 1),
            Self::R21 => (2, 1),
            Self::R22 => (2, 2),
        }
    }

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::R10 => "r10",
            Self::R11 => "r11",
            Self::R21 => "r21",
            Self::R22 => "r22",
        }
    }

    /// Fewest values the ratio is defined for.
    #[must_use]
    pub fn min_n(self) -> usize {
        let (i, j) = self.gaps();
        i + j + 2
    }

    /// Dixon's recommended ratio for `n` values.
    #[must_use]
    pub fn recommended(n: usize) -> Self {
        match n {
            0..=7 => Self::R10,
            8..=10 => Self::R11,
            11..=13 => Self::R21,
            _ => Self::R22,
        }
    }
}

fn phi_cdf(x: f64) -> f64 {
    0.5 * erfc(-x * std::f64::consts::FRAC_1_SQRT_2)
}

fn phi_pdf(x: f64) -> f64 {
    (-0.5 * x * x).exp() / (2.0 * std::f64::consts::PI).sqrt()
}

/// Null distribution of one of Dixon's ratios for `n` normal values (the
/// ratio at the largest value; by symmetry the same at the smallest).
#[derive(Debug, Clone)]
pub struct DixonDistribution {
    n: usize,
    ratio: DixonRatio,
    /// `(u, weight·n!/(j!M!)·Φ(u)ʲφ(u), Φ(u))` over the lower variable.
    outer: Vec<(f64, f64, f64)>,
    /// `(w, weight)` over the spread `b − u`.
    inner: Vec<(f64, f64)>,
}

impl DixonDistribution {
    /// Distribution of `ratio` for `n` values.
    ///
    /// # Errors
    /// [`StatError::InsufficientData`] when `n` is below the ratio's minimum.
    pub fn new(n: usize, ratio: DixonRatio) -> Result<Self> {
        if n < ratio.min_n() {
            return Err(StatError::InsufficientData { required: ratio.min_n(), found: n });
        }
        let (_, j) = ratio.gaps();
        let m = n - 2 - j;
        let ln_const = ln_gamma(n as f64 + 1.0) - ln_gamma(j as f64 + 1.0) - ln_gamma(m as f64 + 1.0);
        let (nodes, weights) = gauss_legendre(16);
        let rule = |a: f64, b: f64, panels: usize| -> Vec<(f64, f64)> {
            let half = 0.5 * (b - a) / panels as f64;
            (0..panels)
                .flat_map(|p| {
                    let mid = a + half * (2 * p + 1) as f64;
                    nodes.iter().zip(&weights).map(move |(x, w)| (mid + half * x, half * w))
                })
                .collect()
        };
        let outer = rule(-9.0, 7.0, 32)
            .into_iter()
            .map(|(u, w)| {
                let big = phi_cdf(u);
                let lw =
                    ln_const + (j as f64) * big.max(f64::MIN_POSITIVE).ln() + phi_pdf(u).max(f64::MIN_POSITIVE).ln();
                (u, w * lw.exp(), big)
            })
            .filter(|t| t.1 > 0.0)
            .collect();
        let inner = rule(0.0, 14.0, 28);
        Ok(Self { n, ratio, outer, inner })
    }

    /// `P(r > c)`.
    #[must_use]
    pub fn sf(&self, c: f64) -> f64 {
        if c <= 0.0 {
            return 1.0;
        }
        if c >= 1.0 {
            return 0.0;
        }
        let (i, j) = self.ratio.gaps();
        let m = self.n - 2 - j;
        // ln C(m, k) for k < i.
        let ln_binom: Vec<f64> = (0..i)
            .map(|k| ln_gamma(m as f64 + 1.0) - ln_gamma(k as f64 + 1.0) - ln_gamma((m - k) as f64 + 1.0))
            .collect();
        let mut total = 0.0;
        for &(u, wu, big_u) in &self.outer {
            let mut s = 0.0;
            for &(w, ww) in &self.inner {
                let b = u + w;
                let t = b - c * w;
                let big_b = phi_cdf(b);
                let big_t = phi_cdf(t);
                let above = (big_b - big_t).max(0.0);
                let below = (big_t - big_u).max(0.0);
                let mut prob = 0.0;
                for (k, lb) in ln_binom.iter().enumerate() {
                    let term = if k == 0 {
                        below.powi(m as i32)
                    } else {
                        lb.exp() * above.powi(k as i32) * below.powi((m - k) as i32)
                    };
                    prob += term;
                }
                s += ww * phi_pdf(b) * prob;
            }
            total += wu * s;
        }
        total.clamp(0.0, 1.0)
    }

    /// The `c` with `P(r > c) = alpha`.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] for `alpha ∉ (0, 1)`;
    /// [`StatError::ConvergenceFailure`] if root finding fails.
    pub fn critical_value(&self, alpha: f64) -> Result<f64> {
        check_alpha(alpha)?;
        increasing_root(|c| Ok(alpha - self.sf(c)), 0.0, 1.0, "Dixon critical value")
    }
}

/// Result of Dixon's test.
#[derive(Debug, Clone, PartialEq)]
pub struct DixonTest {
    /// Ratio used.
    pub ratio: DixonRatio,
    /// Number of values.
    pub n: usize,
    /// Index (into the input) of the value tested.
    pub index: usize,
    /// The value tested.
    pub value: f64,
    /// Value of the ratio.
    pub statistic: f64,
    /// p-value.
    pub p_value: f64,
    /// Critical value of the ratio at `alpha`.
    pub critical_value: f64,
    /// Significance level of the critical value.
    pub alpha: f64,
    /// End tested (see [`GrubbsTest::alternative`]).
    pub alternative: Alternative,
}

/// Dixon's ratio test for one outlier in `x`; `ratio = None` uses Dixon's
/// recommendation for the sample size.
///
/// # Errors
/// [`StatError::InsufficientData`] below the ratio's minimum size;
/// [`StatError::NonFiniteInput`] for NaN/±∞; [`StatError::InvalidArgument`]
/// when the ratio's denominator is zero or `alpha ∉ (0, 1)`.
pub fn dixon_test(x: &[f64], ratio: Option<DixonRatio>, alternative: Alternative, alpha: f64) -> Result<DixonTest> {
    check_finite(x, "sample values")?;
    check_alpha(alpha)?;
    let n = x.len();
    let ratio = ratio.unwrap_or_else(|| DixonRatio::recommended(n));
    let dist = DixonDistribution::new(n, ratio)?;
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| x[a].total_cmp(&x[b]));
    let (i, j) = ratio.gaps();
    let sorted = |k: usize| x[order[k]];
    let high_den = sorted(n - 1) - sorted(j);
    let low_den = sorted(n - 1 - j) - sorted(0);
    if high_den <= 0.0 || low_den <= 0.0 {
        return Err(invalid("the ratio's denominator is zero (too many tied values)"));
    }
    let high = (sorted(n - 1) - sorted(n - 1 - i)) / high_den;
    let low = (sorted(i) - sorted(0)) / low_den;
    let (statistic, index) = match alternative {
        Alternative::Greater => (high, order[n - 1]),
        Alternative::Less => (low, order[0]),
        Alternative::TwoSided => {
            if high >= low {
                (high, order[n - 1])
            } else {
                (low, order[0])
            }
        }
    };
    let one = dist.sf(statistic);
    let (p_value, crit_alpha) = match alternative {
        Alternative::TwoSided => ((2.0 * one).min(1.0), alpha / 2.0),
        _ => (one, alpha),
    };
    Ok(DixonTest {
        ratio,
        n,
        index,
        value: x[index],
        statistic,
        p_value,
        critical_value: dist.critical_value(crit_alpha)?,
        alpha,
        alternative,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_core::rng::Rng;

    /// Rosner's (1983) 54 values, as listed in the NIST/SEMATECH e-Handbook
    /// §1.3.5.17.3.
    const ROSNER: [f64; 54] = [
        -0.25, 0.68, 0.94, 1.15, 1.20, 1.26, 1.26, 1.34, 1.38, 1.43, 1.49, 1.49, 1.55, 1.56, 1.58, 1.65, 1.69, 1.70,
        1.76, 1.77, 1.81, 1.91, 1.94, 1.96, 1.99, 2.06, 2.09, 2.10, 2.14, 2.15, 2.23, 2.24, 2.26, 2.35, 2.37, 2.40,
        2.47, 2.54, 2.62, 2.64, 2.90, 2.92, 2.92, 2.93, 3.21, 3.26, 3.30, 3.59, 3.68, 4.30, 4.64, 5.34, 5.42, 6.01,
    ];

    /// NIST's table: R₁…R₁₀ and λ₁…λ₁₀ (5%), three outliers.
    #[test]
    fn generalized_esd_matches_nist() {
        // NIST truncates to three decimals (R₁ is 3.1189…).
        let r = [3.118, 2.942, 3.179, 2.810, 2.815, 2.848, 2.279, 2.310, 2.101, 2.067];
        let lambda = [3.158, 3.151, 3.143, 3.136, 3.128, 3.120, 3.111, 3.103, 3.094, 3.085];
        let t = generalized_esd_test(&ROSNER, 10, 0.05).unwrap();
        for (k, step) in t.steps.iter().enumerate() {
            assert!((step.statistic - r[k]).abs() < 1e-3, "R{}: {}", k + 1, step.statistic);
            assert!((step.critical_value - lambda[k]).abs() < 1e-3, "λ{}: {}", k + 1, step.critical_value);
        }
        assert_eq!(t.outliers, 3);
        let mut flagged: Vec<f64> = t.outlier_indices().iter().map(|&i| ROSNER[i]).collect();
        flagged.sort_by(f64::total_cmp);
        assert_eq!(flagged, [5.34, 5.42, 6.01]);
        assert!(generalized_esd_test(&ROSNER[..4], 2, 0.05).is_err());
    }

    /// The first ESD step is Grubbs' two-sided test: G = 3.118 below the
    /// 5% critical value 3.158 for n = 54 (NIST table above).
    #[test]
    fn grubbs_matches_nist_and_its_own_critical_value() {
        let g = grubbs_test(&ROSNER, Alternative::TwoSided, 0.05).unwrap();
        assert!((g.statistic - 3.118).abs() < 1e-3);
        assert!((g.critical_value - 3.158).abs() < 1e-3);
        assert_eq!(g.value, 6.01);
        assert!(g.p_value > 0.05 && g.p_value < 0.1, "{}", g.p_value);
        // At the critical value the p-value is exactly α.
        for alt in [Alternative::TwoSided, Alternative::Greater] {
            for n in [3usize, 7, 20, 100] {
                let c = grubbs_critical_value(n, 0.05, alt).unwrap();
                let nf = n as f64;
                let t = (nf * (nf - 2.0) * c * c / ((nf - 1.0).powi(2) - nf * c * c)).sqrt();
                let p = nf * StudentsT::new(nf - 2.0).unwrap().sf(t).unwrap();
                let p = if alt == Alternative::TwoSided { 2.0 * p } else { p };
                assert!((p - 0.05).abs() < 1e-10, "n={n}: {p}");
            }
        }
        // Grubbs' (1950) table: n = 3, one-sided 5% critical value 1.153.
        assert!((grubbs_critical_value(3, 0.05, Alternative::Greater).unwrap() - 1.153).abs() < 6e-4);
        // n = 10, one-sided 5%: 2.176; 1%: 2.410.
        assert!((grubbs_critical_value(10, 0.05, Alternative::Greater).unwrap() - 2.176).abs() < 6e-4);
        assert!((grubbs_critical_value(10, 0.01, Alternative::Greater).unwrap() - 2.410).abs() < 6e-4);
        let low = grubbs_test(&ROSNER, Alternative::Less, 0.05).unwrap();
        assert_eq!(low.value, -0.25);
        assert!(grubbs_test(&[1.0, 2.0], Alternative::TwoSided, 0.05).is_err());
        assert!(grubbs_test(&[1.0, 1.0, 1.0], Alternative::TwoSided, 0.05).is_err());
    }

    /// Dean & Dixon (1951) / Rorabacher (1991) critical values of r₁₀ at one-
    /// tailed 5% and 2.5% (the Q test at 90% and 95%). (The deeper tails are
    /// checked by simulation below; the published 99% column differs from
    /// simulation by up to 0.005.)
    #[test]
    fn dixon_r10_critical_values_match_published_tables() {
        let table =
            [(3, [0.941, 0.970]), (4, [0.765, 0.829]), (5, [0.642, 0.710]), (7, [0.507, 0.568]), (10, [0.412, 0.466])];
        for (n, crit) in table {
            let d = DixonDistribution::new(n, DixonRatio::R10).unwrap();
            for (alpha, c) in [0.05, 0.025].into_iter().zip(crit) {
                let got = d.critical_value(alpha).unwrap();
                assert!((got - c).abs() < 1.5e-3, "n={n} α={alpha}: {got} vs {c}");
            }
        }
    }

    /// Every ratio's tail probabilities against simulation of normal samples.
    #[test]
    fn dixon_distributions_match_simulation() {
        let mut rng = Rng::new(20261014);
        let reps = 40_000;
        for (n, ratio, alpha) in [
            (5, DixonRatio::R10, 0.10),
            (4, DixonRatio::R10, 0.01),
            (9, DixonRatio::R11, 0.10),
            (12, DixonRatio::R21, 0.10),
            (16, DixonRatio::R22, 0.05),
        ] {
            let d = DixonDistribution::new(n, ratio).unwrap();
            let c = d.critical_value(alpha).unwrap();
            let (i, j) = ratio.gaps();
            let mut hits = 0;
            for _ in 0..reps {
                let mut v: Vec<f64> = (0..n).map(|_| rng.normal()).collect();
                v.sort_by(f64::total_cmp);
                let r = (v[n - 1] - v[n - 1 - i]) / (v[n - 1] - v[j]);
                if r > c {
                    hits += 1;
                }
            }
            let share = hits as f64 / reps as f64;
            // Within 4 standard errors.
            let se = (alpha * (1.0 - alpha) / reps as f64).sqrt();
            assert!((share - alpha).abs() < 4.0 * se, "{ratio:?} n={n}: {share}");
        }
    }

    #[test]
    fn dixon_test_on_a_small_sample() {
        // 0.189 0.167 0.187 0.183 0.186 0.182 0.181 0.184 0.181 0.177 (n = 10, r₁₁).
        let x = [0.189, 0.167, 0.187, 0.183, 0.186, 0.182, 0.181, 0.184, 0.181, 0.177];
        let t = dixon_test(&x, None, Alternative::TwoSided, 0.05).unwrap();
        assert_eq!(t.ratio, DixonRatio::R11);
        assert_eq!(t.value, 0.167);
        assert!((t.statistic - (0.177 - 0.167) / (0.187 - 0.167)).abs() < 1e-12);
        let d = DixonDistribution::new(10, DixonRatio::R11).unwrap();
        assert!((t.p_value - 2.0 * d.sf(t.statistic)).abs() < 1e-12);
        assert!((d.sf(t.critical_value) - 0.025).abs() < 1e-9);
        assert!(dixon_test(&[1.0, 2.0], None, Alternative::TwoSided, 0.05).is_err());
        assert!(dixon_test(&[1.0, 1.0, 1.0, 2.0], Some(DixonRatio::R11), Alternative::Greater, 0.05).is_err());
    }
}
