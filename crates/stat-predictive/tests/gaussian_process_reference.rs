//! Gaussian processes against scikit-learn 1.9.1 (see
//! `tests/data/gaussian_process_reference.py`): the log marginal likelihood
//! and its gradient at a trial point agree to rounding, the maximized
//! likelihood is at least scikit-learn's, and at scikit-learn's optimum the
//! predicted means and standard deviations agree.

use stat_predictive::{
    CovarianceKernel, GaussianProcess, GpHyperparameters, GpOptions, Nugget, fit_gaussian_process,
    log_marginal_likelihood,
};

struct Case {
    kernel: CovarianceKernel,
    nugget: Option<f64>,
    x: Vec<Vec<f64>>,
    y: Vec<f64>,
    theta: Vec<f64>,
    lml: f64,
    trial: Vec<f64>,
    trial_lml: f64,
    trial_gradient: Vec<f64>,
    predict: Vec<(Vec<f64>, f64, f64)>,
}

fn read(name: &str) -> Case {
    let path = format!("{}/tests/data/gp_{name}.txt", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(path).unwrap();
    let mut c = Case {
        kernel: CovarianceKernel::Matern52,
        nugget: None,
        x: Vec::new(),
        y: Vec::new(),
        theta: Vec::new(),
        lml: 0.0,
        trial: Vec::new(),
        trial_lml: 0.0,
        trial_gradient: Vec::new(),
        predict: Vec::new(),
    };
    let nums = |rest: &[&str]| rest.iter().map(|v| v.parse::<f64>().unwrap()).collect::<Vec<_>>();
    for line in text.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        match parts[0] {
            "kernel" => {
                c.kernel =
                    if parts[1] == "se" { CovarianceKernel::SquaredExponential } else { CovarianceKernel::Matern52 }
            }
            "nugget" => c.nugget = parts[1].parse().ok(),
            "x" => {
                let mut v = nums(&parts[1..]);
                c.y.push(v.pop().unwrap());
                c.x.push(v);
            }
            "theta" => c.theta = nums(&parts[1..]),
            "lml" => c.lml = parts[1].parse().unwrap(),
            "trial" => c.trial = nums(&parts[1..]),
            "trial_lml" => c.trial_lml = parts[1].parse().unwrap(),
            "trial_gradient" => c.trial_gradient = nums(&parts[1..]),
            "predict" => {
                let mut v = nums(&parts[1..]);
                let sd = v.pop().unwrap();
                let mean = v.pop().unwrap();
                c.predict.push((v, mean, sd));
            }
            other => panic!("unknown line {other}"),
        }
    }
    c
}

fn standardized(y: &[f64]) -> (Vec<f64>, f64, f64) {
    let n = y.len() as f64;
    let m = y.iter().sum::<f64>() / n;
    let s = (y.iter().map(|v| (v - m).powi(2)).sum::<f64>() / n).sqrt();
    (y.iter().map(|v| (v - m) / s).collect(), m, s)
}

fn check(name: &str) {
    let c = read(name);
    let (ys, m, s) = standardized(&c.y);
    let d = c.x[0].len();
    // The likelihood surface itself.
    let (lml, grad) = log_marginal_likelihood(&c.x, &ys, c.kernel, &c.trial, c.nugget).unwrap();
    assert!((lml - c.trial_lml).abs() < 1e-8 * (1.0 + lml.abs()), "{name}: {lml} vs {}", c.trial_lml);
    for (a, b) in grad.iter().zip(&c.trial_gradient) {
        assert!((a - b).abs() < 1e-7 * (1.0 + b.abs()), "{name}: gradient {grad:?} vs {:?}", c.trial_gradient);
    }
    let (at_optimum, _) = log_marginal_likelihood(&c.x, &ys, c.kernel, &c.theta, c.nugget).unwrap();
    assert!((at_optimum - c.lml).abs() < 1e-8 * (1.0 + c.lml.abs()), "{name}: {at_optimum} vs {}", c.lml);
    // The maximum: at least scikit-learn's, and the same hyperparameters.
    let nugget = c.nugget.map_or(Nugget::Estimated, Nugget::Fixed);
    let gp = fit_gaussian_process(&c.x, &c.y, &GpOptions { kernel: c.kernel, nugget, ..GpOptions::default() }).unwrap();
    assert!(gp.log_likelihood > c.lml - 1e-6, "{name}: {} below scikit-learn's {}", gp.log_likelihood, c.lml);
    let theta = gp.hyper.theta(c.nugget.is_none());
    for (k, (a, b)) in theta.iter().zip(&c.theta).enumerate() {
        // A length scale at its upper bound (an irrelevant input) may stop a
        // little short of it: the likelihood is flat there.
        let tolerance = if b.exp() > 900.0 { 0.5 } else { 2e-3 };
        assert!((a - b).abs() < tolerance, "{name}: θ{k} {a} vs {b}");
    }
    // Predictions at scikit-learn's hyperparameters.
    let hyper = GpHyperparameters::from_theta(&c.theta, d, c.nugget);
    let at = GaussianProcess::with_hyperparameters(&c.x, &c.y, c.kernel, hyper, (m, s)).unwrap();
    for (x, mean, sd) in &c.predict {
        let (pm, ps) = at.predict(x);
        // scikit-learn's standard deviation includes the white-noise term.
        let ps = if c.nugget.is_none() { (ps * ps + at.noise_sd().powi(2)).sqrt() } else { ps };
        assert!((pm - mean).abs() < 1e-8 * (1.0 + mean.abs()), "{name}: mean {pm} vs {mean}");
        assert!((ps - sd).abs() < 1e-6 * (1.0 + sd), "{name}: sd {ps} vs {sd}");
        // And at our own optimum, close to them.
        let (qm, _) = gp.predict(x);
        assert!((qm - mean).abs() < 1e-3 * (1.0 + mean.abs()), "{name}: fitted mean {qm} vs {mean}");
    }
}

#[test]
fn squared_exponential_two_inputs() {
    check("se2");
}

#[test]
fn matern_two_inputs() {
    check("matern2");
}

#[test]
fn matern_three_inputs_one_irrelevant() {
    check("matern3");
}

#[test]
fn fixed_nugget_interpolates() {
    check("se1_fixed");
}
