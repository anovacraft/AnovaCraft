use super::*;

fn close(a: f64, b: f64, tol: f64) {
    assert!((a - b).abs() <= tol * b.abs().max(1.0), "{a} vs {b}");
}

/// Deterministic sample of `n` draws from `dist`.
fn draw(dist: &FittedDistribution, n: usize, seed: u64) -> Vec<f64> {
    let mut rng = Rng::new(seed);
    (0..n).map(|_| dist.sample(&mut rng)).collect()
}

fn dist(family: FitFamily, threshold: f64, location: f64, scale: f64, shape: f64) -> FittedDistribution {
    FittedDistribution { family, threshold, location, scale, shape, log_likelihood: 0.0, n: 0 }
}

/// Log-likelihood written directly from each family's density, independent
/// of the location–scale machinery.
fn direct_ll(f: &FittedDistribution, x: &[f64]) -> f64 {
    x.iter()
        .map(|&v| {
            let (g, m, s, k) = (f.threshold, f.location, f.scale, f.shape);
            match f.family {
                FitFamily::Normal => {
                    let z = (v - m) / s;
                    -0.5 * z * z - (s * (2.0 * std::f64::consts::PI).sqrt()).ln()
                }
                FitFamily::Logistic => {
                    let z = (v - m) / s;
                    -z - 2.0 * (1.0 + (-z).exp()).ln() - s.ln()
                }
                FitFamily::SmallestExtremeValue => {
                    let z = (v - m) / s;
                    z - z.exp() - s.ln()
                }
                FitFamily::LargestExtremeValue => {
                    let z = (v - m) / s;
                    -z - (-z).exp() - s.ln()
                }
                FitFamily::Weibull | FitFamily::Weibull3 => {
                    let (beta, alpha, u) = (1.0 / s, m.exp(), v - g);
                    (beta / alpha).ln() + (beta - 1.0) * (u / alpha).ln() - (u / alpha).powf(beta)
                }
                FitFamily::Lognormal | FitFamily::Lognormal3 => {
                    let u = v - g;
                    let z = (u.ln() - m) / s;
                    -0.5 * z * z - (u * s * (2.0 * std::f64::consts::PI).sqrt()).ln()
                }
                FitFamily::LogLogistic | FitFamily::LogLogistic3 => {
                    let u = v - g;
                    let z = (u.ln() - m) / s;
                    -z - 2.0 * (1.0 + (-z).exp()).ln() - (u * s).ln()
                }
                FitFamily::Gamma | FitFamily::Gamma3 => {
                    let u = v - g;
                    (k - 1.0) * u.ln() - u / s - ln_gamma(k) - k * s.ln()
                }
                FitFamily::Exponential | FitFamily::Exponential2 => -(v - g) / s - s.ln(),
            }
        })
        .sum()
}

#[test]
fn every_family_round_trips_quantile_and_cdf() {
    let cases = [
        dist(FitFamily::Normal, 0.0, 3.0, 2.0, 1.0),
        dist(FitFamily::Lognormal3, 5.0, 1.0, 0.5, 1.0),
        dist(FitFamily::Exponential2, -1.0, 0.0, 4.0, 1.0),
        dist(FitFamily::Weibull3, 2.0, 1.5, 0.4, 1.0),
        dist(FitFamily::Gamma3, 1.0, 0.0, 2.0, 0.6),
        dist(FitFamily::Gamma, 0.0, 0.0, 0.5, 7.5),
        dist(FitFamily::Logistic, 0.0, -2.0, 1.5, 1.0),
        dist(FitFamily::LogLogistic3, -3.0, 0.2, 0.3, 1.0),
        dist(FitFamily::SmallestExtremeValue, 0.0, 10.0, 2.0, 1.0),
        dist(FitFamily::LargestExtremeValue, 0.0, 10.0, 2.0, 1.0),
    ];
    for d in &cases {
        for &p in &[1e-6, 0.01, 0.2, 0.5, 0.8, 0.99, 1.0 - 1e-6] {
            let q = d.quantile(p);
            close(d.cdf(q), p, 1e-9);
            close(d.ln_sf(q).exp(), 1.0 - p, 1e-8);
            close(d.standardize(q), d.standard_quantile(p), 1e-8);
            // The density is the derivative of the distribution function.
            let h = 1e-6 * (1.0 + q.abs());
            if d.family.has_threshold() && q - d.threshold < 100.0 * h {
                continue;
            }
            let numeric = (d.cdf(q + h) - d.cdf(q - h)) / (2.0 * h);
            assert!((numeric - d.pdf(q)).abs() < 1e-5 * (1.0 + d.pdf(q)), "{:?} p={p}", d.family);
            // ... and agrees with the independently written log density.
            close(d.pdf(q).ln(), direct_ll(d, &[q]), 1e-10);
        }
    }
    // Below a threshold.
    let w = dist(FitFamily::Weibull3, 2.0, 1.5, 0.4, 1.0);
    assert_eq!(w.cdf(1.0), 0.0);
    assert_eq!(w.ln_sf(1.5), 0.0);
    assert_eq!(w.pdf(2.0), 0.0);
}

#[test]
fn far_tails_stay_finite() {
    for d in [
        dist(FitFamily::Normal, 0.0, 0.0, 1.0, 1.0),
        dist(FitFamily::Logistic, 0.0, 0.0, 1.0, 1.0),
        dist(FitFamily::SmallestExtremeValue, 0.0, 0.0, 1.0, 1.0),
        dist(FitFamily::LargestExtremeValue, 0.0, 0.0, 1.0, 1.0),
    ] {
        for z in [-40.0, -8.0, 8.0, 40.0] {
            assert!(d.ln_cdf(z).is_finite() && d.ln_sf(z).is_finite(), "{:?} at {z}", d.family);
        }
    }
    // ln F₀ of the smallest extreme value near its left tail: ≈ z.
    close(Standard::Sev.ln_cdf(-30.0), -30.0, 1e-12);
    close(Standard::Lev.ln_sf(30.0), -30.0, 1e-12);
}

#[test]
fn gamma_quantile_inverts_incomplete_gamma() {
    for &k in &[0.05, 0.3, 1.0, 2.5, 40.0, 900.0] {
        for &p in &[1e-10, 1e-3, 0.3, 0.5, 0.9, 0.999_999] {
            let u = gamma_quantile(k, p);
            close(regularized_gamma_p(k, u).unwrap(), p, 1e-9);
        }
    }
}

#[test]
fn gamma_variates_have_the_right_moments() {
    for &k in &[0.4, 1.0, 3.0] {
        let mut rng = Rng::new(7);
        let n = 200_000;
        let v: Vec<f64> = (0..n).map(|_| gamma_variate(k, &mut rng)).collect();
        let mean = v.iter().sum::<f64>() / n as f64;
        let var = v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n as f64;
        assert!((mean - k).abs() < 0.02 * k.max(1.0), "k={k}: mean {mean}");
        assert!((var - k).abs() < 0.05 * k.max(1.0), "k={k}: var {var}");
    }
}

#[test]
fn closed_form_fits() {
    let x = [3.1, 4.7, 2.2, 8.9, 5.5, 3.3, 12.0, 6.1, 4.4, 2.9];
    let n = x.len() as f64;
    let mean = x.iter().sum::<f64>() / n;
    let ss = x.iter().map(|v| (v - mean).powi(2)).sum::<f64>();
    // Normal: μ̂ = x̄, reported σ = s, ℓ at the maximum-likelihood σ̂.
    let f = fit_distribution(&x, FitFamily::Normal).unwrap();
    close(f.location, mean, 1e-13);
    close(f.scale, (ss / (n - 1.0)).sqrt(), 1e-13);
    let mle = dist(FitFamily::Normal, 0.0, mean, (ss / n).sqrt(), 1.0);
    close(f.log_likelihood, direct_ll(&mle, &x), 1e-12);
    // Lognormal: mean and divisor-n SD of the logs.
    let y: Vec<f64> = x.iter().map(|v| v.ln()).collect();
    let my = y.iter().sum::<f64>() / n;
    let sy = (y.iter().map(|v| (v - my).powi(2)).sum::<f64>() / n).sqrt();
    let f = fit_distribution(&x, FitFamily::Lognormal).unwrap();
    close(f.location, my, 1e-13);
    close(f.scale, sy, 1e-13);
    close(f.log_likelihood, direct_ll(&f, &x), 1e-12);
    // Exponential: θ̂ = x̄.
    let f = fit_distribution(&x, FitFamily::Exponential).unwrap();
    close(f.scale, mean, 1e-13);
    close(f.log_likelihood, direct_ll(&f, &x), 1e-12);
    // Two-parameter exponential: unbiased estimates, maximum log-likelihood.
    let f = fit_distribution(&x, FitFamily::Exponential2).unwrap();
    let theta = n * (mean - 2.2) / (n - 1.0);
    close(f.scale, theta, 1e-13);
    close(f.threshold, 2.2 - theta / n, 1e-13);
    close(f.log_likelihood, direct_ll(&dist(FitFamily::Exponential2, 2.2, 0.0, mean - 2.2, 1.0), &x), 1e-12);
    assert!(f.cdf(2.2) > 0.0);
    assert_eq!(f.parameters().len(), 2);
}

/// At a maximum every one-parameter perturbation lowers the independently
/// computed log-likelihood, and the reported value matches it.
fn assert_local_max(f: &FittedDistribution, x: &[f64]) {
    let ll = direct_ll(f, x);
    close(f.log_likelihood, ll, 1e-10);
    for which in 0..4 {
        for h in [-1e-4, 1e-4] {
            let mut g = f.clone();
            match which {
                0 => g.location += h,
                1 => g.scale *= 1.0 + h,
                2 => g.shape *= 1.0 + h,
                _ if f.family.has_threshold() => g.threshold -= h.abs() * (1.0 + f.threshold.abs()) * h.signum(),
                _ => continue,
            }
            let other = direct_ll(&g, x);
            assert!(other <= ll + 1e-9 * ll.abs().max(1.0), "{:?} param {which} step {h}: {other} > {ll}", f.family);
        }
    }
}

#[test]
fn iterative_fits_solve_their_likelihood_equations() {
    let base = draw(&dist(FitFamily::Weibull, 0.0, 2.0, 0.6, 1.0), 60, 11);
    for family in [
        FitFamily::Weibull,
        FitFamily::Gamma,
        FitFamily::Logistic,
        FitFamily::LogLogistic,
        FitFamily::SmallestExtremeValue,
        FitFamily::LargestExtremeValue,
    ] {
        let f = fit_distribution(&base, family).unwrap();
        assert_local_max(&f, &base);
    }
    // Weibull score equations: Σtᵝ ln t / Σtᵝ − 1/β − mean(ln t) = 0 and α = (Σtᵝ/n)^{1/β}.
    let f = fit_distribution(&base, FitFamily::Weibull).unwrap();
    let beta = 1.0 / f.scale;
    let n = base.len() as f64;
    let st: f64 = base.iter().map(|t| t.powf(beta)).sum();
    let stl: f64 = base.iter().map(|t| t.powf(beta) * t.ln()).sum();
    let ml: f64 = base.iter().map(|t| t.ln()).sum::<f64>() / n;
    assert!((stl / st - 1.0 / beta - ml).abs() < 1e-9);
    close(f.location.exp(), (st / n).powf(1.0 / beta), 1e-9);
    // Gamma: ln k − ψ(k) = ln x̄ − mean(ln x), θ k = x̄.
    let g = fit_distribution(&base, FitFamily::Gamma).unwrap();
    let mean = base.iter().sum::<f64>() / n;
    close(g.shape.ln() - digamma(g.shape), mean.ln() - ml, 1e-10);
    close(g.shape * g.scale, mean, 1e-12);
}

#[test]
fn mirrored_extreme_values_and_log_logistic_agree() {
    let x = draw(&dist(FitFamily::Normal, 0.0, 5.0, 2.0, 1.0), 40, 3);
    let neg: Vec<f64> = x.iter().map(|v| -v).collect();
    let sev = fit_distribution(&neg, FitFamily::SmallestExtremeValue).unwrap();
    let lev = fit_distribution(&x, FitFamily::LargestExtremeValue).unwrap();
    close(lev.location, -sev.location, 1e-9);
    close(lev.scale, sev.scale, 1e-9);
    close(lev.log_likelihood, sev.log_likelihood, 1e-10);
    // Log-logistic on x is the logistic on ln x, with the Jacobian.
    let pos: Vec<f64> = x.iter().map(|v| v.abs() + 0.5).collect();
    let logs: Vec<f64> = pos.iter().map(|v| v.ln()).collect();
    let ll = fit_distribution(&pos, FitFamily::LogLogistic).unwrap();
    let lo = fit_distribution(&logs, FitFamily::Logistic).unwrap();
    close(ll.location, lo.location, 1e-9);
    close(ll.scale, lo.scale, 1e-9);
    close(ll.log_likelihood, lo.log_likelihood - logs.iter().sum::<f64>(), 1e-10);
}

#[test]
fn threshold_forms_recover_their_parameters() {
    let truth = dist(FitFamily::Weibull3, 10.0, 5f64.ln(), 0.5, 1.0); // β = 2, α = 5
    let x = draw(&truth, 3000, 21);
    let f = fit_distribution(&x, FitFamily::Weibull3).unwrap();
    assert!((f.threshold - 10.0).abs() < 0.3, "γ {}", f.threshold);
    assert!((1.0 / f.scale - 2.0).abs() < 0.15, "β {}", 1.0 / f.scale);
    assert!((f.location.exp() - 5.0).abs() < 0.3, "α {}", f.location.exp());
    assert_local_max(&f, &x);

    let truth = dist(FitFamily::Lognormal3, -20.0, 3.0, 0.3, 1.0);
    let x = draw(&truth, 3000, 22);
    let f = fit_distribution(&x, FitFamily::Lognormal3).unwrap();
    assert!((f.threshold + 20.0).abs() < 3.0, "γ {}", f.threshold);
    assert!((f.scale - 0.3).abs() < 0.05, "σ {}", f.scale);
    assert_local_max(&f, &x);

    let truth = dist(FitFamily::Gamma3, 100.0, 0.0, 2.0, 3.0);
    let x = draw(&truth, 3000, 23);
    let f = fit_distribution(&x, FitFamily::Gamma3).unwrap();
    assert!((f.threshold - 100.0).abs() < 1.0, "γ {}", f.threshold);
    assert!((f.shape - 3.0).abs() < 0.6, "k {}", f.shape);
    assert_local_max(&f, &x);

    let truth = dist(FitFamily::LogLogistic3, 50.0, 2.0, 0.25, 1.0);
    let x = draw(&truth, 3000, 24);
    let f = fit_distribution(&x, FitFamily::LogLogistic3).unwrap();
    assert!((f.threshold - 50.0).abs() < 2.0, "γ {}", f.threshold);
    assert_local_max(&f, &x);
}

#[test]
fn threshold_likelihood_never_below_two_parameter_form() {
    let x = draw(&dist(FitFamily::Weibull, 0.0, 1.0, 0.5, 1.0), 80, 5);
    for family in [FitFamily::Weibull3, FitFamily::Lognormal3, FitFamily::Gamma3, FitFamily::LogLogistic3] {
        let three = fit_distribution(&x, family);
        let two = fit_distribution(&x, family.without_threshold().unwrap()).unwrap();
        if let Ok(three) = three {
            assert!(three.log_likelihood >= two.log_likelihood - 1e-6, "{family:?}");
            assert!(three.threshold < x.iter().copied().fold(f64::INFINITY, f64::min));
        }
    }
}

#[test]
fn anderson_darling_statistic_matches_direct_formula() {
    // Exponential with θ = x̄: zᵢ = 1 − exp(−xᵢ/θ).
    let x = [0.3, 1.2, 0.7, 2.9, 0.1, 4.4, 1.8, 0.9, 0.5, 2.2];
    let f = fit_distribution(&x, FitFamily::Exponential).unwrap();
    let mut s = x.to_vec();
    s.sort_by(f64::total_cmp);
    let n = s.len();
    let z: Vec<f64> = s.iter().map(|v| 1.0 - (-v / f.scale).exp()).collect();
    let direct = -(n as f64)
        - (0..n).map(|i| (2.0 * i as f64 + 1.0) * (z[i].ln() + (1.0 - z[n - 1 - i]).ln())).sum::<f64>() / n as f64;
    close(anderson_darling_statistic(&s, &f), direct, 1e-12);
    // A value outside the support makes the statistic infinite.
    let shifted = dist(FitFamily::Weibull3, 0.5, 0.0, 1.0, 1.0);
    assert_eq!(anderson_darling_statistic(&s, &shifted), f64::INFINITY);
}

/// Upper percentage point of the simulated null, on the modified scale.
fn upper_point(family: FitFamily, n: usize, alpha: f64) -> f64 {
    let null = pivotal_null(family, n, 6000, 99);
    let i = ((1.0 - alpha) * null.len() as f64) as usize;
    null[i] * modifier(family, n)
}

/// The simulated null distributions reproduce the published percentage
/// points of the modified statistics (D'Agostino & Stephens 1986: Table 4.7
/// for the normal, 4.14 for the exponential, 4.17 for the extreme-value and
/// Weibull, 4.22 for the logistic).
#[test]
fn simulated_null_matches_published_percentage_points() {
    let cases = [
        (FitFamily::Lognormal, 0.05, 0.752),
        (FitFamily::Lognormal, 0.01, 1.035),
        (FitFamily::Exponential, 0.05, 1.321),
        (FitFamily::Weibull, 0.05, 0.757),
        (FitFamily::Weibull, 0.01, 1.038),
        (FitFamily::SmallestExtremeValue, 0.05, 0.757),
        (FitFamily::LargestExtremeValue, 0.05, 0.757),
        (FitFamily::Logistic, 0.05, 0.660),
    ];
    for (family, alpha, published) in cases {
        let simulated = upper_point(family, 40, alpha);
        assert!((simulated - published).abs() < 0.06 * published, "{family:?} {alpha}: {simulated} vs {published}");
    }
}

#[test]
fn identification_prefers_the_generating_family() {
    let truth = dist(FitFamily::Weibull, 0.0, 3.0, 0.4, 1.0); // β = 2.5
    let x = draw(&truth, 150, 31);
    let opts =
        IdentifyOptions { threshold_replicates: 60, gamma_replicates: 200, replicates: 500, ..Default::default() };
    let fits = identify_distribution(&x, &FitFamily::ALL, &opts).unwrap();
    assert_eq!(fits.len(), FitFamily::ALL.len());
    let get = |f: FitFamily| fits.iter().find(|r| r.0 == f).unwrap().1.as_ref().unwrap();
    let weibull = get(FitFamily::Weibull);
    // (Over 300 such samples, 12% of p-values fell below 0.1.)
    assert!(weibull.anderson_darling.p_value > 0.01, "{:?}", weibull.anderson_darling);
    assert!(matches!(weibull.anderson_darling.method, PValueMethod::Simulation { replicates } if replicates > 400));
    // A badly fitting family is rejected.
    let expo = get(FitFamily::Exponential);
    assert!(expo.anderson_darling.p_value < 0.01, "{:?}", expo.anderson_darling);
    // The Weibull has one of the smallest AICs among two-parameter families.
    let aic = |f: FitFamily| get(f).fit.aic();
    for other in [FitFamily::Exponential, FitFamily::Lognormal, FitFamily::LogLogistic, FitFamily::LargestExtremeValue]
    {
        assert!(aic(FitFamily::Weibull) < aic(other), "{other:?}");
    }
    // Threshold forms carry their test; the two-parameter forms do not.
    if let Ok(w3) = &fits.iter().find(|r| r.0 == FitFamily::Weibull3).unwrap().1 {
        let t = w3.threshold_test.unwrap();
        assert!(t.statistic >= 0.0 && (0.0..=1.0).contains(&t.p_value));
    }
    assert!(weibull.threshold_test.is_none());
    // The normal uses the published formula.
    assert_eq!(get(FitFamily::Normal).anderson_darling.method, PValueMethod::Formula);
    // Repeatable.
    let again = identify_distribution(&x, &FitFamily::ALL, &opts).unwrap();
    assert_eq!(fits, again);
}

#[test]
fn simulated_p_values_are_roughly_uniform_under_the_null() {
    // For 200 logistic samples of size 30 the p-values should not pile up.
    let truth = dist(FitFamily::Logistic, 0.0, 0.0, 1.0, 1.0);
    let opts = IdentifyOptions { replicates: 1000, ..Default::default() };
    let mut below = 0;
    for seed in 0..200 {
        let x = draw(&truth, 30, 1000 + seed);
        let r = identify_distribution(&x, &[FitFamily::Logistic], &opts).unwrap();
        if r[0].1.as_ref().unwrap().anderson_darling.p_value < 0.1 {
            below += 1;
        }
    }
    assert!((8..=35).contains(&below), "{below} of 200 below 0.1");
}

#[test]
fn rejects_bad_input() {
    assert!(fit_distribution(&[1.0, 2.0], FitFamily::Normal).is_err());
    assert!(fit_distribution(&[2.0, 2.0, 2.0, 2.0], FitFamily::Normal).is_err());
    assert!(fit_distribution(&[1.0, f64::NAN, 3.0], FitFamily::Normal).is_err());
    assert!(fit_distribution(&[-1.0, 2.0, 3.0], FitFamily::Lognormal).is_err());
    assert!(fit_distribution(&[0.0, 2.0, 3.0], FitFamily::Weibull).is_err());
    // Threshold forms accept any sign.
    let x = draw(&dist(FitFamily::Lognormal3, -50.0, 2.0, 0.5, 1.0), 200, 4);
    assert!(x.iter().any(|v| *v < 0.0));
    assert!(fit_distribution(&x, FitFamily::Lognormal3).is_ok());
    let r = identify_distribution(&x, &[FitFamily::Weibull, FitFamily::Normal], &IdentifyOptions::default()).unwrap();
    assert!(r[0].1.is_err() && r[1].1.is_ok());
    assert_eq!(FitFamily::from_key("weibull-threshold"), Some(FitFamily::Weibull3));
    assert!(FitFamily::ALL.iter().all(|f| FitFamily::from_key(f.key()) == Some(*f)));
}

#[test]
fn johnson_percentile_fit_maps_its_four_percentiles() {
    for (seed, make) in [
        (41u64, (|z: f64| (0.8 * z + 0.4).sinh() * 3.0 + 10.0) as fn(f64) -> f64), // SU
        (42, |z: f64| 5.0 + 4.0 / (1.0 + (-(z - 0.3) / 1.3).exp())),               // SB
    ] {
        let mut rng = Rng::new(seed);
        let mut s: Vec<f64> = (0..400).map(|_| make(rng.normal())).collect();
        s.sort_by(f64::total_cmp);
        for &z in &[0.5, 0.8] {
            let Some((family, gamma, eta, epsilon, lambda)) = johnson_at(&s, z) else { panic!() };
            let t = JohnsonTransformation {
                family,
                gamma,
                eta,
                epsilon,
                lambda,
                z,
                normality: NormalityTest { statistic: 0.0, p_value: 0.0 },
            };
            let phi = |v: f64| 0.5 * erfc(-v * std::f64::consts::FRAC_1_SQRT_2);
            for k in [-3.0, -1.0, 1.0, 3.0] {
                let x = percentile(&s, phi(k * z));
                close(t.transform(x), k * z, 1e-9);
                close(t.inverse(k * z), x, 1e-9);
            }
        }
    }
}

#[test]
fn johnson_selection_normalizes_skewed_data() {
    let mut rng = Rng::new(5);
    let su: Vec<f64> = (0..300).map(|_| (0.6 * rng.normal() + 0.5).sinh() * 2.0 + 1.0).collect();
    let t = johnson_transformation(&su).unwrap();
    assert_eq!(t.family, JohnsonFamily::Su);
    assert!(t.normality.p_value > 0.1, "{:?}", t.normality);
    assert!((0.25..=1.25).contains(&t.z));
    for &v in su.iter().take(20) {
        close(t.inverse(t.transform(v)), v, 1e-9);
    }
    let sb: Vec<f64> = (0..300).map(|_| 1.0 / (1.0 + (-1.5 * rng.normal()).exp())).collect();
    let t = johnson_transformation(&sb).unwrap();
    assert_eq!(t.family, JohnsonFamily::Sb);
    assert!(t.normality.p_value > 0.1, "{:?}", t.normality);
    assert!(sb.iter().all(|v| t.transform(*v).is_finite()));
    assert!(johnson_transformation(&[1.0, 1.0, 1.0, 1.0]).is_err());
}

/// The density at a threshold is the right-hand limit (from the closed-form
/// densities: exponential `e^{−u/θ}/θ` → `1/θ`; Weibull
/// `(β/α)(u/α)^{β−1}e^{−(u/α)^β}` → `1/α` at `β = 1`, `∞` below, 0 above;
/// SciPy 1.18 `expon.pdf(0, scale=2)` = 0.5, `weibull_min.pdf(0, 1, scale=3)` = 1/3).
#[test]
fn density_at_threshold_is_the_right_limit() {
    let exp = dist(FitFamily::Exponential2, 1.0, 0.0, 2.0, 1.0);
    close(exp.pdf(1.0), 0.5, 1e-15);
    assert_eq!(exp.pdf(0.999), 0.0);
    close(exp.pdf(1.0 + 1e-12), 0.5, 1e-11);
    let weibull = dist(FitFamily::Weibull3, 1.0, 3f64.ln(), 1.0, 1.0);
    close(weibull.pdf(1.0), 1.0 / 3.0, 1e-15);
    close(weibull.pdf(1.0 + 1e-12), 1.0 / 3.0, 1e-11);
    assert_eq!(dist(FitFamily::Weibull3, 1.0, 0.0, 2.0, 1.0).pdf(1.0), f64::INFINITY);
    assert_eq!(dist(FitFamily::Weibull3, 1.0, 0.0, 0.5, 1.0).pdf(1.0), 0.0);
    assert_eq!(dist(FitFamily::Gamma3, 1.0, 0.0, 2.0, 0.5).pdf(1.0), f64::INFINITY);
    assert_eq!(dist(FitFamily::Gamma3, 1.0, 0.0, 2.0, 2.5).pdf(1.0), 0.0);
    assert_eq!(dist(FitFamily::Lognormal3, 1.0, 0.0, 1.0, 1.0).pdf(1.0), 0.0);
}
