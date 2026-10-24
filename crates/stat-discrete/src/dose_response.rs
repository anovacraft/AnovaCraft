//! Dose–response (quantal response) analysis: probit, logit or
//! complementary log-log regression of the proportion responding on a
//! (usually log-transformed) dose, with effective doses and their fiducial
//! limits.
//!
//! # Model
//! At dose `d` with `x = h(d)` (`h` the identity, `log₁₀` or `ln`), each of
//! `nᵢ` subjects responds independently with probability
//! `P = F(α + βx)`, where `F` is the standard normal cdf (probit), the
//! logistic cdf (logit) or `1 − exp(−e^η)` (complementary log-log). The model
//! is the binomial GLM of [`crate::glm`] with the trials as prior weights,
//! fitted by maximum likelihood. Rows with the same dose are pooled first, so
//! one-row-per-subject (0/1) data and grouped data give the same fit.
//!
//! # Goodness of fit and heterogeneity
//! Pearson's `χ² = Σ (rᵢ − nᵢP̂ᵢ)² / (nᵢP̂ᵢ(1 − P̂ᵢ))` on `k − 2` df (`k` dose
//! groups) tests the model. When it is significant at the chosen level the
//! variation between groups exceeds binomial variation, and Finney (*Probit
//! Analysis*, 3rd ed., 1971, §4.4) multiplies the variances by the
//! heterogeneity factor `h = χ²/(k − 2)` and uses Student's t on `k − 2` df in
//! place of the normal deviate.
//!
//! # Effective doses and fiducial limits
//! The dose giving response probability `p` is `m = (F⁻¹(p) − α̂)/β̂` on the
//! `x` scale (ED50, the median effective dose, at `p = ½`). Its fiducial
//! limits (Finney 1971, §4.3; Fieller's theorem, Fieller 1954, *JRSS B* 16)
//! are the values of `x` with
//!
//! `(α̂ + β̂x − F⁻¹(p))² ≤ t² (v₁₁ + 2x v₁₂ + x² v₂₂)`,
//!
//! `vᵢⱼ` the (heterogeneity-scaled) covariances of `(α̂, β̂)` and `t` the normal
//! (or t) quantile: the roots of
//! `(β̂² − t²v₂₂)x² + 2(β̂(α̂ − ξ) − t²v₁₂)x + (α̂ − ξ)² − t²v₁₁ = 0`, `ξ = F⁻¹(p)`.
//! They exist only when `g = t²v₂₂/β̂² < 1` (the slope is significantly
//! non-zero); otherwise the limits are unbounded and are not reported. They
//! are asymmetric about `m`, unlike the delta-method interval, which is also
//! given (`se(m) = √(v₁₁ + 2m v₁₂ + m²v₂₂)/|β̂|`). Limits on the `x` scale are
//! transformed back to doses.

use crate::glm::{Family, GlmFit, GlmOptions, Link, fit_glm};
use stat_core::distributions::{ChiSquared, ContinuousDistribution, Normal, StudentsT};
use stat_core::special::ln_gamma;
use stat_core::{Matrix, Result, StatError};

/// Scale on which the dose enters the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DoseScale {
    /// The dose itself.
    Linear,
    /// `log₁₀(dose)` (the classical probit-analysis scale).
    Log10,
    /// Natural log of the dose.
    Ln,
}

impl DoseScale {
    /// Every scale.
    pub const ALL: [Self; 3] = [Self::Log10, Self::Ln, Self::Linear];

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Linear => "dose",
            Self::Log10 => "log₁₀ dose",
            Self::Ln => "ln dose",
        }
    }

    /// Stable key for project files and scripts.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Linear => "linear",
            Self::Log10 => "log10",
            Self::Ln => "ln",
        }
    }

    /// The scale with this key.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.key() == key)
    }

    /// Transform a dose to the model scale (`None` when the log is undefined).
    #[must_use]
    pub fn forward(self, dose: f64) -> Option<f64> {
        match self {
            Self::Linear => Some(dose),
            Self::Log10 => (dose > 0.0).then(|| dose.log10()),
            Self::Ln => (dose > 0.0).then(|| dose.ln()),
        }
    }

    /// Transform back from the model scale to a dose.
    #[must_use]
    pub fn back(self, x: f64) -> f64 {
        match self {
            Self::Linear => x,
            Self::Log10 => 10f64.powf(x),
            Self::Ln => x.exp(),
        }
    }
}

/// Options for [`fit_dose_response`].
#[derive(Debug, Clone, PartialEq)]
pub struct DoseResponseOptions {
    /// Link: probit (default), logit or complementary log-log.
    pub link: Link,
    /// Dose scale (default log₁₀).
    pub scale: DoseScale,
    /// Response probabilities whose effective doses are wanted, each in
    /// (0, 1). Default 0.1, 0.5, 0.9.
    pub probabilities: Vec<f64>,
    /// Confidence (fiducial) level. Default 0.95.
    pub confidence_level: f64,
    /// Significance level of the χ² goodness-of-fit test above which the
    /// heterogeneity factor is applied. Default 0.05.
    pub heterogeneity_alpha: f64,
}

impl Default for DoseResponseOptions {
    fn default() -> Self {
        Self {
            link: Link::Probit,
            scale: DoseScale::Log10,
            probabilities: vec![0.1, 0.5, 0.9],
            confidence_level: 0.95,
            heterogeneity_alpha: 0.05,
        }
    }
}

/// One dose group after pooling rows with the same dose.
#[derive(Debug, Clone, PartialEq)]
pub struct DoseGroup {
    /// Dose.
    pub dose: f64,
    /// Dose on the model scale.
    pub x: f64,
    /// Subjects.
    pub trials: f64,
    /// Subjects responding.
    pub responded: f64,
    /// Fitted response probability `P̂`.
    pub expected_probability: f64,
    /// Pearson residual `(r − nP̂)/√(nP̂(1 − P̂))`.
    pub pearson_residual: f64,
}

/// An effective dose with its limits.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectiveDose {
    /// Response probability `p`.
    pub probability: f64,
    /// `m` on the model scale.
    pub x: f64,
    /// The dose `h⁻¹(m)`.
    pub dose: f64,
    /// Delta-method standard error of `m` (model scale).
    pub std_error: f64,
    /// Fiducial limits as doses, `None` when `g ≥ 1`.
    pub fiducial: Option<(f64, f64)>,
    /// Delta-method limits as doses (symmetric on the model scale).
    pub delta: (f64, f64),
}

/// A fitted dose–response model.
#[derive(Debug, Clone, PartialEq)]
pub struct DoseResponseFit {
    /// The binomial GLM (intercept α, slope β on the model scale).
    pub glm: GlmFit,
    /// Options used.
    pub options: DoseResponseOptions,
    /// Pooled dose groups, by increasing dose.
    pub groups: Vec<DoseGroup>,
    /// Pearson's goodness-of-fit χ².
    pub chi_square: f64,
    /// Its degrees of freedom `k − 2`.
    pub df: usize,
    /// Its p-value (`NaN` when `df = 0`).
    pub p_value: f64,
    /// Heterogeneity factor `χ²/df` applied to the variances (1 when not
    /// applied).
    pub heterogeneity: f64,
    /// `true` when the heterogeneity factor and t were used.
    pub heterogeneity_applied: bool,
    /// Critical value used for the limits.
    pub critical: f64,
    /// `g = t²v₂₂/β̂²`.
    pub g: f64,
    /// Effective doses, in the order of [`DoseResponseOptions::probabilities`].
    pub effective_doses: Vec<EffectiveDose>,
    /// Rows left out because the dose cannot be transformed (e.g. zero dose
    /// on a log scale).
    pub rows_left_out: usize,
}

impl DoseResponseFit {
    /// Intercept `α̂` and slope `β̂` on the model scale.
    #[must_use]
    pub fn coefficients(&self) -> (f64, f64) {
        (self.glm.coefficients[0].estimate, self.glm.coefficients[1].estimate)
    }

    /// Fitted response probability at a dose (`NaN` where the dose cannot
    /// be transformed).
    #[must_use]
    pub fn probability_at(&self, dose: f64) -> f64 {
        let (a, b) = self.coefficients();
        self.options.scale.forward(dose).map_or(f64::NAN, |x| self.options.link.inverse(a + b * x))
    }

    /// The effective dose for response probability `p` with its limits, at
    /// the fit's confidence level.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] unless `0 < p < 1`.
    pub fn effective_dose(&self, p: f64) -> Result<EffectiveDose> {
        effective_dose(&self.glm, self.options.link, self.options.scale, p, self.heterogeneity, self.critical)
    }
}

fn quantile_of_link(link: Link, p: f64) -> Result<f64> {
    Ok(match link {
        Link::Probit => Normal::standard().quantile(p)?,
        Link::Logit => (p / (1.0 - p)).ln(),
        Link::ComplementaryLogLog => (-(-p).ln_1p()).ln(),
        other => {
            return Err(StatError::InvalidArgument {
                message: format!("the {} link is not a dose–response link", other.label()),
            });
        }
    })
}

fn effective_dose(glm: &GlmFit, link: Link, scale: DoseScale, p: f64, h: f64, t: f64) -> Result<EffectiveDose> {
    if !(p > 0.0 && p < 1.0) {
        return Err(StatError::InvalidArgument {
            message: format!("a response probability must lie in (0, 1), got {p}"),
        });
    }
    let (a, b) = (glm.coefficients[0].estimate, glm.coefficients[1].estimate);
    let v = &glm.covariance;
    let (v11, v12, v22) = (h * v[(0, 0)], h * v[(0, 1)], h * v[(1, 1)]);
    let xi = quantile_of_link(link, p)?;
    let m = (xi - a) / b;
    let std_error = (v11 + 2.0 * m * v12 + m * m * v22).max(0.0).sqrt() / b.abs();
    let t2 = t * t;
    let qa = b * b - t2 * v22;
    let qb = 2.0 * (b * (a - xi) - t2 * v12);
    let qc = (a - xi) * (a - xi) - t2 * v11;
    let disc = qb * qb - 4.0 * qa * qc;
    let fiducial = (qa > 0.0 && disc >= 0.0).then(|| {
        let s = disc.sqrt();
        // Numerically stable roots.
        let q = -0.5 * (qb + s.copysign(qb));
        let (r1, r2) = if q == 0.0 { (0.0, 0.0) } else { (q / qa, qc / q) };
        let (lo, hi) = (r1.min(r2), r1.max(r2));
        (scale.back(lo), scale.back(hi))
    });
    let delta = (scale.back(m - t * std_error), scale.back(m + t * std_error));
    Ok(EffectiveDose { probability: p, x: m, dose: scale.back(m), std_error, fiducial, delta })
}

/// Fit a quantal dose–response model.
///
/// `dose`, `responded` and `trials` are parallel (one row per subject or per
/// group of subjects). Rows whose dose cannot be put on the model scale (zero
/// or negative doses on a log scale) are left out and counted.
///
/// # Errors
/// * [`StatError::DimensionMismatch`] if the slices differ in length.
/// * [`StatError::InvalidArgument`] for a link other than probit, logit or
///   complementary log-log, non-finite values, trials not positive, responses
///   outside `[0, trials]`, a probability outside (0, 1), or fewer than two
///   distinct usable doses.
/// * [`StatError::InvalidArgument`] with exactly two doses when either is at
///   0% or 100% response (the saturated line is then infinitely steep).
/// * Errors from [`fit_glm`] (e.g. complete separation fails to converge).
pub fn fit_dose_response(
    dose: &[f64],
    responded: &[f64],
    trials: &[f64],
    options: &DoseResponseOptions,
) -> Result<DoseResponseFit> {
    let n = dose.len();
    if responded.len() != n || trials.len() != n {
        return Err(StatError::DimensionMismatch {
            context: "dose, responses and trials",
            expected: n,
            found: if responded.len() == n { trials.len() } else { responded.len() },
        });
    }
    quantile_of_link(options.link, 0.5)?;
    if !(options.confidence_level > 0.0 && options.confidence_level < 1.0) {
        return Err(StatError::InvalidArgument { message: "the confidence level must lie in (0, 1)".into() });
    }
    if let Some(p) = options.probabilities.iter().find(|p| !(**p > 0.0 && **p < 1.0)) {
        return Err(StatError::InvalidArgument {
            message: format!("a response probability must lie in (0, 1), got {p}"),
        });
    }
    let mut pooled: Vec<(f64, f64, f64, f64)> = Vec::new();
    let mut rows_left_out = 0;
    for i in 0..n {
        let (d, r, m) = (dose[i], responded[i], trials[i]);
        if !(d.is_finite() && r.is_finite() && m.is_finite()) {
            return Err(StatError::NonFiniteInput { context: "dose–response data" });
        }
        if m <= 0.0 || r < 0.0 || r > m {
            return Err(StatError::InvalidArgument {
                message: "each row needs positive trials and between 0 and that many responses".into(),
            });
        }
        let Some(x) = options.scale.forward(d) else {
            rows_left_out += 1;
            continue;
        };
        match pooled.iter_mut().find(|g| g.0 == d) {
            Some(g) => {
                g.2 += m;
                g.3 += r;
            }
            None => pooled.push((d, x, m, r)),
        }
    }
    pooled.sort_by(|a, b| a.0.total_cmp(&b.0));
    if pooled.len() < 2 {
        return Err(StatError::InvalidArgument { message: "at least two different usable doses are needed".into() });
    }
    let k = pooled.len();
    let xs: Vec<f64> = pooled.iter().map(|g| g.1).collect();
    let props: Vec<f64> = pooled.iter().map(|g| g.3 / g.2).collect();
    let weights: Vec<f64> = pooled.iter().map(|g| g.2).collect();
    let x = Matrix::from_columns(&[&xs[..]])?;
    let glm_options = GlmOptions {
        link: Some(options.link),
        weights,
        names: vec![options.scale.label().to_owned()],
        confidence_level: options.confidence_level,
        ..GlmOptions::new(Family::Binomial)
    };
    // With two doses the model is saturated; fit_glm needs n > p, so fit
    // the expanded 0/1 rows instead in that case.
    let glm = if k > 2 { fit_glm(&x, &props, &glm_options)? } else { expanded_fit(&pooled, &glm_options)? };
    let groups: Vec<DoseGroup> = pooled
        .iter()
        .map(|&(dose, x, trials, responded)| {
            // Clamp as the GLM does: P̂ can round to exactly 0 or 1 far out
            // in the tails, which would make the residual 0/0.
            let p = options
                .link
                .inverse(glm.coefficients[0].estimate + glm.coefficients[1].estimate * x)
                .clamp(1e-15, 1.0 - 1e-15);
            let sd = (trials * p * (1.0 - p)).sqrt();
            DoseGroup {
                dose,
                x,
                trials,
                responded,
                expected_probability: p,
                pearson_residual: (responded - trials * p) / sd,
            }
        })
        .collect();
    let chi_square: f64 = groups.iter().map(|g| g.pearson_residual * g.pearson_residual).sum();
    let df = k - 2;
    let p_value = if df > 0 { ChiSquared::new(df as f64)?.sf(chi_square)? } else { f64::NAN };
    let heterogeneity_applied = df > 0 && p_value < options.heterogeneity_alpha;
    let heterogeneity = if heterogeneity_applied { chi_square / df as f64 } else { 1.0 };
    let upper = 1.0 - (1.0 - options.confidence_level) / 2.0;
    let critical = if heterogeneity_applied {
        StudentsT::new(df as f64)?.quantile(upper)?
    } else {
        Normal::standard().quantile(upper)?
    };
    let b = glm.coefficients[1].estimate;
    let g = critical * critical * heterogeneity * glm.covariance[(1, 1)] / (b * b);
    let effective_doses = options
        .probabilities
        .iter()
        .map(|&p| effective_dose(&glm, options.link, options.scale, p, heterogeneity, critical))
        .collect::<Result<Vec<_>>>()?;
    Ok(DoseResponseFit {
        glm,
        options: options.clone(),
        groups,
        chi_square,
        df,
        p_value,
        heterogeneity,
        heterogeneity_applied,
        critical,
        g,
        effective_doses,
        rows_left_out,
    })
}

/// Fit two dose groups as individual 0/1 rows (the grouped fit would have
/// no residual degrees of freedom), then report the fit per group.
fn expanded_fit(pooled: &[(f64, f64, f64, f64)], options: &GlmOptions) -> Result<GlmFit> {
    // Two doses give a saturated model whose line passes through both
    // observed proportions: a dose at 0% or 100% puts it at ±∞.
    if pooled.iter().any(|&(_, _, m, r)| r == 0.0 || r == m) {
        return Err(StatError::InvalidArgument {
            message: "with only two doses, each needs some but not all subjects responding: a dose at 0% or 100% \
                      response has no finite fit (add a dose between them)"
                .into(),
        });
    }
    let (mut xs, mut ys, mut ws) = (Vec::new(), Vec::new(), Vec::new());
    for &(_, x, m, r) in pooled {
        for (y, w) in [(1.0, r), (0.0, m - r)] {
            if w > 0.0 {
                xs.push(x);
                ys.push(y);
                ws.push(w);
            }
        }
    }
    let x = Matrix::from_columns(&[&xs[..]])?;
    let mut fit = fit_glm(&x, &ys, &GlmOptions { weights: ws, ..options.clone() })?;
    // The expanded rows give the Bernoulli deviance and count individual
    // rows; the grouped two-dose model is saturated, so its residual
    // deviance, Pearson χ² and residual df are all zero, and the null
    // deviance is the grouped G² against the pooled proportion on 1 df.
    let (total_m, total_r) = pooled.iter().fold((0.0, 0.0), |(m0, r0), g| (m0 + g.2, r0 + g.3));
    let p0 = total_r / total_m;
    let term = |o: f64, e: f64| if o > 0.0 { o * (o / e).ln() } else { 0.0 };
    fit.null_deviance =
        2.0 * pooled.iter().map(|&(_, _, m, r)| term(r, m * p0) + term(m - r, m * (1.0 - p0))).sum::<f64>();
    fit.deviance = 0.0;
    fit.pearson_chi_square = 0.0;
    fit.df_residual = 0;
    fit.df_null = 1;
    // Fitted values, likelihood and criteria per dose group (as the grouped
    // fit reports them), not per expanded row: the binomial likelihood keeps
    // its ln C(m, r) terms.
    let group_x = Matrix::from_columns(&[&pooled.iter().map(|g| g.1).collect::<Vec<_>>()[..]])?;
    fit.fitted = fit.predict(&group_x)?;
    fit.log_likelihood = pooled
        .iter()
        .zip(&fit.fitted)
        .map(|(&(_, _, m, r), &p)| {
            ln_gamma(m + 1.0) - ln_gamma(r + 1.0) - ln_gamma(m - r + 1.0) + r * p.ln() + (m - r) * (1.0 - p).ln()
        })
        .sum();
    let k = fit.coefficients.len() as f64;
    fit.aic = -2.0 * fit.log_likelihood + 2.0 * k;
    fit.bic = -2.0 * fit.log_likelihood + k * (pooled.len() as f64).ln();
    Ok(fit)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol * b.abs().max(1.0)
    }

    /// Martin's (1942) rotenone sprays on *Macrosiphoniella sanborni*, the
    /// worked example of Finney (1971, *Probit Analysis*, ch. 3–4): doses in
    /// mg/l, aphids sprayed and killed (the zero-dose control, 0 of 49
    /// killed, is included and left out on the log scale).
    fn rotenone() -> (Vec<f64>, Vec<f64>, Vec<f64>) {
        (
            vec![10.2, 7.7, 5.1, 3.8, 2.6, 0.0],
            vec![44.0, 42.0, 24.0, 16.0, 6.0, 0.0],
            vec![50.0, 49.0, 46.0, 48.0, 50.0, 49.0],
        )
    }

    /// Reference: maximum likelihood by Newton–Raphson on the binomial
    /// log-likelihood and Fieller limits, computed independently in Python
    /// (standard library only; `ref_glm.py` in the PR notes):
    /// probit α = −2.88746325636171, β = 4.21319675611494 on log₁₀ dose
    /// (Finney's probit scale adds 5: Y = 2.1125 + 4.2132x), ED50 =
    /// 4.84549178762459 mg/l with fiducial limits 4.36449216108095 and
    /// 5.35438615613947, χ² = 1.72887503165302 on 3 df.
    #[test]
    fn rotenone_probit_matches_reference() {
        let (d, r, n) = rotenone();
        let fit = fit_dose_response(&d, &r, &n, &DoseResponseOptions::default()).unwrap();
        assert_eq!(fit.rows_left_out, 1);
        assert_eq!(fit.groups.len(), 5);
        let (a, b) = fit.coefficients();
        assert!(close(a, -2.887_463_256_361_71, 1e-7), "{a}");
        assert!(close(b, 4.213_196_756_114_94, 1e-7), "{b}");
        assert!(close(fit.glm.coefficients[0].std_error, 0.351_039_879_809_45, 1e-6));
        assert!(close(fit.glm.coefficients[1].std_error, 0.480_561_751_814_927, 1e-6));
        assert!(close(fit.glm.deviance, 1.738_968_595_411_16, 1e-7));
        assert!(close(fit.chi_square, 1.728_875_031_653_02, 1e-7));
        assert_eq!(fit.df, 3);
        assert!(!fit.heterogeneity_applied);
        assert!(close(fit.g, 0.049_977_105_991_396_9, 1e-6));
        let want = [
            (0.1, 2.405_261_402_573_36, 1.888_612_240_707_33, 2.832_346_596_493_18),
            (0.5, 4.845_491_787_624_59, 4.364_492_161_080_95, 5.354_386_156_139_47),
            (0.9, 9.761_429_938_058_99, 8.399_066_970_182_4, 12.155_324_920_430_9),
        ];
        for (ed, (p, dose, lo, hi)) in fit.effective_doses.iter().zip(want) {
            assert_eq!(ed.probability, p);
            assert!(close(ed.dose, dose, 1e-7), "{} vs {dose}", ed.dose);
            let (l, h) = ed.fiducial.unwrap();
            assert!(close(l, lo, 1e-6) && close(h, hi, 1e-6), "{l} {h} vs {lo} {hi}");
            // Fiducial limits are asymmetric and wider than the delta method's
            // on the side away from the data's centre.
            assert!(l < ed.dose && ed.dose < h);
        }
        assert!(close(fit.probability_at(fit.effective_doses[1].dose), 0.5, 1e-12));
    }

    /// Same data with logit and complementary log-log links; references from
    /// the same independent Python fit.
    #[test]
    fn other_links_match_reference() {
        let (d, r, n) = rotenone();
        for (link, a, b, dev) in [
            (Link::Logit, -4.886_912_275_999_41, 7.146_177_555_935_41, 1.424_056_584_808_54),
            (Link::ComplementaryLogLog, -3.524_934_946_394_1, 4.450_178_781_854_54, 4.326_712_418_958_65),
        ] {
            let o = DoseResponseOptions { link, ..DoseResponseOptions::default() };
            let fit = fit_dose_response(&d, &r, &n, &o).unwrap();
            let (fa, fb) = fit.coefficients();
            assert!(close(fa, a, 1e-7) && close(fb, b, 1e-7), "{link:?}: {fa} {fb}");
            assert!(close(fit.glm.deviance, dev, 1e-7));
            // The ED50 solves F(α + βx) = ½.
            assert!(close(fit.probability_at(fit.effective_dose(0.5).unwrap().dose), 0.5, 1e-12));
        }
    }

    /// Individual 0/1 rows pool into the same groups and the same fit.
    #[test]
    fn individual_rows_pool_to_groups() {
        let (d, r, n) = rotenone();
        let (mut dd, mut rr, mut nn) = (Vec::new(), Vec::new(), Vec::new());
        for i in 0..d.len() {
            for j in 0..n[i] as usize {
                dd.push(d[i]);
                rr.push(if (j as f64) < r[i] { 1.0 } else { 0.0 });
                nn.push(1.0);
            }
        }
        let a = fit_dose_response(&d, &r, &n, &DoseResponseOptions::default()).unwrap();
        let b = fit_dose_response(&dd, &rr, &nn, &DoseResponseOptions::default()).unwrap();
        assert!(close(a.coefficients().1, b.coefficients().1, 1e-9));
        assert!(close(a.chi_square, b.chi_square, 1e-9));
    }

    /// Heterogeneity: with an over-dispersed series the variances are scaled
    /// by h = χ²/df and t(df) replaces z. Reference limits from Fieller's
    /// quadratic with the scaled covariance (independent Python, as above):
    /// χ² = 17.63829205313365 on 3 df, ED50 = 5.068703183050884 with limits
    /// 0.18248858613530983 and 31.336918840429842.
    #[test]
    fn heterogeneity_factor_widens_limits() {
        let d = [10.2, 7.7, 5.1, 3.8, 2.6];
        let r = [44.0, 30.0, 34.0, 10.0, 8.0];
        let n = [50.0, 49.0, 46.0, 48.0, 50.0];
        let fit = fit_dose_response(&d, &r, &n, &DoseResponseOptions::default()).unwrap();
        assert!(fit.heterogeneity_applied, "p = {}", fit.p_value);
        assert!(close(fit.chi_square, 17.638_292_053_133_65, 1e-7));
        let ed50 = &fit.effective_doses[1];
        assert!(close(ed50.dose, 5.068_703_183_050_884, 1e-7));
        let (lo, hi) = ed50.fiducial.unwrap();
        assert!(close(lo, 0.182_488_586_135_309_83, 1e-6) && close(hi, 31.336_918_840_429_842, 1e-6), "{lo} {hi}");
        assert!(close(fit.heterogeneity, fit.chi_square / 3.0, 1e-12));
        let t = StudentsT::new(3.0).unwrap().quantile(0.975).unwrap();
        assert!(close(fit.critical, t, 1e-12));
        let plain =
            fit_dose_response(&d, &r, &n, &DoseResponseOptions { heterogeneity_alpha: 0.0, ..Default::default() })
                .unwrap();
        let (a, b) = (fit.effective_doses[1].fiducial.unwrap(), plain.effective_doses[1].fiducial.unwrap());
        assert!(a.0 < b.0 && a.1 > b.1);
        assert!(close(fit.effective_doses[1].dose, plain.effective_doses[1].dose, 1e-12));
    }

    #[test]
    fn rejects_bad_input_and_flags_unbounded_limits() {
        let o = DoseResponseOptions::default();
        assert!(fit_dose_response(&[1.0, 2.0], &[1.0], &[2.0, 2.0], &o).is_err());
        assert!(fit_dose_response(&[1.0, 2.0, 3.0], &[1.0, 3.0, 1.0], &[2.0, 2.0, 2.0], &o).is_err());
        assert!(fit_dose_response(&[1.0, 1.0, 0.0], &[1.0, 1.0, 0.0], &[2.0, 2.0, 2.0], &o).is_err());
        let bad_link = DoseResponseOptions { link: Link::Log, ..DoseResponseOptions::default() };
        assert!(fit_dose_response(&[1.0, 2.0, 3.0], &[1.0, 1.0, 2.0], &[3.0, 3.0, 3.0], &bad_link).is_err());
        // A flat response: the slope is not significant, g ≥ 1, no fiducial limits.
        let fit = fit_dose_response(&[1.0, 2.0, 4.0, 8.0], &[5.0, 6.0, 5.0, 6.0], &[10.0; 4], &o).unwrap();
        assert!(fit.g >= 1.0);
        assert!(fit.effective_doses.iter().all(|e| e.fiducial.is_none()));
        // Two doses: the saturated model still fits (through the 0/1 rows).
        let two = fit_dose_response(&[1.0, 10.0], &[2.0, 8.0], &[10.0, 10.0], &o).unwrap();
        assert_eq!(two.df, 0);
        assert!(two.p_value.is_nan());
        assert!(close(two.probability_at(1.0), 0.2, 1e-8));
    }

    /// A high-dose group in which every subject responded drives P̂ to
    /// exactly 1.0 under the complementary log-log link; the fit must still
    /// report its χ². Reference (statsmodels 0.15 GLM, binomial cloglog on
    /// log₁₀ dose): α = −3.524935, β = 4.450179, Pearson χ² = 4.327225 on 4 df.
    #[test]
    fn group_at_certain_response_keeps_finite_chi_square() {
        let d = [10.2, 7.7, 5.1, 3.8, 2.6, 50.0];
        let r = [44.0, 42.0, 24.0, 16.0, 6.0, 50.0];
        let n = [50.0, 49.0, 46.0, 48.0, 50.0, 50.0];
        let o = DoseResponseOptions { link: Link::ComplementaryLogLog, ..DoseResponseOptions::default() };
        let fit = fit_dose_response(&d, &r, &n, &o).unwrap();
        let (a, b) = fit.coefficients();
        assert!(close(a, -3.524_935, 1e-5), "{a}");
        assert!(close(b, 4.450_179, 1e-5), "{b}");
        assert_eq!(fit.df, 4);
        assert!(close(fit.chi_square, 4.327_225, 1e-5), "{}", fit.chi_square);
    }

    /// Two doses: the grouped model is saturated, so the residual deviance
    /// and df are zero; the null deviance is the grouped G² against the
    /// pooled proportion 0.5: 2·Σ o·ln(o/e) over (2, 8, 8, 2) with e = 5.
    #[test]
    fn two_doses_report_grouped_deviance() {
        let fit = fit_dose_response(&[1.0, 10.0], &[2.0, 8.0], &[10.0, 10.0], &DoseResponseOptions::default()).unwrap();
        assert_eq!(fit.glm.deviance, 0.0);
        assert_eq!(fit.glm.df_residual, 0);
        assert_eq!(fit.glm.df_null, 1);
        let g2 = 2.0 * (2.0 * (2.0 * (2.0f64 / 5.0).ln() + 8.0 * (8.0f64 / 5.0).ln()));
        assert!(close(fit.glm.null_deviance, g2, 1e-12), "{}", fit.glm.null_deviance);
    }

    /// Two doses: the saturated fit passes through both observed proportions,
    /// and the fitted values and log-likelihood are per dose group,
    /// ℓ = Σ [ln C(m, r) + r ln p + (m − r) ln(1 − p)] (hand computation),
    /// not per expanded 0/1 row. A dose at 0% response is a clear error.
    #[test]
    fn two_doses_report_per_group_fit_and_refuse_separation() {
        let fit = fit_dose_response(&[1.0, 10.0], &[2.0, 8.0], &[10.0, 10.0], &DoseResponseOptions::default()).unwrap();
        assert_eq!(fit.glm.fitted.len(), 2);
        assert!(close(fit.glm.fitted[0], 0.2, 1e-8) && close(fit.glm.fitted[1], 0.8, 1e-8), "{:?}", fit.glm.fitted);
        let ll = 2.0 * (45f64.ln() + 2.0 * 0.2f64.ln() + 8.0 * 0.8f64.ln());
        assert!(close(fit.glm.log_likelihood, ll, 1e-8), "{} vs {ll}", fit.glm.log_likelihood);
        assert!(close(fit.glm.aic, -2.0 * ll + 4.0, 1e-8));
        let err = fit_dose_response(&[1.0, 10.0], &[0.0, 8.0], &[10.0, 10.0], &DoseResponseOptions::default());
        assert!(
            matches!(&err, Err(StatError::InvalidArgument { message }) if message.contains("0% or 100%")),
            "{err:?}"
        );
    }
}
