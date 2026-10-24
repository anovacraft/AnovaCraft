//! Differencing and Box–Cox transformation of a series.
//!
//! # Differencing
//!
//! With the backshift operator `B y_t = y_{t−1}`, `d` regular and `D`
//! seasonal differences at period `m` apply
//!
//! ```text
//! w_t = (1 − B)^d (1 − B^m)^D y_t,
//! ```
//!
//! defined for `t = d + mD, …, n − 1`, so `n − d − mD` values remain (Box,
//! Jenkins & Reinsel, *Time Series Analysis*, §4.1 and §9.1). Expanding the
//! operator as `1 + Σ_{j=1}^{K} a_j B^j` (`K = d + mD`) gives the inverse
//! recursion `y_t = w_t − Σ_j a_j y_{t−j}`, which rebuilds the series from its
//! first `K` values or extends it with forecasts of `w`.
//!
//! # Box–Cox
//!
//! The transform `(y^λ − 1)/λ` (`log y` at `λ = 0`) of Box & Cox (1964,
//! *JRSS B* 26:211) is shared with [`stat_core::box_cox`]. For a seasonal
//! series Guerrero (1993, *J. Forecasting* 12:37) chooses λ to stabilise the
//! variance across seasonal cycles: split the series into consecutive
//! subseries of length `R` (the period, or 2 for a non-seasonal series), with
//! means `ȳ_h` and standard deviations `s_h`, and minimise the coefficient of
//! variation of `s_h / ȳ_h^{1−λ}` over `λ ∈ [−1, 2]`.

use stat_core::box_cox::{box_cox_inverse, box_cox_transform};

use crate::{Result, StatError};

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// Regular and seasonal differencing orders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Differencing {
    /// Number of lag-1 differences `d`.
    pub order: usize,
    /// Number of seasonal differences `D`.
    pub seasonal_order: usize,
    /// Seasonal period `m` (ignored when `D = 0`).
    pub period: usize,
}

impl Differencing {
    /// `d` lag-1 differences and `D` lag-`m` differences.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] when `D > 0` and `m < 1`.
    pub fn new(order: usize, seasonal_order: usize, period: usize) -> Result<Self> {
        if seasonal_order > 0 && period < 1 {
            return Err(invalid("the seasonal period must be at least 1"));
        }
        Ok(Self { order, seasonal_order, period })
    }

    /// Number of values lost, `K = d + mD`.
    #[must_use]
    pub fn lost(&self) -> usize {
        self.order.saturating_add(if self.seasonal_order > 0 {
            self.period.saturating_mul(self.seasonal_order)
        } else {
            0
        })
    }

    /// Coefficients `1, a₁, …, a_K` of `(1 − B)^d (1 − B^m)^D`.
    #[must_use]
    pub fn polynomial(&self) -> Vec<f64> {
        let mut p = vec![1.0];
        let mul = |p: &[f64], lag: usize| {
            let mut out = vec![0.0; p.len() + lag];
            for (i, &c) in p.iter().enumerate() {
                out[i] += c;
                out[i + lag] -= c;
            }
            out
        };
        for _ in 0..self.order {
            p = mul(&p, 1);
        }
        for _ in 0..self.seasonal_order {
            p = mul(&p, self.period);
        }
        p
    }

    /// The differenced series `w_t`, `t = K..n`, of length `n − K`.
    ///
    /// # Errors
    /// [`StatError::NonFiniteInput`] for NaN or ±∞;
    /// [`StatError::InsufficientData`] unless `n > K`.
    pub fn apply(&self, y: &[f64]) -> Result<Vec<f64>> {
        if y.iter().any(|v| !v.is_finite()) {
            return Err(StatError::NonFiniteInput { context: "series" });
        }
        let k = self.lost();
        if y.len() <= k {
            return Err(StatError::InsufficientData { required: k.saturating_add(1), found: y.len() });
        }
        // Repeated differencing is exact in floating point for integer data
        // and avoids the cancellation of large binomial coefficients.
        let mut w = y.to_vec();
        for _ in 0..self.seasonal_order {
            w = (self.period..w.len()).map(|t| w[t] - w[t - self.period]).collect();
        }
        for _ in 0..self.order {
            w = (1..w.len()).map(|t| w[t] - w[t - 1]).collect();
        }
        Ok(w)
    }

    /// Invert the differencing: given the `K` original values immediately
    /// before the first differenced value (`history`; extra leading values are
    /// ignored) and differenced values `w`, return the `w.len()` original
    /// values that follow. Pass the first `K` observations to rebuild a
    /// series, or the last `K` observations and forecasts of `w` to turn them
    /// into forecasts of `y`.
    ///
    /// # Errors
    /// [`StatError::InsufficientData`] when `history` has fewer than `K`
    /// values; [`StatError::NonFiniteInput`] for NaN or ±∞ in `history`.
    pub fn integrate(&self, history: &[f64], w: &[f64]) -> Result<Vec<f64>> {
        let k = self.lost();
        if history.len() < k {
            return Err(StatError::InsufficientData { required: k, found: history.len() });
        }
        if history.iter().any(|v| !v.is_finite()) {
            return Err(StatError::NonFiniteInput { context: "initial values" });
        }
        // Undo the operators one at a time (each a running sum), mirroring
        // `apply`; this keeps the rounding of integer data exact.
        // Stage values: the history differenced by the first j operators.
        let mut stages: Vec<(Vec<f64>, usize)> = Vec::new();
        let mut h = history[history.len() - k..].to_vec();
        for _ in 0..self.seasonal_order {
            stages.push((h.clone(), self.period));
            h = (self.period..h.len()).map(|t| h[t] - h[t - self.period]).collect();
        }
        for _ in 0..self.order {
            stages.push((h.clone(), 1));
            h = (1..h.len()).map(|t| h[t] - h[t - 1]).collect();
        }
        let mut out = w.to_vec();
        for (hist, lag) in stages.into_iter().rev() {
            let mut full = hist;
            let start = full.len();
            full.extend_from_slice(&out);
            for t in start..full.len() {
                full[t] += full[t - lag];
            }
            out = full.split_off(start);
        }
        Ok(out)
    }
}

/// `d` lag-1 and `D` lag-`m` differences of `y`; see [`Differencing::apply`].
///
/// # Errors
/// As [`Differencing::new`] and [`Differencing::apply`].
pub fn difference(y: &[f64], order: usize, seasonal_order: usize, period: usize) -> Result<Vec<f64>> {
    Differencing::new(order, seasonal_order, period)?.apply(y)
}

/// Box–Cox transform of every value: `(y^λ − 1)/λ`, `log y` at `λ = 0`.
///
/// # Errors
/// [`StatError::InvalidArgument`] unless every value is positive and finite
/// (for every λ, as in [`stat_core::box_cox`]), or λ is not finite.
pub fn box_cox(y: &[f64], lambda: f64) -> Result<Vec<f64>> {
    if !lambda.is_finite() {
        return Err(invalid("λ must be finite"));
    }
    box_cox_transform(y, lambda)
}

/// Inverse Box–Cox transform `(λz + 1)^{1/λ}` (`exp z` at `λ = 0`); NaN where
/// `λz + 1 ≤ 0`, which lies outside the transform's range.
#[must_use]
pub fn inverse_box_cox(z: &[f64], lambda: f64) -> Vec<f64> {
    z.iter().map(|&v| box_cox_inverse(v, lambda)).collect()
}

/// Inverse Box–Cox transform of prediction limits, clipped to the range of
/// the transform first: for `λ > 0` the transformed values are bounded below
/// by `−1/λ`, so a lower value below it is moved to `−1/λ` and comes back as
/// 0; for `λ < 0` they are bounded above by `−1/λ`, so a value above it comes
/// back as `+∞`. Returns the back-transformed values and how many were
/// clipped. (`λ = 0`, the log, has no bound.) NaN is never produced from a
/// finite input.
#[must_use]
pub fn inverse_box_cox_clipped(z: &[f64], lambda: f64) -> (Vec<f64>, usize) {
    let mut clipped = 0;
    let out = z
        .iter()
        .map(|&v| {
            if lambda != 0.0 && lambda * v + 1.0 <= 0.0 {
                clipped += 1;
                if lambda > 0.0 { 0.0 } else { f64::INFINITY }
            } else {
                box_cox_inverse(v, lambda)
            }
        })
        .collect();
    (out, clipped)
}

fn guerrero_cv(groups: &[(f64, f64)], lambda: f64) -> f64 {
    let ratios: Vec<f64> = groups.iter().map(|&(m, s)| s / m.powf(1.0 - lambda)).collect();
    let k = ratios.len() as f64;
    let mean = ratios.iter().sum::<f64>() / k;
    let var = ratios.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / (k - 1.0);
    var.sqrt() / mean
}

/// Guerrero's (1993) choice of the Box–Cox λ in `[lower, upper]` (normally
/// `[−1, 2]`): the λ minimising the coefficient of variation (sample sd over
/// mean) of `s_h / ȳ_h^{1−λ}` across consecutive non-overlapping subseries
/// of length `R = max(period, 2)` ending at the last observation (an
/// incomplete leading subseries is dropped, as in R's
/// `forecast::BoxCox.lambda(method = "guerrero")`). The minimum is located on a grid
/// of 0.01 and refined by golden-section search to about 10⁻⁹.
///
/// # Errors
/// [`StatError::InvalidArgument`] for non-positive or non-finite values or
/// an empty interval; [`StatError::InsufficientData`] for fewer than two
/// complete subseries.
pub fn guerrero_lambda(y: &[f64], period: usize, lower: f64, upper: f64) -> Result<f64> {
    if y.iter().any(|v| !(v.is_finite() && *v > 0.0)) {
        return Err(invalid("Guerrero's method needs positive values"));
    }
    if !(lower.is_finite() && upper.is_finite() && lower < upper) {
        return Err(invalid("the λ interval must be finite and non-empty"));
    }
    let r = period.max(2);
    let groups: Vec<(f64, f64)> = y[y.len() % r..]
        .chunks_exact(r)
        .map(|c| {
            let m = c.iter().sum::<f64>() / r as f64;
            let s = (c.iter().map(|v| (v - m).powi(2)).sum::<f64>() / (r - 1) as f64).sqrt();
            (m, s)
        })
        .collect();
    if groups.len() < 2 {
        return Err(StatError::InsufficientData { required: 2 * r, found: y.len() });
    }
    if groups.iter().all(|g| g.1 == 0.0) {
        return Err(invalid("every subseries is constant, so λ is not identified"));
    }
    let f = |l: f64| guerrero_cv(&groups, l);
    let steps = ((upper - lower) / 0.01).ceil().max(2.0) as usize;
    let grid: Vec<f64> = (0..=steps).map(|i| lower + (upper - lower) * i as f64 / steps as f64).collect();
    let best = (0..grid.len()).min_by(|&a, &b| f(grid[a]).total_cmp(&f(grid[b]))).unwrap_or(0);
    let (mut a, mut b) = (grid[best.saturating_sub(1)], grid[(best + 1).min(steps)]);
    let g = (5f64.sqrt() - 1.0) / 2.0;
    let (mut c, mut d) = (b - g * (b - a), a + g * (b - a));
    let (mut fc, mut fd) = (f(c), f(d));
    while b - a > 1e-10 {
        if fc < fd {
            b = d;
            d = c;
            fd = fc;
            c = b - g * (b - a);
            fc = f(c);
        } else {
            a = c;
            c = d;
            fc = fd;
            d = a + g * (b - a);
            fd = f(d);
        }
    }
    Ok((a + b) / 2.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn differencing_round_trip() {
        let y: Vec<f64> = (0..60).map(|t| (t as f64 * 0.7).sin() * 10.0 + t as f64 * t as f64 * 0.1).collect();
        for (d, dd, m) in [(0, 0, 1), (1, 0, 1), (2, 0, 1), (1, 1, 12), (0, 2, 4), (2, 1, 4)] {
            let op = Differencing::new(d, dd, m).unwrap();
            let w = op.apply(&y).unwrap();
            assert_eq!(w.len(), y.len() - op.lost());
            let k = op.lost();
            let back = op.integrate(&y[..k], &w).unwrap();
            for (a, b) in back.iter().zip(&y[k..]) {
                assert!((a - b).abs() < 1e-9, "{d} {dd} {m}");
            }
            // The polynomial form agrees with repeated differencing.
            let p = op.polynomial();
            for t in k..y.len() {
                let v: f64 = p.iter().enumerate().map(|(j, c)| c * y[t - j]).sum();
                assert!((v - w[t - k]).abs() < 1e-8);
            }
            // Forecast use: the tail of the series plus future differences.
            let split = 40;
            let ext = op.integrate(&y[..split], &w[split - k..]).unwrap();
            for (a, b) in ext.iter().zip(&y[split..]) {
                assert!((a - b).abs() < 1e-9);
            }
        }
    }

    #[test]
    fn rejects_short_or_bad_series() {
        assert!(difference(&[1.0, 2.0], 1, 1, 12).is_err());
        assert!(difference(&[1.0, f64::NAN, 3.0], 1, 0, 1).is_err());
        assert!(Differencing::new(1, 0, 1).unwrap().integrate(&[], &[1.0]).is_err());
        assert!(box_cox(&[1.0, 0.0], 0.5).is_err());
        assert!(guerrero_lambda(&[1.0, 2.0, 3.0], 12, -1.0, 2.0).is_err());
    }

    /// Values beyond the transform's range come back at its edge: 0 below
    /// −1/λ for λ > 0, +∞ above −1/λ for λ < 0; inside the range the inverse
    /// is unchanged.
    #[test]
    fn clipped_inverse_box_cox() {
        let (v, k) = inverse_box_cox_clipped(&[-2.94, -2.0, 0.0, 1.0], 0.5);
        assert_eq!(k, 2);
        assert_eq!(&v[..2], &[0.0, 0.0]);
        assert!((v[2] - 1.0).abs() < 1e-12 && (v[3] - 2.25).abs() < 1e-12);
        let (v, k) = inverse_box_cox_clipped(&[3.0, 0.0], -0.5);
        assert_eq!(k, 1);
        assert!(v[0].is_infinite() && v[0] > 0.0 && (v[1] - 1.0).abs() < 1e-12);
        let (v, k) = inverse_box_cox_clipped(&[-50.0], 0.0);
        assert_eq!(k, 0);
        assert!(v[0] > 0.0);
    }

    /// A huge seasonal period errors instead of overflowing.
    #[test]
    fn huge_period_errors() {
        let op = Differencing::new(1, 1, usize::MAX).unwrap();
        assert_eq!(op.lost(), usize::MAX);
        assert!(matches!(op.apply(&[1.0, 2.0, 3.0]), Err(StatError::InsufficientData { .. })));
        assert!(difference(&[1.0, 2.0, 3.0], 0, 2, usize::MAX / 2 + 1).is_err());
    }

    #[test]
    fn box_cox_inverse_round_trip() {
        let y = [0.5, 1.0, 3.0, 10.0];
        for l in [-1.0, -0.3, 0.0, 0.5, 1.0, 2.0] {
            let back = inverse_box_cox(&box_cox(&y, l).unwrap(), l);
            for (a, b) in back.iter().zip(&y) {
                assert!((a - b).abs() < 1e-12 * b);
            }
        }
    }

    #[test]
    fn guerrero_recovers_power_law_variance() {
        // Cycles whose sd is proportional to the level: λ = 0 makes s/ȳ constant.
        let mut y = Vec::new();
        for c in 0..10 {
            let level = 10.0 * 1.3f64.powi(c);
            for j in 0..12 {
                y.push(level * (1.0 + 0.2 * ((j as f64) * 0.5).sin()));
            }
        }
        let l = guerrero_lambda(&y, 12, -1.0, 2.0).unwrap();
        assert!(l.abs() < 1e-6, "{l}");
    }
}
