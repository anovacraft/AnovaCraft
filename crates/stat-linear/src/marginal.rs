//! Least-squares (estimated marginal) means, comparisons within the slices
//! of an interaction, and comparisons of slopes across groups, for any model
//! whose fixed effects follow a [`ModelDesign`].
//!
//! # Least-squares means
//! The least-squares mean of a combination of levels is the model's
//! prediction there, averaged with equal weight over the levels of every
//! other categorical predictor, with each continuous predictor at its mean
//! over the rows fitted (Searle, Speed & Milliken, "Population marginal
//! means in the linear model", *Am. Stat.* 34, 1980). It is a linear
//! function `ℓᵀβ` of the coefficients; [`marginal_row`] gives `ℓ` (with a
//! leading 1 for the intercept).
//!
//! # Slices of an interaction
//! For an interaction `A × B`, the slice of `B` at level `b` compares the
//! least-squares means of `A`'s levels with `B` fixed at `b`
//! (Milliken & Johnson, *Analysis of Messy Data*, vol. 1, 2nd ed., §10.4).
//! The test of `A` within the slice is the joint Wald test of the `k − 1`
//! differences from `A`'s first level, `F = (Lβ̂)ᵀ(L V Lᵀ)⁻¹(Lβ̂)/r` on
//! `r = rank(L V Lᵀ)` and the model's error degrees of freedom (a χ² test,
//! reported as `F` on `r` and ∞ df, when the model has no error df, as for
//! a GLM). Pairwise differences of `A`'s levels within the slice are
//! `t = (ℓ₁ − ℓ₂)ᵀβ̂ / SE` with `SE² = (ℓ₁ − ℓ₂)ᵀ V (ℓ₁ − ℓ₂)`, adjusted
//! within each slice by [`PairAdjust`].
//!
//! # Slopes
//! In a model with separate slopes of a continuous `x` for the levels of a
//! categorical `G` (the effect `G × x`), each level's slope is the
//! derivative of its least-squares mean with respect to `x` at `x̄`
//! (computed by a central difference, exact for terms up to `x²`), again
//! averaged over the other categorical predictors (Milliken & Johnson,
//! vol. 3, 2002, ch. 3). The test of equal slopes is the joint test of the
//! differences from the first level's slope; pairwise slope differences are
//! adjusted like the pairs of a slice.
//!
//! # Adjustments
//! Tukey (Tukey–Kramer for unequal precision): `p = P(Q_{k,ν} ≥ |t|√2)`
//! with `k` the number of means compared and `ν` the error df, intervals
//! `±q_{k,ν,1−α}/√2 · SE`; Bonferroni: `p·m` (capped at 1), intervals at
//! `1 − α/m`; Holm's step-down p-values with Bonferroni intervals (Holm's
//! procedure has no matching intervals); or none.

use crate::design::{ModelDesign, TermKind, TermValue};
use stat_core::distributions::{ChiSquared, ContinuousDistribution, FisherSnedecor, Normal, StudentsT};
use stat_core::eigen::symmetric_eigen;
use stat_core::{Matrix, Result, StatError, StudentizedRange};

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// Most level combinations averaged over for one least-squares mean.
const MAX_GRID: usize = 100_000;

/// How pairwise comparisons in one family are adjusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PairAdjust {
    /// Tukey's studentized range (Tukey–Kramer).
    Tukey,
    /// Holm's step-down Bonferroni.
    Holm,
    /// Bonferroni.
    Bonferroni,
    /// No adjustment.
    None,
}

impl PairAdjust {
    /// Every adjustment, in menu order.
    pub const ALL: [Self; 4] = [Self::Tukey, Self::Holm, Self::Bonferroni, Self::None];

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Tukey => "Tukey",
            Self::Holm => "Holm",
            Self::Bonferroni => "Bonferroni",
            Self::None => "None",
        }
    }

    /// Identifier in project files and scripts.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Tukey => "tukey",
            Self::Holm => "holm",
            Self::Bonferroni => "bonferroni",
            Self::None => "none",
        }
    }

    /// The adjustment with identifier `key`.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.key() == key)
    }
}

/// A linear function `ℓᵀβ` of the coefficients with its standard error.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContrastEstimate {
    /// `ℓᵀβ̂`.
    pub estimate: f64,
    /// `√(ℓᵀ V ℓ)`.
    pub std_error: f64,
    /// Degrees of freedom of its t statistic (`None`: normal).
    pub df: Option<f64>,
}

/// A joint test of several linear functions being zero.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct JointTest {
    /// Numerator degrees of freedom (rank of the hypothesis).
    pub num_df: usize,
    /// Denominator degrees of freedom (`None`: a χ² test, shown as F on ∞ df).
    pub den_df: Option<f64>,
    /// `F` (χ²/r when `den_df` is `None`).
    pub f: f64,
    /// p-value.
    pub p_value: f64,
}

/// What comparisons need from a fitted model: estimates and joint tests of
/// linear functions of its coefficients (intercept first).
pub trait ContrastInference {
    /// Number of coefficients, intercept included.
    fn n_coefficients(&self) -> usize;
    /// `ℓᵀβ̂` with its standard error and degrees of freedom.
    ///
    /// # Errors
    /// A row of the wrong length.
    fn estimate(&self, l: &[f64]) -> Result<ContrastEstimate>;
    /// Joint test of `Lβ = 0`.
    ///
    /// # Errors
    /// Rows of the wrong length, or no estimable row.
    fn joint(&self, l: &[Vec<f64>]) -> Result<JointTest>;
}

/// Wald inference from coefficients and their covariance, with `df` error
/// degrees of freedom (t and F tests) or none (normal and χ² tests).
#[derive(Debug, Clone, Copy)]
pub struct WaldInference<'a> {
    /// Coefficients, intercept first.
    pub beta: &'a [f64],
    /// Their covariance.
    pub covariance: &'a Matrix,
    /// Error degrees of freedom (`None`: large-sample tests).
    pub df: Option<f64>,
}

impl WaldInference<'_> {
    fn quadratic(&self, a: &[f64], b: &[f64]) -> f64 {
        let p = self.beta.len();
        (0..p).map(|i| a[i] * (0..p).map(|j| self.covariance[(i, j)] * b[j]).sum::<f64>()).sum()
    }

    fn check(&self, l: &[f64]) -> Result<()> {
        if l.len() == self.beta.len() {
            Ok(())
        } else {
            Err(StatError::DimensionMismatch { context: "contrast row", expected: self.beta.len(), found: l.len() })
        }
    }
}

impl ContrastInference for WaldInference<'_> {
    fn n_coefficients(&self) -> usize {
        self.beta.len()
    }

    fn estimate(&self, l: &[f64]) -> Result<ContrastEstimate> {
        self.check(l)?;
        let estimate = l.iter().zip(self.beta).map(|(a, b)| a * b).sum();
        Ok(ContrastEstimate { estimate, std_error: self.quadratic(l, l).max(0.0).sqrt(), df: self.df })
    }

    fn joint(&self, l: &[Vec<f64>]) -> Result<JointTest> {
        for r in l {
            self.check(r)?;
        }
        let m: Vec<Vec<f64>> = l.iter().map(|a| l.iter().map(|b| self.quadratic(a, b)).collect()).collect();
        let lb: Vec<f64> = l.iter().map(|r| r.iter().zip(self.beta).map(|(a, b)| a * b).sum()).collect();
        wald_from(&m, &lb, self.df)
    }
}

/// Joint Wald test from `M = L V Lᵀ` and `Lβ̂`, through the eigenvalues of
/// `M` (dropping directions with no variance, so dependent rows count once).
///
/// # Errors
/// No estimable row.
pub fn wald_from(m: &[Vec<f64>], lb: &[f64], df: Option<f64>) -> Result<JointTest> {
    let (vals, vecs) = symmetric_eigen(m)?;
    let top = vals.iter().copied().fold(0.0, f64::max);
    let mut chi2 = 0.0;
    let mut r = 0;
    for (k, &v) in vals.iter().enumerate() {
        if v > 1e-10 * top.max(f64::MIN_POSITIVE) {
            let proj: f64 = vecs[k].iter().zip(lb).map(|(a, b)| a * b).sum();
            chi2 += proj * proj / v;
            r += 1;
        }
    }
    if r == 0 {
        return Err(invalid("the hypothesis has no estimable rows"));
    }
    let f = chi2 / r as f64;
    let p_value = match df {
        Some(d) => FisherSnedecor::new(r as f64, d)?.sf(f)?,
        None => ChiSquared::new(r as f64)?.sf(chi2)?,
    };
    Ok(JointTest { num_df: r, den_df: df, f, p_value })
}

/// The row `ℓ` (intercept first) of the least-squares mean with the
/// predictors in `fixed` set as given: other categorical predictors averaged
/// with equal weight over their levels, other continuous ones at their mean.
///
/// # Errors
/// An unknown predictor or level, or more than 100,000 level combinations
/// to average over.
pub fn marginal_row(design: &ModelDesign, fixed: &[(usize, TermValue)]) -> Result<Vec<f64>> {
    let terms = design.terms();
    if let Some(&(i, _)) = fixed.iter().find(|(i, _)| *i >= terms.len()) {
        return Err(invalid(format!("no predictor {i}")));
    }
    // Each predictor's settings: the fixed value, its mean, or every level.
    let settings: Vec<Vec<TermValue>> = terms
        .iter()
        .enumerate()
        .map(|(i, t)| match fixed.iter().find(|(j, _)| *j == i) {
            Some(&(_, v)) => vec![v],
            None => match &t.kind {
                TermKind::Continuous => vec![TermValue::Number(design.centers()[i])],
                TermKind::Categorical { levels } => (0..levels.len()).map(TermValue::Level).collect(),
            },
        })
        .collect();
    let combos: usize = settings.iter().map(Vec::len).product();
    if combos > MAX_GRID {
        return Err(invalid(format!("averaging over {combos} level combinations is too many")));
    }
    let mut sum = vec![0.0; design.column_names().len()];
    let mut at = vec![0usize; settings.len()];
    for _ in 0..combos {
        let point: Vec<TermValue> = settings.iter().zip(&at).map(|(s, &k)| s[k]).collect();
        for (s, v) in sum.iter_mut().zip(design.encode(&point)?) {
            *s += v;
        }
        for k in (0..at.len()).rev() {
            at[k] += 1;
            if at[k] < settings[k].len() {
                break;
            }
            at[k] = 0;
        }
    }
    Ok(std::iter::once(1.0).chain(sum.into_iter().map(|v| v / combos as f64)).collect())
}

/// One least-squares mean (or slope).
#[derive(Debug, Clone, PartialEq)]
pub struct MarginalEstimate {
    /// Level of the compared factor.
    pub level: String,
    /// Estimate.
    pub estimate: f64,
    /// Standard error.
    pub std_error: f64,
    /// Degrees of freedom (`None`: normal).
    pub df: Option<f64>,
    /// Confidence interval at the requested level (unadjusted).
    pub interval: (f64, f64),
    /// t (or z) of the estimate against zero.
    pub statistic: f64,
    /// Its two-sided p-value.
    pub p_value: f64,
}

/// One pairwise difference within a family.
#[derive(Debug, Clone, PartialEq)]
pub struct PairDifference {
    /// First level.
    pub first: String,
    /// Second level.
    pub second: String,
    /// First minus second.
    pub estimate: f64,
    /// Standard error.
    pub std_error: f64,
    /// Degrees of freedom (`None`: normal).
    pub df: Option<f64>,
    /// t (or z).
    pub statistic: f64,
    /// Adjusted two-sided p-value.
    pub p_value: f64,
    /// Adjusted confidence interval.
    pub interval: (f64, f64),
}

/// The comparisons of one factor's levels within one slice.
#[derive(Debug, Clone, PartialEq)]
pub struct Slice {
    /// Factor compared.
    pub factor: String,
    /// Slicing factor.
    pub by: String,
    /// Level of the slicing factor.
    pub at: String,
    /// Least-squares means of the compared factor's levels.
    pub means: Vec<MarginalEstimate>,
    /// Joint test of equal means within the slice.
    pub test: JointTest,
    /// Pairwise differences (empty when not requested).
    pub pairs: Vec<PairDifference>,
}

fn level_names(design: &ModelDesign, i: usize) -> Result<Vec<String>> {
    match design.terms().get(i).map(|t| &t.kind) {
        Some(TermKind::Categorical { levels }) => Ok(levels.clone()),
        Some(TermKind::Continuous) => Err(invalid(format!("{:?} is not categorical", design.terms()[i].name))),
        None => Err(invalid(format!("no predictor {i}"))),
    }
}

fn critical(df: Option<f64>, upper: f64) -> Result<f64> {
    match df {
        Some(d) => StudentsT::new(d)?.quantile(upper),
        None => Normal::standard().quantile(upper),
    }
}

fn two_sided(df: Option<f64>, t: f64) -> Result<f64> {
    Ok(2.0
        * match df {
            Some(d) => StudentsT::new(d)?.sf(t.abs())?,
            None => Normal::standard().sf(t.abs())?,
        })
}

fn estimate_of(
    inference: &dyn ContrastInference,
    level: String,
    row: &[f64],
    confidence: f64,
) -> Result<MarginalEstimate> {
    let e = inference.estimate(row)?;
    let c = critical(e.df, 0.5 + confidence / 2.0)?;
    let statistic = e.estimate / e.std_error;
    Ok(MarginalEstimate {
        level,
        estimate: e.estimate,
        std_error: e.std_error,
        df: e.df,
        interval: (e.estimate - c * e.std_error, e.estimate + c * e.std_error),
        statistic,
        p_value: two_sided(e.df, statistic)?,
    })
}

/// Joint test that every row's function equals the first row's.
fn equality_test(inference: &dyn ContrastInference, rows: &[Vec<f64>]) -> Result<JointTest> {
    let l: Vec<Vec<f64>> = rows[1..].iter().map(|r| r.iter().zip(&rows[0]).map(|(a, b)| a - b).collect()).collect();
    inference.joint(&l)
}

/// Every pair of `rows` (labelled by `levels`), adjusted as one family.
///
/// # Errors
/// From the inference or the distributions.
pub fn pairwise(
    inference: &dyn ContrastInference,
    levels: &[String],
    rows: &[Vec<f64>],
    adjust: PairAdjust,
    confidence: f64,
) -> Result<Vec<PairDifference>> {
    let k = rows.len();
    let m = k * (k - 1) / 2;
    let mut out = Vec::with_capacity(m);
    for a in 0..k {
        for b in a + 1..k {
            let l: Vec<f64> = rows[a].iter().zip(&rows[b]).map(|(x, y)| x - y).collect();
            let e = inference.estimate(&l)?;
            let statistic = e.estimate / e.std_error;
            out.push(PairDifference {
                first: levels[a].clone(),
                second: levels[b].clone(),
                estimate: e.estimate,
                std_error: e.std_error,
                df: e.df,
                statistic,
                p_value: two_sided(e.df, statistic)?,
                interval: (f64::NAN, f64::NAN),
            });
        }
    }
    let alpha = 1.0 - confidence;
    for p in &mut out {
        let half = match adjust {
            PairAdjust::Tukey if k > 2 => {
                let q = StudentizedRange::new(k, p.df.unwrap_or(f64::INFINITY))?;
                p.p_value = q.sf(p.statistic.abs() * std::f64::consts::SQRT_2)?.clamp(0.0, 1.0);
                q.quantile(confidence)? / std::f64::consts::SQRT_2
            }
            PairAdjust::Tukey | PairAdjust::None => critical(p.df, 1.0 - alpha / 2.0)?,
            PairAdjust::Bonferroni | PairAdjust::Holm => {
                if adjust == PairAdjust::Bonferroni {
                    p.p_value = (p.p_value * m as f64).min(1.0);
                }
                critical(p.df, 1.0 - alpha / (2.0 * m as f64))?
            }
        };
        p.interval = (p.estimate - half * p.std_error, p.estimate + half * p.std_error);
    }
    if adjust == PairAdjust::Holm {
        let raw: Vec<f64> = out.iter().map(|p| p.p_value).collect();
        for (p, h) in out.iter_mut().zip(holm(&raw)) {
            p.p_value = h;
        }
    }
    Ok(out)
}

/// Holm's step-down adjusted p-values.
fn holm(p: &[f64]) -> Vec<f64> {
    let m = p.len();
    let mut order: Vec<usize> = (0..m).collect();
    order.sort_by(|&a, &b| p[a].total_cmp(&p[b]));
    let mut out = vec![0.0; m];
    let mut running: f64 = 0.0;
    for (rank, &i) in order.iter().enumerate() {
        running = running.max(((m - rank) as f64 * p[i]).min(1.0));
        out[i] = running;
    }
    out
}

/// Comparisons of factor `a`'s levels within each level of factor `b`
/// (both categorical predictors of `design`), with pairwise differences
/// adjusted by `adjust` within each slice (`None` for no pairs).
///
/// # Errors
/// A predictor that is not categorical, `a == b`, or a failure of the
/// inference.
pub fn slices(
    design: &ModelDesign,
    inference: &dyn ContrastInference,
    a: usize,
    b: usize,
    adjust: Option<PairAdjust>,
    confidence: f64,
) -> Result<Vec<Slice>> {
    if a == b {
        return Err(invalid("a factor cannot be sliced by itself"));
    }
    let (la, lb) = (level_names(design, a)?, level_names(design, b)?);
    let names = (design.terms()[a].name.clone(), design.terms()[b].name.clone());
    let mut out = Vec::with_capacity(lb.len());
    for (j, at) in lb.iter().enumerate() {
        let rows: Vec<Vec<f64>> = (0..la.len())
            .map(|i| marginal_row(design, &[(a, TermValue::Level(i)), (b, TermValue::Level(j))]))
            .collect::<Result<_>>()?;
        let means = la
            .iter()
            .zip(&rows)
            .map(|(l, r)| estimate_of(inference, l.clone(), r, confidence))
            .collect::<Result<_>>()?;
        let test = equality_test(inference, &rows)?;
        let pairs = match adjust {
            Some(adj) => pairwise(inference, &la, &rows, adj, confidence)?,
            None => Vec::new(),
        };
        out.push(Slice { factor: names.0.clone(), by: names.1.clone(), at: at.clone(), means, test, pairs });
    }
    Ok(out)
}

/// Separate slopes of a continuous predictor across a factor's levels.
#[derive(Debug, Clone, PartialEq)]
pub struct SlopeComparison {
    /// Factor.
    pub group: String,
    /// Continuous predictor.
    pub x: String,
    /// Each level's slope.
    pub slopes: Vec<MarginalEstimate>,
    /// Joint test of equal slopes.
    pub equal_slopes: JointTest,
    /// Pairwise differences of slopes.
    pub pairs: Vec<PairDifference>,
}

/// Slopes of continuous predictor `x` for each level of factor `group` at
/// the mean of `x`, with the test of equal slopes and pairwise differences
/// adjusted by `adjust`.
///
/// # Errors
/// `group` not categorical, `x` not continuous, or a failure of the
/// inference.
pub fn group_slopes(
    design: &ModelDesign,
    inference: &dyn ContrastInference,
    group: usize,
    x: usize,
    adjust: PairAdjust,
    confidence: f64,
) -> Result<SlopeComparison> {
    let levels = level_names(design, group)?;
    match design.terms().get(x).map(|t| &t.kind) {
        Some(TermKind::Continuous) => {}
        _ => return Err(invalid("the slope variable must be a continuous predictor")),
    }
    let c = design.centers()[x];
    let h = 1e-3 * c.abs().max(1.0);
    let rows: Vec<Vec<f64>> = (0..levels.len())
        .map(|i| {
            let up = marginal_row(design, &[(group, TermValue::Level(i)), (x, TermValue::Number(c + h))])?;
            let down = marginal_row(design, &[(group, TermValue::Level(i)), (x, TermValue::Number(c - h))])?;
            Ok(up.iter().zip(&down).map(|(u, d)| (u - d) / (2.0 * h)).collect())
        })
        .collect::<Result<_>>()?;
    let slopes = levels
        .iter()
        .zip(&rows)
        .map(|(l, r)| estimate_of(inference, l.clone(), r, confidence))
        .collect::<Result<_>>()?;
    Ok(SlopeComparison {
        group: design.terms()[group].name.clone(),
        x: design.terms()[x].name.clone(),
        slopes,
        equal_slopes: equality_test(inference, &rows)?,
        pairs: pairwise(inference, &levels, &rows, adjust, confidence)?,
    })
}

/// Pairs `(categorical, continuous)` of predictors in an effect `G × x` of
/// `design`, so separate slopes of `x` across `G` can be compared.
#[must_use]
pub fn slope_pairs(design: &ModelDesign) -> Vec<(usize, usize)> {
    let terms = design.terms();
    let mut out = Vec::new();
    for e in design.effects() {
        if let [(i, 1), (j, 1)] = e.factors.as_slice() {
            let (i, j) = (*i, *j);
            match (&terms[i].kind, &terms[j].kind) {
                (TermKind::Categorical { .. }, TermKind::Continuous) => out.push((i, j)),
                (TermKind::Continuous, TermKind::Categorical { .. }) => out.push((j, i)),
                _ => {}
            }
        }
    }
    out
}

/// Pairs `(a, b)` of categorical predictors in an effect `A × B` of
/// `design` (two-factor interactions only).
#[must_use]
pub fn interaction_pairs(design: &ModelDesign) -> Vec<(usize, usize)> {
    let terms = design.terms();
    design
        .effects()
        .iter()
        .filter_map(|e| match e.factors.as_slice() {
            [(i, 1), (j, 1)]
                if matches!(terms[*i].kind, TermKind::Categorical { .. })
                    && matches!(terms[*j].kind, TermKind::Categorical { .. }) =>
            {
                Some((*i, *j))
            }
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn holm_is_monotone() {
        let h = holm(&[0.01, 0.04, 0.03, 0.5]);
        assert_eq!(h, vec![0.04, 0.09, 0.09, 0.5]);
    }

    #[test]
    fn adjust_keys_round_trip() {
        for a in PairAdjust::ALL {
            assert_eq!(PairAdjust::from_key(a.key()), Some(a));
        }
    }
}
