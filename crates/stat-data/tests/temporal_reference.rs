//! Date and time arithmetic checked against Python's `datetime` module and
//! `openpyxl` (see `tests/data/temporal_reference.py`, which generated the
//! reference file).

use stat_data::temporal::{TemporalDisplay, TemporalFormat, detect_column, from_spreadsheet_serial};

const REFERENCE: &str = include_str!("data/temporal_reference.csv");

fn rows() -> impl Iterator<Item = (&'static str, &'static str, i128)> {
    REFERENCE.lines().skip(1).map(|line| {
        let mut parts = line.split(',');
        let kind = parts.next().unwrap();
        let text = parts.next().unwrap();
        let expected = parts.next().unwrap().parse().unwrap();
        (kind, text, expected)
    })
}

fn micros(seconds: f64) -> i128 {
    (seconds * 1e6).round() as i128
}

/// `seconds` equals the reference to the microsecond, or to within two units
/// in the last place where an `f64` cannot hold microseconds (far from 1970:
/// about 15 µs per ulp in the year 4700).
fn close(seconds: f64, expected_micros: i128) -> bool {
    let ulp = seconds.abs() * f64::EPSILON;
    (seconds - expected_micros as f64 / 1e6).abs() <= (2.0 * ulp).max(1e-6)
}

#[test]
fn matches_python_datetime_and_openpyxl() {
    let mut counts = std::collections::BTreeMap::new();
    let date = TemporalDisplay::new(TemporalFormat::Date);
    let instant = TemporalDisplay { format: TemporalFormat::DateTime, fraction_digits: 6, offset_minutes: 0 };
    for (kind, text, expected) in rows() {
        *counts.entry(kind).or_insert(0) += 1;
        match kind {
            "date" => {
                let col = detect_column(&[text]).unwrap_or_else(|| panic!("{text} not read"));
                assert_eq!(col.display.format, TemporalFormat::Date);
                assert_eq!(micros(col.values[0]), expected, "{text}");
                assert_eq!(date.text(col.values[0]), text, "formatting");
            }
            "instant" | "named" => {
                let col = detect_column(&[text]).unwrap_or_else(|| panic!("{text} not read"));
                assert_eq!(col.display.format, TemporalFormat::DateTime);
                assert!(close(col.values[0], expected), "{text}");
                assert_eq!(micros(instant.parse(text).unwrap()), micros(col.values[0]));
                // Written back at the value's own offset, the text reads the same.
                let shown = TemporalDisplay { offset_minutes: col.display.offset_minutes, ..instant };
                let back = detect_column(&[shown.iso_text(col.values[0])]).unwrap();
                assert!(close(back.values[0], expected), "{text}");
            }
            "serial1900" | "serial1904" => {
                let serial: f64 = text.parse().unwrap();
                let seconds = from_spreadsheet_serial(serial, kind == "serial1904").unwrap();
                assert!(close(seconds, expected), "{kind} {text}");
            }
            other => panic!("unknown kind {other}"),
        }
    }
    assert!(counts["date"] >= 20_000 && counts["instant"] >= 4_900 && counts["named"] >= 800, "{counts:?}");
    assert!(counts["serial1900"] >= 2_000 && counts["serial1904"] >= 2_000, "{counts:?}");
}
