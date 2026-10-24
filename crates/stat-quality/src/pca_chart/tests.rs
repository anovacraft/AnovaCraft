use super::*;

fn data() -> Vec<Vec<f64>> {
    let mut rng = Rng::new(11);
    (0..30)
        .map(|_| {
            let f = rng.normal();
            vec![f + 0.1 * rng.normal(), 2.0 * f + 0.1 * rng.normal(), -f + 0.1 * rng.normal(), rng.normal()]
        })
        .collect()
}

#[test]
fn all_components_leave_no_residual() {
    let x = data();
    let c =
        pca_chart(&x, None, &PcaChartOptions { components: ComponentChoice::Fixed(4), ..Default::default() }).unwrap();
    assert!(c.spe.iter().all(|p| p.statistic.abs() < 1e-20 && !p.signal));
    assert!(c.spe_limit.is_nan());
    assert!((c.explained() - 1.0).abs() < 1e-12);
    assert!(c.t2_monitoring_limit.is_nan(), "every point is in the reference");
}

#[test]
fn limits_methods_and_errors() {
    // Jackson–Mudholkar and the scaled χ² agree closely for a single
    // residual eigenvalue: both are then λ·χ²₁ exactly or nearly.
    let one = [2.0];
    let jm = spe_limit(&one, 0.01, SpeLimitMethod::JacksonMudholkar).unwrap();
    let chi = spe_limit(&one, 0.01, SpeLimitMethod::ScaledChiSquare).unwrap();
    assert!((chi - 2.0 * 6.634_896_601_021_214).abs() < 1e-9, "{chi}");
    assert!((jm / chi - 1.0).abs() < 0.05, "{jm} vs {chi}");
    assert!(spe_limit(&[0.0, 0.0], 0.01, SpeLimitMethod::JacksonMudholkar).is_none());
    for k in SpeLimitMethod::ALL {
        assert_eq!(SpeLimitMethod::from_key(k.key()), Some(k));
    }
    let x = data();
    let fixed = |a| PcaChartOptions { components: ComponentChoice::Fixed(a), ..Default::default() };
    assert!(pca_chart(&x, None, &fixed(0)).is_err());
    assert!(pca_chart(&x, None, &fixed(5)).is_err());
    assert!(pca_chart(&x, Some(2), &fixed(1)).is_err());
    assert!(pca_chart(&x, None, &PcaChartOptions { alpha: 1.0, ..fixed(1) }).is_err());
    assert!(pca_chart(&[vec![1.0], vec![2.0], vec![3.0]], None, &fixed(1)).is_err());
    assert!(pca_cross_validation(&x, true, 1, 3).is_err());
    let cv = pca_chart(&x, None, &PcaChartOptions::default()).unwrap();
    assert_eq!(cv.components, 1, "one latent factor drives three of the four variables: {:?}", cv.press);
}

#[test]
fn change_point_finds_a_shift() {
    let mut rng = Rng::new(5);
    let y: Vec<Vec<f64>> =
        (0..40).map(|i| vec![rng.normal() + if i >= 25 { 3.0 } else { 0.0 }, rng.normal()]).collect();
    let cp = mean_change_point(&y, 199, 1).unwrap();
    assert_eq!(cp.before, 25);
    assert!(cp.p_value < 0.01, "{}", cp.p_value);
    assert_eq!(cp.statistics.len(), 39);
    assert!(mean_change_point(&y[..2], 9, 1).is_err());
}
