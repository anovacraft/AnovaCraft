use std::f64::consts::PI;

use stat_data::Column;
use stat_signal::convolve::CorrelationScaling;
use stat_signal::filter::IirFamily;

use super::*;
use crate::project::{SavedReport, report_from_json, report_to_json};

fn table_of(sections: &[ReportSection], kind: SectionKind) -> &TextTable {
    match &sections.iter().find(|s| s.kind == kind).unwrap_or_else(|| panic!("{kind:?} missing")).content {
        SectionContent::Table(t) => t,
        other => panic!("{kind:?} is not a table: {other:?}"),
    }
}

fn cell(t: &TextTable, row: usize, column: &str) -> f64 {
    t.cell(row, column)
        .unwrap_or_else(|| panic!("no {column}"))
        .replace('−', "-")
        .trim_end_matches('%')
        .parse()
        .unwrap()
}

/// 400 samples at 100 per second: tones at 5 and 30 Hz, a copy of the first
/// tone delayed by 7 samples, a short kernel, and two groups.
fn bench() -> DataTable {
    let n = 400;
    let fs = 100.0;
    let t: Vec<f64> = (0..n).map(|i| i as f64 / fs).collect();
    let tone = |f: f64, d: usize| -> Vec<f64> {
        (0..n).map(|i| if i >= d { (2.0 * PI * f * (i - d) as f64 / fs).sin() } else { 0.0 }).collect()
    };
    let low = tone(5.0, 0);
    let high = tone(30.0, 0);
    let mix: Vec<f64> = low.iter().zip(&high).map(|(a, b)| a + 0.5 * b).collect();
    let delayed = tone(5.0, 7);
    let mut kernel = vec![f64::NAN; n];
    kernel[..3].copy_from_slice(&[1.0, 2.0, 1.0]);
    let group: Vec<String> = (0..n).map(|i| if i < n / 2 { "A" } else { "B" }.to_owned()).collect();
    DataTable::new(
        "bench",
        vec![
            Column::numeric("Time", t),
            Column::numeric("Mix", mix),
            Column::numeric("Delayed", delayed),
            Column::numeric("Kernel", kernel),
            Column::text("Run", group),
            Column::numeric("Low", low),
        ],
    )
    .unwrap()
}

fn run(t: &DataTable, r: &SignalProcessingRequest) -> SignalAnalysis {
    SignalAnalysis::run(t, &RowStateManager::new(t.n_rows()), r).unwrap()
}

fn timed(column: usize, method: SignalMethod) -> SignalProcessingRequest {
    SignalProcessingRequest { input: SignalInput { column, x: Some(0), group: None, sampling_rate: None }, method }
}

#[test]
fn spectrum_finds_both_tones() {
    let t = bench();
    let a = run(
        &t,
        &timed(
            1,
            SignalMethod::Spectrum(SpectrumSettings {
                spectrogram: true,
                welch: true,
                segment_length: 64,
                ..SpectrumSettings::default()
            }),
        ),
    );
    assert_eq!(a.title(), "Fourier Spectrum: Mix");
    let s = a.sections();
    let top = table_of(&s, SectionKind::DominantFrequencies);
    let mut freqs = [cell(top, 0, "Frequency"), cell(top, 1, "Frequency")];
    freqs.sort_by(f64::total_cmp);
    assert!((freqs[0] - 5.0).abs() < 0.3 && (freqs[1] - 30.0).abs() < 0.3, "{freqs:?}");
    let summary = table_of(&s, SectionKind::SignalProcessingSummary);
    assert_eq!(summary.cell(0, "Value"), Some("Mix"));
    assert!(s.iter().any(|x| matches!(x.content, SectionContent::IntensityMap(_))), "spectrogram");
    assert!(s.iter().any(|x| x.kind == SectionKind::WelchSpectrum));
    assert!(a.saveable_columns().is_empty());
}

#[test]
fn uneven_rows_are_resampled_and_groups_split() {
    let t = bench();
    let mut states = RowStateManager::new(t.n_rows());
    states.set_excluded([10, 11, 250], true);
    let input = SignalInput { column: 1, x: Some(0), group: Some(4), sampling_rate: None };
    let sig = Signals::extract(&t, &states, &input, 4).unwrap();
    assert_eq!(sig.traces.len(), 2);
    assert_eq!(sig.traces[0].label, "A");
    assert!(sig.traces[0].resampled && sig.traces[1].resampled);
    assert!((sig.sampling_rate - 100.0).abs() < 1e-9);
    assert_eq!(sig.left_out, 3);
    assert_eq!(sig.frequency_unit, "cycles per unit of Time");
    // The grid keeps every 0.01 s step; the excluded samples are interpolated.
    assert_eq!(sig.traces[0].values.len(), 200);
    // Without an x column: row order at the given rate.
    let plain =
        Signals::extract(&t, &states, &SignalInput { sampling_rate: Some(50.0), ..SignalInput::new(1) }, 4).unwrap();
    assert!((plain.sampling_rate - 50.0).abs() < 1e-9);
    assert_eq!(plain.time_label, "Time");
}

#[test]
fn lowpass_filter_removes_the_high_tone() {
    let t = bench();
    let settings = FilterSettings {
        design: FilterDesign::Iir { family: IirFamily::Butterworth, order: 6 },
        ..FilterSettings::lowpass(12.0)
    };
    let a = run(&t, &timed(1, SignalMethod::Filter(settings)));
    let cols = a.saveable_columns();
    let filtered = &cols.iter().find(|c| c.key == "filtered").unwrap().values;
    let low = t.column(5).unwrap().as_numeric().unwrap();
    // Away from the ends the zero-phase filter leaves the 5 Hz tone.
    let err = (50..350).map(|i| (filtered[i] - low[i]).abs()).fold(0.0, f64::max);
    assert!(err < 0.02, "max error {err}");
    let kinds: Vec<SectionKind> = a.sections().iter().map(|s| s.kind).collect();
    assert!(kinds.contains(&SectionKind::FilterResponse) && kinds.contains(&SectionKind::FilterCoefficients));
    assert!(!kinds.contains(&SectionKind::FilterPhase), "zero phase has no phase plot");
    for design in [
        FilterDesign::Fir { taps: 61, window: stat_signal::window::Window::Hamming },
        FilterDesign::Fft { shape: stat_signal::filter::FftFilterShape::Ideal },
        FilterDesign::Iir { family: IirFamily::Elliptic { ripple_db: 0.5, attenuation_db: 60.0 }, order: 4 },
    ] {
        let a = run(&t, &timed(1, SignalMethod::Filter(FilterSettings { design, ..FilterSettings::lowpass(12.0) })));
        let f = &a.saveable_columns()[0].values;
        let err = (80..320).map(|i| (f[i] - low[i]).abs()).fold(0.0, f64::max);
        assert!(err < 0.08, "{design:?}: max error {err}");
    }
}

#[test]
fn cross_correlation_finds_the_delay() {
    let t = bench();
    let op = CorrelationOperation::Cross { other: 5, scaling: CorrelationScaling::Coefficient, max_lag: Some(20) };
    let a = run(&t, &timed(2, SignalMethod::Correlation(CorrelationSettings { operation: op })));
    let peaks = table_of(&a.sections(), SectionKind::CorrelationPeaks).clone();
    assert_eq!(cell(&peaks, 0, "Lag of maximum (samples)"), 7.0);
    assert!((cell(&peaks, 0, "Lag of maximum (time)") - 0.07).abs() < 1e-9);
    // Envelope of a unit tone is about 1 away from the ends.
    let a = run(
        &t,
        &timed(5, SignalMethod::Correlation(CorrelationSettings { operation: CorrelationOperation::Envelope })),
    );
    let env = &a.saveable_columns().into_iter().find(|c| c.key == "envelope").unwrap().values;
    assert!((50..350).all(|i| (env[i] - 1.0).abs() < 0.02));
    // Convolution with [1, 2, 1] / 4 keeps the length.
    let op = CorrelationOperation::Convolution { kernel: 3, normalize: true };
    let a = run(&t, &timed(5, SignalMethod::Correlation(CorrelationSettings { operation: op })));
    let conv = &a.saveable_columns()[0].values;
    assert_eq!(conv.iter().filter(|v| v.is_finite()).count(), 400);
}

#[test]
fn wavelet_denoising_recovers_the_tone() {
    let t = bench();
    let mut rng = stat_core::rng::Rng::new(3);
    // A 1 Hz tone lies in the coarsest approximation, which is not thresholded.
    let low: Vec<f64> = (0..400).map(|i| (2.0 * PI * i as f64 / 100.0).sin()).collect();
    let noisy: Vec<f64> = low.iter().map(|v| v + 0.2 * rng.normal()).collect();
    let mut t2 = t.clone();
    t2.insert_column(t2.n_cols(), Column::numeric("Noisy", noisy.clone())).unwrap();
    let col = t2.n_cols() - 1;
    let settings = WaveletSettings { wavelet: "sym8".to_owned(), scalogram: true, ..WaveletSettings::default() };
    let a = run(&t2, &timed(col, SignalMethod::Wavelet(settings)));
    let s = a.sections();
    let den = &a.saveable_columns().into_iter().find(|c| c.key == "denoised").unwrap().values;
    let rmse = |v: &[f64]| ((0..400).map(|i| (v[i] - low[i]).powi(2)).sum::<f64>() / 400.0).sqrt();
    assert!(rmse(den) < 0.5 * rmse(&noisy), "{} vs {}", rmse(den), rmse(&noisy));
    let energy = table_of(&s, SectionKind::WaveletEnergy);
    let noise_sd = cell(energy, 0, "Noise SD");
    assert!((noise_sd - 0.2).abs() < 0.05, "noise sd {noise_sd}");
    let SectionContent::IntensityMap(map) = &s.iter().find(|x| x.kind == SectionKind::Scalogram).unwrap().content
    else {
        panic!()
    };
    // The scalogram peaks near 1 Hz.
    let (mut best, mut at) = (0.0, 0.0);
    for col in &map.values {
        for (j, &v) in col.iter().enumerate() {
            if v > best {
                best = v;
                at = map.y_value(map.y[j]);
            }
        }
    }
    assert!((at - 1.0).abs() < 0.25, "scalogram peak at {at}");
    let SectionContent::Plot(p) = &s.iter().find(|x| x.kind == SectionKind::WaveletDecomposition).unwrap().content
    else {
        panic!()
    };
    let levels = p.groups.as_ref().unwrap().lines.len();
    assert_eq!(levels, p.y_ticks.as_ref().unwrap().len());
}

#[test]
fn requests_round_trip_through_project_json() {
    let t = bench();
    let requests = [
        timed(
            1,
            SignalMethod::Spectrum(SpectrumSettings {
                window: stat_signal::window::Window::Kaiser { beta: 5.0 },
                welch: true,
                ..SpectrumSettings::default()
            }),
        ),
        timed(
            1,
            SignalMethod::Filter(FilterSettings {
                design: FilterDesign::Iir { family: IirFamily::ChebyshevII { attenuation_db: 50.0 }, order: 5 },
                band: BandKind::Bandstop,
                low: 10.0,
                high: 20.0,
                zero_phase: false,
            }),
        ),
        timed(
            1,
            SignalMethod::Filter(FilterSettings {
                design: FilterDesign::Notch { quality: 20.0 },
                ..FilterSettings::lowpass(30.0)
            }),
        ),
        SignalProcessingRequest {
            input: SignalInput { column: 2, x: None, group: Some(4), sampling_rate: Some(100.0) },
            method: SignalMethod::Correlation(CorrelationSettings {
                operation: CorrelationOperation::Deconvolution {
                    kernel: 3,
                    method: DeconvolutionMethod::Wiener { noise_ratio: 0.01 },
                },
            }),
        },
        timed(
            1,
            SignalMethod::Wavelet(WaveletSettings {
                levels: Some(3),
                rule: stat_signal::wavelet::ThresholdRule::Fixed(0.3),
                kind: stat_signal::wavelet::ThresholdKind::Hard,
                extension: stat_signal::wavelet::Extension::Periodic,
                continuous: stat_signal::wavelet::ContinuousWavelet::MexicanHat,
                ..WaveletSettings::default()
            }),
        ),
    ];
    for r in requests {
        let saved = SavedReport::SignalProcessing { request: r.clone() };
        assert_eq!(report_from_json(&report_to_json(&saved, &t), &t).unwrap(), saved, "{r:?}");
        SignalAnalysis::run(&t, &RowStateManager::new(t.n_rows()), &r).unwrap_or_else(|e| panic!("{r:?}: {e}"));
    }
}

#[test]
fn formula_transforms_work_within_groups() {
    let t = bench();
    let mix = t.column(1).unwrap().as_numeric().unwrap();
    let groups: Vec<usize> = (0..400).map(|i| usize::from(i >= 200)).collect();
    let low = formula_transform("lowpass:0.12", mix, &groups).unwrap();
    let want = t.column(5).unwrap().as_numeric().unwrap();
    assert!((40..160).all(|i| (low[i] - want[i]).abs() < 0.05));
    assert!((240..360).all(|i| (low[i] - want[i]).abs() < 0.05));
    let env = formula_transform("envelope", want, &vec![0; 400]).unwrap();
    assert!((50..350).all(|i| (env[i] - 1.0).abs() < 0.02));
    assert!(formula_transform("wavelet_denoise:db4", mix, &groups).unwrap().iter().all(|v| v.is_finite()));
    assert!(formula_transform("lowpass:0.7", mix, &groups).is_err());
    assert!(formula_transform("nonsense", mix, &groups).is_err());
}
