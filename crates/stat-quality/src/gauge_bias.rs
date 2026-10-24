//! Gauge accuracy studies: bias and linearity against reference values, and
//! the single-gauge repeatability study of one reference part.
//!
//! # Bias and linearity
//! Parts with known reference values `xᵢ` (from a more accurate method) are
//! measured `nᵢ ≥ 2` times each. Each measurement's **bias** is
//! `b = y − x`. At each reference value the average bias is tested against
//! zero with a one-sample t test on that value's biases
//! (`t = b̄ᵢ / (sᵢ/√nᵢ)`, `nᵢ − 1` df), with a `100(1 − α)%` interval
//! `b̄ᵢ ± t_{α/2} sᵢ/√nᵢ`. The average bias over all measurements is tested the
//! same way.
//!
//! **Linearity** is the change of bias over the operating range: the
//! least-squares line `b = a + c·x` fitted to every individual bias, with
//! `s² = Σ(b − a − c x)²/(n − 2)`, `SE(c) = s/√Sxx`,
//! `SE(a) = s √(1/n + x̄²/Sxx)` and t tests of `c = 0` (no linearity problem)
//! and `a = 0`. The confidence band for the mean bias at `x₀` is
//! `a + c x₀ ± t_{α/2, n−2} s √(1/n + (x₀ − x̄)²/Sxx)`; a gauge whose band
//! contains zero over the whole range shows no evidence of bias anywhere
//! (`LinearityStudy::zero_within_band` checks it at the reference values
//! studied).
//! Given the process variation `PV` (usually 6σ of the process),
//! `%Linearity = 100 |c|` and `%Bias = 100 |b̄| / PV`
//! (Automotive Industry Action Group, *Measurement Systems Analysis Reference
//! Manual*, 4th ed., 2010, ch. III §B; Montgomery, *Introduction to
//! Statistical Quality Control*, §8.7).
//!
//! # Single-gauge repeatability (Cg, Cgk)
//! One reference part with value `x_m` is measured `n` (typically 25–50)
//! times on one gauge, by one operator, under repeatability conditions. With
//! mean `ȳ`, standard deviation `s`, tolerance `T = USL − LSL`, a share `K`
//! of the tolerance allowed for the gauge (default 20%) and a spread of `L`
//! standard deviations (default 6):
//!
//! ```text
//! Cg  = (K/100 · T) / (L · s)
//! Cgk = (K/200 · T − |ȳ − x_m|) / (L/2 · s)
//! ```
//!
//! A gauge is usually accepted when both are at least 1.33. Equivalently the
//! repeatability takes `%Var = 100 L s / T = K / Cg` percent of the tolerance,
//! and repeatability with bias (the band `x_m ± (|ȳ − x_m| + L s/2)`)
//! `100 (L s + 2|ȳ − x_m|) / T` percent. The bias is tested against
//! zero with a one-sample t test (Dietrich & Schulze, *Measurement Process
//! Qualification*, Hanser 2011, ch. 4).
//!
//! # Hysteresis and drift
//! **Hysteresis** is a reading that depends on the direction from which the
//! gauge approaches the value (ISO 22514-7:2021, *Capability of measurement
//! processes*; VDA Volume 5, 2011). With the readings split by the direction
//! of approach into groups `A` (say, from below) and `B` (from above), the
//! hysteresis is `h = ȳ_A − ȳ_B`, with the Welch standard error
//! `√(s²_A/n_A + s²_B/n_B)`, Welch–Satterthwaite degrees of freedom
//! `ν = (s²_A/n_A + s²_B/n_B)² / ((s²_A/n_A)²/(n_A − 1) + (s²_B/n_B)²/(n_B − 1))`,
//! a t test of `h = 0` and the interval `h ± t_{α/2,ν} SE` (Welch 1947,
//! *Biometrika* 34).
//!
//! **Drift** is a steady change of the readings over time: the least-squares
//! slope `c` of the readings `y` on time or run order `t`, with
//! `SE(c) = s/√S_tt`, `s² = Σ(y − a − c t)²/(n − 2)`, a t test of `c = 0` on
//! `n − 2` df and the interval `c ± t_{α/2,n−2} SE(c)`.

use stat_core::distributions::{ContinuousDistribution, StudentsT};
use stat_core::summation::sum_iter;
use stat_core::{Result, StatError};

/// A one-sample t test of an average bias against zero, with its interval.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BiasTest {
    /// Number of measurements.
    pub n: usize,
    /// Average bias `b̄`.
    pub bias: f64,
    /// Standard deviation of the biases (equal to that of the measurements).
    pub std_dev: f64,
    /// `s / √n`.
    pub std_error: f64,
    /// `b̄ / SE` (`None` when the standard deviation is zero).
    pub t: Option<f64>,
    /// Two-sided p-value of `t`, `n − 1` df.
    pub p_value: Option<f64>,
    /// Lower confidence limit of the bias.
    pub lower: f64,
    /// Upper confidence limit of the bias.
    pub upper: f64,
}

/// The bias at one reference value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReferenceBias {
    /// Reference value.
    pub reference: f64,
    /// Mean of the measurements of this reference.
    pub mean: f64,
    /// Test of the bias at this reference.
    pub test: BiasTest,
}

/// One coefficient of the linearity line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineCoefficient {
    /// Estimate.
    pub estimate: f64,
    /// Standard error.
    pub std_error: f64,
    /// `estimate / std_error` (`None` when the standard error is zero).
    pub t: Option<f64>,
    /// Two-sided p-value, `n − 2` df.
    pub p_value: Option<f64>,
    /// Lower confidence limit.
    pub lower: f64,
    /// Upper confidence limit.
    pub upper: f64,
}

/// A bias and linearity study.
#[derive(Debug, Clone, PartialEq)]
pub struct LinearityStudy {
    /// Confidence level of the intervals.
    pub confidence: f64,
    /// Bias at each distinct reference value, in increasing order.
    pub references: Vec<ReferenceBias>,
    /// Average bias over all measurements.
    pub average: BiasTest,
    /// Intercept `a` of the bias line.
    pub intercept: LineCoefficient,
    /// Slope `c` of the bias line (the linearity).
    pub slope: LineCoefficient,
    /// Residual standard deviation `s` of the line.
    pub residual_sd: f64,
    /// Proportion of the variation in bias explained by the line.
    pub r_squared: f64,
    /// Residual degrees of freedom `n − 2`.
    pub df: usize,
    /// Mean reference value `x̄` over all measurements.
    pub mean_reference: f64,
    /// `Σ(x − x̄)²` over all measurements.
    pub sxx: f64,
    /// `t_{α/2, n−2}` for the band.
    pub t_quantile: f64,
    /// Process variation, if given.
    pub process_variation: Option<f64>,
}

impl LinearityStudy {
    /// Fitted bias `a + c x₀` and the half-width of its confidence band.
    #[must_use]
    pub fn band(&self, x0: f64) -> (f64, f64) {
        let n = (self.df + 2) as f64;
        let fit = self.intercept.estimate + self.slope.estimate * x0;
        let half =
            self.t_quantile * self.residual_sd * (1.0 / n + (x0 - self.mean_reference).powi(2) / self.sxx).sqrt();
        (fit, half)
    }

    /// `100 |slope|`.
    #[must_use]
    pub fn percent_linearity(&self) -> f64 {
        100.0 * self.slope.estimate.abs()
    }

    /// `|slope| · PV`, when the process variation is known.
    #[must_use]
    pub fn linearity(&self) -> Option<f64> {
        self.process_variation.map(|pv| self.slope.estimate.abs() * pv)
    }

    /// `100 |average bias| / PV`, when the process variation is known.
    #[must_use]
    pub fn percent_bias(&self) -> Option<f64> {
        self.process_variation.map(|pv| 100.0 * self.average.bias.abs() / pv)
    }

    /// `true` when the band contains zero at every reference value studied
    /// (the smallest and largest among them included); it is checked at
    /// those values only.
    ///
    /// Between them the answer can differ. The values `x₀` where the band
    /// contains zero solve `(a + c x₀)² ≤ t² s² (1/n + (x₀ − x̄)²/Sxx)`. When
    /// the slope is significant (`|c| > t s/√Sxx`) they form one interval,
    /// so zero at the extreme references means zero over the whole studied
    /// range. Otherwise the values where zero is outside the band (if any)
    /// form one bounded interval, which can lie between two references even
    /// though every reference passes.
    #[must_use]
    pub fn zero_within_band(&self) -> bool {
        self.references.iter().all(|r| {
            let (fit, half) = self.band(r.reference);
            (fit - half..=fit + half).contains(&0.0)
        })
    }
}

/// Bias at each reference value and the linearity line.
///
/// `references[i]` is the reference value of the part measured as
/// `measurements[i]`.
///
/// # Errors
/// * [`StatError::DimensionMismatch`] if the slices differ in length.
/// * [`StatError::NonFiniteInput`] for NaN or ±∞.
/// * [`StatError::InvalidArgument`] for fewer than two distinct reference
///   values, a reference measured fewer than twice, a confidence level outside
///   (0, 1) or a process variation that is not positive.
pub fn bias_linearity(
    references: &[f64],
    measurements: &[f64],
    confidence: f64,
    process_variation: Option<f64>,
) -> Result<LinearityStudy> {
    if references.len() != measurements.len() {
        return Err(StatError::DimensionMismatch {
            context: "reference values vs. measurements",
            expected: references.len(),
            found: measurements.len(),
        });
    }
    if references.iter().chain(measurements).any(|v| !v.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "bias study" });
    }
    check_confidence(confidence)?;
    if process_variation.is_some_and(|pv| !(pv > 0.0 && pv.is_finite())) {
        return Err(invalid("the process variation must be positive"));
    }
    let mut distinct: Vec<f64> = references.to_vec();
    distinct.sort_by(f64::total_cmp);
    distinct.dedup();
    if distinct.len() < 2 {
        return Err(invalid("a linearity study needs at least two different reference values"));
    }
    let biases: Vec<f64> = measurements.iter().zip(references).map(|(y, x)| y - x).collect();
    let mut levels = Vec::with_capacity(distinct.len());
    for &x in &distinct {
        let b: Vec<f64> = biases.iter().zip(references).filter(|&(_, r)| *r == x).map(|(b, _)| *b).collect();
        if b.len() < 2 {
            return Err(invalid(format!("reference value {x} is measured only once; measure each at least twice")));
        }
        let test = bias_test(&b, confidence)?;
        levels.push(ReferenceBias { reference: x, mean: x + test.bias, test });
    }
    let average = bias_test(&biases, confidence)?;

    let n = biases.len();
    let nf = n as f64;
    let x_bar = sum_iter(references.iter().copied()) / nf;
    let b_bar = average.bias;
    let sxx = sum_iter(references.iter().map(|x| (x - x_bar).powi(2)));
    let sxy = sum_iter(references.iter().zip(&biases).map(|(x, b)| (x - x_bar) * (b - b_bar)));
    let syy = sum_iter(biases.iter().map(|b| (b - b_bar).powi(2)));
    let slope = sxy / sxx;
    let intercept = b_bar - slope * x_bar;
    let sse = sum_iter(references.iter().zip(&biases).map(|(x, b)| (b - intercept - slope * x).powi(2)));
    let df = n - 2;
    if df == 0 {
        return Err(invalid("a linearity study needs at least three measurements"));
    }
    let s = (sse / df as f64).sqrt();
    let t_quantile = StudentsT::new(df as f64)?.quantile(0.5 + confidence / 2.0)?;
    let t_dist = StudentsT::new(df as f64)?;
    let coefficient = |estimate: f64, se: f64| -> Result<LineCoefficient> {
        let (t, p_value) = if se > 0.0 {
            let t = estimate / se;
            (Some(t), Some(2.0 * t_dist.sf(t.abs())?))
        } else {
            (None, None)
        };
        Ok(LineCoefficient {
            estimate,
            std_error: se,
            t,
            p_value,
            lower: estimate - t_quantile * se,
            upper: estimate + t_quantile * se,
        })
    };
    Ok(LinearityStudy {
        confidence,
        references: levels,
        average,
        intercept: coefficient(intercept, s * (1.0 / nf + x_bar * x_bar / sxx).sqrt())?,
        slope: coefficient(slope, s / sxx.sqrt())?,
        residual_sd: s,
        r_squared: if syy > 0.0 { 1.0 - sse / syy } else { f64::NAN },
        df,
        mean_reference: x_bar,
        sxx,
        t_quantile,
        process_variation,
    })
}

/// Settings of a single-gauge repeatability study.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RepeatabilityOptions {
    /// Reference value `x_m` of the part.
    pub reference: f64,
    /// Tolerance `T = USL − LSL` of the characteristic.
    pub tolerance: f64,
    /// Percentage `K` of the tolerance allowed for the gauge (default 20).
    pub percent_of_tolerance: f64,
    /// Number `L` of standard deviations in the gauge's spread (default 6).
    pub spread: f64,
    /// Confidence level of the bias interval (default 0.95).
    pub confidence: f64,
}

impl RepeatabilityOptions {
    /// Default `K = 20%`, `L = 6`, 95% confidence.
    #[must_use]
    pub fn new(reference: f64, tolerance: f64) -> Self {
        Self { reference, tolerance, percent_of_tolerance: 20.0, spread: 6.0, confidence: 0.95 }
    }
}

/// Result of a single-gauge repeatability study.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RepeatabilityStudy {
    /// The settings.
    pub options: RepeatabilityOptions,
    /// Mean of the measurements.
    pub mean: f64,
    /// Bias test (`ȳ − x_m` against zero).
    pub bias: BiasTest,
    /// Capability of the gauge, `Cg` (`None` when s = 0).
    pub cg: Option<f64>,
    /// Capability with bias, `Cgk` (`None` when s = 0).
    pub cgk: Option<f64>,
    /// `100 L s / T`: the share of the tolerance taken by repeatability.
    pub percent_repeatability: f64,
    /// `100 (L s + 2|bias|) / T`: repeatability and bias together.
    pub percent_repeatability_bias: f64,
}

/// Cg and Cgk of repeated measurements of one reference part.
///
/// # Errors
/// [`StatError::InvalidArgument`] for fewer than two measurements, a
/// tolerance, `K` or `L` that is not positive, or a confidence level outside
/// (0, 1); [`StatError::NonFiniteInput`] for NaN or ±∞.
pub fn repeatability_study(measurements: &[f64], options: &RepeatabilityOptions) -> Result<RepeatabilityStudy> {
    let o = *options;
    if [o.reference, o.tolerance, o.percent_of_tolerance, o.spread].iter().any(|v| !v.is_finite())
        || measurements.iter().any(|v| !v.is_finite())
    {
        return Err(StatError::NonFiniteInput { context: "repeatability study" });
    }
    if o.tolerance <= 0.0 || o.percent_of_tolerance <= 0.0 || o.spread <= 0.0 {
        return Err(invalid("the tolerance, the percentage of it and the spread must be positive"));
    }
    check_confidence(o.confidence)?;
    let biases: Vec<f64> = measurements.iter().map(|y| y - o.reference).collect();
    let bias = bias_test(&biases, o.confidence)?;
    let s = bias.std_dev;
    let allowed = o.percent_of_tolerance / 100.0 * o.tolerance;
    let (cg, cgk) = if s > 0.0 {
        (Some(allowed / (o.spread * s)), Some((allowed / 2.0 - bias.bias.abs()) / (o.spread / 2.0 * s)))
    } else {
        (None, None)
    };
    Ok(RepeatabilityStudy {
        options: o,
        mean: o.reference + bias.bias,
        bias,
        cg,
        cgk,
        percent_repeatability: 100.0 * o.spread * s / o.tolerance,
        percent_repeatability_bias: 100.0 * (o.spread * s + 2.0 * bias.bias.abs()) / o.tolerance,
    })
}

/// The difference in mean reading between two directions of approach.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hysteresis {
    /// Readings in each group.
    pub n: (usize, usize),
    /// Mean of each group.
    pub means: (f64, f64),
    /// Standard deviation of each group.
    pub std_devs: (f64, f64),
    /// `h = ȳ_A − ȳ_B`.
    pub difference: f64,
    /// Welch standard error of `h`.
    pub std_error: f64,
    /// Welch–Satterthwaite degrees of freedom.
    pub df: f64,
    /// `t = h / SE` (`None` when both groups have no spread).
    pub t: Option<f64>,
    /// Two-sided p-value of `h = 0`.
    pub p_value: Option<f64>,
    /// Interval for `h`.
    pub lower: f64,
    /// Upper limit of the interval.
    pub upper: f64,
}

/// Hysteresis between readings approached from one direction (`first`)
/// and the other (`second`), with a `confidence` interval (see the module
/// documentation).
///
/// # Errors
/// [`StatError::InsufficientData`] for fewer than two readings in a group;
/// [`StatError::NonFiniteInput`] for NaN or ±∞; [`StatError::InvalidArgument`]
/// for a confidence level outside (0, 1).
pub fn hysteresis(first: &[f64], second: &[f64], confidence: f64) -> Result<Hysteresis> {
    check_confidence(confidence)?;
    if first.iter().chain(second).any(|v| !v.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "hysteresis" });
    }
    let stats = |x: &[f64]| -> Result<(usize, f64, f64)> {
        let n = x.len();
        if n < 2 {
            return Err(StatError::InsufficientData { required: 2, found: n });
        }
        let m = sum_iter(x.iter().copied()) / n as f64;
        let v = sum_iter(x.iter().map(|y| (y - m).powi(2))) / (n as f64 - 1.0);
        Ok((n, m, v))
    };
    let (na, ma, va) = stats(first)?;
    let (nb, mb, vb) = stats(second)?;
    let (wa, wb) = (va / na as f64, vb / nb as f64);
    let se = (wa + wb).sqrt();
    let difference = ma - mb;
    let df = if wa + wb > 0.0 {
        (wa + wb).powi(2) / (wa * wa / (na as f64 - 1.0) + wb * wb / (nb as f64 - 1.0))
    } else {
        (na + nb - 2) as f64
    };
    let t_dist = StudentsT::new(df)?;
    let q = t_dist.quantile(0.5 + confidence / 2.0)?;
    let (t, p_value) = if se > 0.0 {
        let t = difference / se;
        (Some(t), Some(2.0 * t_dist.sf(t.abs())?))
    } else {
        (None, None)
    };
    Ok(Hysteresis {
        n: (na, nb),
        means: (ma, mb),
        std_devs: (va.sqrt(), vb.sqrt()),
        difference,
        std_error: se,
        df,
        t,
        p_value,
        lower: difference - q * se,
        upper: difference + q * se,
    })
}

/// Steady change of readings over time or run order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Drift {
    /// Readings.
    pub n: usize,
    /// Intercept `a` of the fitted line.
    pub intercept: f64,
    /// Slope `c`: change in reading per unit of time or order.
    pub slope: f64,
    /// `SE(c)`.
    pub std_error: f64,
    /// Residual standard deviation `s`.
    pub residual_sd: f64,
    /// `t = c / SE(c)` (`None` when the readings fall exactly on the line).
    pub t: Option<f64>,
    /// Two-sided p-value of `c = 0`.
    pub p_value: Option<f64>,
    /// Interval for `c`.
    pub lower: f64,
    /// Upper limit of the interval.
    pub upper: f64,
    /// Smallest and largest time or order.
    pub span: (f64, f64),
}

impl Drift {
    /// The fitted change over the whole span, `c (t_max − t_min)`.
    #[must_use]
    pub fn total_change(&self) -> f64 {
        self.slope * (self.span.1 - self.span.0)
    }
}

/// Drift of `readings` over `times` (run order when every time is its
/// index), with a `confidence` interval for the slope.
///
/// # Errors
/// [`StatError::InvalidArgument`] for lengths that differ, times that do not
/// vary or a confidence outside (0, 1); [`StatError::InsufficientData`] for
/// fewer than three readings; [`StatError::NonFiniteInput`] for NaN or ±∞.
pub fn drift(times: &[f64], readings: &[f64], confidence: f64) -> Result<Drift> {
    check_confidence(confidence)?;
    if times.len() != readings.len() {
        return Err(invalid("give one time per reading"));
    }
    let n = readings.len();
    if n < 3 {
        return Err(StatError::InsufficientData { required: 3, found: n });
    }
    if times.iter().chain(readings).any(|v| !v.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "drift" });
    }
    let nf = n as f64;
    let tm = sum_iter(times.iter().copied()) / nf;
    let ym = sum_iter(readings.iter().copied()) / nf;
    let stt = sum_iter(times.iter().map(|t| (t - tm).powi(2)));
    if stt <= 0.0 {
        return Err(invalid("the times (or run order) must vary to estimate a drift"));
    }
    let sty = sum_iter(times.iter().zip(readings).map(|(t, y)| (t - tm) * (y - ym)));
    let slope = sty / stt;
    let intercept = ym - slope * tm;
    let sse = sum_iter(times.iter().zip(readings).map(|(t, y)| (y - intercept - slope * t).powi(2)));
    let s = (sse / (nf - 2.0)).sqrt();
    let se = s / stt.sqrt();
    let t_dist = StudentsT::new(nf - 2.0)?;
    let q = t_dist.quantile(0.5 + confidence / 2.0)?;
    let (t, p_value) = if se > 0.0 {
        let t = slope / se;
        (Some(t), Some(2.0 * t_dist.sf(t.abs())?))
    } else {
        (None, None)
    };
    let span = times.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &t| (a.min(t), b.max(t)));
    Ok(Drift {
        n,
        intercept,
        slope,
        std_error: se,
        residual_sd: s,
        t,
        p_value,
        lower: slope - q * se,
        upper: slope + q * se,
        span,
    })
}

/// One-sample t test of the mean of `b` against zero.
fn bias_test(b: &[f64], confidence: f64) -> Result<BiasTest> {
    let n = b.len();
    if n < 2 {
        return Err(StatError::InsufficientData { required: 2, found: n });
    }
    let nf = n as f64;
    let mean = sum_iter(b.iter().copied()) / nf;
    let sd = (sum_iter(b.iter().map(|v| (v - mean).powi(2))) / (nf - 1.0)).sqrt();
    let se = sd / nf.sqrt();
    let t_dist = StudentsT::new(nf - 1.0)?;
    let q = t_dist.quantile(0.5 + confidence / 2.0)?;
    let (t, p_value) = if se > 0.0 {
        let t = mean / se;
        (Some(t), Some(2.0 * t_dist.sf(t.abs())?))
    } else {
        (None, None)
    };
    Ok(BiasTest { n, bias: mean, std_dev: sd, std_error: se, t, p_value, lower: mean - q * se, upper: mean + q * se })
}

fn check_confidence(c: f64) -> Result<()> {
    if c > 0.0 && c < 1.0 { Ok(()) } else { Err(invalid("the confidence level must lie strictly between 0 and 1")) }
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The gauge linearity example of the AIAG *Measurement Systems Analysis*
    /// reference manual (4th ed., ch. III §B, Table III-B4): five parts with
    /// reference values 2, 4, 6, 8, 10, each measured 12 times. The manual
    /// reports slope −0.1317, intercept 0.7367 and R² = 0.71; the remaining
    /// values were computed independently in Python (standard library only,
    /// `statistics.mean` / `stdev` and the textbook least-squares sums).
    fn aiag() -> (Vec<f64>, Vec<f64>) {
        let data: [(f64, [f64; 12]); 5] = [
            (2.0, [2.70, 2.50, 2.40, 2.50, 2.70, 2.30, 2.50, 2.50, 2.40, 2.40, 2.60, 2.40]),
            (4.0, [5.10, 3.90, 4.20, 5.00, 3.80, 3.90, 3.90, 3.90, 3.90, 4.00, 4.10, 3.80]),
            (6.0, [5.80, 5.70, 5.90, 5.90, 6.00, 6.10, 6.00, 6.10, 6.40, 6.30, 6.00, 6.10]),
            (8.0, [7.60, 7.70, 7.80, 7.70, 7.80, 7.80, 7.80, 7.70, 7.80, 7.50, 7.60, 7.70]),
            (10.0, [9.10, 9.30, 9.50, 9.30, 9.40, 9.50, 9.50, 9.50, 9.60, 9.20, 9.30, 9.40]),
        ];
        let refs = data.iter().flat_map(|(r, v)| std::iter::repeat_n(*r, v.len())).collect();
        let meas = data.iter().flat_map(|(_, v)| v.iter().copied()).collect();
        (refs, meas)
    }

    fn close(a: f64, b: f64, tol: f64) {
        assert!((a - b).abs() <= tol, "{a} vs {b}");
    }

    /// R 4.x `t.test(a, b, var.equal = FALSE)` and `summary(lm(y ~ t))`,
    /// `confint`.
    #[test]
    fn hysteresis_and_drift_match_r() {
        let a = [25.003, 25.002, 25.004, 25.001, 25.003, 25.004, 25.002, 25.003];
        let b = [24.999, 25.000, 24.998, 25.001, 24.999, 25.000, 24.998, 25.002, 25.000];
        let h = hysteresis(&a, &b, 0.95).unwrap();
        close(h.difference, 0.003_083_333_333_333, 1e-12);
        close(h.t.unwrap(), 5.380_674_830_484_13, 1e-8);
        close(h.p_value.unwrap(), 8.014_081_158_489_525e-5, 1e-11);
        close(h.df, 14.794_401_898_186_788, 1e-8);
        close(h.lower, 0.001_860_450_845_914_33, 1e-11);
        close(h.upper, 0.004_306_215_820_750_989, 1e-11);
        assert_eq!(h.n, (8, 9));
        let y = [10.01, 10.02, 10.00, 10.03, 10.03, 10.05, 10.04, 10.06, 10.05, 10.08];
        let t: Vec<f64> = (1..=10).map(f64::from).collect();
        let d = drift(&t, &y, 0.95).unwrap();
        close(d.slope, 0.007_212_121_212_121_074, 1e-12);
        close(d.std_error, 0.001_179_874_080_844_38, 1e-12);
        close(d.t.unwrap(), 6.112_619_413_556_14, 1e-8);
        close(d.p_value.unwrap(), 2.854_763_998_497_55e-4, 1e-10);
        close(d.intercept, 9.997_333_333_333_335, 1e-10);
        close(d.residual_sd, 0.010_716_738_535_976_841, 1e-12);
        close(d.lower, 0.004_491_326_702_673_715, 1e-11);
        close(d.upper, 0.009_932_915_721_568_434, 1e-11);
        close(d.total_change(), 9.0 * d.slope, 1e-15);
        assert!(hysteresis(&a[..1], &b, 0.95).is_err());
        assert!(drift(&[1.0, 1.0, 1.0], &[1.0, 2.0, 3.0], 0.95).is_err());
        assert!(drift(&t[..2], &y[..2], 0.95).is_err());
    }

    #[test]
    fn aiag_linearity_example() {
        let (refs, meas) = aiag();
        let s = bias_linearity(&refs, &meas, 0.95, Some(6.0)).unwrap();
        close(s.slope.estimate, -0.131_666_666_666_666_65, 1e-12);
        close(s.intercept.estimate, 0.736_666_666_666_666_6, 1e-12);
        close(s.r_squared, 0.714_318_415_932_242_1, 1e-12);
        close(s.residual_sd, 0.239_539_788_646_536_94, 1e-12);
        close(s.slope.std_error, 0.010_933_445_471_810_68, 1e-12);
        close(s.intercept.std_error, 0.072_524_272_591_612_58, 1e-12);
        close(s.slope.t.unwrap(), -12.042_559_411_499_166, 1e-9);
        close(s.intercept.t.unwrap(), 10.157_518_860_132_104, 1e-9);
        assert_eq!(s.df, 58);
        assert!(s.slope.p_value.unwrap() < 1e-15);
        // Bias at each reference (Python: statistics.mean / stdev).
        let expected = [
            (0.491_666_666_666_666_64, 0.124_011_240_937_214_66, 13.734_104_112_156_222),
            (0.125, 0.447_467_621_987_639_1, 0.967_696_165_297_475_8),
            (0.025, 0.195_982_373_975_546_3, 0.441_889_434_349_078_96),
            (-0.291_666_666_666_666_74, 0.099_620_491_989_562_17, -10.142_119_868_146_274),
            (-0.616_666_666_666_666_6, 0.146_680_440_124_617_53, -14.563_605_032_727_844),
        ];
        for (r, (bias, sd, t)) in s.references.iter().zip(expected) {
            close(r.test.bias, bias, 1e-12);
            close(r.test.std_dev, sd, 1e-12);
            close(r.test.t.unwrap(), t, 1e-9);
            assert_eq!(r.test.n, 12);
        }
        close(s.average.bias, -0.053_333_333_333_333_35, 1e-12);
        close(s.average.t.unwrap(), -0.929_714_967_624_768_6, 1e-9);
        // t(0.975, 58) = 2.001717 (standard t tables give 2.0017).
        close(s.t_quantile, 2.001_717, 1e-5);
        close(s.percent_linearity(), 13.166_666_666_666_666, 1e-9);
        close(s.linearity().unwrap(), 0.79, 1e-12);
        close(s.percent_bias().unwrap(), 100.0 * 0.053_333_333_333_333_35 / 6.0, 1e-9);
        // The manual's conclusion: the bias = 0 line is not inside the band.
        assert!(!s.zero_within_band());
        let (fit, half) = s.band(s.mean_reference);
        close(fit, s.average.bias, 1e-12);
        close(half, s.t_quantile * s.residual_sd / 60f64.sqrt(), 1e-12);
    }

    #[test]
    fn unbiased_gauge_has_zero_inside_the_band() {
        let refs = [1.0, 1.0, 1.0, 2.0, 2.0, 2.0, 3.0, 3.0, 3.0];
        let meas = [1.01, 0.99, 1.0, 2.02, 1.98, 2.0, 2.99, 3.01, 3.0];
        let s = bias_linearity(&refs, &meas, 0.95, None).unwrap();
        close(s.slope.estimate, 0.0, 1e-12);
        assert!(s.zero_within_band());
        assert!(s.linearity().is_none() && s.percent_bias().is_none());
    }

    #[test]
    fn linearity_validation() {
        assert!(bias_linearity(&[1.0, 1.0], &[1.0, 1.1], 0.95, None).is_err(), "one reference");
        assert!(bias_linearity(&[1.0, 1.0, 2.0], &[1.0, 1.1, 2.0], 0.95, None).is_err(), "single measurement");
        assert!(bias_linearity(&[1.0, 2.0], &[1.0], 0.95, None).is_err());
        assert!(bias_linearity(&[1.0, 1.0, 2.0, 2.0], &[1.0, f64::NAN, 2.0, 2.0], 0.95, None).is_err());
        assert!(bias_linearity(&[1.0, 1.0, 2.0, 2.0], &[1.0, 1.1, 2.0, 2.1], 1.0, None).is_err());
        assert!(bias_linearity(&[1.0, 1.0, 2.0, 2.0], &[1.0, 1.1, 2.0, 2.1], 0.95, Some(0.0)).is_err());
    }

    /// Hand computation: measurements 10.01, 10.03, 9.99, 10.02, 10.00 of a
    /// part with reference 10.00; mean 10.01, bias 0.01,
    /// s² = (0 + 4 + 4 + 1 + 1)·10⁻⁴/4 = 2.5·10⁻⁴, s = 0.0158114.
    /// T = 0.5, K = 20%, L = 6: Cg = 0.1/(6 s) = 1.054093;
    /// Cgk = (0.05 − 0.01)/(3 s) = 0.843274.
    #[test]
    fn cg_cgk_hand_example() {
        let y = [10.01, 10.03, 9.99, 10.02, 10.00];
        let r = repeatability_study(&y, &RepeatabilityOptions::new(10.0, 0.5)).unwrap();
        let s = 2.5e-4_f64.sqrt();
        close(r.bias.std_dev, s, 1e-12);
        close(r.mean, 10.01, 1e-12);
        close(r.bias.bias, 0.01, 1e-12);
        close(r.cg.unwrap(), 0.1 / (6.0 * s), 1e-9);
        close(r.cgk.unwrap(), 0.04 / (3.0 * s), 1e-9);
        close(r.percent_repeatability, 20.0 / r.cg.unwrap(), 1e-9);
        close(r.percent_repeatability_bias, 100.0 * (6.0 * s + 0.02) / 0.5, 1e-9);
        // t = 0.01 / (s/√5) = 1.414214, 4 df: two-sided p = 0.2302 (t tables).
        close(r.bias.t.unwrap(), 0.01 / (s / 5f64.sqrt()), 1e-9);
        close(r.bias.p_value.unwrap(), 0.2302, 5e-4);
        // L = 4 (an alternative convention): Cg = 0.1/(4 s).
        let r4 = repeatability_study(&y, &RepeatabilityOptions { spread: 4.0, ..RepeatabilityOptions::new(10.0, 0.5) })
            .unwrap();
        close(r4.cg.unwrap(), 0.1 / (4.0 * s), 1e-9);
    }

    #[test]
    fn repeatability_validation() {
        let o = RepeatabilityOptions::new(1.0, 1.0);
        assert!(repeatability_study(&[1.0], &o).is_err());
        assert!(repeatability_study(&[1.0, 1.1], &RepeatabilityOptions { tolerance: 0.0, ..o }).is_err());
        assert!(repeatability_study(&[1.0, f64::INFINITY], &o).is_err());
        let constant = repeatability_study(&[1.0, 1.0, 1.0], &o).unwrap();
        assert!(constant.cg.is_none() && constant.bias.t.is_none());
    }
}
