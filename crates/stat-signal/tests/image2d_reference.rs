//! `stat_signal::image2d` compared with NumPy, SciPy and scikit-image used
//! as black boxes; see `tests/data/image2d_reference_cases.py`, which writes
//! `image2d_reference_cases.json` (values to 14 significant digits), and
//! `tests/data/image2d_profile_reference.py`, which writes
//! `image2d_profile_reference.json` for the radial profiles.
//!
//! Errors are measured relative to the largest magnitude in each reference
//! array. Tolerances are 1e-10 unless noted; Mac and Linux floating point
//! may differ by an ulp.

use stat_data::{JsonValue, parse_json};
use stat_signal::complex::Complex;
use stat_signal::filter::Band;
use stat_signal::image2d::convolve::{ConvolutionMethod, ConvolutionMode};
use stat_signal::image2d::{
    ComplexGrid, FilterShape, FrequencyFilter, Grid, Padding, Regularizer, SpectrumOptions2, Taper2, autocorrelation2,
    convolve2, correlate2, directional_profile, dominant_frequencies, fft_shift2, fft2, fourier_shift, frequency_filter, ifft_shift2, ifft2,
    irfft2, phase_correlation, power_law_fit, radial_spectrum, rfft2, richardson_lucy2, spectrum2, taper2, wiener2,
};
use stat_signal::window::Window;
use std::f64::consts::PI;

fn data() -> &'static JsonValue {
    use std::sync::OnceLock;
    static DATA: OnceLock<JsonValue> = OnceLock::new();
    DATA.get_or_init(|| parse_json(include_str!("data/image2d_reference_cases.json")).unwrap())
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

fn int(v: &JsonValue, k: &str) -> usize {
    num(v, k) as usize
}

fn text<'a>(v: &'a JsonValue, k: &str) -> &'a str {
    key(v, k).as_str().unwrap()
}

fn cvec(v: &JsonValue, k: &str) -> Vec<Complex> {
    let c = key(v, k);
    vec(c, "re").into_iter().zip(vec(c, "im")).map(|(re, im)| Complex::new(re, im)).collect()
}

fn image(v: &JsonValue, k: &str) -> Grid {
    let g = key(v, k);
    Grid::new(int(g, "rows"), int(g, "cols"), vec(g, "data")).unwrap()
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
        "hann" => Window::Hann,
        "hamming" => Window::Hamming,
        "tukey" => Window::Tukey { alpha: 0.3 },
        other => panic!("unknown window {other}"),
    }
}

#[test]
fn fft2_and_ifft2_match_numpy() {
    for case in data().get("fft2").unwrap().as_array().unwrap() {
        let (rows, cols) = (int(case, "rows"), int(case, "cols"));
        let x = ComplexGrid::new(rows, cols, cvec(case, "x")).unwrap();
        cclose(&fft2(&x).data, &cvec(case, "fft2"), 1e-12, &format!("fft2 {rows}×{cols}"));
        cclose(&ifft2(&x).data, &cvec(case, "ifft2"), 1e-12, &format!("ifft2 {rows}×{cols}"));
    }
}

#[test]
fn real_transforms_match_numpy() {
    for case in data().get("rfft2").unwrap().as_array().unwrap() {
        let x = image(case, "x");
        let what = format!("{}×{}", x.rows, x.cols);
        cclose(&rfft2(&x).data, &cvec(case, "rfft2"), 1e-12, &format!("rfft2 {what}"));
        let noisy = ComplexGrid::new(x.rows, x.cols / 2 + 1, cvec(case, "noisy_half")).unwrap();
        close(&irfft2(&noisy, x.cols).data, &vec(case, "irfft2"), 1e-12, &format!("irfft2 {what}"));
    }
    let s = key(data(), "shift");
    let x = image(s, "x");
    close(&fft_shift2(&x.data, x.rows, x.cols), &vec(s, "fftshift"), 0.0, "fftshift");
    close(&ifft_shift2(&x.data, x.rows, x.cols), &vec(s, "ifftshift"), 0.0, "ifftshift");
}

#[test]
fn windows_match_scipy_and_scikit_image() {
    for case in data().get("windows").unwrap().as_array().unwrap() {
        let (rows, cols) = (int(case, "rows"), int(case, "cols"));
        let name = text(case, "window");
        let w = match (text(case, "kind"), name) {
            ("separable", n) => Taper2::Separable(window(n)),
            ("radial", "tukey") => Taper2::Radial(Window::Tukey { alpha: 0.5 }),
            (_, n) => Taper2::Radial(window(n)),
        };
        // scikit-image builds the radial coordinates in float32.
        let tol = if text(case, "kind") == "radial" { 2e-6 } else { 1e-13 };
        close(&taper2(w, rows, cols).unwrap().data, &vec(case, "w"), tol, &format!("{} {rows}×{cols}", w.label()));
    }
}

#[test]
fn centred_spectra_match_numpy_composition() {
    let s = key(data(), "spectrum");
    let im = image(s, "image").with_spacing(num(s, "dx"), num(s, "dy")).unwrap();
    for case in arr(s, "cases") {
        let (pr, pc) = (int(case, "pad_rows"), int(case, "pad_cols"));
        let opts = SpectrumOptions2 {
            remove_mean: true,
            taper: if text(case, "taper") == "hann" { Taper2::Separable(Window::Hann) } else { Taper2::None },
            padding: if (pr, pc) == (im.rows, im.cols) { Padding::None } else { Padding::Zero { rows: pr, cols: pc } },
        };
        let sp = spectrum2(&im, &opts).unwrap();
        let what = format!("{} {pr}×{pc}", text(case, "taper"));
        close(&sp.power.data, &vec(case, "power"), 1e-11, &format!("power {what}"));
        close(&sp.magnitude.data, &vec(case, "magnitude"), 1e-11, &format!("magnitude {what}"));
        close(&sp.fx, &vec(case, "fx"), 1e-14, "fx");
        close(&sp.fy, &vec(case, "fy"), 1e-14, "fy");
    }
}

#[test]
fn butterworth_and_gaussian_filters_match_scikit_image_and_scipy() {
    let f = key(data(), "filters");
    let im = image(f, "image");
    for case in arr(f, "butterworth") {
        let fc = num(case, "cutoff");
        let band = if key(case, "high_pass").as_bool().unwrap() { Band::Highpass(fc) } else { Band::Lowpass(fc) };
        let filter = FrequencyFilter {
            band: Some((band, FilterShape::Butterworth { order: num(case, "order") })),
            ..Default::default()
        };
        let out = frequency_filter(&im, &filter).unwrap();
        close(&out.filtered.data, &vec(case, "out"), 1e-11, &format!("butterworth {fc}"));
    }
    for case in arr(f, "gaussian") {
        let sigma = num(case, "sigma");
        let filter = FrequencyFilter {
            band: Some((Band::Lowpass(1.0 / (2.0 * PI * sigma)), FilterShape::Gaussian)),
            ..Default::default()
        };
        let out = frequency_filter(&im, &filter).unwrap();
        close(&out.filtered.data, &vec(case, "out"), 1e-11, &format!("gaussian σ = {sigma}"));
    }
}

#[test]
fn convolution_and_correlation_match_scipy() {
    let c = key(data(), "convolve");
    let a = image(c, "a");
    for case in arr(c, "cases") {
        let b = image(case, "b");
        let mode = match text(case, "mode") {
            "full" => ConvolutionMode::Full,
            "same" => ConvolutionMode::Same,
            _ => ConvolutionMode::Valid,
        };
        for method in [ConvolutionMethod::Direct, ConvolutionMethod::Fft] {
            let what = format!("{}×{} {mode:?} {method:?}", b.rows, b.cols);
            let want = image(case, "convolve");
            let got = convolve2(&a, &b, mode, method).unwrap();
            assert_eq!((got.rows, got.cols), (want.rows, want.cols), "{what}");
            close(&got.data, &want.data, 1e-12, &format!("convolve {what}"));
            let want = image(case, "correlate");
            let got = correlate2(&a, &b, mode, method).unwrap();
            assert_eq!((got.rows, got.cols), (want.rows, want.cols), "{what}");
            close(&got.data, &want.data, 1e-12, &format!("correlate {what}"));
        }
    }
    let ac = autocorrelation2(&image(c, "autocorr_image"), true, true).unwrap();
    close(&ac.value.data, &image(c, "autocorr").data, 1e-12, "autocorrelation");
}

#[test]
fn phase_correlation_matches_scikit_image() {
    let r = key(data(), "registration");
    let reference = image(r, "reference");
    let moving = image(r, "moving");
    let applied = vec(r, "shift_applied");
    let shifted = fourier_shift(&reference, applied[0], applied[1]).unwrap();
    close(&shifted.data, &vec(r, "fourier_shift"), 1e-12, "fourier_shift");
    for case in arr(r, "cases") {
        let up = int(case, "upsample");
        let want = vec(case, "shift");
        let got = phase_correlation(&reference, &moving, up).unwrap();
        assert!((got.row_shift - want[0]).abs() < 1e-9, "upsample {up}: rows {} vs {}", got.row_shift, want[0]);
        assert!((got.col_shift - want[1]).abs() < 1e-9, "upsample {up}: cols {} vs {}", got.col_shift, want[1]);
        assert!(got.peak > 0.0 && got.peak <= 1.0 + 1e-12);
    }
}

#[test]
fn deconvolution_matches_scikit_image() {
    let d = key(data(), "deconvolution");
    let im = image(d, "image");
    let psf = image(d, "psf");
    for case in arr(d, "wiener") {
        let b = num(case, "balance");
        let got = wiener2(&im, &psf, Regularizer::Laplacian(b)).unwrap();
        close(&got.data, &vec(case, "out"), 1e-10, &format!("wiener balance {b}"));
    }
    for case in arr(d, "richardson_lucy") {
        let n = int(case, "iterations");
        let got = richardson_lucy2(&im, &psf, n).unwrap();
        close(&got.data, &vec(case, "out"), 1e-9, &format!("richardson-lucy {n}"));
    }
}

/// A random field with power exactly `ρ^{−β}` (unit-modulus random phases,
/// conjugate-symmetric so the image is real); the fitted slope must recover β.
#[test]
fn radial_power_law_recovers_a_known_exponent() {
    let n = 128;
    let mut state = 987_654_321u64;
    let mut next = || {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (state >> 11) as f64 / (1u64 << 53) as f64
    };
    for beta in [1.0, 2.0, 3.0] {
        let mut spec = vec![Complex::ZERO; n * n];
        let idx = |k: usize| if k <= n / 2 { k as f64 } else { k as f64 - n as f64 };
        for k in 0..n {
            for l in 0..n {
                let (ck, cl) = ((n - k) % n, (n - l) % n);
                if (ck, cl) < (k, l) {
                    continue;
                }
                let rho = idx(k).hypot(idx(l)) / n as f64;
                if rho == 0.0 {
                    continue;
                }
                let amp = rho.powf(-beta / 2.0);
                let z =
                    if (ck, cl) == (k, l) { Complex::real(amp) } else { Complex::from_polar(amp, 2.0 * PI * next()) };
                spec[k * n + l] = z;
                spec[ck * n + cl] = z.conj();
            }
        }
        let field = ifft2(&ComplexGrid::new(n, n, spec).unwrap()).real();
        let sp = spectrum2(&field, &SpectrumOptions2::default()).unwrap();
        let fit = power_law_fit(&radial_spectrum(&sp), Some(4.0 / n as f64), None).unwrap();
        assert!((fit.exponent - beta).abs() < 0.03, "β = {beta}: fitted {}", fit.exponent);
        assert!(fit.r_squared > 0.999);
    }
}

#[test]
fn dominant_frequencies_find_two_tool_mark_directions() {
    let (rows, cols) = (96, 128);
    let g = Grid::from_fn(rows, cols, |r, c| {
        let (x, y) = (c as f64, r as f64);
        // Period 8 px across (direction 0°), and a pattern with 12 cycles across and 6 down
        // whose wave vector points up and to the right on screen.
        (2.0 * PI * x / 8.0).cos() + 0.5 * (2.0 * PI * (12.0 * x / 128.0 - 6.0 * y / 96.0)).cos()
    });
    let sp = spectrum2(&g, &SpectrumOptions2::default()).unwrap();
    let peaks = dominant_frequencies(&sp, 2, 0.0);
    assert!((peaks[0].period - 8.0).abs() < 1e-9 && peaks[0].direction.abs() < 1e-9);
    assert!((peaks[0].amplitude - 1.0).abs() < 1e-9);
    let p = peaks[1];
    assert!((p.fx - 12.0 / 128.0).abs() < 1e-12 && (p.fy + 6.0 / 96.0).abs() < 1e-12, "{p:?}");
    assert!((p.direction - (6.0f64 / 96.0).atan2(12.0 / 128.0).to_degrees()).abs() < 1e-9);
    assert!((p.period - 1.0 / (12.0f64 / 128.0).hypot(6.0 / 96.0)).abs() < 1e-9);
    assert!((p.amplitude - 0.5).abs() < 1e-9);
}

#[test]
fn radial_profiles_match_scipy_interpolation_and_numpy_wedges() {
    let d = parse_json(include_str!("data/image2d_profile_reference.json")).unwrap();
    let g = key(&d, "image");
    let grid = Grid::new(int(g, "rows"), int(g, "cols"), vec(g, "data"))
        .unwrap()
        .with_spacing(num(g, "dx"), num(g, "dy"))
        .unwrap();
    let s = spectrum2(&grid, &SpectrumOptions2::default()).unwrap();
    for case in arr(&d, "cases") {
        let (direction, half) = (num(case, "direction"), num(case, "half_width"));
        let p = directional_profile(&s, direction, half).unwrap();
        let (f, power) = (vec(case, "frequency"), vec(case, "power"));
        assert_eq!(p.frequency.len(), f.len(), "{direction}° ±{half}");
        let scale = power.iter().fold(0.0f64, |m, v| m.max(v.abs()));
        for k in 0..f.len() {
            assert!((p.frequency[k] - f[k]).abs() < 1e-12, "{direction}° ±{half}: frequency {k}");
            assert!((p.power[k] - power[k]).abs() <= 1e-10 * scale, "{direction}° ±{half}: power {k}");
        }
        if half > 0.0 {
            let n: Vec<usize> = vec(case, "count").into_iter().map(|v| v as usize).collect();
            assert_eq!(p.count, n, "{direction}° ±{half}");
        }
    }
}
