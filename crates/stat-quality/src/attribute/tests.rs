use super::*;

fn close(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-12, "{a} vs {b}");
}

fn near(a: f64, b: f64, rel: f64) {
    assert!((a - b).abs() <= rel * b.abs().max(1e-300), "{a} vs {b}");
}

fn signals(c: &AttributeChart) -> Vec<usize> {
    c.points.iter().enumerate().filter(|(_, p)| p.signal).map(|(i, _)| i + 1).collect()
}

/// Montgomery Example 7.1: 30 samples of 50 cans; published p̄ = 0.2313,
/// UCL = 0.4102, LCL = 0.0524, samples 15 and 23 out of control.
#[test]
fn p_and_np_charts() {
    let d = [
        12.0, 15.0, 8.0, 10.0, 4.0, 7.0, 16.0, 9.0, 14.0, 10.0, 5.0, 6.0, 17.0, 12.0, 22.0, 8.0, 10.0, 5.0, 13.0, 11.0,
        20.0, 18.0, 24.0, 15.0, 9.0, 12.0, 7.0, 13.0, 9.0, 6.0,
    ];
    let n = [50.0; 30];
    let p = attribute_chart(AttributeChartKind::P, &d, Some(&n)).unwrap();
    close(p.center, 347.0 / 1500.0);
    close(p.points[0].upper, 0.410_239_118_594_738_44);
    close(p.points[0].lower, 0.052_427_548_071_928_23);
    assert_eq!(signals(&p), [15, 23]);
    let np = attribute_chart(AttributeChartKind::Np, &d, Some(&n)).unwrap();
    close(np.center, 50.0 * 347.0 / 1500.0);
    close(np.points[0].upper, 50.0 * 0.410_239_118_594_738_44);
    assert_eq!(signals(&np), [15, 23]);
    // Unequal sizes: limits vary; np refuses them.
    let mut sizes = n;
    sizes[0] = 200.0;
    let v = attribute_chart(AttributeChartKind::P, &d, Some(&sizes)).unwrap();
    assert!(v.points[0].upper < v.points[1].upper);
    assert!(attribute_chart(AttributeChartKind::Np, &d, Some(&sizes)).is_err());
}

/// Montgomery Example 7.3: nonconformities in 26 samples of 100 boards;
/// published c̄ = 19.85, UCL = 33.22, LCL = 6.48, samples 6 and 20 out.
#[test]
fn c_chart() {
    let c = [
        21.0, 24.0, 16.0, 12.0, 15.0, 5.0, 28.0, 20.0, 31.0, 25.0, 20.0, 24.0, 16.0, 19.0, 10.0, 17.0, 13.0, 22.0,
        18.0, 39.0, 30.0, 24.0, 16.0, 19.0, 17.0, 15.0,
    ];
    let chart = attribute_chart(AttributeChartKind::C, &c, None).unwrap();
    close(chart.center, 516.0 / 26.0);
    close(chart.points[0].upper, 33.210_860_525_141_78);
    close(chart.points[0].lower, 6.481_447_167_165_914);
    assert_eq!(signals(&chart), [6, 20]);
}

/// Montgomery Example 7.5: 20 samples of 5 computers; published ū = 1.93,
/// UCL = 3.79, LCL = 0.07, in control.
#[test]
fn u_chart_and_invalid_input() {
    let c =
        [10.0, 12.0, 8.0, 14.0, 10.0, 16.0, 11.0, 7.0, 10.0, 15.0, 9.0, 5.0, 7.0, 11.0, 12.0, 6.0, 8.0, 10.0, 7.0, 5.0];
    let u = attribute_chart(AttributeChartKind::U, &c, Some(&[5.0; 20])).unwrap();
    close(u.center, 1.93);
    close(u.points[0].upper, 3.793_866_948_041_088);
    close(u.points[0].lower, 0.066_133_051_958_911_84);
    assert!(signals(&u).is_empty());
    assert!(attribute_chart(AttributeChartKind::U, &c, None).is_err(), "sizes needed");
    assert!(attribute_chart(AttributeChartKind::C, &[], None).is_err());
    assert!(attribute_chart(AttributeChartKind::C, &[1.5, 2.0], None).is_err());
    assert!(attribute_chart(AttributeChartKind::C, &[0.0, 0.0], None).is_err(), "zero centre line");
    assert!(attribute_chart(AttributeChartKind::P, &[6.0], Some(&[5.0])).is_err(), "more defectives than units");
    assert!(attribute_chart(AttributeChartKind::P, &[5.0, 5.0], Some(&[5.0, 5.0])).is_err(), "all nonconforming");
    assert!(attribute_chart(AttributeChartKind::P, &[1.0, 2.0], Some(&[5.0])).is_err(), "length mismatch");
    // Lower limits are floored at zero.
    let small = attribute_chart(AttributeChartKind::C, &[1.0, 2.0, 0.0], None).unwrap();
    assert!(small.points.iter().all(|p| p.lower == 0.0));
}

/// Fifteen large subgroups with more variation between them than binomial
/// sampling allows. Reference values from an independent NumPy computation of
/// Laney's formulas: p̄ = 0.030407830342577487, σ_z = 2.5162698120528297,
/// UCL₁ = 0.0609591149531062; the p chart flags subgroup 15, the p′ chart
/// nothing.
fn overdispersed() -> ([f64; 15], [f64; 15]) {
    (
        [52.0, 71.0, 40.0, 88.0, 49.0, 70.0, 58.0, 45.0, 82.0, 39.0, 75.0, 60.0, 47.0, 66.0, 90.0],
        [
            1800.0, 2100.0, 1950.0, 2300.0, 2050.0, 1900.0, 2200.0, 2000.0, 2150.0, 1850.0, 2250.0, 2000.0, 1950.0,
            2100.0, 2050.0,
        ],
    )
}

#[test]
fn laney_p_and_u_prime() {
    let (d, n) = overdispersed();
    let p = attribute_chart(AttributeChartKind::P, &d, Some(&n)).unwrap();
    assert_eq!(signals(&p), [15]);
    let lp = attribute_chart(AttributeChartKind::LaneyP, &d, Some(&n)).unwrap();
    near(lp.center, 0.030_407_830_342_577_487, 1e-14);
    near(lp.sigma_z.unwrap(), 2.516_269_812_052_829_7, 1e-12);
    near(lp.points[0].upper, 0.060_959_114_953_106_2, 1e-12);
    assert_eq!(lp.points[0].lower, 0.0);
    assert!(signals(&lp).is_empty());
    // The limits are the p chart's widened by σ_z.
    for (a, b) in lp.points.iter().zip(&p.points) {
        near(a.upper - lp.center, lp.sigma_z.unwrap() * (b.upper - p.center), 1e-12);
    }
    // u′: σ_z = 2.4777173224210323; with the same counts its limits equal p′'s
    // (σ_z,u = σ_z,p·√(1 − p̄) while σ_uᵢ = σ_pᵢ/√(1 − p̄)).
    let lu = attribute_chart(AttributeChartKind::LaneyU, &d, Some(&n)).unwrap();
    near(lu.sigma_z.unwrap(), 2.477_717_322_421_032_3, 1e-12);
    for (a, b) in lu.points.iter().zip(&lp.points) {
        near(a.upper, b.upper, 1e-12);
    }
    // σ_z = 1 when the standardized points move exactly as binomial noise would: here, by construction.
    assert!(attribute_chart(AttributeChartKind::LaneyP, &[3.0], Some(&[100.0])).is_err(), "one subgroup");
    assert!(attribute_chart(AttributeChartKind::LaneyU, &[3.0, 3.0], Some(&[100.0, 100.0])).is_err(), "no variation");
}

/// g chart by hand: ḡ = 160/10 = 16, UCL = 16 + 3√(16·17) = 65.477…, LCL 0.
#[test]
fn g_chart_by_hand() {
    let g = [12.0, 3.0, 25.0, 7.0, 40.0, 18.0, 2.0, 9.0, 30.0, 14.0];
    let c = attribute_chart(AttributeChartKind::G, &g, None).unwrap();
    close(c.center, 16.0);
    close(c.points[0].upper, 16.0 + 3.0 * 272.0_f64.sqrt());
    near(c.points[0].upper, 65.477_267_507_411_92, 1e-14);
    assert_eq!(c.points[0].lower, 0.0);
    assert!(signals(&c).is_empty());
    let mut long = g.to_vec();
    // ḡ = 360/11, UCL = 132.3…
    long.push(200.0);
    assert_eq!(signals(&attribute_chart(AttributeChartKind::G, &long, None).unwrap()), [11]);
    assert!(attribute_chart(AttributeChartKind::G, &[0.0, 0.0], None).is_err());
    assert!(attribute_chart(AttributeChartKind::G, &[1.5, 2.0], None).is_err());
}

/// t chart: independent NumPy computation of Nelson's transformation:
/// x̄ = 2.4116089411799844, σ̂ = 0.96087733461427904, CL = 23.785089356332513,
/// UCL = 403.36046498809725, LCL = 0.
#[test]
fn t_chart_by_independent_computation() {
    let t = [30.2, 4.5, 61.0, 12.8, 22.4, 95.3, 7.7, 41.6, 18.9, 2.1, 55.0, 33.3];
    let c = attribute_chart(AttributeChartKind::T, &t, None).unwrap();
    let (mean, sigma) = c.transformed.unwrap();
    near(mean, 2.411_608_941_179_984_4, 1e-13);
    near(sigma, 0.960_877_334_614_279, 1e-12);
    near(c.center, 23.785_089_356_332_513, 1e-12);
    near(c.points[0].upper, 403.360_464_988_097_25, 1e-11);
    assert_eq!(c.points[0].lower, 0.0);
    assert!(signals(&c).is_empty());
    // Rules apply on the transformed scale, with symmetric limits.
    let (values, limits) = c.rule_scale();
    near(values[0].unwrap(), 30.2_f64.powf(1.0 / 3.6), 1e-14);
    near(limits[0].upper - limits[0].center, limits[0].center - limits[0].lower, 1e-12);
    assert!(attribute_chart(AttributeChartKind::T, &[1.0], None).is_err());
    assert!(attribute_chart(AttributeChartKind::T, &[2.0, 2.0, 2.0], None).is_err());
    assert!(attribute_chart(AttributeChartKind::T, &[2.0, -1.0], None).is_err());
}

#[test]
fn keys_round_trip() {
    for k in AttributeChartKind::ALL {
        assert_eq!(AttributeChartKind::from_key(k.key()), Some(k));
        assert!(k.key().is_ascii());
    }
    assert!(!AttributeChartKind::G.needs_size() && AttributeChartKind::LaneyU.needs_size());
}

/// A p chart whose upper limit is capped at one must still measure its run-
/// rule zones in units of the binomial σ, not of `(1 − p̄)/3`. Hand
/// derivation: sizes 2, counts alternating 2, 1 (16 subgroups) give
/// p̄ = 0.75, σ = √(0.75·0.25/2) = 0.306186, uncapped UCL = 1.668559 (capped
/// at 1), so the points 1.0 and 0.5 lie at z = ±0.816497: inside 1σ. Rule 7
/// (15 within 1σ) fires at subgroups 15 and 16 and no zone rule beyond 1σ
/// fires; with the capped limit the zones shrank to σ = 0.083 and rules 5
/// and 8 fired instead.
#[test]
fn capped_p_limit_keeps_binomial_rule_zones() {
    use crate::phased::varying_violations;
    use crate::rules::RunRule;
    let counts: Vec<f64> = (0..16).map(|i| if i % 2 == 0 { 2.0 } else { 1.0 }).collect();
    let sizes = vec![2.0; 16];
    for kind in [AttributeChartKind::P, AttributeChartKind::LaneyP] {
        let c = attribute_chart(kind, &counts, Some(&sizes)).unwrap();
        if kind == AttributeChartKind::P {
            assert_eq!(c.points[0].upper, 1.0, "the plotted limit is capped at one");
        }
        let sigma = (0.75_f64 * 0.25 / 2.0).sqrt() * c.sigma_z.unwrap_or(1.0);
        let (values, limits) = c.rule_scale();
        for l in &limits {
            near(l.zone_sigma(), sigma, 1e-12);
        }
        let v = varying_violations(&values, &limits, &RunRule::ALL, &[(0, 16)]);
        let rules = |r: RunRule| v.iter().filter(|x| x.rule == r).map(|x| x.index).collect::<Vec<_>>();
        assert!(rules(RunRule::TwoOfThreeBeyondTwoSigma).is_empty(), "{kind:?}: {v:?}");
        assert!(rules(RunRule::EightBeyondOneSigma).is_empty(), "{kind:?}: {v:?}");
        assert!(rules(RunRule::BeyondLimits).is_empty());
        if kind == AttributeChartKind::P {
            assert_eq!(rules(RunRule::FifteenWithinOneSigma), [14, 15]);
        }
    }
}

/// u-chart sizes are inspection units (an area of opportunity), so they may be
/// fractional (Montgomery §7.3.2; NIST/SEMATECH 6.3.3.2): ū = Σc/Σn and limits
/// ū ± 3√(ū/nᵢ), computed by hand. p-type sizes count units and stay whole.
#[test]
fn u_chart_fractional_inspection_units() {
    let u = attribute_chart(AttributeChartKind::U, &[3.0, 5.0, 2.0], Some(&[2.5, 2.5, 1.0])).unwrap();
    let ubar = 10.0 / 6.0;
    near(u.center, ubar, 1e-10);
    near(u.points[0].statistic, 1.2, 1e-10);
    near(u.points[0].upper, ubar + 3.0 * (ubar / 2.5).sqrt(), 1e-10);
    near(u.points[2].upper, ubar + 3.0 * ubar.sqrt(), 1e-10);
    assert_eq!(u.points[2].lower, 0.0);
    let e = attribute_chart(AttributeChartKind::P, &[1.0, 2.0], Some(&[2.5, 3.0])).unwrap_err().to_string();
    assert!(e.contains("whole"), "{e}");
    assert!(attribute_chart(AttributeChartKind::U, &[1.0, 2.0], Some(&[0.0, 3.0])).is_err());
}
