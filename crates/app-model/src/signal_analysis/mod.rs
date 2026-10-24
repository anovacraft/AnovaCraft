//! Signal processing reports (Series and Signals ▸ Signals and spectra), bound to the row state
//! ([`stat_signal`]).
//!
//! **Signals.** A numeric column, read in table order or ordered by an x
//! column (numeric or date-time). Rows that are excluded or missing are left
//! out. When the x values (or, without an x column, the row positions of the
//! remaining rows) are not evenly spaced, the signal is resampled onto an
//! even grid at the median spacing by linear interpolation, and the summary
//! says so. With a group column, each level is a separate signal (long
//! format) analysed the same way; plots overlay the groups and tables list
//! each.
//!
//! **Sampling rate.** With an x column, one sample per median x spacing
//! (frequencies are in cycles per unit of x; per second for date-times).
//! Without one, the rate given in the request (default 1: cycles per
//! sample).
//!
//! Four analyses share the input ([`SignalMethod`]):
//!
//! * [`spectrum`] — amplitude, phase and power spectra with windows,
//!   detrending and zero padding, dominant frequencies, Welch's averaged
//!   periodogram and a spectrogram.
//! * [`filtering`] — IIR (Butterworth, Chebyshev, elliptic), windowed-sinc
//!   FIR, FFT and notch filters, applied forwards or with zero phase, with
//!   frequency and phase responses; filtered signal saved as a column.
//! * [`correlation`] — cross- and autocorrelation, convolution with a kernel
//!   column, Wiener and Richardson–Lucy deconvolution, and the envelope and
//!   instantaneous phase from the Hilbert transform.
//! * [`wavelets`] — multilevel discrete wavelet decomposition, wavelet
//!   denoising and a continuous-wavelet scalogram.

pub mod correlation;
pub mod filtering;
pub mod spectrum;
pub mod wavelets;

#[cfg(test)]
mod tests;

pub use correlation::{CorrelationOperation, CorrelationSettings, DeconvolutionMethod};
pub use filtering::{BandKind, DesignedFilter, FilterDesign, FilterSettings};
pub use spectrum::SpectrumSettings;
pub use wavelets::WaveletSettings;

use stat_core::{Result, StatError};
use stat_data::{DataTable, JsonValue, RowStateManager, TemporalDisplay};
use stat_signal::window::Window;

use crate::format::format_number;
use crate::project::s;
use crate::report::{PlotSpec, PointGroups, Polyline, ReportSection, RowPoint, SectionContent, SectionKind, TextTable};
use crate::report_columns::ReportColumn;
use crate::time_series_data::json_column;

/// A usable row of a signal: `(row, x, value)`.
type RowSample = (usize, f64, f64);

/// A per-group column transform used by the formula functions.
type ColumnTransform = dyn Fn(&[f64]) -> std::result::Result<Vec<f64>, String>;

pub(crate) fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

pub(crate) fn section(kind: SectionKind, title: &str, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.to_owned(), content }
}

/// Most groups drawn in one overlaid plot (tables list them all).
pub const MAX_PLOTTED_GROUPS: usize = 12;

/// The signal columns and sampling of a signal report.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SignalInput {
    /// The numeric signal column.
    pub column: usize,
    /// Numeric or date-time column giving each sample's position (`None`:
    /// row order).
    pub x: Option<usize>,
    /// Column whose levels are separate signals (`None`: one signal).
    pub group: Option<usize>,
    /// Samples per unit of time when there is no x column (`None`: 1).
    pub sampling_rate: Option<f64>,
}

impl SignalInput {
    /// Signal `column` in row order at one sample per unit.
    #[must_use]
    pub fn new(column: usize) -> Self {
        Self { column, x: None, group: None, sampling_rate: None }
    }

    /// Every column the input reads.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        std::iter::once(self.column).chain(self.x).chain(self.group).collect()
    }

    /// Pass every column index through `f`.
    pub fn map_columns(&mut self, f: &impl Fn(usize) -> usize) {
        self.column = f(self.column);
        self.x = self.x.map(f);
        self.group = self.group.map(f);
    }

    fn json_members(&self, table: &DataTable) -> Vec<(String, JsonValue)> {
        let name = |i: usize| s(table.column(i).map_or("", |c| c.name.as_str()));
        let mut m = vec![("column".to_owned(), name(self.column))];
        if let Some(x) = self.x {
            m.push(("x".to_owned(), name(x)));
        }
        if let Some(g) = self.group {
            m.push(("group".to_owned(), name(g)));
        }
        if let Some(r) = self.sampling_rate {
            m.push(("sampling_rate".to_owned(), JsonValue::number(r)));
        }
        m
    }

    fn from_json(v: &JsonValue, table: &DataTable) -> Result<Self> {
        let column = json_column(v, "column", table)?.ok_or_else(|| invalid("the signal report needs a \"column\""))?;
        Ok(Self {
            column,
            x: json_column(v, "x", table)?,
            group: json_column(v, "group", table)?,
            sampling_rate: v.get("sampling_rate").and_then(JsonValue::as_f64),
        })
    }
}

/// One evenly spaced signal taken from the table.
#[derive(Debug, Clone)]
pub struct Trace {
    /// Group level (empty without a group column).
    pub label: String,
    /// Samples on the even grid.
    pub values: Vec<f64>,
    /// Time (x) of each grid sample.
    pub time: Vec<f64>,
    /// Table rows used, in signal order.
    pub rows: Vec<usize>,
    /// Their x positions (time units).
    pub row_time: Vec<f64>,
    /// Their values.
    pub row_values: Vec<f64>,
    /// Whether the rows were resampled onto an even grid.
    pub resampled: bool,
}

impl Trace {
    /// One value per table row from one value per grid sample: the grid
    /// value at each used row's position (interpolated when resampled), NaN
    /// on other rows.
    #[must_use]
    pub fn per_row(&self, grid_values: &[f64], n_rows: usize) -> Vec<f64> {
        let mut out = vec![f64::NAN; n_rows];
        self.fill_rows(grid_values, &mut out);
        out
    }

    /// Write [`Trace::per_row`] values into `out`.
    pub fn fill_rows(&self, grid_values: &[f64], out: &mut [f64]) {
        if self.resampled {
            for (&r, &t) in self.rows.iter().zip(&self.row_time) {
                out[r] = interpolate(&self.time, grid_values, t);
            }
        } else {
            for (&r, &v) in self.rows.iter().zip(grid_values) {
                out[r] = v;
            }
        }
    }
}

/// Linear interpolation of `(xs, ys)` (xs increasing, evenly spaced) at `x`.
fn interpolate(xs: &[f64], ys: &[f64], x: f64) -> f64 {
    let n = xs.len().min(ys.len());
    if n == 0 {
        return f64::NAN;
    }
    if n == 1 || x <= xs[0] {
        return ys[0];
    }
    if x >= xs[n - 1] {
        return ys[n - 1];
    }
    let k = xs.partition_point(|&v| v <= x).clamp(1, n - 1);
    let (x0, x1) = (xs[k - 1], xs[k]);
    let f = if x1 > x0 { (x - x0) / (x1 - x0) } else { 0.0 };
    ys[k - 1] + f * (ys[k] - ys[k - 1])
}

/// The signals of a request, with their shared sampling.
#[derive(Debug, Clone)]
pub struct Signals {
    /// Name of the signal column.
    pub name: String,
    /// Name of the time axis ("Sample" without an x column).
    pub time_label: String,
    /// Date-time display of the x column, if any.
    pub time_display: Option<TemporalDisplay>,
    /// Name of the group column, if any.
    pub group_name: Option<String>,
    /// One trace per group (one without groups).
    pub traces: Vec<Trace>,
    /// Samples per unit of time.
    pub sampling_rate: f64,
    /// Unit of frequency for labels, e.g. "cycles per second".
    pub frequency_unit: String,
    /// Rows left out because they are excluded or missing.
    pub left_out: usize,
    /// Rows of the table.
    pub n_rows: usize,
}

impl Signals {
    /// Read `column` (and the input's x and group columns).
    ///
    /// # Errors
    /// A missing or non-numeric column, an invalid sampling rate, repeated x
    /// values within a signal, or a signal with fewer than `min` samples.
    pub fn extract(table: &DataTable, states: &RowStateManager, input: &SignalInput, min: usize) -> Result<Self> {
        Self::extract_column(table, states, input, input.column, min)
    }

    /// As [`Signals::extract`], for another column read with the same x and
    /// groups (a second signal of a correlation).
    ///
    /// # Errors
    /// As [`Signals::extract`].
    pub fn extract_column(
        table: &DataTable,
        states: &RowStateManager,
        input: &SignalInput,
        column: usize,
        min: usize,
    ) -> Result<Self> {
        let col = table.column(column).ok_or_else(|| invalid(format!("no column {column}")))?;
        let values = col.as_numeric().ok_or_else(|| invalid(format!("{} must be numeric", col.name)))?;
        if col.temporal().is_some() {
            return Err(invalid(format!("{} holds dates or times; choose a numeric signal", col.name)));
        }
        let x_col = match input.x {
            Some(x) => {
                let c = table.column(x).ok_or_else(|| invalid(format!("no column {x}")))?;
                let v = c.as_numeric().ok_or_else(|| invalid(format!("{} must be numeric or dates", c.name)))?;
                Some((c.name.clone(), v, c.temporal()))
            }
            None => None,
        };
        let groups: Vec<String> = match input.group {
            Some(g) => {
                let c = table.column(g).ok_or_else(|| invalid(format!("no column {g}")))?;
                (0..table.n_rows()).map(|r| c.level_text(r)).collect()
            }
            None => vec![String::new(); table.n_rows()],
        };
        let group_name = input.group.and_then(|g| table.column(g)).map(|c| c.name.clone());
        let mut order: Vec<String> = Vec::new();
        for (r, g) in groups.iter().enumerate() {
            if !states.is_excluded(r) && !order.contains(g) && !(input.group.is_some() && g.trim().is_empty()) {
                order.push(g.clone());
            }
        }
        let mut left_out = 0;
        let mut raw: Vec<(String, Vec<RowSample>)> = order.into_iter().map(|g| (g, Vec::new())).collect();
        for r in 0..table.n_rows() {
            let x = x_col.as_ref().map_or(r as f64, |(_, v, _)| v[r]);
            let usable = !states.is_excluded(r) && values[r].is_finite() && x.is_finite();
            match raw.iter_mut().find(|(g, _)| *g == groups[r]) {
                Some((_, pts)) if usable => pts.push((r, x, values[r])),
                _ => left_out += 1,
            }
        }
        raw.retain(|(_, pts)| !pts.is_empty());
        if raw.is_empty() {
            return Err(invalid("no rows with a value (rows that are excluded or missing are left out)"));
        }
        // Median spacing over all signals sets the grid step.
        let mut steps = Vec::new();
        for (_, pts) in &mut raw {
            pts.sort_by(|a, b| a.1.total_cmp(&b.1));
            for w in pts.windows(2) {
                let d = w[1].1 - w[0].1;
                if d <= 0.0 {
                    return Err(invalid(format!(
                        "rows {} and {} have the same position; each sample needs its own x value",
                        w[0].0 + 1,
                        w[1].0 + 1
                    )));
                }
                steps.push(d);
            }
        }
        steps.sort_by(f64::total_cmp);
        let step = if steps.is_empty() { 1.0 } else { steps[steps.len() / 2] };
        // Positions are x values, or row numbers without an x column (one
        // row is 1 / rate units of time).
        let (time_label, time_display, scale, unit) = match &x_col {
            Some((name, _, display)) => {
                let unit = if display.is_some() {
                    "cycles per second".to_owned()
                } else {
                    format!("cycles per unit of {name}")
                };
                (name.clone(), *display, 1.0, unit)
            }
            None => {
                let rate = input.sampling_rate.unwrap_or(1.0);
                if !(rate.is_finite() && rate > 0.0) {
                    return Err(invalid("the sampling rate must be a positive number"));
                }
                let unit = if input.sampling_rate.is_some() { "cycles per unit of time" } else { "cycles per sample" };
                (
                    if input.sampling_rate.is_some() { "Time" } else { "Sample" }.to_owned(),
                    None,
                    1.0 / rate,
                    unit.to_owned(),
                )
            }
        };
        let rate = 1.0 / (step * scale);
        let to_time = |x: f64| x * scale;
        let mut traces = Vec::with_capacity(raw.len());
        for (label, pts) in raw {
            let row_time: Vec<f64> = pts.iter().map(|p| to_time(p.1)).collect();
            let row_values: Vec<f64> = pts.iter().map(|p| p.2).collect();
            let rows: Vec<usize> = pts.iter().map(|p| p.0).collect();
            let even = pts.windows(2).all(|w| ((w[1].1 - w[0].1) - step).abs() <= 1e-6 * step);
            let (time, values) = if even {
                (row_time.clone(), row_values.clone())
            } else {
                let t0 = row_time[0];
                let dt = 1.0 / rate;
                let span = row_time[row_time.len() - 1] - t0;
                let count = (span / dt + 1e-9).floor() as usize + 1;
                let grid: Vec<f64> = (0..count).map(|k| t0 + k as f64 * dt).collect();
                let v = grid.iter().map(|&t| interpolate(&row_time, &row_values, t)).collect();
                (grid, v)
            };
            if values.len() < min {
                let which = if label.is_empty() { String::new() } else { format!(" (group {label})") };
                return Err(StatError::InsufficientData { required: min, found: values.len() })
                    .map_err(|e| invalid(format!("the signal{which} is too short: {e}")));
            }
            traces.push(Trace { label, values, time, rows, row_time, row_values, resampled: !even });
        }
        Ok(Self {
            name: col.name.clone(),
            time_label,
            time_display,
            group_name,
            traces,
            sampling_rate: rate,
            frequency_unit: unit,
            left_out,
            n_rows: table.n_rows(),
        })
    }

    /// Frequency axis title.
    #[must_use]
    pub fn frequency_label(&self) -> String {
        format!("Frequency ({})", self.frequency_unit)
    }

    /// Display label of trace `i` ("" without groups).
    #[must_use]
    pub fn trace_label(&self, i: usize) -> String {
        self.traces[i].label.clone()
    }

    /// Summary lines common to every signal report.
    #[must_use]
    pub fn summary_rows(&self) -> Vec<(String, String)> {
        let mut out = vec![("Signal".to_owned(), self.name.clone())];
        if let Some(g) = &self.group_name {
            out.push((format!("Signals (levels of {g})"), self.traces.len().to_string()));
        }
        let lengths: Vec<usize> = self.traces.iter().map(|t| t.values.len()).collect();
        let (lo, hi) = (lengths.iter().min().copied().unwrap_or(0), lengths.iter().max().copied().unwrap_or(0));
        out.push(("Samples".to_owned(), if lo == hi { lo.to_string() } else { format!("{lo} to {hi}") }));
        out.push(("Sampling rate".to_owned(), format!("{} per unit of time", format_number(self.sampling_rate, 6))));
        out.push(("Nyquist frequency".to_owned(), format_number(self.sampling_rate / 2.0, 6)));
        out.push(("Frequency unit".to_owned(), self.frequency_unit.clone()));
        let resampled = self.traces.iter().filter(|t| t.resampled).count();
        if resampled > 0 {
            out.push((
                "Resampled".to_owned(),
                format!(
                    "{resampled} signal{} unevenly spaced: interpolated linearly onto an even grid",
                    if resampled == 1 { " was" } else { "s were" }
                ),
            ));
        }
        if self.left_out > 0 {
            out.push(("Rows left out (excluded or missing)".to_owned(), self.left_out.to_string()));
        }
        out
    }

    /// Group labels of the plotted traces (the first [`MAX_PLOTTED_GROUPS`]).
    fn plotted(&self) -> usize {
        self.traces.len().min(MAX_PLOTTED_GROUPS)
    }

    /// A plot of each trace's rows as points with one line per trace (the
    /// grid values `lines[i]` of trace `i`, against its grid time), named
    /// `line_name`. Without groups the line is the plot's fit line.
    #[must_use]
    pub fn rows_with_lines(&self, lines: &[Vec<f64>], line_name: &str, y_label: &str) -> PlotSpec {
        let k = self.plotted();
        let mut points = Vec::new();
        let mut membership = Vec::new();
        for (g, t) in self.traces.iter().take(k).enumerate() {
            for ((&r, &x), &y) in t.rows.iter().zip(&t.row_time).zip(&t.row_values) {
                points.push(RowPoint { row: r, x, y });
                membership.push(g);
            }
        }
        let polylines: Vec<Polyline> = self
            .traces
            .iter()
            .take(k)
            .zip(lines)
            .map(|(t, v)| t.time.iter().zip(v).filter(|(_, y)| y.is_finite()).map(|(&x, &y)| [x, y]).collect())
            .collect();
        let mut p = PlotSpec {
            x_label: self.time_label.clone(),
            y_label: y_label.to_owned(),
            points,
            x_time: self.time_display,
            point_name: Some(self.name.clone()),
            ..PlotSpec::default()
        };
        if self.group_name.is_some() {
            p.groups = Some(PointGroups {
                title: self.group_name.clone().unwrap_or_default(),
                labels: self.traces.iter().take(k).map(|t| t.label.clone()).collect(),
                membership,
                lines: polylines,
            });
        } else {
            p.line = polylines.into_iter().next();
            p.line_name = Some(line_name.to_owned());
        }
        p
    }

    /// One curve per trace (e.g. spectra) with no rows: `curves[i]` for
    /// trace `i`.
    #[must_use]
    pub fn curves(&self, curves: Vec<Polyline>, x_label: &str, y_label: &str, name: &str) -> PlotSpec {
        curve_plot(self.group_name.as_deref(), &self.traces, curves, x_label, y_label, name)
    }
}

/// Curves without rows, one per trace (grouped when there are groups).
pub(crate) fn curve_plot(
    group_name: Option<&str>,
    traces: &[Trace],
    curves: Vec<Polyline>,
    x_label: &str,
    y_label: &str,
    name: &str,
) -> PlotSpec {
    let mut p = PlotSpec { x_label: x_label.to_owned(), y_label: y_label.to_owned(), ..PlotSpec::default() };
    match group_name {
        Some(title) => {
            let k = curves.len().min(MAX_PLOTTED_GROUPS);
            p.groups = Some(PointGroups {
                title: title.to_owned(),
                labels: traces.iter().take(k).map(|t| t.label.clone()).collect(),
                membership: Vec::new(),
                lines: curves.into_iter().take(k).collect(),
            });
        }
        None => {
            p.line = curves.into_iter().next();
            p.line_name = Some(name.to_owned());
        }
    }
    p
}

/// What a signal report does with its signals.
#[derive(Debug, Clone, PartialEq)]
pub enum SignalMethod {
    /// Fourier spectra.
    Spectrum(SpectrumSettings),
    /// Digital filtering.
    Filter(FilterSettings),
    /// Convolution, correlation, deconvolution and envelopes.
    Correlation(CorrelationSettings),
    /// Wavelet decomposition, denoising and scalogram.
    Wavelet(WaveletSettings),
}

impl SignalMethod {
    /// Project-file analysis name.
    #[must_use]
    pub fn key(&self) -> &'static str {
        match self {
            Self::Spectrum(_) => "fourier-spectrum",
            Self::Filter(_) => "digital-filter",
            Self::Correlation(_) => "signal-correlation",
            Self::Wavelet(_) => "wavelet-analysis",
        }
    }

    /// The analysis names this module reads.
    pub const KEYS: [&'static str; 4] =
        ["fourier-spectrum", "digital-filter", "signal-correlation", "wavelet-analysis"];
}

/// A Series and Signals ▸ Signals and spectra request: the signal and the analysis.
#[derive(Debug, Clone, PartialEq)]
pub struct SignalProcessingRequest {
    /// The signal.
    pub input: SignalInput,
    /// What to do with it.
    pub method: SignalMethod,
}

impl SignalProcessingRequest {
    /// A Fourier spectrum of `column` with default settings.
    #[must_use]
    pub fn spectrum(column: usize) -> Self {
        Self { input: SignalInput::new(column), method: SignalMethod::Spectrum(SpectrumSettings::default()) }
    }

    /// Every column the request reads.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        let mut out = self.input.columns();
        if let SignalMethod::Correlation(c) = &self.method {
            out.extend(c.operation.other_column());
        }
        out
    }

    /// Pass every column index through `f`.
    pub fn map_columns(&mut self, f: &impl Fn(usize) -> usize) {
        self.input.map_columns(f);
        if let SignalMethod::Correlation(c) = &mut self.method {
            c.operation.map_column(f);
        }
    }

    /// The project-file (JSON) form, columns by name.
    #[must_use]
    pub fn to_json(&self, table: &DataTable) -> JsonValue {
        let mut m: Vec<(String, JsonValue)> = vec![("analysis".into(), s(self.method.key()))];
        m.extend(self.input.json_members(table));
        match &self.method {
            SignalMethod::Spectrum(x) => m.extend(x.json_members()),
            SignalMethod::Filter(x) => m.extend(x.json_members()),
            SignalMethod::Correlation(x) => m.extend(x.json_members(table)),
            SignalMethod::Wavelet(x) => m.extend(x.json_members()),
        }
        JsonValue::Object(m)
    }

    /// Read the JSON form of analysis `analysis` (one of
    /// [`SignalMethod::KEYS`]); settings left out take their defaults.
    ///
    /// # Errors
    /// Missing or unknown columns, unknown option names.
    pub fn from_json(analysis: &str, v: &JsonValue, table: &DataTable) -> Result<Self> {
        let input = SignalInput::from_json(v, table)?;
        let method = match analysis {
            "fourier-spectrum" => SignalMethod::Spectrum(SpectrumSettings::from_json(v)?),
            "digital-filter" => SignalMethod::Filter(FilterSettings::from_json(v)?),
            "signal-correlation" => SignalMethod::Correlation(CorrelationSettings::from_json(v, table)?),
            "wavelet-analysis" => SignalMethod::Wavelet(WaveletSettings::from_json(v)?),
            other => return Err(invalid(format!("unknown signal analysis {other:?}"))),
        };
        Ok(Self { input, method })
    }
}

/// A computed signal report.
#[derive(Debug, Clone)]
pub struct SignalAnalysis {
    request: SignalProcessingRequest,
    title: String,
    sections: Vec<ReportSection>,
    columns: Vec<ReportColumn>,
}

impl SignalAnalysis {
    /// Read the signals and run the analysis.
    ///
    /// # Errors
    /// Missing or non-numeric columns, too short a signal, or invalid
    /// settings (cut-offs outside the Nyquist range, a kernel longer than
    /// the signal, and so on).
    pub fn run(table: &DataTable, states: &RowStateManager, request: &SignalProcessingRequest) -> Result<Self> {
        let (title, sections, columns) = match &request.method {
            SignalMethod::Spectrum(x) => spectrum::run(table, states, &request.input, x)?,
            SignalMethod::Filter(x) => filtering::run(table, states, &request.input, x)?,
            SignalMethod::Correlation(x) => correlation::run(table, states, &request.input, x)?,
            SignalMethod::Wavelet(x) => wavelets::run(table, states, &request.input, x)?,
        };
        Ok(Self { request: request.clone(), title, sections, columns })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &SignalProcessingRequest {
        &self.request
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        self.title.clone()
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        self.sections.clone()
    }

    /// Columns the report can add (filtered or denoised signals, envelopes,
    /// convolutions).
    #[must_use]
    pub fn saveable_columns(&self) -> Vec<ReportColumn> {
        self.columns.clone()
    }
}

/// A two-column table of `(label, value)` lines.
pub(crate) fn summary_table(rows: Vec<(String, String)>) -> TextTable {
    let mut t = TextTable::new(["", "Value"]);
    for (k, v) in rows {
        t.push([k, v]);
    }
    t
}

// ---------------------------------------------------------------------------
// Windows in project files and dialogs.

/// The windows offered, in menu order (Kaiser and Tukey with their default
/// parameters).
pub const WINDOWS: [Window; 7] = [
    Window::Rectangular,
    Window::Hann,
    Window::Hamming,
    Window::Blackman,
    Window::FlatTop,
    Window::Kaiser { beta: 8.6 },
    Window::Tukey { alpha: 0.25 },
];

/// Project-file key of a window.
#[must_use]
pub fn window_key(w: Window) -> &'static str {
    match w {
        Window::Rectangular => "rectangular",
        Window::Hann => "hann",
        Window::Hamming => "hamming",
        Window::Blackman => "blackman",
        Window::FlatTop => "flat-top",
        Window::Kaiser { .. } => "kaiser",
        Window::Tukey { .. } => "tukey",
    }
}

/// Short menu name of a window (without parameters).
#[must_use]
pub fn window_name(w: Window) -> &'static str {
    match w {
        Window::Rectangular => "Rectangular (none)",
        Window::Hann => "Hann",
        Window::Hamming => "Hamming",
        Window::Blackman => "Blackman",
        Window::FlatTop => "Flat-top",
        Window::Kaiser { .. } => "Kaiser",
        Window::Tukey { .. } => "Tukey",
    }
}

pub(crate) fn window_members(prefix: &str, w: Window) -> Vec<(String, JsonValue)> {
    let mut m = vec![(prefix.to_owned(), s(window_key(w)))];
    match w {
        Window::Kaiser { beta } => m.push((format!("{prefix}_beta"), JsonValue::number(beta))),
        Window::Tukey { alpha } => m.push((format!("{prefix}_alpha"), JsonValue::number(alpha))),
        _ => {}
    }
    m
}

/// The window named by member `prefix` of `v` (with `<prefix>_beta` or
/// `<prefix>_alpha` for Kaiser and Tukey), or `default` when it is absent.
///
/// # Errors
/// An unknown window name.
pub fn window_from_json(v: &JsonValue, prefix: &str, default: Window) -> Result<Window> {
    let Some(key) = v.get(prefix).and_then(JsonValue::as_str) else { return Ok(default) };
    let num = |k: &str, d: f64| v.get(&format!("{prefix}_{k}")).and_then(JsonValue::as_f64).unwrap_or(d);
    Ok(match key {
        "rectangular" => Window::Rectangular,
        "hann" => Window::Hann,
        "hamming" => Window::Hamming,
        "blackman" => Window::Blackman,
        "flat-top" => Window::FlatTop,
        "kaiser" => Window::Kaiser { beta: num("beta", 8.6) },
        "tukey" => Window::Tukey { alpha: num("alpha", 0.25) },
        other => return Err(invalid(format!("unknown window {other:?}"))),
    })
}

pub(crate) fn text<'a>(v: &'a JsonValue, key: &str) -> Option<&'a str> {
    v.get(key).and_then(JsonValue::as_str)
}

pub(crate) fn number(v: &JsonValue, key: &str, default: f64) -> f64 {
    v.get(key).and_then(JsonValue::as_f64).unwrap_or(default)
}

pub(crate) fn count(v: &JsonValue, key: &str, default: usize) -> usize {
    v.get(key).and_then(JsonValue::as_usize).unwrap_or(default)
}

pub(crate) fn flag(v: &JsonValue, key: &str, default: bool) -> bool {
    v.get(key).and_then(JsonValue::as_bool).unwrap_or(default)
}

// ---------------------------------------------------------------------------
// Formula column functions.

/// Apply a signal formula function to `x` within each group (`groups[r]`:
/// group of row r). Missing values are left out of each group's signal
/// (which is taken in row order) and stay missing. `kind` is
/// `lowpass:f`, `highpass:f`, `bandpass:f1:f2`, `bandstop:f1:f2` (fourth-order
/// Butterworth, zero phase, frequencies in cycles per row), `envelope`,
/// `instant_phase` or `wavelet_denoise:name`.
///
/// # Errors
/// An unknown kind, a cut-off outside (0, 0.5), or too short a group.
pub fn formula_transform(kind: &str, x: &[f64], groups: &[usize]) -> std::result::Result<Vec<f64>, String> {
    use stat_signal::filter::{Band, IirFamily, design_iir};
    let parts: Vec<&str> = kind.split(':').collect();
    let freq = |i: usize| -> std::result::Result<f64, String> {
        parts.get(i).and_then(|p| p.parse::<f64>().ok()).ok_or_else(|| format!("{kind}: missing frequency"))
    };
    let transform: Box<ColumnTransform> = match parts[0] {
        "lowpass" | "highpass" | "bandpass" | "bandstop" => {
            let band = match parts[0] {
                "lowpass" => Band::Lowpass(freq(1)?),
                "highpass" => Band::Highpass(freq(1)?),
                "bandpass" => Band::Bandpass(freq(1)?, freq(2)?),
                _ => Band::Bandstop(freq(1)?, freq(2)?),
            };
            let sos = design_iir(IirFamily::Butterworth, 4, band, 1.0).map_err(|e| e.to_string())?;
            Box::new(move |v: &[f64]| sos.filtfilt(v).map_err(|e| e.to_string()))
        }
        "envelope" => {
            Box::new(|v: &[f64]| Ok(stat_signal::convolve::hilbert(v, 1.0).map_err(|e| e.to_string())?.envelope))
        }
        "instant_phase" => {
            Box::new(|v: &[f64]| Ok(stat_signal::convolve::hilbert(v, 1.0).map_err(|e| e.to_string())?.phase))
        }
        "wavelet_denoise" => {
            let w = stat_signal::wavelet::Wavelet::new(parts.get(1).copied().unwrap_or("db4"))
                .map_err(|e| e.to_string())?;
            Box::new(move |v: &[f64]| {
                stat_signal::wavelet::denoise(v, &w, &stat_signal::wavelet::DenoiseOptions::default())
                    .map(|d| d.signal)
                    .map_err(|e| e.to_string())
            })
        }
        other => return Err(format!("unknown signal function {other:?}")),
    };
    let n_groups = groups.iter().max().map_or(0, |m| m + 1);
    let mut out = vec![f64::NAN; x.len()];
    for g in 0..n_groups {
        let rows: Vec<usize> = (0..x.len()).filter(|&r| groups[r] == g && x[r].is_finite()).collect();
        if rows.is_empty() {
            continue;
        }
        let v: Vec<f64> = rows.iter().map(|&r| x[r]).collect();
        let y = transform(&v)?;
        for (&r, y) in rows.iter().zip(y) {
            out[r] = y;
        }
    }
    Ok(out)
}
