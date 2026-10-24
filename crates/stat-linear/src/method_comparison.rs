//! Regression for method-comparison studies, where both measurements carry
//! error: Deming regression and Passing–Bablok regression.
//!
//! # Deming regression
//! With `xᵢ = ξᵢ + δᵢ`, `yᵢ = α + βξᵢ + εᵢ` and a known ratio
//! `λ = Var(ε)/Var(δ)` of the error variances of `y` and `x`, the maximum
//! likelihood slope (Deming 1943, *Statistical Adjustment of Data*; Cornbleet
//! & Gochman 1979, *Clin. Chem.* 25) is
//!
//! `β̂ = [S_yy − λS_xx + √((S_yy − λS_xx)² + 4λS_xy²)] / (2S_xy)`,
//!
//! `α̂ = ȳ − β̂x̄`, with `S` the centred sums of squares and products. With
//! `λ = 1` this is orthogonal regression (the first principal axis); as
//! `λ → ∞` it tends to ordinary least squares of y on x. Standard errors and
//! confidence intervals come from the jackknife (Linnet 1990, *Clin. Chem.*
//! 36; Linnet 1993, *Clin. Chem.* 39): with the leave-one-out estimates
//! `θ₍₋ᵢ₎`, `se = √((n − 1)/n · Σ(θ₍₋ᵢ₎ − θ̄₍.₎)²)` and the interval
//! `θ̂ ± t_{1−α/2, n−2} · se`.
//!
//! # Passing–Bablok regression
//! A non-parametric, outlier-resistant estimate (Passing & Bablok 1983,
//! *J. Clin. Chem. Clin. Biochem.* 21). The slopes `Sᵢⱼ = (yⱼ − yᵢ)/(xⱼ − xᵢ)`
//! of all pairs `i < j` are formed, leaving out pairs with both differences
//! zero and slopes of exactly −1 (a tied `x` gives ±∞ by the sign of the `y`
//! difference). With `N` slopes sorted and `K` of them below −1, the slope is
//! the median shifted by `K`: `S₍(N+1)/2+K₎` for odd `N`, otherwise the mean
//! of `S₍N/2+K₎` and `S₍N/2+1+K₎`. Its confidence limits are `S₍M₁+K₎` and
//! `S₍M₂+K₎` with `C = z_{1−α/2} √(n(n − 1)(2n + 5)/18)`,
//! `M₁ = round((N − C)/2)` and `M₂ = N − M₁ + 1`. The intercept is
//! `median(yᵢ − β̂xᵢ)`, with limits `median(yᵢ − β̂_U xᵢ)` and
//! `median(yᵢ − β̂_L xᵢ)`. The method assumes the two measurements are
//! linearly related; Passing & Bablok's cumulative-sum test checks it: with
//! `l` points above and `L` below the line, points above score `√(L/l)`,
//! points below `−√(l/L)`, points on it 0; ordered by their projection
//! `Dᵢ = (yᵢ + xᵢ/β̂ − α̂/β̂)/√(1 + 1/β̂²)` onto the line, the largest absolute
//! cumulative sum divided by `√(l + L)` is compared with the Kolmogorov
//! distribution (critical value 1.36 at α = 0.05). With these scores the
//! cumulative sum after `k` points is `√(lL)·(F₊(k) − F₋(k))`, where `F₊` and
//! `F₋` are the empirical distribution functions of the points above and below
//! the line, so the statistic is Smirnov's two-sample Kolmogorov–Smirnov
//! statistic `√(lL/(l + L))·D`. (The divisor is often printed `√(L + 1)`; that
//! form rejects a linear relation far more often than the nominal level, about
//! 19% at α = 0.05 for 20 points on each side.)

use crate::INTERCEPT_NAME;
use stat_core::descriptive::median;
use stat_core::distributions::{ContinuousDistribution, Normal, StudentsT};
use stat_core::{Result, StatError};

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// An estimate with its standard error (when one exists) and confidence
/// limits.
#[derive(Debug, Clone, PartialEq)]
pub struct ComparisonEstimate {
    /// `Intercept` or `Slope`.
    pub name: String,
    /// Point estimate.
    pub estimate: f64,
    /// Jackknife standard error (Deming); `None` for Passing–Bablok.
    pub std_error: Option<f64>,
    /// Lower confidence limit.
    pub lower: f64,
    /// Upper confidence limit.
    pub upper: f64,
}

/// A Deming regression fit.
#[derive(Debug, Clone, PartialEq)]
pub struct DemingFit {
    /// Error-variance ratio `λ = Var(ε_y)/Var(δ_x)` used.
    pub variance_ratio: f64,
    /// Intercept and slope with jackknife inference.
    pub intercept: ComparisonEstimate,
    /// Slope.
    pub slope: ComparisonEstimate,
    /// Confidence level.
    pub confidence_level: f64,
    /// Observations.
    pub n: usize,
    /// Leave-one-out slopes, in data order (for influence displays).
    pub jackknife_slopes: Vec<f64>,
    /// Leave-one-out intercepts.
    pub jackknife_intercepts: Vec<f64>,
}

impl DemingFit {
    /// Fitted line at `x`.
    #[must_use]
    pub fn predict(&self, x: f64) -> f64 {
        self.intercept.estimate + self.slope.estimate * x
    }
}

/// Deming slope and intercept, without inference.
fn deming_point(x: &[f64], y: &[f64], lambda: f64) -> Result<(f64, f64)> {
    let n = x.len() as f64;
    let mx = x.iter().sum::<f64>() / n;
    let my = y.iter().sum::<f64>() / n;
    let (mut sxx, mut syy, mut sxy) = (0.0, 0.0, 0.0);
    for (a, b) in x.iter().zip(y) {
        let (dx, dy) = (a - mx, b - my);
        sxx += dx * dx;
        syy += dy * dy;
        sxy += dx * dy;
    }
    if sxy == 0.0 || sxx == 0.0 {
        return Err(invalid("Deming regression needs x and y that vary together (S_xy ≠ 0)"));
    }
    let u = syy - lambda * sxx;
    let root = u.hypot(2.0 * lambda.sqrt() * sxy);
    // (u + √(u² + 4λS_xy²))/(2S_xy) cancels when u ≪ 0 (a huge λ); there the
    // conjugate form 2λS_xy/(√(u² + 4λS_xy²) − u) is exact.
    let slope = if u >= 0.0 { (u + root) / (2.0 * sxy) } else { 2.0 * lambda * sxy / (root - u) };
    Ok((my - slope * mx, slope))
}

fn check_pairs(x: &[f64], y: &[f64], min: usize) -> Result<()> {
    if x.len() != y.len() {
        return Err(StatError::DimensionMismatch { context: "x vs. y", expected: x.len(), found: y.len() });
    }
    if x.iter().chain(y).any(|v| !v.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "method-comparison data" });
    }
    if x.len() < min {
        return Err(StatError::InsufficientData { required: min, found: x.len() });
    }
    Ok(())
}

fn check_level(level: f64) -> Result<()> {
    if level > 0.0 && level < 1.0 { Ok(()) } else { Err(invalid("the confidence level must lie in (0, 1)")) }
}

/// Deming regression of `y` (test method) on `x` (comparison method) with
/// error-variance ratio `variance_ratio = Var(error of y)/Var(error of x)`
/// (1 for orthogonal regression), with jackknife standard errors and
/// t-based confidence limits on `n − 2` df.
///
/// # Errors
/// * [`StatError::DimensionMismatch`] if `x` and `y` differ in length.
/// * [`StatError::NonFiniteInput`] for NaN or infinite values.
/// * [`StatError::InsufficientData`] for fewer than 3 pairs.
/// * [`StatError::InvalidArgument`] for a ratio that is not positive and
///   finite, a bad confidence level, or data (or a leave-one-out subset)
///   with no covariation.
pub fn deming_regression(x: &[f64], y: &[f64], variance_ratio: f64, confidence_level: f64) -> Result<DemingFit> {
    check_pairs(x, y, 3)?;
    check_level(confidence_level)?;
    if !(variance_ratio.is_finite() && variance_ratio > 0.0) {
        return Err(invalid("the error-variance ratio must be positive"));
    }
    let n = x.len();
    let (a, b) = deming_point(x, y, variance_ratio)?;
    let mut jack_a = Vec::with_capacity(n);
    let mut jack_b = Vec::with_capacity(n);
    let mut xs = Vec::with_capacity(n - 1);
    let mut ys = Vec::with_capacity(n - 1);
    for i in 0..n {
        xs.clear();
        ys.clear();
        for j in (0..n).filter(|&j| j != i) {
            xs.push(x[j]);
            ys.push(y[j]);
        }
        let (ai, bi) = deming_point(&xs, &ys, variance_ratio)?;
        jack_a.push(ai);
        jack_b.push(bi);
    }
    let t = StudentsT::new((n - 2) as f64)?.quantile(1.0 - (1.0 - confidence_level) / 2.0)?;
    let estimate = |name: &str, value: f64, leave_out: &[f64]| {
        let m = leave_out.iter().sum::<f64>() / n as f64;
        let ss: f64 = leave_out.iter().map(|v| (v - m) * (v - m)).sum();
        let se = ((n - 1) as f64 / n as f64 * ss).sqrt();
        ComparisonEstimate {
            name: name.to_owned(),
            estimate: value,
            std_error: Some(se),
            lower: value - t * se,
            upper: value + t * se,
        }
    };
    Ok(DemingFit {
        variance_ratio,
        intercept: estimate(INTERCEPT_NAME, a, &jack_a),
        slope: estimate("Slope", b, &jack_b),
        confidence_level,
        n,
        jackknife_slopes: jack_b,
        jackknife_intercepts: jack_a,
    })
}

/// Passing–Bablok's cumulative-sum test of linearity.
#[derive(Debug, Clone, PartialEq)]
pub struct LinearityTest {
    /// Points above the line (`l`).
    pub n_above: usize,
    /// Points below the line (`L`).
    pub n_below: usize,
    /// Largest absolute cumulative sum.
    pub max_cusum: f64,
    /// `max |cusum| / √(l + L)`, the two-sample Kolmogorov–Smirnov statistic.
    pub statistic: f64,
    /// Asymptotic Kolmogorov p-value of the statistic.
    pub p_value: f64,
    /// Cumulative sums in projection order, with the data row of each step.
    pub cusum: Vec<(usize, f64)>,
}

/// A Passing–Bablok fit.
#[derive(Debug, Clone, PartialEq)]
pub struct PassingBablokFit {
    /// Intercept with its confidence limits.
    pub intercept: ComparisonEstimate,
    /// Slope with its confidence limits.
    pub slope: ComparisonEstimate,
    /// Confidence level.
    pub confidence_level: f64,
    /// Observations.
    pub n: usize,
    /// Pairwise slopes used (`N`).
    pub n_slopes: usize,
    /// Slopes below −1 (`K`, the median offset).
    pub offset: usize,
    /// Linearity test (`None` when every point lies on one side or on the line).
    pub linearity: Option<LinearityTest>,
    /// Why a slope limit is infinite, when the shifted rank of a limit falls
    /// outside the `N` slopes. That happens when the methods are negatively
    /// related (many slopes below −1); Passing & Bablok (1983) assume a
    /// positive relation, and their procedure then gives no finite limit.
    pub note: Option<String>,
}

impl PassingBablokFit {
    /// Fitted line at `x`.
    #[must_use]
    pub fn predict(&self, x: f64) -> f64 {
        self.intercept.estimate + self.slope.estimate * x
    }
}

/// Upper tail of the Kolmogorov distribution,
/// `P(K > h) = 2 Σ_{k≥1} (−1)^{k−1} e^{−2k²h²}`.
fn kolmogorov_sf(h: f64) -> f64 {
    // Below 0.2 the series converges slowly and the tail is 1 to 15 digits.
    if h < 0.2 {
        return 1.0;
    }
    let mut s = 0.0;
    for i in 1..=100u32 {
        let k = f64::from(i);
        let term = (-2.0 * k * k * h * h).exp();
        s += if i % 2 == 1 { term } else { -term };
        if term < 1e-17 {
            break;
        }
    }
    (2.0 * s).clamp(0.0, 1.0)
}

/// Passing–Bablok regression of `y` (test method) on `x` (comparison
/// method), with the cumulative-sum linearity test.
///
/// # Errors
/// * [`StatError::DimensionMismatch`] if `x` and `y` differ in length.
/// * [`StatError::NonFiniteInput`] for NaN or infinite values.
/// * [`StatError::InsufficientData`] for fewer than 3 pairs.
/// * [`StatError::InvalidArgument`] for a bad confidence level or when no
///   pairwise slope can be formed.
pub fn passing_bablok(x: &[f64], y: &[f64], confidence_level: f64) -> Result<PassingBablokFit> {
    check_pairs(x, y, 3)?;
    check_level(confidence_level)?;
    let n = x.len();
    let mut slopes = Vec::with_capacity(n * (n - 1) / 2);
    for i in 0..n {
        for j in i + 1..n {
            let (dx, dy) = (x[j] - x[i], y[j] - y[i]);
            if dx == 0.0 {
                if dy != 0.0 {
                    slopes.push(if dy > 0.0 { f64::INFINITY } else { f64::NEG_INFINITY });
                }
                continue;
            }
            let s = dy / dx;
            if s != -1.0 {
                slopes.push(s);
            }
        }
    }
    if slopes.is_empty() {
        return Err(invalid("Passing–Bablok regression needs pairs with different values"));
    }
    slopes.sort_by(f64::total_cmp);
    let big_n = slopes.len();
    let k = slopes.iter().filter(|&&s| s < -1.0).count();
    // 1-based order statistic with the offset, clamped to the slopes.
    let at = |i: usize| slopes[(i + k).clamp(1, big_n) - 1];
    let b = if big_n % 2 == 1 { at(big_n.div_ceil(2)) } else { 0.5 * (at(big_n / 2) + at(big_n / 2 + 1)) };
    let z = Normal::standard().quantile(1.0 - (1.0 - confidence_level) / 2.0)?;
    let nf = n as f64;
    let c = z * (nf * (nf - 1.0) * (2.0 * nf + 5.0) / 18.0).sqrt();
    let m1 = ((big_n as f64 - c) / 2.0).round().max(1.0) as usize;
    let m2 = (big_n + 1).saturating_sub(m1).max(1);
    // An upper limit whose shifted rank lies beyond N is undefined (Passing &
    // Bablok assume positively related methods); report it as infinite.
    // (The lower rank M₁ + K is at least 1.)
    let b_lo = at(m1);
    let b_hi = if m2 + k > big_n { f64::INFINITY } else { at(m2) };
    let note = (b_lo.is_infinite() || b_hi.is_infinite()).then(|| {
        if k > 0 && m2 + k > big_n {
            format!(
                "{k} of the {big_n} pairwise slopes are below −1, so the methods are not positively related: \
                 Passing–Bablok regression assumes they are, and the slope's confidence limit is undefined"
            )
        } else {
            "a slope confidence limit is infinite (vertical pairwise slopes)".to_owned()
        }
    });
    let intercept_with = |slope: f64| -> Result<f64> {
        let r: Vec<f64> = x.iter().zip(y).map(|(a, b)| b - slope * a).collect();
        median(&r)
    };
    let a = intercept_with(b)?;
    let (a_lo, a_hi) = if b_lo.is_finite() && b_hi.is_finite() {
        (intercept_with(b_hi)?, intercept_with(b_lo)?)
    } else {
        (f64::NEG_INFINITY, f64::INFINITY)
    };
    let linearity = (b.is_finite() && b != 0.0).then(|| cusum_test(x, y, a, b)).flatten();
    Ok(PassingBablokFit {
        intercept: ComparisonEstimate {
            name: INTERCEPT_NAME.to_owned(),
            estimate: a,
            std_error: None,
            lower: a_lo.min(a_hi),
            upper: a_lo.max(a_hi),
        },
        slope: ComparisonEstimate { name: "Slope".to_owned(), estimate: b, std_error: None, lower: b_lo, upper: b_hi },
        confidence_level,
        n,
        n_slopes: big_n,
        offset: k,
        linearity,
        note,
    })
}

fn cusum_test(x: &[f64], y: &[f64], a: f64, b: f64) -> Option<LinearityTest> {
    let resid: Vec<f64> = x.iter().zip(y).map(|(xi, yi)| yi - (a + b * xi)).collect();
    let above = resid.iter().filter(|&&r| r > 0.0).count();
    let below = resid.iter().filter(|&&r| r < 0.0).count();
    if above == 0 || below == 0 {
        return None;
    }
    let (l, big_l) = (above as f64, below as f64);
    let score = |r: f64| {
        if r > 0.0 {
            (big_l / l).sqrt()
        } else if r < 0.0 {
            -(l / big_l).sqrt()
        } else {
            0.0
        }
    };
    let scale = (1.0 + 1.0 / (b * b)).sqrt();
    let mut order: Vec<(usize, f64)> =
        x.iter().zip(y).enumerate().map(|(i, (xi, yi))| (i, (yi + xi / b - a / b) / scale)).collect();
    order.sort_by(|p, q| p.1.total_cmp(&q.1).then(p.0.cmp(&q.0)));
    let mut sum = 0.0;
    let mut max_cusum: f64 = 0.0;
    let cusum: Vec<(usize, f64)> = order
        .iter()
        .map(|&(i, _)| {
            sum += score(resid[i]);
            max_cusum = max_cusum.max(sum.abs());
            (i, sum)
        })
        .collect();
    let statistic = max_cusum / (l + big_l).sqrt();
    Some(LinearityTest {
        n_above: above,
        n_below: below,
        max_cusum,
        statistic,
        p_value: kolmogorov_sf(statistic),
        cusum,
    })
}

#[cfg(test)]
mod tests;
