//! Verify `stat_linear::one_way_anova` against the NIST StRD ANOVA datasets.
//! Run with `--nocapture` for the accuracy table.

use stat_linear::one_way_anova;
use stat_strd::{Difficulty, MAX_LRE, anova_datasets, log_relative_error, parse_strd_anova};

/// AtmWtAg, NIST's one ANOVA problem built from real measurements, cannot be
/// generated; NIST's file is bundled here (`scripts/fetch-nist-anova.sh`
/// downloads it again).
const ATMWTAG: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/data/anova/AtmWtAg.dat");

#[test]
fn one_way_anova_matches_nist_certified_values() {
    let mut failures = Vec::new();
    println!("\n{:<7} {:>8} {:>8} {:>8} {:>8} {:>8}  (LRE)", "dataset", "SS btw", "SS wth", "F", "R²", "resid sd");
    let mut all = anova_datasets();
    match std::fs::read_to_string(ATMWTAG) {
        Ok(text) => all.push(parse_strd_anova("AtmWtAg", Difficulty::Average, &text).expect("AtmWtAg.dat parses")),
        Err(_) => println!("AtmWtAg.dat not found (run scripts/fetch-nist-anova.sh to add it)"),
    }
    for d in all {
        let a = one_way_anova(&d.groups).unwrap();
        let c = d.certified;
        assert_eq!((a.df_between, a.df_within), (c.df_between, c.df_within), "{}", d.name);
        let lre = [
            log_relative_error(a.ss_between, c.ss_between),
            log_relative_error(a.ss_within, c.ss_within),
            log_relative_error(a.f_statistic.unwrap(), c.f_statistic),
            log_relative_error(a.r_squared, c.r_squared),
            log_relative_error(a.residual_std_dev, c.residual_sd),
        ];
        println!("{:<7} {:>8.1} {:>8.1} {:>8.1} {:>8.1} {:>8.1}", d.name, lre[0], lre[1], lre[2], lre[3], lre[4]);
        let need = required(d.name);
        for (what, got) in ["SS between", "SS within", "F", "R²", "residual sd"].iter().zip(lre) {
            if got < need {
                failures.push(format!("{} {what}: LRE {got:.1} < {need}", d.name));
            }
        }
    }
    println!("(LRE capped at {MAX_LRE})");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Required LRE.
///
/// SmLs04–06 and SmLs07–09 store values such as 1000000.3 and
/// 1000000000001.3, which binary64 cannot represent exactly. The representation
/// error is a fraction of the ±0.1 deviations the certified sums of squares are
/// built from, so no f64 computation can match more digits than the stored data
/// carry; these are held to their demonstrated floors (see the tests below).
fn required(name: &str) -> f64 {
    match name {
        "SmLs04" | "SmLs05" | "SmLs06" => 8.0,
        "SmLs07" | "SmLs08" | "SmLs09" => 2.0,
        // AtmWtAg's values (e.g. 107.8681568) are not exact in binary, and the
        // sums of squares come from deviations of about 10⁻⁵: computing them in
        // exact rational arithmetic from the stored binary64 data matches only
        // 10.2 (between) and 10.9 (within) digits, so CLAUDE.md's 10 is the bar.
        "AtmWtAg" => 10.0,
        // SiRstv's values (e.g. 196.3052) are not exact in binary either; its
        // 12.9–13.4 digits are at that limit and well above CLAUDE.md's 10.
        _ => 12.0,
    }
}

/// The floor for an offset dataset: the stored difference between two values
/// that are certified to differ by exactly 0.1, relative to 0.1. Sums of squares
/// of such deviations cannot be more accurate than this.
fn representation_floor(offset: f64) -> f64 {
    let (a, b) = (offset + 1.3, offset + 1.4);
    let stored = b - a; // exact (Sterbenz lemma)
    -(((stored - 0.1) / 0.1).abs()).log10()
}

#[test]
fn offset_datasets_floors_are_set_by_binary_representation() {
    // 10⁶ offset: ≈ 9 digits survive; 10¹² offset: only ≈ 3.
    let f6 = representation_floor(1e6);
    let f12 = representation_floor(1e12);
    assert!((8.0..11.0).contains(&f6), "SmLs04–06 floor {f6}");
    assert!((2.0..4.5).contains(&f12), "SmLs07–09 floor {f12}");
}
