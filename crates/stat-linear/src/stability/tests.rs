use super::*;

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol * b.abs().max(1.0)
}

/// Three batches measured at 0–24 months (the "Stability" sample dataset).
/// Reference values from an independent standard-library Python script
/// (`ref_stab.py` in the PR notes: OLS by Gauss–Jordan normal equations,
/// t quantiles by numerical integration of the density)
/// following ICH Q1E's decision rules.
pub(crate) const TIME: [f64; 21] =
    [0.0, 3.0, 6.0, 9.0, 12.0, 18.0, 24.0, 0.0, 3.0, 6.0, 9.0, 12.0, 18.0, 24.0, 0.0, 3.0, 6.0, 9.0, 12.0, 18.0, 24.0];
pub(crate) const ASSAY: [f64; 21] = [
    100.7, 100.6, 100.0, 99.7, 99.1, 98.7, 98.4, 101.7, 101.6, 101.0, 100.7, 100.2, 98.9, 99.0, 100.4, 100.0, 98.9,
    98.6, 98.5, 98.0, 97.6,
];

fn batches() -> Vec<String> {
    ["A", "B", "C"].iter().flat_map(|b| std::iter::repeat_n((*b).to_owned(), 7)).collect()
}

#[test]
fn common_slope_model_matches_reference() {
    let r = shelf_life(&TIME, &ASSAY, &batches(), &ShelfLifeOptions::lower(95.0)).unwrap();
    let slopes = r.slope_test.as_ref().unwrap();
    assert_eq!((slopes.df_numerator, slopes.df_denominator), (2, 15));
    assert!(close(slopes.f, 0.848_174_451_424_795_4, 1e-9));
    // With 2 numerator df the F tail is exact: P(F > f) = (1 + 2f/d₂)^(−d₂/2).
    assert!(close(slopes.p_value, 0.447_737_124_774_327_5, 1e-10));
    let intercepts = r.intercept_test.as_ref().unwrap();
    assert_eq!((intercepts.df_numerator, intercepts.df_denominator), (2, 17));
    assert!(close(intercepts.f, 47.751_190_631_999_01, 1e-9));
    assert!((intercepts.p_value / 1.056_674_566_776_680_4e-7 - 1.0).abs() < 1e-8);
    assert_eq!(r.model, PoolingModel::CommonSlope);
    assert_eq!(r.df_error, 17);
    assert!(close(r.rmse, 0.303_766_258_027_327_7, 1e-10));
    let slope = -0.115_968_063_872_256_22;
    let intercepts = [
        100.792_814_371_257_52,
        100.792_814_371_257_52 + 0.842_857_142_857_123_4,
        100.792_814_371_257_52 - 0.742_857_142_857_165_5,
    ];
    for (line, a) in r.lines.iter().zip(intercepts) {
        assert!(close(line.slope, slope, 1e-10) && close(line.intercept, a, 1e-10));
        assert_eq!(line.n, 7);
    }
    assert!(close(r.lines[0].shelf_life.unwrap(), 45.196_832_979_365_91, 1e-6));
    assert!(r.lines[1].shelf_life.is_none(), "beyond the 48-month horizon");
    assert!(close(r.lines[2].shelf_life.unwrap(), 39.461_310_015_069_216, 1e-6));
    assert!(close(r.shelf_life.unwrap(), 39.461_310_015_069_216, 1e-6));
    assert_eq!(r.limiting_line, Some(2));
    assert_eq!(r.horizon, 48.0);
    // At the shelf life the one-sided lower bound equals the limit.
    let (_, lo, _) = r.bounds(2, r.shelf_life.unwrap()).unwrap();
    assert!((lo - 95.0).abs() < 1e-8);
}

#[test]
fn separate_lines_when_a_batch_degrades_faster() {
    let mut y = ASSAY;
    for (v, (t, b)) in y.iter_mut().zip(TIME.iter().zip(batches())) {
        if b == "C" {
            *v = ((*v - 0.10 * t) * 100.0).round() / 100.0;
        }
    }
    let r = shelf_life(&TIME, &y, &batches(), &ShelfLifeOptions::lower(95.0)).unwrap();
    assert_eq!(r.model, PoolingModel::Separate);
    assert!(r.intercept_test.is_none());
    assert!(close(r.slope_test.as_ref().unwrap().f, 14.715_423_836_838_696, 1e-8));
    assert!((r.slope_test.as_ref().unwrap().p_value / 2.904_356_679_853_469_6e-4 - 1.0).abs() < 1e-8);
    assert_eq!(r.df_error, 15);
    assert!(close(r.rmse, 0.306_516_061_256_802_4, 1e-9));
    let want = [45.492_748_602_830_23, 44.859_925_937_321_18, 21.864_981_004_254_02];
    for (line, w) in r.lines.iter().zip(want) {
        assert!(close(line.shelf_life.unwrap(), w, 1e-6), "{:?} vs {w}", line.shelf_life);
    }
    assert!(close(r.lines[2].slope, -0.104_091_816_367_265_44 - 0.108_882_235_528_938_08, 1e-10));
    assert_eq!(r.limiting_line, Some(2));
}

#[test]
fn pooled_upper_and_two_sided_limits() {
    // Identical batches pool; with an upper limit on an increasing impurity
    // the upper bound is used, and both limits use two-sided bounds.
    let t = [0.0, 3.0, 6.0, 9.0, 12.0, 0.0, 3.0, 6.0, 9.0, 12.0];
    let y = [0.10, 0.16, 0.21, 0.29, 0.33, 0.11, 0.15, 0.22, 0.28, 0.34];
    let b: Vec<String> = (0..10).map(|i| if i < 5 { "1" } else { "2" }.to_owned()).collect();
    let up = ShelfLifeOptions { lower_limit: None, upper_limit: Some(0.5), ..ShelfLifeOptions::lower(0.0) };
    let r = shelf_life(&t, &y, &b, &up).unwrap();
    assert_eq!(r.model, PoolingModel::Pooled);
    assert_eq!(r.lines.len(), 1);
    let s = r.shelf_life.unwrap();
    let (mean, _, hi) = r.bounds(0, s).unwrap();
    assert!((hi - 0.5).abs() < 1e-9 && mean < 0.5);
    let both = ShelfLifeOptions { lower_limit: Some(0.0), upper_limit: Some(0.5), ..up.clone() };
    let r2 = shelf_life(&t, &y, &b, &both).unwrap();
    assert!(r2.shelf_life.unwrap() < s, "the two-sided bound is wider");
    // Errors.
    let none = ShelfLifeOptions { lower_limit: None, ..up.clone() };
    let none = ShelfLifeOptions { upper_limit: None, ..none };
    assert!(shelf_life(&t, &y, &b, &none).is_err());
    assert!(shelf_life(&t[..1], &y[..1], &b[..1], &up).is_err());
    let one_time: Vec<String> = (0..10).map(|i| if i == 0 { "x" } else { "y" }.to_owned()).collect();
    assert!(shelf_life(&t, &y, &one_time, &up).is_err());
    // A single batch is simply the pooled line.
    let single = shelf_life(&t[..5], &y[..5], &b[..5], &up).unwrap();
    assert!(single.slope_test.is_none() && single.model == PoolingModel::Pooled);
}

/// ICH Q1E Appendix A: a proposed shelf life of at most `min(2X, X + 12
/// months)` for long-term data covering `X`. With the sample's 24 months the
/// 12-month part binds (36 < 48); the same numbers in days or weeks leave
/// twice the data as the limit, and in years 12 months is one unit.
#[test]
fn extrapolation_limit_follows_ich_q1e() {
    let run = |unit: Option<StabilityTimeUnit>, horizon: Option<f64>| {
        let options = ShelfLifeOptions { time_unit: unit, horizon, ..ShelfLifeOptions::lower(95.0) };
        shelf_life(&TIME, &ASSAY, &batches(), &options).unwrap()
    };
    let months = run(Some(StabilityTimeUnit::Months), None);
    assert_eq!((months.extrapolation_limit, months.extrapolation_rule), (36.0, ExtrapolationRule::TwelveMonthsBeyond));
    // The bound meets 95% at 39.46 months, beyond the limit.
    assert!(close(months.shelf_life.unwrap(), 39.461_310_015_069_216, 1e-6));
    assert_eq!(months.supported_shelf_life(), Some(36.0));
    for unit in [None, Some(StabilityTimeUnit::Days), Some(StabilityTimeUnit::Weeks)] {
        let r = run(unit, None);
        assert_eq!((r.extrapolation_limit, r.extrapolation_rule), (48.0, ExtrapolationRule::TwiceObserved), "{unit:?}");
        assert!(close(r.supported_shelf_life().unwrap(), 39.461_310_015_069_216, 1e-6));
    }
    let years = run(Some(StabilityTimeUnit::Years), None);
    assert_eq!(years.extrapolation_limit, 25.0);
    assert_eq!(years.supported_shelf_life(), Some(25.0));
    // A bound that never meets the limit within a horizon past the limit
    // supports the limit; within a shorter horizon nothing can be said.
    let never = |horizon| {
        let options = ShelfLifeOptions { horizon: Some(horizon), ..ShelfLifeOptions::lower(80.0) };
        shelf_life(&TIME, &ASSAY, &batches(), &options).unwrap()
    };
    assert_eq!(never(40.0).supported_shelf_life(), Some(36.0));
    assert_eq!(never(30.0).supported_shelf_life(), None);
    assert_eq!(StabilityTimeUnit::from_label("weeks"), Some(StabilityTimeUnit::Weeks));
    assert_eq!(StabilityTimeUnit::from_label("fortnights"), None);
}

/// Three batches measured only at 0 and 12 months: the separate-lines model
/// has no residual df, so the slope test is skipped with a note and the
/// intercept test chooses between parallel lines and one line. Reference
/// (`numpy.linalg.lstsq`, `scipy.stats.f.sf`, `scipy.stats.t.ppf` and
/// `scipy.optimize.brentq` on the one-sided 95% lower bound): F = 67 on
/// (2, 2) df, p = 0.0147, common slope −0.25, shelf life 16.351450308 months
/// for batch C.
#[test]
fn two_times_per_batch_skip_the_slope_test() {
    let time = [0.0, 12.0, 0.0, 12.0, 0.0, 12.0];
    let assay = [100.0, 97.0, 101.0, 98.2, 99.6, 96.4];
    let batch: Vec<String> = ["A", "A", "B", "B", "C", "C"].iter().map(|s| (*s).to_owned()).collect();
    let r = shelf_life(&time, &assay, &batch, &ShelfLifeOptions::lower(95.0)).unwrap();
    assert!(r.slope_test.is_none());
    assert!(r.notes.iter().any(|n| n.contains("could not be tested")), "{:?}", r.notes);
    let intercepts = r.intercept_test.as_ref().unwrap();
    assert_eq!((intercepts.df_numerator, intercepts.df_denominator), (2, 2));
    assert!(close(intercepts.f, 67.0, 1e-9));
    assert!(close(intercepts.p_value, 0.014_705_882_352_940_531, 1e-9));
    assert_eq!(r.model, PoolingModel::CommonSlope);
    assert!(close(r.lines[0].slope, -0.25, 1e-12));
    assert!(close(r.shelf_life.unwrap(), 16.351_450_308_264_86, 1e-6));
    assert_eq!(r.limiting_line, Some(2));
}

/// Lines that fit every batch exactly (residual SS 0): identical lines pool
/// (F = 0, p = 1) instead of the old 0/0 → p = 0; different exact slopes
/// give F = ∞, p = 0.
#[test]
fn exact_fits_pool_or_separate_cleanly() {
    let months = [0.0, 3.0, 6.0, 9.0];
    let time: Vec<f64> = (0..3).flat_map(|_| months).collect();
    let batch: Vec<String> = ["A", "B", "C"].iter().flat_map(|b| std::iter::repeat_n((*b).to_owned(), 4)).collect();
    let same: Vec<f64> = time.iter().map(|t| 100.0 - t).collect();
    let r = shelf_life(&time, &same, &batch, &ShelfLifeOptions::lower(95.0)).unwrap();
    let slopes = r.slope_test.as_ref().unwrap();
    assert!(slopes.f == 0.0 && slopes.p_value == 1.0, "{slopes:?}");
    assert_eq!(r.model, PoolingModel::Pooled);
    assert!(close(r.shelf_life.unwrap(), 5.0, 1e-6));
    let steeper: Vec<f64> =
        time.iter().zip(&batch).map(|(t, b)| if b == "C" { 100.0 - 2.0 * t } else { 100.0 - t }).collect();
    let r = shelf_life(&time, &steeper, &batch, &ShelfLifeOptions::lower(95.0)).unwrap();
    let slopes = r.slope_test.as_ref().unwrap();
    // F = ∞ for an exact zero residual SS; huge if QR leaves rounding.
    assert!(slopes.f > 1e12 && slopes.p_value < 1e-12, "{slopes:?}");
    assert_eq!(r.model, PoolingModel::Separate);
}
