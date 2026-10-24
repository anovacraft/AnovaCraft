//! Multivariate adaptive regression splines (MARS).
//!
//! **Model** (Friedman, "Multivariate adaptive regression splines",
//! *Annals of Statistics* 19(1), 1991). The prediction is a linear
//! combination `ŷ = b₀ + Σ_m b_m B_m(x)` of basis functions, each a product
//! of factors on distinct predictors:
//!
//! * a *hinge* `max(0, x − t)` (right) or `max(0, t − x)` (left) with knot
//!   `t` at an observed value of a continuous predictor;
//! * a *linear* factor `x`, for predictors declared to enter linearly;
//! * a *level indicator* `1{x = level}` of a categorical predictor. A
//!   predictor with `L` levels gives `L − 1` indicators (every level but
//!   the first; the first is the reference, like treatment contrasts), each
//!   treated as a candidate variable of its own. Friedman's (1991, §3.7)
//!   alternative, factors on level *subsets*, is not used: indicators span
//!   the same functions once enough of them enter, and keep the formula
//!   readable.
//!
//! **Forward pass** (Friedman's Algorithm 2). Starting from the intercept,
//! each step tries every *parent* term `B_p` of degree below the maximum
//! interaction degree, every variable not already in it, and every allowed
//! knot `t`, and adds the reflected pair `B_p · max(0, x − t)`,
//! `B_p · max(0, t − x)` (or the single term `B_p · x` or
//! `B_p · 1{x = level}`) that most reduces the residual sum of squares.
//! Since `max(0, x − t) − max(0, t − x) = x − t`, the pair spans the same
//! space as `B_p · x` and the right hinge, so the reduction for every knot
//! follows from running sums over the parent's cases sorted by `x`
//! (Friedman's fast update, eq. 52): `O(n·M)` per parent and variable. A
//! new term already in the span of the model (for example the left hinge
//! when the predictor has a pair at another knot) is dropped, so a step
//! may add one term. The pass stops when
//! (i) another step would exceed the maximum number of terms (each pair
//! step counts two, even when one of the pair is dropped; a single-term
//! step counts one), (ii) the best step raises R² by less than the
//! threshold (default 0.001), (iii) R² reaches 1 − threshold, (iv) the
//! generalized R², `1 − GCV/GCV(intercept only)`, falls below −10, or (v)
//! no step reduces the residual sum of squares.
//!
//! **Knots.** For a parent with `N_m` nonzero cases among `p` candidate
//! variables, knots are placed at the sorted values leaving at least
//! `L_e = ⌊3 − log₂(α/p)⌋` cases at each end (end span, eq. 45) and
//! every `L = ⌊−log₂(−ln(1 − α)/(p N_m)) / 2.5⌋` cases in between
//! (minimum span, eq. 43), with `α = 0.05`; both at least 1. Either can be
//! set instead.
//!
//! **Backward pass.** From the forward model, terms are deleted one at a
//! time, each time the one whose removal increases the residual sum of
//! squares least (the intercept stays), giving the best subset of each
//! size. The size is chosen by the smallest generalized cross-validation
//! criterion (Friedman eq. 30, after Craven & Wahba)
//! `GCV = RSS / (n (1 − C/n)²)`, `C = k + d (k − 1)/2` for `k` terms, so
//! `d` (default 2 for additive models, 3 with interactions) is charged per
//! knot (a pair of terms); `GCV = ∞` once `C ≥ n`. Alternatively the size
//! is chosen on held-out (validation) rows ([`MarsFit::validation_errors`],
//! [`MarsFit::select`]).
//!
//! **Variable importance** over the subsets of size 2 up to the selected
//! size: the number of subsets that use a predictor, and the sums of the
//! decreases in GCV and in RSS from the subset one term smaller over the
//! subsets that use it.
//!
//! Least squares uses a modified Gram–Schmidt basis with
//! re-orthogonalization (`X = QR` on the fitting rows), so any subset's
//! fit is a small `M × k` problem in `R` and `Qᵀy`.

use stat_core::matrix::Matrix;
use stat_core::qr::QrDecomposition;
use stat_core::{Result, StatError};

use crate::tree::{Feature, Value};

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// A column is redundant when its part orthogonal to the model has less
/// than this share of its squared length.
const REDUNDANT: f64 = 1e-10;

/// Significance level `α` of Friedman's span rules.
const SPAN_ALPHA: f64 = 0.05;

/// Which side of the knot a hinge is nonzero on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// `max(0, x − t)`.
    Right,
    /// `max(0, t − x)`.
    Left,
}

/// One factor of a basis function.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Factor {
    /// A hinge on continuous predictor `feature` with knot `knot`.
    Hinge {
        /// Predictor index.
        feature: usize,
        /// Knot `t`.
        knot: f64,
        /// Side of the knot.
        direction: Direction,
    },
    /// The value of continuous predictor `feature`.
    Linear {
        /// Predictor index.
        feature: usize,
    },
    /// `1` when categorical predictor `feature` is at `level`, else `0`.
    Level {
        /// Predictor index.
        feature: usize,
        /// Level index.
        level: usize,
    },
}

impl Factor {
    /// Predictor the factor reads.
    #[must_use]
    pub fn feature(&self) -> usize {
        match *self {
            Self::Hinge { feature, .. } | Self::Linear { feature } | Self::Level { feature, .. } => feature,
        }
    }

    /// The factor's value (NaN when the predictor is missing or the wrong
    /// kind).
    #[must_use]
    pub fn value(&self, v: Value) -> f64 {
        match (*self, v) {
            (Self::Hinge { knot, direction, .. }, Value::Number(x)) => match direction {
                Direction::Right => (x - knot).max(0.0),
                Direction::Left => (knot - x).max(0.0),
            },
            (Self::Linear { .. }, Value::Number(x)) => x,
            (Self::Level { level, .. }, Value::Level(l)) => f64::from(u8::from(l == level)),
            _ => f64::NAN,
        }
    }
}

/// A basis function: the product of its factors (the intercept has none).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BasisFunction {
    /// Factors, on distinct predictors.
    pub factors: Vec<Factor>,
}

impl BasisFunction {
    /// Interaction degree (number of factors).
    #[must_use]
    pub fn degree(&self) -> usize {
        self.factors.len()
    }

    /// `true` when a factor reads predictor `feature`.
    #[must_use]
    pub fn uses(&self, feature: usize) -> bool {
        self.factors.iter().any(|f| f.feature() == feature)
    }

    /// Value for a row whose predictor `j` has value `x(j)` (NaN when a
    /// predictor it reads is missing).
    #[must_use]
    pub fn value(&self, x: &dyn Fn(usize) -> Value) -> f64 {
        self.factors.iter().map(|f| f.value(x(f.feature()))).product()
    }
}

/// Settings of [`fit_mars`].
#[derive(Debug, Clone, PartialEq)]
pub struct MarsOptions {
    /// Greatest interaction degree (1: additive).
    pub max_degree: usize,
    /// Greatest number of forward-pass terms including the intercept (0:
    /// `min(200, max(20, 2p)) + 1` for `p` candidate variables).
    pub max_terms: usize,
    /// GCV cost per knot `d` (`None`: 2 when additive, else 3).
    pub penalty: Option<f64>,
    /// Forward-pass threshold on the gain in R².
    pub threshold: f64,
    /// Cases between knots (0: Friedman's rule).
    pub min_span: usize,
    /// Cases before the first and after the last knot (0: Friedman's rule).
    pub end_span: usize,
}

impl Default for MarsOptions {
    fn default() -> Self {
        Self { max_degree: 1, max_terms: 0, penalty: None, threshold: 0.001, min_span: 0, end_span: 0 }
    }
}

impl MarsOptions {
    /// The penalty `d` in effect.
    #[must_use]
    pub fn effective_penalty(&self) -> f64 {
        self.penalty.unwrap_or(if self.max_degree > 1 { 3.0 } else { 2.0 })
    }
}

/// Why the forward pass stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    /// Another step would exceed the maximum number of terms.
    MaxTerms,
    /// The best step raised R² by less than the threshold.
    SmallGain,
    /// R² reached 1 − threshold.
    PerfectFit,
    /// The generalized R² fell below −10 (too many terms for the rows).
    GcvLimit,
    /// No step reduced the residual sum of squares.
    NoImprovement,
}

impl StopReason {
    /// Description for reports.
    #[must_use]
    pub fn describe(self) -> &'static str {
        match self {
            Self::MaxTerms => "reached the maximum number of terms",
            Self::SmallGain => "the next step raised R² by less than the threshold",
            Self::PerfectFit => "R² reached 1 − threshold",
            Self::GcvLimit => "the generalized R² fell below −10",
            Self::NoImprovement => "no step reduced the residual sum of squares",
        }
    }
}

/// One step of the forward pass.
#[derive(Debug, Clone, PartialEq)]
pub struct ForwardStep {
    /// Parent term index.
    pub parent: usize,
    /// Predictor added.
    pub feature: usize,
    /// Knot (hinge steps).
    pub knot: Option<f64>,
    /// Terms added (one or two indices into [`MarsFit::basis`]).
    pub added: Vec<usize>,
    /// R² after the step.
    pub r_squared: f64,
}

/// The best subset of one size found by the backward pass.
#[derive(Debug, Clone, PartialEq)]
pub struct Subset {
    /// Term indices (always including the intercept, 0), ascending.
    pub terms: Vec<usize>,
    /// Residual sum of squares on the fitting rows.
    pub rss: f64,
    /// Generalized cross-validation criterion.
    pub gcv: f64,
}

/// A fitted MARS model.
#[derive(Debug, Clone, PartialEq)]
pub struct MarsFit {
    /// Forward-pass terms (index 0 is the intercept).
    pub basis: Vec<BasisFunction>,
    /// Forward-pass steps.
    pub forward: Vec<ForwardStep>,
    /// Why the forward pass stopped.
    pub stop: StopReason,
    /// `subsets[k − 1]`: the best subset of `k` terms.
    pub subsets: Vec<Subset>,
    /// Terms of the selected model (ascending).
    pub selected: Vec<usize>,
    /// Coefficients of the selected terms.
    pub coefficients: Vec<f64>,
    /// Fitting rows used (those with the response and every predictor).
    pub n: usize,
    /// Total sum of squares about the mean on the fitting rows.
    pub tss: f64,
    /// GCV penalty per knot.
    pub penalty: f64,
    /// Minimum span used for the intercept parent (cases).
    pub min_span: usize,
    /// End span used (cases).
    pub end_span: usize,
    /// Number of candidate variables (continuous predictors plus level
    /// indicators).
    pub variables: usize,
    /// Upper-triangular `R` of `X = QR` on the fitting rows (row-major, `M × M`).
    r: Vec<Vec<f64>>,
    /// `Qᵀy`.
    z: Vec<f64>,
}

/// The generalized cross-validation criterion of `k` terms with residual
/// sum of squares `rss` on `n` rows and penalty `d` per knot.
#[must_use]
pub fn gcv(rss: f64, n: usize, k: usize, d: f64) -> f64 {
    let n = n as f64;
    let c = k as f64 + d * (k as f64 - 1.0) / 2.0;
    if c >= n { f64::INFINITY } else { rss / (n * (1.0 - c / n).powi(2)) }
}

/// Friedman's minimum span (eq. 43) for `p` variables and `nm` cases.
#[must_use]
pub fn friedman_min_span(p: usize, nm: usize) -> usize {
    let v = -(-(1.0 - SPAN_ALPHA).ln() / (p.max(1) * nm.max(1)) as f64).log2() / 2.5;
    (v.floor() as usize).max(1)
}

/// Friedman's end span (eq. 45) for `p` variables.
#[must_use]
pub fn friedman_end_span(p: usize) -> usize {
    ((3.0 - (SPAN_ALPHA / p.max(1) as f64).log2()).floor() as usize).max(1)
}

/// A candidate variable of the forward pass.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Variable {
    /// Continuous predictor entering through hinges.
    Hinged(usize),
    /// Continuous predictor entering linearly.
    Linear(usize),
    /// A level indicator of a categorical predictor.
    Level(usize, usize),
}

impl Variable {
    fn feature(self) -> usize {
        match self {
            Self::Hinged(f) | Self::Linear(f) | Self::Level(f, _) => f,
        }
    }
}

/// The working state of the forward pass on the fitting rows.
struct Forward<'a> {
    /// Predictor values on the fitting rows (`NaN` not used: rows complete).
    x: &'a [Vec<f64>],
    /// Row order of each continuous predictor (ascending), else empty.
    order: &'a [Vec<usize>],
    /// Basis columns.
    columns: Vec<Vec<f64>>,
    /// Orthonormal columns.
    q: Vec<Vec<f64>>,
    /// `R` columns: `r[j][k] = q_k · column_j` for `k ≤ j`.
    r: Vec<Vec<f64>>,
    /// Residual `y − Q Qᵀ y`.
    resid: Vec<f64>,
    /// `Qᵀ y`.
    z: Vec<f64>,
    y: &'a [f64],
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

impl Forward<'_> {
    /// Part of `c` orthogonal to `Q` (two passes of Gram–Schmidt), with the
    /// projections.
    fn orthogonalize(&self, c: &[f64]) -> (Vec<f64>, Vec<f64>) {
        let mut v = c.to_vec();
        let mut coef = vec![0.0; self.q.len()];
        for _ in 0..2 {
            for (k, q) in self.q.iter().enumerate() {
                let a = dot(q, &v);
                coef[k] += a;
                v.iter_mut().zip(q).for_each(|(vi, qi)| *vi -= a * qi);
            }
        }
        (v, coef)
    }

    /// Add column `c` unless it is redundant; returns `true` when added.
    fn add(&mut self, c: Vec<f64>) -> bool {
        let norm2 = dot(&c, &c);
        if norm2 == 0.0 || !norm2.is_finite() {
            return false;
        }
        let (mut v, mut coef) = self.orthogonalize(&c);
        let rest = dot(&v, &v);
        if rest <= REDUNDANT * norm2 {
            return false;
        }
        let len = rest.sqrt();
        v.iter_mut().for_each(|vi| *vi /= len);
        coef.push(len);
        let a = dot(&v, &self.resid);
        self.resid.iter_mut().zip(&v).for_each(|(ri, vi)| *ri -= a * vi);
        self.z.push(dot(&v, self.y));
        self.q.push(v);
        self.r.push(coef);
        self.columns.push(c);
        true
    }

    /// Reduction in RSS from adding the single column `c`.
    fn single_gain(&self, c: &[f64]) -> f64 {
        let norm2 = dot(c, c);
        if norm2 == 0.0 {
            return 0.0;
        }
        let (v, _) = self.orthogonalize(c);
        let rest = dot(&v, &v);
        if rest <= REDUNDANT * norm2 {
            return 0.0;
        }
        dot(&v, &self.resid).powi(2) / rest
    }

    /// Best knot for the hinge pair on `parent` × predictor `f`: the knot,
    /// and the reduction in RSS.
    fn best_knot(
        &self,
        parent: usize,
        f: usize,
        min_span: &dyn Fn(usize) -> usize,
        end_span: usize,
    ) -> Option<(f64, f64)> {
        let bp = &self.columns[parent];
        let x = &self.x[f];
        let idx: Vec<usize> = self.order[f].iter().copied().filter(|&i| bp[i] != 0.0).collect();
        let nm = idx.len();
        if nm == 0 {
            return None;
        }
        let span = min_span(nm);
        if nm < 2 * end_span + 1 {
            return None;
        }
        // The linear part B_p · x, orthogonalized once.
        let lin: Vec<f64> = bp.iter().zip(x).map(|(b, xi)| if *b == 0.0 { 0.0 } else { b * xi }).collect();
        let lin_norm2 = dot(&lin, &lin);
        let (mut l, _) = self.orthogonalize(&lin);
        let l_rest = dot(&l, &l);
        let has_linear = lin_norm2 > 0.0 && l_rest > REDUNDANT * lin_norm2;
        let a = if has_linear {
            let len = l_rest.sqrt();
            l.iter_mut().for_each(|v| *v /= len);
            dot(&l, &self.resid)
        } else {
            0.0
        };
        // Running sums over the cases above the knot, x centred at c.
        let c = idx.iter().map(|&i| x[i]).sum::<f64>() / nm as f64;
        let m = self.q.len();
        let (mut sww, mut swwx, mut swwxx) = (0.0, 0.0, 0.0);
        let (mut rw, mut rwx, mut lw, mut lwx) = (0.0, 0.0, 0.0, 0.0);
        let mut qw = vec![0.0; m];
        let mut qwx = vec![0.0; m];
        let mut best: Option<(f64, f64)> = None;
        let mut last_knot = f64::NAN;
        let (lo, hi) = (end_span, nm - 1 - end_span);
        let mut pos = nm;
        // Candidate positions lo, lo + span, …, up to hi; scanned downwards.
        let top = lo + (hi - lo) / span * span;
        let mut cand = top as isize;
        while pos > 0 {
            pos -= 1;
            if cand >= lo as isize && pos == cand as usize {
                cand -= span as isize;
                let t = x[idx[pos]];
                if t != last_knot {
                    last_knot = t;
                    let tc = t - c;
                    let hh_raw = swwxx - 2.0 * tc * swwx + tc * tc * sww;
                    if hh_raw > 0.0 {
                        let mut hh = hh_raw;
                        for k in 0..m {
                            let qh = qwx[k] - tc * qw[k];
                            hh -= qh * qh;
                        }
                        let lh = if has_linear { lwx - tc * lw } else { 0.0 };
                        hh -= lh * lh;
                        let rh = (rwx - tc * rw) - a * lh;
                        let gain = if hh > REDUNDANT * hh_raw { a * a + rh * rh / hh } else { a * a };
                        if best.is_none_or(|(_, g)| gain > g) {
                            best = Some((t, gain));
                        }
                    } else if best.is_none_or(|(_, g)| a * a > g) {
                        best = Some((t, a * a));
                    }
                }
            }
            // Add case `pos` to the sums (cases strictly above the next knot;
            // tied values contribute zero).
            let i = idx[pos];
            let w = bp[i];
            let xc = x[i] - c;
            let ww = w * w;
            sww += ww;
            swwx += ww * xc;
            swwxx += ww * xc * xc;
            rw += self.resid[i] * w;
            rwx += self.resid[i] * w * xc;
            if has_linear {
                lw += l[i] * w;
                lwx += l[i] * w * xc;
            }
            for k in 0..m {
                let qk = self.q[k][i] * w;
                qw[k] += qk;
                qwx[k] += qk * xc;
            }
        }
        best
    }
}

/// Fit a MARS model of `y` on `features` over `rows` (rows missing the
/// response or any predictor are skipped).
///
/// `linear[j]` makes continuous predictor `j` enter only linearly (`x`, no
/// knots); it is ignored for categorical predictors.
///
/// # Errors
/// No predictors, mismatched lengths, a degree of 0, a negative or
/// non-finite penalty, a threshold outside [0, 1), fewer than three
/// complete rows, or a constant response.
pub fn fit_mars(
    features: &[Feature],
    linear: &[bool],
    y: &[f64],
    rows: &[usize],
    options: &MarsOptions,
) -> Result<MarsFit> {
    if features.is_empty() {
        return Err(invalid("adaptive regression splines need at least one predictor"));
    }
    if linear.len() != features.len() {
        return Err(StatError::DimensionMismatch {
            context: "linear flags vs. predictors",
            expected: features.len(),
            found: linear.len(),
        });
    }
    if options.max_degree == 0 {
        return Err(invalid("the interaction degree must be at least 1"));
    }
    let d = options.effective_penalty();
    if !d.is_finite() || d < 0.0 {
        return Err(invalid("the GCV penalty must be zero or positive"));
    }
    if !(0.0..1.0).contains(&options.threshold) {
        return Err(invalid("the forward-pass threshold must be in [0, 1)"));
    }
    let complete: Vec<usize> = rows
        .iter()
        .copied()
        .filter(|&r| y.get(r).is_some_and(|v| v.is_finite()) && features.iter().all(|f| f.value(r) != Value::Missing))
        .collect();
    let n = complete.len();
    if n < 3 {
        return Err(StatError::InsufficientData { required: 3, found: n });
    }
    // Candidate variables and the predictor values on the fitting rows.
    let mut variables = Vec::new();
    let mut x = Vec::with_capacity(features.len());
    let mut order = Vec::with_capacity(features.len());
    for (j, f) in features.iter().enumerate() {
        match f {
            Feature::Numeric(v) => {
                let vals: Vec<f64> = complete.iter().map(|&r| v[r]).collect();
                let mut o: Vec<usize> = (0..n).collect();
                o.sort_by(|&a, &b| vals[a].total_cmp(&vals[b]));
                variables.push(if linear[j] { Variable::Linear(j) } else { Variable::Hinged(j) });
                x.push(vals);
                order.push(o);
            }
            Feature::Categorical { codes, levels } => {
                let vals: Vec<f64> = complete.iter().map(|&r| codes[r].map_or(f64::NAN, |c| c as f64)).collect();
                let mut seen = vec![false; *levels];
                for v in &vals {
                    seen[*v as usize] = true;
                }
                // Every level present but the first is an indicator.
                let present: Vec<usize> = (0..*levels).filter(|&l| seen[l]).collect();
                for &l in present.iter().skip(1) {
                    variables.push(Variable::Level(j, l));
                }
                x.push(vals);
                order.push(Vec::new());
            }
        }
    }
    let p = variables.len();
    let yv: Vec<f64> = complete.iter().map(|&r| y[r]).collect();
    let mean = yv.iter().sum::<f64>() / n as f64;
    let tss: f64 = yv.iter().map(|v| (v - mean).powi(2)).sum();
    if tss <= 0.0 {
        return Err(invalid("the response is constant on the fitting rows"));
    }
    let max_terms = if options.max_terms == 0 { (2 * p).clamp(20, 200) + 1 } else { options.max_terms };
    let end_span = if options.end_span == 0 { friedman_end_span(p) } else { options.end_span };
    let min_span_rule = |nm: usize| if options.min_span == 0 { friedman_min_span(p, nm) } else { options.min_span };

    let mut fw = Forward {
        x: &x,
        order: &order,
        columns: Vec::new(),
        q: Vec::new(),
        r: Vec::new(),
        resid: yv.clone(),
        z: Vec::new(),
        y: &yv,
    };
    fw.add(vec![1.0; n]);
    let mut basis = vec![BasisFunction::default()];
    let mut forward = Vec::new();
    let mut rss = tss;
    let mut slots = 1usize;
    let gcv_null = gcv(tss, n, 1, d);
    let stop = loop {
        let mut best: Option<(f64, usize, Variable, Option<f64>)> = None;
        for parent in 0..basis.len() {
            if basis[parent].degree() >= options.max_degree {
                continue;
            }
            for &v in &variables {
                if basis[parent].uses(v.feature()) {
                    continue;
                }
                let needed = if matches!(v, Variable::Hinged(_)) { 2 } else { 1 };
                if slots + needed > max_terms {
                    continue;
                }
                let (gain, knot) = match v {
                    Variable::Hinged(f) => match fw.best_knot(parent, f, &min_span_rule, end_span) {
                        Some((t, g)) => (g, Some(t)),
                        None => continue,
                    },
                    Variable::Linear(f) => {
                        let c: Vec<f64> = fw.columns[parent].iter().zip(&x[f]).map(|(b, xi)| b * xi).collect();
                        (fw.single_gain(&c), None)
                    }
                    Variable::Level(f, l) => {
                        let c: Vec<f64> = fw.columns[parent]
                            .iter()
                            .zip(&x[f])
                            .map(|(b, xi)| if *xi as usize == l { *b } else { 0.0 })
                            .collect();
                        (fw.single_gain(&c), None)
                    }
                };
                if best.is_none_or(|(g, ..)| gain > g) {
                    best = Some((gain, parent, v, knot));
                }
            }
        }
        let Some((gain, parent, v, knot)) = best else {
            break if slots + 2 > max_terms { StopReason::MaxTerms } else { StopReason::NoImprovement };
        };
        if gain <= 0.0 || !gain.is_finite() {
            break StopReason::NoImprovement;
        }
        if gain / tss < options.threshold {
            break StopReason::SmallGain;
        }
        let pcol = fw.columns[parent].clone();
        let pfactors = basis[parent].factors.clone();
        let mut new_terms: Vec<(Vec<f64>, Factor)> = Vec::new();
        match v {
            Variable::Hinged(f) => {
                let t = knot.unwrap_or(0.0);
                for direction in [Direction::Right, Direction::Left] {
                    let factor = Factor::Hinge { feature: f, knot: t, direction };
                    let c = pcol.iter().zip(&x[f]).map(|(b, xi)| b * factor.value(Value::Number(*xi))).collect();
                    new_terms.push((c, factor));
                }
                slots += 2;
            }
            Variable::Linear(f) => {
                new_terms.push((pcol.iter().zip(&x[f]).map(|(b, xi)| b * xi).collect(), Factor::Linear { feature: f }));
                slots += 1;
            }
            Variable::Level(f, l) => {
                new_terms.push((
                    pcol.iter().zip(&x[f]).map(|(b, xi)| if *xi as usize == l { *b } else { 0.0 }).collect(),
                    Factor::Level { feature: f, level: l },
                ));
                slots += 1;
            }
        }
        let mut added = Vec::new();
        for (c, factor) in new_terms {
            if fw.add(c) {
                let mut factors = pfactors.clone();
                factors.push(factor);
                added.push(basis.len());
                basis.push(BasisFunction { factors });
            }
        }
        if added.is_empty() {
            break StopReason::NoImprovement;
        }
        rss = dot(&fw.resid, &fw.resid).min(rss);
        let r_squared = 1.0 - rss / tss;
        forward.push(ForwardStep { parent, feature: v.feature(), knot, added, r_squared });
        if r_squared >= 1.0 - options.threshold {
            break StopReason::PerfectFit;
        }
        let g = gcv(rss, n, basis.len(), d);
        if 1.0 - g / gcv_null < -10.0 {
            break StopReason::GcvLimit;
        }
    };

    // R as row-major M × M.
    let m = basis.len();
    let mut r = vec![vec![0.0; m]; m];
    for (j, col) in fw.r.iter().enumerate() {
        for (k, v) in col.iter().enumerate() {
            r[k][j] = *v;
        }
    }
    let rss_full = dot(&fw.resid, &fw.resid);
    let mut fit = MarsFit {
        basis,
        forward,
        stop,
        subsets: Vec::new(),
        selected: Vec::new(),
        coefficients: Vec::new(),
        n,
        tss,
        penalty: d,
        min_span: min_span_rule(n),
        end_span,
        variables: p,
        r,
        z: fw.z,
    };
    fit.backward(rss_full)?;
    let best = (0..fit.subsets.len())
        .min_by(|&a, &b| fit.subsets[a].gcv.total_cmp(&fit.subsets[b].gcv).then(a.cmp(&b)))
        .unwrap_or(0);
    fit.select(best + 1)?;
    Ok(fit)
}

impl MarsFit {
    /// Least squares of `Qᵀy` on the columns `terms` of `R`: coefficients
    /// and `(XᵀX)⁻¹` diagonal, and the residual sum of squares.
    fn subset_fit(&self, terms: &[usize], rss_out: f64) -> Result<(Vec<f64>, Vec<f64>, f64)> {
        let m = self.r.len();
        let cols: Vec<Vec<f64>> = terms.iter().map(|&j| (0..m).map(|k| self.r[k][j]).collect()).collect();
        let qr = QrDecomposition::new(Matrix::from_columns(&cols)?)?;
        let b = qr.solve_least_squares(&self.z)?;
        let mut c = self.z.clone();
        qr.apply_qt(&mut c)?;
        let tail: f64 = c[terms.len()..].iter().map(|v| v * v).sum();
        let inv = qr.inverse_gram()?;
        let diag = (0..terms.len()).map(|k| inv[(k, k)]).collect();
        Ok((b, diag, rss_out + tail))
    }

    /// Backward elimination by residual sum of squares; fills `subsets`.
    fn backward(&mut self, rss_out: f64) -> Result<()> {
        let mut current: Vec<usize> = (0..self.basis.len()).collect();
        let mut subsets = Vec::new();
        loop {
            let (b, diag, rss) = self.subset_fit(&current, rss_out)?;
            subsets.push(Subset { terms: current.clone(), rss, gcv: gcv(rss, self.n, current.len(), self.penalty) });
            if current.len() == 1 {
                break;
            }
            // Removing term k raises the RSS by b_k² / (XᵀX)⁻¹_kk.
            let drop = (1..current.len())
                .min_by(|&a, &c| (b[a] * b[a] / diag[a]).total_cmp(&(b[c] * b[c] / diag[c])))
                .unwrap_or(1);
            current.remove(drop);
        }
        subsets.reverse();
        self.subsets = subsets;
        Ok(())
    }

    /// Select the best subset of `size` terms and refit its coefficients.
    ///
    /// # Errors
    /// `size` is 0 or larger than the forward model.
    pub fn select(&mut self, size: usize) -> Result<()> {
        if size == 0 || size > self.subsets.len() {
            return Err(invalid(format!("choose between 1 and {} terms", self.subsets.len())));
        }
        let terms = self.subsets[size - 1].terms.clone();
        let (b, _, _) = self.subset_fit(&terms, 0.0)?;
        self.selected = terms;
        self.coefficients = b;
        Ok(())
    }

    /// Coefficients of the best subset of `size` terms (aligned with its
    /// [`Subset::terms`]).
    ///
    /// # Errors
    /// `size` out of range.
    pub fn subset_coefficients(&self, size: usize) -> Result<Vec<f64>> {
        let s = self.subsets.get(size.wrapping_sub(1)).ok_or_else(|| invalid("no subset of that size"))?;
        Ok(self.subset_fit(&s.terms, 0.0)?.0)
    }

    /// Prediction of the selected model for a row whose predictor `j` has
    /// value `x(j)` (NaN when a predictor it uses is missing).
    #[must_use]
    pub fn predict(&self, x: &dyn Fn(usize) -> Value) -> f64 {
        self.selected.iter().zip(&self.coefficients).map(|(&j, b)| b * self.basis[j].value(x)).sum()
    }

    /// Prediction of the best subset of `size` terms with `coefficients`
    /// (from [`MarsFit::subset_coefficients`]).
    #[must_use]
    pub fn predict_subset(&self, size: usize, coefficients: &[f64], x: &dyn Fn(usize) -> Value) -> f64 {
        self.subsets[size - 1].terms.iter().zip(coefficients).map(|(&j, b)| b * self.basis[j].value(x)).sum()
    }

    /// Sum of squared prediction errors on `rows` for each subset size
    /// (index `k − 1` for `k` terms), with the number of rows it sums over;
    /// rows missing the response or a used predictor are skipped, so the
    /// count can differ between sizes. The mean squared error is `sse / n`.
    ///
    /// # Errors
    /// A subset cannot be refitted (not expected).
    pub fn validation_errors(&self, features: &[Feature], y: &[f64], rows: &[usize]) -> Result<Vec<(f64, usize)>> {
        let mut out = Vec::with_capacity(self.subsets.len());
        for size in 1..=self.subsets.len() {
            let b = self.subset_coefficients(size)?;
            let (mut sse, mut n) = (0.0, 0);
            for e in rows
                .iter()
                .filter(|&&r| y[r].is_finite())
                .map(|&r| self.predict_subset(size, &b, &|j| features[j].value(r)) - y[r])
                .filter(|e| e.is_finite())
            {
                sse += e * e;
                n += 1;
            }
            out.push((sse, n));
        }
        Ok(out)
    }

    /// The selected model's residual sum of squares on the fitting rows.
    #[must_use]
    pub fn rss(&self) -> f64 {
        self.subsets[self.selected.len() - 1].rss
    }

    /// The selected model's GCV.
    #[must_use]
    pub fn gcv(&self) -> f64 {
        self.subsets[self.selected.len() - 1].gcv
    }

    /// R² on the fitting rows.
    #[must_use]
    pub fn r_squared(&self) -> f64 {
        1.0 - self.rss() / self.tss
    }

    /// Generalized R²: `1 − GCV / GCV(intercept only)`.
    #[must_use]
    pub fn generalized_r_squared(&self) -> f64 {
        1.0 - self.gcv() / self.subsets[0].gcv
    }

    /// Variable importance over the subsets of size 2 to the selected size:
    /// for each of `n_features` predictors, the number of subsets using it
    /// and the summed decreases in GCV and in RSS from the subset one term
    /// smaller.
    #[must_use]
    pub fn importance(&self, n_features: usize) -> Vec<Importance> {
        let mut out = vec![Importance::default(); n_features];
        for k in 2..=self.selected.len() {
            let (s, prev) = (&self.subsets[k - 1], &self.subsets[k - 2]);
            let mut used = vec![false; n_features];
            for &t in &s.terms {
                for f in &self.basis[t].factors {
                    used[f.feature()] = true;
                }
            }
            for (j, u) in used.iter().enumerate() {
                if *u {
                    out[j].subsets += 1;
                    out[j].gcv += prev.gcv - s.gcv;
                    out[j].rss += prev.rss - s.rss;
                }
            }
        }
        out
    }
}

/// Importance of one predictor (see [`MarsFit::importance`]).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Importance {
    /// Number of subsets using the predictor.
    pub subsets: usize,
    /// Summed GCV decrease.
    pub gcv: f64,
    /// Summed RSS decrease.
    pub rss: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// y = 2 + 3·max(0, x₁ − 0.3) − 2·max(0, 0.6 − x₂), on a grid whose
    /// values include both knots, with a third, unused predictor.
    fn known_hinges() -> (Vec<Feature>, Vec<f64>) {
        let (mut x1, mut x2, mut x3, mut y) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for i in 0..21 {
            for j in 0..21 {
                let (a, b) = (f64::from(i) / 20.0, f64::from(j) / 20.0);
                x1.push(a);
                x2.push(b);
                x3.push(f64::from((i * 7 + j * 3) % 11));
                y.push(2.0 + 3.0 * (a - 0.3).max(0.0) - 2.0 * (0.6 - b).max(0.0));
            }
        }
        (vec![Feature::Numeric(x1), Feature::Numeric(x2), Feature::Numeric(x3)], y)
    }

    fn all(n: usize) -> Vec<usize> {
        (0..n).collect()
    }

    #[test]
    fn recovers_known_knots_and_coefficients() {
        let (features, y) = known_hinges();
        let opts = MarsOptions { min_span: 1, end_span: 1, ..MarsOptions::default() };
        let fit = fit_mars(&features, &[false; 3], &y, &all(y.len()), &opts).unwrap();
        assert_eq!(fit.stop, StopReason::PerfectFit);
        assert!(fit.rss() < 1e-20, "{}", fit.rss());
        // The two hinges that carry the signal (their reflected partners, if
        // kept, have zero coefficients), and nothing on x3.
        let mut terms: Vec<(usize, f64, Direction, f64)> = fit
            .selected
            .iter()
            .zip(&fit.coefficients)
            .filter(|(_, b)| b.abs() > 1e-8)
            .filter_map(|(&t, &b)| match fit.basis[t].factors.as_slice() {
                [Factor::Hinge { feature, knot, direction }] => Some((*feature, *knot, *direction, b)),
                _ => None,
            })
            .collect();
        terms.sort_by_key(|t| t.0);
        assert_eq!(terms.len(), 2, "{terms:?}");
        assert_eq!((terms[0].0, terms[0].2), (0, Direction::Right));
        assert!((terms[0].1 - 0.3).abs() < 1e-12 && (terms[0].3 - 3.0).abs() < 1e-10, "{terms:?}");
        assert_eq!((terms[1].0, terms[1].2), (1, Direction::Left));
        assert!((terms[1].1 - 0.6).abs() < 1e-12 && (terms[1].3 + 2.0).abs() < 1e-10, "{terms:?}");
        let intercept = fit.coefficients[fit.selected.iter().position(|&t| t == 0).unwrap()];
        assert!((intercept - 2.0).abs() < 1e-10);
        let imp = fit.importance(3);
        assert_eq!(imp[2].subsets, 0);
        assert!(imp[0].rss > 0.0 && imp[1].rss > 0.0);
        // Predictions reproduce the function, also off the grid.
        let p = fit.predict(&|j| Value::Number([0.83, 0.12, 4.0][j]));
        assert!((p - (2.0 + 3.0 * 0.53 - 2.0 * 0.48)).abs() < 1e-10, "{p}");
        assert!(fit.predict(&|j| if j == 0 { Value::Missing } else { Value::Number(0.5) }).is_nan());
    }

    #[test]
    fn interaction_of_two_hinges() {
        // y = 4·max(0, x₁ − 0.5)·max(0, x₂ − 0.25) + 0.5 x₂ needs degree 2.
        let (features, _) = known_hinges();
        let (Feature::Numeric(a), Feature::Numeric(b)) = (&features[0], &features[1]) else { panic!() };
        let y: Vec<f64> =
            a.iter().zip(b).map(|(a, b)| 4.0 * (a - 0.5).max(0.0) * (b - 0.25).max(0.0) + 0.5 * b).collect();
        let opts = MarsOptions { max_degree: 2, min_span: 1, end_span: 1, ..MarsOptions::default() };
        let fit = fit_mars(&features, &[false; 3], &y, &all(y.len()), &opts).unwrap();
        assert!(fit.r_squared() > 1.0 - 1e-12, "{}", fit.r_squared());
        let pair = fit.selected.iter().any(|&t| {
            let f = &fit.basis[t].factors;
            f.len() == 2
                && f.iter().any(|f| matches!(f, Factor::Hinge { feature: 0, knot, direction: Direction::Right } if (knot - 0.5).abs() < 1e-12))
                && f.iter().any(|f| matches!(f, Factor::Hinge { feature: 1, knot, direction: Direction::Right } if (knot - 0.25).abs() < 1e-12))
        });
        assert!(pair, "{:?}", fit.selected.iter().map(|&t| &fit.basis[t]).collect::<Vec<_>>());
        assert_eq!(fit.penalty, 3.0);
        // An additive fit cannot reproduce it.
        let additive =
            fit_mars(&features, &[false; 3], &y, &all(y.len()), &MarsOptions { max_degree: 1, ..opts }).unwrap();
        assert!(additive.r_squared() < 0.999);
    }

    #[test]
    fn linear_predictors_and_levels() {
        // y = 1 + 2x + level effect (+1 for level 1, −3 for level 2).
        let n = 60;
        let x: Vec<f64> = (0..n).map(|i| f64::from(i as u32) / 10.0).collect();
        let codes: Vec<Option<usize>> = (0..n).map(|i| Some(i % 3)).collect();
        let y: Vec<f64> = (0..n).map(|i| 1.0 + 2.0 * x[i] + [0.0, 1.0, -3.0][i % 3]).collect();
        let features = vec![Feature::Numeric(x), Feature::Categorical { codes, levels: 3 }];
        let fit = fit_mars(&features, &[true, false], &y, &all(n), &MarsOptions::default()).unwrap();
        assert!(fit.rss() < 1e-18, "{}", fit.rss());
        assert_eq!(fit.variables, 3, "x and two indicators");
        assert!(fit.basis.iter().all(|b| b.factors.iter().all(|f| !matches!(f, Factor::Hinge { .. }))));
        for (&t, &b) in fit.selected.iter().zip(&fit.coefficients) {
            match fit.basis[t].factors.as_slice() {
                [] => assert!((b - 1.0).abs() < 1e-10),
                [Factor::Linear { feature: 0 }] => assert!((b - 2.0).abs() < 1e-10),
                [Factor::Level { feature: 1, level: 1 }] => assert!((b - 1.0).abs() < 1e-10),
                [Factor::Level { feature: 1, level: 2 }] => assert!((b + 3.0).abs() < 1e-10),
                other => panic!("unexpected term {other:?}"),
            }
        }
        // A missing level (or an unseen one) gives no prediction.
        assert!(fit.predict(&|j| if j == 0 { Value::Number(1.0) } else { Value::Missing }).is_nan());
    }

    #[test]
    fn spans_gcv_and_validation_pruning() {
        assert_eq!((friedman_min_span(10, 200), friedman_end_span(10)), (6, 10));
        assert_eq!(friedman_end_span(1), 7);
        assert!((gcv(10.0, 100, 5, 2.0) - 10.0 / (100.0 * (1.0 - 9.0 / 100.0f64).powi(2))).abs() < 1e-15);
        assert!(gcv(1.0, 5, 5, 2.0).is_infinite());

        // Noisy hinge data: GCV prunes the forward model, and the subset
        // sizes can be scored on held-out rows.
        let mut rng = stat_core::rng::Rng::new(3);
        let n = 300;
        let x: Vec<f64> = (0..n).map(|_| rng.next_f64()).collect();
        let z: Vec<f64> = (0..n).map(|_| rng.next_f64()).collect();
        let y: Vec<f64> = (0..n).map(|i| 5.0 * (x[i] - 0.4).max(0.0) + 0.3 * rng.normal()).collect();
        let features = vec![Feature::Numeric(x), Feature::Numeric(z)];
        let train: Vec<usize> = (0..200).collect();
        let valid: Vec<usize> = (200..n).collect();
        let mut fit = fit_mars(&features, &[false, false], &y, &train, &MarsOptions::default()).unwrap();
        assert_eq!(fit.n, 200);
        assert!(fit.selected.len() < fit.basis.len());
        for w in fit.subsets.windows(2) {
            assert!(w[1].rss <= w[0].rss + 1e-9, "nested subsets never fit worse with more terms");
        }
        let counted = fit.validation_errors(&features, &y, &valid).unwrap();
        assert!(counted.iter().all(|&(_, n)| n == valid.len()));
        let errors: Vec<f64> = counted.iter().map(|&(sse, _)| sse).collect();
        assert_eq!(errors.len(), fit.subsets.len());
        assert!(errors[0] > 3.0 * errors[2], "{errors:?}");
        let best = (0..errors.len()).min_by(|&a, &b| errors[a].total_cmp(&errors[b])).unwrap() + 1;
        fit.select(best).unwrap();
        assert_eq!(fit.selected.len(), best);
        assert!(fit.select(0).is_err() && fit.select(fit.subsets.len() + 1).is_err());
        assert!(fit.predict(&|j| features[j].value(250)).is_finite());
    }

    #[test]
    fn errors() {
        let f = vec![Feature::Numeric(vec![1.0, 2.0, 3.0, 4.0])];
        let y = vec![1.0, 2.0, 3.0, 5.0];
        let rows = all(4);
        assert!(fit_mars(&[], &[], &y, &rows, &MarsOptions::default()).is_err());
        assert!(fit_mars(&f, &[], &y, &rows, &MarsOptions::default()).is_err());
        assert!(fit_mars(&f, &[false], &y, &rows, &MarsOptions { max_degree: 0, ..MarsOptions::default() }).is_err());
        assert!(
            fit_mars(&f, &[false], &y, &rows, &MarsOptions { penalty: Some(-1.0), ..MarsOptions::default() }).is_err()
        );
        assert!(fit_mars(&f, &[false], &y, &rows, &MarsOptions { threshold: 1.0, ..MarsOptions::default() }).is_err());
        assert!(fit_mars(&f, &[false], &[1.0; 4], &rows, &MarsOptions::default()).is_err(), "constant response");
        assert!(fit_mars(&f, &[false], &y, &[0, 1], &MarsOptions::default()).is_err(), "too few rows");
    }
}
