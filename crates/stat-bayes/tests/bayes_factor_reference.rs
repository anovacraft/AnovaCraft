//! Bayes factors against R's BayesFactor package (0.9.12), from
//! `tests/data/bayes_factor_reference.R`. BayesFactor computes these by
//! numerical integration, so agreement is to about 1e-6 in ln BF.

use stat_bayes::bayes_factor::{Direction, Sampling, TDesign, correlation, ln_contingency, t_test};

const X: [f64; 12] = [0.42, 1.31, -0.25, 0.88, 1.92, 0.63, -0.47, 1.18, 0.95, 0.21, 1.57, 0.74];
const Y: [f64; 10] = [-0.31, 0.52, 0.11, -0.84, 0.36, -0.15, 0.67, -0.52, 0.08, 0.29];
const MEDIUM: f64 = std::f64::consts::FRAC_1_SQRT_2;

fn mean_var(x: &[f64]) -> (f64, f64, f64) {
    let n = x.len() as f64;
    let m = x.iter().sum::<f64>() / n;
    (n, m, x.iter().map(|v| (v - m) * (v - m)).sum::<f64>() / (n - 1.0))
}

fn one_t(x: &[f64]) -> (f64, f64) {
    let (n, m, v) = mean_var(x);
    (m / (v / n).sqrt(), n)
}

fn close(label: &str, got: f64, want: f64, tol: f64) {
    assert!((got - want).abs() < tol, "{label}: {got} vs BayesFactor {want}");
}

#[test]
fn t_tests() {
    let (t, n) = one_t(&X);
    let d = TDesign::OneSample { n };
    let bf = |r, dir| t_test(t, d, r, dir, 0.95).unwrap().ln_bf10;
    close("one-sample medium", bf(MEDIUM, Direction::TwoSided), 2.655_864_021_758_46, 1e-6);
    close("one-sample wide", bf(1.0, Direction::TwoSided), 2.672_884_363_094_21, 1e-6);
    close("one-sample greater", bf(MEDIUM, Direction::Greater), 3.345_818_178_274_63, 1e-4);
    close("one-sample less", bf(MEDIUM, Direction::Less), -2.399_371_718_457_18, 1e-3);
    let (n1, m1, v1) = mean_var(&X);
    let (n2, m2, v2) = mean_var(&Y);
    let sp = (((n1 - 1.0) * v1 + (n2 - 1.0) * v2) / (n1 + n2 - 2.0) * (1.0 / n1 + 1.0 / n2)).sqrt();
    let t2 = (m1 - m2) / sp;
    let d2 = TDesign::TwoSample { n1, n2 };
    close(
        "two-sample medium",
        t_test(t2, d2, MEDIUM, Direction::TwoSided, 0.95).unwrap().ln_bf10,
        1.570_987_327_203_89,
        1e-6,
    );
    close(
        "two-sample r = 1",
        t_test(t2, d2, 1.0, Direction::TwoSided, 0.95).unwrap().ln_bf10,
        1.590_397_934_329_59,
        1e-6,
    );
    let (t0, n0) = one_t(&[-1.0, 0.0, 1.0, 0.1, -0.12, 0.05]);
    close(
        "null-ish",
        t_test(t0, TDesign::OneSample { n: n0 }, MEDIUM, Direction::TwoSided, 0.95).unwrap().ln_bf10,
        -0.985_576_624_211_977,
        1e-6,
    );
    close(
        "large n",
        t_test(2.2, TDesign::OneSample { n: 400.0 }, MEDIUM, Direction::TwoSided, 0.95).unwrap().ln_bf10,
        -0.491_827_483_568_708,
        1e-6,
    );
    close(
        "from t",
        t_test(-1.3, TDesign::TwoSample { n1: 25.0, n2: 31.0 }, MEDIUM, Direction::TwoSided, 0.95).unwrap().ln_bf10,
        -0.605_704_563_359_782,
        1e-6,
    );
}

#[test]
fn posterior_of_effect_size() {
    // BayesFactor's posterior() by MCMC with 200,000 iterations: mean
    // 0.92799, SD 0.36405, 2.5% 0.23959, 97.5% 1.66219 (Monte Carlo error
    // about 0.002 on the mean and 0.005 on the quantiles).
    let (t, n) = one_t(&X);
    let r = t_test(t, TDesign::OneSample { n }, MEDIUM, Direction::TwoSided, 0.95).unwrap();
    close("δ mean", r.summary.mean, 0.927_986_740_232_025, 0.004);
    close("δ sd", r.summary.sd, 0.364_048_323_481_689, 0.004);
    close("δ 2.5%", r.summary.equal_tailed.0, 0.239_593_660_334_01, 0.01);
    close("δ 97.5%", r.summary.equal_tailed.1, 1.662_191_055_837_27, 0.01);
}

#[test]
fn correlations() {
    let u = [1.2, 2.3, 2.9, 4.1, 5.5, 5.9, 7.2, 8.1, 8.8, 10.4, 11.0, 12.3, 13.1, 14.2];
    let v = [2.0, 1.4, 3.8, 3.1, 4.9, 6.6, 5.2, 7.9, 7.1, 8.3, 10.9, 9.4, 12.2, 11.1];
    let r = 0.959_649_304_066_288;
    let bf = |k, d| correlation(r, u.len() as f64, k, d, 0.95).unwrap().ln_bf10;
    close("r medium", bf(1.0 / 3.0, Direction::TwoSided), 8.806_810_293_482_78, 1e-6);
    close("r ultrawide", bf(1.0, Direction::TwoSided), 11.911_267_519_99, 1e-6);
    // One-sided: exactly 2·BF₁₀·P(ρ > 0 │ y). BayesFactor reports 9.491328,
    // 0.9% below what its own two-sided BF implies (its BF₊₀ and BF₋₀ average
    // to e^8.798, not its two-sided e^8.807), so it is checked to 0.01.
    let two = correlation(r, u.len() as f64, 1.0 / 3.0, Direction::TwoSided, 0.95).unwrap();
    let plus = bf(1.0 / 3.0, Direction::Greater);
    close(
        "r greater (identity)",
        plus,
        two.ln_bf10 + (2.0 * stat_bayes::Univariate::sf(&two.posterior, 0.0)).ln(),
        1e-4,
    );
    close("r greater", plus, 9.491_327_821_576_06, 0.01);
    let _ = v;
    close(
        "r null",
        correlation(-0.437_381_938_984_546, 10.0, 1.0 / 3.0, Direction::TwoSided, 0.95).unwrap().ln_bf10,
        0.044_797_090_943_943_4,
        1e-6,
    );
}

#[test]
fn contingency_tables() {
    let t = vec![vec![18.0, 7.0, 11.0], vec![9.0, 14.0, 21.0]];
    let bf = |s, a| ln_contingency(&t, s, a).unwrap();
    close("joint", bf(Sampling::JointMultinomial, 1.0), 2.133_108_642_629_17, 1e-9);
    close("rows", bf(Sampling::FixedRows, 1.0), 1.547_131_212_731_45, 1e-9);
    close("cols", bf(Sampling::FixedColumns, 1.0), 1.411_318_218_537_17, 1e-9);
    close("poisson", bf(Sampling::Poisson, 1.0), 2.685_668_770_357_89, 1e-9);
    close("joint a=2", bf(Sampling::JointMultinomial, 2.0), 1.750_597_200_546_96, 1e-9);
    close("rows a=2", bf(Sampling::FixedRows, 2.0), 1.576_084_794_067_23, 1e-9);
    close("poisson a=2", bf(Sampling::Poisson, 2.0), 1.987_022_157_079_68, 1e-9);
    let t3 = vec![vec![10.0, 4.0, 6.0], vec![3.0, 9.0, 8.0], vec![5.0, 7.0, 12.0]];
    let bf3 = |s| ln_contingency(&t3, s, 1.0).unwrap();
    close("3×3 joint", bf3(Sampling::JointMultinomial), 1.215_451_286_507_37, 1e-9);
    close("3×3 cols", bf3(Sampling::FixedColumns), 0.125_807_653_291_27, 1e-9);
    close("3×3 poisson", bf3(Sampling::Poisson), 2.437_910_501_565_37, 1e-9);
}
