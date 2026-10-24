//! Dates, times of day and durations.
//!
//! A date-time column stores `f64` seconds since 1970-01-01T00:00:00 UTC (the
//! POSIX epoch; IEEE Std 1003.1, "Seconds Since the Epoch"), with `NaN` for a
//! missing value. Like POSIX time, the scale has no leap seconds: every day is
//! 86 400 s long, so `23:59:60` is rejected. Whole seconds are exact for
//! ±2⁵³ s (about ±285 million years); near the present the resolution is about
//! 0.24 µs. Storing seconds as `f64` keeps date columns on the same numeric
//! path as every other continuous column (fits, smoothers, axes).
//!
//! Calendar arithmetic is the proleptic Gregorian calendar of ISO 8601,
//! implemented with the public-domain `days_from_civil` / `civil_from_days`
//! algorithms of H. Hinnant, "chrono-Compatible Low-Level Date Algorithms".
//!
//! Text forms follow ISO 8601-1:2019 and its RFC 3339 profile; spreadsheet
//! serial dates follow ECMA-376 Part 1 §18.17.4 (the 1900 and 1904 date
//! systems).
//!
//! Each column has a [`TemporalDisplay`]: what the values are (calendar date,
//! date-time, time of day, duration), how many fractional-second digits to
//! show, and a fixed offset from UTC in minutes at which dates and times are
//! shown. Values are always stored in UTC; the offset changes only display.

/// Seconds in a day.
pub const SECONDS_PER_DAY: i64 = 86_400;

/// Largest number of fractional-second digits shown or written.
pub const MAX_FRACTION_DIGITS: u8 = 6;

/// Largest offset from UTC accepted, in minutes (±18:00).
pub const MAX_OFFSET_MINUTES: i16 = 18 * 60;

/// What the values of a date-time column represent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TemporalFormat {
    /// Calendar dates, shown `YYYY-MM-DD`.
    Date,
    /// Instants, shown `YYYY-MM-DD hh:mm:ss`.
    DateTime,
    /// Times of day (seconds after midnight), shown `hh:mm:ss`.
    TimeOfDay,
    /// Lengths of time, shown `[-][d ]hh:mm:ss`.
    Duration,
}

impl TemporalFormat {
    /// Every format, in menu order.
    pub const ALL: [Self; 4] = [Self::Date, Self::DateTime, Self::TimeOfDay, Self::Duration];

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Date => "Date",
            Self::DateTime => "Date and time",
            Self::TimeOfDay => "Time of day",
            Self::Duration => "Duration",
        }
    }

    /// Short name used in project files.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Date => "date",
            Self::DateTime => "datetime",
            Self::TimeOfDay => "time",
            Self::Duration => "duration",
        }
    }

    /// The format with this [`TemporalFormat::key`].
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.key() == key)
    }
}

/// How a date-time column shows its values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TemporalDisplay {
    /// What the values represent.
    pub format: TemporalFormat,
    /// Fractional-second digits shown (0 to [`MAX_FRACTION_DIGITS`]).
    pub fraction_digits: u8,
    /// Offset from UTC, in minutes, at which dates and times are shown
    /// (ignored for durations).
    pub offset_minutes: i16,
}

impl TemporalDisplay {
    /// Whole-second display of `format` at UTC.
    #[must_use]
    pub fn new(format: TemporalFormat) -> Self {
        Self { format, fraction_digits: 0, offset_minutes: 0 }
    }

    /// Display text for `seconds`, as shown in the grid and in reports; empty
    /// for a missing (`NaN`) value.
    ///
    /// * Date: `2026-10-13` (the calendar date at the column's offset).
    /// * Date and time: `2026-10-13 15:57:58` (local time at the offset, no
    ///   suffix), with [`fraction_digits`](Self::fraction_digits) decimals.
    /// * Time of day: `15:57:58`.
    /// * Duration: `03:15:00`, or `2 03:15:00` for two days and more, with a
    ///   leading `-` for negative lengths.
    #[must_use]
    pub fn text(&self, seconds: f64) -> String {
        if !seconds.is_finite() {
            return String::new();
        }
        let digits = self.fraction_digits.min(MAX_FRACTION_DIGITS);
        match self.format {
            TemporalFormat::Duration => duration_text(seconds, digits),
            format => {
                let local = seconds + f64::from(self.offset_minutes) * 60.0;
                let (whole, frac) = split_seconds(local, if format == TemporalFormat::Date { 0 } else { digits });
                let (days, secs) = (whole.div_euclid(SECONDS_PER_DAY), whole.rem_euclid(SECONDS_PER_DAY));
                match format {
                    TemporalFormat::Date => date_text(days),
                    TemporalFormat::DateTime => format!("{} {}", date_text(days), clock_text(secs, frac, digits)),
                    _ => clock_text(secs, frac, digits),
                }
            }
        }
    }

    /// ISO 8601 text for `seconds` as written to delimited text and JSON
    /// (RFC 3339 profile); empty for a missing value. Fractional seconds are
    /// written to microseconds with trailing zeros dropped, so the stored value
    /// reads back as written.
    ///
    /// * Date: `2026-10-13`.
    /// * Date and time: `2026-10-13T15:57:58`, followed by the offset when it
    ///   is not zero (`2026-10-13T15:57:58+01:00`).
    /// * Time of day: `15:57:58`.
    /// * Duration: `PT3H15M`, `P2DT1H`, `-PT0.5S`, `PT0S`.
    #[must_use]
    pub fn iso_text(&self, seconds: f64) -> String {
        if !seconds.is_finite() {
            return String::new();
        }
        let digits = MAX_FRACTION_DIGITS;
        match self.format {
            TemporalFormat::Duration => iso_duration_text(seconds),
            format => {
                let local = seconds + f64::from(self.offset_minutes) * 60.0;
                let (whole, frac) = split_seconds(local, if format == TemporalFormat::Date { 0 } else { digits });
                let (days, secs) = (whole.div_euclid(SECONDS_PER_DAY), whole.rem_euclid(SECONDS_PER_DAY));
                let clock = || trim_fraction(clock_text(secs, frac, digits));
                match format {
                    TemporalFormat::Date => date_text(days),
                    TemporalFormat::DateTime => {
                        let offset =
                            if self.offset_minutes == 0 { String::new() } else { offset_text(self.offset_minutes) };
                        format!("{}T{}{offset}", date_text(days), clock())
                    }
                    _ => clock(),
                }
            }
        }
    }

    /// Text for a length of time (a spread statistic of this column, such as
    /// a standard deviation), in the duration style of [`text`](Self::text).
    #[must_use]
    pub fn span_text(&self, seconds: f64) -> String {
        if seconds.is_finite() {
            duration_text(seconds, self.fraction_digits.min(MAX_FRACTION_DIGITS))
        } else {
            String::new()
        }
    }

    /// Interpret typed text as a value of a column with this display.
    ///
    /// Date and date-time columns accept any date or date-time form that
    /// [`parse_value`] reads (slash dates month first); a date or time without
    /// an offset is read at this display's offset. A date column keeps only
    /// the calendar date as typed: a time of day (and its offset) is dropped,
    /// giving midnight at this display's offset. Time-of-day columns accept
    /// times of day; duration columns accept ISO 8601 durations and clock
    /// lengths `[-][d ]h:mm[:ss[.f]]`. Blank text is a missing value.
    ///
    /// # Errors
    /// A message naming what was expected when the text is not a value of
    /// this kind.
    pub fn parse(&self, text: &str) -> Result<f64, String> {
        let t = text.trim();
        if t.is_empty() {
            return Ok(f64::NAN);
        }
        let wrong = || {
            let what = match self.format {
                TemporalFormat::Date | TemporalFormat::DateTime => "a date",
                TemporalFormat::TimeOfDay => "a time of day",
                TemporalFormat::Duration => "a length of time",
            };
            format!("“{t}” is not {what} (for example {})", self.example())
        };
        match self.format {
            TemporalFormat::Duration => parse_clock_duration(t).or_else(|| match parse_value(t)? {
                Parsed::Duration { seconds, .. } => Some(seconds),
                _ => None,
            }),
            TemporalFormat::TimeOfDay => match parse_value(t) {
                Some(Parsed::Time(time)) => Some(time.seconds()),
                _ => None,
            },
            TemporalFormat::Date | TemporalFormat::DateTime => match parse_value(t) {
                // A date column holds whole days: a typed time of day (and
                // its offset) is dropped, so the cell equals its neighbours
                // that display the same date.
                Some(Parsed::Date { date, .. }) if self.format == TemporalFormat::Date => {
                    date.resolve(DateOrder::MonthFirst).map(|days| instant(days, None, None, self.offset_minutes))
                }
                Some(Parsed::Date { date, time, offset }) => {
                    date.resolve(DateOrder::MonthFirst).map(|days| instant(days, time, offset, self.offset_minutes))
                }
                _ => None,
            },
        }
        .ok_or_else(wrong)
    }

    fn example(&self) -> &'static str {
        match self.format {
            TemporalFormat::Date => "2026-10-13",
            TemporalFormat::DateTime => "2026-10-13 15:57:58",
            TemporalFormat::TimeOfDay => "15:57:58",
            TemporalFormat::Duration => "1:30:00 or PT1H30M",
        }
    }
}

// ---------------------------------------------------------------------------
// Calendar arithmetic (Hinnant, "chrono-Compatible Low-Level Date Algorithms")
// ---------------------------------------------------------------------------

/// Days from 1970-01-01 to the proleptic Gregorian date `year-month-day`
/// (negative before 1970). `month` is 1–12 and `day` 1–31; the date is not
/// validated (see [`is_valid_date`]).
#[must_use]
pub fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400; // [0, 399]
    let m = i64::from(month);
    let mp = (m + 9) % 12; // March = 0
    let doy = (153 * mp + 2) / 5 + i64::from(day) - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe - 719_468
}

/// The proleptic Gregorian date `(year, month, day)` that is `days` days after
/// 1970-01-01. Inverse of [`days_from_civil`].
#[must_use]
pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let day = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let month = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let year = yoe + era * 400 + i64::from(month <= 2);
    // Both are within their documented ranges, so the conversions cannot fail.
    (year, u32::try_from(month).unwrap_or(1), u32::try_from(day).unwrap_or(1))
}

/// `true` for a Gregorian leap year.
#[must_use]
pub fn is_leap_year(year: i64) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

/// Number of days in `month` (1–12) of `year`.
#[must_use]
pub fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

/// `true` if `year-month-day` is a real calendar date.
#[must_use]
pub fn is_valid_date(year: i64, month: u32, day: u32) -> bool {
    (1..=12).contains(&month) && day >= 1 && day <= days_in_month(year, month)
}

/// Seconds since the epoch of the date-time `year-month-day hh:mm:ss` in UTC.
#[must_use]
pub fn seconds_from_civil(year: i64, month: u32, day: u32, hour: u32, minute: u32, second: f64) -> f64 {
    let whole = days_from_civil(year, month, day) * SECONDS_PER_DAY + i64::from(hour * 3600 + minute * 60);
    whole as f64 + second
}

/// The same instant with its month and day exchanged, keeping the time of day,
/// as seen at `offset_minutes` from UTC: 2026-03-04 10:00 becomes 2026-04-03
/// 10:00. `None` if the day is above 12 (no such month). Used to correct slash
/// dates read month first that were written day first.
#[must_use]
pub fn swap_day_month(seconds: f64, offset_minutes: i16) -> Option<f64> {
    if !seconds.is_finite() {
        return None;
    }
    let shift = f64::from(offset_minutes) * 60.0;
    let local = seconds + shift;
    let days = (local / SECONDS_PER_DAY as f64).floor();
    let time = local - days * SECONDS_PER_DAY as f64;
    let (y, m, d) = civil_from_days(days as i64);
    is_valid_date(y, d, m).then(|| (days_from_civil(y, d, m) * SECONDS_PER_DAY) as f64 + time - shift)
}

// ---------------------------------------------------------------------------
// Spreadsheet serial dates (ECMA-376 Part 1 §18.17.4)
// ---------------------------------------------------------------------------

/// Days from 1970-01-01 back to 1899-12-30, the origin of 1900-system serials
/// from 1900-03-01 on.
const SERIAL_1900_ORIGIN: i64 = -25_569;
/// Days from 1970-01-01 back to 1904-01-01, the origin of 1904-system serials.
const SERIAL_1904_ORIGIN: i64 = -24_107;

/// Seconds since the epoch for a spreadsheet serial date-time (whole days plus
/// a fraction of a day), or `None` if it names no real date.
///
/// In the 1900 date system serial 1 is 1900-01-01 and the system counts a
/// 29 February 1900 that the Gregorian calendar never had (serial 60), so
/// serials from 61 on count days from 1899-12-30 and serials below 60 from
/// 1899-12-31; serial 60 and negative serials name no date. In the 1904 system
/// serial 0 is 1904-01-01 and negative serials count back from it.
#[must_use]
pub fn from_spreadsheet_serial(serial: f64, date_1904: bool) -> Option<f64> {
    if !serial.is_finite() {
        return None;
    }
    let day = serial.floor();
    let origin = if date_1904 {
        SERIAL_1904_ORIGIN
    } else if day < 0.0 || day == 60.0 {
        return None;
    } else if day < 60.0 {
        SERIAL_1900_ORIGIN + 1
    } else {
        SERIAL_1900_ORIGIN
    };
    // Time of day to the nearest millisecond, the resolution spreadsheets
    // keep: the file stores a decimal fraction of a day to 15 significant
    // digits, so 15:57:58 comes back as 15:57:57.99999744.
    let millis = ((serial - day) * 86_400e3).round();
    Some((day + origin as f64) * SECONDS_PER_DAY as f64 + millis / 1e3)
}

/// The 1900-system spreadsheet serial for `seconds` since the epoch, or `None`
/// before 1900-01-01 (which the system cannot represent).
#[must_use]
pub fn to_spreadsheet_serial(seconds: f64) -> Option<f64> {
    if !seconds.is_finite() {
        return None;
    }
    let days = seconds / SECONDS_PER_DAY as f64;
    let since_origin = days - SERIAL_1900_ORIGIN as f64;
    if since_origin >= 61.0 {
        Some(since_origin)
    } else if since_origin >= 2.0 {
        Some(since_origin - 1.0)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Formatting
// ---------------------------------------------------------------------------

/// Split `seconds` into whole seconds and a count of `10^-digits` units,
/// rounding to `digits` decimals first so that 59.9996 s shown with three
/// digits reads 60.000 → the next minute rather than 59.1000.
fn split_seconds(seconds: f64, digits: u8) -> (i64, u64) {
    let scale = 10_f64.powi(i32::from(digits));
    let whole = seconds.floor();
    let mut units = ((seconds - whole) * scale).round();
    let mut whole = whole as i64;
    if units >= scale {
        whole += 1;
        units = 0.0;
    }
    (whole, units as u64)
}

fn date_text(days: i64) -> String {
    let (y, m, d) = civil_from_days(days);
    if (0..=9999).contains(&y) { format!("{y:04}-{m:02}-{d:02}") } else { format!("{y:+05}-{m:02}-{d:02}") }
}

fn clock_text(seconds_of_day: i64, frac_units: u64, digits: u8) -> String {
    let (h, m, s) = (seconds_of_day / 3600, seconds_of_day / 60 % 60, seconds_of_day % 60);
    if digits == 0 {
        format!("{h:02}:{m:02}:{s:02}")
    } else {
        format!("{h:02}:{m:02}:{s:02}.{frac_units:0width$}", width = usize::from(digits))
    }
}

fn trim_fraction(mut text: String) -> String {
    if text.contains('.') {
        while text.ends_with('0') {
            text.pop();
        }
        if text.ends_with('.') {
            text.pop();
        }
    }
    text
}

fn offset_text(minutes: i16) -> String {
    let sign = if minutes < 0 { '-' } else { '+' };
    let a = minutes.unsigned_abs();
    format!("{sign}{:02}:{:02}", a / 60, a % 60)
}

fn duration_text(seconds: f64, digits: u8) -> String {
    let sign = if seconds < 0.0 { "-" } else { "" };
    let (whole, frac) = split_seconds(seconds.abs(), digits);
    let (days, rest) = (whole / SECONDS_PER_DAY, whole % SECONDS_PER_DAY);
    let clock = clock_text(rest, frac, digits);
    if days == 0 { format!("{sign}{clock}") } else { format!("{sign}{days} {clock}") }
}

fn iso_duration_text(seconds: f64) -> String {
    let sign = if seconds < 0.0 { "-" } else { "" };
    let (whole, frac) = split_seconds(seconds.abs(), MAX_FRACTION_DIGITS);
    let (days, rest) = (whole / SECONDS_PER_DAY, whole % SECONDS_PER_DAY);
    let (h, m, s) = (rest / 3600, rest / 60 % 60, rest % 60);
    let mut out = format!("{sign}P");
    if days > 0 {
        out.push_str(&format!("{days}D"));
    }
    if h > 0 || m > 0 || s > 0 || frac > 0 || days == 0 {
        out.push('T');
        if h > 0 {
            out.push_str(&format!("{h}H"));
        }
        if m > 0 {
            out.push_str(&format!("{m}M"));
        }
        if s > 0 || frac > 0 || (h == 0 && m == 0) {
            let secs = trim_fraction(format!("{s}.{frac:06}"));
            out.push_str(&format!("{secs}S"));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// Which of the first two numbers of a slash date such as `03/04/2026` is the
/// month.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateOrder {
    /// Month, day, year: `03/04/2026` is 2026-03-04.
    MonthFirst,
    /// Day, month, year: `03/04/2026` is 2026-04-03.
    DayFirst,
}

/// A calendar date as written, before a slash date's field order is known.
#[derive(Debug, Clone, Copy, PartialEq)]
enum RawDate {
    /// Year, month and day are known.
    Ymd(i64, u32, u32),
    /// `first/second/year` where either of the first two may be the month.
    Numeric { first: u32, second: u32, year: i64 },
}

impl RawDate {
    /// Days since the epoch under `order`, or `None` if not a real date.
    fn resolve(self, order: DateOrder) -> Option<i64> {
        let (y, m, d) = match (self, order) {
            (Self::Ymd(y, m, d), _) => (y, m, d),
            (Self::Numeric { first, second, year }, DateOrder::MonthFirst) => (year, first, second),
            (Self::Numeric { first, second, year }, DateOrder::DayFirst) => (year, second, first),
        };
        is_valid_date(y, m, d).then(|| days_from_civil(y, m, d))
    }
}

/// A time of day as written.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Clock {
    hour: u32,
    minute: u32,
    second: u32,
    fraction: f64,
    digits: u8,
}

impl Clock {
    fn seconds(self) -> f64 {
        f64::from(self.hour * 3600 + self.minute * 60 + self.second) + self.fraction
    }
}

/// One value read from text.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Parsed {
    /// A date, with an optional time and an optional offset (minutes).
    Date { date: RawDate, time: Option<Clock>, offset: Option<i16> },
    /// A time of day with no date.
    Time(Clock),
    /// An ISO 8601 duration.
    Duration { seconds: f64, digits: u8 },
}

/// Seconds since the epoch for a date (days), an optional time and an
/// optional explicit offset; without one the value is read at `default_offset`.
fn instant(days: i64, time: Option<Clock>, offset: Option<i16>, default_offset: i16) -> f64 {
    let local = (days * SECONDS_PER_DAY) as f64 + time.map_or(0.0, Clock::seconds);
    local - f64::from(offset.unwrap_or(default_offset)) * 60.0
}

struct Cursor<'a> {
    s: &'a [u8],
    i: usize,
}

impl<'a> Cursor<'a> {
    fn new(s: &'a str) -> Self {
        Self { s: s.as_bytes(), i: 0 }
    }
    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }
    fn done(&self) -> bool {
        self.i >= self.s.len()
    }
    fn eat(&mut self, c: u8) -> bool {
        if self.peek() == Some(c) {
            self.i += 1;
            true
        } else {
            false
        }
    }
    fn eat_any(&mut self, set: &[u8]) -> Option<u8> {
        let c = self.peek().filter(|c| set.contains(c))?;
        self.i += 1;
        Some(c)
    }
    fn spaces(&mut self) -> usize {
        let start = self.i;
        while self.peek() == Some(b' ') {
            self.i += 1;
        }
        self.i - start
    }
    /// Between `min` and `max` ASCII digits, as a number and the digit count.
    fn digits(&mut self, min: usize, max: usize) -> Option<(u64, usize)> {
        let start = self.i;
        let mut n: u64 = 0;
        while self.i - start < max && self.peek().is_some_and(|c| c.is_ascii_digit()) {
            n = n * 10 + u64::from(self.s[self.i] - b'0');
            self.i += 1;
        }
        let count = self.i - start;
        if count < min || self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.i = start;
            return None;
        }
        Some((n, count))
    }
    /// Exactly `n` ASCII digits, whatever follows.
    fn fixed(&mut self, n: usize) -> Option<u64> {
        let digits = self.s.get(self.i..self.i + n)?;
        if !digits.iter().all(u8::is_ascii_digit) {
            return None;
        }
        self.i += n;
        Some(digits.iter().fold(0, |acc, &d| acc * 10 + u64::from(d - b'0')))
    }
    fn letters(&mut self) -> &'a [u8] {
        let start = self.i;
        while self.peek().is_some_and(|c| c.is_ascii_alphabetic()) {
            self.i += 1;
        }
        &self.s[start..self.i]
    }
}

fn small(n: u64) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// Month number for an English month name or its three-letter (or `sept`)
/// abbreviation, ignoring case.
fn month_from_name(word: &[u8]) -> Option<u32> {
    const NAMES: [&str; 12] = [
        "january",
        "february",
        "march",
        "april",
        "may",
        "june",
        "july",
        "august",
        "september",
        "october",
        "november",
        "december",
    ];
    let w = std::str::from_utf8(word).ok()?.to_ascii_lowercase();
    if w.len() < 3 {
        return None;
    }
    let found = NAMES
        .iter()
        .position(|n| *n == w || (w.len() == 3 && n.starts_with(&w)) || (w == "sept" && *n == "september"))?;
    u32::try_from(found + 1).ok()
}

/// A year of two or four digits; two-digit years follow the POSIX `strptime`
/// rule (69–99 → 1969–1999, 00–68 → 2000–2068).
fn year(c: &mut Cursor<'_>) -> Option<i64> {
    let (n, count) = c.digits(2, 4)?;
    let n = i64::try_from(n).ok()?;
    match count {
        2 => Some(if n >= 69 { 1900 + n } else { 2000 + n }),
        4 => Some(n),
        _ => None,
    }
}

/// The date part of a value: ISO `YYYY-MM-DD` (also with `/` or `.`), a numeric
/// `a/b/y` date (`/`, `.` or `-`), `13 Oct 2026` / `13-Oct-2026`, or
/// `Oct 13, 2026` / `October 13 2026`.
fn date_part(c: &mut Cursor<'_>) -> Option<RawDate> {
    let start = c.i;
    // Year first.
    if let Some((y, 4)) = c.digits(4, 4) {
        if let Some(sep) = c.eat_any(b"-/.") {
            if let Some((m, _)) = c.digits(1, 2) {
                if c.eat(sep) {
                    if let Some((d, _)) = c.digits(1, 2) {
                        return Some(RawDate::Ymd(i64::try_from(y).ok()?, small(m), small(d)));
                    }
                }
            }
        }
        c.i = start;
        return None;
    }
    // Day or month number first.
    if let Some((a, _)) = c.digits(1, 2) {
        let after_first = c.i;
        if let Some(sep) = c.eat_any(b"/.-") {
            if let Some((b, _)) = c.digits(1, 2) {
                if c.eat(sep) {
                    if let Some(y) = year(c) {
                        return Some(RawDate::Numeric { first: small(a), second: small(b), year: y });
                    }
                }
            }
        }
        // `13 Oct 2026`, `13-Oct-2026`.
        c.i = after_first;
        let sep = c.eat(b'-');
        if !sep && c.spaces() == 0 {
            c.i = start;
            return None;
        }
        if let Some(m) = month_from_name(c.letters()) {
            let sep_ok = if sep { c.eat(b'-') } else { c.spaces() > 0 };
            if sep_ok {
                if let Some(y) = year(c) {
                    return Some(RawDate::Ymd(y, m, small(a)));
                }
            }
        }
        c.i = start;
        return None;
    }
    // `Oct 13, 2026`, `October 13 2026`.
    if let Some(m) = month_from_name(c.letters()) {
        if c.spaces() > 0 {
            if let Some((d, _)) = c.digits(1, 2) {
                let comma = c.eat(b',');
                if c.spaces() > 0 || comma {
                    c.spaces();
                    if let Some(y) = year(c) {
                        return Some(RawDate::Ymd(y, m, small(d)));
                    }
                }
            }
        }
    }
    c.i = start;
    None
}

/// `h:mm[:ss[.f]]` with an optional `AM`/`PM` suffix.
fn clock_part(c: &mut Cursor<'_>) -> Option<Clock> {
    let start = c.i;
    let fail = |c: &mut Cursor<'_>| {
        c.i = start;
        None
    };
    let Some((h, _)) = c.digits(1, 2) else { return fail(c) };
    if !c.eat(b':') {
        return fail(c);
    }
    let Some((m, _)) = c.digits(2, 2) else { return fail(c) };
    let (mut s, mut fraction, mut digits) = (0, 0.0, 0u8);
    if c.eat(b':') {
        let Some((sec, _)) = c.digits(2, 2) else { return fail(c) };
        s = sec;
        if c.eat_any(b".,").is_some() {
            let f0 = c.i;
            while c.peek().is_some_and(|ch| ch.is_ascii_digit()) {
                c.i += 1;
            }
            let text = std::str::from_utf8(&c.s[f0..c.i]).ok()?;
            if text.is_empty() {
                return fail(c);
            }
            fraction = format!("0.{text}").parse().ok()?;
            digits = u8::try_from(text.len()).unwrap_or(u8::MAX).min(MAX_FRACTION_DIGITS);
        }
    }
    let mut hour = small(h);
    // AM/PM.
    let before = c.i;
    c.spaces();
    let word = c.letters().to_ascii_lowercase();
    match word.as_slice() {
        b"am" | b"pm" => {
            if !(1..=12).contains(&hour) {
                return fail(c);
            }
            hour %= 12;
            if word == b"pm" {
                hour += 12;
            }
        }
        _ => c.i = before,
    }
    if hour >= 24 || m >= 60 || s >= 60 {
        return fail(c);
    }
    Some(Clock { hour, minute: small(m), second: small(s), fraction, digits })
}

/// `Z`, `±hh:mm`, `±hhmm` or `±hh`, in minutes.
fn offset_part(c: &mut Cursor<'_>) -> Option<i16> {
    if c.eat_any(b"Zz").is_some() {
        return Some(0);
    }
    let start = c.i;
    let sign: i16 = match c.eat_any(b"+-") {
        Some(b'+') => 1,
        Some(_) => -1,
        None => return None,
    };
    let parsed = (|| {
        let h = c.fixed(2)?;
        let colon = c.eat(b':');
        let m = match c.fixed(2) {
            Some(m) => m,
            None if colon => return None,
            None => 0,
        };
        let total = i16::try_from(h * 60 + m).ok()?;
        (m < 60 && total <= MAX_OFFSET_MINUTES).then_some(sign * total)
    })();
    if parsed.is_none() {
        c.i = start;
    }
    parsed
}

/// ISO 8601 duration: `[-]P[nW]` or `[-]P[nD][T[nH][nM][n[.f]S]]`.
/// Years and months are rejected: their length in seconds is not fixed.
fn iso_duration(text: &str) -> Option<(f64, u8)> {
    let (negative, rest) = match text.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, text),
    };
    let rest = rest.strip_prefix(['P', 'p'])?;
    let (date, time) = match rest.find(['T', 't']) {
        Some(k) => (&rest[..k], Some(&rest[k + 1..])),
        None => (rest, None),
    };
    let mut total = 0.0;
    let mut digits = 0u8;
    let mut any = false;
    let mut take = |part: &str, units: &[(char, f64)], allow_fraction: char| -> Option<()> {
        let mut number = String::new();
        let mut next_unit = 0;
        for ch in part.chars() {
            if ch.is_ascii_digit() || ch == '.' || ch == ',' {
                number.push(if ch == ',' { '.' } else { ch });
                continue;
            }
            let ch = ch.to_ascii_uppercase();
            let k = units[next_unit..].iter().position(|(u, _)| *u == ch)? + next_unit;
            if number.is_empty() || (number.contains('.') && ch != allow_fraction) {
                return None;
            }
            if let Some(dot) = number.find('.') {
                digits = u8::try_from(number.len() - dot - 1).unwrap_or(u8::MAX).min(MAX_FRACTION_DIGITS);
            }
            total += number.parse::<f64>().ok()? * units[k].1;
            number.clear();
            next_unit = k + 1;
            any = true;
        }
        number.is_empty().then_some(())
    };
    take(date, &[('W', 604_800.0), ('D', 86_400.0)], '\0')?;
    if let Some(time) = time {
        if time.is_empty() {
            return None;
        }
        take(time, &[('H', 3600.0), ('M', 60.0), ('S', 1.0)], 'S')?;
    }
    any.then_some((if negative { -total } else { total }, digits))
}

/// `[-][d ]h:mm[:ss[.f]]` with any number of hours.
fn parse_clock_duration(text: &str) -> Option<f64> {
    let (negative, rest) = match text.strip_prefix('-') {
        Some(r) => (true, r.trim_start()),
        None => (false, text),
    };
    let (days, clock) = match rest.split_once(' ') {
        Some((d, c)) => (d.parse::<u64>().ok()?, c.trim()),
        None => (0, rest),
    };
    let parts: Vec<&str> = clock.split(':').collect();
    if !(2..=3).contains(&parts.len()) || parts.iter().any(|p| p.is_empty()) {
        return None;
    }
    let h: u64 = parts[0].parse().ok()?;
    let m: u64 = parts[1].parse().ok()?;
    let s: f64 = match parts.get(2) {
        Some(p) if p.chars().all(|c| c.is_ascii_digit() || c == '.') => p.parse().ok()?,
        Some(_) => return None,
        None => 0.0,
    };
    if m >= 60 || s >= 60.0 || (days > 0 && h >= 24) {
        return None;
    }
    let total = (days * 86_400 + h * 3600 + m * 60) as f64 + s;
    Some(if negative { -total } else { total })
}

/// Read one value: a date with an optional time and offset, a time of day, or
/// an ISO 8601 duration. Plain numbers are not dates.
fn parse_value(text: &str) -> Option<Parsed> {
    let text = text.trim();
    if !text.is_ascii() || text.is_empty() {
        return None;
    }
    let mut c = Cursor::new(text);
    if let Some(date) = date_part(&mut c) {
        if c.done() {
            return Some(Parsed::Date { date, time: None, offset: None });
        }
        let iso_t = c.eat_any(b"Tt").is_some();
        if !iso_t && c.spaces() == 0 {
            return None;
        }
        let time = clock_part(&mut c)?;
        c.spaces();
        let offset = if c.done() { None } else { Some(offset_part(&mut c)?) };
        return c.done().then_some(Parsed::Date { date, time: Some(time), offset });
    }
    if let Some(time) = clock_part(&mut c) {
        return c.done().then_some(Parsed::Time(time));
    }
    iso_duration(text).map(|(seconds, digits)| Parsed::Duration { seconds, digits })
}

/// A date-time column read from text by [`detect_column`].
#[derive(Debug, Clone, PartialEq)]
pub struct DetectedColumn {
    /// Seconds since the epoch (or since midnight, or lengths); `NaN` for blank
    /// text.
    pub values: Vec<f64>,
    /// The display the values were written in.
    pub display: TemporalDisplay,
    /// `true` if slash dates could be read either way round and were read
    /// month first.
    pub ambiguous_order: bool,
}

/// Read a column of text as dates, times of day or durations.
///
/// The column qualifies only if every non-blank value reads under one kind:
/// all dates (with or without times; dates with times give a date-and-time
/// column), all times of day, or all ISO 8601 durations. Numeric slash dates
/// are read month first if every value is a real date that way, otherwise day
/// first; [`DetectedColumn::ambiguous_order`] reports when both readings were
/// possible. The first explicit UTC offset becomes the display offset, and
/// values without an offset are read at it. Returns `None` if the column has no
/// non-blank values or any value does not qualify.
#[must_use]
pub fn detect_column<S: AsRef<str>>(texts: &[S]) -> Option<DetectedColumn> {
    let mut parsed = Vec::with_capacity(texts.len());
    let mut any = false;
    for t in texts {
        let t = t.as_ref().trim();
        if t.is_empty() {
            parsed.push(None);
        } else {
            parsed.push(Some(parse_value(t)?));
            any = true;
        }
    }
    if !any {
        return None;
    }
    let first = parsed.iter().flatten().next()?;
    let mut digits = 0u8;
    match first {
        Parsed::Date { .. } => {
            let mut has_time = false;
            let mut offset_minutes = None;
            let (mut month_first_ok, mut day_first_ok, mut ambiguous) = (true, true, false);
            for p in parsed.iter().flatten() {
                let Parsed::Date { date, time, offset } = p else { return None };
                if let Some(t) = time {
                    has_time = true;
                    digits = digits.max(t.digits);
                }
                if offset_minutes.is_none() {
                    offset_minutes = *offset;
                }
                month_first_ok &= date.resolve(DateOrder::MonthFirst).is_some();
                day_first_ok &= date.resolve(DateOrder::DayFirst).is_some();
                if let RawDate::Numeric { first, second, .. } = date {
                    ambiguous |= first != second;
                }
            }
            let order = if month_first_ok {
                DateOrder::MonthFirst
            } else if day_first_ok {
                DateOrder::DayFirst
            } else {
                return None;
            };
            let offset = offset_minutes.unwrap_or(0);
            let values = parsed
                .iter()
                .map(|p| match p {
                    Some(Parsed::Date { date, time, offset: o }) => {
                        date.resolve(order).map_or(f64::NAN, |days| instant(days, *time, *o, offset))
                    }
                    _ => f64::NAN,
                })
                .collect();
            let format = if has_time { TemporalFormat::DateTime } else { TemporalFormat::Date };
            Some(DetectedColumn {
                values,
                display: TemporalDisplay { format, fraction_digits: digits, offset_minutes: offset },
                ambiguous_order: ambiguous && month_first_ok && day_first_ok,
            })
        }
        Parsed::Time(_) => {
            let values = parsed
                .iter()
                .map(|p| match p {
                    None => Some(f64::NAN),
                    Some(Parsed::Time(t)) => {
                        digits = digits.max(t.digits);
                        Some(t.seconds())
                    }
                    Some(_) => None,
                })
                .collect::<Option<Vec<f64>>>()?;
            let display =
                TemporalDisplay { format: TemporalFormat::TimeOfDay, fraction_digits: digits, offset_minutes: 0 };
            Some(DetectedColumn { values, display, ambiguous_order: false })
        }
        Parsed::Duration { .. } => {
            let values = parsed
                .iter()
                .map(|p| match p {
                    None => Some(f64::NAN),
                    Some(Parsed::Duration { seconds, digits: d }) => {
                        digits = digits.max(*d);
                        Some(*seconds)
                    }
                    Some(_) => None,
                })
                .collect::<Option<Vec<f64>>>()?;
            let display =
                TemporalDisplay { format: TemporalFormat::Duration, fraction_digits: digits, offset_minutes: 0 };
            Some(DetectedColumn { values, display, ambiguous_order: false })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ymd(y: i64, m: u32, d: u32) -> f64 {
        seconds_from_civil(y, m, d, 0, 0, 0.0)
    }

    #[test]
    fn calendar_edges() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(1969, 12, 31), -1);
        assert_eq!(days_from_civil(2000, 3, 1) - days_from_civil(2000, 2, 28), 2, "2000 is a leap year");
        assert_eq!(days_from_civil(1900, 3, 1) - days_from_civil(1900, 2, 28), 1, "1900 is not");
        assert_eq!(days_from_civil(2100, 3, 1) - days_from_civil(2100, 2, 28), 1, "2100 is not");
        assert_eq!(days_from_civil(1, 1, 1), -719_162);
        for d in [-800_000, -719_162, -1, 0, 1, 11_016, 2_932_896] {
            let (y, m, dd) = civil_from_days(d);
            assert!(is_valid_date(y, m, dd));
            assert_eq!(days_from_civil(y, m, dd), d);
        }
        assert!(!is_valid_date(2026, 2, 29));
        assert!(is_valid_date(2024, 2, 29));
        assert!(!is_valid_date(2026, 13, 1));
    }

    #[test]
    fn display_text() {
        let t = ymd(2026, 10, 13) + 15.0 * 3600.0 + 57.0 * 60.0 + 58.25;
        let mut d = TemporalDisplay::new(TemporalFormat::DateTime);
        assert_eq!(d.text(t), "2026-10-13 15:57:58");
        assert_eq!(d.iso_text(t), "2026-10-13T15:57:58.25");
        d.fraction_digits = 3;
        assert_eq!(d.text(t), "2026-10-13 15:57:58.250");
        d.offset_minutes = 60;
        assert_eq!(d.text(t), "2026-10-13 16:57:58.250");
        assert_eq!(d.iso_text(t), "2026-10-13T16:57:58.25+01:00");
        d.offset_minutes = -330;
        assert_eq!(d.iso_text(t), "2026-10-13T10:27:58.25-05:30");
        assert_eq!(d.text(f64::NAN), "");
        // Rounding carries into the next minute, hour and day.
        let d0 = TemporalDisplay::new(TemporalFormat::DateTime);
        assert_eq!(d0.text(ymd(2026, 12, 31) + 86_399.6), "2027-01-01 00:00:00");
        assert_eq!(TemporalDisplay::new(TemporalFormat::Date).text(t), "2026-10-13");
        assert_eq!(TemporalDisplay::new(TemporalFormat::Date).text(ymd(1969, 12, 31) + 1.0), "1969-12-31");
        assert_eq!(TemporalDisplay::new(TemporalFormat::TimeOfDay).text(57_478.0), "15:57:58");
        let dur = TemporalDisplay::new(TemporalFormat::Duration);
        assert_eq!(dur.text(3.25 * 3600.0), "03:15:00");
        assert_eq!(dur.text(2.0 * 86_400.0 + 3.25 * 3600.0), "2 03:15:00");
        assert_eq!(dur.text(-90.0), "-00:01:30");
        assert_eq!(dur.iso_text(3.25 * 3600.0), "PT3H15M");
        assert_eq!(dur.iso_text(2.0 * 86_400.0 + 3600.0), "P2DT1H");
        assert_eq!(dur.iso_text(-0.5), "-PT0.5S");
        assert_eq!(dur.iso_text(0.0), "PT0S");
        assert_eq!(dur.iso_text(86_400.0), "P1D");
        assert_eq!(TemporalDisplay::new(TemporalFormat::Date).text(ymd(-44, 3, 15)), "-0044-03-15");
    }

    #[test]
    fn date_column_drops_a_typed_time() {
        let date = TemporalDisplay::new(TemporalFormat::Date);
        assert_eq!(date.parse("2026-03-04 10:00"), date.parse("2026-03-04"));
        assert_eq!(date.parse("2026-03-04T23:30:00-05:00"), Ok(ymd(2026, 3, 4)));
        let mut shifted = date;
        shifted.offset_minutes = 60;
        assert_eq!(shifted.parse("2026-03-04 10:00"), Ok(ymd(2026, 3, 4) - 3600.0));
        // A date-time column still keeps the time.
        let dt = TemporalDisplay::new(TemporalFormat::DateTime);
        assert_eq!(dt.parse("2026-03-04 10:00"), Ok(ymd(2026, 3, 4) + 36_000.0));
    }

    #[test]
    fn parses_common_forms() {
        let day = ymd(2026, 10, 13);
        let at = |h: f64, m: f64, s: f64| day + h * 3600.0 + m * 60.0 + s;
        let one = |s: &str| detect_column(&[s]).unwrap_or_else(|| panic!("{s:?} not read"));
        for (text, expected, format) in [
            ("2026-10-13", day, TemporalFormat::Date),
            ("2026/10/13", day, TemporalFormat::Date),
            ("10/13/2026", day, TemporalFormat::Date),
            ("13/10/2026", day, TemporalFormat::Date),
            ("13.10.26", day, TemporalFormat::Date),
            ("13 Oct 2026", day, TemporalFormat::Date),
            ("13-oct-2026", day, TemporalFormat::Date),
            ("Oct 13, 2026", day, TemporalFormat::Date),
            ("October 13 2026", day, TemporalFormat::Date),
            ("Sept 3, 2026", ymd(2026, 9, 3), TemporalFormat::Date),
            ("2026-10-13T15:57:58", at(15.0, 57.0, 58.0), TemporalFormat::DateTime),
            ("2026-10-13 15:57", at(15.0, 57.0, 0.0), TemporalFormat::DateTime),
            ("2026-10-13T15:57:58Z", at(15.0, 57.0, 58.0), TemporalFormat::DateTime),
            ("2026-10-13T15:57:58.250+01:00", at(14.0, 57.0, 58.25), TemporalFormat::DateTime),
            ("2026-10-13T15:57:58-0530", at(21.0, 27.0, 58.0), TemporalFormat::DateTime),
            ("10/13/2026 3:57 PM", at(15.0, 57.0, 0.0), TemporalFormat::DateTime),
            ("10/13/2026 12:05 am", at(0.0, 5.0, 0.0), TemporalFormat::DateTime),
            ("15:57:58", 57_478.0, TemporalFormat::TimeOfDay),
            ("7:05", 25_500.0, TemporalFormat::TimeOfDay),
            ("PT1H30M", 5400.0, TemporalFormat::Duration),
            ("P1DT0.5S", 86_400.5, TemporalFormat::Duration),
            ("P2W", 1_209_600.0, TemporalFormat::Duration),
            ("-PT10M", -600.0, TemporalFormat::Duration),
        ] {
            let c = one(text);
            assert_eq!(c.values, vec![expected], "{text}");
            assert_eq!(c.display.format, format, "{text}");
        }
        let c = one("2026-10-13T15:57:58.250+01:00");
        assert_eq!((c.display.offset_minutes, c.display.fraction_digits), (60, 3));
    }

    #[test]
    fn rejects_non_dates() {
        for text in [
            "2026-02-30",
            "2026-13-01",
            "24:00:01",
            "23:59:60",
            "12:60",
            "13:00 PM",
            "0:30 am",
            "12",
            "1.5",
            "2026",
            "20261013",
            "Oct 2026",
            "13 Foo 2026",
            "2026-10-13T",
            "2026-10-13 15:57 +25:00",
            "2026-10-13x",
            "P",
            "PT",
            "P1Y",
            "P1M",
            "PT1.5H30M",
            "hello",
            "1/2/3/4",
            "10/13/2026 15:57 PM",
        ] {
            assert!(detect_column(&[text]).is_none(), "{text:?} should not be a date");
        }
        // Mixed kinds in one column.
        assert!(detect_column(&["2026-10-13", "15:00"]).is_none());
        assert!(detect_column(&["2026-10-13", "x"]).is_none());
        assert!(detect_column(&["", " "]).is_none());
    }

    #[test]
    fn slash_order_from_the_data() {
        // A day above 12 settles it.
        let c = detect_column(&["03/04/2026", "25/04/2026"]).unwrap();
        assert_eq!(c.values, vec![ymd(2026, 4, 3), ymd(2026, 4, 25)]);
        assert!(!c.ambiguous_order);
        let c = detect_column(&["03/04/2026", "04/25/2026", ""]).unwrap();
        assert_eq!(c.values[0], ymd(2026, 3, 4));
        assert!(c.values[2].is_nan());
        assert!(!c.ambiguous_order);
        // Undecidable: month first, and reported.
        let c = detect_column(&["03/04/2026", "05/06/2026"]).unwrap();
        assert_eq!(c.values, vec![ymd(2026, 3, 4), ymd(2026, 5, 6)]);
        assert!(c.ambiguous_order);
        // Same number twice is not ambiguous.
        assert!(!detect_column(&["03/03/2026"]).unwrap().ambiguous_order);
        // Neither order works for every value.
        assert!(detect_column(&["13/04/2026", "04/13/2026"]).is_none());
    }

    #[test]
    fn mixed_dates_and_times_and_offsets() {
        let c = detect_column(&["2026-10-13", "2026-10-14 06:00:00.5", "2026-10-15T00:00+02:00"]).unwrap();
        assert_eq!(c.display.format, TemporalFormat::DateTime);
        assert_eq!((c.display.offset_minutes, c.display.fraction_digits), (120, 1));
        // Values without an offset are read at the column's offset.
        assert_eq!(c.values[0], ymd(2026, 10, 13) - 7200.0);
        assert_eq!(c.values[1], ymd(2026, 10, 14) + 6.0 * 3600.0 + 0.5 - 7200.0);
        assert_eq!(c.values[2], ymd(2026, 10, 15) - 7200.0);
        // Display at the offset gives back what was written.
        assert_eq!(c.display.text(c.values[2]), "2026-10-15 00:00:00.0");
    }

    #[test]
    fn swaps_day_and_month() {
        let d = ymd(2026, 3, 4) + 36_000.5;
        assert_eq!(swap_day_month(d, 0), Some(ymd(2026, 4, 3) + 36_000.5));
        assert_eq!(swap_day_month(ymd(2026, 3, 13), 0), None);
        // At an offset the local date is what is swapped.
        assert_eq!(swap_day_month(ymd(2026, 3, 4) - 3600.0, 60), Some(ymd(2026, 4, 3) - 3600.0));
        assert_eq!(swap_day_month(f64::NAN, 0), None);
    }

    #[test]
    fn two_digit_years() {
        assert_eq!(detect_column(&["1/2/69"]).unwrap().values, vec![ymd(1969, 1, 2)]);
        assert_eq!(detect_column(&["1/2/68"]).unwrap().values, vec![ymd(2068, 1, 2)]);
        assert_eq!(detect_column(&["1/2/00"]).unwrap().values, vec![ymd(2000, 1, 2)]);
    }

    #[test]
    fn entry_parsing() {
        let date = TemporalDisplay::new(TemporalFormat::Date);
        assert_eq!(date.parse("2026-10-13").unwrap(), ymd(2026, 10, 13));
        assert_eq!(date.parse("03/04/2026").unwrap(), ymd(2026, 3, 4), "month first");
        assert!(date.parse("").unwrap().is_nan());
        assert!(date.parse("soon").unwrap_err().contains("not a date"));
        let mut dt = TemporalDisplay::new(TemporalFormat::DateTime);
        dt.offset_minutes = 60;
        assert_eq!(dt.parse("2026-10-13 01:00").unwrap(), ymd(2026, 10, 13), "read at the column offset");
        assert_eq!(dt.parse("2026-10-13 01:00Z").unwrap(), ymd(2026, 10, 13) + 3600.0);
        let tod = TemporalDisplay::new(TemporalFormat::TimeOfDay);
        assert_eq!(tod.parse("1:30 pm").unwrap(), 48_600.0);
        assert!(tod.parse("2026-10-13").is_err());
        let dur = TemporalDisplay::new(TemporalFormat::Duration);
        assert_eq!(dur.parse("1:30:00").unwrap(), 5400.0);
        assert_eq!(dur.parse("36:00").unwrap(), 129_600.0);
        assert_eq!(dur.parse("2 03:15:00").unwrap(), 2.0 * 86_400.0 + 11_700.0);
        assert_eq!(dur.parse("-00:01:30").unwrap(), -90.0);
        assert_eq!(dur.parse("PT90M").unwrap(), 5400.0);
        assert!(dur.parse("1:75").is_err());
    }

    #[test]
    fn text_round_trips() {
        let values = [
            ymd(2026, 10, 13) + 57_478.125,
            ymd(1900, 2, 28),
            ymd(2000, 2, 29) + 86_399.0,
            ymd(1, 1, 1),
            ymd(9999, 12, 31) + 0.000_001,
            ymd(1969, 12, 31) + 0.5,
        ];
        for offset in [0, 60, -330, 840, -720] {
            for format in [TemporalFormat::DateTime, TemporalFormat::Date] {
                let d = TemporalDisplay { format, fraction_digits: 3, offset_minutes: offset };
                for &v in &values {
                    let v = if format == TemporalFormat::Date {
                        (v / 86_400.0).floor() * 86_400.0 - f64::from(offset) * 60.0
                    } else {
                        v
                    };
                    let text = d.iso_text(v);
                    let back = d.parse(&text).unwrap();
                    assert!((back - v).abs() < 1e-6, "{text}: {back} vs {v}");
                    if format == TemporalFormat::DateTime || offset == 0 {
                        // A date carries no offset, so it reads back at UTC.
                        let col = detect_column(&[text.as_str()]).unwrap();
                        assert!((col.values[0] - v).abs() < 1e-6, "{text}");
                    }
                }
            }
        }
        let dur = TemporalDisplay::new(TemporalFormat::Duration);
        for v in [0.0, 1.5, -90.0, 86_400.0 * 3.0 + 1.25, 59.999_999] {
            assert_eq!(detect_column(&[dur.iso_text(v)]).unwrap().values[0], v);
            assert!((dur.parse(&TemporalDisplay { fraction_digits: 6, ..dur }.text(v)).unwrap() - v).abs() < 1e-9);
        }
    }

    #[test]
    fn spreadsheet_serials() {
        // ECMA-376 §18.17.4: 1900 system.
        assert_eq!(from_spreadsheet_serial(1.0, false), Some(ymd(1900, 1, 1)));
        assert_eq!(from_spreadsheet_serial(59.0, false), Some(ymd(1900, 2, 28)));
        assert_eq!(from_spreadsheet_serial(60.0, false), None, "29 Feb 1900 does not exist");
        assert_eq!(from_spreadsheet_serial(61.0, false), Some(ymd(1900, 3, 1)));
        assert_eq!(from_spreadsheet_serial(25_569.0, false), Some(0.0));
        assert_eq!(from_spreadsheet_serial(46_308.5, false), Some(ymd(2026, 10, 13) + 43_200.0));
        assert_eq!(from_spreadsheet_serial(-1.0, false), None);
        // 1904 system.
        assert_eq!(from_spreadsheet_serial(0.0, true), Some(ymd(1904, 1, 1)));
        assert_eq!(from_spreadsheet_serial(1462.0, true), Some(ymd(1908, 1, 2)));
        // Back again.
        for s in [1.0, 59.0, 61.0, 25_569.0, 46_308.75] {
            assert_eq!(to_spreadsheet_serial(from_spreadsheet_serial(s, false).unwrap()), Some(s));
        }
        assert_eq!(to_spreadsheet_serial(ymd(1899, 12, 31)), None);
        // Fractions of a day round to the millisecond.
        let serial = 46_308.0 + 57_478.0 / 86_400.0;
        assert_eq!(from_spreadsheet_serial(serial, false), Some(ymd(2026, 10, 13) + 57_478.0));
        assert_eq!(from_spreadsheet_serial(46_308.665_254_629_6, false), Some(ymd(2026, 10, 13) + 57_478.0));
    }
}
