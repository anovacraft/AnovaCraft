//! Verify `stat_linear::fit_linear_model` against the NIST StRD certified values.
//!
//! For every dataset, the log relative error (≈ correct significant digits) of each
//! certified quantity is computed and compared with a required minimum. Run with
//! `cargo test -p stat-strd -- --nocapture` to print the full accuracy table.

use stat_linear::{LinearModelFit, OlsOptions, fit_linear_model};
use stat_strd::{LinearRegressionDataset, MAX_LRE, linear_regression_datasets, log_relative_error};

/// Minimum acceptable LRE (correct significant digits) for every certified value,
/// at every difficulty level. The implementation currently achieves ≥ 13.2 on all
/// bundled datasets; the margin absorbs platform differences in libm.
///
/// Values certified as exactly zero (standard errors of the exact-fit Wampler1/2
/// problems) are scored on absolute error instead and use `EXACT_ZERO_MIN_LRE`.
const REQUIRED_LRE: f64 = 12.0;
const EXACT_ZERO_MIN_LRE: f64 = 8.0;

/// Filip's floor is set by the data, not the solver: its certified values are for
/// the decimal x values, which binary64 cannot represent exactly, and the design's
/// condition number (~7·10⁸ after centering and scaling) amplifies that half-ulp
/// rounding to ~7.5 correct digits for *any* f64 implementation. See
/// `filip_accuracy_is_limited_by_data_representation` below.
const FILIP_REQUIRED_LRE: f64 = 7.0;

fn required_lre(d: &LinearRegressionDataset) -> f64 {
    if d.name == "Filip" { FILIP_REQUIRED_LRE } else { REQUIRED_LRE }
}

struct Scored {
    label: String,
    lre: f64,
    certified_zero: bool,
}

fn score(d: &LinearRegressionDataset, fit: &LinearModelFit) -> Vec<Scored> {
    let c = &d.certified;
    let mut out = Vec::new();
    let mut push = |label: String, computed: f64, certified: f64| {
        out.push(Scored { label, lre: log_relative_error(computed, certified), certified_zero: certified == 0.0 });
    };
    for (j, (p, &(est, se))) in fit.parameters().iter().zip(&c.parameters).enumerate() {
        push(format!("B{j} estimate"), p.estimate, est);
        push(format!("B{j} std error"), p.std_error, se);
    }
    let s = fit.summary();
    let a = fit.anova();
    push("residual SD".into(), s.root_mean_square_error, c.residual_sd);
    push("R-squared".into(), s.r_squared, c.r_squared);
    push("SS regression".into(), a.model.sum_of_squares, c.ss_regression);
    push("SS residual".into(), a.error.sum_of_squares, c.ss_residual);
    if let Some(f) = c.f_statistic {
        push("F statistic".into(), a.f_statistic.unwrap_or(f64::NAN), f);
    }
    out
}

#[test]
fn ols_matches_nist_certified_values() {
    let mut failures = Vec::new();
    println!("\n{:<10} {:<8} {:>9} {:>9}  worst quantity", "dataset", "level", "min LRE", "mean LRE");
    for d in linear_regression_datasets() {
        let options = OlsOptions { intercept: d.intercept, ..OlsOptions::default() };
        let fit = match fit_linear_model(&d.predictors, &d.response, &options) {
            Ok(f) => f,
            Err(e) => {
                failures.push(format!("{}: fit failed: {e}", d.name));
                continue;
            }
        };
        assert_eq!(fit.anova().model.df, d.certified.df_regression, "{}", d.name);
        assert_eq!(fit.anova().error.df, d.certified.df_residual, "{}", d.name);

        let scored = score(&d, &fit);
        let worst = scored.iter().min_by(|a, b| a.lre.total_cmp(&b.lre)).expect("non-empty");
        let mean = scored.iter().map(|s| s.lre).sum::<f64>() / scored.len() as f64;
        println!(
            "{:<10} {:<8} {:>9.1} {:>9.1}  {}",
            d.name,
            format!("{:?}", d.difficulty),
            worst.lre,
            mean,
            worst.label
        );

        for s in &scored {
            let need = if s.certified_zero { EXACT_ZERO_MIN_LRE } else { required_lre(&d) };
            if s.lre < need {
                failures.push(format!("{} {}: LRE {:.1} < required {need:.1}", d.name, s.label, s.lre));
            }
        }
    }
    println!("(LRE is capped at {MAX_LRE}: all certified digits correct)");
    assert!(failures.is_empty(), "accuracy failures:\n{}", failures.join("\n"));
}

/// Guard the reason the accuracy is achieved: on Wampler5 (huge residuals, κ ≈ 10⁶
/// after centering) a plain QR solve gets the slopes to only ~7 digits, and the
/// augmented-system refinement against the exact data restores ~13.
#[test]
fn refinement_is_what_makes_wampler5_accurate() {
    let d = linear_regression_datasets().into_iter().find(|d| d.name == "Wampler5").unwrap();
    let lre_b1 = |steps| {
        let o = OlsOptions { refinement_steps: steps, ..OlsOptions::default() };
        let fit = fit_linear_model(&d.predictors, &d.response, &o).unwrap();
        log_relative_error(fit.parameters()[1].estimate, d.certified.parameters[1].0)
    };
    let (unrefined, refined) = (lre_b1(0), lre_b1(2));
    assert!(unrefined < 10.0, "unrefined LRE unexpectedly high: {unrefined}");
    assert!(refined >= REQUIRED_LRE, "refined LRE {refined}");
}

/// Demonstrates why Filip gets its own threshold: perturbing every design value by
/// half an ulp (the rounding any f64 representation of the decimal data incurs)
/// moves the estimates by about as much as they differ from the certified values.
#[test]
fn filip_accuracy_is_limited_by_data_representation() {
    let d = linear_regression_datasets().into_iter().find(|d| d.name == "Filip").unwrap();
    let base = fit_linear_model(&d.predictors, &d.response, &OlsOptions::default()).unwrap();
    let mut perturbed = d.predictors.clone();
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    for v in perturbed.as_mut_slice() {
        state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        let sign = if state >> 63 == 1 { 1.0 } else { -1.0 };
        *v += sign * 0.5 * f64::EPSILON * v.abs();
    }
    let moved = fit_linear_model(&perturbed, &d.response, &OlsOptions::default()).unwrap();
    let sensitivity = base
        .parameters()
        .iter()
        .zip(moved.parameters())
        .map(|(a, b)| log_relative_error(b.estimate, a.estimate))
        .fold(MAX_LRE, f64::min);
    assert!(sensitivity < 9.0, "half-ulp data rounding only moved estimates to LRE {sensitivity:.1}");
}

/// The exact-fit Wampler problems must not report spurious test statistics.
#[test]
fn exact_fits_report_negligible_residuals() {
    for d in linear_regression_datasets().into_iter().filter(|d| d.certified.ss_residual == 0.0) {
        let fit = fit_linear_model(&d.predictors, &d.response, &OlsOptions::default()).unwrap();
        let tss = fit.anova().total.sum_of_squares;
        assert!(fit.anova().error.sum_of_squares <= 1e-20 * tss, "{}", d.name);
        assert!(fit.summary().r_squared > 1.0 - 1e-15, "{}", d.name);
    }
}
