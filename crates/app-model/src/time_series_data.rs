//! Equally spaced series taken from a data table, shared by every analysis
//! of the Time Series menu.
//!
//! **Rows.** Rows are taken in table order. Leading and trailing rows that
//! are excluded or have a missing value in any of the series columns are
//! dropped; an excluded or missing row between the first and last used rows
//! is a gap, which is an error (the methods need equally spaced, complete
//! data).
//!
//! **Time column.** An optional date-time or numeric column labels the time
//! axis and the time stamps of forecasts; it is not used in the computations.
//! Without one (or when a used row has no time value) observations are
//! numbered 1, 2, …. The stamps must run in time order: when they strictly
//! decrease down the table (a newest-first export) the rows are taken in
//! reverse and the row description says so; any other order (or a repeated
//! stamp) is an error naming the first row out of order.
//!
//! **Seasonal period.** The number of observations per cycle. By default it
//! follows the time column's spacing: monthly dates give 12, quarterly 4,
//! weekly 52, daily 7, hourly 24; a numeric time column in steps of 1/12 or
//! 1/4 gives 12 or 4; anything else gives 1 (no seasonality). The request can
//! override it.

use stat_core::{Result, StatError};
use stat_data::temporal::{civil_from_days, days_from_civil, days_in_month};
use stat_data::{DataTable, JsonValue, RowStateManager, TemporalDisplay, TemporalFormat};

use crate::report::{Polyline, RowPoint};

const SECONDS_PER_DAY: f64 = 86_400.0;

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// The columns and period of one series: the series column, an optional time
/// column and an optional seasonal period (`None`: from the time column).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeriesInput {
    /// The numeric series column.
    pub column: usize,
    /// Date-time or numeric column labelling the observations (`None`:
    /// observation numbers).
    pub time: Option<usize>,
    /// Observations per seasonal cycle (`None`: from the time column's
    /// spacing, see [`default_period`]).
    pub period: Option<usize>,
}

impl SeriesInput {
    /// Series `column` with observation numbers and the default period.
    #[must_use]
    pub fn new(column: usize) -> Self {
        Self { column, time: None, period: None }
    }

    /// Every column the input reads.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        std::iter::once(self.column).chain(self.time).collect()
    }

    /// Pass every column index through `f`.
    pub fn map_columns(&mut self, f: &impl Fn(usize) -> usize) {
        self.column = f(self.column);
        self.time = self.time.map(f);
    }

    /// Project-file members: `column`, `time` (when set) and `period` (when
    /// set), columns by name.
    #[must_use]
    pub fn json_members(&self, table: &DataTable) -> Vec<(String, JsonValue)> {
        let name = |i: usize| crate::project::s(table.column(i).map_or("", |c| c.name.as_str()));
        let mut m = vec![("column".to_owned(), name(self.column))];
        if let Some(t) = self.time {
            m.push(("time".to_owned(), name(t)));
        }
        if let Some(p) = self.period {
            m.push(("period".to_owned(), JsonValue::number(p as f64)));
        }
        m
    }

    /// Read the members written by [`json_members`](Self::json_members).
    ///
    /// # Errors
    /// A missing `column`, or a column name the table does not have.
    pub fn from_json(v: &JsonValue, table: &DataTable) -> Result<Self> {
        let column = json_column(v, "column", table)?.ok_or_else(|| invalid("missing \"column\""))?;
        Ok(Self {
            column,
            time: json_column(v, "time", table)?,
            period: v.get("period").and_then(JsonValue::as_usize).filter(|&p| p >= 1),
        })
    }
}

/// The column named by `v[key]` (`None` when absent or null).
///
/// # Errors
/// A value that is not a string, or a name the table does not have.
pub(crate) fn json_column(v: &JsonValue, key: &str, table: &DataTable) -> Result<Option<usize>> {
    match v.get(key) {
        None | Some(JsonValue::Null) => Ok(None),
        Some(x) => {
            let name = x.as_str().ok_or_else(|| invalid(format!("{key:?} must be a column name")))?;
            table.find(name).map(Some).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")))
        }
    }
}

/// How consecutive time stamps step, for extending them into the future.
#[derive(Debug, Clone, Copy, PartialEq)]
enum TimeStep {
    /// Calendar months (1 monthly, 3 quarterly, 12 yearly), keeping the day
    /// of the month (or the month's last day).
    Months(i64),
    /// A fixed step in the time column's units (seconds for date-times).
    Fixed(f64),
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    let n = v.len();
    if n == 0 {
        f64::NAN
    } else if n % 2 == 1 {
        v[n / 2]
    } else {
        0.5 * (v[n / 2 - 1] + v[n / 2])
    }
}

fn ymd(seconds: f64) -> (i64, u32, u32) {
    civil_from_days((seconds / SECONDS_PER_DAY).floor() as i64)
}

/// `seconds` moved by `months` calendar months, keeping the time of day and
/// the day of the month (or the last day, when the start is a month's last).
fn add_months(seconds: f64, months: i64) -> f64 {
    let day_start = (seconds / SECONDS_PER_DAY).floor() * SECONDS_PER_DAY;
    let time_of_day = seconds - day_start;
    let (y, m, d) = ymd(seconds);
    let last = d == days_in_month(y, m);
    let total = y * 12 + i64::from(m) - 1 + months;
    let (ny, nm) = (total.div_euclid(12), u32::try_from(total.rem_euclid(12)).unwrap_or(0) + 1);
    let nd = if last { days_in_month(ny, nm) } else { d.min(days_in_month(ny, nm)) };
    days_from_civil(ny, nm, nd) as f64 * SECONDS_PER_DAY + time_of_day
}

/// The step between consecutive stamps `x` and the period it implies.
fn spacing(x: &[f64], temporal: Option<TemporalDisplay>) -> Option<(TimeStep, usize)> {
    if x.len() < 2 {
        return None;
    }
    let diffs: Vec<f64> = x.windows(2).map(|w| w[1] - w[0]).collect();
    let step = median(diffs.clone());
    if !(step.is_finite() && step > 0.0) {
        return None;
    }
    let date = temporal.is_some_and(|t| matches!(t.format, TemporalFormat::Date | TemporalFormat::DateTime));
    if date {
        let days = step / SECONDS_PER_DAY;
        // Calendar months: every step lands the same number of months later.
        for (months, lo, hi, period) in [(1, 28.0, 31.0, 12), (3, 89.0, 92.0, 4), (12, 365.0, 366.0, 1)] {
            if (lo - 0.01..=hi + 0.01).contains(&days)
                && x.windows(2).all(|w| (add_months(w[0], months) - w[1]).abs() < 1.0)
            {
                return Some((TimeStep::Months(months), period));
            }
        }
        let regular = diffs.iter().all(|d| (d - step).abs() <= 1e-6 * step.max(1.0));
        let period = if !regular {
            1
        } else if (days - 7.0).abs() < 1e-9 {
            52
        } else if (days - 1.0).abs() < 1e-9 {
            7
        } else if (step - 3600.0).abs() < 1e-6 {
            24
        } else {
            1
        };
        return Some((TimeStep::Fixed(step), period));
    }
    let close = |a: f64, b: f64| (a - b).abs() <= 1e-6 * b;
    let period = if temporal.is_some() && close(step, 3600.0) {
        24
    } else if temporal.is_none() && close(step, 1.0 / 12.0) {
        12
    } else if temporal.is_none() && close(step, 0.25) {
        4
    } else {
        1
    };
    Some((TimeStep::Fixed(step), period))
}

/// Seasonal period implied by the time column's spacing over the rows that
/// have a value: 12 for monthly dates, 4 quarterly, 52 weekly, 7 daily, 24
/// hourly; 12 or 4 for a numeric column in steps of 1/12 or 1/4; otherwise
/// (or with no time column) 1.
#[must_use]
pub fn default_period(table: &DataTable, time: Option<usize>) -> usize {
    let Some(c) = time.and_then(|t| table.column(t)) else { return 1 };
    let Some(values) = c.as_numeric() else { return 1 };
    let x: Vec<f64> = values.iter().copied().filter(|v| v.is_finite()).collect();
    spacing(&x, c.temporal()).map_or(1, |s| s.1)
}

/// One equally spaced series, with the table rows and time stamps of its
/// observations.
#[derive(Debug, Clone, PartialEq)]
pub struct Series {
    /// The series column's name.
    pub name: String,
    /// The observations, in time order.
    pub values: Vec<f64>,
    /// Table row of each observation.
    pub rows: Vec<usize>,
    /// Horizontal coordinate of each observation: the time column's value, or
    /// the observation number (1, 2, …).
    pub x: Vec<f64>,
    /// Horizontal axis label (the time column's name, or "Observation").
    pub x_label: String,
    /// Date-time display of the time column (`None` for numbers).
    pub x_time: Option<TemporalDisplay>,
    /// Observations per seasonal cycle.
    pub period: usize,
    /// Whether the period came from the time column (not the request).
    pub period_from_time: bool,
    /// Leading and trailing rows left out (excluded or missing).
    pub n_trimmed: usize,
    /// Whether the rows were taken in reverse table order because the time
    /// column decreases down the table.
    pub reversed: bool,
    step: Option<TimeStep>,
}

impl Series {
    /// The series of `input` in `table` (see the module documentation).
    ///
    /// # Errors
    /// A missing or non-numeric column, no usable rows, or a gap.
    pub fn extract(table: &DataTable, states: &RowStateManager, input: &SeriesInput) -> Result<Self> {
        Ok(Self::extract_aligned(table, states, &[input.column], input.time, input.period)?.remove(0))
    }

    /// Several series over the same rows: rows are dropped at either end
    /// while any column is missing there, and a gap in any column is an error.
    ///
    /// # Errors
    /// No columns, a missing or non-numeric column, no usable rows, a gap, or
    /// time stamps that neither strictly increase nor strictly decrease.
    pub fn extract_aligned(
        table: &DataTable,
        states: &RowStateManager,
        columns: &[usize],
        time: Option<usize>,
        period: Option<usize>,
    ) -> Result<Vec<Self>> {
        if columns.is_empty() {
            return Err(invalid("choose a series column"));
        }
        let mut data = Vec::new();
        for &c in columns {
            let col = table.column(c).ok_or_else(|| invalid(format!("no column {c}")))?;
            let values = col.as_numeric().ok_or_else(|| invalid(format!("{} must be numeric", col.name)))?;
            if col.temporal().is_some() {
                return Err(invalid(format!("{} holds dates or times; choose a numeric series", col.name)));
            }
            data.push((col.name.clone(), values));
        }
        let usable = |r: usize| !states.is_excluded(r) && data.iter().all(|(_, v)| v[r].is_finite());
        let n = table.n_rows();
        let Some(first) = (0..n).find(|&r| usable(r)) else {
            return Err(invalid("no rows with a value (rows that are excluded or missing are left out)"));
        };
        let last = (0..n).rev().find(|&r| usable(r)).unwrap_or(first);
        if let Some(gap) = (first..=last).find(|&r| !usable(r)) {
            let why = if states.is_excluded(gap) { "is excluded" } else { "has a missing value" };
            return Err(invalid(format!(
                "the series has a gap at row {} (the row {why}); time series methods need equally spaced, \
                 complete data",
                gap + 1
            )));
        }
        let mut rows: Vec<usize> = (first..=last).collect();
        let time_col = time.and_then(|t| table.column(t));
        let mut stamps: Option<(Vec<f64>, Option<TemporalDisplay>, String)> = time_col.and_then(|c| {
            let v = c.as_numeric()?;
            let x: Vec<f64> = rows.iter().map(|&r| v[r]).collect();
            x.iter().all(|t| t.is_finite()).then(|| (x, c.temporal(), c.name.clone()))
        });
        let mut reversed = false;
        if let Some((x, temporal, name)) = &mut stamps {
            if x.len() >= 2 && x.windows(2).all(|w| w[1] < w[0]) {
                // A newest-first table: take the rows in reverse.
                x.reverse();
                rows.reverse();
                reversed = true;
            } else if let Some(k) = x.windows(2).position(|w| w[1] <= w[0]) {
                let text = |v: f64| temporal.map_or_else(|| crate::format::format_number(v, 10), |t| t.text(v));
                return Err(invalid(format!(
                    "the time column {name} is not in time order at row {} ({} does not come after {} in row {}); \
                     sort the table by {name} first",
                    rows[k + 1] + 1,
                    text(x[k + 1]),
                    text(x[k]),
                    rows[k] + 1
                )));
            }
        }
        let (x, x_time, x_label, step, implied) = match stamps {
            Some((x, temporal, name)) => {
                let s = spacing(&x, temporal);
                (x, temporal, name, s.map(|s| s.0), s.map_or(1, |s| s.1))
            }
            None => ((1..=rows.len()).map(|i| i as f64).collect(), None, "Observation".to_owned(), None, 1),
        };
        let resolved = period.unwrap_or(implied).max(1);
        Ok(data
            .into_iter()
            .map(|(name, v)| Self {
                name,
                values: rows.iter().map(|&r| v[r]).collect(),
                rows: rows.clone(),
                x: x.clone(),
                x_label: x_label.clone(),
                x_time,
                period: resolved,
                period_from_time: period.is_none(),
                n_trimmed: n - rows.len(),
                reversed,
                step,
            })
            .collect())
    }

    /// Number of observations.
    #[must_use]
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// `true` for a series with no observations (never, once extracted).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Horizontal coordinates of the `h` periods after the last observation:
    /// calendar months for monthly, quarterly or yearly dates, otherwise the
    /// typical spacing (1 for observation numbers).
    #[must_use]
    pub fn future_x(&self, h: usize) -> Vec<f64> {
        let last = self.x.last().copied().unwrap_or(0.0);
        (1..=h)
            .map(|k| match self.step {
                Some(TimeStep::Months(m)) => add_months(last, m * k as i64),
                Some(TimeStep::Fixed(s)) => last + s * k as f64,
                None => last + k as f64,
            })
            .collect()
    }

    /// Text of a horizontal coordinate: the date or time, or the number.
    #[must_use]
    pub fn x_text(&self, x: f64) -> String {
        match self.x_time {
            Some(t) => t.text(x),
            None => crate::format::format_number(x, 10),
        }
    }

    /// Name of position `j` (0-based, position 0 being the first
    /// observation's) in the seasonal cycle: the month or quarter for
    /// monthly or quarterly dates, else "Season j+1".
    #[must_use]
    pub fn season_label(&self, j: usize) -> String {
        const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
        match (self.step, self.x.get(j)) {
            (Some(TimeStep::Months(1)), Some(&x)) if self.period == 12 => MONTHS[ymd(x).1 as usize - 1].to_owned(),
            (Some(TimeStep::Months(3)), Some(&x)) if self.period == 4 => format!("Q{}", (ymd(x).1 - 1) / 3 + 1),
            _ => format!("Season {}", j + 1),
        }
    }

    /// Brushable points `(x, value)` of `values` (parallel to the
    /// observations; non-finite values are left out).
    #[must_use]
    pub fn points(&self, values: &[f64]) -> Vec<RowPoint> {
        self.rows
            .iter()
            .zip(&self.x)
            .zip(values)
            .filter(|(_, v)| v.is_finite())
            .map(|((&row, &x), &y)| RowPoint { row, x, y })
            .collect()
    }

    /// Polyline through `values` (parallel to the observations; non-finite
    /// values are left out).
    #[must_use]
    pub fn polyline(&self, values: &[f64]) -> Polyline {
        self.x.iter().zip(values).filter(|(_, v)| v.is_finite()).map(|(&x, &y)| [x, y]).collect()
    }

    /// One value per table row (`n_rows` rows): `values` at the series'
    /// rows, NaN elsewhere, for saving as a column.
    #[must_use]
    pub fn to_column(&self, values: &[f64], n_rows: usize) -> Vec<f64> {
        let mut out = vec![f64::NAN; n_rows];
        for (&r, &v) in self.rows.iter().zip(values) {
            if r < n_rows {
                out[r] = v;
            }
        }
        out
    }

    /// A note on the period and rows used, for report headings.
    #[must_use]
    pub fn describe_rows(&self) -> String {
        let mut text = format!("{} observations", self.len());
        if self.period > 1 {
            let source = if self.period_from_time { "from the time column" } else { "as set" };
            text.push_str(&format!(", {} per seasonal cycle ({source})", self.period));
        }
        if self.n_trimmed > 0 {
            text.push_str(&format!(
                "; {} leading or trailing {} left out (excluded or missing)",
                self.n_trimmed,
                if self.n_trimmed == 1 { "row" } else { "rows" }
            ));
        }
        if self.reversed {
            text.push_str("; rows taken from the bottom of the table up (the time column runs backwards)");
        }
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    fn monthly(n: usize) -> Vec<f64> {
        (0..n).map(|k| add_months(days_from_civil(2020, 1, 1) as f64 * SECONDS_PER_DAY, k as i64)).collect()
    }

    #[test]
    fn period_follows_the_time_column() {
        let dates = Column::date_time("Month", monthly(24), TemporalDisplay::new(TemporalFormat::Date));
        let quarters = Column::date_time(
            "Quarter",
            (0..24).map(|k| add_months(0.0, 3 * k)).collect(),
            TemporalDisplay::new(TemporalFormat::Date),
        );
        let weeks = Column::date_time(
            "Week",
            (0..24).map(|k| k as f64 * 7.0 * SECONDS_PER_DAY).collect(),
            TemporalDisplay::new(TemporalFormat::Date),
        );
        let decimal = Column::numeric("Year", (0..24).map(|k| 2000.0 + k as f64 / 12.0).collect());
        let plain = Column::numeric("t", (0..24).map(|k| k as f64).collect());
        let y = Column::numeric("y", (0..24).map(|k| k as f64).collect());
        let t = DataTable::new("t", vec![dates, quarters, weeks, decimal, plain, y]).unwrap();
        assert_eq!(default_period(&t, Some(0)), 12);
        assert_eq!(default_period(&t, Some(1)), 4);
        assert_eq!(default_period(&t, Some(2)), 52);
        assert_eq!(default_period(&t, Some(3)), 12);
        assert_eq!(default_period(&t, Some(4)), 1);
        assert_eq!(default_period(&t, None), 1);
        let s = Series::extract(&t, &RowStateManager::new(24), &SeriesInput { column: 5, time: Some(0), period: None })
            .unwrap();
        assert_eq!(s.period, 12);
        assert_eq!(s.season_label(0), "Jan");
        assert_eq!(s.season_label(13), "Feb");
        // Forecast stamps continue month by month.
        let next = s.future_x(2);
        assert_eq!(ymd(next[0]), (2022, 1, 1));
        assert_eq!(ymd(next[1]), (2022, 2, 1));
    }

    #[test]
    fn month_ends_stay_month_ends() {
        let jan31 = days_from_civil(2021, 1, 31) as f64 * SECONDS_PER_DAY;
        assert_eq!(ymd(add_months(jan31, 1)), (2021, 2, 28));
        let feb28 = days_from_civil(2021, 2, 28) as f64 * SECONDS_PER_DAY;
        assert_eq!(ymd(add_months(feb28, 1)), (2021, 3, 31));
    }

    #[test]
    fn ends_are_trimmed_and_gaps_refused() {
        let y = Column::numeric("y", vec![f64::NAN, 1.0, 2.0, 3.0, 4.0, f64::NAN]);
        let x = Column::numeric("x", vec![1.0, 2.0, 3.0, f64::NAN, 5.0, 6.0]);
        let t = DataTable::new("t", vec![y, x]).unwrap();
        let mut rows = RowStateManager::new(6);
        let s = Series::extract(&t, &rows, &SeriesInput::new(0)).unwrap();
        assert_eq!(s.values, [1.0, 2.0, 3.0, 4.0]);
        assert_eq!(s.rows, [1, 2, 3, 4]);
        assert_eq!(s.x, [1.0, 2.0, 3.0, 4.0]);
        assert_eq!(s.n_trimmed, 2);
        rows.set_excluded([1], true);
        assert_eq!(Series::extract(&t, &rows, &SeriesInput::new(0)).unwrap().values, [2.0, 3.0, 4.0]);
        rows.set_excluded([2], true);
        rows.set_excluded([1], false);
        let e = Series::extract(&t, &rows, &SeriesInput::new(0)).unwrap_err().to_string();
        assert!(e.contains("gap at row 3"), "{e}");
        // Aligned: the second column's gap at row 4 is inside the common range.
        let e = Series::extract_aligned(&t, &RowStateManager::new(6), &[0, 1], None, None).unwrap_err();
        assert!(e.to_string().contains("gap at row 4"), "{e}");
        // A time column with a missing stamp falls back to observation numbers.
        let s = Series::extract(&t, &RowStateManager::new(6), &SeriesInput { column: 0, time: Some(1), period: None })
            .unwrap();
        assert_eq!(s.x_label, "Observation");
    }

    #[test]
    fn a_newest_first_series_is_reversed_and_other_orders_refused() {
        let display = TemporalDisplay::new(TemporalFormat::Date);
        let month = |m: u32| days_from_civil(2024, m, 1) as f64 * SECONDS_PER_DAY;
        let t = DataTable::new(
            "t",
            vec![
                Column::date_time("Month", vec![month(3), month(2), month(1)], display),
                Column::numeric("y", vec![3.0, 2.0, 1.0]),
            ],
        )
        .unwrap();
        let input = SeriesInput { column: 1, time: Some(0), period: None };
        let s = Series::extract(&t, &RowStateManager::new(3), &input).unwrap();
        assert_eq!(s.values, [1.0, 2.0, 3.0]);
        assert_eq!(s.rows, [2, 1, 0]);
        assert_eq!(s.x, [month(1), month(2), month(3)]);
        assert!(s.reversed);
        assert!(s.describe_rows().contains("time column runs backwards"), "{}", s.describe_rows());
        // Monthly step: the next stamp is 2024-04-01.
        assert_eq!(ymd(s.future_x(1)[0]), (2024, 4, 1));

        let mixed = DataTable::new(
            "t",
            vec![
                Column::date_time("Month", vec![month(1), month(3), month(2)], display),
                Column::numeric("y", vec![1.0, 3.0, 2.0]),
            ],
        )
        .unwrap();
        let e = Series::extract(&mixed, &RowStateManager::new(3), &input).unwrap_err().to_string();
        assert!(e.contains("not in time order at row 3") && e.contains("2024-02-01"), "{e}");
    }

    #[test]
    fn json_round_trip() {
        let t =
            DataTable::new("t", vec![Column::numeric("y", vec![1.0, 2.0]), Column::numeric("time", vec![1.0, 2.0])])
                .unwrap();
        let input = SeriesInput { column: 0, time: Some(1), period: Some(4) };
        let v = JsonValue::Object(input.json_members(&t));
        assert_eq!(SeriesInput::from_json(&v, &t).unwrap(), input);
        let plain = SeriesInput::new(1);
        assert_eq!(SeriesInput::from_json(&JsonValue::Object(plain.json_members(&t)), &t).unwrap(), plain);
    }
}
