use super::*;
use crate::charts::{individuals_chart, mean_and_range_chart, mean_and_sd_chart};
use crate::constants::ChartConstants;

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-12 * a.abs().max(b.abs()).max(1.0)
}

fn same_limits(a: &ControlLimits, b: &ControlLimits) -> bool {
    close(a.center, b.center) && close(a.lower, b.lower) && close(a.upper, b.upper)
}

fn groups() -> Vec<Vec<f64>> {
    // 12 subgroups of 4 with a drift at the end, so some rules fire.
    (0..12)
        .map(|i| {
            let shift = if i >= 9 { 3.0 } else { 0.0 };
            (0..4).map(|j| 10.0 + shift + f64::from((i * 7 + j * 3) % 5) * 0.5).collect()
        })
        .collect()
}

#[test]
fn equal_sizes_reproduce_the_classic_charts() {
    let g = groups();
    let single = vec![0; g.len()];
    for (kind, classic) in [
        (ChartKind::MeanAndRange, mean_and_range_chart(&g).unwrap()),
        (ChartKind::MeanAndStdDev, mean_and_sd_chart(&g).unwrap()),
    ] {
        let p = phased_chart(kind, &g, &single).unwrap();
        assert_eq!(p.phases.len(), 1);
        assert!(close(p.phases[0].sigma, classic.sigma));
        assert_eq!(p.phases[0].sizes, (4, 4));
        for (a, b) in [(&p.location, &classic.location), (&p.dispersion, &classic.dispersion)] {
            assert_eq!(a.values.len(), b.values.len());
            for (x, y) in a.values.iter().zip(&b.values) {
                assert!(close(x.unwrap(), y.unwrap()));
            }
            assert!(a.limits.iter().all(|l| same_limits(l, &b.limits)), "{kind:?} {:?}", a.statistic);
            assert_eq!(a.violations(&RunRule::ALL, &p.phases), b.violations(&RunRule::ALL), "{kind:?}");
        }
    }
    // Individuals.
    let x: Vec<f64> = vec![49.6, 47.6, 49.9, 51.3, 47.8, 51.2, 52.6, 52.4, 53.6, 52.1, 58.0];
    let classic = individuals_chart(&x).unwrap();
    let ones: Vec<Vec<f64>> = x.iter().map(|&v| vec![v]).collect();
    let p = phased_chart(ChartKind::IndividualsAndMovingRange, &ones, &vec![0; x.len()]).unwrap();
    assert!(close(p.phases[0].sigma, classic.sigma));
    assert!(p.location.limits.iter().all(|l| same_limits(l, &classic.location.limits)));
    assert!(p.dispersion.limits.iter().all(|l| same_limits(l, &classic.dispersion.limits)));
    assert_eq!(p.dispersion.values[0], None);
    assert_eq!(p.location.violations(&RunRule::ALL, &p.phases), classic.location.violations(&RunRule::ALL));
    assert!(!classic.location.violations(&RunRule::ALL).is_empty(), "the last value is beyond the limits");
}

#[test]
fn varying_sizes_by_hand() {
    // Sizes 2, 3, 4: means 2, 5, 10; ranges 2, 2, 6.
    let g = vec![vec![1.0, 3.0], vec![4.0, 5.0, 6.0], vec![7.0, 9.0, 11.0, 13.0]];
    let p = phased_chart(ChartKind::MeanAndRange, &g, &[0, 0, 0]).unwrap();
    // x̿ = (2·2 + 3·5 + 4·10)/9 = 59/9.
    let grand = 59.0 / 9.0;
    let sigma = (2.0 / d2(2).unwrap() + 2.0 / d2(3).unwrap() + 6.0 / d2(4).unwrap()) / 3.0;
    assert!(close(p.phases[0].sigma, sigma));
    assert_eq!(p.phases[0].sizes, (2, 4));
    for (i, n) in [2.0f64, 3.0, 4.0].iter().enumerate() {
        let l = &p.location.limits[i];
        assert!(close(l.center, grand));
        assert!(close(l.upper - l.center, 3.0 * sigma / n.sqrt()));
    }
    // Range chart for n = 3: center d₂σ̂, upper D₄-equivalent.
    let k = ChartConstants::for_subgroup_size(3).unwrap();
    let r = &p.dispersion.limits[1];
    assert!(close(r.center, k.d2 * sigma));
    assert!(close(r.upper, k.range_upper * k.d2 * sigma));
    assert_eq!(r.lower, 0.0);
    // S chart with sizes 2 and 3: σ̂ = mean of sᵢ/c₄(nᵢ).
    let s = phased_chart(ChartKind::MeanAndStdDev, &g[..2], &[0, 0]).unwrap();
    let s1 = 2.0f64.sqrt();
    let want = (s1 / c4(2).unwrap() + 1.0 / c4(3).unwrap()) / 2.0;
    assert!(close(s.phases[0].sigma, want));
    let k3 = ChartConstants::for_subgroup_size(3).unwrap();
    assert!(close(s.dispersion.limits[1].upper, k3.sd_upper * k3.c4 * want));
}

#[test]
fn phases_get_their_own_limits_and_rules_restart() {
    // Phase 0 around 10, phase 1 around 20 with one wild value.
    let a = [10.0, 10.5, 9.8, 10.2, 9.9, 10.4, 10.1, 9.7];
    let mut b = vec![20.0, 20.4, 19.8, 20.3, 19.9, 20.2, 20.1, 19.6];
    b[5] = 26.0;
    let values: Vec<f64> = a.iter().chain(&b).copied().collect();
    let ones: Vec<Vec<f64>> = values.iter().map(|&v| vec![v]).collect();
    let phases: Vec<usize> = (0..16).map(|i| usize::from(i >= 8)).collect();
    let p = phased_chart(ChartKind::IndividualsAndMovingRange, &ones, &phases).unwrap();
    assert_eq!(p.phases.len(), 2);
    assert_eq!((p.phases[1].start, p.phases[1].end), (8, 16));
    let first = individuals_chart(&a).unwrap();
    let second = individuals_chart(&b).unwrap();
    assert!(same_limits(&p.location.limits[0], &first.location.limits));
    assert!(same_limits(&p.location.limits[8], &second.location.limits));
    // The jump between phases is not a moving range.
    assert_eq!(p.dispersion.values[8], None);
    // Violations in phase 1 are those of its own chart, shifted by 8.
    let want: Vec<usize> = second.location.violations(&RunRule::ALL).iter().map(|v| v.index + 8).collect();
    let got: Vec<usize> = p.location.violations(&RunRule::ALL, &p.phases).iter().map(|v| v.index).collect();
    let a_violations: Vec<usize> = first.location.violations(&RunRule::ALL).iter().map(|v| v.index).collect();
    assert_eq!(got, [a_violations, want].concat());
    // As one phase the step from 10 to 20 would flag nearly every point.
    let one = phased_chart(ChartKind::IndividualsAndMovingRange, &ones, &[0; 16]).unwrap();
    assert!(one.location.violations(&RunRule::ALL, &one.phases).len() > got.len());
}

#[test]
fn varying_limits_rules_use_standardized_values() {
    // Sizes alternate 2 and 8, so the limits alternate in width; a mean just
    // inside the wide limits but outside the narrow ones is flagged only there.
    let mut g: Vec<Vec<f64>> = (0..10)
        .map(|i| if i % 2 == 0 { vec![9.0, 11.0] } else { vec![9.0, 11.0, 9.5, 10.5, 9.0, 11.0, 10.0, 10.0] })
        .collect();
    let p = phased_chart(ChartKind::MeanAndStdDev, &g, &[0; 10]).unwrap();
    let wide = p.location.limits[0];
    let narrow = p.location.limits[1];
    assert!(wide.upper > narrow.upper);
    let between = 0.5 * (wide.upper + narrow.upper) + 0.0;
    // Shift subgroups 4 (n = 2) and 5 (n = 8) so their means sit between the two upper limits.
    for i in [4, 5] {
        let m = g[i].iter().sum::<f64>() / g[i].len() as f64;
        g[i].iter_mut().for_each(|v| *v += between - m);
    }
    let q = phased_chart(ChartKind::MeanAndStdDev, &g, &[0; 10]).unwrap();
    let beyond: Vec<usize> =
        q.location.violations(&[RunRule::BeyondLimits], &q.phases).iter().map(|v| v.index).collect();
    // The limits moved a little (the grand mean rose), so recheck against them.
    let expect: Vec<usize> = (0..10).filter(|&i| q.location.values[i].unwrap() > q.location.limits[i].upper).collect();
    assert_eq!(beyond, expect);
    assert!(expect.contains(&5) && !expect.contains(&4), "{expect:?}");
}

#[test]
fn errors() {
    assert!(phased_chart(ChartKind::MeanAndRange, &[vec![1.0, 2.0], vec![2.0, 3.0]], &[0]).is_err());
    let e = phased_chart(ChartKind::IndividualsAndMovingRange, &[vec![1.0, 2.0], vec![3.0]], &[0, 0]).unwrap_err();
    assert!(e.to_string().contains("choose an X̄ chart"), "{e}");
    assert!(phased_chart(ChartKind::MeanAndRange, &[vec![1.0], vec![2.0, 3.0]], &[0, 0]).is_err());
    assert!(phased_chart(ChartKind::MeanAndRange, &[vec![1.0; 26], vec![2.0; 26]], &[0, 0]).is_err());
    assert!(phased_chart(ChartKind::MeanAndStdDev, &[vec![1.0; 26], vec![2.0, 3.0]], &[0, 0]).is_ok());
    let lone = phased_chart(ChartKind::IndividualsAndMovingRange, &[vec![1.0], vec![2.0], vec![3.0]], &[0, 0, 1]);
    assert!(lone.unwrap_err().to_string().contains("only one point"));
    assert!(phased_chart(ChartKind::IndividualsAndMovingRange, &[vec![1.0], vec![1.0]], &[0, 0]).is_err());
    assert!(phased_chart(ChartKind::MeanAndRange, &[vec![1.0, f64::NAN], vec![2.0, 3.0]], &[0, 0]).is_err());
}

fn near(a: f64, b: f64, rel: f64) {
    assert!((a - b).abs() <= rel * b.abs().max(1.0), "{a} vs {b}");
}

/// Hand-computed with the tabled constants d₂(3) = 1.6926, d₃(3) = 0.8884,
/// d₂(4) = 2.059, d₃(4) = 0.8794, c₄(5) = 0.9400 (Montgomery, Appendix VI).
#[test]
fn sigma_multiple_range_span_and_standard_values() {
    // Individuals, moving ranges of 3, limits at 2σ.
    let x = [1.0, 3.0, 2.0, 6.0, 4.0];
    let singles: Vec<Vec<f64>> = x.iter().map(|&v| vec![v]).collect();
    let settings = ChartSettings { sigma_multiple: 2.0, range_span: 3, ..ChartSettings::default() };
    let c = phased_chart_with(ChartKind::IndividualsAndMovingRange, &singles, &[0; 5], &settings).unwrap();
    let mr_bar = (2.0 + 4.0 + 4.0) / 3.0;
    let sigma = mr_bar / 1.6926;
    near(c.phases[0].sigma, sigma, 1e-3);
    near(c.location.limits[0].upper, 3.2 + 2.0 * sigma, 1e-3);
    near(c.location.limits[0].lower, 3.2 - 2.0 * sigma, 1e-3);
    assert_eq!(c.dispersion.values[..3], [None, None, Some(2.0)]);
    near(c.dispersion.limits[0].center, mr_bar, 1e-12);
    near(c.dispersion.limits[0].upper, mr_bar * (1.0 + 2.0 * 0.8884 / 1.6926), 1e-3);
    assert_eq!(c.dispersion.limits[0].lower, 0.0, "1 − 2d₃/d₂ < 0 is clipped at zero");
    assert_eq!(c.location.sigma_multiple, 2.0);

    // X̄–R with standard values μ₀ = 10, σ₀ = 2 and n = 4: X̄ limits 10 ± 3,
    // R center d₂σ₀ and upper limit (d₂ + 3d₃)σ₀ = D₂σ₀ (D₂(4) = 4.698).
    let g = vec![vec![9.0, 10.0, 11.0, 12.0], vec![8.0, 9.5, 10.5, 10.0], vec![10.0, 10.0, 11.0, 9.0]];
    let standard = Some(StandardValues { center: 10.0, sigma: 2.0 });
    let settings = ChartSettings { standard, ..ChartSettings::default() };
    let c = phased_chart_with(ChartKind::MeanAndRange, &g, &[0; 3], &settings).unwrap();
    near(c.location.limits[0].center, 10.0, 1e-12);
    near(c.location.limits[0].upper, 13.0, 1e-12);
    near(c.location.limits[0].lower, 7.0, 1e-12);
    near(c.dispersion.limits[0].center, 2.059 * 2.0, 1e-3);
    near(c.dispersion.limits[0].upper, 4.698 * 2.0, 1e-3);
    assert_eq!(c.dispersion.limits[0].lower, 0.0, "D₁(4) = 0");
    assert_eq!(c.phases[0].sigma, 2.0);

    // X̄–S, n = 5, standard values at 2σ: S center c₄σ₀, limits (c₄ ± 2√(1 − c₄²))σ₀.
    let g: Vec<Vec<f64>> = (0..4).map(|i| (0..5).map(|j| f64::from(i + j)).collect()).collect();
    let standard = Some(StandardValues { center: 4.0, sigma: 1.5 });
    let settings = ChartSettings { standard, sigma_multiple: 2.0, ..ChartSettings::default() };
    let c = phased_chart_with(ChartKind::MeanAndStdDev, &g, &[0; 4], &settings).unwrap();
    let c4: f64 = 0.9400;
    let w = (1.0 - c4 * c4).sqrt();
    near(c.dispersion.limits[0].center, c4 * 1.5, 1e-3);
    near(c.dispersion.limits[0].upper, (c4 + 2.0 * w) * 1.5, 1e-3);
    near(c.dispersion.limits[0].lower, (c4 - 2.0 * w) * 1.5, 1e-3);
    near(c.location.limits[0].upper, 4.0 + 2.0 * 1.5 / 5f64.sqrt(), 1e-12);

    // Standard values chart data with no spread; estimates would refuse it.
    let flat = vec![vec![5.0, 5.0], vec![5.0, 5.0]];
    let settings =
        ChartSettings { standard: Some(StandardValues { center: 5.0, sigma: 1.0 }), ..ChartSettings::default() };
    assert!(phased_chart_with(ChartKind::MeanAndRange, &flat, &[0; 2], &settings).is_ok());
    assert!(phased_chart_with(ChartKind::MeanAndRange, &flat, &[0; 2], &ChartSettings::default()).is_err());

    // Defaults reproduce the plain chart exactly.
    let explicit = ChartSettings { sigma_multiple: 3.0, range_span: 2, ..ChartSettings::default() };
    assert_eq!(
        phased_chart_with(ChartKind::IndividualsAndMovingRange, &singles, &[0; 5], &explicit).unwrap(),
        phased_chart(ChartKind::IndividualsAndMovingRange, &singles, &[0; 5]).unwrap()
    );

    // Only individuals, X̄–R and X̄–S charts take these settings; bad values are refused.
    let k2 = ChartSettings { sigma_multiple: 2.0, ..ChartSettings::default() };
    assert!(phased_chart_with(ChartKind::Zone, &singles, &[0; 5], &k2).is_err());
    for bad in [
        ChartSettings { sigma_multiple: 0.0, ..ChartSettings::default() },
        ChartSettings { range_span: 26, ..ChartSettings::default() },
        ChartSettings { standard: Some(StandardValues { center: 0.0, sigma: 0.0 }), ..ChartSettings::default() },
    ] {
        assert!(phased_chart_with(ChartKind::IndividualsAndMovingRange, &singles, &[0; 5], &bad).is_err());
    }
}

/// With limits at 2σ the 1σ zone is half the limit distance, not a third.
#[test]
fn rules_follow_the_sigma_multiple() {
    let limits = vec![ControlLimits { center: 0.0, lower: -2.0, upper: 2.0 }; 5];
    let values = vec![Some(0.8); 5];
    let rule = [RunRule::FourOfFiveBeyondOneSigma];
    // Read as 3σ limits, 1σ is 0.667 and 0.8 is beyond it; at 2σ, 1σ is 1.
    assert!(!varying_violations(&values, &limits, &rule, &[(0, 5)]).is_empty());
    assert!(varying_violations_at(&values, &limits, &rule, &[(0, 5)], 2.0).is_empty());
    let mut beyond = values.clone();
    beyond[4] = Some(2.5);
    let v = varying_violations_at(&beyond, &limits, &[RunRule::BeyondLimits], &[(0, 5)], 2.0);
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].index, 4);
}
