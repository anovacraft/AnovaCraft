//! Shared result types and helpers.

use stat_core::{Result, StatError};

/// The alternative hypothesis about a parameter `θ` and its null value `θ₀`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum Alternative {
    /// `θ ≠ θ₀`.
    #[default]
    TwoSided,
    /// `θ < θ₀`.
    Less,
    /// `θ > θ₀`.
    Greater,
}

impl Alternative {
    /// The relation symbol of the alternative (`≠`, `<`, `>`).
    #[must_use]
    pub fn symbol(self) -> &'static str {
        match self {
            Self::TwoSided => "≠",
            Self::Less => "<",
            Self::Greater => ">",
        }
    }

    /// Short description.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::TwoSided => "two-sided",
            Self::Less => "less than",
            Self::Greater => "greater than",
        }
    }

    /// p-value from the probabilities below and above the observed statistic:
    /// `Less` → lower tail, `Greater` → upper tail, two-sided → twice the
    /// smaller tail (capped at 1).
    #[must_use]
    pub fn p_value(self, lower_tail: f64, upper_tail: f64) -> f64 {
        match self {
            Self::TwoSided => (2.0 * lower_tail.min(upper_tail)).min(1.0),
            Self::Less => lower_tail,
            Self::Greater => upper_tail,
        }
    }

    /// Tail probability placed on each bounded side of a confidence interval
    /// at `level`: `α/2` two-sided, `α` one-sided.
    #[must_use]
    pub fn tail(self, level: f64) -> f64 {
        match self {
            Self::TwoSided => (1.0 - level) / 2.0,
            _ => 1.0 - level,
        }
    }

    /// Replace the unbounded side of `(lower, upper)` for a one-sided
    /// alternative: `Less` gives `(−∞, upper]`, `Greater` gives `[lower, ∞)`.
    #[must_use]
    pub fn bound(self, lower: f64, upper: f64) -> (f64, f64) {
        match self {
            Self::TwoSided => (lower, upper),
            Self::Less => (f64::NEG_INFINITY, upper),
            Self::Greater => (lower, f64::INFINITY),
        }
    }
}

/// One hypothesis test with its estimate and confidence interval.
#[derive(Debug, Clone, PartialEq)]
pub struct TestResult {
    /// Name of the test.
    pub method: &'static str,
    /// Point estimate of the parameter tested.
    pub estimate: f64,
    /// Value of the parameter under the null hypothesis.
    pub null_value: f64,
    /// Standard error of the estimate (NaN when not used by the test).
    pub std_error: f64,
    /// Test statistic.
    pub statistic: f64,
    /// Symbol of the statistic (`t`, `z`, `χ²`, `F`, `V`, `U`, `S`, …).
    pub statistic_name: &'static str,
    /// Degrees of freedom (numerator for F), if any.
    pub df: Option<f64>,
    /// Denominator degrees of freedom of an F statistic.
    pub df2: Option<f64>,
    /// p-value for [`alternative`](Self::alternative).
    pub p_value: f64,
    /// Confidence interval matching the alternative (one-sided intervals have
    /// an infinite side). NaN limits mean no interval is available.
    pub confidence_interval: (f64, f64),
    /// Confidence level of the interval.
    pub level: f64,
    /// Alternative hypothesis.
    pub alternative: Alternative,
}

/// A chi-square test (statistic, degrees of freedom, upper-tail p-value).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChiSquareResult {
    /// χ² statistic.
    pub statistic: f64,
    /// Degrees of freedom.
    pub df: f64,
    /// Upper-tail p-value.
    pub p_value: f64,
}

pub(crate) fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

pub(crate) fn check_level(level: f64) -> Result<()> {
    if level > 0.0 && level < 1.0 {
        Ok(())
    } else {
        Err(invalid(format!("confidence level must lie strictly between 0 and 1 (got {level})")))
    }
}

pub(crate) fn check_finite(x: &[f64], context: &'static str) -> Result<()> {
    if x.iter().all(|v| v.is_finite()) { Ok(()) } else { Err(StatError::NonFiniteInput { context }) }
}

/// Sample mean and variance (divisor `n − 1`), two-pass.
pub(crate) fn mean_var(x: &[f64]) -> (f64, f64) {
    let n = x.len() as f64;
    let m = stat_core::summation::sum(x) / n;
    let ss = stat_core::summation::sum_iter(x.iter().map(|v| (v - m) * (v - m)));
    (m, if x.len() > 1 { ss / (n - 1.0) } else { f64::NAN })
}

/// Mid-ranks of `x` (1-based) and the tie sum `Σ(t³ − t)`.
pub(crate) fn ranks_with_ties(x: &[f64]) -> (Vec<f64>, f64) {
    let mut order: Vec<usize> = (0..x.len()).collect();
    order.sort_by(|&a, &b| x[a].total_cmp(&x[b]));
    let mut ranks = vec![0.0; x.len()];
    let mut ties = 0.0;
    let mut i = 0;
    while i < order.len() {
        let mut j = i + 1;
        while j < order.len() && x[order[j]] == x[order[i]] {
            j += 1;
        }
        let rank = (i + j + 1) as f64 / 2.0;
        for &k in &order[i..j] {
            ranks[k] = rank;
        }
        let t = (j - i) as f64;
        ties += t * t * t - t;
        i = j;
    }
    (ranks, ties)
}

/// Median of a slice (sorted copy).
pub(crate) fn median(x: &[f64]) -> f64 {
    let mut v = x.to_vec();
    v.sort_by(f64::total_cmp);
    let n = v.len();
    if n == 0 {
        f64::NAN
    } else if n % 2 == 1 {
        v[n / 2]
    } else {
        0.5 * (v[n / 2 - 1] + v[n / 2])
    }
}

/// Root of an increasing function `f` on `[lo, hi]` with `f(lo) ≤ 0 ≤ f(hi)`,
/// by the Illinois variant of regula falsi with a bisection safeguard, to a
/// relative tolerance of about `1e-13`. `hi` is doubled (up to 60 times) while
/// `f(hi) < 0`.
pub(crate) fn increasing_root(
    mut f: impl FnMut(f64) -> Result<f64>,
    mut lo: f64,
    mut hi: f64,
    routine: &'static str,
) -> Result<f64> {
    let mut f_lo = f(lo)?;
    let mut f_hi = f(hi)?;
    let mut grow = 0;
    while f_hi < 0.0 {
        lo = hi;
        f_lo = f_hi;
        hi = if hi > 0.0 { 2.0 * hi } else { 1.0 };
        f_hi = f(hi)?;
        grow += 1;
        if grow > 60 {
            return Err(StatError::ConvergenceFailure { routine, iterations: grow });
        }
    }
    if f_lo > 0.0 {
        return Err(invalid(format!("{routine}: no sign change on the starting interval")));
    }
    let mut side = 0i8;
    for _ in 0..300 {
        let width = hi - lo;
        if width <= 1e-13 * hi.abs().max(lo.abs()).max(0.1) {
            return Ok(0.5 * (lo + hi));
        }
        let mut c = (lo * f_hi - hi * f_lo) / (f_hi - f_lo);
        if !(c > lo && c < hi) {
            c = 0.5 * (lo + hi);
        }
        let fc = f(c)?;
        if fc == 0.0 {
            return Ok(c);
        }
        if fc < 0.0 {
            lo = c;
            f_lo = fc;
            if side == -1 {
                f_hi *= 0.5;
            }
            side = -1;
        } else {
            hi = c;
            f_hi = fc;
            if side == 1 {
                f_lo *= 0.5;
            }
            side = 1;
        }
    }
    Err(StatError::ConvergenceFailure { routine, iterations: 300 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alternative_helpers() {
        assert_eq!(Alternative::TwoSided.p_value(0.01, 0.99), 0.02);
        assert_eq!(Alternative::Less.p_value(0.01, 0.99), 0.01);
        assert_eq!(Alternative::Greater.p_value(0.7, 0.8), 0.8);
        assert_eq!(Alternative::TwoSided.p_value(0.7, 0.8), 1.0);
        assert!((Alternative::TwoSided.tail(0.95) - 0.025).abs() < 1e-15);
        assert_eq!(Alternative::Less.bound(1.0, 2.0), (f64::NEG_INFINITY, 2.0));
        let (r, t) = ranks_with_ties(&[3.0, 1.0, 3.0, 2.0]);
        assert_eq!(r, [3.5, 1.0, 3.5, 2.0]);
        assert_eq!(t, 6.0);
        assert_eq!(median(&[3.0, 1.0, 2.0, 10.0]), 2.5);
    }
}
