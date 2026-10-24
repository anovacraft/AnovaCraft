//! Smoothing filters for evenly spaced signals.
//!
//! # Savitzky–Golay
//! Savitzky & Golay ("Smoothing and differentiation of data by simplified
//! least squares procedures", *Analytical Chemistry* 36, 1964): at each point
//! a polynomial of degree `k` is fitted by least squares to the `w = 2m + 1`
//! points centred on it, and the smoothed value (or its `d`-th derivative)
//! is the polynomial's value (derivative) at the centre. For evenly spaced
//! data the fit is a fixed linear combination of the window's values,
//!
//! ```text
//! ŷᵢ⁽ᵈ⁾ = Σⱼ₌₋ₘᵐ hⱼ yᵢ₊ⱼ,   hⱼ = d! · [(AᵀA)⁻¹Aᵀ]_{d,j} / Δᵈ,
//! ```
//!
//! with `A` the `w × (k+1)` Vandermonde matrix of offsets `−m … m` and `Δ`
//! the spacing. Near the ends, where a centred window does not fit, a single
//! polynomial is fitted to the first (last) `w` points and evaluated at the
//! first (last) `m` positions (SciPy's `mode="interp"`).
//!
//! # Other filters
//! Centred filters see the signal extended beyond its ends by mirror
//! reflection including the end value (`… y₂ y₁ y₀ | y₀ y₁ y₂ …`, SciPy's
//! `mode="reflect"`):
//!
//! * **moving average** of `w` points (centred), or of the current and up to
//!   `w − 1` previous points (trailing, fewer at the start);
//! * **exponential smoothing** `sᵢ = α yᵢ + (1 − α) sᵢ₋₁`, `s₀ = y₀`;
//! * **running median** of `w` points (Tukey, *Exploratory Data Analysis*,
//!   1977), resistant to spikes; the **running percentile** takes the value
//!   of rank `⌊w·p/100⌋` (from 0; `w − 1` at p = 100) of the sorted window;
//! * **Gaussian kernel** with standard deviation `σ` samples, weights
//!   `exp(−j²/2σ²)` normalized over `|j| ≤ ⌊4σ + ½⌋`.

use stat_core::matrix::Matrix;
use stat_core::qr::QrDecomposition;

use crate::{Result, invalid};

/// Which window a moving average uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MovingAverage {
    /// `w` points centred on each point (w odd).
    Centred,
    /// The current point and the `w − 1` before it.
    Trailing,
}

/// A smoothing filter and its settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Smoother {
    /// Savitzky–Golay polynomial smoothing (or a derivative).
    SavitzkyGolay {
        /// Window length `w` (odd, greater than `order`).
        window: usize,
        /// Polynomial degree `k`.
        order: usize,
        /// Derivative `d` (0: the smoothed values; at most `order`).
        derivative: usize,
    },
    /// Moving average of `window` points.
    MovingAverage {
        /// Window length (odd for a centred average).
        window: usize,
        /// Centred or trailing.
        kind: MovingAverage,
    },
    /// Exponential smoothing with weight `alpha` on the newest value.
    Exponential {
        /// Smoothing weight, `0 < α ≤ 1`.
        alpha: f64,
    },
    /// Running median of `window` points (odd).
    RunningMedian {
        /// Window length.
        window: usize,
    },
    /// Running percentile of `window` points (odd).
    RunningPercentile {
        /// Window length.
        window: usize,
        /// Percentile, 0–100.
        percentile: f64,
    },
    /// Gaussian kernel with standard deviation `sigma` samples.
    Gaussian {
        /// Kernel standard deviation in samples (positive).
        sigma: f64,
    },
}

impl Smoother {
    /// A short description, e.g. `"Savitzky–Golay (11 points, degree 3)"`.
    #[must_use]
    pub fn label(&self) -> String {
        match *self {
            Self::SavitzkyGolay { window, order, derivative: 0 } => {
                format!("Savitzky–Golay ({window} points, degree {order})")
            }
            Self::SavitzkyGolay { window, order, derivative } => {
                format!("Savitzky–Golay derivative {derivative} ({window} points, degree {order})")
            }
            Self::MovingAverage { window, kind: MovingAverage::Centred } => {
                format!("Centred moving average ({window} points)")
            }
            Self::MovingAverage { window, kind: MovingAverage::Trailing } => {
                format!("Trailing moving average ({window} points)")
            }
            Self::Exponential { alpha } => format!("Exponential smoothing (α = {alpha})"),
            Self::RunningMedian { window } => format!("Running median ({window} points)"),
            Self::RunningPercentile { window, percentile } => {
                format!("Running {percentile}th percentile ({window} points)")
            }
            Self::Gaussian { sigma } => format!("Gaussian kernel (σ = {sigma} samples)"),
        }
    }
}

/// Index of the reflected extension (`… y₁ y₀ | y₀ y₁ …`) of a signal of
/// length `n` at position `i`.
fn reflect(i: isize, n: usize) -> usize {
    let n = n as isize;
    let period = 2 * n;
    let mut j = i.rem_euclid(period);
    if j >= n {
        j = period - 1 - j;
    }
    j as usize
}

fn odd_window(window: usize, what: &str) -> Result<usize> {
    if window == 0 || window % 2 == 0 {
        return Err(invalid(format!("the {what} window must be an odd number of points (got {window})")));
    }
    Ok(window / 2)
}

/// Coefficients of a polynomial of degree `order` fitted by least squares to
/// `y` at positions `z`.
fn polyfit(z: &[f64], y: &[f64], order: usize) -> Result<Vec<f64>> {
    let mut a = Matrix::zeros(z.len(), order + 1);
    for (i, &zi) in z.iter().enumerate() {
        let mut p = 1.0;
        for q in 0..=order {
            a[(i, q)] = p;
            p *= zi;
        }
    }
    QrDecomposition::new(a)?.solve_least_squares(y)
}

/// `d`-th derivative at `z` of the polynomial with coefficients `c`.
fn poly_derivative(c: &[f64], d: usize, z: f64) -> f64 {
    let mut sum = 0.0;
    let mut power = 1.0;
    for q in d..c.len() {
        let falling: f64 = ((q - d + 1)..=q).map(|v| v as f64).product();
        sum += c[q] * falling * power;
        power *= z;
    }
    sum
}

/// Savitzky–Golay coefficients `h₋ₘ … hₘ` for a centred window of `window`
/// points, polynomial degree `order`, derivative `derivative` and spacing
/// `delta`: the filtered value is `Σ hⱼ yᵢ₊ⱼ`.
///
/// # Errors
/// [`crate::StatError::InvalidArgument`] for an even window, a degree not below
/// the window, a derivative above the degree, or a non-positive spacing.
pub fn savitzky_golay_coefficients(window: usize, order: usize, derivative: usize, delta: f64) -> Result<Vec<f64>> {
    let m = odd_window(window, "Savitzky–Golay")?;
    check_sg(window, order, derivative, delta)?;
    let z: Vec<f64> = (0..window).map(|j| j as f64 - m as f64).collect();
    let mut a = Matrix::zeros(window, order + 1);
    for (i, &zi) in z.iter().enumerate() {
        let mut p = 1.0;
        for q in 0..=order {
            a[(i, q)] = p;
            p *= zi;
        }
    }
    let qr = QrDecomposition::new(a)?;
    let factorial: f64 = (1..=derivative).map(|v| v as f64).product();
    let scale = factorial / delta.powi(derivative as i32);
    let mut h = Vec::with_capacity(window);
    let mut e = vec![0.0; window];
    for j in 0..window {
        e.fill(0.0);
        e[j] = 1.0;
        h.push(qr.solve_least_squares(&e)?[derivative] * scale);
    }
    Ok(h)
}

fn check_sg(window: usize, order: usize, derivative: usize, delta: f64) -> Result<()> {
    if order >= window {
        return Err(invalid(format!(
            "the polynomial degree ({order}) must be less than the window length ({window})"
        )));
    }
    if derivative > order {
        return Err(invalid(format!("the derivative ({derivative}) cannot exceed the polynomial degree ({order})")));
    }
    if !(delta.is_finite() && delta > 0.0) {
        return Err(invalid("the spacing between points must be positive"));
    }
    Ok(())
}

/// Savitzky–Golay smoothing (`derivative = 0`) or derivative of the evenly
/// spaced signal `y` with spacing `delta`, with polynomial fits to the first
/// and last `window` points near the ends (see the module documentation).
///
/// # Errors
/// [`crate::StatError::InvalidArgument`] for invalid settings (see
/// [`savitzky_golay_coefficients`]) or a signal shorter than the window.
pub fn savitzky_golay(y: &[f64], window: usize, order: usize, derivative: usize, delta: f64) -> Result<Vec<f64>> {
    let h = savitzky_golay_coefficients(window, order, derivative, delta)?;
    let n = y.len();
    if n < window {
        return Err(invalid(format!("the signal has {n} points, fewer than the {window}-point window")));
    }
    let m = window / 2;
    let mut out = vec![0.0; n];
    for i in m..n - m {
        out[i] = h.iter().zip(&y[i - m..=i + m]).map(|(a, b)| a * b).sum();
    }
    if m > 0 {
        // Edges: one polynomial through the first (last) `window` points.
        let z: Vec<f64> = (0..window).map(|j| j as f64 - m as f64).collect();
        let scale = delta.powi(derivative as i32);
        let left = polyfit(&z, &y[..window], order)?;
        let right = polyfit(&z, &y[n - window..], order)?;
        for i in 0..m {
            out[i] = poly_derivative(&left, derivative, z[i]) / scale;
            out[n - m + i] = poly_derivative(&right, derivative, z[window - m + i]) / scale;
        }
    }
    Ok(out)
}

fn window_values(y: &[f64], i: usize, m: usize, buf: &mut Vec<f64>) {
    buf.clear();
    let n = y.len();
    for k in -(m as isize)..=(m as isize) {
        buf.push(y[reflect(i as isize + k, n)]);
    }
}

fn rank_filter(y: &[f64], window: usize, rank: usize) -> Vec<f64> {
    let m = window / 2;
    let mut buf = Vec::with_capacity(window);
    (0..y.len())
        .map(|i| {
            window_values(y, i, m, &mut buf);
            let (_, v, _) = buf.select_nth_unstable_by(rank, f64::total_cmp);
            *v
        })
        .collect()
}

fn correlate_reflect(y: &[f64], weights: &[f64]) -> Vec<f64> {
    let m = weights.len() / 2;
    let n = y.len();
    (0..n)
        .map(|i| {
            weights.iter().enumerate().map(|(j, w)| w * y[reflect(i as isize + j as isize - m as isize, n)]).sum()
        })
        .collect()
}

/// Apply `smoother` to the evenly spaced signal `y` (spacing `delta`, used
/// only by Savitzky–Golay derivatives).
///
/// # Errors
/// [`crate::StatError::InvalidArgument`] for an empty signal, a non-finite
/// value, or invalid settings (even or zero windows, α outside (0, 1], a
/// percentile outside 0–100, a non-positive σ).
pub fn smooth(y: &[f64], smoother: &Smoother, delta: f64) -> Result<Vec<f64>> {
    if y.is_empty() {
        return Err(invalid("the signal has no values"));
    }
    if y.iter().any(|v| !v.is_finite()) {
        return Err(invalid("the signal must not contain missing or infinite values"));
    }
    match *smoother {
        Smoother::SavitzkyGolay { window, order, derivative } => savitzky_golay(y, window, order, derivative, delta),
        Smoother::MovingAverage { window, kind: MovingAverage::Centred } => {
            odd_window(window, "moving average")?;
            Ok(correlate_reflect(y, &vec![1.0 / window as f64; window]))
        }
        Smoother::MovingAverage { window, kind: MovingAverage::Trailing } => {
            if window == 0 {
                return Err(invalid("the moving average needs at least one point"));
            }
            Ok((0..y.len())
                .map(|i| {
                    let from = (i + 1).saturating_sub(window);
                    y[from..=i].iter().sum::<f64>() / (i + 1 - from) as f64
                })
                .collect())
        }
        Smoother::Exponential { alpha } => {
            if !(alpha > 0.0 && alpha <= 1.0) {
                return Err(invalid("the smoothing weight α must lie in (0, 1]"));
            }
            let mut s = y[0];
            Ok(y.iter()
                .map(|&v| {
                    s = alpha * v + (1.0 - alpha) * s;
                    s
                })
                .collect())
        }
        Smoother::RunningMedian { window } => {
            let m = odd_window(window, "running median")?;
            Ok(rank_filter(y, window, m))
        }
        Smoother::RunningPercentile { window, percentile } => {
            odd_window(window, "running percentile")?;
            if !(0.0..=100.0).contains(&percentile) {
                return Err(invalid("the percentile must lie between 0 and 100"));
            }
            let rank = if percentile == 100.0 {
                window - 1
            } else {
                ((window as f64 * percentile / 100.0) as usize).min(window - 1)
            };
            Ok(rank_filter(y, window, rank))
        }
        Smoother::Gaussian { sigma } => {
            if !(sigma.is_finite() && sigma > 0.0) {
                return Err(invalid("the kernel's standard deviation must be positive"));
            }
            let radius = (4.0 * sigma + 0.5) as usize;
            let mut w: Vec<f64> =
                (0..=2 * radius).map(|j| (-0.5 * ((j as f64 - radius as f64) / sigma).powi(2)).exp()).collect();
            let total: f64 = w.iter().sum();
            for v in &mut w {
                *v /= total;
            }
            Ok(correlate_reflect(y, &w))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn savitzky_golay_classic_coefficients() {
        // Savitzky & Golay (1964), Table I: 5 points, quadratic: (−3, 12, 17, 12, −3)/35.
        let h = savitzky_golay_coefficients(5, 2, 0, 1.0).unwrap();
        for (a, b) in h.iter().zip([-3.0, 12.0, 17.0, 12.0, -3.0]) {
            assert!((a - b / 35.0).abs() < 1e-14);
        }
        // First derivative, 5 points, quadratic: (−2, −1, 0, 1, 2)/10.
        let d = savitzky_golay_coefficients(5, 2, 1, 1.0).unwrap();
        for (a, b) in d.iter().zip([-2.0, -1.0, 0.0, 1.0, 2.0]) {
            assert!((a - b / 10.0).abs() < 1e-14);
        }
    }

    #[test]
    fn savitzky_golay_reproduces_polynomials() {
        let x: Vec<f64> = (0..30).map(|i| 0.5 * i as f64).collect();
        let y: Vec<f64> = x.iter().map(|t| 1.0 + 2.0 * t - 0.3 * t * t + 0.01 * t * t * t).collect();
        let s = savitzky_golay(&y, 9, 3, 0, 0.5).unwrap();
        let d = savitzky_golay(&y, 9, 3, 1, 0.5).unwrap();
        for i in 0..30 {
            assert!((s[i] - y[i]).abs() < 1e-9, "{i}");
            let t = x[i];
            assert!((d[i] - (2.0 - 0.6 * t + 0.03 * t * t)).abs() < 1e-9, "{i}");
        }
    }

    #[test]
    fn reflected_filters() {
        let y = [1.0, 5.0, 2.0, 8.0, 3.0];
        // Reflect: 1 | 1 5 2 8 3 | 3.
        let ma = smooth(&y, &Smoother::MovingAverage { window: 3, kind: MovingAverage::Centred }, 1.0).unwrap();
        assert!((ma[0] - 7.0 / 3.0).abs() < 1e-14 && (ma[4] - 14.0 / 3.0).abs() < 1e-14);
        let med = smooth(&y, &Smoother::RunningMedian { window: 3 }, 1.0).unwrap();
        assert_eq!(med, vec![1.0, 2.0, 5.0, 3.0, 3.0]);
        let tr = smooth(&y, &Smoother::MovingAverage { window: 2, kind: MovingAverage::Trailing }, 1.0).unwrap();
        assert_eq!(tr, vec![1.0, 3.0, 3.5, 5.0, 5.5]);
        let ex = smooth(&y, &Smoother::Exponential { alpha: 0.5 }, 1.0).unwrap();
        assert_eq!(ex, vec![1.0, 3.0, 2.5, 5.25, 4.125]);
        assert!(smooth(&y, &Smoother::RunningMedian { window: 4 }, 1.0).is_err());
    }
}
