use super::*;

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol * b.abs().max(1.0)
}

/// Reference: scikit-learn 1.9 unpenalized `LogisticRegression` /
/// `PoissonRegressor` estimates, standard errors from (XᵀWX)⁻¹ at those
/// estimates (NumPy).
#[test]
fn logistic_regression_matches_reference() {
    let x1 = [1.0, 1.5, 2.0, 2.5, 3.0, 3.5, 4.0, 4.5, 5.0, 5.5, 6.0, 6.5, 7.0, 7.5, 8.0, 8.5];
    let x2 = [0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0];
    let y = [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0];
    let x = Matrix::from_columns(&[&x1[..], &x2[..]]).unwrap();
    let fit = fit_glm(&x, &y, &GlmOptions::new(Family::Binomial)).unwrap();
    let b = [-3.656_592_58, 0.812_576_13, 0.478_440_27];
    let se = [1.977_038_12, 0.386_965_49, 1.338_723_15];
    for (c, (b, se)) in fit.coefficients.iter().zip(b.iter().zip(se)) {
        assert!(close(c.estimate, *b, 1e-6), "{} vs {b}", c.estimate);
        assert!(close(c.std_error, se, 1e-6), "{} vs {se}", c.std_error);
    }
    assert!(close(fit.deviance, 14.026_588_002_532_316, 1e-9));
    assert!(close(fit.log_likelihood, -fit.deviance / 2.0, 1e-12), "for 0/1 data ℓ = −D/2");
    assert!(close(fit.aic, fit.deviance + 6.0, 1e-12));
    // Null deviance of 9 successes in 16: −2 Σ ln p̂.
    let p = 9.0 / 16.0;
    let null = -2.0 * (9.0 * f64::ln(p) + 7.0 * f64::ln(1.0 - p));
    assert!(close(fit.null_deviance, null, 1e-12));
    assert!(fit.warnings.is_empty());
    // Likelihood-ratio test of x2 equals the deviance change.
    let reduced = fit_glm(&Matrix::from_columns(&[&x1[..]]).unwrap(), &y, &GlmOptions::new(Family::Binomial)).unwrap();
    let lr = likelihood_ratio_test(&fit, &reduced).unwrap();
    assert_eq!(lr.df, 1);
    assert!(close(lr.chi_square, reduced.deviance - fit.deviance, 1e-12));
}

#[test]
fn poisson_regression_matches_reference() {
    let c = [2.0, 3.0, 6.0, 7.0, 8.0, 9.0, 10.0, 12.0, 15.0, 1.0, 2.0, 4.0, 5.0, 7.0, 8.0, 11.0];
    let t = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0];
    let g = [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0];
    let x = Matrix::from_columns(&[&t[..], &g[..]]).unwrap();
    let fit = fit_glm(&x, &c, &GlmOptions::new(Family::Poisson)).unwrap();
    let b = [0.791_504_11, 0.224_948_81, -0.098_773_75];
    let se = [0.302_375_32, 0.043_475_08, 0.211_409_56];
    for (co, (b, se)) in fit.coefficients.iter().zip(b.iter().zip(se)) {
        assert!(close(co.estimate, *b, 1e-6), "{} vs {b}", co.estimate);
        assert!(close(co.std_error, se, 1e-6));
    }
    assert!(close(fit.deviance, 3.827_061_496_053_892_7, 1e-8));
    // Poisson with intercept: fitted means sum to the observed total.
    assert!(close(fit.fitted.iter().sum::<f64>(), c.iter().sum(), 1e-9));
    // Prediction limits: at the origin they are the intercept's interval
    // mapped through exp; elsewhere se² = x̃ᵀVx̃.
    let at = Matrix::from_rows(&[vec![0.0, 0.0], vec![5.0, 1.0]]).unwrap();
    let pred = fit.predict_with_limits(&at).unwrap();
    let ci = fit.coefficients[0].confidence_interval;
    assert!(close(pred[0].0, fit.coefficients[0].estimate.exp(), 1e-12));
    assert!(close(pred[0].1, ci.0.exp(), 1e-12) && close(pred[0].2, ci.1.exp(), 1e-12));
    let v = &fit.covariance;
    let xv = [1.0, 5.0, 1.0];
    let var: f64 = (0..3).flat_map(|a| (0..3).map(move |b| (a, b))).map(|(a, b)| xv[a] * v[(a, b)] * xv[b]).sum();
    let eta = fit.coefficients.iter().zip(&xv).map(|(c, x)| c.estimate * x).sum::<f64>();
    assert!(close(pred[1].0, eta.exp(), 1e-12));
    assert!(close(pred[1].2, (eta + 1.959_963_984_540_054 * var.sqrt()).exp(), 1e-9));
    assert!(close(pred[1].0, fit.predict(&at).unwrap()[1], 1e-12));
    assert!(fit.predict_with_limits(&Matrix::from_rows(&[vec![1.0]]).unwrap()).is_err());
}

#[test]
fn gaussian_family_equals_least_squares() {
    let x = [1.0, 2.0, 3.0, 4.0, 5.0];
    let y = [2.0, 4.0, 5.0, 4.0, 5.0];
    let fit = fit_glm(&Matrix::from_columns(&[&x[..]]).unwrap(), &y, &GlmOptions::new(Family::Gaussian)).unwrap();
    assert!(close(fit.coefficients[0].estimate, 2.2, 1e-12));
    assert!(close(fit.coefficients[1].estimate, 0.6, 1e-12));
    assert!(close(fit.deviance, 2.4, 1e-12), "residual sum of squares");
    assert!(close(fit.dispersion, 0.8, 1e-12));
    // The slope's standard error is OLS's: √(0.8/10).
    assert!(close(fit.coefficients[1].std_error, (0.08f64).sqrt(), 1e-12));
    assert_eq!(fit.iterations, 2, "one step, plus one to confirm");
    let pred = fit.predict(&Matrix::from_columns(&[&[6.0][..]]).unwrap()).unwrap();
    assert!(close(pred[0], 5.8, 1e-12));
}

#[test]
fn rejects_bad_input_and_flags_separation() {
    let x = Matrix::from_columns(&[&[1.0, 2.0, 3.0, 4.0][..]]).unwrap();
    assert!(fit_glm(&x, &[0.0, 1.0, 2.0, 1.0], &GlmOptions::new(Family::Binomial)).is_err());
    assert!(fit_glm(&x, &[1.0, -1.0, 2.0, 1.0], &GlmOptions::new(Family::Poisson)).is_err());
    assert!(fit_glm(&x, &[0.0, 1.0], &GlmOptions::new(Family::Gaussian)).is_err());
    // Perfect separation: either a convergence failure or a warning.
    let sep = fit_glm(&x, &[0.0, 0.0, 1.0, 1.0], &GlmOptions::new(Family::Binomial));
    assert!(sep.map_or(true, |f| !f.warnings.is_empty()));
}

/// Gamma family on McCullagh & Nelder (1989, *Generalized Linear Models*,
/// §8.4.2) clotting times, lot 1, against log plasma concentration. Published
/// inverse-link estimates −0.01655 and 0.01534 (also the example in R's
/// `?glm`: −0.01655438, 0.01534311, AIC 37.99); standard errors, deviance,
/// Pearson dispersion and AIC from an independent standard-library Python
/// fit (Fisher scoring, `ref_glm.py`).
#[test]
fn gamma_family_matches_mccullagh_nelder() {
    let u = [5.0f64, 10.0, 15.0, 20.0, 30.0, 40.0, 60.0, 80.0, 100.0];
    let y = [118.0, 58.0, 42.0, 35.0, 27.0, 25.0, 21.0, 19.0, 18.0];
    let lu: Vec<f64> = u.iter().map(|v| v.ln()).collect();
    let x = Matrix::from_columns(&[&lu[..]]).unwrap();
    let fit = fit_glm(&x, &y, &GlmOptions::new(Family::Gamma)).unwrap();
    assert_eq!(fit.link, Link::Inverse);
    assert!(close(fit.coefficients[0].estimate, -0.016_554_381_726_200_3, 1e-8));
    assert!(close(fit.coefficients[1].estimate, 0.015_343_114_910_324_7, 1e-8));
    assert!(close(fit.coefficients[0].std_error, 0.000_927_549_138_658_195, 1e-6));
    assert!(close(fit.coefficients[1].std_error, 0.000_414_959_642_666_335, 1e-6));
    assert!(close(fit.deviance, 0.016_729_715_178_483_9, 1e-7));
    assert!(close(fit.dispersion, 0.002_446_036_242_093_29, 1e-6));
    assert!(close(fit.aic, 37.989_923_949_550_3, 1e-7));
    assert_eq!(fit.label(), "Gamma (inverse link)");
    // Log link.
    let log = fit_glm(&x, &y, &GlmOptions { link: Some(Link::Log), ..GlmOptions::new(Family::Gamma) }).unwrap();
    assert!(close(log.coefficients[0].estimate, 5.503_230_227_515_95, 1e-7), "{}", log.coefficients[0].estimate);
    assert!(close(log.coefficients[1].estimate, -0.601_917_671_742_359, 1e-7));
    assert!(close(log.coefficients[1].std_error, 0.055_307_803_032_634_8, 1e-6));
    assert!(close(log.aic, 58.481_656_206_584_3, 1e-7));
    // Prediction limits come back ordered under the decreasing inverse link.
    let pred = fit.predict_with_limits(&Matrix::from_columns(&[&[3.0][..]]).unwrap()).unwrap();
    assert!(pred[0].1 < pred[0].0 && pred[0].0 < pred[0].2);
    assert!(fit_glm(&x, &[1.0, -1.0, 2.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0], &GlmOptions::new(Family::Gamma)).is_err());
}

/// Probit and complementary log-log links with trials as prior weights, on
/// Martin's rotenone data (Finney 1971); references from the same
/// independent Python fit (Newton–Raphson on the binomial likelihood).
#[test]
fn binomial_links_with_trials_as_weights() {
    let x: Vec<f64> = [10.2f64, 7.7, 5.1, 3.8, 2.6].iter().map(|d| d.log10()).collect();
    let n = [50.0, 49.0, 46.0, 48.0, 50.0];
    let r = [44.0, 42.0, 24.0, 16.0, 6.0];
    let y: Vec<f64> = r.iter().zip(&n).map(|(a, b)| a / b).collect();
    let xm = Matrix::from_columns(&[&x[..]]).unwrap();
    for (link, b0, b1, se1) in [
        (Link::Probit, -2.887_463_256_361_71, 4.213_196_756_114_94, 0.480_561_751_814_927),
        (Link::ComplementaryLogLog, -3.524_934_946_394_1, 4.450_178_781_854_54, 0.527_382_554_247_762),
        (Link::Logit, -4.886_912_275_999_41, 7.146_177_555_935_41, 0.892_753_306_957_877),
    ] {
        let o = GlmOptions { link: Some(link), weights: n.to_vec(), ..GlmOptions::new(Family::Binomial) };
        let fit = fit_glm(&xm, &y, &o).unwrap();
        assert!(close(fit.coefficients[0].estimate, b0, 1e-7), "{link:?}");
        assert!(close(fit.coefficients[1].estimate, b1, 1e-7), "{link:?}");
        assert!(close(fit.coefficients[1].std_error, se1, 1e-6), "{link:?}");
        assert_eq!(fit.dispersion, 1.0);
        // Grouped and expanded 0/1 data give the same estimates and the same
        // deviance differences.
        let (mut xs, mut ys) = (Vec::new(), Vec::new());
        for i in 0..5 {
            for j in 0..n[i] as usize {
                xs.push(x[i]);
                ys.push(if (j as f64) < r[i] { 1.0 } else { 0.0 });
            }
        }
        let ex = fit_glm(
            &Matrix::from_columns(&[&xs[..]]).unwrap(),
            &ys,
            &GlmOptions { link: Some(link), ..GlmOptions::new(Family::Binomial) },
        )
        .unwrap();
        assert!(close(ex.coefficients[1].estimate, b1, 1e-7));
        assert!(close(ex.null_deviance - ex.deviance, fit.null_deviance - fit.deviance, 1e-7));
    }
    // A link the family does not offer, and proportions without weights.
    let bad = GlmOptions { link: Some(Link::Inverse), ..GlmOptions::new(Family::Binomial) };
    assert!(fit_glm(&xm, &[0.0, 1.0, 0.0, 1.0, 1.0], &bad).is_err());
    assert!(fit_glm(&xm, &y, &GlmOptions::new(Family::Binomial)).is_err());
}

#[test]
fn links_invert_and_differentiate() {
    for link in Link::ALL {
        assert_eq!(Link::from_key(link.key()), Some(link));
        for eta in [-1.3, 0.2, 0.9] {
            let eta = if link == Link::Inverse { eta + 2.0 } else { eta };
            let mu = link.inverse(eta);
            assert!(close(link.link(mu), eta, 1e-10), "{link:?} at {eta}");
            let h = 1e-6;
            let numeric = (link.inverse(eta + h) - link.inverse(eta - h)) / (2.0 * h);
            assert!(close(link.mu_eta(eta), numeric, 1e-7), "{link:?} derivative at {eta}");
        }
    }
    for family in Family::ALL {
        assert_eq!(family.links()[0], family.default_link());
    }
}

/// Identity links whose iterations overshoot to a non-positive mean halve the
/// step instead of failing. References: maximum likelihood by constrained
/// Nelder–Mead in Python (standard library), score ≈ 1e-8 at the optimum.
#[test]
fn identity_links_halve_steps_that_leave_the_mean_range() {
    let xs = [1.0f64, 2.0, 3.0, 4.0, 5.0, 6.0];
    let x = Matrix::from_columns(&[&xs[..]]).unwrap();
    let identity = |family| GlmOptions { link: Some(Link::Identity), ..GlmOptions::new(family) };
    let poisson = fit_glm(&x, &[1.0, 1.0, 1.0, 1.0, 6.0, 20.0], &identity(Family::Poisson)).unwrap();
    assert!(close(poisson.coefficients[0].estimate, -1.3852, 1e-3), "{}", poisson.coefficients[0].estimate);
    assert!(close(poisson.coefficients[1].estimate, 1.8243, 1e-3), "{}", poisson.coefficients[1].estimate);
    let gamma = fit_glm(&x, &[3.0, 1.0, 1.0, 2.0, 6.0, 12.0], &identity(Family::Gamma)).unwrap();
    assert!(close(gamma.coefficients[0].estimate, 1.0079, 1e-3), "{}", gamma.coefficients[0].estimate);
    assert!(close(gamma.coefficients[1].estimate, 0.8146, 1e-3), "{}", gamma.coefficients[1].estimate);
}

/// Gaussian responses with a log link may include zeros; the start used to be
/// μ = y, whose log is −∞, so the fit failed at the first iteration.
/// Reference: least squares of exp(b₀ + b₁x) on the data (SciPy 1.18
/// `optimize.least_squares`).
#[test]
fn gaussian_log_link_with_zero_response() {
    let x = [1.0, 2.0, 3.0, 4.0, 5.0];
    let y = [0.0, 1.0, 2.0, 4.0, 8.0];
    let o = GlmOptions { link: Some(Link::Log), ..GlmOptions::new(Family::Gaussian) };
    let fit = fit_glm(&Matrix::from_columns(&[&x[..]]).unwrap(), &y, &o).unwrap();
    let b0 = fit.coefficients[0].estimate;
    let b1 = fit.coefficients[1].estimate;
    assert!((b0 - -1.505_429_62).abs() < 1e-6, "{b0}");
    assert!((b1 - 0.717_994_81).abs() < 1e-6, "{b1}");
}

/// Poisson with identity link where the first IRLS step leaves the mean range
/// (one fitted mean ≤ 0) but the maximum is interior. With no previous
/// estimate to halve toward, the step is pulled back toward the starting
/// linear predictors. Reference: the score equations Xᵀ(y/μ − 1) = 0 solved
/// by `scipy.optimize.fsolve`; deviance from the Poisson formula (NumPy).
#[test]
fn identity_link_recovers_from_an_invalid_first_step() {
    let x: Vec<f64> = (0..8).map(f64::from).collect();
    let y = [2.0, 0.0, 5.0, 6.0, 9.0, 11.0, 14.0, 15.0];
    let o = GlmOptions { link: Some(Link::Identity), ..GlmOptions::new(Family::Poisson) };
    let fit = fit_glm(&Matrix::from_columns(&[&x[..]]).unwrap(), &y, &o).unwrap();
    // The identity link converges linearly; IRLS stops on a flat deviance.
    assert!(close(fit.coefficients[0].estimate, 1.071_929_209_014_052_9, 2e-6), "{}", fit.coefficients[0].estimate);
    assert!(close(fit.coefficients[1].estimate, 1.908_020_225_995_984_9, 2e-6));
    assert!(close(fit.deviance, 6.912_927_058_444_816, 1e-8));
}

/// Without an intercept the null model is η = 0. Poisson/log: μ = 1, so the
/// null deviance is 2Σ[y ln y − (y − 1)] = 179.806931133227 (hand formula,
/// NumPy). Gamma/inverse: μ = 1/0 is no mean, so the null deviance is ∞
/// (as R's `glm`), not NaN.
#[test]
fn null_deviance_without_intercept() {
    let x: Vec<f64> = (0..8).map(|v| f64::from(v) / 8.0).collect();
    let y = [2.0, 0.0, 5.0, 6.0, 9.0, 11.0, 14.0, 15.0];
    let xm = Matrix::from_columns(&[&x[..]]).unwrap();
    let fit = fit_glm(&xm, &y, &GlmOptions { intercept: false, ..GlmOptions::new(Family::Poisson) }).unwrap();
    assert!(close(fit.null_deviance, 179.806_931_133_227_07, 1e-10), "{}", fit.null_deviance);
    let g = [2.0, 1.5, 5.0, 6.0, 9.0, 11.0, 14.0, 15.0];
    let x1: Vec<f64> = (1..=8).map(|v| 1.0 / f64::from(v)).collect();
    let fit = fit_glm(
        &Matrix::from_columns(&[&x1[..]]).unwrap(),
        &g,
        &GlmOptions { intercept: false, ..GlmOptions::new(Family::Gamma) },
    )
    .unwrap();
    assert_eq!(fit.null_deviance, f64::INFINITY);
}

/// μ = 1/η: when the link-scale interval η̂ ± c·se contains 0 the image is
/// two unbounded rays, so the limits are NaN rather than a finite band that
/// excludes the fit.
#[test]
fn inverse_link_limits_through_zero_are_undefined() {
    // McCullagh & Nelder's clotting times against log concentration.
    let x: Vec<f64> = [5.0f64, 10.0, 15.0, 20.0, 30.0, 40.0, 60.0, 80.0, 100.0].iter().map(|u| u.ln()).collect();
    let y = [118.0, 58.0, 42.0, 35.0, 27.0, 25.0, 21.0, 19.0, 18.0];
    let fit = fit_glm(&Matrix::from_columns(&[&x[..]]).unwrap(), &y, &GlmOptions::new(Family::Gamma)).unwrap();
    assert_eq!(fit.link, Link::Inverse);
    // Where η̂ = 0 any positive standard error straddles zero.
    let x0 = -fit.coefficients[0].estimate / fit.coefficients[1].estimate;
    let p = fit.predict_with_limits(&Matrix::from_columns(&[&[x0][..]]).unwrap()).unwrap()[0];
    assert!(p.1.is_nan() && p.2.is_nan(), "{p:?}");
    // Away from it the limits are finite and contain the fit.
    let p = fit.predict_with_limits(&Matrix::from_columns(&[&[3.0][..]]).unwrap()).unwrap()[0];
    assert!(p.1 < p.0 && p.0 < p.2);
}
