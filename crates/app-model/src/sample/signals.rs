//! Samples for Series and Signals ▸ Signals and spectra: overlapping Raman bands on
//! a fluorescence background, a machine vibration record and an ECG-like
//! trace. All invented or simulated.

use super::{Sample, col, table};
use crate::project::SavedReport;
use crate::signal_curves::{
    CalculusRequest, PeakDetection, PeakFitSettings, SignalRequest, SignalSmoothingRequest, SpectrumBaseline,
    SpectrumRequest,
};
use stat_core::rng::Rng;
use stat_data::{Column, DataTable};
use stat_signal::peak_fit::BaselineTerm;
use stat_signal::{BaselineMethod, Integration, PeakShape, Smoother};

/// Pseudo-Voigt profile of height 1: Lorentzian share `eta`.
fn pseudo_voigt(x: f64, centre: f64, fwhm: f64, eta: f64) -> f64 {
    let u = (x - centre) / (fwhm / 2.0);
    eta / (1.0 + u * u) + (1.0 - eta) * (-std::f64::consts::LN_2 * u * u).exp()
}

/// Four invented Raman spectra (1450–1800 cm⁻¹ every 0.5 cm⁻¹) of mixtures:
/// two overlapping bands near 1585 and 1604 cm⁻¹ and a broader band at
/// 1655 cm⁻¹, whose heights change with the mixture, on a sloping, curved
/// fluorescence background that differs between samples, with noise. The
/// background itself is included, for comparison with the estimated one.
pub fn raman_bands() -> DataTable {
    let mut rng = Rng::new(0x2A3A_0B1D);
    let (mut shift, mut intensity, mut sample, mut mixture, mut known) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (k, (name, share)) in [("S1", 0.35), ("S2", 0.45), ("S3", 0.55), ("S4", 0.65)].into_iter().enumerate() {
        let tilt = 0.05 + 0.02 * k as f64;
        let level = 120.0 - 15.0 * k as f64;
        for i in 0..=700 {
            let x = 1450.0 + 0.5 * f64::from(i);
            let t = (x - 1450.0) / 350.0;
            let background = level - tilt * (x - 1450.0) + 40.0 * (t - 0.5).powi(2) + 30.0 * (-t * 2.5).exp();
            let bands = 60.0 * share * pseudo_voigt(x, 1585.0, 12.0, 0.5)
                + 45.0 * (1.0 - share) * pseudo_voigt(x, 1604.0, 11.0, 0.5)
                + 35.0 * pseudo_voigt(x, 1655.0, 26.0, 0.3);
            shift.push(x);
            intensity.push(((background + bands + 0.8 * rng.normal()) * 100.0).round() / 100.0);
            sample.push(name.to_owned());
            mixture.push(share);
            known.push((background * 100.0).round() / 100.0);
        }
    }
    table(
        "Raman bands (invented)",
        vec![
            Column::text("Sample", sample),
            Column::numeric("Mixture fraction", mixture),
            Column::numeric("Raman shift (1/cm)", shift),
            Column::numeric("Intensity (counts)", intensity),
            Column::numeric("Background (true)", known),
        ],
    )
}

pub(super) fn raman_bands_sample() -> Sample {
    let t = raman_bands();
    let request = SpectrumRequest {
        curve: Some(col(&t, "Sample")),
        baseline: SpectrumBaseline::Estimated(BaselineMethod::Arpls { lambda: 1e6 }),
        detection: PeakDetection { min_separation: 8.0, ..PeakDetection::default() },
        fit: Some(PeakFitSettings {
            shape: PeakShape::PseudoVoigt,
            baseline: BaselineTerm::Constant,
            ..PeakFitSettings::default()
        }),
        ..SpectrumRequest::new(col(&t, "Raman shift (1/cm)"), col(&t, "Intensity (counts)"))
    };
    Sample { table: t, reports: vec![SavedReport::Signal { request: SignalRequest::Spectrum(request) }] }
}

/// Two seconds of a simulated accelerometer on a bearing housing, sampled
/// at 500 Hz: a 12 Hz shaft vibration with a 36 Hz harmonic, broadband noise
/// and a few short impacts.
pub fn machine_vibration() -> DataTable {
    let mut rng = Rng::new(0x0B1B_2A7E);
    let tau = std::f64::consts::TAU;
    let n = 1000;
    let mut time = Vec::with_capacity(n);
    let mut accel = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f64 / 500.0;
        let mut a = 2.0 * (tau * 12.0 * t).sin() + 0.6 * (tau * 36.0 * t + 0.8).sin() + 0.35 * rng.normal();
        // Impacts every 0.45 s, decaying over a few samples.
        let since = t.rem_euclid(0.45);
        if t > 0.2 && since < 0.012 {
            a += 4.0 * (-since / 0.004).exp();
        }
        time.push(t);
        accel.push((a * 1000.0).round() / 1000.0);
    }
    table(
        "Machine vibration (simulated)",
        vec![Column::numeric("Time (s)", time), Column::numeric("Acceleration (m/s²)", accel)],
    )
}

pub(super) fn machine_vibration_sample() -> Sample {
    let t = machine_vibration();
    let (x, y) = (col(&t, "Time (s)"), col(&t, "Acceleration (m/s²)"));
    let smooth = SignalSmoothingRequest {
        x: Some(x),
        method: Smoother::SavitzkyGolay { window: 9, order: 3, derivative: 0 },
        ..SignalSmoothingRequest::new(y)
    };
    let median = SignalSmoothingRequest { x: Some(x), method: Smoother::RunningMedian { window: 7 }, ..SignalSmoothingRequest::new(y) };
    let calculus = CalculusRequest { integral: Integration::Simpson, ..CalculusRequest::new(x, y) };
    Sample {
        table: t,
        reports: vec![
            SavedReport::Signal { request: SignalRequest::Smoothing(smooth) },
            SavedReport::Signal { request: SignalRequest::Smoothing(median) },
            SavedReport::Signal { request: SignalRequest::Calculus(calculus) },
        ],
    }
}

/// Ten seconds of a simulated ECG-like trace at 250 Hz: beats built from
/// P, Q, R, S and T waves (Gaussian bumps) at about 72 beats a minute with
/// slight rate variation, on a slow baseline wander, with 50 Hz mains hum
/// and noise. Not a recording.
pub fn ecg_like() -> DataTable {
    let mut rng = Rng::new(0x0EC6_1234);
    let tau = std::f64::consts::TAU;
    let n = 2500;
    // Beat times.
    let mut beats = Vec::new();
    let mut b = 0.35;
    while b < 10.5 {
        beats.push(b);
        b += 0.83 + 0.04 * rng.normal();
    }
    // (offset s, amplitude mV, width s) of each wave.
    let waves = [(-0.20, 0.15, 0.025), (-0.04, -0.12, 0.010), (0.0, 1.2, 0.012), (0.04, -0.25, 0.012), (0.28, 0.32, 0.045)];
    let mut time = Vec::with_capacity(n);
    let mut voltage = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f64 / 250.0;
        let mut v = 0.15 * (tau * 0.3 * t).sin() + 0.04 * (tau * 50.0 * t).sin() + 0.02 * rng.normal();
        for &beat in &beats {
            if (t - beat).abs() < 0.6 {
                for &(off, amp, w) in &waves {
                    v += amp * (-0.5 * ((t - beat - off) / w).powi(2)).exp();
                }
            }
        }
        time.push(t);
        voltage.push((v * 10_000.0).round() / 10_000.0);
    }
    table("ECG-like signal (simulated)", vec![Column::numeric("Time (s)", time), Column::numeric("Voltage (mV)", voltage)])
}

pub(super) fn ecg_like_sample() -> Sample {
    let t = ecg_like();
    let (x, y) = (col(&t, "Time (s)"), col(&t, "Voltage (mV)"));
    let smooth = SignalSmoothingRequest { x: Some(x), method: Smoother::MovingAverage { window: 5, kind: stat_signal::MovingAverage::Centred }, ..SignalSmoothingRequest::new(y) };
    let peaks = SpectrumRequest {
        baseline: SpectrumBaseline::Estimated(BaselineMethod::Snip { half_window: 40, decreasing: true, lls: false }),
        detection: PeakDetection { smoothing: Some((5, 2)), min_height: 10.0, min_prominence: 20.0, min_separation: 0.3 },
        ..SpectrumRequest::new(x, y)
    };
    Sample {
        table: t,
        reports: vec![
            SavedReport::Signal { request: SignalRequest::Smoothing(smooth) },
            SavedReport::Signal { request: SignalRequest::Spectrum(peaks) },
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signal_curves::SpectrumAnalysis;
    use stat_data::RowStateManager;

    #[test]
    fn raman_fit_recovers_the_three_bands() {
        let s = raman_bands_sample();
        let SavedReport::Signal { request: SignalRequest::Spectrum(r) } = &s.reports[0] else { panic!() };
        let a = SpectrumAnalysis::run(&s.table, &RowStateManager::new(s.table.n_rows()), r).unwrap();
        for (name, c) in a.curves() {
            let c = c.unwrap();
            assert_eq!(c.peaks.len(), 3, "{name}: {:?}", c.peaks.iter().map(|p| 1450.0 + 0.5 * p.index as f64).collect::<Vec<_>>());
            let f = c.fit.as_ref().unwrap().as_ref().unwrap();
            assert!(f.converged(), "{name}: {} after {}, {:?}", f.termination.label(), f.iterations, f.peaks.iter().map(|p| (p.centre.0, p.height.0, p.fwhm.0)).collect::<Vec<_>>());
            let mut centres: Vec<f64> = f.peaks.iter().map(|p| p.centre.0).collect();
            centres.sort_by(f64::total_cmp);
            for (got, want) in centres.iter().zip([1585.0, 1604.0, 1655.0]) {
                assert!((got - want).abs() < 1.5, "{name}: {centres:?}");
            }
        }
    }

    #[test]
    fn ecg_peaks_are_the_r_waves() {
        let s = ecg_like_sample();
        let SavedReport::Signal { request: SignalRequest::Spectrum(r) } = &s.reports[1] else { panic!() };
        let a = SpectrumAnalysis::run(&s.table, &RowStateManager::new(s.table.n_rows()), r).unwrap();
        let c = a.curves()[0].1.unwrap();
        let times: Vec<f64> = c.peaks.iter().map(|p| p.index as f64 / 250.0).collect();
        assert!((11..=13).contains(&times.len()), "{times:?}");
        assert!(times.windows(2).all(|w| (w[1] - w[0] - 0.83).abs() < 0.2), "{times:?}");
    }

    /// Rounding-level changes to the data (as between platforms' maths
    /// libraries) must not stall the fit: a Lorentzian share on its bound at
    /// 0 once made the derivative steps vanish.
    #[test]
    fn raman_fits_converge_under_rounding_changes() {
        let s = raman_bands_sample();
        let SavedReport::Signal { request: SignalRequest::Spectrum(r) } = &s.reports[0] else { panic!() };
        let yi = s.table.find("Intensity (counts)").unwrap();
        for seed in 1..=6u64 {
            let mut t = s.table.clone();
            let mut rng = Rng::new(seed);
            let y: Vec<f64> =
                t.column(yi).unwrap().as_numeric().unwrap().iter().map(|v| v * (1.0 + 4e-16 * rng.normal())).collect();
            t.replace_column(yi, Column::numeric("Intensity (counts)", y)).unwrap();
            let a = SpectrumAnalysis::run(&t, &RowStateManager::new(t.n_rows()), r).unwrap();
            for (name, c) in a.curves() {
                let f = c.unwrap().fit.as_ref().unwrap().as_ref().unwrap();
                assert!(f.converged(), "seed {seed} {name}: {} after {}", f.termination.label(), f.iterations);
            }
        }
    }
}
