use super::*;
use crate::project::SavedReport;
use stat_data::Column;

/// Two curves (A, B) of two Gaussian bands on a sloping background, with one
/// excluded outlier row in curve A.
fn spectra() -> DataTable {
    let mut x = Vec::new();
    let mut y = Vec::new();
    let mut g = Vec::new();
    let mut known = Vec::new();
    let mut rng = stat_core::rng::Rng::new(20261019);
    for (name, scale) in [("A", 1.0), ("B", 0.6)] {
        for i in 0..240 {
            let t = 1000.0 + 2.0 * i as f64;
            let base = 3.0 + 0.004 * (t - 1000.0);
            let bands = scale
                * (40.0 * (-4.0 * 2f64.ln() * ((t - 1200.0) / 30.0).powi(2)).exp()
                    + 25.0 * (-4.0 * 2f64.ln() * ((t - 1240.0) / 26.0).powi(2)).exp());
            let wiggle = 0.05 * rng.normal();
            x.push(t);
            y.push(base + bands + wiggle);
            g.push(name.to_owned());
            known.push(base);
        }
    }
    x.push(1100.0);
    y.push(5000.0);
    g.push("A".to_owned());
    known.push(0.0);
    DataTable::new(
        "spectra",
        vec![
            Column::numeric("Wavenumber", x),
            Column::numeric("Intensity", y),
            Column::text("Sample", g),
            Column::numeric("Background", known),
        ],
    )
    .unwrap()
}

fn rows(t: &DataTable) -> RowStateManager {
    let mut r = RowStateManager::new(t.n_rows());
    r.set_excluded([t.n_rows() - 1], true);
    r
}

#[test]
fn requests_round_trip_through_json() {
    let t = spectra();
    let requests = [
        SignalRequest::Smoothing(SignalSmoothingRequest {
            x: Some(0),
            group: Some(2),
            method: Smoother::RunningPercentile { window: 9, percentile: 25.0 },
            ..SignalSmoothingRequest::new(1)
        }),
        SignalRequest::Smoothing(SignalSmoothingRequest::new(1)),
        SignalRequest::Calculus(CalculusRequest {
            derivative: DerivativeMethod::SavitzkyGolay { window: 7, order: 2 },
            integral: Integration::Simpson,
            resample: Some(ResampleSettings { method: Interpolation::Pchip, points: 50 }),
            ..CalculusRequest::new(0, 1)
        }),
        SignalRequest::Spectrum(SpectrumRequest {
            curve: Some(2),
            baseline: SpectrumBaseline::Column(3),
            fit: Some(PeakFitSettings {
                shape: PeakShape::Voigt,
                baseline: BaselineTerm::Linear,
                count: Some(2),
                centres: vec![1200.0, 1240.0],
                compare: false,
                common_width: true,
                spacing: Spacing::Fixed(40.0),
            }),
            ..SpectrumRequest::new(0, 1)
        }),
        SignalRequest::Spectrum(SpectrumRequest {
            baseline: SpectrumBaseline::Estimated(BaselineMethod::Snip { half_window: 15, decreasing: true, lls: true }),
            detection: PeakDetection { smoothing: None, min_height: 2.0, min_prominence: 4.0, min_separation: 10.0 },
            fit: Some(PeakFitSettings { spacing: Spacing::Equal, ..PeakFitSettings::default() }),
            ..SpectrumRequest::new(0, 1)
        }),
    ];
    for r in requests {
        let json = r.to_json(&t);
        let key = json.get("analysis").and_then(JsonValue::as_str).unwrap().to_owned();
        let back = SignalRequest::from_json(&key, &json, &t).unwrap();
        assert_eq!(back, r, "{key}");
    }
}

#[test]
fn smoothing_by_group_saves_one_value_per_row() {
    let t = spectra();
    let r = SignalSmoothingRequest { x: Some(0), group: Some(2), ..SignalSmoothingRequest::new(1) };
    let a = SignalSmoothingAnalysis::run(&t, &rows(&t), &r).unwrap();
    assert_eq!(a.title(), "Smoothing: Intensity (Sample) — Savitzky–Golay (11 points, degree 3)");
    let s = a.sections();
    let SectionContent::Table(summary) = &s[0].content else { panic!() };
    assert_eq!(summary.rows.len(), 2);
    assert_eq!(summary.rows[0][1], "240", "the excluded row is left out");
    let cols = a.saveable_columns();
    assert_eq!(cols[0].key, "smoothed");
    assert!(cols[0].values[..480].iter().all(|v| v.is_finite()));
    assert!(cols[0].values[480].is_nan());
    // The same values as the engine on one curve.
    let ys: Vec<f64> = t.columns()[1].as_numeric().unwrap()[..240].to_vec();
    let direct = savitzky_golay(&ys, 11, 3, 0, 2.0).unwrap();
    assert!((cols[0].values[17] - direct[17]).abs() < 1e-12);
}

#[test]
fn calculus_differentiates_and_integrates() {
    let x: Vec<f64> = (0..21).map(|i| 0.5 * i as f64).collect();
    let y: Vec<f64> = x.iter().map(|t| t * t).collect();
    let t = DataTable::new("q", vec![Column::numeric("x", x), Column::numeric("y", y)]).unwrap();
    let r = CalculusRequest {
        integral: Integration::Simpson,
        resample: Some(ResampleSettings { method: Interpolation::CubicSpline, points: 11 }),
        ..CalculusRequest::new(0, 1)
    };
    let a = CalculusAnalysis::run(&t, &RowStateManager::new(21), &r).unwrap();
    let cols = a.saveable_columns();
    assert!((cols[0].values[10] - 10.0).abs() < 1e-12, "d(x²)/dx = 2x at x = 5");
    assert!((cols[1].values[20] - 1000.0 / 3.0).abs() < 1e-9, "∫₀¹⁰ x² dx");
    let s = a.sections();
    let resampled = s.iter().find(|s| s.kind == SectionKind::SignalResampled).unwrap();
    let SectionContent::Table(tab) = &resampled.content else { panic!() };
    assert_eq!(tab.rows.len(), 11);
    assert_eq!(tab.rows[1], ["1", "1"]);
}

#[test]
fn spectrum_peaks_and_batch_fit() {
    let t = spectra();
    let r = SpectrumRequest {
        curve: Some(2),
        baseline: SpectrumBaseline::Column(3),
        fit: Some(PeakFitSettings { count: Some(2), ..PeakFitSettings::default() }),
        ..SpectrumRequest::new(0, 1)
    };
    let a = SpectrumAnalysis::run(&t, &rows(&t), &r).unwrap();
    let curves = a.curves();
    assert_eq!(curves.len(), 2);
    for (name, c) in curves {
        let c = c.unwrap();
        assert_eq!(c.peaks.len(), 2, "{name}");
        let f = c.fit.as_ref().unwrap().as_ref().unwrap();
        assert!(f.converged());
        assert!((f.peaks[0].centre.0 - 1200.0).abs() < 0.5, "{name}: {}", f.peaks[0].centre.0);
        assert!((f.peaks[1].centre.0 - 1240.0).abs() < 0.5, "{name}");
        let scale = if name == "A" { 1.0 } else { 0.6 };
        assert!((f.peaks[0].height.0 - 40.0 * scale).abs() < 0.3, "{name}");
        assert!(c.comparison.as_ref().is_some_and(|rows| rows.len() == 2));
    }
    let s = a.sections();
    let batch = s.iter().find(|s| s.kind == SectionKind::SpectrumBatch).unwrap();
    let SectionContent::Table(tab) = &batch.content else { panic!() };
    assert_eq!(tab.rows.len(), 2);
    assert_eq!(tab.columns.len(), 1 + 8 + 2);
    let keys: Vec<String> = a.saveable_columns().into_iter().map(|c| c.key).collect();
    assert_eq!(keys, ["baseline", "corrected", "smoothed", "fitted", "residual"]);
    // Through the shared entry points.
    let saved = SavedReport::Signal { request: SignalRequest::Spectrum(r) };
    let computed = crate::run_report(&saved, &t, &rows(&t)).unwrap();
    assert!(computed.title.starts_with("Spectrum Peaks"));
}

#[test]
fn estimated_baseline_finds_both_bands() {
    let t = spectra();
    let r = SpectrumRequest::new(0, 1);
    let mut states = rows(&t);
    // Only curve A.
    states.set_excluded(240..480, true);
    let a = SpectrumAnalysis::run(&t, &states, &r).unwrap();
    let curves = a.curves();
    let c = curves[0].1.unwrap();
    let positions: Vec<f64> = c.peaks.iter().map(|p| 1000.0 + 2.0 * p.index as f64).collect();
    assert_eq!(positions.len(), 2, "{positions:?}");
    assert!((positions[0] - 1200.0).abs() <= 4.0 && (positions[1] - 1240.0).abs() <= 4.0, "{positions:?}");
}

/// Settings left out of a script's or project's request take the dialog's
/// defaults.
#[test]
fn missing_settings_take_the_dialog_defaults() {
    let t = spectra();
    let parse = |key: &str, text: &str| SignalRequest::from_json(key, &stat_data::json::parse_json(text).unwrap(), &t).unwrap();
    let xy = r#"{"x": "Wavenumber", "y": "Intensity"}"#;
    assert_eq!(parse("spectrum-peaks", xy), SignalRequest::Spectrum(SpectrumRequest::new(0, 1)));
    assert_eq!(parse("curve-calculus", xy), SignalRequest::Calculus(CalculusRequest::new(0, 1)));
    assert_eq!(parse("signal-smoothing", r#"{"y": "Intensity"}"#), SignalRequest::Smoothing(SignalSmoothingRequest::new(1)));
}
