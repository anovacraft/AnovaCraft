//! Simulated responses for a design from random coefficients, with effect
//! sparsity, hierarchy and heredity.
//!
//! Before an experiment is run, simulating plausible responses shows how
//! well its analysis would find the effects that matter. The coefficients
//! follow the hierarchical prior of Chipman, Hamada & Wu (1997), "A Bayesian
//! variable-selection approach for analyzing designed experiments with
//! complex aliasing", *Technometrics* 39(4), in its generative form:
//!
//! * **Effect sparsity:** each main effect is active with probability `p`.
//! * **Heredity:** a higher-order term (an interaction, or a squared factor)
//!   is active with probability `p₁₁` when all its parent main effects are
//!   active (strong heredity), `p₀₁` when only some are (weak heredity) and
//!   `p₀₀` when none is. A squared factor has one parent, so only `p₁₁` and
//!   `p₀₀` apply to it ([`SparsityPrior::quadratic`] may give its own pair).
//! * **Hierarchy:** the defaults make higher-order terms rarer than main
//!   effects (`p₁₁ ≤ p`), the effect-hierarchy principle.
//! * **Sizes:** an active coefficient is drawn from `N(0, τ²)`, an inactive
//!   one from `N(0, (τ/c)²)` with `c` large (CHW's mixture prior), or is
//!   exactly zero when `inactive_sd` is 0.
//!
//! Each simulated response is `y = β₀ + Σ βₜ fₜ(x) + ε` over the design's
//! coded runs, with `ε ~ N(0, σ²)`. Draws come from one seeded generator, so a
//! simulation is reproducible.

use crate::design::Rng;
use crate::optimal::{ModelFormula, Term};
use stat_core::{Result, StatError};

/// Probabilities and sizes of the random coefficients.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SparsityPrior {
    /// `p`: probability that a main effect is active.
    pub main: f64,
    /// `p₁₁`: probability that a higher-order term is active when all its
    /// parents are.
    pub all_parents: f64,
    /// `p₀₁`: … when some but not all of its parents are.
    pub some_parents: f64,
    /// `p₀₀`: … when none of its parents is.
    pub no_parents: f64,
    /// For squared factors: (active parent, inactive parent); `None` uses
    /// `all_parents` and `no_parents`.
    pub quadratic: Option<(f64, f64)>,
    /// `τ`: standard deviation of an active coefficient.
    pub effect_sd: f64,
    /// Standard deviation of an inactive coefficient (`τ/c`; 0 for exactly
    /// zero).
    pub inactive_sd: f64,
    /// `σ`: standard deviation of the noise.
    pub noise_sd: f64,
    /// `β₀`.
    pub intercept: f64,
}

impl Default for SparsityPrior {
    /// The values of Chipman, Hamada & Wu (1997, §3): `p = 0.25`,
    /// `p₁₁ = 0.25`, `p₀₁ = 0.10`, `p₀₀ = 0.01`; `τ = 2σ`, inactive
    /// coefficients exactly zero, `σ = 1`, `β₀ = 0`.
    fn default() -> Self {
        Self {
            main: 0.25,
            all_parents: 0.25,
            some_parents: 0.10,
            no_parents: 0.01,
            quadratic: None,
            effect_sd: 2.0,
            inactive_sd: 0.0,
            noise_sd: 1.0,
            intercept: 0.0,
        }
    }
}

impl SparsityPrior {
    /// Check that the probabilities lie in [0, 1] and the SDs are finite and
    /// not negative.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] naming the bad value.
    pub fn validate(&self) -> Result<()> {
        let mut probabilities = vec![self.main, self.all_parents, self.some_parents, self.no_parents];
        if let Some((a, b)) = self.quadratic {
            probabilities.extend([a, b]);
        }
        if probabilities.iter().any(|p| !(0.0..=1.0).contains(p)) {
            return Err(invalid("probabilities must be between 0 and 1"));
        }
        if [self.effect_sd, self.inactive_sd, self.noise_sd].iter().any(|s| !(s.is_finite() && *s >= 0.0)) {
            return Err(invalid("standard deviations must be finite and not negative"));
        }
        if !self.intercept.is_finite() {
            return Err(invalid("the intercept must be a finite number"));
        }
        Ok(())
    }

    /// Probability that `term` is active given which main effects are.
    #[must_use]
    pub fn probability(&self, term: &Term, active_main: &[bool]) -> f64 {
        let parents: Vec<usize> = (0..term.powers.len()).filter(|&j| term.powers[j] > 0).collect();
        if term.degree() == 1 {
            return self.main;
        }
        let on = parents.iter().filter(|&&j| active_main[j]).count();
        if parents.len() == 1 {
            let (yes, no) = self.quadratic.unwrap_or((self.all_parents, self.no_parents));
            return if on == 1 { yes } else { no };
        }
        if on == parents.len() {
            self.all_parents
        } else if on > 0 {
            self.some_parents
        } else {
            self.no_parents
        }
    }
}

/// One simulated response and the coefficients that made it.
#[derive(Debug, Clone, PartialEq)]
pub struct SimulatedDesignResponse {
    /// Coefficient of each model term (the intercept first), in the order of
    /// [`ModelFormula::terms`].
    pub coefficients: Vec<f64>,
    /// Whether each term is active (the intercept always is).
    pub active: Vec<bool>,
    /// The response at each run.
    pub values: Vec<f64>,
}

/// Most simulated responses at once.
pub const MAX_SIMULATIONS: usize = 1000;

/// Simulate `count` responses at coded `runs` for `formula` (see the module
/// documentation), from `seed`.
///
/// # Errors
/// [`StatError::InvalidArgument`] for no runs, a run of the wrong length,
/// a count of 0 or above [`MAX_SIMULATIONS`], or an invalid prior.
pub fn simulate_responses(
    runs: &[Vec<f64>],
    formula: &ModelFormula,
    prior: &SparsityPrior,
    count: usize,
    seed: u64,
) -> Result<Vec<SimulatedDesignResponse>> {
    prior.validate()?;
    let k = formula.n_factors();
    if runs.is_empty() {
        return Err(invalid("the design has no runs"));
    }
    if runs.iter().any(|r| r.len() != k || r.iter().any(|v| !v.is_finite())) {
        return Err(invalid(format!("every run needs a setting for each of the {k} factors")));
    }
    if !(1..=MAX_SIMULATIONS).contains(&count) {
        return Err(invalid(format!("simulate 1 to {MAX_SIMULATIONS} responses")));
    }
    let rows: Vec<Vec<f64>> = runs.iter().map(|r| formula.expand(r)).collect();
    let terms = formula.terms();
    let mut rng = Rng::new(seed);
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let mut active = vec![false; terms.len()];
        let mut active_main = vec![false; k];
        // Main effects first: their activity decides the heredity of the rest.
        for (t, term) in terms.iter().enumerate() {
            if term.degree() == 1 {
                let on = rng.next_f64() < prior.main;
                active[t] = on;
                let j = term.powers.iter().position(|&p| p > 0).unwrap_or(0);
                active_main[j] = on;
            }
        }
        for (t, term) in terms.iter().enumerate() {
            if term.degree() > 1 {
                active[t] = rng.next_f64() < prior.probability(term, &active_main);
            }
        }
        let coefficients: Vec<f64> = terms
            .iter()
            .zip(&mut active)
            .map(|(term, on)| {
                if term.degree() == 0 {
                    *on = true;
                    prior.intercept
                } else if *on {
                    prior.effect_sd * rng.normal()
                } else if prior.inactive_sd > 0.0 {
                    prior.inactive_sd * rng.normal()
                } else {
                    0.0
                }
            })
            .collect();
        let values = rows
            .iter()
            .map(|row| row.iter().zip(&coefficients).map(|(x, b)| x * b).sum::<f64>() + prior.noise_sd * rng.normal())
            .collect();
        out.push(SimulatedDesignResponse { coefficients, active, values });
    }
    Ok(out)
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::factorial::two_level_factorial;

    /// Over many simulations, each term is active as often as the prior
    /// says: `p` for a main effect, `p²p₁₁ + 2p(1 − p)p₀₁ + (1 − p)²p₀₀` for a
    /// two-factor interaction and `p·p₁₁ + (1 − p)p₀₀` for a squared factor;
    /// active coefficients have SD τ; the noise has SD σ.
    #[test]
    fn activity_frequencies_follow_the_prior() {
        let d = two_level_factorial(3).unwrap();
        let mut runs = d.runs.clone();
        runs.push(vec![0.0; 3]);
        let f = ModelFormula::quadratic(3);
        let prior = SparsityPrior {
            main: 0.4,
            all_parents: 0.5,
            some_parents: 0.2,
            no_parents: 0.05,
            ..SparsityPrior::default()
        };
        let n = MAX_SIMULATIONS;
        let mut sims = Vec::new();
        for seed in 1..=40 {
            sims.extend(simulate_responses(&runs, &f, &prior, n, seed).unwrap());
        }
        let total = sims.len() as f64;
        let (p, p11, p01, p00) = (prior.main, prior.all_parents, prior.some_parents, prior.no_parents);
        let interaction = p * p * p11 + 2.0 * p * (1.0 - p) * p01 + (1.0 - p) * (1.0 - p) * p00;
        let square = p * p11 + (1.0 - p) * p00;
        let mut active_coefficients = Vec::new();
        for (t, term) in f.terms().iter().enumerate() {
            let rate = sims.iter().filter(|s| s.active[t]).count() as f64 / total;
            let want = match (term.degree(), term.powers.iter().filter(|&&q| q > 0).count()) {
                (0, _) => 1.0,
                (1, _) => p,
                (2, 2) => interaction,
                _ => square,
            };
            let se = (want * (1.0 - want) / total).sqrt().max(1e-12);
            assert!((rate - want).abs() < 4.0 * se, "term {t}: {rate} vs {want}");
            if term.degree() > 0 {
                active_coefficients.extend(sims.iter().filter(|s| s.active[t]).map(|s| s.coefficients[t]));
                assert!(sims.iter().filter(|s| !s.active[t]).all(|s| s.coefficients[t] == 0.0));
            }
        }
        let m = active_coefficients.len() as f64;
        let sd = (active_coefficients.iter().map(|b| b * b).sum::<f64>() / m).sqrt();
        assert!((sd / prior.effect_sd - 1.0).abs() < 0.02, "{sd}");
        // Residuals from the true coefficients are the noise.
        let residuals: Vec<f64> = sims
            .iter()
            .flat_map(|s| {
                runs.iter()
                    .zip(&s.values)
                    .map(|(x, y)| y - f.expand(x).iter().zip(&s.coefficients).map(|(a, b)| a * b).sum::<f64>())
            })
            .collect();
        let sigma = (residuals.iter().map(|e| e * e).sum::<f64>() / residuals.len() as f64).sqrt();
        assert!((sigma - 1.0).abs() < 0.01, "{sigma}");
    }

    #[test]
    fn strong_heredity_only_and_reproducibility() {
        let runs = two_level_factorial(4).unwrap().runs;
        let f = ModelFormula::interactions(4);
        let prior = SparsityPrior { some_parents: 0.0, no_parents: 0.0, all_parents: 1.0, ..SparsityPrior::default() };
        let sims = simulate_responses(&runs, &f, &prior, 200, 3).unwrap();
        for s in &sims {
            for (t, term) in f.terms().iter().enumerate().filter(|(_, t)| t.degree() == 2) {
                let parents: Vec<usize> = (0..4).filter(|&j| term.powers[j] > 0).collect();
                let both = parents.iter().all(|&j| {
                    let main = f.terms().iter().position(|u| u.degree() == 1 && u.powers[j] == 1).unwrap();
                    s.active[main]
                });
                assert_eq!(s.active[t], both, "strong heredity with p₁₁ = 1");
            }
        }
        assert_eq!(sims, simulate_responses(&runs, &f, &prior, 200, 3).unwrap());
        assert!(simulate_responses(&runs, &f, &prior, 0, 3).is_err());
        assert!(simulate_responses(&runs, &f, &SparsityPrior { main: 1.5, ..prior }, 1, 3).is_err());
        assert!(simulate_responses(&[vec![0.0]], &f, &prior, 1, 3).is_err());
    }
}
