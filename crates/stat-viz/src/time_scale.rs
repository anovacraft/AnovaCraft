//! Axes for date-time columns: tick steps on calendar units, tick positions on
//! calendar boundaries, and labels that name each tick by the units that
//! change between ticks.
//!
//! The data stay in seconds (see `stat_data::temporal`); only where ticks fall
//! and how they read differ from a numeric axis. Steps come from a ladder of
//! calendar units rather than 1, 2, 2.5, 5 × 10ⁿ (Wilkinson, *The Grammar of
//! Graphics*, 2nd ed., 2005, on time scales): 1, 2, 5, 10, 15, 30 seconds and
//! minutes; 1, 2, 3, 6, 12 hours; 1 and 2 days; 1 week (starting Monday, as in
//! ISO 8601); 1, 2, 3, 6 months; then 1, 2, 5 × 10ⁿ years. Month and year
//! ticks fall on the first day of the month or year in the column's display
//! offset, so they are unevenly spaced in seconds. Durations and times of day
//! use the fixed-length units only (no calendar), and steps below a second are
//! 1, 2, 5 × 10ⁿ seconds.

use stat_data::temporal::{SECONDS_PER_DAY, TemporalDisplay, TemporalFormat, civil_from_days, days_from_civil};

/// Calendar unit of a tick step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TimeUnit {
    /// Fractions of a second.
    SubSecond,
    /// Seconds.
    Second,
    /// Minutes.
    Minute,
    /// Hours.
    Hour,
    /// Days.
    Day,
    /// Weeks starting on Monday.
    Week,
    /// Calendar months.
    Month,
    /// Calendar years.
    Year,
}

/// A tick step: `count` of `unit` (for [`TimeUnit::SubSecond`], `seconds` is
/// the step and `count` is 0).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimeStep {
    /// Unit.
    pub unit: TimeUnit,
    /// Number of units per step.
    pub count: i64,
    /// Approximate length in seconds (months as 30.436875 days, years as
    /// 365.2425 days, the Gregorian means).
    pub seconds: f64,
}

const MONTH_SECONDS: f64 = 30.436_875 * 86_400.0;
const YEAR_SECONDS: f64 = 365.242_5 * 86_400.0;
/// Most ticks ever produced for one axis.
const MAX_TICKS: usize = 2000;

fn unit_seconds(unit: TimeUnit) -> f64 {
    match unit {
        TimeUnit::SubSecond | TimeUnit::Second => 1.0,
        TimeUnit::Minute => 60.0,
        TimeUnit::Hour => 3600.0,
        TimeUnit::Day => 86_400.0,
        TimeUnit::Week => 604_800.0,
        TimeUnit::Month => MONTH_SECONDS,
        TimeUnit::Year => YEAR_SECONDS,
    }
}

fn step(unit: TimeUnit, count: i64) -> TimeStep {
    TimeStep { unit, count, seconds: unit_seconds(unit) * count as f64 }
}

/// The smallest 1, 2 or 5 × 10ⁿ that is at least `raw` (> 0).
fn nice_125(raw: f64) -> f64 {
    let magnitude = 10f64.powf(raw.log10().floor());
    let m = raw / magnitude;
    let nice = if m <= 1.0 + 1e-9 {
        1.0
    } else if m <= 2.0 + 1e-9 {
        2.0
    } else if m <= 5.0 + 1e-9 {
        5.0
    } else {
        10.0
    };
    nice * magnitude
}

/// The smallest step on the ladder for `format` that is at least
/// `raw_seconds` long.
#[must_use]
pub fn time_step(raw_seconds: f64, format: TemporalFormat) -> TimeStep {
    use TimeUnit::{Day, Hour, Minute, Month, Second, Week, Year};
    let raw = if raw_seconds > 0.0 && raw_seconds.is_finite() { raw_seconds } else { 1.0 };
    if raw < 1.0 {
        let s = nice_125(raw);
        return if s >= 1.0 { step(Second, 1) } else { TimeStep { unit: TimeUnit::SubSecond, count: 0, seconds: s } };
    }
    let fixed: &[(TimeUnit, i64)] = &[
        (Second, 1),
        (Second, 2),
        (Second, 5),
        (Second, 10),
        (Second, 15),
        (Second, 30),
        (Minute, 1),
        (Minute, 2),
        (Minute, 5),
        (Minute, 10),
        (Minute, 15),
        (Minute, 30),
        (Hour, 1),
        (Hour, 2),
        (Hour, 3),
        (Hour, 6),
        (Hour, 12),
        (Day, 1),
        (Day, 2),
    ];
    if let Some(&(u, n)) = fixed.iter().find(|&&(u, n)| unit_seconds(u) * n as f64 >= raw * (1.0 - 1e-9)) {
        return step(u, n);
    }
    match format {
        TemporalFormat::Duration | TemporalFormat::TimeOfDay => {
            // Days counted 1, 2, 5 × 10ⁿ.
            let days = nice_125(raw / 86_400.0).max(1.0);
            step(Day, days.round() as i64)
        }
        TemporalFormat::Date | TemporalFormat::DateTime => {
            for (u, n) in [(Week, 1), (Month, 1), (Month, 2), (Month, 3), (Month, 6)] {
                if unit_seconds(u) * n as f64 >= raw * (1.0 - 1e-9) {
                    return step(u, n);
                }
            }
            let years = nice_125(raw / YEAR_SECONDS).max(1.0);
            step(Year, years.round() as i64)
        }
    }
}

/// Offset applied for display: durations have none.
fn offset_seconds(display: &TemporalDisplay) -> f64 {
    if display.format == TemporalFormat::Duration { 0.0 } else { f64::from(display.offset_minutes) * 60.0 }
}

/// Tick positions (seconds, increasing) of `step` within `[lo, hi]` on an axis
/// shown with `display`. Fixed-length steps are aligned to the epoch at the
/// display offset (weeks to Mondays); month and year steps fall on the first
/// day of every `count`-th month or year (months counted from January, years
/// from year 0).
#[must_use]
pub fn time_ticks(lo: f64, hi: f64, step: TimeStep, display: &TemporalDisplay) -> Vec<f64> {
    let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
    if !(lo.is_finite() && hi.is_finite()) || step.seconds <= 0.0 || (hi - lo) / step.seconds > MAX_TICKS as f64 {
        return Vec::new();
    }
    let shift = offset_seconds(display);
    let (llo, lhi) = (lo + shift, hi + shift);
    let mut ticks = Vec::new();
    match step.unit {
        TimeUnit::Month | TimeUnit::Year => {
            let months = if step.unit == TimeUnit::Year { 12 * step.count } else { step.count };
            let (y, m, _) = civil_from_days((llo / SECONDS_PER_DAY as f64).floor() as i64);
            let mut index = (y * 12 + i64::from(m) - 1).div_euclid(months) * months;
            loop {
                let (year, month) = (index.div_euclid(12), u32::try_from(index.rem_euclid(12) + 1).unwrap_or(1));
                let t = (days_from_civil(year, month, 1) * SECONDS_PER_DAY) as f64;
                if t > lhi || ticks.len() > MAX_TICKS {
                    break;
                }
                if t >= llo {
                    ticks.push(t - shift);
                }
                index += months;
            }
        }
        _ => {
            let size = step.seconds;
            // 1970-01-05 was a Monday.
            let origin = if step.unit == TimeUnit::Week { 4.0 * 86_400.0 } else { 0.0 };
            let first = ((llo - origin) / size).ceil() as i64;
            let last = ((lhi - origin) / size).floor() as i64;
            for k in first..=last {
                ticks.push(origin + k as f64 * size - shift);
            }
        }
    }
    ticks
}

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// Decimals needed to write multiples of `step` seconds (at most 6).
fn decimals(step: f64) -> usize {
    (0..=6)
        .find(|&d| {
            let scaled = step * 10f64.powi(d as i32);
            (scaled - scaled.round()).abs() < 1e-6
        })
        .unwrap_or(6)
}

/// `hh:mm`, `hh:mm:ss` or `hh:mm:ss.f` for `seconds_of_day`, as `step` needs.
fn clock(seconds_of_day: f64, step: TimeStep) -> String {
    let whole = seconds_of_day.floor();
    let s = whole as i64;
    let (h, m, sec) = (s / 3600, s / 60 % 60, s % 60);
    match step.unit {
        TimeUnit::SubSecond => {
            let d = decimals(step.seconds);
            let frac = format!("{:.*}", d, seconds_of_day - whole);
            format!("{h:02}:{m:02}:{sec:02}{}", frac.trim_start_matches('0'))
        }
        TimeUnit::Second => format!("{h:02}:{m:02}:{sec:02}"),
        _ => format!("{h:02}:{m:02}"),
    }
}

/// Labels for `ticks` (from [`time_ticks`]) on an axis shown with `display`.
///
/// * Dates: years `2026`; months `Oct 2026`; days and weeks `13 Oct`, with the
///   year added to the first label and wherever the year changes; hours and
///   finer `15:00` (or `15:57:30`, `15:57:30.5`), with the day added to the
///   first label and wherever the day changes (`13 Oct 15:00`).
/// * Times of day: `15:00`, `15:57:30`.
/// * Durations: `2 d` for day steps, otherwise hours and minutes since zero
///   (`36:00`, `1:30:15`), with a leading `-` below zero.
#[must_use]
pub fn time_labels(ticks: &[f64], step: TimeStep, display: &TemporalDisplay) -> Vec<String> {
    let shift = offset_seconds(display);
    let mut previous: Option<(i64, i64)> = None; // (year, day) of the last label
    ticks
        .iter()
        .map(|&t| {
            // Round to the microsecond so 23:59:59.9999999 reads as midnight.
            let local = ((t + shift) * 1e6).round() / 1e6;
            match display.format {
                TemporalFormat::Duration => duration_label(local, step),
                TemporalFormat::TimeOfDay => clock(local.rem_euclid(86_400.0), step),
                TemporalFormat::Date | TemporalFormat::DateTime => {
                    let days = (local / SECONDS_PER_DAY as f64).floor() as i64;
                    let (y, m, d) = civil_from_days(days);
                    let month = MONTHS[(m as usize).saturating_sub(1).min(11)];
                    let new_year = previous.is_none_or(|(py, _)| py != y);
                    let new_day = previous.is_none_or(|(_, pd)| pd != days);
                    previous = Some((y, days));
                    match step.unit {
                        TimeUnit::Year => y.to_string(),
                        TimeUnit::Month => format!("{month} {y}"),
                        TimeUnit::Week | TimeUnit::Day if new_year => format!("{d} {month} {y}"),
                        TimeUnit::Week | TimeUnit::Day => format!("{d} {month}"),
                        _ => {
                            let time = clock(local - (days * SECONDS_PER_DAY) as f64, step);
                            if new_day { format!("{d} {month} {time}") } else { time }
                        }
                    }
                }
            }
        })
        .collect()
}

fn duration_label(seconds: f64, step: TimeStep) -> String {
    let sign = if seconds < 0.0 { "-" } else { "" };
    let a = seconds.abs();
    match step.unit {
        TimeUnit::Day | TimeUnit::Week | TimeUnit::Month | TimeUnit::Year => {
            format!("{sign}{:.*} d", decimals(step.seconds / 86_400.0), a / 86_400.0)
        }
        _ => {
            let whole = a.floor() as i64;
            let (h, m, s) = (whole / 3600, whole / 60 % 60, whole % 60);
            match step.unit {
                TimeUnit::SubSecond => {
                    let frac = format!("{:.*}", decimals(step.seconds), a - a.floor());
                    format!("{sign}{h}:{m:02}:{s:02}{}", frac.trim_start_matches('0'))
                }
                TimeUnit::Second => format!("{sign}{h}:{m:02}:{s:02}"),
                _ => format!("{sign}{h}:{m:02}"),
            }
        }
    }
}

/// Default range of a date-time axis showing data from `data_min` to
/// `data_max`: the data padded by [`crate::view::RANGE_MARGIN`] of their span
/// on each side, without the numeric axes' floor at zero (which would stretch
/// a date axis back to 1970); a single value is framed by a day either side
/// (an hour for times of day and durations).
#[must_use]
pub fn time_range(data_min: f64, data_max: f64, format: TemporalFormat) -> (f64, f64) {
    let (lo, hi) = if data_min <= data_max { (data_min, data_max) } else { (data_max, data_min) };
    if !(lo.is_finite() && hi.is_finite()) {
        return (0.0, 86_400.0);
    }
    if hi - lo <= 0.0 {
        let half = if matches!(format, TemporalFormat::Date | TemporalFormat::DateTime) { 86_400.0 } else { 3600.0 };
        return (lo - half, hi + half);
    }
    let pad = (hi - lo) * crate::view::RANGE_MARGIN;
    (lo - pad, hi + pad)
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::temporal::seconds_from_civil;

    fn at(y: i64, m: u32, d: u32, h: u32, mi: u32) -> f64 {
        seconds_from_civil(y, m, d, h, mi, 0.0)
    }

    fn display(format: TemporalFormat) -> TemporalDisplay {
        TemporalDisplay::new(format)
    }

    #[test]
    fn ladder() {
        let f = TemporalFormat::DateTime;
        assert_eq!(time_step(0.3, f).unit, TimeUnit::SubSecond);
        assert!((time_step(0.3, f).seconds - 0.5).abs() < 1e-12);
        assert_eq!((time_step(7.0, f).unit, time_step(7.0, f).count), (TimeUnit::Second, 10));
        assert_eq!((time_step(100.0, f).unit, time_step(100.0, f).count), (TimeUnit::Minute, 2));
        assert_eq!((time_step(4000.0, f).unit, time_step(4000.0, f).count), (TimeUnit::Hour, 2));
        assert_eq!((time_step(3.0 * 86_400.0, f).unit, time_step(3.0 * 86_400.0, f).count), (TimeUnit::Week, 1));
        assert_eq!(time_step(20.0 * 86_400.0, f).unit, TimeUnit::Month);
        assert_eq!(time_step(70.0 * 86_400.0, f).count, 3);
        assert_eq!((time_step(400.0 * 86_400.0, f).unit, time_step(400.0 * 86_400.0, f).count), (TimeUnit::Year, 2));
        assert_eq!(time_step(30.0 * YEAR_SECONDS, f).count, 50);
        let d = TemporalFormat::Duration;
        assert_eq!((time_step(3.0 * 86_400.0, d).unit, time_step(3.0 * 86_400.0, d).count), (TimeUnit::Day, 5));
    }

    #[test]
    fn ticks_land_on_calendar_boundaries() {
        let dt = display(TemporalFormat::DateTime);
        // 10 seconds.
        let lo = at(2026, 10, 13, 15, 57) + 3.0;
        let t = time_ticks(lo, lo + 10.0, step(TimeUnit::Second, 2), &dt);
        assert_eq!(t.first().copied(), Some(lo + 1.0));
        assert_eq!(t.len(), 5);
        // One hour in 15-minute steps.
        let t = time_ticks(at(2026, 10, 13, 15, 5), at(2026, 10, 13, 16, 5), step(TimeUnit::Minute, 15), &dt);
        assert_eq!(
            t,
            vec![at(2026, 10, 13, 15, 15), at(2026, 10, 13, 15, 30), at(2026, 10, 13, 15, 45), at(2026, 10, 13, 16, 0)]
        );
        // Weeks start on Monday (2026-10-12 is a Monday).
        let t = time_ticks(at(2026, 10, 8, 0, 0), at(2026, 10, 27, 0, 0), step(TimeUnit::Week, 1), &dt);
        assert_eq!(t, vec![at(2026, 10, 12, 0, 0), at(2026, 10, 19, 0, 0), at(2026, 10, 26, 0, 0)]);
        // Months over a leap February, and quarters.
        let t = time_ticks(at(2024, 1, 15, 0, 0), at(2024, 4, 2, 0, 0), step(TimeUnit::Month, 1), &dt);
        assert_eq!(t, vec![at(2024, 2, 1, 0, 0), at(2024, 3, 1, 0, 0), at(2024, 4, 1, 0, 0)]);
        assert_eq!(t[1] - t[0], 29.0 * 86_400.0);
        let t = time_ticks(at(2025, 11, 1, 0, 0), at(2027, 2, 1, 0, 0), step(TimeUnit::Month, 3), &dt);
        assert_eq!(
            t,
            vec![
                at(2026, 1, 1, 0, 0),
                at(2026, 4, 1, 0, 0),
                at(2026, 7, 1, 0, 0),
                at(2026, 10, 1, 0, 0),
                at(2027, 1, 1, 0, 0)
            ]
        );
        // Years over three decades in 5s.
        let t = time_ticks(at(1993, 6, 1, 0, 0), at(2023, 6, 1, 0, 0), step(TimeUnit::Year, 5), &dt);
        assert_eq!(
            t,
            vec![
                at(1995, 1, 1, 0, 0),
                at(2000, 1, 1, 0, 0),
                at(2005, 1, 1, 0, 0),
                at(2010, 1, 1, 0, 0),
                at(2015, 1, 1, 0, 0),
                at(2020, 1, 1, 0, 0)
            ]
        );
        // At an offset, ticks fall on local midnight.
        let east = TemporalDisplay { offset_minutes: 120, ..dt };
        let t = time_ticks(at(2026, 10, 12, 12, 0), at(2026, 10, 14, 12, 0), step(TimeUnit::Day, 1), &east);
        assert_eq!(t, vec![at(2026, 10, 12, 22, 0), at(2026, 10, 13, 22, 0)]);
        // Too many ticks gives none.
        assert!(time_ticks(0.0, 1e12, step(TimeUnit::Second, 1), &dt).is_empty());
    }

    #[test]
    fn labels_name_what_changes() {
        let dt = display(TemporalFormat::DateTime);
        let s = step(TimeUnit::Day, 1);
        let t = time_ticks(at(2026, 12, 30, 12, 0), at(2027, 1, 2, 12, 0), s, &dt);
        assert_eq!(time_labels(&t, s, &dt), ["31 Dec 2026", "1 Jan 2027", "2 Jan"]);
        let s = step(TimeUnit::Hour, 6);
        let t = time_ticks(at(2026, 10, 13, 10, 0), at(2026, 10, 14, 7, 0), s, &dt);
        assert_eq!(time_labels(&t, s, &dt), ["13 Oct 12:00", "18:00", "14 Oct 00:00", "06:00"]);
        let s = step(TimeUnit::Month, 1);
        assert_eq!(time_labels(&[at(2026, 10, 1, 0, 0)], s, &dt), ["Oct 2026"]);
        let s = step(TimeUnit::Year, 1);
        assert_eq!(time_labels(&[at(2026, 1, 1, 0, 0)], s, &dt), ["2026"]);
        let sub = time_step(0.4, TemporalFormat::DateTime);
        assert_eq!(time_labels(&[at(2026, 10, 13, 15, 57) + 30.5], sub, &dt), ["13 Oct 15:57:30.5"]);
        let tod = display(TemporalFormat::TimeOfDay);
        assert_eq!(time_labels(&[57_450.0], step(TimeUnit::Second, 30), &tod), ["15:57:30"]);
        let dur = display(TemporalFormat::Duration);
        assert_eq!(time_labels(&[129_600.0, -5400.0], step(TimeUnit::Minute, 30), &dur), ["36:00", "-1:30"]);
        assert_eq!(time_labels(&[172_800.0], step(TimeUnit::Day, 2), &dur), ["2 d"]);
    }

    #[test]
    fn ranges_have_no_zero_floor() {
        let (lo, hi) = time_range(at(2026, 1, 1, 0, 0), at(2026, 1, 11, 0, 0), TemporalFormat::Date);
        assert_eq!((lo, hi), (at(2026, 1, 1, 0, 0) - 0.6 * 86_400.0, at(2026, 1, 11, 0, 0) + 0.6 * 86_400.0));
        let v = at(2026, 1, 1, 0, 0);
        assert_eq!(time_range(v, v, TemporalFormat::Date), (v - 86_400.0, v + 86_400.0));
        assert_eq!(time_range(600.0, 600.0, TemporalFormat::Duration), (-3000.0, 4200.0));
    }
}
