//! Response distributions of a mixed model, each with its link.

use stat_core::special::ln_gamma;

/// Response distribution and link of a mixed model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MixedFamily {
    /// Normal response, identity link (a linear mixed model).
    Normal,
    /// Proportion of successes out of a number of trials (the prior weight;
    /// 1 for a 0/1 response), logit link.
    Binomial,
    /// Counts, log link.
    Poisson,
    /// Positive continuous response with constant coefficient of variation,
    /// log link; the dispersion (squared coefficient of variation) is estimated.
    Gamma,
    /// Overdispersed counts, variance `μ + μ²/k`, log link; the shape `k` is
    /// estimated.
    NegativeBinomial,
    /// Ordered categories (codes `0 … K−1`, lowest first), cumulative logit
    /// link: `logit P(Y ≤ k) = αₖ − η` with thresholds `α₁ < … < α_{K−1}`
    /// and no intercept in the fixed effects (proportional odds; Hedeker &
    /// Gibbons 1994). Fitted by [`crate::fit_mixed`] through its own
    /// likelihood, so the mean-based methods below treat it as a binomial
    /// response on the logit scale.
    Ordinal,
}

impl MixedFamily {
    /// Every family.
    pub const ALL: [MixedFamily; 6] = [
        MixedFamily::Normal,
        MixedFamily::Binomial,
        MixedFamily::Poisson,
        MixedFamily::Gamma,
        MixedFamily::NegativeBinomial,
        MixedFamily::Ordinal,
    ];

    /// Display name with the link.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "Normal (identity link)",
            Self::Binomial => "Binomial (logit link)",
            Self::Poisson => "Poisson (log link)",
            Self::Gamma => "Gamma (log link)",
            Self::NegativeBinomial => "Negative binomial (log link)",
            Self::Ordinal => "Ordinal (cumulative logit link)",
        }
    }

    /// Name used in saved files.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Binomial => "binomial",
            Self::Poisson => "poisson",
            Self::Gamma => "gamma",
            Self::NegativeBinomial => "negative_binomial",
            Self::Ordinal => "ordinal",
        }
    }

    /// The family with this [`MixedFamily::key`].
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.key() == key)
    }

    /// `true` for families with an estimated dispersion or shape beyond the
    /// variance components (normal σ², Gamma dispersion, negative binomial k).
    #[must_use]
    pub fn has_scale(self) -> bool {
        matches!(self, Self::Normal | Self::Gamma | Self::NegativeBinomial)
    }

    /// Mean from the linear predictor.
    #[must_use]
    pub fn inverse_link(self, eta: f64) -> f64 {
        match self {
            Self::Normal => eta,
            Self::Binomial | Self::Ordinal => {
                // Bounded away from 0 and 1 so logs stay finite.
                let e = eta.clamp(-35.0, 35.0);
                1.0 / (1.0 + (-e).exp())
            }
            Self::Poisson | Self::Gamma | Self::NegativeBinomial => eta.clamp(-700.0, 700.0).exp(),
        }
    }

    /// Linear predictor from the mean.
    #[must_use]
    pub fn link(self, mu: f64) -> f64 {
        match self {
            Self::Normal => mu,
            Self::Binomial | Self::Ordinal => (mu / (1.0 - mu)).ln(),
            Self::Poisson | Self::Gamma | Self::NegativeBinomial => mu.ln(),
        }
    }

    /// `dμ/dη`.
    #[must_use]
    pub fn mu_eta(self, mu: f64) -> f64 {
        match self {
            Self::Normal => 1.0,
            Self::Binomial | Self::Ordinal => (mu * (1.0 - mu)).max(f64::MIN_POSITIVE),
            Self::Poisson | Self::Gamma | Self::NegativeBinomial => mu.max(f64::MIN_POSITIVE),
        }
    }

    /// Variance function `V(μ)` (for the negative binomial with shape `k`;
    /// the Gamma variance is `φ μ²` with the dispersion applied separately).
    #[must_use]
    pub fn variance(self, mu: f64, k: f64) -> f64 {
        match self {
            Self::Normal => 1.0,
            Self::Binomial | Self::Ordinal => (mu * (1.0 - mu)).max(f64::MIN_POSITIVE),
            Self::Poisson => mu.max(f64::MIN_POSITIVE),
            Self::Gamma => (mu * mu).max(f64::MIN_POSITIVE),
            Self::NegativeBinomial => (mu + mu * mu / k).max(f64::MIN_POSITIVE),
        }
    }

    /// Working (Fisher scoring) weight of a row with prior weight `w`:
    /// `w (dμ/dη)² / (φ V(μ))`, where `scale` is the Gamma dispersion φ or the
    /// negative binomial shape `k` (ignored otherwise).
    #[must_use]
    pub fn working_weight(self, mu: f64, w: f64, scale: f64) -> f64 {
        let d = self.mu_eta(mu);
        match self {
            Self::Gamma => w * d * d / (scale * self.variance(mu, 1.0)),
            Self::NegativeBinomial => w * d * d / self.variance(mu, scale),
            _ => w * d * d / self.variance(mu, 1.0),
        }
    }

    /// Log density (or probability) of response `y` with mean `mu`, prior
    /// weight `w` and `scale` as in [`MixedFamily::working_weight`] (the
    /// normal family is handled separately and returns the density with unit
    /// variance here).
    #[must_use]
    pub fn log_density(self, y: f64, mu: f64, w: f64, scale: f64) -> f64 {
        let xlogy = |x: f64, y: f64| if x == 0.0 { 0.0 } else { x * y.ln() };
        match self {
            Self::Normal => -0.5 * ((2.0 * std::f64::consts::PI).ln() - w.ln() + w * (y - mu) * (y - mu)),
            // Ordinal rows have their own likelihood (see `fit::ordinal_row`).
            Self::Ordinal => 0.0,
            Self::Binomial => {
                // `w` trials with `w y` successes.
                let n = w;
                let s = (w * y).round();
                let mu = mu.clamp(1e-300, 1.0 - 1e-16);
                ln_gamma(n + 1.0) - ln_gamma(s + 1.0) - ln_gamma(n - s + 1.0) + xlogy(s, mu) + xlogy(n - s, 1.0 - mu)
            }
            Self::Poisson => {
                // Prior weights scale the log-likelihood (as in a GLM).
                w * (xlogy(y, mu.max(1e-300)) - mu - ln_gamma(y + 1.0))
            }
            Self::Gamma => {
                let shape = w / scale;
                let mu = mu.max(1e-300);
                shape * (shape / mu).ln() + (shape - 1.0) * y.ln() - shape * y / mu - ln_gamma(shape)
            }
            Self::NegativeBinomial => {
                let k = scale;
                let mu = mu.max(1e-300);
                w * (ln_gamma(y + k) - ln_gamma(k) - ln_gamma(y + 1.0)
                    + k * (k / (k + mu)).ln()
                    + xlogy(y, mu / (k + mu)))
            }
        }
    }

    /// A starting mean for response `y` (inside the family's range).
    #[must_use]
    pub fn start_mean(self, y: f64, w: f64) -> f64 {
        match self {
            Self::Normal => y,
            Self::Binomial | Self::Ordinal => (w * y + 0.5) / (w + 1.0),
            Self::Poisson | Self::NegativeBinomial => y + 0.1,
            Self::Gamma => y,
        }
    }

    /// Why `y` (with prior weight `w`) cannot be modelled, if it cannot.
    #[must_use]
    pub fn check(self, y: f64, w: f64) -> Option<&'static str> {
        if !(y.is_finite() && w.is_finite() && w > 0.0) {
            return Some("responses and weights must be finite, weights positive");
        }
        match self {
            Self::Binomial if !(0.0..=1.0).contains(&y) => Some("a binomial response must be a proportion from 0 to 1"),
            Self::Binomial if ((w * y) - (w * y).round()).abs() > 1e-6 * w.max(1.0) => {
                Some("a binomial response times its number of trials must be a whole number of successes")
            }
            Self::Poisson | Self::NegativeBinomial if y < 0.0 => Some("a count response must be non-negative"),
            Self::Gamma if y <= 0.0 => Some("a Gamma response must be positive"),
            Self::Ordinal if y < 0.0 || y.fract() != 0.0 => {
                Some("an ordinal response must be a category code 0, 1, 2, …")
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Numerically integrate/sum the density to 1 and check its mean.
    #[test]
    fn densities_are_normalised_with_the_right_mean() {
        // Binomial with 7 trials.
        let (mut total, mut mean) = (0.0, 0.0);
        for s in 0..=7 {
            let p = MixedFamily::Binomial.log_density(s as f64 / 7.0, 0.3, 7.0, 1.0).exp();
            total += p;
            mean += p * s as f64 / 7.0;
        }
        assert!((total - 1.0).abs() < 1e-12 && (mean - 0.3).abs() < 1e-12);
        for (family, scale) in [(MixedFamily::Poisson, 1.0), (MixedFamily::NegativeBinomial, 2.5)] {
            let (mut total, mut mean, mut m2) = (0.0, 0.0, 0.0);
            for y in 0..400 {
                let p = family.log_density(y as f64, 4.2, 1.0, scale).exp();
                total += p;
                mean += p * y as f64;
                m2 += p * (y as f64) * (y as f64);
            }
            assert!((total - 1.0).abs() < 1e-10, "{family:?}");
            assert!((mean - 4.2).abs() < 1e-9, "{family:?}");
            let var = m2 - mean * mean;
            assert!((var - family.variance(4.2, scale)).abs() < 1e-7, "{family:?} variance {var}");
        }
        // Gamma, dispersion 0.4 (shape 2.5), mean 3: trapezoid integral.
        let (mut total, mut mean, mut m2) = (0.0, 0.0, 0.0);
        let h = 1e-3;
        for i in 1..200_000 {
            let y = i as f64 * h;
            let p = MixedFamily::Gamma.log_density(y, 3.0, 1.0, 0.4).exp();
            total += p * h;
            mean += p * y * h;
            m2 += p * y * y * h;
        }
        assert!((total - 1.0).abs() < 1e-6 && (mean - 3.0).abs() < 1e-5);
        assert!((m2 - mean * mean - 0.4 * 9.0).abs() < 1e-4);
        // Normal with weight 4: variance 1/4.
        let d = MixedFamily::Normal.log_density(1.0, 0.0, 4.0, 1.0);
        assert!((d - (-0.5 * (2.0 * std::f64::consts::PI / 4.0).ln() - 2.0)).abs() < 1e-14);
    }

    #[test]
    fn links_invert_and_keys_round_trip() {
        for f in MixedFamily::ALL {
            assert_eq!(MixedFamily::from_key(f.key()), Some(f));
            let mu = if matches!(f, MixedFamily::Binomial | MixedFamily::Ordinal) { 0.27 } else { 2.7 };
            assert!((f.inverse_link(f.link(mu)) - mu).abs() < 1e-12);
            // dμ/dη by finite differences.
            let eta = f.link(mu);
            let num = (f.inverse_link(eta + 1e-6) - f.inverse_link(eta - 1e-6)) / 2e-6;
            assert!((num - f.mu_eta(mu)).abs() < 1e-6);
        }
        assert!(MixedFamily::Binomial.check(0.5, 3.0).is_some());
        assert!(MixedFamily::Binomial.check(2.0 / 3.0, 3.0).is_none());
        assert!(MixedFamily::Gamma.check(0.0, 1.0).is_some());
        assert!(MixedFamily::Poisson.check(-1.0, 1.0).is_some());
        assert!(MixedFamily::Normal.check(f64::NAN, 1.0).is_some());
    }
}
