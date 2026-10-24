//! Multiple imputation: `m` completed copies of a data matrix, each with the
//! missing values drawn from their predictive distribution, so that analyses
//! repeated on every copy and combined by Rubin's rules
//! (`stat_inference::pooling`) carry the uncertainty the missing values add.
//!
//! Data are row vectors (`data[i][j]`) with `NaN` marking a missing value.
//! A categorical column holds level codes `0, 1, …, L − 1` (see
//! [`ColumnKind`]). Observed values are never changed.
//!
//! # Chained equations
//! [`chained_equations`] is fully conditional specification (van Buuren &
//! Groothuis-Oudshoorn 2011, "mice: Multivariate imputation by chained
//! equations in R", *J. Stat. Softw.* 45(3); van Buuren 2018, *Flexible
//! Imputation of Missing Data*, 2nd ed., §4.5). Each column with missing
//! values gets its own imputation model given the other columns. Missing
//! cells start as random draws from their column's observed values; each
//! iteration then visits the columns in the visit order and redraws the
//! column's missing cells from its model fitted to the rows where it is
//! observed, using the current values of its predictors (observed or
//! imputed). After the last iteration the data are one imputation; `m`
//! independent chains (seeded `seed`, `seed + 1`, …, run in parallel) give
//! `m` imputations. Predictors enter the models with an intercept,
//! continuous columns standardized over the fitting rows and categorical
//! columns as indicator variables against their first level; predictors
//! that are constant on the fitting rows are left out.
//!
//! The models (van Buuren 2018, §3.2–3.6):
//!
//! * **Bayesian linear regression** ([`bayesian_regression_draw`],
//!   Algorithm 3.1 of van Buuren 2018; Rubin 1987, p. 167): with
//!   `V = (XᵀX + κ diag(XᵀX))⁻¹`, `β̂ = V Xᵀy` and residual sum of squares
//!   `S`, draw `σ̇² = S/g` with `g ~ χ²(n − q)`, then `β̇ = β̂ + σ̇ L z`
//!   (`V = L Lᵀ`, `z` standard normal), and impute `xβ̇ + σ̇ ε`. As in
//!   mice, `κ = 0` (plain least squares) unless `XᵀX` is numerically
//!   singular, when a small ridge is added.
//! * **Predictive mean matching** (the default for continuous columns;
//!   Little 1988, "Missing-data adjustments in large surveys", *J. Bus.
//!   Econ. Stat.* 6; van Buuren 2018, Algorithm 3.3, "type 1" matching):
//!   draw `β̇` as above; for each missing cell find the `k` observed rows
//!   (`k = 5` donors by default) whose predicted means `xᵢβ̂` are closest to
//!   the cell's `xβ̇` ([`pmm_donors`]) and copy the observed value of one
//!   of them chosen at random. Imputed values are always values that occur
//!   in the data.
//! * **Logistic regression** (two-level columns; van Buuren 2018,
//!   Algorithm 3.4): fit the logistic regression by Newton's method, draw
//!   `β̇ ~ N(β̂, V)` from its asymptotic covariance, and draw each missing
//!   level with probability `logit⁻¹(xβ̇)`.
//! * **Multinomial logit** (three or more levels): the same with a
//!   baseline-category logit model, drawing `β̇` from its asymptotic normal
//!   posterior and each level with its fitted probability.
//!
//! The logistic and multinomial fits add a small ridge penalty (0.001 on
//! the standardized slopes), which keeps them finite when a predictor
//! separates the levels perfectly, the problem White, Daniel & Royston
//! (2010, *Comput. Stat. Data Anal.* 54) solve by augmenting the data.
//!
//! # Joint multivariate normal model
//! [`normal_data_augmentation`] draws from the posterior predictive
//! distribution of a multivariate normal model by data augmentation
//! (Tanner & Wong 1987, *JASA* 82; Schafer 1997, *Analysis of Incomplete
//! Multivariate Data*, §5.4), each chain starting from the maximum
//! likelihood estimates of the EM algorithm ([`crate::imputation::em_normal`]).
//! Each iteration alternates
//!
//! * the I-step: draw every row's missing part from
//!   `N(μ_M + Σ_MO Σ_OO⁻¹ (x_O − μ_O), Σ_MM − Σ_MO Σ_OO⁻¹ Σ_OM)`, and
//! * the P-step: with the completed data's mean `x̄` and cross-product
//!   matrix `A = Σ (xᵢ − x̄)(xᵢ − x̄)ᵀ`, draw `Σ ~ W⁻¹(n − 1, A)` and then
//!   `μ ~ N(x̄, Σ/n)`, the posterior under the noninformative prior
//!   `π(μ, Σ) ∝ |Σ|^{−(p+1)/2}` (Schafer 1997, §5.4.2). The inverse
//!   Wishart draw uses Bartlett's decomposition (Smith & Hocking 1972,
//!   *Appl. Stat.* 21).
//!
//! The imputation is the I-step after the last iteration.
//!
//! # Convergence
//! Both engines record, for every chain, iteration and column with missing
//! values, the mean and standard deviation of the column's imputed values
//! ([`Chains`]); chains that mix well wander over the same range with no
//! trend (van Buuren 2018, §6.5).

use rayon::prelude::*;
use stat_core::rng::Rng;
use stat_core::{Result, StatError};

use crate::imputation::{EmOptions, em_normal, missing_patterns};
use crate::spd::{cholesky, forward, inverse};

/// One chain's result: its completed data and the per-iteration means and
/// standard deviations of the imputed values.
type ChainRun = (Vec<Vec<f64>>, Vec<Vec<f64>>, Vec<Vec<f64>>);

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// What a column holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnKind {
    /// Numbers on a continuous scale.
    Continuous,
    /// Categories coded `0, 1, …, levels − 1`.
    Categorical {
        /// Number of levels (at least two).
        levels: usize,
    },
}

/// How [`chained_equations`] imputes one column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FcsMethod {
    /// Not imputed (no missing values, or left missing on purpose). Such a
    /// column predicts other columns only when it has no missing values.
    None,
    /// Predictive mean matching (continuous columns).
    PredictiveMeanMatching,
    /// Bayesian linear regression (continuous columns).
    BayesianRegression,
    /// Logistic regression (two-level categorical columns).
    Logistic,
    /// Multinomial logit (categorical columns with any number of levels).
    Multinomial,
}

impl FcsMethod {
    /// Whether the method suits a column of `kind`.
    #[must_use]
    pub fn suits(self, kind: ColumnKind) -> bool {
        match (self, kind) {
            (Self::None, _) => true,
            (Self::PredictiveMeanMatching | Self::BayesianRegression, ColumnKind::Continuous) => true,
            (Self::Logistic, ColumnKind::Categorical { levels }) => levels == 2,
            (Self::Multinomial, ColumnKind::Categorical { .. }) => true,
            _ => false,
        }
    }
}

/// The default method for each column: none without missing values,
/// predictive mean matching for continuous columns, logistic regression for
/// two levels and the multinomial logit for more (van Buuren 2018, §4.5.2).
#[must_use]
pub fn default_methods(data: &[Vec<f64>], kinds: &[ColumnKind]) -> Vec<FcsMethod> {
    kinds
        .iter()
        .enumerate()
        .map(|(j, &kind)| {
            if !data.iter().any(|r| r[j].is_nan()) {
                FcsMethod::None
            } else {
                match kind {
                    ColumnKind::Continuous => FcsMethod::PredictiveMeanMatching,
                    ColumnKind::Categorical { levels: 2 } => FcsMethod::Logistic,
                    ColumnKind::Categorical { .. } => FcsMethod::Multinomial,
                }
            }
        })
        .collect()
}

/// Settings of [`chained_equations`].
#[derive(Debug, Clone, PartialEq)]
pub struct FcsOptions {
    /// Number of imputations `m` (at least 1).
    pub m: usize,
    /// Iterations per chain (at least 1).
    pub iterations: usize,
    /// Donors for predictive mean matching (at least 1).
    pub donors: usize,
    /// Seed of the first chain; chain `l` uses `seed + l`.
    pub seed: u64,
    /// Method per column.
    pub methods: Vec<FcsMethod>,
    /// `predictors[j][k]`: column `k` predicts column `j` (the diagonal is
    /// ignored).
    pub predictors: Vec<Vec<bool>>,
    /// Order in which an iteration visits the columns (columns not listed
    /// are not imputed).
    pub visit_order: Vec<usize>,
}

impl FcsOptions {
    /// Defaults for `data`: 20 imputations, 10 iterations, 5 donors, seed 1,
    /// [`default_methods`], every other column a predictor, columns visited
    /// left to right.
    #[must_use]
    pub fn new(data: &[Vec<f64>], kinds: &[ColumnKind]) -> Self {
        let p = kinds.len();
        Self {
            m: 20,
            iterations: 10,
            donors: 5,
            seed: 1,
            methods: default_methods(data, kinds),
            predictors: (0..p).map(|j| (0..p).map(|k| k != j).collect()).collect(),
            visit_order: (0..p).collect(),
        }
    }
}

/// Means and standard deviations of the imputed values of each column, per
/// chain and iteration (`NaN` for columns with no imputed values).
#[derive(Debug, Clone, PartialEq)]
pub struct Chains {
    /// `means[l][t][j]`: chain `l`, iteration `t` (0 is the starting fill),
    /// column `j`.
    pub means: Vec<Vec<Vec<f64>>>,
    /// Standard deviations, indexed like `means` (`NaN` with one imputed value).
    pub sds: Vec<Vec<Vec<f64>>>,
}

/// The result of a multiple imputation.
#[derive(Debug, Clone, PartialEq)]
pub struct MultipleImputation {
    /// `imputations[l]` is completed data set `l` (rows and columns as the input).
    pub imputations: Vec<Vec<Vec<f64>>>,
    /// Convergence traces.
    pub chains: Chains,
    /// Columns that were imputed.
    pub imputed: Vec<bool>,
}

fn check(data: &[Vec<f64>], kinds: &[ColumnKind]) -> Result<usize> {
    let p = kinds.len();
    if p == 0 {
        return Err(invalid("choose at least one column"));
    }
    if let Some(bad) = data.iter().find(|r| r.len() != p) {
        return Err(StatError::DimensionMismatch { context: "imputation rows", expected: p, found: bad.len() });
    }
    if data.iter().flatten().any(|x| x.is_infinite()) {
        return Err(StatError::NonFiniteInput { context: "imputation data" });
    }
    for (j, kind) in kinds.iter().enumerate() {
        if let ColumnKind::Categorical { levels } = *kind {
            if levels < 2 {
                return Err(invalid(format!("categorical column {} needs at least two levels", j + 1)));
            }
            if data.iter().map(|r| r[j]).any(|x| !x.is_nan() && (x < 0.0 || x.fract() != 0.0 || x >= levels as f64)) {
                return Err(invalid(format!("categorical column {} must hold level codes 0 to {}", j + 1, levels - 1)));
            }
        }
    }
    Ok(p)
}

/// The pieces of a Bayesian linear regression draw.
#[derive(Debug, Clone, PartialEq)]
pub struct RegressionDraw {
    /// Least-squares (ridge-guarded) coefficients `β̂`.
    pub coefficients: Vec<f64>,
    /// Drawn coefficients `β̇`.
    pub drawn: Vec<f64>,
    /// Drawn residual standard deviation `σ̇`.
    pub sigma: f64,
}

/// Lower Cholesky factor, retrying with a growing ridge when the matrix is
/// numerically singular.
fn guarded_cholesky(a: &[Vec<f64>]) -> Result<Vec<Vec<f64>>> {
    if let Some(l) = cholesky(a) {
        return Ok(l);
    }
    let scale = a.iter().enumerate().map(|(i, r)| r[i].abs()).fold(0.0_f64, f64::max).max(1e-300);
    for e in [1e-10, 1e-8, 1e-6, 1e-4] {
        let mut b = a.to_vec();
        for (i, r) in b.iter_mut().enumerate() {
            r[i] += e * scale;
        }
        if let Some(l) = cholesky(&b) {
            return Ok(l);
        }
    }
    Err(invalid("an imputation model could not be fitted: its predictors are collinear"))
}

/// `XᵀX` and `Xᵀy` for row vectors `x`.
fn cross_products(x: &[Vec<f64>], y: &[f64]) -> (Vec<Vec<f64>>, Vec<f64>) {
    let q = x.first().map_or(0, Vec::len);
    let mut xtx = vec![vec![0.0; q]; q];
    let mut xty = vec![0.0; q];
    for (row, &yi) in x.iter().zip(y) {
        for a in 0..q {
            xty[a] += row[a] * yi;
            for b in a..q {
                xtx[a][b] += row[a] * row[b];
            }
        }
    }
    for a in 0..q {
        for b in 0..a {
            xtx[a][b] = xtx[b][a];
        }
    }
    (xtx, xty)
}

fn mat_vec(a: &[Vec<f64>], v: &[f64]) -> Vec<f64> {
    a.iter().map(|r| r.iter().zip(v).map(|(x, y)| x * y).sum()).collect()
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// The Bayesian linear regression draw of van Buuren (2018, Algorithm 3.1)
/// given its random numbers: `chi_square` is the `χ²(n − q)` draw `g` and
/// `z` the `q` standard normal draws. `x` holds the predictor rows
/// (intercept column included) of the `n` fitting rows and `y` their
/// response; `ridge` is `κ`.
///
/// # Errors
/// `x` and `y` of different lengths, rows of different lengths, fewer rows
/// than predictors, or collinear predictors.
pub fn bayesian_regression_draw(
    x: &[Vec<f64>],
    y: &[f64],
    ridge: f64,
    chi_square: f64,
    z: &[f64],
) -> Result<RegressionDraw> {
    let q = x.first().map_or(0, Vec::len);
    if x.len() != y.len() || x.iter().any(|r| r.len() != q) || z.len() != q {
        return Err(StatError::DimensionMismatch { context: "regression draw", expected: q, found: z.len() });
    }
    if q == 0 || x.len() <= q {
        return Err(StatError::InsufficientData { required: q + 1, found: x.len() });
    }
    let (mut xtx, xty) = cross_products(x, y);
    for (i, r) in xtx.iter_mut().enumerate() {
        r[i] *= 1.0 + ridge;
    }
    let l = guarded_cholesky(&xtx)?;
    let v = inverse(&l);
    let coefficients = mat_vec(&v, &xty);
    let rss: f64 = x.iter().zip(y).map(|(r, &yi)| (yi - dot(r, &coefficients)).powi(2)).sum();
    let sigma = (rss / chi_square).sqrt();
    let lv = guarded_cholesky(&v)?;
    let drawn = (0..q).map(|a| coefficients[a] + sigma * (0..=a).map(|b| lv[a][b] * z[b]).sum::<f64>()).collect();
    Ok(RegressionDraw { coefficients, drawn, sigma })
}

/// The `k` observed rows whose predicted means `fitted_observed[i]` are
/// nearest `target` (ties to the earlier row), nearest first.
#[must_use]
pub fn pmm_donors(fitted_observed: &[f64], target: f64, k: usize) -> Vec<usize> {
    let dist = |i: usize| (fitted_observed[i] - target).abs();
    let mut order: Vec<usize> = (0..fitted_observed.len()).collect();
    order.sort_by(|&a, &b| dist(a).total_cmp(&dist(b)).then(a.cmp(&b)));
    order.truncate(k.max(1));
    order
}

/// Observed predicted means sorted once, so each missing cell finds its
/// donors by bisection instead of a full sort.
struct DonorPool {
    /// `(predicted mean, row)` in ascending order of the mean.
    sorted: Vec<(f64, usize)>,
}

impl DonorPool {
    fn new(fitted: &[f64]) -> Self {
        let mut sorted: Vec<(f64, usize)> = fitted.iter().copied().zip(0..).collect();
        sorted.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        Self { sorted }
    }

    /// The same donors as [`pmm_donors`]: widen a window around `target`,
    /// taking the nearer side each time (on equal distance, the earlier row).
    fn donors(&self, target: f64, k: usize) -> Vec<usize> {
        let s = &self.sorted;
        let k = k.max(1).min(s.len());
        let mut right = s.partition_point(|&(v, _)| v < target);
        let mut left = right;
        let mut out = Vec::with_capacity(k);
        while out.len() < k {
            let take_left = match (left > 0, right < s.len()) {
                (true, true) => {
                    let (dl, dr) = (target - s[left - 1].0, s[right].0 - target);
                    dl < dr || (dl == dr && s[left - 1].1 < s[right].1)
                }
                (true, false) => true,
                (false, true) => false,
                (false, false) => break,
            };
            if take_left {
                left -= 1;
                out.push(s[left].1);
            } else {
                out.push(s[right].1);
                right += 1;
            }
        }
        out
    }
}

/// A predictor block in an imputation model.
#[derive(Clone, Copy)]
enum Block {
    /// Continuous column, standardized by (mean, sd).
    Continuous(usize, f64, f64),
    /// Indicator of level `level` of a categorical column.
    Indicator(usize, usize),
}

/// The predictors of column `j`'s model: an intercept plus the blocks that
/// vary over the fitting rows.
fn design_blocks(
    current: &[Vec<f64>],
    kinds: &[ColumnKind],
    predictors: &[bool],
    j: usize,
    fit_rows: &[usize],
) -> Vec<Block> {
    let mut blocks = Vec::new();
    for (k, &use_it) in predictors.iter().enumerate() {
        if !use_it || k == j {
            continue;
        }
        match kinds[k] {
            ColumnKind::Continuous => {
                let n = fit_rows.len() as f64;
                let mean = fit_rows.iter().map(|&r| current[r][k]).sum::<f64>() / n;
                let var = fit_rows.iter().map(|&r| (current[r][k] - mean).powi(2)).sum::<f64>() / n;
                if var > 1e-24 * mean.abs().max(1.0).powi(2) {
                    blocks.push(Block::Continuous(k, mean, var.sqrt()));
                }
            }
            ColumnKind::Categorical { levels } => {
                for level in 1..levels {
                    let count = fit_rows.iter().filter(|&&r| current[r][k] == level as f64).count();
                    if count > 0 && count < fit_rows.len() {
                        blocks.push(Block::Indicator(k, level));
                    }
                }
            }
        }
    }
    blocks
}

fn design_row(row: &[f64], blocks: &[Block]) -> Vec<f64> {
    let mut out = Vec::with_capacity(blocks.len() + 1);
    out.push(1.0);
    for b in blocks {
        out.push(match *b {
            Block::Continuous(k, mean, sd) => (row[k] - mean) / sd,
            Block::Indicator(k, level) => f64::from(u8::from(row[k] == level as f64)),
        });
    }
    out
}

/// Penalty on the standardized slopes of the logistic and multinomial fits.
const LOGIT_RIDGE: f64 = 1e-3;
/// Ridge `κ` of the linear regression draws (a singular `XᵀX` still gets a
/// small one from [`guarded_cholesky`]).
const LINEAR_RIDGE: f64 = 0.0;

/// Fit a baseline-category logit model with `classes` classes (`y` coded
/// `0..classes`) by Newton's method; returns the stacked coefficients
/// (`(classes − 1) × q`, class 1 first) and the Cholesky factor of their
/// asymptotic covariance.
fn fit_logit(x: &[Vec<f64>], y: &[usize], classes: usize) -> Result<(Vec<f64>, Vec<Vec<f64>>)> {
    let q = x.first().map_or(0, Vec::len);
    let c = classes - 1;
    let dim = c * q;
    let mut beta = vec![0.0; dim];
    let probabilities = |beta: &[f64], row: &[f64]| -> Vec<f64> {
        let eta: Vec<f64> = (0..c).map(|k| dot(&beta[k * q..(k + 1) * q], row)).collect();
        let top = eta.iter().copied().fold(0.0_f64, f64::max);
        let e: Vec<f64> = eta.iter().map(|v| (v - top).exp()).collect();
        let denom = (-top).exp() + e.iter().sum::<f64>();
        e.iter().map(|v| v / denom).collect()
    };
    let penalty = |a: usize| if a % q == 0 { 0.0 } else { LOGIT_RIDGE };
    let information = |beta: &[f64]| -> (Vec<Vec<f64>>, Vec<f64>, f64) {
        let mut h = vec![vec![0.0; dim]; dim];
        let mut g = vec![0.0; dim];
        let mut ll = 0.0;
        for (row, &yi) in x.iter().zip(y) {
            let p = probabilities(beta, row);
            let p0 = 1.0 - p.iter().sum::<f64>();
            ll += if yi == 0 { p0.max(1e-300).ln() } else { p[yi - 1].max(1e-300).ln() };
            for k in 0..c {
                let resid = f64::from(u8::from(yi == k + 1)) - p[k];
                for a in 0..q {
                    g[k * q + a] += resid * row[a];
                }
                for l in 0..c {
                    let w = if k == l { p[k] * (1.0 - p[k]) } else { -p[k] * p[l] };
                    for a in 0..q {
                        for b in 0..q {
                            h[k * q + a][l * q + b] += w * row[a] * row[b];
                        }
                    }
                }
            }
        }
        for a in 0..dim {
            g[a] -= penalty(a) * beta[a];
            h[a][a] += penalty(a);
            ll -= 0.5 * penalty(a) * beta[a] * beta[a];
        }
        (h, g, ll)
    };
    let (mut h, mut g, mut ll) = information(&beta);
    for _ in 0..50 {
        let l = guarded_cholesky(&h)?;
        let step = {
            let z = forward(&l, &g);
            let mut s = vec![0.0; dim];
            for i in (0..dim).rev() {
                s[i] = (z[i] - (i + 1..dim).map(|k| l[k][i] * s[k]).sum::<f64>()) / l[i][i];
            }
            s
        };
        // Step halving keeps the penalized log-likelihood from decreasing.
        let mut scale = 1.0;
        let mut accepted = false;
        for _ in 0..30 {
            let trial: Vec<f64> = beta.iter().zip(&step).map(|(b, s)| b + scale * s).collect();
            let (h2, g2, ll2) = information(&trial);
            if ll2 >= ll - 1e-12 * ll.abs() {
                beta = trial;
                (h, g, accepted) = (h2, g2, true);
                let gain = ll2 - ll;
                ll = ll2;
                if gain.abs() < 1e-10 * (ll.abs() + 1.0) {
                    return Ok((beta, guarded_cholesky(&inverse(&guarded_cholesky(&h)?))?));
                }
                break;
            }
            scale *= 0.5;
        }
        if !accepted {
            break;
        }
    }
    Ok((beta, guarded_cholesky(&inverse(&guarded_cholesky(&h)?))?))
}

/// Draw coefficients `β̂ + L z`.
fn draw_normal(mean: &[f64], l: &[Vec<f64>], rng: &mut Rng) -> Vec<f64> {
    let z: Vec<f64> = (0..mean.len()).map(|_| rng.normal()).collect();
    (0..mean.len()).map(|a| mean[a] + (0..=a).map(|b| l[a][b] * z[b]).sum::<f64>()).collect()
}

/// Redraw the missing cells `missing_rows` of column `j` in `current`.
#[allow(clippy::too_many_arguments)]
fn impute_column(
    current: &mut [Vec<f64>],
    kinds: &[ColumnKind],
    options: &FcsOptions,
    j: usize,
    observed_rows: &[usize],
    missing_rows: &[usize],
    usable: &[bool],
    rng: &mut Rng,
) -> Result<()> {
    if missing_rows.is_empty() {
        return Ok(());
    }
    let predictors: Vec<bool> = options.predictors[j].iter().zip(usable).map(|(a, b)| *a && *b).collect();
    let blocks = design_blocks(current, kinds, &predictors, j, observed_rows);
    let x_obs: Vec<Vec<f64>> = observed_rows.iter().map(|&r| design_row(&current[r], &blocks)).collect();
    let x_mis: Vec<Vec<f64>> = missing_rows.iter().map(|&r| design_row(&current[r], &blocks)).collect();
    let q = blocks.len() + 1;
    match options.methods[j] {
        FcsMethod::None => {}
        FcsMethod::PredictiveMeanMatching | FcsMethod::BayesianRegression => {
            let y: Vec<f64> = observed_rows.iter().map(|&r| current[r][j]).collect();
            if y.len() <= q {
                return Err(invalid(format!(
                    "column {} has {} observed values, too few for its {} predictors",
                    j + 1,
                    y.len(),
                    q - 1
                )));
            }
            let g = rng.chi_square((y.len() - q) as f64);
            let z: Vec<f64> = (0..q).map(|_| rng.normal()).collect();
            let draw = bayesian_regression_draw(&x_obs, &y, LINEAR_RIDGE, g, &z)?;
            if options.methods[j] == FcsMethod::BayesianRegression {
                for (row, &r) in x_mis.iter().zip(missing_rows) {
                    current[r][j] = dot(row, &draw.drawn) + draw.sigma * rng.normal();
                }
            } else {
                let fitted: Vec<f64> = x_obs.iter().map(|row| dot(row, &draw.coefficients)).collect();
                let pool = DonorPool::new(&fitted);
                for (row, &r) in x_mis.iter().zip(missing_rows) {
                    let donors = pool.donors(dot(row, &draw.drawn), options.donors);
                    current[r][j] = y[donors[rng.below(donors.len())]];
                }
            }
        }
        FcsMethod::Logistic | FcsMethod::Multinomial => {
            if !matches!(kinds[j], ColumnKind::Categorical { .. }) {
                return Err(invalid(format!("column {} is not categorical", j + 1)));
            }
            let y: Vec<usize> = observed_rows.iter().map(|&r| current[r][j] as usize).collect();
            // Fit only the levels that occur; others cannot be drawn.
            let mut present: Vec<usize> = y.clone();
            present.sort_unstable();
            present.dedup();
            if present.len() == 1 {
                for &r in missing_rows {
                    current[r][j] = present[0] as f64;
                }
                return Ok(());
            }
            let code: Vec<usize> = y.iter().map(|v| present.binary_search(v).unwrap_or(0)).collect();
            let classes = present.len();
            let (beta, l) = fit_logit(&x_obs, &code, classes)?;
            let drawn = draw_normal(&beta, &l, rng);
            for (row, &r) in x_mis.iter().zip(missing_rows) {
                let eta: Vec<f64> = (0..classes - 1).map(|k| dot(&drawn[k * q..(k + 1) * q], row)).collect();
                let top = eta.iter().copied().fold(0.0_f64, f64::max);
                let mut weights = vec![(-top).exp()];
                weights.extend(eta.iter().map(|v| (v - top).exp()));
                let total: f64 = weights.iter().sum();
                let mut u = rng.next_f64() * total;
                let mut pick = classes - 1;
                for (k, w) in weights.iter().enumerate() {
                    if u < *w {
                        pick = k;
                        break;
                    }
                    u -= w;
                }
                current[r][j] = present[pick] as f64;
            }
        }
    }
    Ok(())
}

/// Mean and standard deviation of the imputed cells of each column.
fn trace(current: &[Vec<f64>], missing: &[Vec<usize>]) -> (Vec<f64>, Vec<f64>) {
    missing
        .iter()
        .enumerate()
        .map(|(j, rows)| {
            if rows.is_empty() {
                return (f64::NAN, f64::NAN);
            }
            let n = rows.len() as f64;
            let mean = rows.iter().map(|&r| current[r][j]).sum::<f64>() / n;
            let sd = if rows.len() > 1 {
                (rows.iter().map(|&r| (current[r][j] - mean).powi(2)).sum::<f64>() / (n - 1.0)).sqrt()
            } else {
                f64::NAN
            };
            (mean, sd)
        })
        .unzip()
}

/// The seed of chain `l`.
fn chain_seed(seed: u64, l: usize) -> u64 {
    seed.wrapping_add(l as u64)
}

/// Multiple imputation by chained equations (see the
/// [module documentation](self)).
///
/// # Errors
/// Ragged rows, infinite values, categorical codes out of range, settings
/// of the wrong size, a method that does not suit its column, `m`,
/// `iterations` or `donors` of zero, a column to impute with no observed
/// value or fewer observed values than predictors, or an imputation model
/// whose predictors are collinear.
pub fn chained_equations(data: &[Vec<f64>], kinds: &[ColumnKind], options: &FcsOptions) -> Result<MultipleImputation> {
    let p = check(data, kinds)?;
    if options.m == 0 || options.iterations == 0 || options.donors == 0 {
        return Err(invalid("the number of imputations, iterations and donors must each be at least 1"));
    }
    if options.methods.len() != p || options.predictors.len() != p || options.predictors.iter().any(|r| r.len() != p) {
        return Err(StatError::DimensionMismatch { context: "imputation settings", expected: p, found: 0 });
    }
    if options.visit_order.iter().any(|&j| j >= p) {
        return Err(invalid("the visit order names a column that does not exist"));
    }
    for (j, (m, k)) in options.methods.iter().zip(kinds).enumerate() {
        if !m.suits(*k) {
            return Err(invalid(format!("the method chosen for column {} does not suit its kind", j + 1)));
        }
    }
    let missing: Vec<Vec<usize>> = (0..p).map(|j| (0..data.len()).filter(|&r| data[r][j].is_nan()).collect()).collect();
    let observed: Vec<Vec<usize>> =
        (0..p).map(|j| (0..data.len()).filter(|&r| !data[r][j].is_nan()).collect()).collect();
    let imputed: Vec<bool> = (0..p)
        .map(|j| options.methods[j] != FcsMethod::None && !missing[j].is_empty() && options.visit_order.contains(&j))
        .collect();
    // A column with missing values that stay missing cannot predict others.
    let usable: Vec<bool> = (0..p).map(|j| missing[j].is_empty() || imputed[j]).collect();
    for j in (0..p).filter(|&j| imputed[j]) {
        if observed[j].is_empty() {
            return Err(invalid(format!("column {} has no observed value to impute from", j + 1)));
        }
    }
    let run_chain = |l: usize| -> Result<ChainRun> {
        let mut rng = Rng::new(chain_seed(options.seed, l));
        let mut current = data.to_vec();
        for j in (0..p).filter(|&j| imputed[j]) {
            for &r in &missing[j] {
                current[r][j] = data[observed[j][rng.below(observed[j].len())]][j];
            }
        }
        let (mut means, mut sds) = (Vec::new(), Vec::new());
        let masked = |c: &[Vec<f64>]| {
            let rows: Vec<Vec<usize>> =
                missing.iter().enumerate().map(|(j, m)| if imputed[j] { m.clone() } else { Vec::new() }).collect();
            trace(c, &rows)
        };
        let (m0, s0) = masked(&current);
        means.push(m0);
        sds.push(s0);
        for _ in 0..options.iterations {
            for &j in &options.visit_order {
                if imputed[j] {
                    impute_column(&mut current, kinds, options, j, &observed[j], &missing[j], &usable, &mut rng)?;
                }
            }
            let (mt, st) = masked(&current);
            means.push(mt);
            sds.push(st);
        }
        Ok((current, means, sds))
    };
    let results: Vec<_> = (0..options.m).into_par_iter().map(run_chain).collect::<Result<Vec<_>>>()?;
    let mut out = MultipleImputation {
        imputations: Vec::with_capacity(options.m),
        chains: Chains { means: Vec::new(), sds: Vec::new() },
        imputed,
    };
    for (data, means, sds) in results {
        out.imputations.push(data);
        out.chains.means.push(means);
        out.chains.sds.push(sds);
    }
    Ok(out)
}

/// Settings of [`normal_data_augmentation`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DataAugmentationOptions {
    /// Number of imputations `m` (independent chains).
    pub m: usize,
    /// Iterations (I-step plus P-step) per chain before the imputation.
    pub iterations: usize,
    /// Seed of the first chain; chain `l` uses `seed + l`.
    pub seed: u64,
}

impl Default for DataAugmentationOptions {
    fn default() -> Self {
        Self { m: 20, iterations: 50, seed: 1 }
    }
}

/// Draw the missing part of every row from its conditional normal
/// distribution given the observed part.
fn i_step(data: &[Vec<f64>], mean: &[f64], sigma: &[Vec<f64>], rng: &mut Rng) -> Result<Vec<Vec<f64>>> {
    let pats = missing_patterns(data)?;
    let p = mean.len();
    let mut out = data.to_vec();
    for pat in &pats.patterns {
        if pat.n_missing() == 0 {
            continue;
        }
        let obs: Vec<usize> = (0..p).filter(|&j| !pat.missing[j]).collect();
        let mis: Vec<usize> = (0..p).filter(|&j| pat.missing[j]).collect();
        // B = Σ_MO Σ_OO⁻¹ and C = Σ_MM − B Σ_OM.
        let (b, c) = if obs.is_empty() {
            (vec![Vec::new(); mis.len()], mis.iter().map(|&a| mis.iter().map(|&bb| sigma[a][bb]).collect()).collect())
        } else {
            let soo: Vec<Vec<f64>> = obs.iter().map(|&a| obs.iter().map(|&bb| sigma[a][bb]).collect()).collect();
            let inv = inverse(&guarded_cholesky(&soo)?);
            let b: Vec<Vec<f64>> = mis
                .iter()
                .map(|&m| (0..obs.len()).map(|k| (0..obs.len()).map(|t| sigma[m][obs[t]] * inv[t][k]).sum()).collect())
                .collect();
            let c: Vec<Vec<f64>> = mis
                .iter()
                .enumerate()
                .map(|(a, &ma)| {
                    mis.iter()
                        .map(|&mb| {
                            sigma[ma][mb] - obs.iter().enumerate().map(|(k, &o)| b[a][k] * sigma[o][mb]).sum::<f64>()
                        })
                        .collect()
                })
                .collect();
            (b, c)
        };
        let l = guarded_cholesky(&c)?;
        for &r in &pat.rows {
            let d: Vec<f64> = obs.iter().map(|&o| data[r][o] - mean[o]).collect();
            let z: Vec<f64> = (0..mis.len()).map(|_| rng.normal()).collect();
            for (a, &m) in mis.iter().enumerate() {
                let cond = mean[m] + b[a].iter().zip(&d).map(|(x, y)| x * y).sum::<f64>();
                out[r][m] = cond + (0..=a).map(|k| l[a][k] * z[k]).sum::<f64>();
            }
        }
    }
    Ok(out)
}

/// Draw `(μ, Σ)` from their posterior given completed data (P-step).
fn p_step(completed: &[Vec<f64>], rng: &mut Rng) -> Result<(Vec<f64>, Vec<Vec<f64>>)> {
    let n = completed.len();
    let p = completed.first().map_or(0, Vec::len);
    let nf = n as f64;
    let mean: Vec<f64> = (0..p).map(|j| completed.iter().map(|r| r[j]).sum::<f64>() / nf).collect();
    let mut a = vec![vec![0.0; p]; p];
    for row in completed {
        for i in 0..p {
            for j in 0..=i {
                a[i][j] += (row[i] - mean[i]) * (row[j] - mean[j]);
            }
        }
    }
    for i in 0..p {
        for j in 0..i {
            a[j][i] = a[i][j];
        }
    }
    // Σ ~ W⁻¹(ν, A) with ν = n − 1: Σ⁻¹ ~ W(ν, A⁻¹). With A⁻¹ = L Lᵀ and
    // Bartlett's lower-triangular T (Tᵢᵢ² ~ χ²(ν − i), i = 0.., Tᵢⱼ ~ N(0, 1)),
    // Σ⁻¹ = (L T)(L T)ᵀ, so Σ = M⁻ᵀ M⁻¹ with M = L T.
    let nu = nf - 1.0;
    let l = guarded_cholesky(&inverse(&guarded_cholesky(&a)?))?;
    let mut t = vec![vec![0.0; p]; p];
    for i in 0..p {
        t[i][i] = rng.chi_square(nu - i as f64).sqrt();
        for j in 0..i {
            t[i][j] = rng.normal();
        }
    }
    let mut mm = vec![vec![0.0; p]; p];
    for i in 0..p {
        for j in 0..=i {
            mm[i][j] = (j..=i).map(|k| l[i][k] * t[k][j]).sum();
        }
    }
    // M⁻¹ (lower triangular) column by column.
    let mut minv = vec![vec![0.0; p]; p];
    for j in 0..p {
        let mut e = vec![0.0; p];
        e[j] = 1.0;
        let x = forward(&mm, &e);
        for i in 0..p {
            minv[i][j] = x[i];
        }
    }
    let sigma: Vec<Vec<f64>> =
        (0..p).map(|i| (0..p).map(|j| (0..p).map(|k| minv[k][i] * minv[k][j]).sum()).collect()).collect();
    let ls = guarded_cholesky(&sigma)?;
    let z: Vec<f64> = (0..p).map(|_| rng.normal()).collect();
    let s = 1.0 / nf.sqrt();
    let mu = (0..p).map(|i| mean[i] + s * (0..=i).map(|k| ls[i][k] * z[k]).sum::<f64>()).collect();
    Ok((mu, sigma))
}

/// Multiple imputation under a multivariate normal model by data
/// augmentation (see the [module documentation](self)). Every column is
/// treated as continuous.
///
/// # Errors
/// As [`em_normal`]; also `m` or `iterations` of zero, or fewer rows than
/// columns plus one (the posterior of `Σ` needs `n − 1 ≥ p`).
pub fn normal_data_augmentation(data: &[Vec<f64>], options: &DataAugmentationOptions) -> Result<MultipleImputation> {
    let p = data.first().map_or(0, Vec::len);
    let kinds = vec![ColumnKind::Continuous; p];
    check(data, &kinds)?;
    if options.m == 0 || options.iterations == 0 {
        return Err(invalid("the number of imputations and iterations must each be at least 1"));
    }
    if data.len() < p + 2 {
        return Err(StatError::InsufficientData { required: p + 2, found: data.len() });
    }
    let em = em_normal(data, &EmOptions::default())?;
    let missing: Vec<Vec<usize>> = (0..p).map(|j| (0..data.len()).filter(|&r| data[r][j].is_nan()).collect()).collect();
    let run_chain = |l: usize| -> Result<ChainRun> {
        let mut rng = Rng::new(chain_seed(options.seed, l));
        let (mut mean, mut sigma) = (em.mean.clone(), em.covariance.clone());
        let (mut means, mut sds) = (Vec::new(), Vec::new());
        let mut completed = i_step(data, &mean, &sigma, &mut rng)?;
        let (m0, s0) = trace(&completed, &missing);
        means.push(m0);
        sds.push(s0);
        for _ in 0..options.iterations {
            (mean, sigma) = p_step(&completed, &mut rng)?;
            completed = i_step(data, &mean, &sigma, &mut rng)?;
            let (mt, st) = trace(&completed, &missing);
            means.push(mt);
            sds.push(st);
        }
        Ok((completed, means, sds))
    };
    let results: Vec<_> = (0..options.m).into_par_iter().map(run_chain).collect::<Result<Vec<_>>>()?;
    let mut out = MultipleImputation {
        imputations: Vec::with_capacity(options.m),
        chains: Chains { means: Vec::new(), sds: Vec::new() },
        imputed: missing.iter().map(|m| !m.is_empty()).collect(),
    };
    for (data, means, sds) in results {
        out.imputations.push(data);
        out.chains.means.push(means);
        out.chains.sds.push(sds);
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
