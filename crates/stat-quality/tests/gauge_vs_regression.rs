//! Independent verification of the gauge R&R sums of squares.
//!
//! For a balanced design, the two-way ANOVA sums of squares equal the reductions
//! in residual sum of squares from fitting nested dummy-variable regressions:
//!
//! ```text
//! M0: intercept                       RSS₀ = SS_total
//! M1: + part indicators               SS_part  = RSS₀ − RSS₁
//! M2: + operator indicators           SS_oper  = RSS₁ − RSS₂
//! M3: + part×operator indicators      SS_inter = RSS₂ − RSS₃,  SS_error = RSS₃
//! ```
//!
//! The regressions are fitted with `stat_linear` (QR least squares), a completely
//! separate computation from the closed-form sums used by `gauge_rr_anova`.

use stat_core::Matrix;
use stat_linear::{OlsOptions, fit_linear_model};
use stat_quality::{GaugeObservation, GaugeOptions, GaugeSource, gauge_rr_anova};

const PARTS: usize = 6;
const OPERATORS: usize = 3;
const REPLICATES: usize = 3;

/// Deterministic study data: part effects, operator biases, a small interaction
/// and pseudo-random measurement error.
fn study() -> Vec<GaugeObservation> {
    let mut state: u64 = 0x2545_F491_4F6C_DD1D;
    let mut noise = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 11) as f64 / (1u64 << 53) as f64 - 0.5
    };
    let part_effect = [-2.1, -0.8, 0.0, 0.6, 1.4, 2.3];
    let operator_bias = [0.0, 0.25, -0.15];
    let mut out = Vec::new();
    for (p, pe) in part_effect.iter().enumerate() {
        for (o, ob) in operator_bias.iter().enumerate() {
            let interaction = 0.1 * ((p * 3 + o) % 4) as f64 - 0.15;
            for _ in 0..REPLICATES {
                out.push(GaugeObservation {
                    part: p,
                    operator: o,
                    value: 50.0 + pe + ob + interaction + 0.3 * noise(),
                });
            }
        }
    }
    out
}

/// Residual sum of squares after regressing `y` on an intercept plus `columns`.
fn rss(columns: &[Vec<f64>], y: &[f64]) -> f64 {
    let x = if columns.is_empty() { Matrix::zeros(y.len(), 0) } else { Matrix::from_columns(columns).unwrap() };
    fit_linear_model(&x, y, &OlsOptions::default()).unwrap().anova().error.sum_of_squares
}

/// Indicator columns for levels 1..levels (level 0 is the baseline).
fn indicators(levels: usize, of: impl Fn(&GaugeObservation) -> usize, data: &[GaugeObservation]) -> Vec<Vec<f64>> {
    (1..levels).map(|l| data.iter().map(|d| f64::from(u8::from(of(d) == l))).collect()).collect()
}

#[test]
fn sums_of_squares_match_nested_regressions() {
    let data = study();
    let y: Vec<f64> = data.iter().map(|d| d.value).collect();
    let parts = indicators(PARTS, |d| d.part, &data);
    let opers = indicators(OPERATORS, |d| d.operator, &data);
    let mut inter = Vec::new();
    for p in &parts {
        for o in &opers {
            inter.push(p.iter().zip(o).map(|(a, b)| a * b).collect::<Vec<f64>>());
        }
    }
    let rss0 = rss(&[], &y);
    let m1: Vec<Vec<f64>> = parts.clone();
    let rss1 = rss(&m1, &y);
    let m2: Vec<Vec<f64>> = parts.iter().chain(&opers).cloned().collect();
    let rss2 = rss(&m2, &y);
    let m3: Vec<Vec<f64>> = parts.iter().chain(&opers).chain(&inter).cloned().collect();
    let rss3 = rss(&m3, &y);

    let s = gauge_rr_anova(&data, &GaugeOptions::default()).unwrap();
    let ss = |src| s.anova.iter().find(|r| r.source == src).unwrap().sum_of_squares;
    let check = |name: &str, got: f64, want: f64| {
        let rel = ((got - want) / want).abs();
        assert!(rel < 1e-10, "{name}: closed form {got} vs regression {want} (relative error {rel:e})");
    };
    check("total", ss(GaugeSource::Total), rss0);
    check("part", ss(GaugeSource::Part), rss0 - rss1);
    check("operator", ss(GaugeSource::Operator), rss1 - rss2);
    check("interaction", ss(GaugeSource::Interaction), rss2 - rss3);
    check("repeatability", ss(GaugeSource::Repeatability), rss3);

    // Degrees of freedom and the variance-component bookkeeping.
    let df: Vec<usize> = s.anova.iter().map(|r| r.df).collect();
    assert_eq!(
        df,
        vec![
            PARTS - 1,
            OPERATORS - 1,
            (PARTS - 1) * (OPERATORS - 1),
            PARTS * OPERATORS * (REPLICATES - 1),
            PARTS * OPERATORS * REPLICATES - 1
        ]
    );
    let c = s.components;
    let sum = c.repeatability.variance + c.operator.variance + c.interaction.variance + c.part.variance;
    assert!((c.total.variance - sum).abs() < 1e-12);
    let pct: f64 = [c.gauge_rr, c.part].iter().map(|v| v.percent_contribution).sum();
    assert!((pct - 100.0).abs() < 1e-10, "%contribution of gauge + part is 100");
    // Strong part effects relative to error: the gauge distinguishes several categories.
    assert!(s.distinct_categories.unwrap() >= 5, "ndc = {:?}", s.distinct_categories);
}
