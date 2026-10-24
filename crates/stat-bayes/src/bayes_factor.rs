//! Default Bayes factors for t tests, correlations and contingency tables.
//!
//! **t tests** (Rouder, Speckman, Sun, Morey & Iverson 2009, *Psychon.
//! Bull. Rev.* 16, eq. 1). The standardized effect `δ` has a Cauchy(0, r)
//! prior under H₁ (Jeffreys–Zellner–Siow), written as `δ │ g ~ N(0, g r²)`
//! with `g ~ Inv-Gamma(½, ½)`; the nuisance mean and variance have Jeffreys
//! priors. With `t` the classical statistic, `ν` its degrees of freedom and
//! `N` the effective sample size (`n` for one sample or paired differences,
//! `n₁n₂/(n₁ + n₂)` for two samples),
//!
//! `BF₁₀ = ∫₀^∞ (1 + Ngr²)^{−1/2} (1 + t²/((1 + Ngr²)ν))^{−(ν+1)/2}
//! (2π)^{−1/2} g^{−3/2} e^{−1/(2g)} dg / (1 + t²/ν)^{−(ν+1)/2}`,
//!
//! integrated in `ln g`. The marginal likelihood of `δ` is the noncentral t
//! density of `t` with noncentrality `δ√N`, so the posterior of `δ` is that
//! density times the Cauchy prior, normalized on a grid. A one-sided
//! alternative (`δ > 0` or `δ < 0`) restricts the prior to that half, giving
//! `BF₊₀ = 2 BF₁₀ P(δ > 0 │ y, H₁)` (Morey & Wagenmakers 2014, *Stat. Prob.
//! Lett.* 92).
//!
//! **Correlation** (Ly, Verhagen & Wagenmakers 2016, *J. Math. Psych.* 72):
//! with a stretched Beta(1/κ, 1/κ) prior on `ρ ∈ (−1, 1)` and Jeffreys
//! priors on the means and variances, the likelihood ratio of `ρ` to 0 is
//! `h(ρ) = (1 − ρ²)^{(n−1)/2} Σₖ Γ²((n + k − 1)/2)(2ρr)ᵏ / (k! Γ²((n − 1)/2))`,
//! evaluated through Hotelling's closed form of the density of `r` (see
//! [`ln_correlation_likelihood_ratio`]), and `BF₁₀ = ∫ h(ρ) π(ρ) dρ`.
//!
//! **Contingency tables** (Gunel & Dickey 1974, *Biometrika* 61): Dirichlet
//! priors with concentration `a` on the cell probabilities under H₁ against
//! independence under H₀, for joint multinomial sampling (only the total
//! fixed), independent multinomial sampling (row or column totals fixed) and
//! Poisson sampling (nothing fixed; the grand total has a gamma prior with
//! mean equal to the observed total). Under independence the row and column
//! probabilities have Gunel & Dickey's consistent Dirichlet priors, with
//! parameters `aJ − (J − 1)` and `aI − (I − 1)`. For joint multinomial
//! sampling `BF₁₀ = [D(y + a)/D(a)] / ([D(y_{i+} + α_r)/D(α_r)]
//! [D(y_{+j} + α_c)/D(α_c)])`, with `D(α) = Π Γ(αᵢ) / Γ(Σ αᵢ)`.
//!
//! Evidence categories follow Jeffreys (1961, *Theory of Probability*,
//! App. B) as adapted by Lee & Wagenmakers (2013, *Bayesian Cognitive
//! Modeling*, table 7.1).

use crate::integrate::{integrate, integrate_log};
use crate::univariate::{Summary, Univariate, summarize};
use stat_core::special::{ln_beta, ln_gamma};
use stat_core::{Result, StatError};

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

const LN_2PI: f64 = 1.837_877_066_409_345_5;

/// Which alternative hypothesis a Bayes factor tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// `δ ≠ 0`.
    TwoSided,
    /// `δ > 0`.
    Greater,
    /// `δ < 0`.
    Less,
}

/// Verbal category of a Bayes factor `BF₁₀` (Lee & Wagenmakers 2013).
#[must_use]
pub fn evidence_label(bf10: f64) -> &'static str {
    let (b, favour_null) = if bf10 >= 1.0 { (bf10, false) } else { (1.0 / bf10, true) };
    let strength = if b < 3.0 {
        "anecdotal"
    } else if b < 10.0 {
        "moderate"
    } else if b < 30.0 {
        "strong"
    } else if b < 100.0 {
        "very strong"
    } else {
        "extreme"
    };
    match (b < 1.0 + 1e-12, favour_null) {
        (true, _) => "no evidence either way",
        (false, false) => match strength {
            "anecdotal" => "anecdotal evidence for H₁",
            "moderate" => "moderate evidence for H₁",
            "strong" => "strong evidence for H₁",
            "very strong" => "very strong evidence for H₁",
            _ => "extreme evidence for H₁",
        },
        (false, true) => match strength {
            "anecdotal" => "anecdotal evidence for H₀",
            "moderate" => "moderate evidence for H₀",
            "strong" => "strong evidence for H₀",
            "very strong" => "very strong evidence for H₀",
            _ => "extreme evidence for H₀",
        },
    }
}

/// `ln BF₁₀` of the two-sided JZS t test for statistic `t` with `df`
/// degrees of freedom, effective sample size `n_eff` and Cauchy scale `r`.
#[must_use]
pub fn ln_jzs_t(t: f64, n_eff: f64, df: f64, r: f64) -> f64 {
    let null = -0.5 * (df + 1.0) * (t * t / df).ln_1p();
    let ln_f = |u: f64| {
        let g = u.exp();
        let a = 1.0 + n_eff * g * r * r;
        -0.5 * a.ln() - 0.5 * (df + 1.0) * (t * t / (a * df)).ln_1p() - 0.5 * LN_2PI - 0.5 * u - 0.5 / g
    };
    integrate_log(ln_f, 40.0) - null
}

/// `ln` of the noncentral t density of `t` (`df`, noncentrality `ncp`), as
/// `∫ φ(t√(v/ν) − λ) √(v/ν) f_{χ²_ν}(v) dv`, integrated in `ln v`.
#[must_use]
pub fn ln_noncentral_t_density(t: f64, df: f64, ncp: f64) -> f64 {
    let h = 0.5 * df;
    let ln_chi_norm = -h * std::f64::consts::LN_2 - ln_gamma(h);
    let ln_f = |w: f64| {
        let v = w.exp();
        let z = t * (v / df).sqrt() - ncp;
        -0.5 * LN_2PI - 0.5 * z * z + 0.5 * (v / df).ln() + ln_chi_norm + (h - 1.0) * w - 0.5 * v + w
    };
    // ln v of the χ² mode ≈ ln ν: centre the search there.
    let centre = df.ln();
    integrate_log(|w| ln_f(w + centre), 30.0)
}

/// A distribution tabulated on a grid of points with its density
/// (normalized by the trapezoid rule) and distribution function.
#[derive(Debug, Clone)]
pub struct GridDist {
    x: Vec<f64>,
    pdf: Vec<f64>,
    cdf: Vec<f64>,
    mean: f64,
    sd: f64,
}

impl GridDist {
    /// Tabulate `exp(ln_density)` on `n` equal steps over `[lo, hi]`.
    ///
    /// # Errors
    /// A density that is zero or non-finite everywhere.
    pub fn new(ln_density: impl Fn(f64) -> f64, lo: f64, hi: f64, n: usize) -> Result<Self> {
        let x: Vec<f64> = (0..=n).map(|i| lo + (hi - lo) * i as f64 / n as f64).collect();
        let l: Vec<f64> = x.iter().map(|&v| ln_density(v)).collect();
        let peak = l.iter().copied().filter(|v| v.is_finite()).fold(f64::NEG_INFINITY, f64::max);
        if !peak.is_finite() {
            return Err(invalid("the posterior density could not be evaluated"));
        }
        let mut pdf: Vec<f64> = l.iter().map(|&v| if v.is_finite() { (v - peak).exp() } else { 0.0 }).collect();
        let h = (hi - lo) / n as f64;
        let mut cdf = vec![0.0; x.len()];
        for i in 1..x.len() {
            cdf[i] = cdf[i - 1] + 0.5 * h * (pdf[i - 1] + pdf[i]);
        }
        let total = cdf[x.len() - 1];
        for v in &mut pdf {
            *v /= total;
        }
        for v in &mut cdf {
            *v /= total;
        }
        let mean = (1..x.len()).map(|i| 0.5 * h * (x[i - 1] * pdf[i - 1] + x[i] * pdf[i])).sum::<f64>();
        let m2 = (1..x.len())
            .map(|i| 0.5 * h * ((x[i - 1] - mean).powi(2) * pdf[i - 1] + (x[i] - mean).powi(2) * pdf[i]))
            .sum::<f64>();
        Ok(Self { x, pdf, cdf, mean, sd: m2.max(0.0).sqrt() })
    }

    /// The grid points and densities.
    #[must_use]
    pub fn curve(&self) -> Vec<[f64; 2]> {
        self.x.iter().zip(&self.pdf).map(|(&x, &d)| [x, d]).collect()
    }

    fn locate(&self, v: f64) -> Option<usize> {
        if v <= self.x[0] || v >= self.x[self.x.len() - 1] {
            return None;
        }
        Some(self.x.partition_point(|&g| g <= v) - 1)
    }
}

impl Univariate for GridDist {
    fn pdf(&self, v: f64) -> f64 {
        self.locate(v).map_or(0.0, |i| {
            let w = (v - self.x[i]) / (self.x[i + 1] - self.x[i]);
            self.pdf[i] * (1.0 - w) + self.pdf[i + 1] * w
        })
    }
    fn cdf(&self, v: f64) -> f64 {
        if v <= self.x[0] {
            return 0.0;
        }
        self.locate(v).map_or(1.0, |i| {
            // Exact integral of the linear density over the partial cell.
            let h = v - self.x[i];
            let slope = (self.pdf[i + 1] - self.pdf[i]) / (self.x[i + 1] - self.x[i]);
            self.cdf[i] + self.pdf[i] * h + 0.5 * slope * h * h
        })
    }
    fn mean(&self) -> f64 {
        self.mean
    }
    fn sd(&self) -> f64 {
        self.sd
    }
    fn support(&self) -> (f64, f64) {
        (self.x[0], self.x[self.x.len() - 1])
    }
}

/// Result of a Bayes factor test.
#[derive(Debug, Clone)]
pub struct BayesFactorTest {
    /// `BF₁₀` for the chosen alternative.
    pub bf10: f64,
    /// `ln BF₁₀`.
    pub ln_bf10: f64,
    /// Prior scale (`r` for t tests, `κ` for correlations).
    pub scale: f64,
    /// Alternative.
    pub direction: Direction,
    /// Posterior of the effect (`δ` or `ρ`) under the two-sided H₁.
    pub posterior: GridDist,
    /// Its summary.
    pub summary: Summary,
    /// `ln BF₁₀` at a range of prior scales: `(scale, ln BF)`.
    pub robustness: Vec<(f64, f64)>,
    /// Classical statistic (`t` or `r`).
    pub statistic: f64,
    /// Sample size or effective sample size.
    pub n: f64,
}

/// Which t test.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TDesign {
    /// One sample (or paired differences) of size `n`.
    OneSample {
        /// Sample size.
        n: f64,
    },
    /// Two independent samples.
    TwoSample {
        /// First group size.
        n1: f64,
        /// Second group size.
        n2: f64,
    },
}

impl TDesign {
    fn n_eff_df(self) -> (f64, f64) {
        match self {
            Self::OneSample { n } => (n, n - 1.0),
            Self::TwoSample { n1, n2 } => (n1 * n2 / (n1 + n2), n1 + n2 - 2.0),
        }
    }
}

/// Prior scales for the robustness curve: from 0.05 to 1.5 times
/// `max(1, scale)`, logarithmically spaced, plus the chosen scale.
fn robustness_scales(scale: f64, top: f64) -> Vec<f64> {
    let hi = top * scale.max(1.0);
    let lo = 0.05_f64;
    let n = 40;
    let mut out: Vec<f64> = (0..=n).map(|i| lo * (hi / lo).powf(i as f64 / n as f64)).collect();
    out.push(scale);
    out.sort_by(f64::total_cmp);
    out.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
    out
}

fn one_sided(ln_bf_two: f64, posterior: &GridDist, direction: Direction) -> f64 {
    match direction {
        Direction::TwoSided => ln_bf_two,
        Direction::Greater => ln_bf_two + (2.0 * posterior.sf(0.0)).ln(),
        Direction::Less => ln_bf_two + (2.0 * posterior.cdf(0.0)).ln(),
    }
}

/// The JZS Bayes factor t test for statistic `t` with Cauchy scale `r`
/// (√2⁄2 is the usual "medium" default).
///
/// # Errors
/// Fewer than two observations (three for two samples), a non-finite `t`,
/// a non-positive scale or an invalid level.
pub fn t_test(t: f64, design: TDesign, r: f64, direction: Direction, level: f64) -> Result<BayesFactorTest> {
    let (n_eff, df) = design.n_eff_df();
    if !(df >= 1.0 && n_eff > 0.0) {
        return Err(invalid("too few observations for a t test"));
    }
    if !t.is_finite() {
        return Err(invalid("the t statistic is not finite (do the values vary?)"));
    }
    if !(r > 0.0 && r.is_finite()) {
        return Err(invalid("the prior scale must be positive"));
    }
    if !(level > 0.0 && level < 1.0) {
        return Err(invalid("the credible level must lie between 0 and 1"));
    }
    let ln_two = ln_jzs_t(t, n_eff, df, r);
    // Posterior of δ: noncentral-t likelihood × Cauchy(0, r) prior.
    let centre = t / n_eff.sqrt();
    let spread = (1.0 + t * t / (2.0 * df)).sqrt() / n_eff.sqrt();
    let (lo, hi) = (centre - 12.0 * spread - 0.5 * r, centre + 12.0 * spread + 0.5 * r);
    let ln_post = |d: f64| ln_noncentral_t_density(t, df, d * n_eff.sqrt()) - (1.0 + (d / r).powi(2)).ln();
    let posterior = GridDist::new(ln_post, lo, hi, 1200)?;
    let ln_bf10 = one_sided(ln_two, &posterior, direction);
    let robustness = robustness_scales(r, 1.5)
        .into_iter()
        .map(|s| {
            let two = ln_jzs_t(t, n_eff, df, s);
            let l = if direction == Direction::TwoSided {
                two
            } else {
                let p = GridDist::new(
                    |d: f64| ln_noncentral_t_density(t, df, d * n_eff.sqrt()) - (1.0 + (d / s).powi(2)).ln(),
                    centre - 12.0 * spread - 0.5 * s,
                    centre + 12.0 * spread + 0.5 * s,
                    300,
                );
                p.map_or(two, |p| one_sided(two, &p, direction))
            };
            (s, l)
        })
        .collect();
    let summary = summarize(&posterior, level);
    Ok(BayesFactorTest {
        bf10: ln_bf10.exp(),
        ln_bf10,
        scale: r,
        direction,
        summary,
        posterior,
        robustness,
        statistic: t,
        n: n_eff,
    })
}

/// `ln ₂F₁(½, ½; c; x)` for `c ≥ 1`, `0 ≤ x < 1`: the power series (positive
/// terms, geometric decay) up to `x = 0.9`, and beyond, where the series
/// slows as `x → 1`, Euler's integral with `t = sin²θ`:
/// `₂F₁ = Γ(c) / (Γ(½) Γ(c − ½)) · 2 ∫₀^{π/2} cos^{2c−2}θ (1 − x sin²θ)^{−½} dθ`
/// (DLMF 15.6.1), by adaptive Gauss–Kronrod quadrature.
fn ln_hyp2f1_half(c: f64, x: f64) -> f64 {
    if x <= 0.9 {
        let (mut term, mut sum) = (1.0_f64, 1.0_f64);
        for k in 0..100_000 {
            let k = f64::from(k);
            term *= (k + 0.5) * (k + 0.5) / ((c + k) * (k + 1.0)) * x;
            sum += term;
            if term < 1e-17 * sum {
                break;
            }
        }
        return sum.ln();
    }
    let p = 2.0 * c - 2.0;
    let f = |theta: f64| {
        let (s, co) = theta.sin_cos();
        (p * co.max(0.0).ln()).exp() / (1.0 - x * s * s).sqrt()
    };
    let half_pi = std::f64::consts::FRAC_PI_2;
    let integral: f64 = integrate(f, 0.0, half_pi, 1e-12, 8);
    ln_gamma(c) - ln_gamma(0.5) - ln_gamma(c - 0.5) + (2.0 * integral).ln()
}

/// `ln h(ρ)` of Ly et al. (2016): the likelihood ratio of `ρ` to 0 given the
/// sample correlation `r` of `n` pairs. Computed from Hotelling's (1953,
/// *JRSS B* 15) exact density of `r`,
/// `h(ρ) = (1 − ρ²)^{(n−1)/2} (1 − ρr)^{3/2−n} F((1 + ρr)/2) / F(½)`
/// with `F = ₂F₁(½, ½; n − ½; ·)`, which stays fast and accurate as `|ρr| → 1`.
#[must_use]
pub fn ln_correlation_likelihood_ratio(n: f64, r: f64, rho: f64) -> f64 {
    if rho == 0.0 {
        return 0.0;
    }
    let c = n - 0.5;
    0.5 * (n - 1.0) * (1.0 - rho * rho).ln()
        + (1.5 - n) * (1.0 - rho * r).ln()
        + ln_hyp2f1_half(c, 0.5 * (1.0 + rho * r))
        - ln_hyp2f1_half(c, 0.5)
}

/// `ln` of the stretched Beta(1/κ, 1/κ) prior density on (−1, 1).
fn ln_stretched_beta(rho: f64, kappa: f64) -> f64 {
    let a = 1.0 / kappa;
    (1.0 - 2.0 * a) * std::f64::consts::LN_2 + (a - 1.0) * (1.0 - rho * rho).ln() - ln_beta(a, a)
}

fn ln_correlation_bf(n: f64, r: f64, kappa: f64, sign: f64) -> f64 {
    // ρ = tanh z on the whole line (sign 0), or on one half.
    let f = |z: f64| {
        let rho = z.tanh();
        if sign * rho < 0.0 {
            return f64::NEG_INFINITY;
        }
        let jac = (1.0 - rho * rho).ln();
        ln_correlation_likelihood_ratio(n, r, rho) + ln_stretched_beta(rho, kappa) + jac
    };
    let l = integrate_log(f, 12.0);
    if sign == 0.0 { l } else { l + std::f64::consts::LN_2 }
}

/// The Bayes factor for a Pearson correlation `r` of `n` pairs with a
/// stretched Beta(1/κ, 1/κ) prior (`κ = 1`: uniform; `⅓` is the usual
/// "medium" scale).
///
/// # Errors
/// Fewer than three pairs, `|r| ≥ 1`, a non-positive `κ` or invalid level.
pub fn correlation(r: f64, n: f64, kappa: f64, direction: Direction, level: f64) -> Result<BayesFactorTest> {
    if n < 3.0 {
        return Err(invalid("a correlation Bayes factor needs at least three pairs"));
    }
    if r.is_nan() || r.abs() >= 1.0 {
        return Err(invalid("the correlation must lie strictly between −1 and 1"));
    }
    if !(kappa > 0.0 && kappa <= 2.0) {
        return Err(invalid("the prior width κ must lie between 0 and 2"));
    }
    if !(level > 0.0 && level < 1.0) {
        return Err(invalid("the credible level must lie between 0 and 1"));
    }
    let sign = match direction {
        Direction::TwoSided => 0.0,
        Direction::Greater => 1.0,
        Direction::Less => -1.0,
    };
    let ln_bf10 = ln_correlation_bf(n, r, kappa, sign);
    let posterior = GridDist::new(
        |rho: f64| ln_correlation_likelihood_ratio(n, r, rho) + ln_stretched_beta(rho, kappa),
        -1.0 + 1e-9,
        1.0 - 1e-9,
        4000,
    )?;
    let robustness = robustness_scales(kappa, 1.0)
        .into_iter()
        .filter(|&k| k <= 2.0)
        .map(|k| (k, ln_correlation_bf(n, r, k, sign)))
        .collect();
    let summary = summarize(&posterior, level);
    Ok(BayesFactorTest {
        bf10: ln_bf10.exp(),
        ln_bf10,
        scale: kappa,
        direction,
        summary,
        posterior,
        robustness,
        statistic: r,
        n,
    })
}

/// How a contingency table was sampled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sampling {
    /// Nothing fixed in advance (Poisson counts).
    Poisson,
    /// Only the grand total fixed.
    JointMultinomial,
    /// Each row's total fixed.
    FixedRows,
    /// Each column's total fixed.
    FixedColumns,
}

fn ln_dirichlet_norm(alpha: &[f64]) -> f64 {
    alpha.iter().map(|&a| ln_gamma(a)).sum::<f64>() - ln_gamma(alpha.iter().sum())
}

/// `ln BF₁₀` (association against independence) of an `r × c` table of
/// counts (`table[i][j]`), with prior concentration `a` (1 is the usual
/// default).
///
/// # Errors
/// Fewer than two rows or columns, ragged rows, negative or non-integer
/// counts, or `a ≤ 0`.
pub fn ln_contingency(table: &[Vec<f64>], sampling: Sampling, a: f64) -> Result<f64> {
    let i_n = table.len();
    let j_n = table.first().map_or(0, Vec::len);
    if i_n < 2 || j_n < 2 || table.iter().any(|r| r.len() != j_n) {
        return Err(invalid("a contingency table needs at least two rows and two columns"));
    }
    if table.iter().flatten().any(|&v| !(v >= 0.0 && (v - v.round()).abs() < 1e-9)) {
        return Err(invalid("contingency table counts must be non-negative whole numbers"));
    }
    if a.is_nan() || a <= 0.0 {
        return Err(invalid("the prior concentration must be positive"));
    }
    let (fi, fj) = (i_n as f64, j_n as f64);
    let rows: Vec<f64> = table.iter().map(|r| r.iter().sum()).collect();
    let cols: Vec<f64> = (0..j_n).map(|j| table.iter().map(|r| r[j]).sum()).collect();
    let cells: Vec<f64> = table.iter().flatten().copied().collect();
    let post = |counts: &[f64], prior: f64| {
        let alpha: Vec<f64> = counts.iter().map(|&c| c + prior).collect();
        ln_dirichlet_norm(&alpha) - ln_dirichlet_norm(&vec![prior; counts.len()])
    };
    // Gunel & Dickey's consistent priors under independence: Dirichlet
    // parameters aJ − (J − 1) on the row and aI − (I − 1) on the column
    // probabilities.
    let joint = || post(&cells, a) - post(&rows, a * fj - (fj - 1.0)) - post(&cols, a * fi - (fi - 1.0));
    Ok(match sampling {
        Sampling::JointMultinomial => joint(),
        Sampling::FixedRows => table.iter().map(|r| post(r, a)).sum::<f64>() - post(&cols, a * fi - (fi - 1.0)),
        Sampling::FixedColumns => {
            (0..j_n).map(|j| post(&table.iter().map(|r| r[j]).collect::<Vec<_>>(), a)).sum::<f64>()
                - post(&rows, a * fj - (fj - 1.0))
        }
        Sampling::Poisson => {
            // The grand total λ₊₊ has a Gamma(s, b) prior with shape
            // s₁ = aIJ under H₁ and s₀ = s₁ − (I − 1)(J − 1) under H₀ (the
            // consistent priors), and rate b = aIJ/N, so its prior mean is
            // the observed total.
            let n: f64 = cells.iter().sum();
            let s1 = a * fi * fj;
            let s0 = s1 - (fi - 1.0) * (fj - 1.0);
            let b = s1 / n.max(1.0);
            joint() + (ln_gamma(n + s1) - ln_gamma(s1)) - (ln_gamma(n + s0) - ln_gamma(s0))
                + (s1 - s0) * (b / (1.0 + b)).ln()
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrate::integrate;

    #[test]
    fn noncentral_t_density_matches_central_and_integrates_to_one() {
        // ncp 0: the central t density.
        use stat_core::ContinuousDistribution;
        let t = stat_core::StudentsT::new(7.0).unwrap();
        for x in [-2.0, 0.3, 1.7] {
            assert!((ln_noncentral_t_density(x, 7.0, 0.0) - t.pdf(x).ln()).abs() < 1e-9);
        }
        let total = integrate(|x| ln_noncentral_t_density(x, 9.0, 2.5).exp(), -15.0, 40.0, 1e-10, 32);
        assert!((total - 1.0).abs() < 1e-8);
    }

    #[test]
    fn correlation_likelihood_series_matches_both_forms() {
        // Σₖ form at small n by direct summation.
        let (n, r, rho) = (6.0_f64, 0.4_f64, -0.3_f64);
        let mut s = 0.0;
        for k in 0..200 {
            let kf = f64::from(k);
            s += (2.0 * ln_gamma((n + kf - 1.0) / 2.0) - ln_gamma(kf + 1.0) - 2.0 * ln_gamma((n - 1.0) / 2.0)).exp()
                * (2.0 * rho * r).powi(k);
        }
        let direct = 0.5 * (n - 1.0) * (1.0 - rho * rho).ln() + s.ln();
        assert!((ln_correlation_likelihood_ratio(n, r, rho) - direct).abs() < 1e-10);
        assert_eq!(ln_correlation_likelihood_ratio(n, r, 0.0), 0.0);
        // Near |ρr| = 1, where the integral form takes over (x > 0.9): the
        // Σₖ series by direct summation with many terms.
        for (n, r, rho) in [(5.0_f64, 0.97_f64, 0.95_f64), (12.0, -0.9, -0.9), (4.0, 0.999, 0.99)] {
            let mut s = 0.0_f64;
            for k in 0..200_000 {
                let kf = f64::from(k);
                s += (2.0 * ln_gamma((n + kf - 1.0) / 2.0) - ln_gamma(kf + 1.0) - 2.0 * ln_gamma((n - 1.0) / 2.0)
                    + kf * (2.0 * rho * r).ln())
                .exp();
            }
            let direct = 0.5 * (n - 1.0) * (1.0 - rho * rho).ln() + s.ln();
            let got = ln_correlation_likelihood_ratio(n, r, rho);
            assert!((got - direct).abs() < 1e-9, "n {n}, r {r}, ρ {rho}: {got} vs {direct}");
        }
    }

    #[test]
    fn evidence_labels() {
        assert_eq!(evidence_label(5.0), "moderate evidence for H₁");
        assert_eq!(evidence_label(1.0 / 40.0), "very strong evidence for H₀");
        assert_eq!(evidence_label(1.0), "no evidence either way");
    }
}
