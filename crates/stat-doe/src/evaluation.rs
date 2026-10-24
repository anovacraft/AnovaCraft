//! Design evaluation: what a design can do for a model before any data are
//! collected.
//!
//! Everything here depends only on the coded runs `x₁ … xₙ` (in `[−1, 1]^k`)
//! and the [`ModelFormula`], through the model matrix `X` (one row `f(xᵢ)ᵀ`
//! per run) and `(XᵀX)⁻¹`, so it serves both a planned design and one already
//! run. The response's error variance `σ²` is unknown before the experiment,
//! so variances are *relative* (in units of `σ²`) and anticipated effects are
//! given in units of `σ`.
//!
//! # Power of the test of each term
//! For term `j` with anticipated coefficient `β` (in units of `σ`) the
//! two-sided `t` test at level `α` rejects when `t² = β̂²/(s² cⱼⱼ)` exceeds
//! `F_{1−α}(1, n − p)`, with `cⱼⱼ = [(XᵀX)⁻¹]ⱼⱼ`. Under the alternative `t²`
//! follows a noncentral `F(1, n − p; λ)` with `λ = β²/cⱼⱼ`, so
//! `power = P(F(1, n − p; λ) > F_{1−α}(1, n − p))` (e.g. Montgomery,
//! *Design and Analysis of Experiments*, §2.4 and §13; Ryan, *Sample Size
//! Determination and Power*, §6). Power needs error degrees of freedom: a
//! saturated design (`n = p`) has none.
//!
//! The relative standard error of a coefficient is `√cⱼⱼ`, and its variance
//! inflation factor is the `j`th diagonal element of the inverse correlation
//! matrix of the non-intercept model columns, `1/(1 − R²ⱼ)` (Marquardt 1970,
//! *Technometrics* 12(3)); 1 when the column is orthogonal to the others.
//!
//! # Prediction variance
//! The relative variance of the predicted mean at `x` is
//! `d(x) = f(x)ᵀ (XᵀX)⁻¹ f(x)`. The report gives its profile along each factor
//! (the others at 0), its maximum over the region (searched over a grid
//! containing the vertices, a uniform random sample, and a local refinement
//! of the best points) and its average over the region, `I = tr(M (XᵀX)⁻¹)`
//! with the exact moment matrix `M` of `[−1, 1]^k`
//! ([`crate::optimal::region_moments`]).
//!
//! # Constrained regions
//! [`evaluate_design_in`] evaluates over a [`Region`]: the cube cut by
//! linear constraints and disallowed combinations, with two-level
//! categorical factors at ±1 only. The largest `d(x)` is then searched over
//! the grid points inside the region and the sample points, and the local
//! refinement stays inside it; the average variance uses the region's moment
//! matrix, estimated from quasi-random (Halton) points of the region
//! ([`crate::optimal::region_moments_in`]); and the fraction-of-design-space
//! sample is the first `samples` Halton points that fall in the region
//! ([`Region::quasi_random_points`]) instead of uniform random points of the
//! cube. Without constraints everything is as for the cube.
//!
//! # Fraction of design space
//! Zahran, Anderson-Cook & Myers (2003), "Fraction of design space to assess
//! prediction capability of response surface designs", *Journal of Quality
//! Technology* 35(4): for a seeded uniform sample of points of the region,
//! the sorted values of `d(x)` plotted against the fraction of the region at
//! or below each value. A flatter, lower curve predicts more uniformly well.
//!
//! # Alias matrix
//! When the true model also contains terms `X₂` that the fitted model `X₁`
//! omits, `E(β̂₁) = β₁ + A β₂` with the alias matrix
//! `A = (X₁ᵀX₁)⁻¹ X₁ᵀ X₂` (Montgomery, §8.6 and §10; Box, Hunter & Hunter,
//! *Statistics for Experimenters*, 2nd ed., §6). The default alias terms are
//! the two-factor interactions not in the model, or the three-factor
//! interactions when the model holds every two-factor interaction.
//!
//! # Efficiencies
//! Atkinson, Donev & Tobias, *Optimum Experimental Designs, with SAS* (2007),
//! §10–11, for `n` runs and `p` terms:
//! `D = 100 |XᵀX|^{1/p}/n`, `A = 100 p/(n tr (XᵀX)⁻¹)`,
//! `G = 100 p/(n max d(x))`. Each is 100% for an orthogonal two-level
//! design whose model matrix has entries ±1 (such as a full factorial for
//! its interaction model); `G` uses the largest `d(x)` found by the search,
//! so it is an upper bound when the search misses the true maximum.

use crate::constraints::Region;
use crate::design::Rng;
use crate::optimal::{ModelFormula, Term, log_det_information, region_moments_in};
use stat_core::{ContinuousDistribution, FisherSnedecor, Matrix, NoncentralF, QrDecomposition, Result, StatError};

/// Settings of [`evaluate_design`].
#[derive(Debug, Clone, PartialEq)]
pub struct EvaluationOptions {
    /// Significance level `α` of the tests whose power is reported.
    pub alpha: f64,
    /// Anticipated coefficient of each non-intercept term, in units of `σ`:
    /// one value for every term, or one per term in model order.
    pub coefficients: Vec<f64>,
    /// Alias terms (`None`: [`default_alias_terms`]). Terms in the model, and
    /// the intercept, are dropped.
    pub alias_terms: Option<Vec<Term>>,
    /// Uniform points of the region for the fraction-of-design-space curve.
    pub samples: usize,
    /// Seed of those points.
    pub seed: u64,
}

impl Default for EvaluationOptions {
    /// `α = 0.05`, coefficient 1, default alias terms, 10 000 points, seed 1.
    fn default() -> Self {
        Self { alpha: 0.05, coefficients: vec![1.0], alias_terms: None, samples: 10_000, seed: 1 }
    }
}

/// One model term's estimability and power.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TermPower {
    /// Index of the term in [`ModelFormula::terms`] (≥ 1).
    pub term: usize,
    /// Anticipated coefficient (units of `σ`).
    pub coefficient: f64,
    /// `√[(XᵀX)⁻¹]ⱼⱼ`: the standard error of the estimate in units of `σ`.
    pub relative_se: f64,
    /// Variance inflation factor (NaN for a constant column).
    pub vif: f64,
    /// Noncentrality `λ = β²/[(XᵀX)⁻¹]ⱼⱼ`.
    pub noncentrality: f64,
    /// Power of the two-sided test (`None` without error degrees of freedom).
    pub power: Option<f64>,
}

/// The fraction-of-design-space curve.
#[derive(Debug, Clone, PartialEq)]
pub struct FractionOfDesignSpace {
    /// `d(x)` at every sampled point, in increasing order; point `i` is at
    /// fraction `(i + 0.5)/N`.
    pub sorted: Vec<f64>,
}

impl FractionOfDesignSpace {
    /// The value at fraction `f ∈ [0, 1]` (linear interpolation between the
    /// order statistics at `(i + 0.5)/N`; the extremes at 0 and 1).
    #[must_use]
    pub fn quantile(&self, f: f64) -> f64 {
        let n = self.sorted.len();
        if n == 0 {
            return f64::NAN;
        }
        let pos = (f.clamp(0.0, 1.0) * n as f64 - 0.5).clamp(0.0, (n - 1) as f64);
        let (i, t) = (pos.floor() as usize, pos - pos.floor());
        if i + 1 < n { self.sorted[i] * (1.0 - t) + self.sorted[i + 1] * t } else { self.sorted[i] }
    }

    /// At most `points` points `[fraction, d]` of the curve, for plotting.
    #[must_use]
    pub fn curve(&self, points: usize) -> Vec<[f64; 2]> {
        let points = points.max(2);
        (0..points)
            .map(|i| {
                let f = i as f64 / (points - 1) as f64;
                [f, self.quantile(f)]
            })
            .collect()
    }
}

/// The evaluation of a design for a model.
#[derive(Debug, Clone, PartialEq)]
pub struct DesignEvaluation {
    /// The model.
    pub formula: ModelFormula,
    /// Number of runs `n`.
    pub runs: usize,
    /// Error degrees of freedom `n − p`.
    pub error_df: usize,
    /// `(XᵀX)⁻¹`, `p × p` in model term order.
    pub inverse: Vec<Vec<f64>>,
    /// Power and standard error of each non-intercept term.
    pub terms: Vec<TermPower>,
    /// D-efficiency (%).
    pub d_efficiency: f64,
    /// A-efficiency (%).
    pub a_efficiency: f64,
    /// G-efficiency (%), from the largest `d(x)` found.
    pub g_efficiency: f64,
    /// Average relative prediction variance over the region, `tr(M(XᵀX)⁻¹)`.
    pub average_variance: f64,
    /// The largest relative prediction variance found.
    pub max_variance: f64,
    /// Where it was found (coded).
    pub max_at: Vec<f64>,
    /// `ln |XᵀX|`.
    pub log_det: f64,
    /// `tr (XᵀX)⁻¹`.
    pub trace_inverse: f64,
    /// Alias terms (columns of [`alias_matrix`](Self::alias_matrix)).
    pub alias_terms: Vec<Term>,
    /// `A = (X₁ᵀX₁)⁻¹X₁ᵀX₂`: one row per model term (intercept first), one
    /// column per alias term.
    pub alias_matrix: Vec<Vec<f64>>,
    /// Absolute correlations among the non-intercept model columns followed
    /// by the alias columns (NaN where a column is constant).
    pub correlations: Vec<Vec<f64>>,
    /// Fraction-of-design-space curve.
    pub fds: FractionOfDesignSpace,
    /// Number of quasi-random points of a constrained region behind the
    /// average variance (its moments are estimated); `None` for the whole
    /// cube, whose moments are exact.
    pub region_points: Option<usize>,
}

impl DesignEvaluation {
    /// Relative prediction variance `d(x) = f(x)ᵀ(XᵀX)⁻¹f(x)` at coded `x`.
    #[must_use]
    pub fn variance(&self, x: &[f64]) -> f64 {
        quad_form(&self.inverse, &self.formula.expand(x))
    }

    /// `d(x)` along factor `j` from −1 to +1 in `points` steps with the other
    /// factors at 0, as `[x_j, d]`.
    #[must_use]
    pub fn profile(&self, j: usize, points: usize) -> Vec<[f64; 2]> {
        let points = points.max(2);
        let mut x = vec![0.0; self.formula.n_factors()];
        (0..points)
            .map(|i| {
                let v = -1.0 + 2.0 * i as f64 / (points - 1) as f64;
                x[j] = v;
                [v, self.variance(&x)]
            })
            .collect()
    }

    /// The model terms followed by the alias terms (the rows and columns of
    /// [`correlations`](Self::correlations)), without the intercept.
    #[must_use]
    pub fn correlation_terms(&self) -> Vec<Term> {
        self.formula.terms()[1..].iter().chain(&self.alias_terms).cloned().collect()
    }
}

/// The default alias terms of `formula`: the two-factor interactions it does
/// not contain; when it contains all of them, the three-factor interactions
/// it does not contain (empty with fewer factors).
#[must_use]
pub fn default_alias_terms(formula: &ModelFormula) -> Vec<Term> {
    let k = formula.n_factors();
    let products = |order: usize| -> Vec<Term> {
        let mut out = Vec::new();
        let mut pick = vec![0usize; order];
        fn rec(k: usize, start: usize, depth: usize, pick: &mut [usize], out: &mut Vec<Term>) {
            if depth == pick.len() {
                let mut powers = vec![0u8; k];
                pick.iter().for_each(|&j| powers[j] = 1);
                out.push(Term { powers });
                return;
            }
            for j in start..k {
                pick[depth] = j;
                rec(k, j + 1, depth + 1, pick, out);
            }
        }
        rec(k, 0, 0, &mut pick, &mut out);
        out.retain(|t| !formula.terms().contains(t));
        out
    };
    let two = products(2);
    if two.is_empty() && k >= 3 { products(3) } else { two }
}

/// Evaluate coded `runs` for `formula` over the whole cube `[−1, 1]^k` (see
/// the [module](self) documentation).
///
/// # Errors
/// As [`evaluate_design_in`].
pub fn evaluate_design(
    formula: &ModelFormula,
    runs: &[Vec<f64>],
    options: &EvaluationOptions,
) -> Result<DesignEvaluation> {
    evaluate_design_in(formula, runs, &Region::cube(formula.n_factors()), options)
}

/// Evaluate coded `runs` for `formula` over `region`: the maximum and
/// average prediction variance, G- and I-criteria and the
/// fraction-of-design-space curve refer to the points of the region (see
/// the [module](self) documentation). The runs themselves need not lie in
/// the region.
///
/// # Errors
/// * [`StatError::InvalidArgument`] if `region` has a different number of
///   factors, or is too small to sample (as [`region_moments_in`]); and
/// * [`StatError::InvalidArgument`] for no runs, runs of the wrong length,
///   non-finite settings, fewer runs than model terms, `α` outside (0, 1), a
///   coefficient list of the wrong length, or no sample points.
/// * [`StatError::RankDeficient`] if the design cannot estimate every term.
pub fn evaluate_design_in(
    formula: &ModelFormula,
    runs: &[Vec<f64>],
    region: &Region,
    options: &EvaluationOptions,
) -> Result<DesignEvaluation> {
    let (k, p, n) = (formula.n_factors(), formula.n_terms(), runs.len());
    if region.n_factors() != k {
        return Err(invalid(format!("the region has {} factors but the model has {k}", region.n_factors())));
    }
    if k == 0 || n == 0 {
        return Err(invalid("a design needs at least one factor and one run"));
    }
    if runs.iter().any(|r| r.len() != k || r.iter().any(|v| !v.is_finite())) {
        return Err(invalid(format!("every run needs {k} finite coded settings")));
    }
    if n < p {
        return Err(invalid(format!("the model has {p} terms, so it needs at least {p} runs (the design has {n})")));
    }
    if !(options.alpha > 0.0 && options.alpha < 1.0) {
        return Err(invalid("the significance level must be between 0 and 1"));
    }
    if options.samples == 0 {
        return Err(invalid("use at least one sample point"));
    }
    let coefficients: Vec<f64> = match options.coefficients.len() {
        1 => vec![options.coefficients[0]; p - 1],
        m if m == p - 1 => options.coefficients.clone(),
        m => return Err(invalid(format!("give one anticipated coefficient, or one per term ({}); got {m}", p - 1))),
    };
    if coefficients.iter().any(|b| !b.is_finite()) {
        return Err(invalid("anticipated coefficients must be finite"));
    }

    let rows: Vec<Vec<f64>> = runs.iter().map(|x| formula.expand(x)).collect();
    let x1 = Matrix::from_rows(&rows)?;
    let qr = QrDecomposition::new(x1)?;
    let rank = qr.rank(qr.default_rank_tolerance() * 1e3);
    if rank < p {
        return Err(StatError::RankDeficient { rank, columns: p });
    }
    let inv_m = qr.inverse_gram()?;
    let inverse: Vec<Vec<f64>> = (0..p).map(|i| (0..p).map(|j| inv_m[(i, j)]).collect()).collect();
    let error_df = n - p;

    // Power, standard errors and variance inflation.
    let vif = variance_inflation(&rows, p);
    let critical = if error_df > 0 {
        Some(FisherSnedecor::new(1.0, error_df as f64)?.quantile(1.0 - options.alpha)?)
    } else {
        None
    };
    let terms = (1..p)
        .map(|j| {
            let c = inverse[j][j];
            let beta = coefficients[j - 1];
            let noncentrality = beta * beta / c;
            let power = match critical {
                Some(fc) => Some(NoncentralF::new(1.0, error_df as f64, noncentrality)?.sf(fc)?),
                None => None,
            };
            Ok(TermPower { term: j, coefficient: beta, relative_se: c.sqrt(), vif: vif[j - 1], noncentrality, power })
        })
        .collect::<Result<Vec<_>>>()?;

    // Efficiencies and prediction variance.
    let log_det = log_det_information(formula, runs);
    let trace_inverse: f64 = (0..p).map(|i| inverse[i][i]).sum();
    let (moments, region_points) = region_moments_in(formula, region)?;
    let constrained = !region.is_unconstrained();
    let average_variance: f64 = (0..p).map(|i| (0..p).map(|j| moments[i][j] * inverse[j][i]).sum::<f64>()).sum::<f64>();
    let mut rng = Rng::new(options.seed);
    let mut sample: Vec<f64> = Vec::with_capacity(options.samples);
    let mut best: Vec<(f64, Vec<f64>)> = Vec::new();
    let consider = |x: Vec<f64>, d: f64, best: &mut Vec<(f64, Vec<f64>)>| {
        if best.len() < 8 || d > best[best.len() - 1].0 {
            best.push((d, x));
            best.sort_by(|a, b| b.0.total_cmp(&a.0));
            best.truncate(8);
        }
    };
    let variance = |x: &[f64]| quad_form(&inverse, &formula.expand(x));
    for x in grid_points(k) {
        if constrained && !region.contains(&x) {
            continue;
        }
        let d = variance(&x);
        consider(x, d, &mut best);
    }
    if constrained {
        let points = region.quasi_random_points(options.samples);
        if points.is_empty() {
            return Err(invalid("no point of the constrained region was found to evaluate the design over"));
        }
        for x in points {
            let d = variance(&x);
            sample.push(d);
            consider(x, d, &mut best);
        }
    } else {
        for _ in 0..options.samples {
            let x: Vec<f64> = (0..k).map(|_| 2.0 * rng.next_f64() - 1.0).collect();
            let d = variance(&x);
            sample.push(d);
            consider(x, d, &mut best);
        }
    }
    // The refinement may not leave the region.
    let inside = |x: &[f64]| if !constrained || region.contains(x) { variance(x) } else { f64::NEG_INFINITY };
    let (mut max_variance, mut max_at) = (f64::NEG_INFINITY, vec![0.0; k]);
    for (d, x) in best {
        let (d, x) = refine(&inside, d, x);
        if d > max_variance {
            (max_variance, max_at) = (d, x);
        }
    }
    sample.sort_by(f64::total_cmp);

    let nf = n as f64;
    let pf = p as f64;
    let d_efficiency = 100.0 * (log_det / pf).exp() / nf;
    let a_efficiency = 100.0 * pf / (nf * trace_inverse);
    let g_efficiency = 100.0 * pf / (nf * max_variance);

    // Alias matrix and correlations.
    let mut alias_terms = options.alias_terms.clone().unwrap_or_else(|| default_alias_terms(formula));
    alias_terms.retain(|t| t.powers.len() == k && t.degree() > 0 && !formula.terms().contains(t));
    let mut seen = Vec::new();
    alias_terms.retain(|t| {
        let new = !seen.contains(t);
        seen.push(t.clone());
        new
    });
    let alias_columns: Vec<Vec<f64>> = alias_terms.iter().map(|t| runs.iter().map(|x| t.eval(x)).collect()).collect();
    let mut alias_matrix = vec![vec![0.0; alias_terms.len()]; p];
    for (c, column) in alias_columns.iter().enumerate() {
        let a = qr.solve_least_squares(column)?;
        for (i, v) in a.into_iter().enumerate() {
            alias_matrix[i][c] = clean(v);
        }
    }
    let mut columns: Vec<Vec<f64>> = (1..p).map(|j| rows.iter().map(|r| r[j]).collect()).collect();
    columns.extend(alias_columns);
    let correlations =
        columns.iter().map(|a| columns.iter().map(|b| clean(correlation(a, b).abs())).collect()).collect();

    Ok(DesignEvaluation {
        formula: formula.clone(),
        runs: n,
        error_df,
        inverse,
        terms,
        d_efficiency,
        a_efficiency,
        g_efficiency,
        average_variance,
        max_variance,
        max_at,
        log_det,
        trace_inverse,
        alias_terms,
        alias_matrix,
        correlations,
        fds: FractionOfDesignSpace { sorted: sample },
        region_points,
    })
}

/// Two designs compared for the same model: each ratio is design 1's value
/// relative to design 2's (above 1 when design 1 is better), and the
/// run-adjusted ratios compare information per run (the ratios of the
/// efficiencies).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DesignComparison {
    /// `(|X₁ᵀX₁| / |X₂ᵀX₂|)^{1/p}`.
    pub relative_d: f64,
    /// The same per run: `D₁ / D₂` (D-efficiencies).
    pub relative_d_per_run: f64,
    /// `tr(X₂ᵀX₂)⁻¹ / tr(X₁ᵀX₁)⁻¹`.
    pub relative_a: f64,
    /// `A₁ / A₂`.
    pub relative_a_per_run: f64,
    /// `max d₂ / max d₁`.
    pub relative_g: f64,
    /// `G₁ / G₂`.
    pub relative_g_per_run: f64,
    /// `I₂ / I₁`.
    pub relative_i: f64,
    /// `n₂ I₂ / (n₁ I₁)`.
    pub relative_i_per_run: f64,
}

/// Compare two evaluations of the same model.
///
/// # Errors
/// [`StatError::InvalidArgument`] if the models differ.
pub fn compare_designs(first: &DesignEvaluation, second: &DesignEvaluation) -> Result<DesignComparison> {
    if first.formula != second.formula {
        return Err(invalid("designs can only be compared for the same model"));
    }
    let p = first.formula.n_terms() as f64;
    let (n1, n2) = (first.runs as f64, second.runs as f64);
    Ok(DesignComparison {
        relative_d: ((first.log_det - second.log_det) / p).exp(),
        relative_d_per_run: first.d_efficiency / second.d_efficiency,
        relative_a: second.trace_inverse / first.trace_inverse,
        relative_a_per_run: first.a_efficiency / second.a_efficiency,
        relative_g: second.max_variance / first.max_variance,
        relative_g_per_run: first.g_efficiency / second.g_efficiency,
        relative_i: second.average_variance / first.average_variance,
        relative_i_per_run: (n2 * second.average_variance) / (n1 * first.average_variance),
    })
}

/// `vᵀ M v`.
fn quad_form(m: &[Vec<f64>], v: &[f64]) -> f64 {
    m.iter().zip(v).map(|(row, &vi)| vi * row.iter().zip(v).map(|(a, b)| a * b).sum::<f64>()).sum()
}

/// Round values within rounding error of 0 (or of ±1 for correlations) to
/// those exact values, so exact aliasing prints as such.
fn clean(v: f64) -> f64 {
    if v.abs() < 1e-12 {
        0.0
    } else if (v.abs() - 1.0).abs() < 1e-12 {
        v.signum()
    } else {
        v
    }
}

fn correlation(a: &[f64], b: &[f64]) -> f64 {
    let n = a.len() as f64;
    let (ma, mb) = (a.iter().sum::<f64>() / n, b.iter().sum::<f64>() / n);
    let sab: f64 = a.iter().zip(b).map(|(x, y)| (x - ma) * (y - mb)).sum();
    let saa: f64 = a.iter().map(|x| (x - ma).powi(2)).sum();
    let sbb: f64 = b.iter().map(|y| (y - mb).powi(2)).sum();
    let scale = 1e-24 * n;
    if saa > scale && sbb > scale { sab / (saa * sbb).sqrt() } else { f64::NAN }
}

/// Variance inflation factors of the non-intercept columns of the model
/// rows: the diagonal of the inverse of their correlation matrix (NaN for a
/// constant column, ∞ when the columns are collinear).
fn variance_inflation(rows: &[Vec<f64>], p: usize) -> Vec<f64> {
    let m = p - 1;
    let n = rows.len() as f64;
    let mut centred: Vec<Vec<f64>> = (1..p)
        .map(|j| {
            let mean = rows.iter().map(|r| r[j]).sum::<f64>() / n;
            rows.iter().map(|r| r[j] - mean).collect()
        })
        .collect();
    let constant: Vec<bool> = centred.iter().map(|c| c.iter().map(|v| v * v).sum::<f64>() < 1e-24 * n).collect();
    for c in &mut centred {
        let norm = c.iter().map(|v| v * v).sum::<f64>().sqrt();
        if norm > 0.0 {
            c.iter_mut().for_each(|v| *v /= norm);
        }
    }
    let used: Vec<usize> = (0..m).filter(|&j| !constant[j]).collect();
    let mut out = vec![f64::NAN; m];
    if used.is_empty() {
        return out;
    }
    let z: Vec<&[f64]> = used.iter().map(|&j| centred[j].as_slice()).collect();
    let inverse = Matrix::from_columns(&z).and_then(QrDecomposition::new).and_then(|q| {
        if q.rank(q.default_rank_tolerance() * 1e3) < used.len() { Err(rank_error()) } else { q.inverse_gram() }
    });
    match inverse {
        Ok(inv) => used.iter().enumerate().for_each(|(i, &j)| out[j] = inv[(i, i)]),
        Err(_) => used.iter().for_each(|&j| out[j] = f64::INFINITY),
    }
    out
}

fn rank_error() -> StatError {
    StatError::RankDeficient { rank: 0, columns: 0 }
}

/// Grid points for the search for the largest `d(x)`: `{−1, −½, 0, ½, 1}^k`
/// for up to 5 factors, `{−1, 0, 1}^k` up to 8, the vertices up to 13, and
/// otherwise the vertices of a seeded random subset of 8192 vertices.
fn grid_points(k: usize) -> Vec<Vec<f64>> {
    let levels: &[f64] = if k <= 5 {
        &[-1.0, -0.5, 0.0, 0.5, 1.0]
    } else if k <= 8 {
        &[-1.0, 0.0, 1.0]
    } else {
        &[-1.0, 1.0]
    };
    if k > 13 {
        let mut rng = Rng::new(7);
        return (0..8192).map(|_| (0..k).map(|_| if rng.next_f64() < 0.5 { -1.0 } else { 1.0 }).collect()).collect();
    }
    let m = levels.len();
    let total = m.pow(k as u32);
    (0..total)
        .map(|mut i| {
            (0..k)
                .map(|_| {
                    let l = levels[i % m];
                    i /= m;
                    l
                })
                .collect()
        })
        .collect()
}

/// Local coordinate search for a larger `d(x)` inside the cube (`f` is −∞
/// outside a constrained region, so moves there are never taken), with steps
/// halving from 0.25 to 1e-6 (at most 100 000 moves).
fn refine(f: &impl Fn(&[f64]) -> f64, mut best: f64, mut x: Vec<f64>) -> (f64, Vec<f64>) {
    let mut step = 0.25;
    for _ in 0..100_000 {
        if step <= 1e-6 {
            break;
        }
        let mut improved = false;
        for j in 0..x.len() {
            for dir in [-1.0, 1.0] {
                let mut y = x.clone();
                y[j] = (y[j] + dir * step).clamp(-1.0, 1.0);
                let d = f(&y);
                if d > best + 1e-15 {
                    (best, x) = (d, y);
                    improved = true;
                }
            }
        }
        if !improved {
            step /= 2.0;
        }
    }
    (best, x)
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(k: usize) -> Vec<String> {
        crate::design::default_factor_names(k)
    }

    fn cube(levels: &[f64], k: usize) -> Vec<Vec<f64>> {
        let m = levels.len();
        (0..m.pow(k as u32))
            .map(|mut i| {
                let mut x = vec![0.0; k];
                for v in x.iter_mut().rev() {
                    *v = levels[i % m];
                    i /= m;
                }
                x
            })
            .collect()
    }

    /// Face-centred central composite in three factors with one centre run.
    fn face_centred() -> Vec<Vec<f64>> {
        let mut runs = cube(&[-1.0, 1.0], 3);
        for j in 0..3 {
            for s in [-1.0, 1.0] {
                let mut r = vec![0.0; 3];
                r[j] = s;
                runs.push(r);
            }
        }
        runs.push(vec![0.0; 3]);
        runs
    }

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol * b.abs().max(1.0)
    }

    /// 2³ factorial and the interaction model: XᵀX = 8I, so by hand
    /// d(x) = (1 + Σxᵢ² + Σxᵢ²xⱼ²)/8 — 1/8 at the centre, 7/8 at a vertex —
    /// D = A = G = 100%, I = (1 + 3·⅓ + 3·⅑)/8 = 7/24, every VIF 1, and the
    /// power with β = 1, α = 0.05 on 1 error df is SciPy's
    /// `ncf.sf(f.ppf(.95, 1, 1), 1, 1, 8)` = 0.17570741109776025.
    #[test]
    fn full_factorial_by_hand() {
        let f = ModelFormula::interactions(3);
        let e = evaluate_design(&f, &cube(&[-1.0, 1.0], 3), &EvaluationOptions::default()).unwrap();
        assert_eq!((e.runs, e.error_df), (8, 1));
        let cases = [([0.0, 0.0, 0.0], 0.125), ([1.0, -1.0, 1.0], 0.875), ([0.5, 0.0, -1.0], 2.5 / 8.0)];
        for (x, want) in cases {
            assert!(close(e.variance(&x), want, 1e-14), "{x:?}");
        }
        assert!(close(e.d_efficiency, 100.0, 1e-12) && close(e.a_efficiency, 100.0, 1e-12));
        assert!(close(e.g_efficiency, 100.0, 1e-12), "{}", e.g_efficiency);
        assert!(close(e.max_variance, 0.875, 1e-14));
        assert!(e.max_at.iter().all(|v| v.abs() == 1.0));
        assert!(close(e.average_variance, 7.0 / 24.0, 1e-14));
        for t in &e.terms {
            assert!(close(t.relative_se, 8f64.powf(-0.5), 1e-14));
            assert!(close(t.vif, 1.0, 1e-12));
            assert!(close(t.noncentrality, 8.0, 1e-12));
            assert!(close(t.power.unwrap(), 0.175_707_411_097_760_25, 1e-8), "{:?}", t.power);
        }
        let profile = e.profile(0, 5);
        assert_eq!(profile.len(), 5);
        for [x, d] in profile {
            assert!(close(d, (1.0 + x * x) / 8.0, 1e-14));
        }
        // All two-factor interactions are in the model: the alias term is ABC,
        // orthogonal to every model column.
        assert_eq!(e.alias_terms.len(), 1);
        assert_eq!(e.alias_terms[0].label(&names(3)), "A × B × C");
        assert!(e.alias_matrix.iter().all(|r| r[0] == 0.0));
    }

    /// Power against SciPy (`tests/data/design_evaluation_reference.py`) for
    /// designs with 4, 3 and 5 error degrees of freedom, common and per-term
    /// coefficients and two levels; VIFs against NumPy.
    #[test]
    fn power_matches_scipy() {
        let two = cube(&[-1.0, 1.0], 3);
        let mut o = EvaluationOptions::default();
        let e = evaluate_design(&ModelFormula::main_effects(3), &two, &o).unwrap();
        assert!(e.terms.iter().all(|t| close(t.power.unwrap(), 0.571_609_457_700_715_5, 1e-8)));
        o.alpha = 0.10;
        o.coefficients = vec![0.5];
        let e = evaluate_design(&ModelFormula::main_effects(3), &two, &o).unwrap();
        assert!(e.terms.iter().all(|t| close(t.power.unwrap(), 0.322_737_395_079_058_35, 1e-8)));

        let o = EvaluationOptions { coefficients: vec![1.5], ..EvaluationOptions::default() };
        let e = evaluate_design(&ModelFormula::quadratic(2), &cube(&[-1.0, 0.0, 1.0], 2), &o).unwrap();
        let want = [
            0.689_782_802_073_648,
            0.689_782_802_073_648,
            0.532_792_607_261_341_7,
            0.316_450_555_697_056_27,
            0.316_450_555_697_056_27,
        ];
        for (t, w) in e.terms.iter().zip(want) {
            assert!(close(t.power.unwrap(), w, 1e-8), "{t:?} {w}");
        }

        let betas = vec![1.0, 2.0, 0.5, 1.0, 1.5, 1.0, 2.0, 2.5, 1.0];
        let o = EvaluationOptions { coefficients: betas, ..EvaluationOptions::default() };
        let e = evaluate_design(&ModelFormula::quadratic(3), &face_centred(), &o).unwrap();
        let want = [
            0.716_117_152_419_253,
            0.998_359_477_849_263_2,
            0.251_758_561_046_980_86,
            0.622_802_674_343_012_1,
            0.918_594_173_516_366_7,
            0.622_802_674_343_012_1,
            0.727_721_405_876_787_2,
            0.888_547_167_528_551_8,
            0.257_476_069_062_986_5,
        ];
        for (t, w) in e.terms.iter().zip(want) {
            assert!(close(t.power.unwrap(), w, 1e-8), "{t:?} {w}");
        }
        let vif = [1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 35.0 / 27.0, 35.0 / 27.0, 35.0 / 27.0];
        for (t, w) in e.terms.iter().zip(vif) {
            assert!(close(t.vif, w, 1e-12), "{t:?} {w}");
        }
        assert_eq!(e.error_df, 5);
    }

    /// The 3² factorial and the quadratic model: I = 0.45 exactly (the NumPy
    /// Monte Carlo mean is 0.44998); the largest d(x) is at a vertex,
    /// 5/9 + 2/6 + 1/4 + 2/2 − 4/3 = 29/36; the fraction-of-design-space
    /// quantiles agree with a 4-million-point NumPy sample within Monte
    /// Carlo error.
    #[test]
    fn three_level_factorial_quadratic() {
        let o = EvaluationOptions { samples: 200_000, seed: 11, ..EvaluationOptions::default() };
        let e = evaluate_design(&ModelFormula::quadratic(2), &cube(&[-1.0, 0.0, 1.0], 2), &o).unwrap();
        assert!(close(e.average_variance, 0.45, 1e-13), "{}", e.average_variance);
        assert!(close(e.max_variance, 29.0 / 36.0, 1e-12), "{}", e.max_variance);
        assert!(close(e.g_efficiency, 100.0 * 6.0 / (9.0 * 29.0 / 36.0), 1e-12));
        let numpy = [
            (0.1, 0.378_837_229_941_701_6),
            (0.25, 0.408_078_450_275_96),
            (0.5, 0.440_006_949_654_392_35),
            (0.75, 0.487_860_938_604_669_9),
            (0.9, 0.527_431_788_965_599_5),
        ];
        for (f, w) in numpy {
            let q = e.fds.quantile(f);
            assert!((q - w).abs() < 0.003, "fraction {f}: {q} vs {w}");
        }
        let mean = e.fds.sorted.iter().sum::<f64>() / e.fds.sorted.len() as f64;
        assert!((mean - 0.45).abs() < 0.002, "{mean}");
        let curve = e.fds.curve(11);
        assert_eq!(curve.len(), 11);
        assert!(curve.windows(2).all(|w| w[1][1] >= w[0][1]));
        assert_eq!(curve[10][1], *e.fds.sorted.last().unwrap());
    }

    /// FDS of the face-centred design against NumPy, with the default sample,
    /// and its exact average variance against the Monte Carlo mean 0.36760.
    #[test]
    fn face_centred_fds() {
        let e = evaluate_design(&ModelFormula::quadratic(3), &face_centred(), &EvaluationOptions::default()).unwrap();
        let numpy = [(0.1, 0.269_409_100_966_720_66), (0.5, 0.344_996_536_982_815_5), (0.9, 0.501_271_246_358_068_5)];
        for (f, w) in numpy {
            assert!((e.fds.quantile(f) - w).abs() < 0.01, "{f}: {}", e.fds.quantile(f));
        }
        assert!((e.average_variance - 0.367_6).abs() < 0.002, "{}", e.average_variance);
    }

    /// The 2⁴⁻¹ with D = ABC (I = ABCD): with main effects and AB, AC, AD in
    /// the model, the remaining two-factor interactions are completely
    /// aliased: AB ↔ CD, AC ↔ BD, AD ↔ BC, each with coefficient exactly 1
    /// (Montgomery §8.2; Box, Hunter & Hunter §6), and absolute correlation 1.
    #[test]
    fn half_fraction_alias_matrix() {
        let runs: Vec<Vec<f64>> =
            cube(&[-1.0, 1.0], 3).into_iter().map(|r| vec![r[0], r[1], r[2], r[0] * r[1] * r[2]]).collect();
        let n = names(4);
        let f = ModelFormula::parse("A + B + C + D + A*B + A*C + A*D", &n).unwrap();
        let e = evaluate_design(&f, &runs, &EvaluationOptions::default()).unwrap();
        assert_eq!(e.error_df, 0);
        assert!(e.terms.iter().all(|t| t.power.is_none()), "saturated: no error df");
        let labels: Vec<String> = e.alias_terms.iter().map(|t| t.label(&n)).collect();
        assert_eq!(labels, ["B × C", "B × D", "C × D"]);
        let model: Vec<String> = f.terms().iter().map(|t| t.label(&n)).collect();
        for (i, row) in e.alias_matrix.iter().enumerate() {
            for (c, &v) in row.iter().enumerate() {
                let want = match (model[i].as_str(), labels[c].as_str()) {
                    ("A × B", "C × D") | ("A × C", "B × D") | ("A × D", "B × C") => 1.0,
                    _ => 0.0,
                };
                assert_eq!(v, want, "{} / {}", model[i], labels[c]);
            }
        }
        // Rows and columns: A B C D AB AC AD BC BD CD.
        assert_eq!(e.correlations.len(), 10);
        assert_eq!(e.correlations[4][9], 1.0, "AB and CD are the same column");
        assert_eq!(e.correlations[0][1], 0.0);
        assert_eq!(e.correlation_terms().len(), 10);
    }

    /// The 12-run Plackett–Burman design (generator row + + − + + + − − − + −,
    /// Plackett & Burman 1946): each main effect is partially aliased with
    /// every two-factor interaction not involving it, with coefficient ±1/3
    /// (Montgomery §8.6; Box, Hunter & Hunter §7), and not at all with those
    /// that involve it.
    #[test]
    fn plackett_burman_partial_aliasing() {
        let g = [1.0, 1.0, -1.0, 1.0, 1.0, 1.0, -1.0, -1.0, -1.0, 1.0, -1.0];
        let mut runs: Vec<Vec<f64>> = (0..11).map(|s| (0..11).map(|j| g[(j + 11 - s) % 11]).collect()).collect();
        runs.push(vec![-1.0; 11]);
        let f = ModelFormula::main_effects(11);
        let o = EvaluationOptions { samples: 500, ..EvaluationOptions::default() };
        let e = evaluate_design(&f, &runs, &o).unwrap();
        assert!(close(e.d_efficiency, 100.0, 1e-12));
        assert_eq!(e.alias_terms.len(), 55);
        assert!(e.alias_matrix[0].iter().all(|&v| v == 0.0), "intercept");
        for i in 1..12 {
            for (c, t) in e.alias_terms.iter().enumerate() {
                let v = e.alias_matrix[i][c];
                if t.powers[i - 1] == 1 {
                    assert_eq!(v, 0.0);
                } else {
                    assert!(close(v.abs(), 1.0 / 3.0, 1e-12), "row {i} {t:?}: {v}");
                }
            }
        }
    }

    #[test]
    fn comparison_and_errors() {
        let f = ModelFormula::interactions(3);
        let one = cube(&[-1.0, 1.0], 3);
        let two: Vec<Vec<f64>> = one.iter().chain(&one).cloned().collect();
        let o = EvaluationOptions { samples: 1000, ..EvaluationOptions::default() };
        let (a, b) = (evaluate_design(&f, &two, &o).unwrap(), evaluate_design(&f, &one, &o).unwrap());
        let c = compare_designs(&a, &b).unwrap();
        assert!(close(c.relative_d, 2.0, 1e-12) && close(c.relative_d_per_run, 1.0, 1e-12));
        assert!(close(c.relative_a, 2.0, 1e-12) && close(c.relative_a_per_run, 1.0, 1e-12));
        assert!(close(c.relative_g, 2.0, 1e-12) && close(c.relative_g_per_run, 1.0, 1e-12));
        assert!(close(c.relative_i, 2.0, 1e-12) && close(c.relative_i_per_run, 1.0, 1e-12));
        let other = evaluate_design(&ModelFormula::main_effects(3), &one, &o).unwrap();
        assert!(compare_designs(&a, &other).is_err());

        // A replicated 2² cannot estimate pure quadratic terms.
        let sq = cube(&[-1.0, 1.0], 2);
        let rep: Vec<Vec<f64>> = sq.iter().chain(&sq).cloned().collect();
        assert!(matches!(evaluate_design(&ModelFormula::quadratic(2), &rep, &o), Err(StatError::RankDeficient { .. })));
        assert!(evaluate_design(&ModelFormula::quadratic(2), &sq, &o).is_err(), "fewer runs than terms");
        let bad = EvaluationOptions { coefficients: vec![1.0, 2.0], ..o.clone() };
        assert!(evaluate_design(&f, &one, &bad).is_err());
        let bad = EvaluationOptions { alpha: 1.0, ..o.clone() };
        assert!(evaluate_design(&f, &one, &bad).is_err());
        assert!(evaluate_design(&f, &[vec![0.0, f64::NAN, 0.0]], &o).is_err());
        // An explicit alias list drops model terms and duplicates.
        let n = names(3);
        let extra = ModelFormula::parse("A*B + A*B*C + A^2", &n).unwrap().terms()[1..].to_vec();
        let o = EvaluationOptions { alias_terms: Some(extra.iter().chain(&extra).cloned().collect()), ..o };
        let e = evaluate_design(&f, &one, &o).unwrap();
        let labels: Vec<String> = e.alias_terms.iter().map(|t| t.label(&n)).collect();
        assert_eq!(labels, ["A²", "A × B × C"]);
        // A² is the constant 1 on a two-level design: aliased with the intercept.
        assert_eq!(e.alias_matrix[0][0], 1.0);
        assert!(e.correlations[6][6].is_nan());
    }

    #[test]
    fn default_alias_terms_by_model() {
        let n = names(3);
        let label = |ts: Vec<Term>| ts.iter().map(|t| t.label(&n)).collect::<Vec<_>>();
        assert_eq!(label(default_alias_terms(&ModelFormula::main_effects(3))), ["A × B", "A × C", "B × C"]);
        assert_eq!(label(default_alias_terms(&ModelFormula::quadratic(3))), ["A × B × C"]);
        assert!(default_alias_terms(&ModelFormula::main_effects(1)).is_empty());
        assert!(default_alias_terms(&ModelFormula::interactions(2)).is_empty());
    }

    /// Evaluation over a constrained region against an independent NumPy
    /// computation over a fine grid of the same region
    /// (`tests/data/design_evaluation_reference.py`).
    #[test]
    fn constrained_region_matches_numpy() {
        let f = ModelFormula::quadratic(2);
        let factors = [crate::Factor::coded("A"), crate::Factor::coded("B")];
        let region = Region::parse("A + B <= 1\nexclude A < -0.5 and B < -0.5", &factors).unwrap();
        let runs: Vec<Vec<f64>> = [
            [-1.0, 1.0],
            [-1.0, 0.0],
            [0.0, -1.0],
            [1.0, -1.0],
            [1.0, 0.0],
            [0.0, 1.0],
            [0.0, 0.0],
            [0.5, 0.5],
            [-0.5, -0.5],
        ]
        .map(|r| r.to_vec())
        .to_vec();
        let o = EvaluationOptions::default();
        let e = evaluate_design_in(&f, &runs, &region, &o).unwrap();
        // NumPy: mean d 0.59307, max d 2.2627315 (a vertex of the region).
        assert!(close(e.average_variance, 0.593_070_075, 2e-3), "I = {}", e.average_variance);
        assert!(close(e.max_variance, 2.262_731_481_481_480_5, 1e-6), "max {}", e.max_variance);
        assert!(region.contains(&e.max_at), "{:?}", e.max_at);
        assert!(close(e.g_efficiency, 100.0 * 6.0 / (9.0 * 2.262_731_481_481_480_5), 1e-4));
        assert_eq!(e.fds.sorted.len(), o.samples);
        assert!(e.region_points.is_some());
        let numpy = [
            (0.1, 0.406_609_025_655_269_6),
            (0.25, 0.481_446_882_909_925_5),
            (0.5, 0.549_772_108_522_537_3),
            (0.75, 0.676_561_333_890_897_9),
            (0.9, 0.825_201_559_152_769_6),
        ];
        for (fr, want) in numpy {
            let got = e.fds.quantile(fr);
            assert!(close(got, want, 0.01), "fraction {fr}: {got} vs {want}");
        }
        // Over the whole square the same runs do worse at the cut corner.
        let cube = evaluate_design(&f, &runs, &o).unwrap();
        assert!(cube.max_variance > e.max_variance + 1.0 && cube.region_points.is_none());
        assert!(cube.average_variance > e.average_variance);
        // A region with no room is an error, not a silent cube.
        let tiny = Region::parse("A + B <= -1.9999999", &factors).unwrap();
        assert!(evaluate_design_in(&f, &runs, &tiny, &o).is_err());
        assert!(evaluate_design_in(&f, &runs, &Region::cube(3), &o).is_err());
    }
}
