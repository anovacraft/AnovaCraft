//! The sampler and its diagnostics against independent references.
//!
//! * Diagnostics of fixed draws (`tests/data/diagnostics_draws.csv`) against
//!   R's posterior package 1.5 (`tests/data/diagnostics_reference.R`), to 1e-8.
//! * Conjugate posteriors recovered by NUTS within Monte Carlo error.
//! * Simulation-based calibration (Talts et al. 2018, arXiv:1804.06788):
//!   ranks of the true parameter among posterior draws are uniform.
//! * Bayesian GLMs against long PyMC 6.3 runs (`tests/data/reference_cases.py`):
//!   posterior means within 4 combined Monte Carlo standard errors.

use stat_bayes::diagnostics::{ess_bulk, ess_tail, rhat};
use stat_bayes::nuts::{NutsSettings, Target, sample};

fn read_draws() -> Vec<Vec<Vec<f64>>> {
    let text =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/diagnostics_draws.csv")).unwrap();
    let mut out = vec![vec![Vec::new(); 4]; 3];
    for line in text.lines().skip(1) {
        let f: Vec<&str> = line.split(',').collect();
        let c: usize = f[0].parse().unwrap();
        for p in 0..3 {
            out[p][c].push(f[2 + p].parse::<f64>().unwrap());
        }
    }
    out
}

#[test]
fn diagnostics_match_posterior_package() {
    let d = read_draws();
    let want = [
        (1.000_737_639_555_3, 603.041_378_924_122, 1_052.253_193_218_88, 0.046_977_593_422_363),
        (1.021_750_596_449_75, 95.154_311_472_517, 187.809_531_578_644, 0.234_753_918_464_507),
        (1.036_216_404_246_81, 88.856_918_651_201_7, 68.980_899_355_754_2, 0.088_284_995_644_233_1),
    ];
    for (p, w) in want.iter().enumerate() {
        let close = |label: &str, got: f64, want: f64| {
            assert!((got / want - 1.0).abs() < 1e-8, "param {p} {label}: {got} vs posterior {want}");
        };
        close("rhat", rhat(&d[p]), w.0);
        close("ess_bulk", ess_bulk(&d[p]), w.1);
        close("ess_tail", ess_tail(&d[p]), w.2);
        close("mcse_mean", stat_bayes::diagnostics::diagnose(&d[p]).mcse_mean, w.3);
    }
}

/// Normal data with known variance and a normal prior on the mean: the
/// posterior is normal with precision τ₀ + n/σ².
struct NormalMean {
    y: Vec<f64>,
    sigma: f64,
    prior: (f64, f64),
}

impl Target for NormalMean {
    fn dim(&self) -> usize {
        1
    }
    fn log_density_gradient(&self, theta: &[f64], grad: &mut [f64]) -> f64 {
        let mu = theta[0];
        let mut lp = -0.5 * ((mu - self.prior.0) / self.prior.1).powi(2);
        grad[0] = -(mu - self.prior.0) / self.prior.1.powi(2);
        for y in &self.y {
            lp -= 0.5 * ((y - mu) / self.sigma).powi(2);
            grad[0] += (y - mu) / self.sigma.powi(2);
        }
        lp
    }
}

#[test]
fn conjugate_normal_recovered() {
    let y = vec![2.1, 1.4, 3.3, 2.8, 1.9, 2.5];
    let t = NormalMean { y: y.clone(), sigma: 0.8, prior: (0.0, 3.0) };
    let prec = 1.0 / 9.0 + 6.0 / 0.64;
    let mean = (y.iter().sum::<f64>() / 0.64) / prec;
    let chains = sample(&t, &NutsSettings { seed: 7, ..NutsSettings::default() }).unwrap();
    let per: Vec<Vec<f64>> = chains.iter().map(|c| c.draws.iter().map(|d| d[0]).collect()).collect();
    let d = stat_bayes::diagnostics::diagnose(&per);
    let all: Vec<f64> = per.iter().flatten().copied().collect();
    let m = all.iter().sum::<f64>() / all.len() as f64;
    let sd = (all.iter().map(|v| (v - m).powi(2)).sum::<f64>() / (all.len() - 1) as f64).sqrt();
    assert!((m - mean).abs() < 4.0 * d.mcse_mean, "{m} vs {mean} (MCSE {})", d.mcse_mean);
    assert!((sd * prec.sqrt() - 1.0).abs() < 0.05, "sd {sd}");
    assert!(d.rhat < 1.01 && ess_bulk(&per) > 1000.0);
}

#[test]
fn simulation_based_calibration() {
    // Draw μ from the prior, data given μ, then the rank of μ among 99
    // thinned posterior draws; over 200 replications the ranks (0..=99) in
    // ten bins must pass a chi-square test of uniformity at the 0.1% level.
    let mut rng = stat_core::rng::Rng::new(2024);
    let mut bins = [0usize; 10];
    let reps = 200;
    for r in 0..reps {
        let mu = 3.0 * rng.normal();
        let y: Vec<f64> = (0..5).map(|_| mu + 0.8 * rng.normal()).collect();
        let t = NormalMean { y, sigma: 0.8, prior: (0.0, 3.0) };
        let chains =
            sample(&t, &NutsSettings { chains: 1, warmup: 300, draws: 495, seed: 100 + r, ..NutsSettings::default() })
                .unwrap();
        let draws: Vec<f64> = chains[0].draws.iter().step_by(5).map(|d| d[0]).collect();
        let rank = draws.iter().filter(|&&d| d < mu).count();
        bins[(rank * 10 / 100).min(9)] += 1;
    }
    let e = reps as f64 / 10.0;
    let chi2: f64 = bins.iter().map(|&b| (b as f64 - e).powi(2) / e).sum();
    // χ²₉ 99.9% point is 27.88.
    assert!(chi2 < 27.88, "ranks not uniform: {bins:?} (χ² = {chi2})");
    let _ = ess_tail;
}

use stat_bayes::glm::{
    BayesGlmFit, BayesGlmModel, CoefficientPrior, GlmFamily, GroupTerm, Priors, ScalePrior, fit_bayes_glm,
};

/// `(name, PyMC mean, PyMC sd, PyMC MCSE)`: means must agree within four
/// combined MCSEs and SDs within `sd_tol` relative.
fn check(fit: &BayesGlmFit, want: &[(&str, f64, f64, f64)], sd_tol: f64) {
    for &(name, mean, sd, mcse) in want {
        let p = fit.parameters.iter().find(|p| p.name == name).unwrap_or_else(|| panic!("no parameter {name}"));
        let tol = 4.0 * (p.diagnostics.mcse_mean.powi(2) + mcse * mcse).sqrt();
        assert!((p.mean - mean).abs() < tol, "{name}: mean {} vs PyMC {mean} (tolerance {tol})", p.mean);
        assert!((p.sd / sd - 1.0).abs() < sd_tol, "{name}: sd {} vs PyMC {sd}", p.sd);
        assert!(p.diagnostics.rhat < 1.01, "{name}: R̂ {}", p.diagnostics.rhat);
    }
}

fn long_run(seed: u64) -> NutsSettings {
    NutsSettings { draws: 5000, seed, target_accept: 0.95, ..NutsSettings::default() }
}

#[test]
fn eight_schools_matches_pymc() {
    let y = vec![28., 8., -3., 7., -1., 1., 18., 12.];
    let sd = vec![15., 10., 16., 11., 9., 11., 10., 18.];
    let m = BayesGlmModel {
        y,
        x: Vec::new(),
        x_names: Vec::new(),
        trials: Vec::new(),
        offset: Vec::new(),
        known_sd: Some(sd),
        family: GlmFamily::Normal,
        groups: vec![GroupTerm {
            name: "School".into(),
            levels: (1..=8).map(|i| i.to_string()).collect(),
            codes: (0..8).collect(),
            effects: vec!["Intercept".into()],
            z: vec![vec![1.0; 8]],
        }],
    };
    let priors = Priors {
        intercept_location: Some(0.0),
        intercept_scale: 5.0,
        sd: ScalePrior::HalfCauchy,
        sd_scale: 5.0,
        autoscale: false,
        ..Priors::default()
    };
    let fit = fit_bayes_glm(&m, &priors, &long_run(11), 0.95).unwrap();
    // PyMC: 4 × 25,000 draws (reference_cases.py).
    check(
        &fit,
        &[("Intercept", 4.41148, 3.31489, 0.00902), ("SD(Intercept | School)", 3.60285, 3.23093, 0.01233)],
        0.06,
    );
    // School effects θⱼ − μ: compare θⱼ's PyMC mean less μ's.
    let theta = [6.20312, 4.95771, 3.93725, 4.78286, 3.62444, 4.04878, 6.29187, 4.86879];
    for (j, t) in theta.iter().enumerate() {
        let p = fit.parameters.iter().find(|p| p.name == format!("School[{}]: Intercept", j + 1)).unwrap();
        let mu = fit.parameters.iter().find(|p| p.name == "Intercept").unwrap();
        let tol = 4.0 * (p.diagnostics.mcse_mean.powi(2) + mu.diagnostics.mcse_mean.powi(2) + 0.0003).sqrt();
        assert!((p.mean - (t - 4.41148)).abs() < tol, "school {}: {} vs {}", j + 1, p.mean, t - 4.41148);
    }
}

#[test]
fn logistic_matches_pymc() {
    let mpg = [
        21.0, 21.0, 22.8, 21.4, 18.7, 18.1, 14.3, 24.4, 22.8, 19.2, 17.8, 16.4, 17.3, 15.2, 10.4, 10.4, 14.7, 32.4,
        30.4, 33.9, 21.5, 15.5, 15.2, 13.3, 19.2, 27.3, 26.0, 30.4, 15.8, 19.7, 15.0, 21.4,
    ];
    let wt = [
        2.62, 2.875, 2.32, 3.215, 3.44, 3.46, 3.57, 3.19, 3.15, 3.44, 3.44, 4.07, 3.73, 3.78, 5.25, 5.424, 5.345, 2.2,
        1.615, 1.835, 2.465, 3.52, 3.435, 3.84, 3.845, 1.935, 2.14, 1.513, 3.17, 2.77, 3.57, 2.78,
    ];
    let vs = [
        0., 0., 1., 1., 0., 1., 0., 1., 1., 1., 1., 0., 0., 0., 0., 0., 0., 1., 1., 1., 1., 0., 0., 0., 0., 1., 0., 1.,
        0., 0., 0., 1.,
    ];
    let m = BayesGlmModel {
        y: vs.to_vec(),
        x: vec![mpg.to_vec(), wt.to_vec()],
        x_names: vec!["mpg".into(), "wt".into()],
        trials: Vec::new(),
        offset: Vec::new(),
        known_sd: None,
        family: GlmFamily::Binomial,
        groups: Vec::new(),
    };
    let priors = Priors {
        coefficient: CoefficientPrior::Normal,
        coefficient_scale: 2.5,
        intercept_location: Some(0.0),
        intercept_scale: 2.5,
        autoscale: false,
        ..Priors::default()
    };
    let fit = fit_bayes_glm(&m, &priors, &long_run(12), 0.95).unwrap();
    check(
        &fit,
        &[
            ("Intercept", -13.72156, 8.32383, 0.04101),
            ("mpg", 0.60176, 0.26553, 0.00129),
            ("wt", 0.46114, 1.15108, 0.00540),
        ],
        0.05,
    );
}

#[test]
fn poisson_random_intercept_matches_pymc() {
    let counts = [2., 4., 3., 1., 0., 2., 5., 7., 6., 9., 8., 6., 1., 0., 2., 1., 3., 2., 4., 6., 5., 3., 4., 7.];
    let x: Vec<f64> = (0..24).map(|i| [0.1, 0.5, 0.9, 1.3, 1.7, 2.1][i % 6]).collect();
    let m = BayesGlmModel {
        y: counts.to_vec(),
        x: vec![x],
        x_names: vec!["x".into()],
        trials: Vec::new(),
        offset: Vec::new(),
        known_sd: None,
        family: GlmFamily::Poisson,
        groups: vec![GroupTerm {
            name: "g".into(),
            levels: (0..4).map(|i| i.to_string()).collect(),
            codes: (0..24).map(|i| i / 6).collect(),
            effects: vec!["Intercept".into()],
            z: vec![vec![1.0; 24]],
        }],
    };
    let priors = Priors {
        coefficient_scale: 2.5,
        intercept_location: Some(1.0),
        intercept_scale: 2.5,
        sd: ScalePrior::Exponential,
        sd_scale: 1.0,
        autoscale: false,
        ..Priors::default()
    };
    let fit = fit_bayes_glm(&m, &priors, &long_run(13), 0.95).unwrap();
    check(
        &fit,
        &[
            ("Intercept", 1.06092, 0.51301, 0.00351),
            ("x", 0.07984, 0.15393, 0.00066),
            ("SD(Intercept | g)", 0.86740, 0.44229, 0.00319),
            ("g[0]: Intercept", -0.42258, 0.51640, 0.00352),
            ("g[1]: Intercept", 0.71386, 0.49650, 0.00342),
            ("g[2]: Intercept", -0.65156, 0.52763, 0.00336),
            ("g[3]: Intercept", 0.37483, 0.50037, 0.00350),
        ],
        0.05,
    );
}
