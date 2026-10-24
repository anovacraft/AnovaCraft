//! PSIS-LOO and WAIC against R's loo package (2.6.0), from
//! `tests/data/loo_reference.R`: the same deterministic log-likelihood
//! matrix built on both sides.

use stat_bayes::loo::loo;

#[test]
fn psis_loo_and_waic() {
    let y = [-0.8, 0.1, 0.5, 1.2, -0.3, 0.9, 0.0, 0.4, -1.1, 0.7, 2.0, -0.2, 0.3, 6.5, 0.6];
    let ll: Vec<Vec<f64>> = (1..=400)
        .map(|s| {
            let s = f64::from(s);
            let mu = 0.2 + 0.4 * (s * 1.7).sin();
            let sigma = 1.0 + 0.3 * (s * 0.37).cos().powi(2);
            y.iter()
                .map(|v| -0.5 * (2.0 * std::f64::consts::PI).ln() - sigma.ln() - 0.5 * ((v - mu) / sigma).powi(2))
                .collect()
        })
        .collect();
    let r = loo(&ll, None);
    let close = |label: &str, got: f64, want: f64| {
        assert!((got - want).abs() < 1e-9 * (1.0 + want.abs()), "{label}: {got} vs loo {want}");
    };
    close("elpd_loo", r.elpd_loo, -38.911_454_182_050_2);
    close("se", r.se_elpd_loo, 18.894_587_435_937_95);
    close("p_loo", r.p_loo, 6.891_226_754_493_4);
    close("looic", r.looic, 77.822_908_364_100_4);
    close("elpd_waic", r.elpd_waic, -42.211_481_207_588_6);
    close("p_waic", r.p_waic, 10.191_253_780_031_8);
    close("se waic", r.se_elpd_waic, 22.184_949_555_249_9);
    let k = [
        0.259_371_946_319_841,
        -0.644_149_253_859_345,
        -0.515_470_145_036_693,
        -0.379_098_460_829_75,
        -0.860_063_557_367_398,
        -0.623_273_934_155_188,
        -0.883_141_810_825_884,
        -0.516_365_507_923_015,
        -0.409_815_178_711_39,
        -0.455_505_528_644_932,
        -0.598_599_028_595_26,
        -0.947_518_948_512_929,
        -0.021_385_599_492_027_2,
        0.254_360_754_799_08,
        -0.590_663_108_926_07,
    ];
    for (i, (&got, &want)) in r.pareto_k.iter().zip(&k).enumerate() {
        close(&format!("k[{i}]"), got, want);
    }
    close("pointwise[13]", r.pointwise[13], -20.180_039_025_237_7);
}
