//! Every routine compared with NumPy, SciPy, PyWavelets and scikit-image
//! used as black boxes; see `tests/data/reference_cases.py`, which writes
//! `reference_cases.json` (values to 14 significant digits).
//!
//! Errors are measured relative to the largest magnitude in each reference
//! vector, so near-zero entries (spectral nulls, stop bands) are judged on
//! the scale of the whole result. Tolerances are 1e-10 unless noted; Mac and
//! Linux floating point may differ by an ulp.

use stat_data::{JsonValue, parse_json};
use stat_signal::complex::Complex;
use stat_signal::convolve::{
    ConvolutionMethod, ConvolutionMode, CorrelationScaling, circular_convolve, convolve, cross_correlation, hilbert,
    richardson_lucy, wiener_deconvolve,
};
use stat_signal::fft::{fft, irfft, rfft};
use stat_signal::filter::{
    Band, IirFamily, design_fir, design_iir, fir_filter, fir_filtfilt, fir_frequency_response, notch,
};
use stat_signal::spectrum::{Detrend, Scaling, SegmentOptions, Sides, SpectrumOptions, spectrogram, spectrum, welch};
use stat_signal::wavelet::{
    ContinuousWavelet, DenoiseOptions, Extension, NoiseEstimate, ThresholdKind, ThresholdRule, Wavelet, cwt, denoise,
    dwt, max_level, wavedec,
};
use stat_signal::window::Window;

fn data() -> &'static JsonValue {
    use std::sync::OnceLock;
    static DATA: OnceLock<JsonValue> = OnceLock::new();
    DATA.get_or_init(|| parse_json(include_str!("data/reference_cases.json")).unwrap())
}

fn key<'a>(v: &'a JsonValue, k: &str) -> &'a JsonValue {
    v.get(k).unwrap_or_else(|| panic!("missing {k}"))
}

fn arr<'a>(v: &'a JsonValue, k: &str) -> &'a [JsonValue] {
    key(v, k).as_array().unwrap()
}

fn vec(v: &JsonValue, k: &str) -> Vec<f64> {
    arr(v, k).iter().map(|x| x.as_f64().unwrap()).collect()
}

fn num(v: &JsonValue, k: &str) -> f64 {
    key(v, k).as_f64().unwrap()
}

fn text<'a>(v: &'a JsonValue, k: &str) -> &'a str {
    key(v, k).as_str().unwrap()
}

fn cvec(v: &JsonValue, k: &str) -> Vec<Complex> {
    let c = key(v, k);
    vec(c, "re").into_iter().zip(vec(c, "im")).map(|(re, im)| Complex::new(re, im)).collect()
}

fn close(got: &[f64], want: &[f64], tol: f64, what: &str) {
    assert_eq!(got.len(), want.len(), "{what}: length");
    let scale = want.iter().fold(0.0f64, |m, v| m.max(v.abs())).max(1e-300);
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        assert!((g - w).abs() <= tol * scale, "{what}[{i}]: got {g}, want {w} (scale {scale})");
    }
}

fn cclose(got: &[Complex], want: &[Complex], tol: f64, what: &str) {
    assert_eq!(got.len(), want.len(), "{what}: length");
    let scale = want.iter().fold(0.0f64, |m, v| m.max(v.abs())).max(1e-300);
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        assert!((*g - *w).abs() <= tol * scale, "{what}[{i}]: got {g:?}, want {w:?}");
    }
}

fn window(name: &str) -> Window {
    match name {
        "rectangular" => Window::Rectangular,
        "hann" => Window::Hann,
        "hamming" => Window::Hamming,
        "blackman" => Window::Blackman,
        "flattop" => Window::FlatTop,
        "kaiser" => Window::Kaiser { beta: 8.6 },
        "tukey" => Window::Tukey { alpha: 0.3 },
        other => panic!("window {other}"),
    }
}

fn signal() -> (Vec<f64>, f64) {
    let s = key(data(), "signal");
    (vec(s, "x"), num(s, "fs"))
}

#[test]
fn fft_matches_numpy() {
    for case in data().get("fft").unwrap().as_array().unwrap() {
        let n = num(case, "n") as usize;
        let x = cvec(case, "x");
        cclose(&fft(&x), &cvec(case, "fft"), 1e-12, &format!("fft n = {n}"));
    }
    let r = key(data(), "rfft");
    let x = vec(r, "x");
    cclose(&rfft(&x, num(r, "n") as usize), &cvec(r, "rfft"), 1e-12, "rfft");
    let back = irfft(&rfft(&x, x.len()), num(r, "irfft_n") as usize);
    close(&back, &vec(r, "irfft"), 1e-12, "irfft");
}

#[test]
fn windows_match_scipy() {
    for case in arr(data(), "windows") {
        let name = text(case, "window");
        let m = num(case, "m") as usize;
        let w = window(name);
        close(&w.periodic(m).unwrap(), &vec(case, "periodic"), 1e-12, &format!("{name} periodic {m}"));
        close(&w.symmetric(m).unwrap(), &vec(case, "symmetric"), 1e-12, &format!("{name} symmetric {m}"));
    }
}

fn detrend_of(s: &str) -> Detrend {
    match s {
        "constant" => Detrend::Constant,
        "linear" => Detrend::Linear,
        _ => Detrend::None,
    }
}

fn scaling_of(s: &str) -> Scaling {
    if s == "density" { Scaling::Density } else { Scaling::Spectrum }
}

fn sides_of(v: &JsonValue) -> Sides {
    if key(v, "onesided").as_bool().unwrap() { Sides::One } else { Sides::Two }
}

#[test]
fn periodogram_matches_scipy() {
    let (x, fs) = signal();
    for case in arr(data(), "periodogram") {
        let nfft = num(case, "nfft") as usize;
        let opts = SpectrumOptions {
            sampling_rate: fs,
            window: window(text(case, "window")),
            detrend: detrend_of(text(case, "detrend")),
            fft_length: (nfft > 0).then_some(nfft),
            sides: sides_of(case),
            scaling: scaling_of(text(case, "scaling")),
        };
        let s = spectrum(&x, &opts).unwrap();
        let what = format!("periodogram {opts:?}");
        close(&s.frequency, &vec(case, "f"), 1e-13, &what);
        close(&s.power, &vec(case, "p"), 1e-10, &what);
    }
}

#[test]
fn amplitude_and_phase_match_numpy() {
    let (x, fs) = signal();
    let case = key(data(), "amplitude");
    let opts = SpectrumOptions {
        sampling_rate: fs,
        window: Window::Hann,
        detrend: Detrend::Linear,
        fft_length: Some(400),
        sides: Sides::One,
        scaling: Scaling::Spectrum,
    };
    let s = spectrum(&x, &opts).unwrap();
    close(&s.amplitude, &vec(case, "amplitude"), 1e-10, "amplitude");
    let want = vec(case, "phase");
    // Compare phase only where the amplitude is not negligible.
    for (i, (&p, &w)) in s.phase.iter().zip(&want).enumerate() {
        if s.amplitude[i] > 1e-8 {
            let d = (p - w + std::f64::consts::PI).rem_euclid(2.0 * std::f64::consts::PI) - std::f64::consts::PI;
            assert!(d.abs() < 1e-8, "phase[{i}]");
        }
    }
}

#[test]
fn welch_matches_scipy() {
    let (x, fs) = signal();
    for case in arr(data(), "welch") {
        let wname = text(case, "window");
        let w = if wname == "tukey" { Window::Tukey { alpha: 0.25 } } else { window(wname) };
        let overlap = num(case, "noverlap");
        let nfft = num(case, "nfft") as usize;
        let opts = SegmentOptions {
            sampling_rate: fs,
            window: w,
            segment_length: num(case, "nperseg") as usize,
            overlap: (overlap >= 0.0).then_some(overlap as usize),
            fft_length: (nfft > 0).then_some(nfft),
            detrend: detrend_of(text(case, "detrend")),
            sides: sides_of(case),
            scaling: scaling_of(text(case, "scaling")),
        };
        let r = welch(&x, &opts).unwrap();
        let what = format!("welch {opts:?}");
        close(&r.frequency, &vec(case, "f"), 1e-13, &what);
        close(&r.power, &vec(case, "p"), 1e-10, &what);
    }
}

#[test]
fn spectrogram_matches_scipy() {
    let (x, fs) = signal();
    let case = key(data(), "spectrogram");
    let opts =
        SegmentOptions { sampling_rate: fs, segment_length: 40, overlap: Some(10), ..SegmentOptions::spectrogram() };
    let s = spectrogram(&x, &opts).unwrap();
    close(&s.frequency, &vec(case, "f"), 1e-13, "spectrogram f");
    close(&s.time, &vec(case, "t"), 1e-13, "spectrogram t");
    let want = arr(case, "S");
    assert_eq!(s.power.len(), want.len());
    for (j, col) in want.iter().enumerate() {
        let w: Vec<f64> = col.as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
        close(&s.power[j], &w, 1e-10, &format!("spectrogram column {j}"));
    }
    let case = key(data(), "spectrogram2");
    let opts = SegmentOptions {
        sampling_rate: fs,
        window: Window::Hann,
        segment_length: 64,
        overlap: None,
        fft_length: Some(128),
        scaling: Scaling::Spectrum,
        ..SegmentOptions::spectrogram()
    };
    let s = spectrogram(&x, &opts).unwrap();
    close(&s.time, &vec(case, "t"), 1e-13, "spectrogram2 t");
    for (j, col) in arr(case, "S").iter().enumerate() {
        let w: Vec<f64> = col.as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
        close(&s.power[j], &w, 1e-10, &format!("spectrogram2 column {j}"));
    }
}

fn band_of(case: &JsonValue, key_name: &str, btype: &str) -> Band {
    let wn = vec(case, key_name);
    match btype {
        "lowpass" => Band::Lowpass(wn[0]),
        "highpass" => Band::Highpass(wn[0]),
        "bandpass" => Band::Bandpass(wn[0], wn[1]),
        "bandstop" => Band::Bandstop(wn[0], wn[1]),
        other => panic!("{other}"),
    }
}

#[test]
fn iir_designs_match_scipy() {
    let (x, fs) = signal();
    for case in arr(data(), "iir") {
        let fam = match text(case, "family") {
            "butter" => IirFamily::Butterworth,
            "cheby1" => IirFamily::ChebyshevI { ripple_db: num(case, "rp") },
            "cheby2" => IirFamily::ChebyshevII { attenuation_db: num(case, "rs") },
            "ellip" => IirFamily::Elliptic { ripple_db: num(case, "rp"), attenuation_db: num(case, "rs") },
            other => panic!("{other}"),
        };
        let order = num(case, "order") as usize;
        let btype = text(case, "btype");
        let sos = design_iir(fam, order, band_of(case, "wn", btype), fs).unwrap();
        let what = format!("{fam:?} order {order} {btype}");
        let r = sos.frequency_response(64, fs);
        close(&r.frequency, &vec(case, "freq"), 1e-13, &what);
        cclose(&r.response, &cvec(case, "h"), 1e-9, &format!("{what} response"));
        close(&sos.filter(&x), &vec(case, "forward"), 1e-9, &format!("{what} forward"));
        close(&sos.filtfilt(&x).unwrap(), &vec(case, "filtfilt"), 1e-9, &format!("{what} filtfilt"));
    }
}

#[test]
fn fir_designs_match_scipy() {
    let (x, fs) = signal();
    for case in arr(data(), "fir") {
        let taps = num(case, "taps") as usize;
        let btype = text(case, "btype");
        let w = match text(case, "window") {
            "kaiser" => Window::Kaiser { beta: 6.0 },
            other => window(other),
        };
        let b = design_fir(taps, band_of(case, "cutoff", btype), w, fs).unwrap();
        let what = format!("FIR {taps} {btype}");
        close(&b, &vec(case, "b"), 1e-12, &what);
        close(&fir_filter(&b, &x), &vec(case, "forward"), 1e-11, &format!("{what} forward"));
        close(&fir_filtfilt(&b, &x).unwrap(), &vec(case, "filtfilt"), 1e-11, &format!("{what} filtfilt"));
        cclose(&fir_frequency_response(&b, 64, fs).response, &cvec(case, "h"), 1e-11, &format!("{what} response"));
    }
}

#[test]
fn notch_matches_scipy() {
    let (x, fs) = signal();
    let case = key(data(), "notch");
    let sos = notch(num(case, "f0"), num(case, "q"), fs).unwrap();
    let (b, a) = sos.to_transfer_function();
    close(&b, &vec(case, "b"), 1e-13, "notch b");
    close(&a, &vec(case, "a"), 1e-13, "notch a");
    // SciPy's filtfilt pads 3·max(len(a), len(b)) = 9 samples, as sosfiltfilt
    // does for one section.
    close(&sos.filtfilt(&x).unwrap(), &vec(case, "filtfilt"), 1e-10, "notch filtfilt");
}

#[test]
fn convolution_matches_scipy() {
    let c = key(data(), "convolution");
    let a = vec(c, "a");
    let k1 = vec(c, "k1");
    let k2 = vec(c, "k2");
    for (mode, name) in
        [(ConvolutionMode::Full, "full"), (ConvolutionMode::Same, "same"), (ConvolutionMode::Valid, "valid")]
    {
        for method in [ConvolutionMethod::Direct, ConvolutionMethod::Fft] {
            close(
                &convolve(&a, &k1, mode, method).unwrap(),
                &vec(c, &format!("k1_{name}")),
                1e-12,
                &format!("k1 {name} {method:?}"),
            );
            close(
                &convolve(&a, &k2, mode, method).unwrap(),
                &vec(c, &format!("k2_{name}")),
                1e-12,
                &format!("k2 {name} {method:?}"),
            );
        }
    }
    close(&circular_convolve(&a, &k1, None).unwrap(), &vec(c, "circular"), 1e-12, "circular");
}

#[test]
fn correlation_matches_scipy() {
    let c = key(data(), "correlation");
    let x = vec(c, "x");
    let y = vec(c, "y");
    let raw = cross_correlation(&x, &y, CorrelationScaling::None, None).unwrap();
    let lags: Vec<f64> = raw.lag.iter().map(|&l| l as f64).collect();
    close(&lags, &vec(c, "lags"), 0.0, "lags");
    close(&raw.value, &vec(c, "raw"), 1e-12, "raw correlation");
    let coef = cross_correlation(&x, &y, CorrelationScaling::Coefficient, None).unwrap();
    close(&coef.value, &vec(c, "coef"), 1e-12, "coefficient");
    assert_eq!(coef.lag_of_max as f64, num(c, "lag_of_max"));
}

#[test]
fn hilbert_matches_scipy() {
    let (_, fs) = signal();
    let h = key(data(), "hilbert");
    let a = hilbert(&vec(h, "x"), fs).unwrap();
    cclose(&a.analytic, &cvec(h, "analytic"), 1e-12, "analytic");
    close(&a.envelope, &vec(h, "envelope"), 1e-12, "envelope");
    close(&a.phase, &vec(h, "phase"), 1e-10, "phase");
    close(&a.frequency, &vec(h, "frequency"), 1e-9, "instantaneous frequency");
    let h = key(data(), "hilbert_odd");
    cclose(&hilbert(&vec(h, "x"), fs).unwrap().analytic, &cvec(h, "analytic"), 1e-12, "analytic (odd length)");
}

#[test]
fn deconvolution_matches_references() {
    let d = key(data(), "deconvolution");
    let y = vec(d, "blurred");
    let psf = vec(d, "psf");
    let rl = richardson_lucy(&y, &psf, num(d, "iterations") as usize).unwrap();
    close(&rl, &vec(d, "richardson_lucy"), 1e-9, "Richardson–Lucy");
    let w = wiener_deconvolve(&y, &psf, num(d, "lambda")).unwrap();
    close(&w, &vec(d, "wiener"), 1e-11, "Wiener");
}

fn extension_of(s: &str) -> Extension {
    match s {
        "symmetric" => Extension::Symmetric,
        "periodization" => Extension::Periodic,
        "zero" => Extension::Zero,
        other => panic!("{other}"),
    }
}

#[test]
fn wavelet_transforms_match_pywavelets() {
    let x = vec(data(), "wavelet_x");
    for case in arr(data(), "wavelets") {
        let name = text(case, "name");
        let mode = extension_of(text(case, "mode"));
        let w = Wavelet::new(name).unwrap();
        let what = format!("{name} {mode:?}");
        close(w.dec_lo(), &vec(case, "dec_lo"), 1e-14, &format!("{what} dec_lo"));
        close(w.dec_hi(), &vec(case, "dec_hi"), 1e-14, &format!("{what} dec_hi"));
        close(w.rec_hi(), &vec(case, "rec_hi"), 1e-14, &format!("{what} rec_hi"));
        assert_eq!(max_level(x.len(), &w) as f64, num(case, "max_level"), "{what} max level");
        let lv = dwt(&x, &w, mode).unwrap();
        close(&lv.approximation, &vec(case, "cA"), 1e-12, &format!("{what} cA"));
        close(&lv.detail, &vec(case, "cD"), 1e-12, &format!("{what} cD"));
        let want = arr(case, "wavedec");
        if want.is_empty() {
            continue;
        }
        let dec = wavedec(&x, &w, mode, Some(num(case, "level") as usize)).unwrap();
        let mut got = vec![dec.approximation.clone()];
        got.extend(dec.details.iter().cloned());
        assert_eq!(got.len(), want.len());
        for (i, (g, wv)) in got.iter().zip(want).enumerate() {
            let wv: Vec<f64> = wv.as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
            close(g, &wv, 1e-12, &format!("{what} wavedec part {i}"));
        }
    }
}

#[test]
fn wavelet_denoising_matches_reference() {
    let x = vec(data(), "denoise_x");
    for case in arr(data(), "denoise") {
        let name = text(case, "name");
        let opts = DenoiseOptions {
            level: None,
            extension: Extension::Symmetric,
            rule: if text(case, "rule") == "sure" { ThresholdRule::Sure } else { ThresholdRule::Universal },
            kind: if text(case, "kind") == "soft" { ThresholdKind::Soft } else { ThresholdKind::Hard },
            noise: if text(case, "noise") == "each" { NoiseEstimate::EachLevel } else { NoiseEstimate::FinestLevel },
        };
        let out = denoise(&x, &Wavelet::new(name).unwrap(), &opts).unwrap();
        let what = format!("denoise {name} {opts:?}");
        close(&out.thresholds, &vec(case, "thresholds"), 1e-10, &what);
        close(&out.signal, &vec(case, "signal"), 1e-10, &what);
    }
}

#[test]
fn continuous_transform_matches_pywavelets() {
    let (_, fs) = signal();
    let c = key(data(), "cwt");
    let x = vec(c, "x");
    let scales = vec(c, "scales");
    for case in arr(c, "cases") {
        let wv =
            if text(case, "wavelet") == "morlet" { ContinuousWavelet::Morlet } else { ContinuousWavelet::MexicanHat };
        let s = cwt(&x, &scales, wv, 1.0 / fs).unwrap();
        for (i, row) in arr(case, "coef").iter().enumerate() {
            let want: Vec<f64> = row.as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
            close(&s.coefficients[i], &want, 1e-10, &format!("{wv:?} scale {}", scales[i]));
        }
    }
}
