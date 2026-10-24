//! Sample for Series and Signals ▸ Signals and spectra: test-bench recordings of three runs.

use std::f64::consts::PI;

use super::{Sample, col, table};
use crate::project::SavedReport;
use crate::signal_analysis::{
    CorrelationOperation, CorrelationSettings, FilterSettings, SignalInput, SignalMethod, SignalProcessingRequest,
    SpectrumSettings, WaveletSettings,
};
use stat_core::rng::Rng;
use stat_data::{Column, DataTable};
use stat_signal::convolve::CorrelationScaling;

/// Samples per second.
const RATE: f64 = 500.0;
/// Samples per run (2 seconds).
const PER_RUN: usize = 1000;

/// **Simulated** recordings from a vibration test bench: three runs (A, B,
/// C) of 2 s each at 500 samples per second, stacked in long format (3 000
/// rows). In each run a shaker drives the structure with a random
/// excitation `e_t = 0.8 e_{t−1} + 0.2 ε_t`.
///
/// * **Microphone** = `a sin(2π·50 t) + 0.3 sin(2π·120 t) + e_t + 0.15 η_t`:
///   mains hum at 50 Hz (amplitude `a` = 0.6, 0.8, 1.0 in runs A, B, C), a
///   motor tone at 120 Hz, the excitation, and noise.
/// * **Accelerometer** = `1.2 e_{t−5} + 0.1 ζ_t`: the excitation reaches the
///   accelerometer 5 samples (10 ms) after the microphone.
/// * **Strain** = `s(t) + 0.08 ξ_t`: a step of 1 at t = 1 s followed by a
///   ringing `0.6 e^{−4(t−1)} sin(2π·8 (t−1))`, plus noise.
/// * **Smoothing kernel**: the binomial weights 1, 4, 6, 4, 1 in its first
///   five rows (missing below).
///
/// `ε, η, ζ, ξ ~ N(0, 1)` independent, drawn with `stat_core::rng::Rng`
/// (seed `0x5161_A15E`); values rounded to 0.0001.
#[must_use]
pub fn test_bench_signals() -> DataTable {
    let mut rng = Rng::new(0x5161_A15E);
    let round = |v: f64| (v * 1e4).round() / 1e4;
    let n = 3 * PER_RUN;
    let (mut run, mut time, mut mic, mut acc, mut strain) =
        (Vec::with_capacity(n), Vec::with_capacity(n), Vec::new(), Vec::new(), Vec::new());
    for (label, hum) in [("A", 0.6), ("B", 0.8), ("C", 1.0)] {
        // Excitation with a short lead-in so the delayed copy starts settled.
        let mut e = vec![0.0; PER_RUN + 50];
        for t in 1..e.len() {
            e[t] = 0.8 * e[t - 1] + 0.2 * rng.normal();
        }
        for k in 0..PER_RUN {
            let t = k as f64 / RATE;
            let now = e[k + 50];
            run.push(label.to_owned());
            time.push(round(t));
            mic.push(round(
                hum * (2.0 * PI * 50.0 * t).sin() + 0.3 * (2.0 * PI * 120.0 * t).sin() + now + 0.15 * rng.normal(),
            ));
            acc.push(round(1.2 * e[k + 45] + 0.1 * rng.normal()));
            let s = if t < 1.0 {
                0.0
            } else {
                let u = t - 1.0;
                1.0 + 0.6 * (-4.0 * u).exp() * (2.0 * PI * 8.0 * u).sin()
            };
            strain.push(round(s + 0.08 * rng.normal()));
        }
    }
    let mut kernel = vec![f64::NAN; n];
    kernel[..5].copy_from_slice(&[1.0, 4.0, 6.0, 4.0, 1.0]);
    table(
        "Test bench signals (simulated)",
        vec![
            Column::text("Run", run),
            Column::numeric("Time (s)", time),
            Column::numeric("Microphone", mic),
            Column::numeric("Accelerometer", acc),
            Column::numeric("Strain", strain),
            Column::numeric("Smoothing kernel", kernel),
        ],
    )
}

pub(super) fn test_bench_signals_sample() -> Sample {
    let t = test_bench_signals();
    let input = |name: &str| SignalInput {
        column: col(&t, name),
        x: Some(col(&t, "Time (s)")),
        group: Some(col(&t, "Run")),
        sampling_rate: None,
    };
    let reports = vec![
        SignalProcessingRequest {
            input: input("Microphone"),
            method: SignalMethod::Spectrum(SpectrumSettings {
                welch: true,
                spectrogram: true,
                segment_length: 128,
                ..SpectrumSettings::default()
            }),
        },
        SignalProcessingRequest {
            input: input("Microphone"),
            method: SignalMethod::Filter(FilterSettings::lowpass(80.0)),
        },
        SignalProcessingRequest {
            input: input("Accelerometer"),
            method: SignalMethod::Correlation(CorrelationSettings {
                operation: CorrelationOperation::Cross {
                    other: col(&t, "Microphone"),
                    scaling: CorrelationScaling::Coefficient,
                    max_lag: Some(50),
                },
            }),
        },
        SignalProcessingRequest {
            input: input("Strain"),
            method: SignalMethod::Wavelet(WaveletSettings {
                wavelet: "sym4".to_owned(),
                scalogram: true,
                ..WaveletSettings::default()
            }),
        },
    ];
    Sample { table: t, reports: reports.into_iter().map(|request| SavedReport::SignalProcessing { request }).collect() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SignalAnalysis;
    use crate::report::{SectionContent, SectionKind};
    use stat_data::RowStateManager;

    /// The reports show what the data were built with: the 50 and 120 Hz
    /// tones, the 10 ms delay, and noise removed from the strain.
    #[test]
    fn sample_reports_find_what_was_built_in() {
        let s = test_bench_signals_sample();
        let rows = RowStateManager::new(s.table.n_rows());
        let run = |i: usize| {
            let SavedReport::SignalProcessing { request } = &s.reports[i] else { panic!() };
            SignalAnalysis::run(&s.table, &rows, request).unwrap()
        };
        let table = |a: &SignalAnalysis, kind: SectionKind| {
            let sections = a.sections();
            match &sections.iter().find(|x| x.kind == kind).unwrap().content {
                SectionContent::Table(t) => t.clone(),
                _ => panic!(),
            }
        };
        let spectrum = run(0);
        let top = table(&spectrum, SectionKind::DominantFrequencies);
        let f: Vec<f64> = (0..top.rows.len())
            .filter(|&r| top.cell(r, "Rank") == Some("1") || top.cell(r, "Rank") == Some("2"))
            .map(|r| top.cell(r, "Frequency").unwrap().parse().unwrap())
            .collect();
        assert!(f.iter().all(|f| (f - 50.0).abs() < 1.0 || (f - 120.0).abs() < 1.0), "{f:?}");
        let cross = run(2);
        let peaks = table(&cross, SectionKind::CorrelationPeaks);
        for r in 0..3 {
            assert_eq!(peaks.cell(r, "Lag of maximum (samples)"), Some("5"), "run {r}");
        }
        assert_eq!(run(1).saveable_columns()[0].values.len(), 3000);
        let wavelet = run(3);
        assert!(wavelet.sections().iter().filter(|x| x.kind == SectionKind::Scalogram).count() == 3);
    }
}
