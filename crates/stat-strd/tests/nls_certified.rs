//! NIST StRD nonlinear least squares: every problem from both certified
//! starting points, scored by the log relative error (LRE) of each certified
//! value.
//!
//! The 27 NIST files are in `crates/stat-strd/data/nls/`
//! (`scripts/fetch-nist-nls.sh` downloads them again). The test checks each
//! file it finds and prints a note naming any it did not find.
//!
//! # Criterion
//! From each start the fit must converge, and every certified value — each
//! estimate, each standard deviation, the residual sum of squares and the
//! residual standard deviation — must be reproduced with LRE ≥
//! [`REQUIRED_LRE`] (four correct significant digits, the threshold commonly
//! used to call an NLS solution correct in assessments of statistical
//! software; NIST certifies 11 digits, so the LRE is capped at 11). The fits
//! use the symbolic derivatives of the model as written in the file, a
//! relative offset tolerance of 10⁻¹⁰ and up to [`MAX_ITERATIONS`] iterations
//! per solver run. The LREs
//! achieved are printed (`cargo test -p stat-strd --test nls_certified --
//! --nocapture`).
//!
//! The same harness runs on `tests/data/synthetic_nls.dat`, a file in NIST's
//! layout with invented data whose "certified" values were computed with
//! SciPy (`tests/data/synthetic_nls.py`), so the parser and the scoring are
//! exercised even without the NIST files.

use stat_core::Matrix;
use stat_linear::nonlinear::{ModelFunction, NonlinearOptions, fit_nonlinear};
use stat_strd::log_relative_error;
use stat_strd::nls::{ModelExpr, NLS_DATASETS, NlsProblem, parse_nls};

/// Minimum LRE of every certified value.
const REQUIRED_LRE: f64 = 4.0;
/// Lanczos1's certified residual sum of squares is 1.4·10⁻²⁵: residuals of
/// about 10⁻¹³ on responses near 1, so half an ulp of each stored response
/// (about 10⁻¹⁶) already moves the minimum RSS in its third digit (shown by
/// `lanczos1_floor_is_the_data_representation`). Its RSS, residual SD and
/// standard deviations (which scale with the residual SD) are held to this
/// floor instead (the CLAUDE.md data-representation exception); its estimates
/// still need [`REQUIRED_LRE`].
const LANCZOS1_RESIDUAL_LRE: f64 = 2.5;
/// Iteration limit of each solver run. MGH10 from its first start (every
/// parameter 10–1000 times its solution) needs about 20 000 iterations along
/// its curved valley; every other problem converges in under 2 500.
const MAX_ITERATIONS: usize = 50_000;
/// NIST certifies 11 significant digits.
const NLS_MAX_LRE: f64 = 11.0;

struct ExprModel<'a> {
    model: &'a ModelExpr,
    derivatives: Vec<ModelExpr>,
    x: &'a [Vec<f64>],
}

impl ModelFunction for ExprModel<'_> {
    fn n_observations(&self) -> usize {
        self.x.len()
    }
    fn values(&mut self, theta: &[f64], out: &mut [f64]) -> bool {
        for (o, x) in out.iter_mut().zip(self.x) {
            *o = self.model.eval(theta, x);
        }
        out.iter().all(|v| v.is_finite())
    }
    fn derivatives(&mut self, theta: &[f64], out: &mut Matrix) -> Option<bool> {
        for (i, x) in self.x.iter().enumerate() {
            for (j, d) in self.derivatives.iter().enumerate() {
                out[(i, j)] = d.eval(theta, x);
            }
        }
        Some(out.is_finite())
    }
}

fn lre(computed: f64, certified: f64) -> f64 {
    log_relative_error(computed, certified).min(NLS_MAX_LRE)
}

/// Fit `p` from start `s` and return the smallest LRE over the certified
/// values (with a description of each).
fn score(p: &NlsProblem, s: usize) -> Result<(f64, String), String> {
    let n_par = p.certified.len();
    let derivatives = (0..n_par).map(|k| p.model.derivative(k)).collect();
    let mut model = ExprModel { model: &p.model, derivatives, x: &p.x };
    let y: Vec<f64> = if p.log_response { p.y.iter().map(|v| v.ln()).collect() } else { p.y.clone() };
    let mut options = NonlinearOptions::new((1..=n_par).map(|k| format!("b{k}")).collect());
    options.max_iterations = MAX_ITERATIONS;
    options.relative_offset = 1e-10;
    options.profile_intervals = false;
    let fit = fit_nonlinear(&mut model, &y, &p.starts[s], &options).map_err(|e| e.to_string())?;
    if !fit.converged() {
        return Err(format!("{} after {} iterations", fit.termination.label(), fit.iterations));
    }
    if fit.df != p.df {
        // Rat43 states 9 degrees of freedom for 15 observations and 4
        // parameters; its certified residual SD is √(RSS/11), so the stated
        // count is a misprint. Accept a differing count only when the
        // certified residual SD agrees with n − p.
        let implied = (p.residual_ss / fit.df as f64).sqrt();
        if lre(implied, p.residual_sd) < 9.0 {
            return Err(format!("{} residual df, certified {}", fit.df, p.df));
        }
    }
    let est = fit.parameters.iter().map(|q| q.estimate).zip(&p.certified).map(|(a, c)| lre(a, c.0));
    let se = fit.parameters.iter().map(|q| q.std_error).zip(&p.certified).map(|(a, c)| lre(a, c.1));
    let est: Vec<f64> = est.collect();
    let se: Vec<f64> = se.collect();
    let rss = lre(fit.sse, p.residual_ss);
    let rsd = lre(fit.residual_se, p.residual_sd);
    let min = |v: &[f64]| v.iter().copied().fold(f64::INFINITY, f64::min);
    // Shortfall below the required LRE, reported on the LRE scale: residual
    // quantities of Lanczos1 are scored against their floor.
    let residual_floor = if p.residual_ss < 1e-20 { LANCZOS1_RESIDUAL_LRE } else { REQUIRED_LRE };
    let shift = REQUIRED_LRE - residual_floor;
    let worst = min(&est).min(min(&se) + shift).min(rss + shift).min(rsd + shift);
    Ok((
        worst,
        format!(
            "estimates {:.1}, std devs {:.1}, RSS {rss:.1}, residual SD {rsd:.1}, {} iterations, relative offset {:.1e}",
            min(&est),
            min(&se),
            fit.iterations,
            fit.relative_offset
        ),
    ))
}

fn check(name: &str, text: &str, failures: &mut Vec<String>) {
    let p = match parse_nls(text) {
        Ok(p) => p,
        Err(e) => {
            failures.push(format!("{name}: cannot parse ({e})"));
            return;
        }
    };
    for s in 0..2 {
        match score(&p, s) {
            Ok((worst, detail)) => {
                println!("{name:10} start {}: score {worst:4.1} ({detail})", s + 1);
                if worst < REQUIRED_LRE {
                    failures.push(format!("{name} start {}: score {worst:.1} < {REQUIRED_LRE} ({detail})", s + 1));
                }
            }
            Err(e) => failures.push(format!("{name} start {}: {e}", s + 1)),
        }
    }
}

#[test]
fn nist_nonlinear_certified_values() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data/nls");
    let mut failures = Vec::new();
    let mut missing = Vec::new();
    for name in NLS_DATASETS {
        match std::fs::read_to_string(dir.join(format!("{name}.dat"))) {
            Ok(text) => check(name, &text, &mut failures),
            Err(_) => missing.push(name),
        }
    }
    if missing.len() == NLS_DATASETS.len() {
        println!(
            "NOTE: skipping the NIST StRD nonlinear regression check: none of the 27 files is in {}. \
             Run scripts/fetch-nist-nls.sh to download them.",
            dir.display()
        );
    } else if !missing.is_empty() {
        println!("NOTE: NIST StRD nonlinear files not found (not checked): {}", missing.join(", "));
    }
    assert!(failures.is_empty(), "NIST StRD nonlinear regression failures:\n{}", failures.join("\n"));
}

/// The floor for Lanczos1: refitting after moving each stored response by
/// half an ulp (random signs) changes the minimum RSS in its third digit,
/// so no binary64 computation can certify it to four.
#[test]
fn lanczos1_floor_is_the_data_representation() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data/nls");
    let Ok(text) = std::fs::read_to_string(dir.join("Lanczos1.dat")) else {
        println!("NOTE: Lanczos1.dat not present; floor not checked");
        return;
    };
    let p = parse_nls(&text).unwrap();
    let rss = |y: &[f64]| {
        let n_par = p.certified.len();
        let derivatives = (0..n_par).map(|k| p.model.derivative(k)).collect();
        let mut model = ExprModel { model: &p.model, derivatives, x: &p.x };
        let mut options = NonlinearOptions::new((1..=n_par).map(|k| format!("b{k}")).collect());
        options.max_iterations = 5000;
        options.relative_offset = 1e-10;
        options.profile_intervals = false;
        fit_nonlinear(&mut model, y, &p.starts[1], &options).unwrap().sse
    };
    let base = rss(&p.y);
    let mut worst = f64::INFINITY;
    for seed in 1..5u64 {
        let mut state = seed.wrapping_mul(6_364_136_223_846_793_005);
        let y: Vec<f64> =
            p.y.iter()
                .map(|v| {
                    state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
                    let sign = if state >> 63 == 1 { 1.0 } else { -1.0 };
                    v + sign * 0.5 * v.abs() * f64::EPSILON
                })
                .collect();
        worst = worst.min(log_relative_error(rss(&y), base));
    }
    println!("Lanczos1: half-ulp data perturbations move the RSS to LRE {worst:.1}");
    assert!(worst < 4.0, "the perturbations should move the RSS beyond four digits, got LRE {worst}");
    assert!(worst >= LANCZOS1_RESIDUAL_LRE - 0.5, "{worst}");
}

#[test]
fn harness_runs_on_a_synthetic_file() {
    let mut failures = Vec::new();
    check("Synthetic", include_str!("data/synthetic_nls.dat"), &mut failures);
    assert!(failures.is_empty(), "{failures:?}");
    // The synthetic "certified" values come from SciPy with 11 digits; the
    // fit should reproduce nearly all of them.
    let p = parse_nls(include_str!("data/synthetic_nls.dat")).unwrap();
    for s in 0..2 {
        let (worst, _) = score(&p, s).unwrap();
        assert!(worst >= 8.0, "start {}: {worst}", s + 1);
    }
}

/// Four hard model shapes in NIST's layout with invented data
/// (`tests/data/hard_nls.py`; "certified" values from SciPy started near
/// the solution), each fitted from a far and a near start.
#[test]
fn hard_synthetic_problems_from_far_starts() {
    let mut failures = Vec::new();
    for (name, text) in [
        ("SatCurve", include_str!("data/hard/SatCurve.dat")),
        ("GenLogistic", include_str!("data/hard/GenLogistic.dat")),
        ("NarrowPeak", include_str!("data/hard/NarrowPeak.dat")),
        ("RationalKinetics", include_str!("data/hard/RationalKinetics.dat")),
    ] {
        check(name, text, &mut failures);
    }
    assert!(failures.is_empty(), "{failures:?}");
}
