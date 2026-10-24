//! StRD linear-regression datasets and certified values.
//!
//! Most datasets are transcribed into Rust tables; Filip is parsed from NIST's
//! original `.dat` file, bundled under `data/`.

use crate::{CertifiedValues, Difficulty, LinearRegressionDataset};
use stat_core::Matrix;

/// All bundled linear-regression reference datasets, ordered by NIST difficulty.
///
/// # Panics
/// Never in practice: the embedded tables have consistent shapes, which the
/// crate's tests verify.
#[must_use]
pub fn linear_regression_datasets() -> Vec<LinearRegressionDataset> {
    vec![
        norris(),
        pontius(),
        no_int1(),
        no_int2(),
        longley(),
        wampler(1),
        wampler(2),
        wampler(3),
        wampler(4),
        wampler(5),
        filip(),
    ]
}

fn columns(cols: &[Vec<f64>]) -> Matrix {
    Matrix::from_columns(cols).expect("embedded dataset columns have equal length")
}

/// Expand `x` into `[x, x², …, x^degree]`.
fn polynomial(x: &[f64], degree: i32) -> Matrix {
    let cols: Vec<Vec<f64>> = (1..=degree).map(|d| x.iter().map(|v| v.powi(d)).collect()).collect();
    columns(&cols)
}

/// Filip: degree-10 polynomial, 82 observations, NIST's hardest linear problem.
fn filip() -> LinearRegressionDataset {
    let parsed = crate::nist_format::parse_strd_linear(include_str!("../data/Filip.dat"))
        .expect("bundled Filip.dat is well-formed");
    let x: Vec<f64> = parsed.rows.iter().map(|r| r[1]).collect();
    LinearRegressionDataset {
        name: "Filip",
        difficulty: Difficulty::Higher,
        intercept: true,
        predictors: polynomial(&x, 10),
        response: parsed.rows.iter().map(|r| r[0]).collect(),
        certified: parsed.certified,
    }
}

fn norris() -> LinearRegressionDataset {
    #[rustfmt::skip]
    const DATA: [(f64, f64); 36] = [
        (0.1, 0.2), (338.8, 337.4), (118.1, 118.2), (888.0, 884.6), (9.2, 10.1), (228.1, 226.5),
        (668.5, 666.3), (998.5, 996.3), (449.1, 448.6), (778.9, 777.0), (559.2, 558.2), (0.3, 0.4),
        (0.1, 0.6), (778.1, 775.5), (668.8, 666.9), (339.3, 338.0), (448.9, 447.5), (10.8, 11.6),
        (557.7, 556.0), (228.3, 228.1), (998.0, 995.8), (888.8, 887.6), (119.6, 120.2), (0.3, 0.3),
        (0.6, 0.3), (557.6, 556.8), (339.3, 339.1), (888.0, 887.2), (998.5, 999.0), (778.9, 779.0),
        (10.2, 11.1), (117.6, 118.3), (228.9, 229.2), (668.4, 669.1), (449.2, 448.9), (0.2, 0.5),
    ];
    LinearRegressionDataset {
        name: "Norris",
        difficulty: Difficulty::Lower,
        intercept: true,
        predictors: columns(&[DATA.iter().map(|r| r.1).collect()]),
        response: DATA.iter().map(|r| r.0).collect(),
        certified: CertifiedValues {
            parameters: vec![
                (-0.262_323_073_774_029, 0.232_818_234_301_152),
                (1.002_116_818_020_45, 0.429_796_848_199_937e-3),
            ],
            residual_sd: 0.884_796_396_144_373,
            r_squared: 0.999_993_745_883_712,
            df_regression: 1,
            df_residual: 34,
            ss_regression: 4_255_954.132_323_69,
            ss_residual: 26.617_398_529_422_4,
            f_statistic: Some(5_436_385.540_797_85),
        },
    }
}

fn pontius() -> LinearRegressionDataset {
    #[rustfmt::skip]
    const DATA: [(f64, f64); 40] = [
        (0.11019, 150_000.0), (0.21956, 300_000.0), (0.32949, 450_000.0), (0.43899, 600_000.0),
        (0.54803, 750_000.0), (0.65694, 900_000.0), (0.76562, 1_050_000.0), (0.87487, 1_200_000.0),
        (0.98292, 1_350_000.0), (1.09146, 1_500_000.0), (1.20001, 1_650_000.0), (1.30822, 1_800_000.0),
        (1.41599, 1_950_000.0), (1.52399, 2_100_000.0), (1.63194, 2_250_000.0), (1.73947, 2_400_000.0),
        (1.84646, 2_550_000.0), (1.95392, 2_700_000.0), (2.06128, 2_850_000.0), (2.16844, 3_000_000.0),
        (0.11052, 150_000.0), (0.22018, 300_000.0), (0.32939, 450_000.0), (0.43886, 600_000.0),
        (0.54798, 750_000.0), (0.65739, 900_000.0), (0.76596, 1_050_000.0), (0.87474, 1_200_000.0),
        (0.98300, 1_350_000.0), (1.09150, 1_500_000.0), (1.20004, 1_650_000.0), (1.30818, 1_800_000.0),
        (1.41613, 1_950_000.0), (1.52408, 2_100_000.0), (1.63159, 2_250_000.0), (1.73965, 2_400_000.0),
        (1.84696, 2_550_000.0), (1.95445, 2_700_000.0), (2.06177, 2_850_000.0), (2.16829, 3_000_000.0),
    ];
    let x: Vec<f64> = DATA.iter().map(|r| r.1).collect();
    LinearRegressionDataset {
        name: "Pontius",
        difficulty: Difficulty::Lower,
        intercept: true,
        predictors: polynomial(&x, 2),
        response: DATA.iter().map(|r| r.0).collect(),
        certified: CertifiedValues {
            parameters: vec![
                (0.673_565_789_473_684e-3, 0.107_938_612_033_077e-3),
                (0.732_059_160_401_003e-6, 0.157_817_399_981_659e-9),
                (-0.316_081_871_345_029e-14, 0.486_652_849_992_036e-16),
            ],
            residual_sd: 0.205_177_424_076_185e-3,
            r_squared: 0.999_999_900_178_537,
            df_regression: 2,
            df_residual: 37,
            ss_regression: 15.604_034_324_419_8,
            ss_residual: 0.155_761_768_796_992e-5,
            f_statistic: Some(185_330_865.995_752),
        },
    }
}

fn no_int1() -> LinearRegressionDataset {
    // x = 60, 61, …, 70 and y = 130, 131, …, 140.
    let x: Vec<f64> = (60..=70).map(f64::from).collect();
    let y: Vec<f64> = (130..=140).map(f64::from).collect();
    LinearRegressionDataset {
        name: "NoInt1",
        difficulty: Difficulty::Average,
        intercept: false,
        predictors: columns(&[x]),
        response: y,
        certified: CertifiedValues {
            parameters: vec![(2.074_380_165_289_26, 0.165_289_256_198_347e-1)],
            residual_sd: 3.567_530_340_063_38,
            r_squared: 0.999_365_492_298_663,
            df_regression: 1,
            df_residual: 10,
            ss_regression: 200_457.727_272_727,
            ss_residual: 127.272_727_272_727,
            f_statistic: Some(15_750.25),
        },
    }
}

fn no_int2() -> LinearRegressionDataset {
    LinearRegressionDataset {
        name: "NoInt2",
        difficulty: Difficulty::Average,
        intercept: false,
        predictors: columns(&[vec![4.0, 5.0, 6.0]]),
        response: vec![3.0, 4.0, 4.0],
        certified: CertifiedValues {
            parameters: vec![(0.727_272_727_272_727, 0.420_827_318_078_432e-1)],
            residual_sd: 0.369_274_472_937_998,
            r_squared: 0.993_348_115_299_335,
            df_regression: 1,
            df_residual: 2,
            ss_regression: 40.727_272_727_272_7,
            ss_residual: 0.272_727_272_727_273,
            f_statistic: Some(298.666_666_666_666_7),
        },
    }
}

fn longley() -> LinearRegressionDataset {
    #[rustfmt::skip]
    const DATA: [[f64; 7]; 16] = [
        // y        x1      x2        x3      x4      x5        x6
        [60323.0,  83.0, 234_289.0, 2356.0, 1590.0, 107_608.0, 1947.0],
        [61122.0,  88.5, 259_426.0, 2325.0, 1456.0, 108_632.0, 1948.0],
        [60171.0,  88.2, 258_054.0, 3682.0, 1616.0, 109_773.0, 1949.0],
        [61187.0,  89.5, 284_599.0, 3351.0, 1650.0, 110_929.0, 1950.0],
        [63221.0,  96.2, 328_975.0, 2099.0, 3099.0, 112_075.0, 1951.0],
        [63639.0,  98.1, 346_999.0, 1932.0, 3594.0, 113_270.0, 1952.0],
        [64989.0,  99.0, 365_385.0, 1870.0, 3547.0, 115_094.0, 1953.0],
        [63761.0, 100.0, 363_112.0, 3578.0, 3350.0, 116_219.0, 1954.0],
        [66019.0, 101.2, 397_469.0, 2904.0, 3048.0, 117_388.0, 1955.0],
        [67857.0, 104.6, 419_180.0, 2822.0, 2857.0, 118_734.0, 1956.0],
        [68169.0, 108.4, 442_769.0, 2936.0, 2798.0, 120_445.0, 1957.0],
        [66513.0, 110.8, 444_546.0, 4681.0, 2637.0, 121_950.0, 1958.0],
        [68655.0, 112.6, 482_704.0, 3813.0, 2552.0, 123_366.0, 1959.0],
        [69564.0, 114.2, 502_601.0, 3931.0, 2514.0, 125_368.0, 1960.0],
        [69331.0, 115.7, 518_173.0, 4806.0, 2572.0, 127_852.0, 1961.0],
        [70551.0, 116.9, 554_894.0, 4007.0, 2827.0, 130_081.0, 1962.0],
    ];
    let cols: Vec<Vec<f64>> = (1..7).map(|j| DATA.iter().map(|r| r[j]).collect()).collect();
    LinearRegressionDataset {
        name: "Longley",
        difficulty: Difficulty::Higher,
        intercept: true,
        predictors: columns(&cols),
        response: DATA.iter().map(|r| r[0]).collect(),
        certified: CertifiedValues {
            parameters: vec![
                (-3_482_258.634_595_82, 890_420.383_607_373),
                (15.061_872_271_373_3, 84.914_925_774_766_9),
                (-0.358_191_792_925_910e-1, 0.334_910_077_722_432e-1),
                (-2.020_229_803_816_83, 0.488_399_681_651_699),
                (-1.033_226_867_173_59, 0.214_274_163_161_675),
                (-0.511_041_056_535_807e-1, 0.226_073_200_069_370),
                (1_829.151_464_613_55, 455.478_499_142_212),
            ],
            residual_sd: 304.854_073_561_965,
            r_squared: 0.995_479_004_577_296,
            df_regression: 6,
            df_residual: 9,
            ss_regression: 184_172_401.944_494,
            ss_residual: 836_424.055_505_915,
            f_statistic: Some(330.285_339_234_588),
        },
    }
}

/// Wampler1–5: degree-5 polynomials in x = 0, 1, …, 20.
///
/// Wampler1 and Wampler2 are exact polynomials (zero residual). Wampler3–5 add the
/// same noise pattern scaled by 1, 100 and 10 000, making the fit progressively
/// worse while the true coefficients stay at 1.
fn wampler(index: u8) -> LinearRegressionDataset {
    #[rustfmt::skip]
    const Y1: [f64; 21] = [
        1.0, 6.0, 63.0, 364.0, 1365.0, 3906.0, 9331.0, 19608.0, 37449.0, 66430.0, 111_111.0,
        177_156.0, 271_453.0, 402_234.0, 579_195.0, 813_616.0, 1_118_481.0, 1_508_598.0,
        2_000_719.0, 2_613_660.0, 3_368_421.0,
    ];
    #[rustfmt::skip]
    const Y2: [f64; 21] = [
        1.000_00, 1.111_11, 1.249_92, 1.427_53, 1.659_84, 1.968_75, 2.383_36, 2.941_17, 3.689_28,
        4.685_59, 6.000_00, 7.715_61, 9.929_92, 12.756_03, 16.323_84, 20.781_25, 26.295_36,
        33.053_67, 41.265_28, 51.162_09, 63.000_00,
    ];
    #[rustfmt::skip]
    const Y3: [f64; 21] = [
        760.0, -2042.0, 2111.0, -1684.0, 3888.0, 1858.0, 11379.0, 17560.0, 39287.0, 64382.0,
        113_159.0, 175_108.0, 273_291.0, 400_186.0, 581_243.0, 811_568.0, 1_121_004.0,
        1_506_550.0, 2_002_767.0, 2_611_612.0, 3_369_180.0,
    ];
    #[rustfmt::skip]
    const Y4: [f64; 21] = [
        75901.0, -204_794.0, 204_863.0, -204_436.0, 253_665.0, -200_894.0, 214_131.0, -185_192.0,
        221_249.0, -138_370.0, 315_911.0, -27644.0, 455_253.0, 197_434.0, 783_995.0, 608_816.0,
        1_370_781.0, 1_303_798.0, 2_205_519.0, 2_408_860.0, 3_444_321.0,
    ];
    #[rustfmt::skip]
    const Y5: [f64; 21] = [
        7_590_001.0, -20_479_994.0, 20_480_063.0, -20_479_636.0, 25_231_365.0, -20_476_094.0,
        20_489_331.0, -20_460_392.0, 18_417_449.0, -20_413_570.0, 20_591_111.0, -20_302_844.0,
        18_651_453.0, -20_077_766.0, 21_059_195.0, -19_666_384.0, 26_348_481.0, -18_971_402.0,
        22_480_719.0, -17_866_340.0, 10_958_421.0,
    ];
    const SS_REG_1345: f64 = 18_814_317_208_116.7;

    let ones = |se: [f64; 6]| se.iter().map(|&s| (1.0, s)).collect::<Vec<_>>();
    let (name, difficulty, y, certified) = match index {
        1 => (
            "Wampler1",
            Difficulty::Average,
            Y1,
            CertifiedValues {
                parameters: ones([0.0; 6]),
                residual_sd: 0.0,
                r_squared: 1.0,
                df_regression: 5,
                df_residual: 15,
                ss_regression: SS_REG_1345,
                ss_residual: 0.0,
                f_statistic: None,
            },
        ),
        2 => (
            "Wampler2",
            Difficulty::Average,
            Y2,
            CertifiedValues {
                parameters: [1.0, 1e-1, 1e-2, 1e-3, 1e-4, 1e-5].iter().map(|&b| (b, 0.0)).collect(),
                residual_sd: 0.0,
                r_squared: 1.0,
                df_regression: 5,
                df_residual: 15,
                ss_regression: 6_602.918_583_651_67,
                ss_residual: 0.0,
                f_statistic: None,
            },
        ),
        3 => (
            "Wampler3",
            Difficulty::Higher,
            Y3,
            CertifiedValues {
                parameters: ones([
                    2_152.326_246_781_70,
                    2_363.551_734_696_81,
                    779.343_524_331_583,
                    101.475_507_550_350,
                    5.645_665_121_707_52,
                    0.112_324_854_679_312,
                ]),
                residual_sd: 2_360.145_023_792_68,
                r_squared: 0.999_995_559_025_820,
                df_regression: 5,
                df_residual: 15,
                ss_regression: SS_REG_1345,
                ss_residual: 83_554_268.0,
                f_statistic: Some(675_524.458_240_122),
            },
        ),
        4 => (
            "Wampler4",
            Difficulty::Higher,
            Y4,
            CertifiedValues {
                parameters: ones([
                    215_232.624_678_170,
                    236_355.173_469_681,
                    77_934.352_433_158_3,
                    10_147.550_755_035_0,
                    564.566_512_170_752,
                    11.232_485_467_931_2,
                ]),
                residual_sd: 236_014.502_379_268,
                r_squared: 0.957_478_440_825_662,
                df_regression: 5,
                df_residual: 15,
                ss_regression: SS_REG_1345,
                ss_residual: 835_542_680_000.0,
                f_statistic: Some(67.552_445_824_012_2),
            },
        ),
        _ => (
            "Wampler5",
            Difficulty::Higher,
            Y5,
            CertifiedValues {
                parameters: ones([
                    21_523_262.467_817_0,
                    23_635_517.346_968_1,
                    7_793_435.243_315_83,
                    1_014_755.075_503_50,
                    56_456.651_217_075_2,
                    1_123.248_546_793_12,
                ]),
                residual_sd: 23_601_450.237_926_8,
                r_squared: 0.224_668_921_574_940e-2,
                df_regression: 5,
                df_residual: 15,
                ss_regression: SS_REG_1345,
                ss_residual: 0.835_542_680_000_000e16,
                f_statistic: Some(6.755_244_582_401_22e-3),
            },
        ),
    };
    let x: Vec<f64> = (0..=20).map(f64::from).collect();
    LinearRegressionDataset {
        name,
        difficulty,
        intercept: true,
        predictors: polynomial(&x, 5),
        response: y.to_vec(),
        certified,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn by_name(name: &str) -> LinearRegressionDataset {
        linear_regression_datasets().into_iter().find(|d| d.name == name).unwrap()
    }

    /// Transcription checks that do not depend on the regression code.
    #[test]
    fn wampler_responses_have_documented_structure() {
        let poly1 = |x: f64| (0..=5).map(|d| x.powi(d)).sum::<f64>();
        let poly2 = |x: f64| (0..=5).map(|d| x.powi(d) * 10f64.powi(-d)).sum::<f64>();
        let w =
            [by_name("Wampler1"), by_name("Wampler2"), by_name("Wampler3"), by_name("Wampler4"), by_name("Wampler5")];
        for i in 0..21 {
            let x = i as f64;
            assert_eq!(w[0].response[i], poly1(x), "Wampler1 row {i}");
            assert!((w[1].response[i] - poly2(x)).abs() < 5e-12, "Wampler2 row {i}");
            // Wampler3–5 = Wampler1 + noise, noise scaled by 1 : 100 : 10 000.
            let noise3 = w[2].response[i] - w[0].response[i];
            assert_eq!(w[3].response[i] - w[0].response[i], 100.0 * noise3, "Wampler4 row {i}");
            assert_eq!(w[4].response[i] - w[0].response[i], 10_000.0 * noise3, "Wampler5 row {i}");
        }
    }

    #[test]
    fn certified_anova_is_internally_consistent() {
        for d in linear_regression_datasets() {
            let c = &d.certified;
            // Residual SD² = SS_residual / df_residual.
            let s2 = c.ss_residual / c.df_residual as f64;
            assert!((c.residual_sd.powi(2) - s2).abs() <= 1e-12 * s2.max(1e-300), "{}", d.name);
            if let Some(f) = c.f_statistic {
                let f_check = (c.ss_regression / c.df_regression as f64) / s2;
                assert!(((f - f_check) / f).abs() < 1e-12, "{}", d.name);
            }
            let r2 = c.ss_regression / (c.ss_regression + c.ss_residual);
            assert!((r2 - c.r_squared).abs() < 1e-13, "{}", d.name);
        }
    }
}
