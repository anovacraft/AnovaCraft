//! Signal processing on arrays of evenly spaced samples ([`stat_signal`]):
//! the Fourier transform and spectra, digital filters, convolution and
//! correlation, deconvolution, the Hilbert transform and wavelets. Signals
//! must be complete (no missing values). Options are maps whose members use
//! the same names as the signal reports' project-file settings; results are
//! maps of arrays.

use app_model::signal_analysis::{DesignedFilter, FilterSettings, WaveletSettings, window_from_json};
use rhai::{Array, Dynamic, Engine, FLOAT, INT, Map};
use stat_data::JsonValue;
use stat_signal::complex::Complex;
use stat_signal::convolve::{
    ConvolutionMethod, ConvolutionMode, Correlation, CorrelationScaling, autocorrelation, circular_convolve, convolve,
    cross_correlation, hilbert, richardson_lucy, wiener_deconvolve,
};
use stat_signal::spectrum::{Detrend, Scaling, SegmentOptions, Sides, SpectrumOptions, spectrogram, spectrum, welch};
use stat_signal::wavelet::{ContinuousWavelet, Decomposition, Extension, Wavelet, cwt, log_scales, wavedec, waverec};
use stat_signal::window::Window;

use crate::value::{ScriptResult, as_number, fail, script_err, to_json};

/// Register the functions.
pub(crate) fn register(engine: &mut Engine) {
    engine.register_fn("fft", |x: Array| transform(&x, None, false));
    engine.register_fn("fft", |re: Array, im: Array| transform(&re, Some(&im), false));
    engine.register_fn("ifft", |re: Array, im: Array| transform(&re, Some(&im), true));
    engine.register_fn("spectrum", |x: Array| power_spectrum(&x, &Map::new()));
    engine.register_fn("spectrum", |x: Array, o: Map| power_spectrum(&x, &o));
    engine.register_fn("welch_spectrum", |x: Array| segments(&x, &Map::new(), false));
    engine.register_fn("welch_spectrum", |x: Array, o: Map| segments(&x, &o, false));
    engine.register_fn("spectrogram", |x: Array| segments(&x, &Map::new(), true));
    engine.register_fn("spectrogram", |x: Array, o: Map| segments(&x, &o, true));
    engine.register_fn("filter_signal", |x: Array, o: Map| filter_signal(&x, &o));
    engine.register_fn("design_filter", |o: Map| design_filter(&o));
    engine.register_fn("convolve", |a: Array, b: Array| convolution(&a, &b, "full"));
    engine.register_fn("convolve", |a: Array, b: Array, mode: &str| convolution(&a, &b, mode));
    engine.register_fn("circular_convolve", |a: Array, b: Array| -> ScriptResult<Array> {
        Ok(numbers(&circular_convolve(&signal(&a, "first")?, &signal(&b, "second")?, None).map_err(script_err)?))
    });
    engine.register_fn("cross_correlation", |x: Array, y: Array| correlate(&x, Some(&y), &Map::new()));
    engine.register_fn("cross_correlation", |x: Array, y: Array, o: Map| correlate(&x, Some(&y), &o));
    engine.register_fn("autocorrelation", |x: Array| correlate(&x, None, &Map::new()));
    engine.register_fn("autocorrelation", |x: Array, o: Map| correlate(&x, None, &o));
    engine.register_fn("deconvolve", |y: Array, h: Array| deconvolve(&y, &h, &Map::new()));
    engine.register_fn("deconvolve", |y: Array, h: Array, o: Map| deconvolve(&y, &h, &o));
    engine.register_fn("hilbert", |x: Array| analytic(&x, &Map::new()));
    engine.register_fn("hilbert", |x: Array, o: Map| analytic(&x, &o));
    engine.register_fn("wavelet_decompose", |x: Array| decompose(&x, &Map::new()));
    engine.register_fn("wavelet_decompose", |x: Array, o: Map| decompose(&x, &o));
    engine.register_fn("wavelet_reconstruct", |d: Map| reconstruct(&d));
    engine.register_fn("wavelet_denoise", |x: Array| wavelet_denoise(&x, &Map::new()));
    engine.register_fn("wavelet_denoise", |x: Array, o: Map| wavelet_denoise(&x, &o));
    engine.register_fn("cwt", |x: Array| continuous(&x, &Map::new()));
    engine.register_fn("cwt", |x: Array, o: Map| continuous(&x, &o));
}

/// The samples of a signal (every value a number).
fn signal(a: &Array, what: &str) -> ScriptResult<Vec<f64>> {
    a.iter()
        .map(|v| match as_number(v) {
            Some(x) if x.is_finite() => Ok(x),
            _ => fail(format!("the {what} signal must be complete numbers (found {})", v.type_name())),
        })
        .collect()
}

/// Samples as floats (whole values too, so arithmetic on them stays in
/// floating point).
fn numbers(v: &[f64]) -> Array {
    v.iter().map(|&x| float(x)).collect()
}

fn float(x: f64) -> Dynamic {
    (x as FLOAT).into()
}

fn map<const N: usize>(entries: [(&str, Dynamic); N]) -> Map {
    entries.into_iter().map(|(k, v)| (k.into(), v)).collect()
}

fn array(v: &[f64]) -> Dynamic {
    Dynamic::from_array(numbers(v))
}

fn grid(rows: &[Vec<f64>]) -> Dynamic {
    Dynamic::from_array(rows.iter().map(|r| array(r)).collect())
}

/// Options as JSON (for the settings readers shared with the reports).
fn json(o: &Map) -> ScriptResult<JsonValue> {
    to_json(&Dynamic::from_map(o.clone()))
}

fn opt_number(o: &Map, key: &str, default: f64) -> ScriptResult<f64> {
    match o.get(key) {
        None => Ok(default),
        Some(v) => match as_number(v) {
            Some(x) if x.is_finite() => Ok(x),
            _ => fail(format!("`{key}` must be a number")),
        },
    }
}

fn opt_count(o: &Map, key: &str) -> ScriptResult<Option<usize>> {
    match o.get(key) {
        None => Ok(None),
        Some(v) => match v.as_int() {
            Ok(i) if i > 0 => Ok(Some(i as usize)),
            _ => fail(format!("`{key}` must be a positive whole number")),
        },
    }
}

fn opt_text(o: &Map, key: &str, default: &str) -> String {
    o.get(key).map_or_else(|| default.to_owned(), |v| v.to_string())
}

fn rate(o: &Map) -> ScriptResult<f64> {
    let r = opt_number(o, "sampling_rate", 1.0)?;
    if r > 0.0 { Ok(r) } else { fail("`sampling_rate` must be positive") }
}

fn window(o: &Map, default: Window) -> ScriptResult<Window> {
    window_from_json(&json(o)?, "window", default).map_err(script_err)
}

fn detrend(o: &Map) -> ScriptResult<Detrend> {
    match opt_text(o, "detrend", "mean").as_str() {
        "none" => Ok(Detrend::None),
        "mean" => Ok(Detrend::Constant),
        "linear" => Ok(Detrend::Linear),
        other => fail(format!("unknown `detrend` \"{other}\" (use \"none\", \"mean\" or \"linear\")")),
    }
}

fn sides_scaling(o: &Map, default: Scaling) -> ScriptResult<(Sides, Scaling)> {
    let sides = match opt_text(o, "sides", "one").as_str() {
        "one" => Sides::One,
        "two" => Sides::Two,
        other => return fail(format!("unknown `sides` \"{other}\" (use \"one\" or \"two\")")),
    };
    let scaling = match o.get("scaling").map(ToString::to_string).as_deref() {
        None => default,
        Some("density") => Scaling::Density,
        Some("spectrum") => Scaling::Spectrum,
        Some(other) => return fail(format!("unknown `scaling` \"{other}\" (use \"density\" or \"spectrum\")")),
    };
    Ok((sides, scaling))
}

fn transform(re: &Array, im: Option<&Array>, inverse: bool) -> ScriptResult<Map> {
    let re = signal(re, "real")?;
    let im = match im {
        Some(a) => signal(a, "imaginary")?,
        None => vec![0.0; re.len()],
    };
    if re.len() != im.len() {
        return fail("the real and imaginary parts must have the same length");
    }
    if re.is_empty() {
        return fail("the signal is empty");
    }
    let z: Vec<Complex> = re.iter().zip(&im).map(|(&a, &b)| Complex::new(a, b)).collect();
    let out = if inverse { stat_signal::fft::ifft(&z) } else { stat_signal::fft::fft(&z) };
    let part = |f: fn(&Complex) -> f64| array(&out.iter().map(f).collect::<Vec<_>>());
    Ok(map([
        ("re", part(|c| c.re)),
        ("im", part(|c| c.im)),
        ("amplitude", part(|c| c.abs())),
        ("phase", part(|c| c.arg())),
    ]))
}

fn power_spectrum(x: &Array, o: &Map) -> ScriptResult<Map> {
    let x = signal(x, "input")?;
    let (sides, scaling) = sides_scaling(o, Scaling::Density)?;
    let opts = SpectrumOptions {
        sampling_rate: rate(o)?,
        window: window(o, Window::Rectangular)?,
        detrend: detrend(o)?,
        fft_length: opt_count(o, "fft_length")?,
        sides,
        scaling,
    };
    let s = spectrum(&x, &opts).map_err(script_err)?;
    Ok(map([
        ("frequency", array(&s.frequency)),
        ("amplitude", array(&s.amplitude)),
        ("phase", array(&s.phase)),
        ("power", array(&s.power)),
        ("resolution", float(s.resolution)),
    ]))
}

fn segments(x: &Array, o: &Map, time: bool) -> ScriptResult<Map> {
    let x = signal(x, "input")?;
    let base = if time { SegmentOptions::spectrogram() } else { SegmentOptions::welch() };
    let (sides, scaling) = sides_scaling(o, Scaling::Density)?;
    let opts = SegmentOptions {
        sampling_rate: rate(o)?,
        window: window(o, base.window)?,
        segment_length: opt_count(o, "segment_length")?.unwrap_or(base.segment_length).min(x.len()),
        overlap: opt_count(o, "overlap")?,
        fft_length: opt_count(o, "fft_length")?,
        detrend: detrend(o)?,
        sides,
        scaling,
    };
    if time {
        let s = spectrogram(&x, &opts).map_err(script_err)?;
        Ok(map([("frequency", array(&s.frequency)), ("time", array(&s.time)), ("power", grid(&s.power))]))
    } else {
        let s = welch(&x, &opts).map_err(script_err)?;
        Ok(map([
            ("frequency", array(&s.frequency)),
            ("power", array(&s.power)),
            ("segments", (s.segments as INT).into()),
        ]))
    }
}

fn filter_settings(o: &Map) -> ScriptResult<FilterSettings> {
    let mut v = json(o)?;
    // `cutoff` is a friendlier name for the one edge of a low- or high-pass filter.
    if let (JsonValue::Object(m), Some(c)) = (&mut v, o.get("cutoff").and_then(as_number)) {
        if !m.iter().any(|(k, _)| k == "low") {
            m.push(("low".to_owned(), JsonValue::number(c)));
        }
    }
    FilterSettings::from_json(&v).map_err(script_err)
}

fn filter_signal(x: &Array, o: &Map) -> ScriptResult<Array> {
    let x = signal(x, "input")?;
    let settings = filter_settings(o)?;
    let fs = rate(o)?;
    let f = DesignedFilter::design(&settings, fs).map_err(script_err)?;
    Ok(numbers(&f.apply(&x, settings.zero_phase, fs).map_err(script_err)?))
}

fn design_filter(o: &Map) -> ScriptResult<Map> {
    let settings = filter_settings(o)?;
    let fs = rate(o)?;
    let f = DesignedFilter::design(&settings, fs).map_err(script_err)?;
    let points = opt_count(o, "points")?.unwrap_or(512);
    let r = f.response(points, fs);
    let mut m = map([
        ("frequency", array(&r.frequency)),
        ("gain", array(&r.magnitude)),
        ("gain_db", array(&r.magnitude_db)),
        ("phase", array(&r.phase)),
    ]);
    match f {
        DesignedFilter::Sections(sos) => {
            m.insert("sections".into(), grid(&sos.sections.iter().map(|s| s.to_vec()).collect::<Vec<_>>()));
        }
        DesignedFilter::Taps(b) => {
            m.insert("taps".into(), array(&b));
        }
        DesignedFilter::Fft(..) => {}
    }
    Ok(m)
}

fn convolution(a: &Array, b: &Array, mode: &str) -> ScriptResult<Array> {
    let mode = match mode {
        "full" => ConvolutionMode::Full,
        "same" => ConvolutionMode::Same,
        "valid" => ConvolutionMode::Valid,
        other => return fail(format!("unknown mode \"{other}\" (use \"full\", \"same\" or \"valid\")")),
    };
    let out = convolve(&signal(a, "first")?, &signal(b, "second")?, mode, ConvolutionMethod::Auto);
    Ok(numbers(&out.map_err(script_err)?))
}

fn correlation_map(r: &Correlation) -> Map {
    map([
        ("lag", Dynamic::from_array(r.lag.iter().map(|&l| (l as INT).into()).collect())),
        ("value", array(&r.value)),
        ("lag_of_max", (r.lag_of_max as INT).into()),
        ("max_value", float(r.max_value)),
    ])
}

fn correlate(x: &Array, y: Option<&Array>, o: &Map) -> ScriptResult<Map> {
    let x = signal(x, "first")?;
    let scaling = match opt_text(o, "scaling", "coefficient").as_str() {
        "none" => CorrelationScaling::None,
        "biased" => CorrelationScaling::Biased,
        "unbiased" => CorrelationScaling::Unbiased,
        "coefficient" => CorrelationScaling::Coefficient,
        other => {
            return fail(format!(
                "unknown `scaling` \"{other}\" (use \"coefficient\", \"biased\", \"unbiased\" or \"none\")"
            ));
        }
    };
    let max_lag = opt_count(o, "max_lag")?;
    let r = match y {
        Some(y) => cross_correlation(&x, &signal(y, "second")?, scaling, max_lag),
        None => autocorrelation(&x, scaling, max_lag),
    };
    Ok(correlation_map(&r.map_err(script_err)?))
}

fn deconvolve(y: &Array, h: &Array, o: &Map) -> ScriptResult<Array> {
    let (y, h) = (signal(y, "observed")?, signal(h, "kernel")?);
    let out = match opt_text(o, "method", "wiener").as_str() {
        "wiener" => wiener_deconvolve(&y, &h, opt_number(o, "noise_ratio", 0.01)?),
        "richardson-lucy" => richardson_lucy(&y, &h, opt_count(o, "iterations")?.unwrap_or(30)),
        other => return fail(format!("unknown `method` \"{other}\" (use \"wiener\" or \"richardson-lucy\")")),
    };
    Ok(numbers(&out.map_err(script_err)?))
}

fn analytic(x: &Array, o: &Map) -> ScriptResult<Map> {
    let a = hilbert(&signal(x, "input")?, rate(o)?).map_err(script_err)?;
    Ok(map([
        ("envelope", array(&a.envelope)),
        ("phase", array(&a.phase)),
        ("frequency", array(&a.frequency)),
        ("imaginary", array(&a.analytic.iter().map(|c| c.im).collect::<Vec<_>>())),
    ]))
}

fn extension(o: &Map) -> ScriptResult<Extension> {
    match opt_text(o, "extension", "symmetric").as_str() {
        "symmetric" => Ok(Extension::Symmetric),
        "periodic" => Ok(Extension::Periodic),
        "zero" => Ok(Extension::Zero),
        other => fail(format!("unknown `extension` \"{other}\" (use \"symmetric\", \"periodic\" or \"zero\")")),
    }
}

fn decompose(x: &Array, o: &Map) -> ScriptResult<Map> {
    let x = signal(x, "input")?;
    let name = opt_text(o, "wavelet", "db4");
    let w = Wavelet::new(&name).map_err(script_err)?;
    let ext = extension(o)?;
    let d = wavedec(&x, &w, ext, opt_count(o, "levels")?).map_err(script_err)?;
    Ok(map([
        ("wavelet", name.into()),
        ("extension", opt_text(o, "extension", "symmetric").into()),
        ("approximation", array(&d.approximation)),
        ("details", grid(&d.details)),
        ("length", (d.signal_length as INT).into()),
    ]))
}

fn reconstruct(d: &Map) -> ScriptResult<Array> {
    let w = Wavelet::new(&opt_text(d, "wavelet", "db4")).map_err(script_err)?;
    let ext = extension(d)?;
    let arr = |key: &str| -> ScriptResult<Array> {
        d.get(key).and_then(|v| v.clone().try_cast::<Array>()).ok_or_else(|| format!("the map needs `{key}`").into())
    };
    let approximation = signal(&arr("approximation")?, "approximation")?;
    let details = arr("details")?
        .iter()
        .map(|v| {
            v.clone()
                .try_cast::<Array>()
                .ok_or_else(|| "`details` must be arrays".into())
                .and_then(|a| signal(&a, "detail"))
        })
        .collect::<ScriptResult<Vec<_>>>()?;
    let length = match d.get("length").map(Dynamic::as_int) {
        Some(Ok(n)) if n > 0 => n as usize,
        _ => usize::MAX,
    };
    let dec = Decomposition { approximation, details, signal_length: length };
    Ok(numbers(&waverec(&dec, &w, ext).map_err(script_err)?))
}

fn wavelet_denoise(x: &Array, o: &Map) -> ScriptResult<Map> {
    let x = signal(x, "input")?;
    let s = WaveletSettings::from_json(&json(o)?).map_err(script_err)?;
    let w = Wavelet::new(&s.wavelet).map_err(script_err)?;
    let opts = stat_signal::wavelet::DenoiseOptions {
        level: s.levels,
        extension: s.extension,
        rule: s.rule,
        kind: s.kind,
        noise: s.noise,
    };
    let d = stat_signal::wavelet::denoise(&x, &w, &opts).map_err(script_err)?;
    Ok(map([
        ("signal", array(&d.signal)),
        ("thresholds", array(&d.thresholds)),
        ("noise_sd", array(&d.noise_sd)),
        ("share_zeroed", float(d.share_zeroed)),
    ]))
}

fn continuous(x: &Array, o: &Map) -> ScriptResult<Map> {
    let x = signal(x, "input")?;
    let wavelet = match opt_text(o, "wavelet", "morlet").as_str() {
        "morlet" => ContinuousWavelet::Morlet,
        "mexican-hat" => ContinuousWavelet::MexicanHat,
        other => return fail(format!("unknown `wavelet` \"{other}\" (use \"morlet\" or \"mexican-hat\")")),
    };
    let scales: Vec<f64> = match o.get("scales") {
        Some(v) if v.is_array() => signal(&v.clone().cast::<Array>(), "scales")?,
        _ => {
            let count = opt_count(o, "scales")?.unwrap_or(32);
            log_scales(1.0, (x.len() as f64 / 4.0).max(2.0), count)
        }
    };
    let s = cwt(&x, &scales, wavelet, 1.0 / rate(o)?).map_err(script_err)?;
    Ok(map([("scales", array(&s.scales)), ("frequency", array(&s.frequency)), ("coefficients", grid(&s.coefficients))]))
}

#[cfg(test)]
mod tests {
    use crate::{RunOptions, run_script};
    use std::cell::RefCell;
    use std::rc::Rc;

    fn run(source: &str) -> Vec<String> {
        let lines = Rc::new(RefCell::new(Vec::new()));
        let sink = Rc::clone(&lines);
        run_script(source, &RunOptions::default(), move |s| sink.borrow_mut().push(s.to_owned())).unwrap();
        lines.take()
    }

    /// The functions call the engine (whose values are checked against
    /// NumPy, SciPy and PyWavelets in `stat-signal`); these check the
    /// options and result maps.
    #[test]
    fn signal_functions_return_maps() {
        let out = run(r#"
            let x = [];
            for i in 0..128 { x.push(sin(2.0 * PI() * 8.0 * i / 128.0) + 0.5 * sin(2.0 * PI() * 40.0 * i / 128.0)); }
            let f = fft(x);
            print(f.re.len()); print(round(f.amplitude[8]));
            let back = ifft(f.re, f.im);
            print(round(back.re[5] * 1e6) == round(x[5] * 1e6));
            let s = spectrum(x, #{ sampling_rate: 128.0, scaling: "spectrum" });
            print(s.frequency.len()); print(s.frequency[8]);
            print(welch_spectrum(x, #{ segment_length: 32 }).segments);
            print(spectrogram(x, #{ segment_length: 32, overlap: 16 }).time.len());
            let low = filter_signal(x, #{ band: "lowpass", cutoff: 20.0, sampling_rate: 128.0, order: 6 });
            print(abs(low[64] - sin(2.0 * PI() * 8.0 * 64 / 128.0)) < 0.02);
            let d = design_filter(#{ design: "fir", taps: 11, low: 0.2 });
            print(d.taps.len());
            print(design_filter(#{ design: "elliptic", order: 4, low: 0.1, ripple_db: 0.5, attenuation_db: 50.0 }).sections.len());
            print(convolve([1, 2, 3], [0, 1, 0.5]));
            print(convolve([1, 2, 3], [0, 1, 0.5], "same"));
            print(round(circular_convolve([1, 2, 3], [0, 1, 0])[1]));
            print(cross_correlation([0, 0, 1, 0, 0], [0, 1, 0, 0, 0]).lag_of_max);
            print(autocorrelation(x, #{ max_lag: 20 }).value.len());
            print(deconvolve([1, 3, 3, 1], [1, 1], #{ noise_ratio: 0.0 }).len());
            print(round(hilbert(x).envelope[64] * 10.0) >= 5.0);
            let w = wavelet_decompose(x, #{ wavelet: "haar", levels: 3 });
            print(w.details.len());
            let r = wavelet_reconstruct(w);
            print(round(r[17] * 1e9) == round(x[17] * 1e9));
            print(wavelet_denoise(x, #{ wavelet: "sym4", shrinkage: "hard" }).signal.len());
            let c = cwt(x, #{ scales: [2.0, 4.0, 8.0], sampling_rate: 128.0 });
            print(c.coefficients.len()); print(c.coefficients[0].len());
        "#);
        let want = [
            "128",
            "64.0",
            "true",
            "65",
            "8.0",
            "7",
            "7",
            "true",
            "11",
            "2",
            "[0.0, 1.0, 2.5, 4.0, 1.5]",
            "[1.0, 2.5, 4.0]",
            "1.0",
            "1",
            "21",
            "4",
            "true",
            "3",
            "true",
            "128",
            "3",
            "128",
        ];
        assert_eq!(out, want);
    }
}
