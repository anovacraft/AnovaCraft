//! Number formatting for report tables.

/// Format `v` with about `sig` significant digits, switching to scientific
/// notation for very large or very small magnitudes. Trailing zeros are trimmed.
#[must_use]
pub fn format_number(v: f64, sig: usize) -> String {
    if v.is_nan() {
        return "—".to_owned();
    }
    if v.is_infinite() {
        return if v > 0.0 { "∞".to_owned() } else { "−∞".to_owned() };
    }
    if v == 0.0 {
        return "0".to_owned();
    }
    let sig = sig.max(1);
    let magnitude = v.abs().log10().floor() as i32;
    if !(-4..9).contains(&magnitude) {
        return format!("{:.*e}", sig - 1, v);
    }
    let decimals = (sig as i32 - 1 - magnitude).max(0) as usize;
    trim_zeros(format!("{v:.decimals$}"))
}

/// Format a p-value: four decimals down to 0.0001, then two significant digits
/// in scientific notation (so very small p-values remain informative).
#[must_use]
pub fn format_p_value(p: f64) -> String {
    if p.is_nan() {
        "—".to_owned()
    } else if p == 0.0 {
        "0".to_owned()
    } else if p >= 1e-4 {
        format!("{p:.4}")
    } else {
        format!("{p:.1e}")
    }
}

/// Format an optional value, using "—" for `None`.
#[must_use]
pub fn format_optional(v: Option<f64>, sig: usize) -> String {
    v.map_or_else(|| "—".to_owned(), |x| format_number(x, sig))
}

fn trim_zeros(s: String) -> String {
    if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_owned() } else { s }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers() {
        assert_eq!(format_number(1.234_567_8, 4), "1.235");
        assert_eq!(format_number(1234.5, 6), "1234.5");
        assert_eq!(format_number(-0.000_123_456, 3), "-0.000123");
        assert_eq!(format_number(2.0, 6), "2");
        assert_eq!(format_number(1.5e-7, 3), "1.50e-7");
        assert_eq!(format_number(-3_482_258.634_6, 6), "-3482259");
        assert_eq!(format_number(-3_482_258.634_6, 9), "-3482258.63");
        assert_eq!(format_number(4.2e12, 3), "4.20e12");
        assert_eq!(format_number(0.0, 6), "0");
        assert_eq!(format_number(f64::NAN, 6), "—");
    }

    #[test]
    fn p_values() {
        assert_eq!(format_p_value(0.031_234), "0.0312");
        assert_eq!(format_p_value(0.000_1), "0.0001");
        assert_eq!(format_p_value(3.2e-15), "3.2e-15");
        assert_eq!(format_optional(None, 4), "—");
    }
}
