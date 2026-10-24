//! Periodogram and smoothed spectral density compared with R 4.3
//! `spec.pgram(fast = FALSE)` (`tests/data/spectral_reference.R` writes
//! `spectral_reference.json`): frequencies, estimates, equivalent degrees of
//! freedom and bandwidth, for raw and modified-Daniell-smoothed estimates
//! with and without tapering and detrending.

use stat_core::distributions::{ChiSquared, ContinuousDistribution};
use stat_data::{JsonValue, parse_json};
use stat_timeseries::spectral::{Detrend, SpectrumOptions, spectral_density};

fn num(v: &JsonValue) -> f64 {
    v.as_f64().unwrap()
}

fn nums(v: &JsonValue, key: &str) -> Vec<f64> {
    v.get(key).unwrap_or_else(|| panic!("missing {key}")).as_array().unwrap().iter().map(num).collect()
}

fn field(v: &JsonValue, key: &str) -> f64 {
    num(v.get(key).unwrap_or_else(|| panic!("missing {key}")))
}

fn close(got: f64, want: f64, rel: f64, what: &str) {
    let scale = want.abs().max(1e-300);
    assert!((got - want).abs() <= rel * scale, "{what}: got {got}, reference {want}");
}

fn close_all(got: &[f64], want: &[f64], rel: f64, what: &str) {
    assert_eq!(got.len(), want.len(), "{what} length");
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        close(*g, *w, rel, &format!("{what}[{i}]"));
    }
}

fn data() -> JsonValue {
    parse_json(include_str!("data/spectral_reference.json")).unwrap()
}

#[test]
fn spectra_match_spec_pgram() {
    let all = data();
    let cases = all.get("cases").unwrap().as_array().unwrap();
    assert_eq!(cases.len(), 15);
    for c in cases {
        let name = c.get("series").unwrap().as_str().unwrap();
        let y = nums(all.get("series").unwrap(), name);
        let detrend = match c.get("detrend").unwrap().as_str().unwrap() {
            "linear" => Detrend::Linear,
            "mean" => Detrend::Mean,
            _ => Detrend::None,
        };
        let spans: Vec<usize> = nums(c, "spans").iter().map(|&s| s as usize).collect();
        let options = SpectrumOptions { detrend, taper: field(c, "taper"), spans: spans.clone(), level: 0.95 };
        let s = spectral_density(&y, &options).unwrap();
        let what = format!("{name} {detrend:?} taper {} spans {spans:?}", options.taper);
        close_all(&s.frequency, &nums(c, "freq"), 1e-13, &format!("{what} freq"));
        // R's FFT and the direct transform differ in rounding only.
        close_all(&s.density, &nums(c, "spec"), 1e-9, &format!("{what} spec"));
        close(s.df, field(c, "df"), 1e-12, &format!("{what} df"));
        close(s.bandwidth, field(c, "bandwidth"), 1e-12, &format!("{what} bandwidth"));
        // Limits from the chi-square approximation.
        let chi = ChiSquared::new(s.df).unwrap();
        let (hi, lo) = (chi.quantile(0.975).unwrap(), chi.quantile(0.025).unwrap());
        for i in 0..s.density.len() {
            close(s.lower[i], s.df * s.density[i] / hi, 1e-12, &what);
            close(s.upper[i], s.df * s.density[i] / lo, 1e-12, &what);
            assert!(s.lower[i] < s.density[i] && s.density[i] < s.upper[i]);
        }
    }
    let chisq = all.get("chisq").unwrap();
    let chi = ChiSquared::new(field(chisq, "df")).unwrap();
    close(chi.quantile(0.975).unwrap(), field(chisq, "q975"), 1e-10, "chi-square 0.975 quantile");
    close(chi.quantile(0.025).unwrap(), field(chisq, "q025"), 1e-10, "chi-square 0.025 quantile");
}

#[test]
fn monthly_units_follow_documented_conversion() {
    // R reports a monthly series in cycles per year: frequencies and
    // bandwidth times 12, density divided by 12.
    let all = data();
    let y = nums(all.get("series").unwrap(), "ldeaths");
    let m = all.get("ldeaths_monthly").unwrap();
    let s = spectral_density(&y, &SpectrumOptions { spans: vec![3, 3], ..SpectrumOptions::default() }).unwrap();
    let freq: Vec<f64> = s.frequency.iter().map(|f| f * 12.0).collect();
    let dens: Vec<f64> = s.density.iter().map(|d| d / 12.0).collect();
    close_all(&freq, &nums(m, "freq"), 1e-13, "monthly freq");
    close_all(&dens, &nums(m, "spec"), 1e-9, "monthly spec");
    close(s.df, field(m, "df"), 1e-12, "monthly df");
    close(s.bandwidth * 12.0, field(m, "bandwidth"), 1e-12, "monthly bandwidth");
}
