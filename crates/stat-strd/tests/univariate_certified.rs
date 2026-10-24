//! Verify `stat_core::descriptive` against the NIST StRD univariate summary
//! statistics datasets (certified mean, standard deviation and lag-1
//! autocorrelation). Run with `--nocapture` for the accuracy table.

use stat_core::descriptive::{lag1_autocorrelation, moments};
use stat_strd::{MAX_LRE, log_relative_error, univariate_datasets};

#[test]
fn descriptive_statistics_match_nist_certified_values() {
    let mut failures = Vec::new();
    println!("\n{:<9} {:>9} {:>9} {:>9}", "dataset", "mean LRE", "sd LRE", "r1 LRE");
    for d in univariate_datasets() {
        let m = moments(&d.values).unwrap();
        let lre_mean = log_relative_error(m.mean, d.mean);
        let lre_sd = log_relative_error(m.std_dev.unwrap(), d.std_dev);
        let lre_r1 = log_relative_error(lag1_autocorrelation(&d.values).unwrap(), d.autocorrelation);
        println!("{:<9} {lre_mean:>9.1} {lre_sd:>9.1} {lre_r1:>9.1}", d.name);
        let (need_mean, need_sd) = required(d.name);
        for (what, got, need) in [("mean", lre_mean, need_mean), ("sd", lre_sd, need_sd), ("r1", lre_r1, need_sd)] {
            if got < need {
                failures.push(format!("{} {what}: LRE {got:.1} < {need}", d.name));
            }
        }
    }
    println!("(LRE capped at {MAX_LRE})");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Required LRE for (mean, sd and autocorrelation).
///
/// NumAcc3 and NumAcc4 store values like 1000000.1 and 10000000.1, which binary64
/// cannot represent exactly: the representation error (up to half an ulp,
/// ≈ 5.8e-11 and 9.3e-10) is a sizeable fraction of the certified sd of 0.1, so no
/// f64 computation can match more digits than the data carry. Their sd and
/// autocorrelation are held to the data-representation floor instead.
fn required(name: &str) -> (f64, f64) {
    match name {
        "NumAcc3" => (13.0, 9.0),
        "NumAcc4" => (13.0, 8.0),
        _ => (13.0, 13.0),
    }
}

/// Demonstrates the NumAcc4 floor from the stored data alone: the certified sd of
/// 0.1 assumes the pair 10000000.1 / 10000000.3 differ by exactly 0.2, but as
/// binary64 values they differ by 0.19999999925…, a relative error of ≈ 3.7e-9 —
/// so even an exact computation on the stored values agrees with the certified
/// sd to only about 8.4 digits.
#[test]
fn numacc4_floor_is_set_by_binary_representation() {
    let (low, high) = (10_000_000.1_f64, 10_000_000.3_f64);
    let spacing = high - low; // exact (Sterbenz lemma)
    let relative = ((spacing - 0.2) / 0.2).abs();
    assert!(relative > 1e-9 && relative < 1e-8, "{relative:e}");
    let attainable = -relative.log10();
    assert!((8.0..9.0).contains(&attainable), "attainable LRE {attainable}");
}
