use super::*;
use crate::report::SectionContent;

/// Five curves `a + b·sin(2πt/10)` read at t = 0, 1, …, 10, with a curve
/// level factor `x` driving the sine amplitude `b = 1 + x`, and a group
/// factor `g`.
fn sine_table(extra_short_curve: bool) -> DataTable {
    let mut id = Vec::new();
    let (mut t, mut y, mut x) = (Vec::new(), Vec::new(), Vec::new());
    let mut g = Vec::new();
    let xs = [0.0, 1.0, 2.0, 3.0, 4.0, 0.5, 2.5, 3.5];
    for (i, &xi) in xs.iter().enumerate() {
        for k in 0..=10 {
            let tk = k as f64;
            id.push(format!("C{}", i + 1));
            t.push(tk);
            let noise = ((i * 31 + k * 17) % 13) as f64 / 13.0 - 0.5;
            y.push(2.0 + 0.3 * i as f64 + (1.0 + xi) * (std::f64::consts::TAU * tk / 10.0).sin() + 0.05 * noise);
            x.push(xi);
            g.push(if i % 2 == 0 { "A" } else { "B" }.to_owned());
        }
    }
    if extra_short_curve {
        for k in 0..2 {
            id.push("Short".to_owned());
            t.push(k as f64);
            y.push(1.0);
            x.push(1.0);
            g.push("A".to_owned());
        }
    }
    DataTable::new(
        "sines",
        vec![
            stat_data::Column::text("Curve", id),
            stat_data::Column::numeric("t", t),
            stat_data::Column::numeric("y", y),
            stat_data::Column::numeric("x", x),
            stat_data::Column::text("g", g),
        ],
    )
    .unwrap()
}

fn run(table: &DataTable, request: &FunctionalRequest) -> Result<FunctionalAnalysis> {
    FunctionalAnalysis::run(table, &RowStateManager::new(table.n_rows()), request)
}

#[test]
fn one_component_carries_the_amplitude_and_the_model_predicts_curves() {
    let t = sine_table(true);
    let request = FunctionalRequest { factors: vec![3, 4], ..FunctionalRequest::new(0, 1, 2) };
    let a = run(&t, &request).unwrap();
    assert_eq!(a.labels().len(), 8, "the two-point curve is skipped");
    let summary = &a.sections()[0];
    let SectionContent::Table(s) = &summary.content else { panic!() };
    assert!(s.rows.iter().any(|r| r[0].starts_with("Curves skipped") && r[1] == "Short"));
    // Amplitude and level vary: two components explain nearly everything.
    let p = a.fpca();
    assert!(p.proportion(0) + p.proportion(1) > 0.99);
    assert!(a.has_model());
    // The predicted curve at a curve's own settings is close to it, and
    // the limits contain the prediction.
    let settings = vec![TermValue::Number(4.0), TermValue::Level(0), TermValue::Number(2.5)];
    let pred = a.predict_with_limits(&[settings]).unwrap()[0];
    let (lo, hi) = pred.1.unwrap();
    assert!(lo < pred.0 && pred.0 < hi);
    let truth = 2.0 + 0.3 * 4.0 + 5.0 * (std::f64::consts::TAU * 0.25).sin();
    assert!((pred.0 - truth).abs() < 0.6, "{} vs {truth}", pred.0);
    // The profiler has the two factors and time.
    let names: Vec<String> = a.profile_factors().into_iter().map(|f| f.name).collect();
    assert_eq!(names, ["x", "g", "t"]);
    assert_eq!(a.observations().len(), 88);
}

#[test]
fn saved_columns_cover_every_row_of_a_curve() {
    let t = sine_table(false);
    let request = FunctionalRequest {
        landmarks: vec![FunctionalLandmark { maximum: true, window: Some((1.0, 4.0)) }],
        components: Some(2),
        ..FunctionalRequest::new(0, 1, 2)
    };
    let a = run(&t, &request).unwrap();
    let cols = a.saveable_columns();
    let keys: Vec<&str> = cols.iter().map(|c| c.key.as_str()).collect();
    assert_eq!(keys, ["fpc1", "fpc2", "smoothed", "registered_time"]);
    assert!(cols.iter().all(|c| c.values.iter().all(|v| v.is_finite())));
    // Rows of one curve share its score.
    assert!(cols[0].values[..11].iter().all(|v| *v == cols[0].values[0]));
    // Registered times run from the start to the end of the range.
    assert!(cols[3].values[0].abs() < 1e-9 && (cols[3].values[10] - 10.0).abs() < 1e-9);
}

#[test]
fn factors_must_be_constant_within_a_curve() {
    let mut t = sine_table(false);
    let mut x = t.column(3).unwrap().as_numeric().unwrap().to_vec();
    x[5] = 9.0;
    t.replace_column(3, stat_data::Column::numeric("x", x)).unwrap();
    let request = FunctionalRequest { factors: vec![3], ..FunctionalRequest::new(0, 1, 2) };
    let e = run(&t, &request).unwrap_err().to_string();
    assert!(e.contains("constant within each curve") && e.contains("C1"), "{e}");
}

#[test]
fn settings_are_checked() {
    let t = sine_table(false);
    let too_many = FunctionalRequest { components: Some(8), ..FunctionalRequest::new(0, 1, 2) };
    assert!(run(&t, &too_many).is_err());
    let text_time = FunctionalRequest::new(0, 4, 2);
    assert!(run(&t, &text_time).unwrap_err().to_string().contains("must be numeric"));
    // A landmark window where the minimum precedes the maximum the wrong way.
    let crossed = FunctionalRequest {
        landmarks: vec![
            FunctionalLandmark { maximum: true, window: Some((1.0, 4.0)) },
            FunctionalLandmark { maximum: false, window: Some((0.5, 1.5)) },
        ],
        ..FunctionalRequest::new(0, 1, 2)
    };
    assert!(run(&t, &crossed).unwrap_err().to_string().contains("increasing order"));
}

#[test]
fn fourier_basis_and_unpenalized_fits() {
    let t = sine_table(false);
    let fourier = FunctionalRequest {
        basis: FunctionalBasis::Fourier { functions: Some(5), period: Some(10.0) },
        ..FunctionalRequest::new(0, 1, 2)
    };
    let a = run(&t, &fourier).unwrap();
    // A periodic sine is in the basis: almost no residual.
    assert!(a.smoothed().sse.iter().all(|s| *s < 0.05), "{:?}", a.smoothed().sse);
    let plain = FunctionalRequest { smoothing: FunctionalSmoothing::None, ..FunctionalRequest::new(0, 1, 2) };
    let b = run(&t, &plain).unwrap();
    assert_eq!(b.smoothed().data.basis.n_basis(), 10);
    assert_eq!(b.smoothed().lambda, 0.0);
}

#[test]
fn json_round_trip_keeps_every_setting() {
    let t = sine_table(false);
    let request = FunctionalRequest {
        factors: vec![3, 4],
        basis: FunctionalBasis::BSpline { functions: Some(9), order: 3 },
        smoothing: FunctionalSmoothing::Fixed(0.5),
        landmarks: vec![FunctionalLandmark { maximum: false, window: Some((5.0, 9.0)) }],
        continuous: true,
        warp_functions: 4,
        warp_penalty: 0.01,
        components: Some(3),
        interactions: true,
        confidence: 0.9,
        ..FunctionalRequest::new(0, 1, 2)
    };
    let profilers = crate::ProfilerOptions { show_profiler: false, show_surface_profiler: true, show_observed: true };
    let json = request.to_json(&t, &profilers);
    let (back, p) = FunctionalRequest::from_json(&json, &t).unwrap();
    assert_eq!(back, request);
    assert_eq!(p, profilers);
}
