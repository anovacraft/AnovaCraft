//! Regression spline bases: a continuous predictor expanded into the columns
//! of a B-spline or natural cubic spline basis, so a model can fit a smooth
//! curve in it with ordinary least squares or a GLM.
//!
//! # B-splines
//! For degree `d` (order `d + 1`), boundary knots `a < b` and interior knots
//! `a < t₁ < … < t_m < b`, the knot sequence repeats each boundary knot
//! `d + 1` times. The `m + d + 1` B-spline functions follow the Cox–de Boor
//! recursion (de Boor, *A Practical Guide to Splines*, 1978, ch. IX):
//!
//! `B_{j,0}(x) = 1` on `[u_j, u_{j+1})`, and
//! `B_{j,q}(x) = (x − u_j)/(u_{j+q} − u_j) B_{j,q−1}(x)
//!            + (u_{j+q+1} − x)/(u_{j+q+1} − u_{j+1}) B_{j+1,q−1}(x)`.
//!
//! The first function is dropped (the model's intercept takes its place), so
//! the basis has `m + d` columns. Outside `[a, b]` each function continues
//! the polynomial of the nearest interval. With these conventions the columns
//! are exactly those of R's `splines::bs(x, knots, degree, Boundary.knots)`.
//!
//! # Natural cubic splines
//! A natural cubic spline is cubic between knots and linear beyond the
//! boundary knots (Hastie, Tibshirani & Friedman, *Elements of Statistical
//! Learning*, 2nd ed., §5.2.1; Harrell, *Regression Modeling Strategies*,
//! 2nd ed., §2.4.5). Its basis is built from the cubic B-splines (without
//! the first) by removing the two directions in which the second derivative
//! at `a` or `b` is not zero: with `C` the `2 × (m + 3)` matrix of second
//! derivatives at the boundary knots and `Q` the orthogonal factor of the
//! Householder QR of `Cᵀ`, the basis is `B Q` without its first two columns,
//! `m + 1` columns. Beyond the boundary knots each column continues linearly
//! (value and slope at the knot). The Householder convention is LINPACK's,
//! so the columns equal those of R's `splines::ns`.
//!
//! # Default knots
//! [`SplineSpec::Count`] places `m` interior knots at the quantiles
//! `k/(m + 1)`, `k = 1…m`, of the values (linear interpolation, Hyndman–Fan
//! type 7), as R's `bs(x, df)` and `ns(x, df)` do; the boundary knots are
//! the smallest and largest value.

use stat_core::{Result, StatError};

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// Most interior knots accepted.
pub const MAX_KNOTS: usize = 20;
/// Highest B-spline degree accepted.
pub const MAX_DEGREE: u8 = 5;

/// The kind of spline basis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SplineKind {
    /// B-spline basis of the given degree (1 to [`MAX_DEGREE`]; 3 is cubic).
    BSpline {
        /// Polynomial degree of each piece.
        degree: u8,
    },
    /// Natural cubic spline (linear beyond the boundary knots).
    Natural,
}

impl SplineKind {
    /// Degree of the pieces (3 for a natural spline).
    #[must_use]
    pub fn degree(self) -> u8 {
        match self {
            Self::BSpline { degree } => degree,
            Self::Natural => 3,
        }
    }

    /// Short description, e.g. `"natural cubic spline"` or `"B-spline, degree 2"`.
    #[must_use]
    pub fn label(self) -> String {
        match self {
            Self::Natural => "natural cubic spline".to_owned(),
            Self::BSpline { degree: 3 } => "cubic B-spline".to_owned(),
            Self::BSpline { degree } => format!("B-spline, degree {degree}"),
        }
    }
}

/// Where the interior knots go.
#[derive(Debug, Clone, PartialEq)]
pub enum SplineKnots {
    /// This many interior knots at equally spaced quantiles of the values.
    Count(usize),
    /// Interior knots at these values.
    At(Vec<f64>),
}

/// A spline term as requested: its kind and knots.
#[derive(Debug, Clone, PartialEq)]
pub struct SplineSpec {
    /// B-spline or natural spline.
    pub kind: SplineKind,
    /// Interior knots.
    pub knots: SplineKnots,
}

// Knot positions are compared bit for bit, so a spec is a usable key.
impl Eq for SplineSpec {}

impl SplineSpec {
    /// A natural cubic spline with `m` knots at quantiles.
    #[must_use]
    pub fn natural(m: usize) -> Self {
        Self { kind: SplineKind::Natural, knots: SplineKnots::Count(m) }
    }

    /// A B-spline of `degree` with `m` knots at quantiles.
    #[must_use]
    pub fn bspline(degree: u8, m: usize) -> Self {
        Self { kind: SplineKind::BSpline { degree }, knots: SplineKnots::Count(m) }
    }

    /// Build the basis for `values` (non-finite values are ignored).
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] for fewer than two distinct values, a
    /// degree outside 1 to [`MAX_DEGREE`], more than [`MAX_KNOTS`] knots, or a
    /// knot outside the open range of the values. Quantile knots that
    /// coincide (heavily tied values) are merged.
    pub fn basis(&self, values: &[f64]) -> Result<SplineBasis> {
        let mut x: Vec<f64> = values.iter().copied().filter(|v| v.is_finite()).collect();
        x.sort_by(f64::total_cmp);
        let (Some(&a), Some(&b)) = (x.first(), x.last()) else {
            return Err(invalid("a spline needs values"));
        };
        if a >= b {
            return Err(invalid("a spline needs at least two distinct values"));
        }
        let interior = match &self.knots {
            SplineKnots::Count(m) => {
                if *m > MAX_KNOTS {
                    return Err(invalid(format!("a spline may have at most {MAX_KNOTS} interior knots")));
                }
                (1..=*m).map(|k| type7(&x, k as f64 / (*m + 1) as f64)).collect()
            }
            SplineKnots::At(k) => k.clone(),
        };
        SplineBasis::new(self.kind, (a, b), interior)
    }
}

/// Hyndman–Fan type 7 quantile of sorted values.
fn type7(sorted: &[f64], p: f64) -> f64 {
    let h = (sorted.len() - 1) as f64 * p;
    let lo = h.floor() as usize;
    let hi = (lo + 1).min(sorted.len() - 1);
    sorted[lo] + (h - lo as f64) * (sorted[hi] - sorted[lo])
}

/// A spline basis with fixed knots: evaluates the columns at any `x`.
#[derive(Debug, Clone, PartialEq)]
pub struct SplineBasis {
    kind: SplineKind,
    boundary: (f64, f64),
    interior: Vec<f64>,
    /// Full knot sequence (boundary knots repeated degree + 1 times).
    knots: Vec<f64>,
    /// Natural splines: the orthogonal factor's columns 3… (rows: B-spline
    /// columns without the first), as LINPACK Householder data.
    natural: Option<NaturalProjection>,
}

/// Householder data of the QR of the boundary second-derivative matrix.
#[derive(Debug, Clone, PartialEq)]
struct NaturalProjection {
    /// Householder vectors (column-major, `n × 2`) as LINPACK stores them.
    qr: Vec<f64>,
    qraux: [f64; 2],
    n: usize,
}

impl SplineBasis {
    /// A basis with boundary knots `boundary` and the given interior knots.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] for boundary knots that are not finite
    /// and increasing, a degree outside 1 to [`MAX_DEGREE`], more than
    /// [`MAX_KNOTS`] interior knots, or an interior knot not strictly between
    /// the boundary knots. Repeated interior knots are merged.
    pub fn new(kind: SplineKind, boundary: (f64, f64), mut interior: Vec<f64>) -> Result<Self> {
        let (a, b) = boundary;
        if !(a.is_finite() && b.is_finite() && a < b) {
            return Err(invalid("a spline needs increasing, finite boundary knots"));
        }
        let degree = kind.degree();
        if degree == 0 || degree > MAX_DEGREE {
            return Err(invalid(format!("a spline's degree must be 1 to {MAX_DEGREE}")));
        }
        interior.sort_by(f64::total_cmp);
        interior.dedup();
        if interior.len() > MAX_KNOTS {
            return Err(invalid(format!("a spline may have at most {MAX_KNOTS} interior knots")));
        }
        if let Some(bad) = interior.iter().find(|&&k| !(k > a && k < b)) {
            return Err(invalid(format!("the knot {bad} is not inside the range of the values ({a} to {b})")));
        }
        let order = usize::from(degree) + 1;
        let mut knots = vec![a; order];
        knots.extend(&interior);
        knots.extend(std::iter::repeat_n(b, order));
        let mut basis = Self { kind, boundary, interior, knots, natural: None };
        if kind == SplineKind::Natural {
            // Second derivatives at both boundary knots, without the first function.
            let ca = basis.bspline_derivative(a, 2);
            let cb = basis.bspline_derivative(b, 2);
            let n = ca.len() - 1;
            let mut qr: Vec<f64> = ca[1..].iter().chain(&cb[1..]).copied().collect();
            let qraux = linpack_qr(&mut qr, n);
            basis.natural = Some(NaturalProjection { qr, qraux, n });
        }
        Ok(basis)
    }

    /// The kind of spline.
    #[must_use]
    pub fn kind(&self) -> SplineKind {
        self.kind
    }

    /// Boundary knots.
    #[must_use]
    pub fn boundary(&self) -> (f64, f64) {
        self.boundary
    }

    /// Interior knots.
    #[must_use]
    pub fn interior(&self) -> &[f64] {
        &self.interior
    }

    /// Number of basis columns: `m + d` for a B-spline, `m + 1` for a natural
    /// spline.
    #[must_use]
    pub fn n_columns(&self) -> usize {
        match self.kind {
            SplineKind::BSpline { degree } => self.interior.len() + usize::from(degree),
            SplineKind::Natural => self.interior.len() + 1,
        }
    }

    /// The basis columns at `x` (NaN for a NaN `x`).
    #[must_use]
    pub fn evaluate(&self, x: f64) -> Vec<f64> {
        if !x.is_finite() {
            return vec![f64::NAN; self.n_columns()];
        }
        match &self.natural {
            None => self.bspline_derivative(x, 0)[1..].to_vec(),
            Some(p) => {
                let (a, b) = self.boundary;
                // Linear beyond the boundary knots: value plus slope times distance.
                let full = if x < a || x > b {
                    let k = if x < a { a } else { b };
                    let v = self.bspline_derivative(k, 0);
                    let s = self.bspline_derivative(k, 1);
                    v.iter().zip(&s).map(|(v, s)| v + s * (x - k)).collect()
                } else {
                    self.bspline_derivative(x, 0)
                };
                let mut y = full[1..].to_vec();
                linpack_qty(&p.qr, p.qraux, p.n, &mut y);
                y[2..].to_vec()
            }
        }
    }

    /// The `deriv`-th derivative of every B-spline function (all `m + d + 1`)
    /// at `x`, continuing the polynomial of the nearest interval outside
    /// `[a, b]`.
    fn bspline_derivative(&self, x: f64, deriv: usize) -> Vec<f64> {
        let u = &self.knots;
        let order = usize::from(self.kind.degree()) + 1;
        let n_basis = u.len() - order;
        // The interval whose polynomial applies: the last non-empty one with
        // u[s] <= x, within [order − 1, n_basis − 1].
        let mut span = order - 1;
        for s in order - 1..n_basis {
            if u[s] <= x && u[s] < u[s + 1] {
                span = s;
            }
        }
        let mut n = vec![0.0; u.len() - 1];
        n[span] = 1.0;
        let degree = order - 1;
        recurse(u, x, &mut n, degree, deriv);
        n.truncate(n_basis);
        n
    }
}

/// Degree-0 indicators `n` raised to `degree` by the Cox–de Boor recursion,
/// differentiated `deriv` times (the last `deriv` steps use the derivative
/// form `q (B_{j,q−1}/(u_{j+q} − u_j) − B_{j+1,q−1}/(u_{j+q+1} − u_{j+1}))`).
fn recurse(u: &[f64], x: f64, n: &mut Vec<f64>, degree: usize, deriv: usize) {
    let ratio = |num: f64, den: f64| if den == 0.0 { 0.0 } else { num / den };
    for q in 1..=degree {
        let len = u.len() - 1 - q;
        let derivative = q + deriv > degree;
        let next: Vec<f64> = (0..len)
            .map(|j| {
                if derivative {
                    q as f64 * (ratio(n[j], u[j + q] - u[j]) - ratio(n[j + 1], u[j + q + 1] - u[j + 1]))
                } else {
                    ratio(x - u[j], u[j + q] - u[j]) * n[j]
                        + ratio(u[j + q + 1] - x, u[j + q + 1] - u[j + 1]) * n[j + 1]
                }
            })
            .collect();
        *n = next;
    }
}

/// LINPACK `dqrdc` (no pivoting) of the column-major `n × 2` matrix in place;
/// returns `qraux`.
fn linpack_qr(x: &mut [f64], n: usize) -> [f64; 2] {
    let mut qraux = [0.0; 2];
    for l in 0..2.min(n) {
        let col = l * n;
        let mut norm = x[col + l..col + n].iter().map(|v| v * v).sum::<f64>().sqrt();
        if norm == 0.0 {
            continue;
        }
        if x[col + l] != 0.0 {
            norm = norm.copysign(x[col + l]);
        }
        for v in &mut x[col + l..col + n] {
            *v /= norm;
        }
        x[col + l] += 1.0;
        for j in l + 1..2 {
            let other = j * n;
            let t = -(l..n).map(|i| x[col + i] * x[other + i]).sum::<f64>() / x[col + l];
            for i in l..n {
                x[other + i] += t * x[col + i];
            }
        }
        qraux[l] = x[col + l];
        x[col + l] = -norm;
    }
    qraux
}

/// LINPACK `dqrsl`: `y ← Qᵀ y` for the factorization [`linpack_qr`] made.
fn linpack_qty(qr: &[f64], qraux: [f64; 2], n: usize, y: &mut [f64]) {
    for (j, &aux) in qraux.iter().enumerate().take(n.saturating_sub(1).min(2)) {
        if aux == 0.0 {
            continue;
        }
        let col = j * n;
        let diag = |i: usize| if i == j { aux } else { qr[col + i] };
        let t = -(j..n).map(|i| diag(i) * y[i]).sum::<f64>() / aux;
        for i in j..n {
            y[i] += t * diag(i);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) {
        assert!((a - b).abs() <= tol * (1.0 + b.abs()), "{a} vs {b}");
    }

    /// The B-spline functions (with the first) sum to one inside the boundary.
    #[test]
    fn partition_of_unity() {
        for kind in [SplineKind::BSpline { degree: 1 }, SplineKind::BSpline { degree: 3 }] {
            let b = SplineBasis::new(kind, (0.0, 10.0), vec![2.0, 5.5, 7.0]).unwrap();
            for x in [0.0, 1.3, 2.0, 5.0, 9.99, 10.0] {
                let s: f64 = b.bspline_derivative(x, 0).iter().sum();
                close(s, 1.0, 1e-13);
            }
        }
    }

    /// A natural spline is linear beyond its boundary knots and has zero
    /// second derivative at them.
    #[test]
    fn natural_is_linear_outside() {
        let b = SplineBasis::new(SplineKind::Natural, (0.0, 1.0), vec![0.3, 0.6]).unwrap();
        assert_eq!(b.n_columns(), 3);
        let (f0, f1, f2) = (b.evaluate(1.0), b.evaluate(1.5), b.evaluate(2.0));
        for j in 0..3 {
            close(f2[j] - f1[j], f1[j] - f0[j], 1e-12);
        }
        let h = 1e-4;
        let (l, m, r) = (b.evaluate(1.0 - 2.0 * h), b.evaluate(1.0 - h), b.evaluate(1.0));
        for j in 0..3 {
            assert!(((r[j] - 2.0 * m[j] + l[j]) / (h * h)).abs() < 1e-2);
        }
    }

    #[test]
    fn rejects_bad_knots() {
        assert!(SplineBasis::new(SplineKind::Natural, (0.0, 1.0), vec![1.0]).is_err());
        assert!(SplineBasis::new(SplineKind::BSpline { degree: 0 }, (0.0, 1.0), vec![]).is_err());
        assert!(SplineSpec::natural(2).basis(&[1.0, 1.0]).is_err());
    }
}
