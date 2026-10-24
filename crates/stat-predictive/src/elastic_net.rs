//! Elastic net regression (lasso, ridge and mixtures) by coordinate descent,
//! with the penalty chosen by k-fold cross-validation.
//!
//! # Objective
//! With predictors standardized to mean 0 and variance 1 (divisor `n`), the
//! fit minimizes
//!
//! ```text
//! (1 / 2n) Σ (yᵢ − β₀ − zᵢᵀβ)²  +  λ [ α Σ|βⱼ| + (1 − α)/2 Σ βⱼ² ]
//! ```
//!
//! — the lasso for `α = 1` (Tibshirani 1996), ridge regression for `α = 0`
//! (Hoerl & Kennard 1970), and the elastic net in between (Zou & Hastie 2005).
//! Coefficients are reported on the original scale (`βⱼ / sdⱼ`, intercept
//! adjusted) and also on the standardized scale, where they are comparable.
//!
//! # Algorithm
//! Cyclic coordinate descent with warm starts along a decreasing path of
//! `λ` values (Friedman, Hastie & Tibshirani 2010, "Regularization paths for
//! generalized linear models via coordinate descent", *J. Stat. Software* 33):
//! each coefficient is updated in turn by
//! `βⱼ ← S(ρⱼ, λα) / (1 + λ(1 − α))`, `ρⱼ = (1/n) Σ zᵢⱼ rᵢ + βⱼ` (partial
//! residual correlation), `S` the soft-threshold, until the largest change is
//! below `10⁻¹⁰` of the response's spread. The path starts at the smallest `λ` that
//! sets every coefficient to zero, `λ_max = max|zⱼᵀ(y − ȳ)| / (n α)` (with
//! `α` floored at 0.001 for ridge), and descends log-linearly to
//! `λ_max · ε` (`ε = 10⁻⁴` if `n > p`, else `10⁻²`).
//!
//! # Choosing λ
//! The rows are split into `k` folds by a seeded shuffle; each fold's
//! prediction error is computed from a path fitted to the other folds. The
//! report gives `λ_min` (smallest mean squared error) and `λ_1SE`, the largest
//! `λ` whose error is within one standard error of the minimum — a simpler
//! model with comparable accuracy (Hastie, Tibshirani & Friedman, *The Elements
//! of Statistical Learning*, 2nd ed., §7.10). A predictor that is constant
//! within a fold's training rows carries no information there and keeps a
//! zero coefficient on that fold's path.

use stat_core::{Matrix, Result, StatError};

/// Options for [`fit_elastic_net`].
#[derive(Debug, Clone, PartialEq)]
pub struct ElasticNetOptions {
    /// Mixing parameter `α ∈ [0, 1]`: 1 = lasso, 0 = ridge.
    pub alpha: f64,
    /// Number of `λ` values on the path.
    pub n_lambda: usize,
    /// Smallest `λ` as a fraction of `λ_max` (`None`: 10⁻⁴ if `n > p`, else 10⁻²).
    pub lambda_min_ratio: Option<f64>,
    /// Cross-validation folds (0 or 1 for none).
    pub folds: usize,
    /// Seed of the fold assignment.
    pub seed: u64,
    /// Predictor names (default `x1, x2, …`).
    pub names: Vec<String>,
}

impl ElasticNetOptions {
    /// Defaults: the given `α`, 100 λ values, 10 folds, seed 1.
    #[must_use]
    pub fn new(alpha: f64) -> Self {
        Self { alpha, n_lambda: 100, lambda_min_ratio: None, folds: 10, seed: 1, names: Vec::new() }
    }
}

/// The fitted path and cross-validation results.
#[derive(Debug, Clone, PartialEq)]
pub struct ElasticNetPath {
    /// Mixing parameter used.
    pub alpha: f64,
    /// Predictor names.
    pub names: Vec<String>,
    /// `λ` values, decreasing.
    pub lambdas: Vec<f64>,
    /// Intercept at each `λ` (original scale).
    pub intercepts: Vec<f64>,
    /// Coefficients at each `λ` (original scale), `coefficients[l][j]`.
    pub coefficients: Vec<Vec<f64>>,
    /// Coefficients at each `λ` on the standardized scale.
    pub standardized: Vec<Vec<f64>>,
    /// Cross-validated mean squared error at each `λ` (empty without CV).
    pub cv_mse: Vec<f64>,
    /// Its standard error across folds.
    pub cv_se: Vec<f64>,
    /// Index of `λ_min` (smallest CV error; the last λ without CV).
    pub index_min: usize,
    /// Index of `λ_1SE`.
    pub index_1se: usize,
    /// Training mean squared error at each `λ`.
    pub train_mse: Vec<f64>,
}

impl ElasticNetPath {
    /// Number of non-zero coefficients at path index `l`.
    #[must_use]
    pub fn n_nonzero(&self, l: usize) -> usize {
        self.coefficients[l].iter().filter(|b| **b != 0.0).count()
    }

    /// Predictions at path index `l` for rows of `x` (original scale).
    #[must_use]
    pub fn predict(&self, x: &Matrix, l: usize) -> Vec<f64> {
        (0..x.nrows())
            .map(|i| self.intercepts[l] + (0..x.ncols()).map(|j| x[(i, j)] * self.coefficients[l][j]).sum::<f64>())
            .collect()
    }
}

/// Standardized copy of the data plus the transformation.
struct Standardized {
    z: Vec<Vec<f64>>, // columns
    y: Vec<f64>,      // centred response
    means: Vec<f64>,
    sds: Vec<f64>,
    y_mean: f64,
}

/// Whether [`standardize`] accepts a predictor that is constant in its rows.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ConstantPredictors {
    /// A constant predictor is an error (the fit on all rows).
    Reject,
    /// A constant predictor gets an all-zero standardized column, so its
    /// coefficient stays zero (a cross-validation training fold, where a
    /// predictor can be constant although it varies over all rows).
    Zero,
}

/// Centre each predictor and scale it to unit variance (divisor `n`) over
/// `rows`, and centre the response. A predictor whose values in `rows` are
/// all equal is rejected or given a zero column, as `constant` says.
fn standardize(x: &Matrix, y: &[f64], rows: &[usize], constant: ConstantPredictors) -> Result<Standardized> {
    let n = rows.len() as f64;
    let p = x.ncols();
    let mut z = Vec::with_capacity(p);
    let (mut means, mut sds) = (Vec::with_capacity(p), Vec::with_capacity(p));
    for j in 0..p {
        let col: Vec<f64> = rows.iter().map(|&i| x[(i, j)]).collect();
        let m = col.iter().sum::<f64>() / n;
        let sd = (col.iter().map(|v| (v - m) * (v - m)).sum::<f64>() / n).sqrt();
        // Exact equality, not `sd == 0`: the mean of repeated values can
        // round, leaving a tiny spurious sd.
        if col.iter().all(|v| *v == col[0]) || sd == 0.0 {
            if constant == ConstantPredictors::Reject {
                return Err(StatError::InvalidArgument {
                    message: format!("predictor {} is constant in the rows used", j + 1),
                });
            }
            // A zero column: coordinate descent's update for it is
            // soft_threshold(0, ·) = 0, and a unit sd maps 0 back to 0.
            z.push(vec![0.0; col.len()]);
            means.push(m);
            sds.push(1.0);
            continue;
        }
        z.push(col.iter().map(|v| (v - m) / sd).collect());
        means.push(m);
        sds.push(sd);
    }
    let yv: Vec<f64> = rows.iter().map(|&i| y[i]).collect();
    let y_mean = yv.iter().sum::<f64>() / n;
    Ok(Standardized { z, y: yv.iter().map(|v| v - y_mean).collect(), means, sds, y_mean })
}

fn soft_threshold(x: f64, t: f64) -> f64 {
    if x > t {
        x - t
    } else if x < -t {
        x + t
    } else {
        0.0
    }
}

/// Coordinate descent at one λ from the warm start `beta`.
fn descend(s: &Standardized, alpha: f64, lambda: f64, beta: &mut [f64], resid: &mut [f64]) {
    let n = s.y.len() as f64;
    let scale = (s.y.iter().map(|v| v * v).sum::<f64>() / n).sqrt().max(1e-300);
    let (l1, l2) = (lambda * alpha, lambda * (1.0 - alpha));
    for _ in 0..100_000 {
        let mut max_change = 0.0f64;
        for (j, zj) in s.z.iter().enumerate() {
            let rho = zj.iter().zip(resid.iter()).map(|(a, b)| a * b).sum::<f64>() / n + beta[j];
            let new = soft_threshold(rho, l1) / (1.0 + l2);
            let delta = new - beta[j];
            if delta != 0.0 {
                for (r, z) in resid.iter_mut().zip(zj) {
                    *r -= delta * z;
                }
                beta[j] = new;
                max_change = max_change.max(delta.abs());
            }
        }
        if max_change < 1e-10 * scale {
            break;
        }
    }
}

fn path_lambdas(s: &Standardized, alpha: f64, n_lambda: usize, ratio: f64) -> Vec<f64> {
    let n = s.y.len() as f64;
    let max_corr =
        s.z.iter().map(|zj| (zj.iter().zip(&s.y).map(|(a, b)| a * b).sum::<f64>() / n).abs()).fold(0.0, f64::max);
    let lambda_max = (max_corr / alpha.max(1e-3)).max(1e-300);
    let m = n_lambda.max(2);
    (0..m).map(|i| lambda_max * ratio.powf(i as f64 / (m - 1) as f64)).collect()
}

/// Fit the path on `rows` at the given λ values; standardized coefficients.
fn fit_path(
    x: &Matrix,
    y: &[f64],
    rows: &[usize],
    alpha: f64,
    lambdas: &[f64],
    constant: ConstantPredictors,
) -> Result<(Standardized, Vec<Vec<f64>>)> {
    let s = standardize(x, y, rows, constant)?;
    let mut beta = vec![0.0; x.ncols()];
    let mut resid = s.y.clone();
    let mut out = Vec::with_capacity(lambdas.len());
    for &lambda in lambdas {
        descend(&s, alpha, lambda, &mut beta, &mut resid);
        out.push(beta.clone());
    }
    Ok((s, out))
}

/// Fit the elastic net path of `y` on the columns of `x`, with cross-validation.
///
/// # Errors
/// [`StatError::InvalidArgument`] for `α ∉ [0, 1]`, mismatched names, a
/// predictor constant over all rows, fewer than two λ values, or more folds
/// than rows. A predictor constant only within one cross-validation training
/// fold is not an error: it carries no information there, so that fold's path
/// keeps its coefficient at zero.
/// [`StatError::DimensionMismatch`] if `y` and `x` differ in length;
/// [`StatError::NonFiniteInput`] for NaN or ±∞; [`StatError::InsufficientData`]
/// for fewer than 3 rows.
pub fn fit_elastic_net(x: &Matrix, y: &[f64], options: &ElasticNetOptions) -> Result<ElasticNetPath> {
    let (n, p) = (x.nrows(), x.ncols());
    if y.len() != n {
        return Err(StatError::DimensionMismatch {
            context: "response vs. predictor rows",
            expected: n,
            found: y.len(),
        });
    }
    if !(0.0..=1.0).contains(&options.alpha) {
        return Err(StatError::InvalidArgument { message: "α must lie between 0 (ridge) and 1 (lasso)".into() });
    }
    if p == 0 || options.n_lambda < 2 {
        return Err(StatError::InvalidArgument { message: "use at least one predictor and two λ values".into() });
    }
    if !options.names.is_empty() && options.names.len() != p {
        return Err(StatError::InvalidArgument { message: "one name per predictor is needed".into() });
    }
    if n < 3 {
        return Err(StatError::InsufficientData { required: 3, found: n });
    }
    if !x.is_finite() || y.iter().any(|v| !v.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "elastic net data" });
    }
    if options.folds > n {
        return Err(StatError::InvalidArgument { message: format!("at most {n} folds are possible") });
    }
    let all: Vec<usize> = (0..n).collect();
    let ratio = options.lambda_min_ratio.unwrap_or(if n > p { 1e-4 } else { 1e-2 });
    let s = standardize(x, y, &all, ConstantPredictors::Reject)?;
    let lambdas = path_lambdas(&s, options.alpha, options.n_lambda, ratio);
    let (s, standardized) = fit_path(x, y, &all, options.alpha, &lambdas, ConstantPredictors::Reject)?;
    let to_original = |s: &Standardized, b: &[f64]| -> (f64, Vec<f64>) {
        let coefs: Vec<f64> = b.iter().zip(&s.sds).map(|(bj, sd)| bj / sd).collect();
        let intercept = s.y_mean - coefs.iter().zip(&s.means).map(|(c, m)| c * m).sum::<f64>();
        (intercept, coefs)
    };
    let (intercepts, coefficients): (Vec<f64>, Vec<Vec<f64>>) = standardized.iter().map(|b| to_original(&s, b)).unzip();
    let mse_at = |rows: &[usize], intercept: f64, coefs: &[f64]| -> f64 {
        rows.iter()
            .map(|&i| {
                let pred = intercept + (0..p).map(|j| x[(i, j)] * coefs[j]).sum::<f64>();
                (y[i] - pred) * (y[i] - pred)
            })
            .sum::<f64>()
            / rows.len() as f64
    };
    let train_mse: Vec<f64> = intercepts.iter().zip(&coefficients).map(|(b0, b)| mse_at(&all, *b0, b)).collect();

    let (mut cv_mse, mut cv_se) = (Vec::new(), Vec::new());
    let (mut index_min, mut index_1se) = (lambdas.len() - 1, lambdas.len() - 1);
    if options.folds >= 2 {
        let mut order = all.clone();
        shuffle(&mut order, options.seed);
        let k = options.folds;
        let mut fold_errors = vec![Vec::with_capacity(k); lambdas.len()];
        for f in 0..k {
            let test: Vec<usize> = order.iter().enumerate().filter(|(i, _)| i % k == f).map(|(_, &r)| r).collect();
            let train: Vec<usize> = order.iter().enumerate().filter(|(i, _)| i % k != f).map(|(_, &r)| r).collect();
            let (st, path) = fit_path(x, y, &train, options.alpha, &lambdas, ConstantPredictors::Zero)?;
            for (l, b) in path.iter().enumerate() {
                let (b0, coefs) = to_original(&st, b);
                fold_errors[l].push(mse_at(&test, b0, &coefs));
            }
        }
        for errs in &fold_errors {
            let m = errs.iter().sum::<f64>() / k as f64;
            let var = errs.iter().map(|e| (e - m) * (e - m)).sum::<f64>() / (k - 1) as f64;
            cv_mse.push(m);
            cv_se.push((var / k as f64).sqrt());
        }
        index_min = (0..lambdas.len()).min_by(|&a, &b| cv_mse[a].total_cmp(&cv_mse[b])).unwrap_or(0);
        let limit = cv_mse[index_min] + cv_se[index_min];
        index_1se = (0..=index_min).find(|&l| cv_mse[l] <= limit).unwrap_or(index_min);
    }
    let names =
        if options.names.is_empty() { (1..=p).map(|j| format!("x{j}")).collect() } else { options.names.clone() };
    Ok(ElasticNetPath {
        alpha: options.alpha,
        names,
        lambdas,
        intercepts,
        coefficients,
        standardized,
        cv_mse,
        cv_se,
        index_min,
        index_1se,
        train_mse,
    })
}

/// Fit at a single λ (standardized coefficients, original-scale intercept and
/// coefficients).
///
/// # Errors
/// As for [`fit_elastic_net`].
pub fn fit_elastic_net_at(x: &Matrix, y: &[f64], alpha: f64, lambda: f64) -> Result<(f64, Vec<f64>, Vec<f64>)> {
    if !(0.0..=1.0).contains(&alpha) || lambda.is_nan() || lambda < 0.0 {
        return Err(StatError::InvalidArgument { message: "need α in [0, 1] and λ ≥ 0".into() });
    }
    let n = x.nrows();
    if y.len() != n {
        return Err(StatError::DimensionMismatch {
            context: "response vs. predictor rows",
            expected: n,
            found: y.len(),
        });
    }
    if x.ncols() == 0 {
        return Err(StatError::InvalidArgument { message: "use at least one predictor".into() });
    }
    if n < 3 {
        return Err(StatError::InsufficientData { required: 3, found: n });
    }
    if !x.is_finite() || y.iter().any(|v| !v.is_finite()) || lambda.is_infinite() {
        return Err(StatError::NonFiniteInput { context: "elastic net data" });
    }
    let all: Vec<usize> = (0..n).collect();
    let (s, path) = fit_path(x, y, &all, alpha, &[lambda], ConstantPredictors::Reject)?;
    let std = path.into_iter().next().unwrap_or_default();
    let coefs: Vec<f64> = std.iter().zip(&s.sds).map(|(b, sd)| b / sd).collect();
    let intercept = s.y_mean - coefs.iter().zip(&s.means).map(|(c, m)| c * m).sum::<f64>();
    Ok((intercept, coefs, std))
}

/// Seeded Fisher–Yates shuffle (SplitMix64).
fn shuffle(items: &mut [usize], seed: u64) {
    let mut state = seed;
    for i in (1..items.len()).rev() {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        let j = ((u128::from(z) * (i as u128 + 1)) >> 64) as usize;
        items.swap(i, j);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data() -> (Matrix, Vec<f64>) {
        let rows: Vec<[f64; 4]> = (0..20)
            .map(|i: i64| {
                let a = ((i * 7) % 11 - 5) as f64;
                [a, ((i * 3) % 7 - 3) as f64, ((i * i) % 13 - 6) as f64, (i % 5 - 2) as f64 + 0.5 * a]
            })
            .collect();
        let y = vec![
            5.1, 11.9, -0.8, 21.2, 5.8, 12.5, 14.8, 18.8, 5.8, 6.0, 11.8, -3.3, 16.3, 2.4, 23.2, 10.5, 2.0, 17.1, 6.2,
            9.6,
        ];
        (Matrix::from_rows(&rows).unwrap(), y)
    }

    /// Standardized coefficients match scikit-learn 1.9 `ElasticNet` (same
    /// objective) fitted to the same standardized predictors.
    #[test]
    fn matches_reference_fits() {
        let (x, y) = data();
        let cases: [(f64, f64, [f64; 4]); 3] = [
            (1.0, 0.5, [4.494_425_87, -4.130_064_11, 1.036_754_02, 0.0]),
            (0.5, 0.3, [3.708_053_87, -3.968_511_74, 1.176_130_03, 0.743_818_77]),
            (1.0, 2.0, [2.898_298_83, -2.741_296_39, 0.0, 0.0]),
        ];
        for (alpha, lambda, expected) in cases {
            let (_, _, std) = fit_elastic_net_at(&x, &y, alpha, lambda).unwrap();
            for (a, b) in std.iter().zip(expected) {
                assert!((a - b).abs() < 1e-7, "α {alpha} λ {lambda}: {a} vs {b}");
            }
        }
    }

    /// Ridge (α = 0) has the closed form β = (ZᵀZ/n + λI)⁻¹ Zᵀy/n.
    #[test]
    fn ridge_matches_closed_form() {
        let (x, y) = data();
        let lambda = 0.7;
        let (_, _, std) = fit_elastic_net_at(&x, &y, 0.0, lambda).unwrap();
        let all: Vec<usize> = (0..20).collect();
        let s = standardize(&x, &y, &all, ConstantPredictors::Reject).unwrap();
        let n = 20.0;
        let p = 4;
        // Solve (G + λI) b = c by Gaussian elimination.
        let mut a = vec![vec![0.0; p + 1]; p];
        for i in 0..p {
            for j in 0..p {
                a[i][j] =
                    s.z[i].iter().zip(&s.z[j]).map(|(u, v)| u * v).sum::<f64>() / n + if i == j { lambda } else { 0.0 };
            }
            a[i][p] = s.z[i].iter().zip(&s.y).map(|(u, v)| u * v).sum::<f64>() / n;
        }
        for c in 0..p {
            for r in c + 1..p {
                let f = a[r][c] / a[c][c];
                for k in c..=p {
                    a[r][k] -= f * a[c][k];
                }
            }
        }
        let mut b = vec![0.0; p];
        for r in (0..p).rev() {
            b[r] = (a[r][p] - (r + 1..p).map(|k| a[r][k] * b[k]).sum::<f64>()) / a[r][r];
        }
        for (u, v) in std.iter().zip(&b) {
            assert!((u - v).abs() < 1e-9, "{u} vs {v}");
        }
    }

    #[test]
    fn path_and_cross_validation() {
        let (x, y) = data();
        let path = fit_elastic_net(&x, &y, &ElasticNetOptions::new(1.0)).unwrap();
        assert_eq!(path.lambdas.len(), 100);
        assert_eq!(path.n_nonzero(0), 0, "λ_max zeroes every coefficient");
        assert!(path.n_nonzero(99) >= 3);
        assert!(path.lambdas.windows(2).all(|w| w[0] > w[1]));
        assert!(path.train_mse.windows(2).all(|w| w[1] <= w[0] + 1e-9), "training error falls along the path");
        assert_eq!(path.cv_mse.len(), 100);
        assert!(path.index_1se <= path.index_min, "the 1-SE choice is at least as regularized");
        // Near the end of the path the lasso approaches least squares (λ = 0).
        let (_, ols, _) = fit_elastic_net_at(&x, &y, 0.0, 0.0).unwrap();
        for (b, o) in path.coefficients[99].iter().zip(&ols) {
            assert!((b - o).abs() < 0.01 * o.abs().max(1.0), "{b} vs {o}");
        }
        // Reproducible.
        assert_eq!(fit_elastic_net(&x, &y, &ElasticNetOptions::new(1.0)).unwrap(), path);
        // Predictions use the original scale.
        let pred = path.predict(&x, 99);
        assert!((pred.iter().map(|p| p * p).sum::<f64>() - y.iter().map(|p| p * p).sum::<f64>()).abs() / 4000.0 < 0.1);
    }

    #[test]
    fn rejects_bad_input() {
        let (x, y) = data();
        assert!(fit_elastic_net(&x, &y, &ElasticNetOptions::new(1.5)).is_err());
        assert!(fit_elastic_net(&x, &y[..5], &ElasticNetOptions::new(1.0)).is_err());
        let mut c = x.clone();
        c.col_mut(0).iter_mut().for_each(|v| *v = 1.0);
        assert!(fit_elastic_net(&c, &y, &ElasticNetOptions::new(1.0)).is_err());
        assert!(fit_elastic_net(&x, &y, &ElasticNetOptions { folds: 21, ..ElasticNetOptions::new(1.0) }).is_err());
    }

    /// A predictor that varies only within one fold's test rows is constant in
    /// that fold's training rows. It gets a zero coefficient there, and the
    /// rest of that fold's path is the fit without it, so cross-validation
    /// completes.
    #[test]
    fn predictor_constant_in_one_training_fold() {
        let (x, y) = data();
        let options = ElasticNetOptions::new(1.0);
        let n = x.nrows();
        let mut order: Vec<usize> = (0..n).collect();
        shuffle(&mut order, options.seed);
        let k = options.folds;
        let test: Vec<usize> = (0..n).filter(|i| i % k == 0).map(|i| order[i]).collect();
        let train: Vec<usize> = (0..n).filter(|i| i % k != 0).map(|i| order[i]).collect();
        let rows: Vec<[f64; 5]> = (0..n)
            .map(|i| {
                let extra = if i == test[0] { 3.0 } else { 0.0 };
                [x[(i, 0)], x[(i, 1)], x[(i, 2)], x[(i, 3)], extra]
            })
            .collect();
        let x5 = Matrix::from_rows(&rows).unwrap();
        let path = fit_elastic_net(&x5, &y, &options).unwrap();
        assert_eq!(path.cv_mse.len(), path.lambdas.len());
        assert!(path.cv_mse.iter().all(|m| m.is_finite()));
        // In the fold where it is constant, the extra predictor stays at zero
        // and the others match the fit without it.
        let (_, with) = fit_path(&x5, &y, &train, options.alpha, &path.lambdas, ConstantPredictors::Zero).unwrap();
        let (_, without) = fit_path(&x, &y, &train, options.alpha, &path.lambdas, ConstantPredictors::Zero).unwrap();
        for (a, b) in with.iter().zip(&without) {
            assert_eq!(a[4], 0.0);
            assert_eq!(&a[..4], &b[..]);
        }
        // On all rows it is still a constant-free fit, and still an error when
        // constant there.
        let mut c = x5.clone();
        c.col_mut(4).iter_mut().for_each(|v| *v = 0.0);
        assert!(fit_elastic_net(&c, &y, &options).is_err());
    }

    /// The single-λ fit validates its input as the path fit does, instead of
    /// panicking on a short response or returning NaN coefficients.
    #[test]
    fn single_lambda_fit_rejects_bad_input() {
        let (x, y) = data();
        assert!(matches!(fit_elastic_net_at(&x, &y[..5], 1.0, 0.5), Err(StatError::DimensionMismatch { .. })));
        let mut bad = y.clone();
        bad[3] = f64::NAN;
        assert!(matches!(fit_elastic_net_at(&x, &bad, 1.0, 0.5), Err(StatError::NonFiniteInput { .. })));
        let x0 = Matrix::zeros(0, 4);
        assert!(fit_elastic_net_at(&x0, &[], 1.0, 0.5).is_err());
    }
}
