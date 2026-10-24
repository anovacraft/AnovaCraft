//! User settings for plot axes, and the tick positions and labels they imply.
//!
//! Each axis has a major tick interval (automatic "nice" steps of 1, 2 or 5 ×
//! 10ᵏ, or a fixed interval from a chosen origin), minor ticks (none,
//! automatic, a number of subdivisions per major interval, or a fixed
//! interval), grid lines, tick marks, a label format, a title, and the side of
//! the plot it is drawn on. [`axis_ticks`] turns the settings and the visible
//! range into tick positions; labels are thinned so they never crowd
//! together, and [`format_tick`] formats them.
//!
//! The look of the axis line and its marks is set here too: tick marks
//! pointing into the plot, out of it or across the axis line, their length
//! and thickness, an axis line moved away from the data area by an offset,
//! ranges widened to the nearest major ticks, ranges typed by the user, and
//! tick labels written by the user (a label in math markup at any value,
//! replacing the number there or adding a tick of its own).

/// Major tick spacing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MajorTicks {
    /// A "nice" step chosen from the visible range and plot size.
    Auto,
    /// A fixed interval in data units.
    Every(f64),
}

/// Minor tick spacing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MinorTicks {
    /// No minor ticks.
    None,
    /// Five subdivisions (four when the major step is 2 × 10ᵏ).
    Auto,
    /// This many equal subdivisions of each major interval (2–100).
    Subdivisions(u32),
    /// A fixed interval in data units (smaller than the major interval).
    Every(f64),
}

/// Which grid lines are drawn across the plot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridLines {
    /// Lines at major and minor ticks.
    MajorAndMinor,
    /// Lines at major ticks only.
    MajorOnly,
    /// No grid lines.
    None,
}

/// How tick labels are written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickFormat {
    /// As many decimals as the major step needs; scientific notation for very
    /// large or small values.
    Auto,
    /// A fixed number of decimals.
    Decimals(u32),
    /// Scientific notation with this many decimals in the mantissa.
    Scientific(u32),
    /// Multiplied by 100 with a percent sign, with this many decimals.
    Percent(u32),
}

/// The side of the plot an axis is drawn on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisSide {
    /// Bottom for the horizontal axis, left for the vertical axis.
    Standard,
    /// Top for the horizontal axis, right for the vertical axis.
    Opposite,
}

/// Which way tick marks point from the axis line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TickDirection {
    /// Into the plot area (the default).
    #[default]
    Inside,
    /// Away from the plot area, towards the tick labels.
    Outside,
    /// Across the axis line, half each way.
    Both,
}

impl TickDirection {
    /// Every direction, in menu order.
    pub const ALL: [Self; 3] = [Self::Inside, Self::Outside, Self::Both];

    /// Menu label.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Inside => "Inside",
            Self::Outside => "Outside",
            Self::Both => "Across",
        }
    }

    /// How far a mark `length` long reaches into the plot and out of it.
    #[must_use]
    pub fn extent(self, length: f64) -> (f64, f64) {
        match self {
            Self::Inside => (length, 0.0),
            Self::Outside => (0.0, length),
            Self::Both => (length / 2.0, length / 2.0),
        }
    }
}

/// Length and line thickness of tick marks, in screen points (exports use
/// the same values scaled like the rest of the figure).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TickMarkSize {
    /// Length (0–30).
    pub length: f64,
    /// Line thickness (0.25–5).
    pub width: f64,
}

impl TickMarkSize {
    /// Major tick marks: 5 points long, 1 thick.
    pub const MAJOR: Self = Self { length: 5.0, width: 1.0 };
    /// Minor tick marks: 3 points long, 1 thick.
    pub const MINOR: Self = Self { length: 3.0, width: 1.0 };

    /// The size with length and thickness held to their ranges.
    #[must_use]
    pub fn clamped(self) -> Self {
        let ok = |v: f64, lo: f64, hi: f64, d: f64| if v.is_finite() { v.clamp(lo, hi) } else { d };
        Self { length: ok(self.length, 0.0, 30.0, 5.0), width: ok(self.width, 0.25, 5.0, 1.0) }
    }
}

/// A tick label written by the user: `label` (read as math markup, so
/// `\pi` reads π) at `value`. It replaces the number of a major tick there,
/// or adds a labelled tick of its own; an empty label leaves the tick
/// unlabelled.
#[derive(Debug, Clone, PartialEq)]
pub struct CustomTick {
    /// Position in data units.
    pub value: f64,
    /// Label text (math markup).
    pub label: String,
}

/// Largest offset of an axis line from the data area, in points.
pub const MAX_AXIS_OFFSET: f64 = 40.0;

/// Settings of one axis.
#[derive(Debug, Clone, PartialEq)]
pub struct AxisSettings {
    /// Major tick spacing.
    pub major: MajorTicks,
    /// Minor tick spacing.
    pub minor: MinorTicks,
    /// A value that falls on a major tick (the ticks are origin + k × interval).
    pub origin: f64,
    /// Grid lines.
    pub grid: GridLines,
    /// Short tick marks along the axis.
    pub tick_marks: bool,
    /// Numbers at the major ticks.
    pub tick_labels: bool,
    /// Number format of the tick labels.
    pub format: TickFormat,
    /// Axis title replacing the default (the variable name); `None` keeps it.
    pub title: Option<String>,
    /// Show the axis title.
    pub show_title: bool,
    /// Side of the plot.
    pub side: AxisSide,
    /// The date-time display of the column this axis shows, set by the view
    /// (not a user setting): automatic ticks then fall on calendar units and
    /// read as dates ([`crate::time_scale`]).
    pub time: Option<stat_data::TemporalDisplay>,
    /// The axis counts whole things (sample or subgroup numbers), set by the
    /// view (not a user setting): automatic ticks then fall on whole numbers.
    pub whole_numbers: bool,
    /// Stretches left out of the axis, with a scale and share per piece
    /// between them and the style of the break marks, on plots that support
    /// breaks (graph canvases and report scatter plots; see
    /// [`crate::axis_breaks`]).
    pub breaks: crate::axis_breaks::AxisBreaks,
    /// Which way the tick marks point.
    pub tick_direction: TickDirection,
    /// Size of major tick marks.
    pub major_mark: TickMarkSize,
    /// Size of minor tick marks (drawn when the axis has minor ticks).
    pub minor_mark: TickMarkSize,
    /// Distance in points between the data area and the axis line, which
    /// moves out with its marks and labels (0: on the data area's edge).
    pub offset: f64,
    /// Widen an automatic range to the major ticks just outside it, so the
    /// axis starts and ends on a labelled value.
    pub extend_to_ticks: bool,
    /// Tick labels written by the user, by value.
    pub custom_ticks: Vec<CustomTick>,
    /// A range typed in Plot settings (`[min, max]`, data units), kept with
    /// the plot; `None` while the range is automatic or only dragged.
    pub range: Option<[f64; 2]>,
}

impl Default for AxisSettings {
    fn default() -> Self {
        Self {
            major: MajorTicks::Auto,
            minor: MinorTicks::None,
            origin: 0.0,
            grid: GridLines::MajorOnly,
            tick_marks: true,
            tick_labels: true,
            format: TickFormat::Auto,
            title: None,
            show_title: true,
            side: AxisSide::Standard,
            time: None,
            whole_numbers: false,
            breaks: crate::axis_breaks::AxisBreaks::default(),
            tick_direction: TickDirection::Inside,
            major_mark: TickMarkSize::MAJOR,
            minor_mark: TickMarkSize::MINOR,
            offset: 0.0,
            extend_to_ticks: false,
            custom_ticks: Vec::new(),
            range: None,
        }
    }
}

impl AxisSettings {
    /// Defaults for the horizontal axis: no vertical grid lines (the
    /// horizontal rules at the vertical axis's ticks carry the grid).
    #[must_use]
    pub fn horizontal() -> Self {
        Self { grid: GridLines::None, ..Self::default() }
    }

    /// How far the tick labels sit beyond their usual place, in points: the
    /// axis line's offset plus the part of the major tick marks outside it.
    #[must_use]
    pub fn label_gap(&self) -> f64 {
        let out = if self.tick_marks { self.tick_direction.extent(self.major_mark.length).1 } else { 0.0 };
        self.offset.clamp(0.0, MAX_AXIS_OFFSET) + out
    }

    /// The user's label for the tick at `value`, if any (`tolerance` in data
    /// units).
    #[must_use]
    pub fn custom_label(&self, value: f64, tolerance: f64) -> Option<&str> {
        self.custom_ticks.iter().find(|c| (c.value - value).abs() <= tolerance).map(|c| c.label.as_str())
    }

    /// Set the user's label for the tick at `value` (replacing one there);
    /// `None` removes it.
    pub fn set_custom_label(&mut self, value: f64, label: Option<String>, tolerance: f64) {
        self.custom_ticks.retain(|c| (c.value - value).abs() > tolerance);
        if let Some(label) = label {
            self.custom_ticks.push(CustomTick { value, label });
            self.custom_ticks.sort_by(|a, b| a.value.total_cmp(&b.value));
        }
    }

    /// The title to draw: the custom title, the default, or nothing when hidden.
    #[must_use]
    pub fn title_or(&self, default: &str) -> String {
        if !self.show_title { String::new() } else { self.title.clone().unwrap_or_else(|| default.to_owned()) }
    }

    /// The title to draw as a label in math markup: a custom title is read as
    /// markup (`x_1`, `\alpha`, `\frac{a}{b}`, see `crate::math_text`);
    /// the default title is read as markup only when `markup_default` is set
    /// (column names are otherwise shown exactly as written). Empty when the
    /// title is hidden.
    #[must_use]
    pub fn title_text(&self, default: &str, markup_default: bool) -> crate::MathText {
        match (&self.title, self.show_title) {
            (_, false) => crate::MathText::default(),
            (Some(t), true) => crate::MathText::parse(t),
            (None, true) => crate::MathText::new(default, markup_default),
        }
    }
}

/// Settings of both axes and of the plot area.
#[derive(Debug, Clone, PartialEq)]
pub struct PlotAxes {
    /// Horizontal axis.
    pub x: AxisSettings,
    /// Vertical axis.
    pub y: AxisSettings,
    /// One data unit has the same length on both axes.
    pub equal_scale: bool,
    /// How category labels along the horizontal axis are turned (plots with
    /// a categorical horizontal axis).
    pub category_labels: crate::CategoryLabels,
    /// Show the pointer's coordinates while hovering.
    pub hover_coordinates: bool,
    /// Show the key (legend) under the plot.
    pub show_key: bool,
    /// Plot title replacing the default (the section's name); `None` keeps it.
    pub title: Option<String>,
    /// Show the title (and subtitle) above the plot.
    pub show_title: bool,
    /// The right-hand vertical axis, on plots that have one (graph canvases
    /// with right-axis columns).
    pub y2: AxisSettings,
    /// Which axes (`[x, y]`) can leave stretches out, set by the view (not a
    /// user setting): Plot settings then offers breaks on them.
    pub breaks: [bool; 2],
    /// Default title of the right-hand vertical axis, set by the view when
    /// the plot has one (not a user setting): Plot settings then shows its
    /// section.
    pub y2_label: Option<String>,
    /// At equal scale: how many vertical units one horizontal unit is as
    /// long as (1 draws them the same length; 2 draws one x unit as long as
    /// two y units).
    pub scale_ratio: f64,
    /// Print scale for saved figures: millimetres per horizontal unit
    /// (`None`: the figure keeps its on-screen size).
    pub print_scale: Option<f64>,
}

impl Default for PlotAxes {
    fn default() -> Self {
        Self {
            x: AxisSettings::horizontal(),
            y: AxisSettings::default(),
            equal_scale: false,
            category_labels: crate::CategoryLabels::Automatic,
            hover_coordinates: true,
            show_key: true,
            title: None,
            show_title: true,
            y2: AxisSettings::default(),
            breaks: [false; 2],
            y2_label: None,
            scale_ratio: 1.0,
            print_scale: None,
        }
    }
}

/// Split a section heading such as `"Survival Curves (steps: Kaplan–Meier; …)"`
/// into a plot title (`"Survival Curves"`) and a subtitle from the trailing
/// parenthesis (`"steps: Kaplan–Meier; …"`). The subtitle is the parenthesis
/// that the final `)` closes, so parentheses inside the title itself (a
/// column named `"Yield (%)"`) stay in the title.
#[must_use]
pub fn split_title(heading: &str) -> (String, Option<String>) {
    let heading = heading.trim();
    if heading.ends_with(')') {
        // Walk back from the final ')' to the '(' that opens it.
        let mut depth = 0usize;
        let mut open = None;
        for (i, c) in heading.char_indices().rev() {
            match c {
                ')' => depth += 1,
                '(' => {
                    depth -= 1;
                    if depth == 0 {
                        open = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }
        if let Some(open) = open.filter(|&o| heading[..o].ends_with(' ')) {
            let main = heading[..open].trim();
            let sub = heading[open + 1..heading.len() - 1].trim();
            if !main.is_empty() {
                return (main.to_owned(), (!sub.is_empty()).then(|| sub.to_owned()));
            }
        }
    }
    (heading.to_owned(), None)
}

impl PlotAxes {
    /// egui_plot's data aspect for equal scale (horizontal units per point
    /// over vertical units per point), or `None` when the scale is free.
    #[must_use]
    pub fn data_aspect(&self) -> Option<f64> {
        let r = if self.scale_ratio.is_finite() && self.scale_ratio > 0.0 { self.scale_ratio } else { 1.0 };
        self.equal_scale.then_some(1.0 / r)
    }

    /// Title and subtitle to draw above the plot, or `None` when hidden. A
    /// custom title replaces both.
    #[must_use]
    pub fn plot_title(&self, heading: &str) -> Option<(String, Option<String>)> {
        if !self.show_title {
            return None;
        }
        Some(match &self.title {
            Some(t) => (t.clone(), None),
            None => split_title(heading),
        })
    }
}

/// Kind of a tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickKind {
    /// Major tick with a label.
    Labelled,
    /// Major tick whose label is skipped to avoid crowding.
    Major,
    /// Minor tick.
    Minor,
}

/// One tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tick {
    /// Position in data units.
    pub value: f64,
    /// Kind.
    pub kind: TickKind,
}

/// Ticks of an axis and the steps they were built from.
#[derive(Debug, Clone, PartialEq)]
pub struct AxisTicks {
    /// Ticks in increasing order.
    pub ticks: Vec<Tick>,
    /// Major interval.
    pub major_step: f64,
    /// Interval between labelled majors (a multiple of the major interval).
    pub label_step: f64,
    /// Minor interval (0 when there are none).
    pub minor_step: f64,
    /// Labels of the labelled ticks of a date-time axis, by position (empty on
    /// numeric axes, whose labels come from [`format_tick`]).
    pub time_labels: Vec<(f64, String)>,
}

/// The label of the tick at `value` on a date-time axis, from
/// [`AxisTicks::time_labels`] (empty if `value` is not a labelled tick).
#[must_use]
pub fn time_label(labels: &[(f64, String)], value: f64, major_step: f64) -> String {
    labels
        .iter()
        .find(|(t, _)| (t - value).abs() <= (1e-9 * major_step.abs()).max(1e-6))
        .map(|(_, l)| l.clone())
        .unwrap_or_default()
}

/// The smallest 1, 2, 2.5 or 5 × 10ᵏ that is at least `raw` (> 0), so ticks
/// fall on round values (…, 10, 20, 25, 50, 100, …).
#[must_use]
pub fn nice_step(raw: f64) -> f64 {
    if !(raw > 0.0 && raw.is_finite()) {
        return 1.0;
    }
    let magnitude = 10f64.powf(raw.log10().floor());
    let m = raw / magnitude;
    let nice = if m <= 1.0 + 1e-9 {
        1.0
    } else if m <= 2.0 + 1e-9 {
        2.0
    } else if m <= 2.5 + 1e-9 {
        2.5
    } else if m <= 5.0 + 1e-9 {
        5.0
    } else {
        10.0
    };
    nice * magnitude
}

/// Most ticks ever produced for one axis (guards against tiny intervals).
pub const MAX_TICKS: usize = 2000;

/// Ticks for the visible range `[lo, hi]` of an axis that is `units_per_point`
/// data units per screen point. Automatic majors aim at `target_gap` points
/// apart; labels are thinned to at least `min_label_gap` points apart.
#[must_use]
pub fn axis_ticks(
    settings: &AxisSettings,
    lo: f64,
    hi: f64,
    units_per_point: f64,
    target_gap: f64,
    min_label_gap: f64,
) -> AxisTicks {
    let mut t = generated_ticks(settings, lo, hi, units_per_point, target_gap, min_label_gap);
    if settings.custom_ticks.is_empty() || !((hi - lo).abs() > 0.0 && (hi - lo).is_finite()) {
        return t;
    }
    // The user's labels: a tick there (a minor one becomes major) is
    // labelled, else one is added.
    let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
    let tolerance = custom_tolerance(t.major_step);
    for c in settings.custom_ticks.iter().filter(|c| c.value >= lo - tolerance && c.value <= hi + tolerance) {
        match t.ticks.iter_mut().find(|k| (k.value - c.value).abs() <= tolerance) {
            Some(k) => k.kind = TickKind::Labelled,
            None => t.ticks.push(Tick { value: c.value, kind: TickKind::Labelled }),
        }
    }
    t.ticks.sort_by(|a, b| a.value.total_cmp(&b.value));
    t
}

/// How close (data units) a tick must be to a user's label to carry it,
/// for majors `major_step` apart.
#[must_use]
pub fn custom_tolerance(major_step: f64) -> f64 {
    (1e-9 * major_step.abs()).max(1e-12)
}

/// The range `[lo, hi]` widened to the major ticks just outside it, as
/// [`axis_ticks`] would place them on an axis `length` points long (only
/// for linear numeric axes: date-time axes keep their range).
#[must_use]
pub fn extended_to_ticks(settings: &AxisSettings, lo: f64, hi: f64, length: f64, target_gap: f64) -> (f64, f64) {
    let span = hi - lo;
    if settings.time.is_some() || !(span > 0.0 && span.is_finite() && length > 0.0) {
        return (lo, hi);
    }
    let units_per_point = span / length;
    let mut step = nice_step(units_per_point * target_gap);
    if settings.whole_numbers && step.fract() != 0.0 {
        step = if step < 1.0 { 1.0 } else { 2.0 * step };
    }
    if let MajorTicks::Every(s) = settings.major {
        if s > 0.0 && s.is_finite() && span / s <= MAX_TICKS as f64 {
            step = s;
        }
    }
    let origin = if settings.origin.is_finite() { settings.origin } else { 0.0 };
    let tol = 1e-9 * step;
    let a = origin + ((lo - origin) / step + tol).floor() * step;
    let b = origin + ((hi - origin) / step - tol).ceil() * step;
    if a < b { (a, b) } else { (lo, hi) }
}

/// The ticks of [`axis_ticks`] before the user's own labels are added.
fn generated_ticks(
    settings: &AxisSettings,
    lo: f64,
    hi: f64,
    units_per_point: f64,
    target_gap: f64,
    min_label_gap: f64,
) -> AxisTicks {
    let empty =
        AxisTicks { ticks: Vec::new(), major_step: 1.0, label_step: 1.0, minor_step: 0.0, time_labels: Vec::new() };
    let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
    let span = hi - lo;
    if !(span > 0.0 && span.is_finite() && units_per_point > 0.0 && units_per_point.is_finite()) {
        return empty;
    }
    if let (Some(display), MajorTicks::Auto) = (settings.time, settings.major) {
        return time_axis_ticks(&display, lo, hi, units_per_point, target_gap, min_label_gap);
    }
    let mut auto = nice_step(units_per_point * target_gap);
    if settings.whole_numbers && auto.fract() != 0.0 {
        // 0.5 and below become 1; 2.5 becomes 5.
        auto = if auto < 1.0 { 1.0 } else { 2.0 * auto };
    }
    let mut major = match settings.major {
        MajorTicks::Every(s) if s > 0.0 && s.is_finite() => s,
        _ => auto,
    };
    // Never more than MAX_TICKS majors (a tiny fixed interval falls back to automatic).
    if span / major > MAX_TICKS as f64 {
        major = auto.max(span / MAX_TICKS as f64);
    }
    let gap_points = major / units_per_point;
    let every = if gap_points >= min_label_gap { 1 } else { (min_label_gap / gap_points).ceil() as i64 };
    let label_step = major * every as f64;
    let minor = match settings.minor {
        MinorTicks::None => 0.0,
        MinorTicks::Auto => {
            let mantissa = major / 10f64.powf(major.log10().floor());
            major / if (mantissa - 2.0).abs() < 1e-6 { 4.0 } else { 5.0 }
        }
        MinorTicks::Subdivisions(n) if n >= 2 => major / f64::from(n.min(100)),
        MinorTicks::Every(s) if s > 0.0 && s < major && major / s <= 100.0 => s,
        _ => 0.0,
    };
    let origin = if settings.origin.is_finite() { settings.origin } else { 0.0 };
    let mut ticks = Vec::new();
    let first = ((lo - origin) / major).ceil() as i64;
    let last = ((hi - origin) / major).floor() as i64;
    for k in first..=last {
        let kind = if k.rem_euclid(every) == 0 { TickKind::Labelled } else { TickKind::Major };
        ticks.push(Tick { value: origin + k as f64 * major, kind });
    }
    if minor > 0.0 && span / minor <= (MAX_TICKS * 2) as f64 {
        let first = ((lo - origin) / minor).ceil() as i64;
        let last = ((hi - origin) / minor).floor() as i64;
        for j in first..=last {
            let value = origin + j as f64 * minor;
            let on_major = ((value - origin) / major).round();
            if (origin + on_major * major - value).abs() > 1e-9 * major {
                ticks.push(Tick { value, kind: TickKind::Minor });
            }
        }
    }
    ticks.sort_by(|a, b| a.value.total_cmp(&b.value));
    let time_labels = match settings.time {
        // A fixed interval on a date axis: labels still read as dates.
        Some(display) => ticks
            .iter()
            .filter(|t| t.kind == TickKind::Labelled)
            .map(|t| {
                (
                    t.value,
                    if display.format == stat_data::TemporalFormat::Duration {
                        display.span_text(t.value)
                    } else {
                        display.text(t.value)
                    },
                )
            })
            .collect(),
        None => Vec::new(),
    };
    AxisTicks { ticks, major_step: major, label_step, minor_step: minor, time_labels }
}

/// Automatic ticks of a date-time axis: every major is a calendar step from
/// [`crate::time_scale::time_step`] wide enough for its label (dates are
/// wider than numbers), so every major is labelled. No minor ticks.
fn time_axis_ticks(
    display: &stat_data::TemporalDisplay,
    lo: f64,
    hi: f64,
    units_per_point: f64,
    target_gap: f64,
    min_label_gap: f64,
) -> AxisTicks {
    use crate::time_scale::{time_labels, time_step, time_ticks};
    let step = time_step(units_per_point * target_gap.max(1.6 * min_label_gap), display.format);
    let values = time_ticks(lo, hi, step, display);
    let labels = time_labels(&values, step, display);
    let ticks = values.iter().map(|&value| Tick { value, kind: TickKind::Labelled }).collect();
    AxisTicks {
        ticks,
        major_step: step.seconds,
        label_step: step.seconds,
        minor_step: 0.0,
        time_labels: values.into_iter().zip(labels).collect(),
    }
}

/// Decimals needed to write multiples of `step` exactly (at most 12).
fn decimals_for(step: f64) -> usize {
    for d in 0..=12usize {
        let scaled = step * 10f64.powi(d as i32);
        if (scaled - scaled.round()).abs() < 1e-6 * scaled.abs().max(1.0) {
            return d;
        }
    }
    12
}

/// Label of the tick at `value` for an axis whose major interval is `major_step`.
#[must_use]
pub fn format_tick(format: TickFormat, value: f64, major_step: f64) -> String {
    // Values within rounding error of zero print as 0, never as -0 or 1e-17.
    let value = if value.abs() < 1e-9 * major_step.abs() { 0.0 } else { value };
    let text = match format {
        TickFormat::Auto => {
            let a = value.abs();
            if a != 0.0 && !(1e-4..1e7).contains(&a) {
                let s = format!("{value:.2e}");
                tidy_exponent(&s)
            } else {
                format!("{value:.*}", decimals_for(major_step))
            }
        }
        TickFormat::Decimals(d) => format!("{value:.*}", d as usize),
        TickFormat::Scientific(d) => tidy_exponent(&format!("{value:.*e}", d as usize)),
        TickFormat::Percent(d) => format!("{:.*}%", d as usize, value * 100.0),
    };
    if text.starts_with('-') && text[1..].chars().all(|c| c == '0' || c == '.' || c == '%') {
        text[1..].to_owned()
    } else {
        text
    }
}

/// `1.50e3` → `1.50×10³`-style compact form is hard to read in small labels;
/// keep Rust's `e` notation but drop a redundant `+`.
fn tidy_exponent(s: &str) -> String {
    s.replace("e+", "e")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_labels_replace_or_add_ticks() {
        let mut s = AxisSettings::default();
        s.set_custom_label(std::f64::consts::PI, Some("\\pi".into()), 1e-9);
        s.set_custom_label(10.0, Some("ten".into()), 1e-9);
        let t = axis_ticks(&s, 0.0, 12.0, 0.05, 45.0, 26.0);
        assert_eq!(t.major_step, 2.5);
        // π is added between the majors; 10 is already a major and stays one tick.
        assert!(t.ticks.iter().any(|k| k.value == std::f64::consts::PI && k.kind == TickKind::Labelled));
        assert_eq!(t.ticks.iter().filter(|k| (k.value - 10.0).abs() < 1e-9).count(), 1);
        assert_eq!(s.custom_label(10.0, 1e-9), Some("ten"));
        assert!(t.ticks.windows(2).all(|w| w[0].value <= w[1].value));
        // Out of range: not drawn. Removing: gone.
        let t = axis_ticks(&s, 4.0, 9.0, 0.05, 45.0, 26.0);
        assert!(!t.ticks.iter().any(|k| k.value == std::f64::consts::PI));
        s.set_custom_label(10.0, None, 1e-9);
        assert_eq!(s.custom_ticks.len(), 1);
    }

    #[test]
    fn ranges_extend_to_the_nearest_major_ticks() {
        let s = AxisSettings::default();
        // 0.13..9.87 over 400 points, 45 points per major: 1.1 units → steps of 2.
        assert_eq!(extended_to_ticks(&s, 0.13, 9.87, 400.0, 45.0), (0.0, 10.0));
        // Already on ticks: unchanged.
        assert_eq!(extended_to_ticks(&s, 0.0, 10.0, 400.0, 45.0), (0.0, 10.0));
        // A fixed interval from an origin.
        let fixed = AxisSettings { major: MajorTicks::Every(3.0), origin: 1.0, ..AxisSettings::default() };
        assert_eq!(extended_to_ticks(&fixed, 2.0, 8.5, 400.0, 45.0), (1.0, 10.0));
        // Negative values.
        assert_eq!(extended_to_ticks(&s, -7.3, -0.4, 400.0, 45.0), (-8.0, 0.0));
    }

    #[test]
    fn tick_marks_point_inside_outside_or_across() {
        assert_eq!(TickDirection::Inside.extent(6.0), (6.0, 0.0));
        assert_eq!(TickDirection::Outside.extent(6.0), (0.0, 6.0));
        assert_eq!(TickDirection::Both.extent(6.0), (3.0, 3.0));
        let mut s = AxisSettings { offset: 8.0, tick_direction: TickDirection::Outside, ..AxisSettings::default() };
        assert_eq!(s.label_gap(), 13.0);
        s.tick_marks = false;
        assert_eq!(s.label_gap(), 8.0);
        assert_eq!(TickMarkSize { length: 99.0, width: 0.0 }.clamped(), TickMarkSize { length: 30.0, width: 0.25 });
    }

    #[test]
    fn equal_scale_ratio_sets_the_data_aspect() {
        let mut a = PlotAxes::default();
        assert_eq!(a.data_aspect(), None);
        a.equal_scale = true;
        assert_eq!(a.data_aspect(), Some(1.0));
        a.scale_ratio = 2.0;
        assert_eq!(a.data_aspect(), Some(0.5));
        a.scale_ratio = -1.0;
        assert_eq!(a.data_aspect(), Some(1.0));
    }

    #[test]
    fn defaults_draw_horizontal_major_rules_only() {
        let a = PlotAxes::default();
        assert_eq!((a.x.grid, a.y.grid), (GridLines::None, GridLines::MajorOnly));
        assert_eq!((a.x.minor, a.y.minor), (MinorTicks::None, MinorTicks::None));
        let t = axis_ticks(&a.y, 0.0, 53.0, 0.1, 45.0, 26.0);
        assert!(t.ticks.iter().all(|t| t.kind != TickKind::Minor));
        assert_eq!(t.ticks.first().map(|t| t.value), Some(0.0), "ticks start at the origin");
    }

    #[test]
    fn nice_steps() {
        assert_eq!(nice_step(0.7), 1.0);
        assert_eq!(nice_step(1.3), 2.0);
        assert_eq!(nice_step(3.0), 5.0);
        assert_eq!(nice_step(2.2), 2.5);
        assert_eq!(nice_step(23.0), 25.0);
        assert_eq!(nice_step(7.0), 10.0);
        assert!((nice_step(0.023) - 0.025).abs() < 1e-15);
        assert!((nice_step(0.031) - 0.05).abs() < 1e-15);
        assert_eq!(nice_step(2000.0), 2000.0);
    }

    #[test]
    fn automatic_ticks() {
        // 0..35 over 700 points: 0.05 units/point; 80-point target → 4 → step 5.
        let t = axis_ticks(
            &AxisSettings { minor: MinorTicks::Auto, ..AxisSettings::default() },
            0.0,
            35.0,
            0.05,
            80.0,
            60.0,
        );
        assert_eq!(t.major_step, 5.0);
        let majors: Vec<f64> = t.ticks.iter().filter(|t| t.kind != TickKind::Minor).map(|t| t.value).collect();
        assert_eq!(majors, [0.0, 5.0, 10.0, 15.0, 20.0, 25.0, 30.0, 35.0]);
        assert_eq!(t.minor_step, 1.0, "five subdivisions of 5");
        assert_eq!(t.ticks.iter().filter(|t| t.kind == TickKind::Minor).count(), 28);
        assert!(t.ticks.iter().all(|t| t.kind != TickKind::Major), "100-point gaps: every major labelled");
        // Step 2 → four subdivisions.
        let t = axis_ticks(
            &AxisSettings { minor: MinorTicks::Auto, ..AxisSettings::default() },
            0.0,
            10.0,
            0.02,
            80.0,
            60.0,
        );
        assert_eq!((t.major_step, t.minor_step), (2.0, 0.5));
    }

    #[test]
    fn fixed_intervals_origin_and_label_thinning() {
        let s = AxisSettings {
            major: MajorTicks::Every(0.25),
            minor: MinorTicks::Subdivisions(5),
            origin: 0.1,
            ..AxisSettings { minor: MinorTicks::Auto, ..AxisSettings::default() }
        };
        // 0.01 units/point: majors 25 points apart; labels need 60 → every 3rd.
        let t = axis_ticks(&s, 0.0, 1.0, 0.01, 80.0, 60.0);
        let majors: Vec<f64> = t.ticks.iter().filter(|t| t.kind != TickKind::Minor).map(|t| t.value).collect();
        assert_eq!(majors.len(), 4);
        assert!((majors[0] - 0.1).abs() < 1e-12 && (majors[3] - 0.85).abs() < 1e-12);
        assert!((t.label_step - 0.75).abs() < 1e-12);
        assert_eq!(t.ticks.iter().filter(|t| t.kind == TickKind::Labelled).count(), 2);
        assert!((t.minor_step - 0.05).abs() < 1e-12);
        // A minor interval at least as large as the major one is ignored.
        let s = AxisSettings {
            minor: MinorTicks::Every(1.0),
            major: MajorTicks::Every(0.5),
            ..AxisSettings { minor: MinorTicks::Auto, ..AxisSettings::default() }
        };
        assert_eq!(axis_ticks(&s, 0.0, 2.0, 0.01, 80.0, 60.0).minor_step, 0.0);
        // A runaway fixed interval falls back to a sane count.
        let s = AxisSettings {
            major: MajorTicks::Every(1e-9),
            ..AxisSettings { minor: MinorTicks::Auto, ..AxisSettings::default() }
        };
        assert!(axis_ticks(&s, 0.0, 100.0, 0.1, 80.0, 60.0).ticks.len() <= 3 * MAX_TICKS);
        assert!(axis_ticks(&s, 1.0, 1.0, 0.1, 80.0, 60.0).ticks.is_empty());
    }

    #[test]
    fn tick_labels() {
        assert_eq!(format_tick(TickFormat::Auto, 0.5, 0.25), "0.50");
        assert_eq!(format_tick(TickFormat::Auto, 30.0, 5.0), "30");
        assert_eq!(format_tick(TickFormat::Auto, -3e-17, 0.1), "0.0");
        assert_eq!(format_tick(TickFormat::Auto, 2.5e8, 5e7), "2.50e8");
        assert_eq!(format_tick(TickFormat::Decimals(3), 1.0, 1.0), "1.000");
        assert_eq!(format_tick(TickFormat::Scientific(1), 12_345.0, 1.0), "1.2e4");
        assert_eq!(format_tick(TickFormat::Percent(0), 0.25, 0.05), "25%");
        assert_eq!(format_tick(TickFormat::Decimals(1), -0.01, 1.0), "0.0");
    }

    #[test]
    fn plot_titles() {
        let h = "Survival Curves (steps: Kaplan–Meier; shaded: pointwise 95% band (log–log))";
        let (t, sub) = split_title(h);
        assert_eq!(t, "Survival Curves");
        assert_eq!(sub.as_deref(), Some("steps: Kaplan–Meier; shaded: pointwise 95% band (log–log)"));
        assert_eq!(split_title("Score Plot"), ("Score Plot".to_owned(), None));
        // A parenthesis in a column name stays in the title.
        let (t, sub) = split_title("Contour Plot of Predicted Yield (%) (contours every 2; factor C held at 0)");
        assert_eq!(t, "Contour Plot of Predicted Yield (%)");
        assert_eq!(sub.as_deref(), Some("contours every 2; factor C held at 0"));
        assert_eq!(split_title("Unbalanced)"), ("Unbalanced)".to_owned(), None));
        let mut a = PlotAxes::default();
        assert_eq!(a.plot_title("Fit (x)").unwrap().0, "Fit");
        a.title = Some("My plot".into());
        assert_eq!(a.plot_title("Fit (x)"), Some(("My plot".to_owned(), None)));
        a.show_title = false;
        assert_eq!(a.plot_title("Fit"), None);
    }

    #[test]
    fn whole_number_axes_skip_fractional_ticks() {
        let s = AxisSettings { whole_numbers: true, ..AxisSettings::default() };
        // 90 points at 0.01 units per point would give a step of 1 (fine) …
        let t = axis_ticks(&s, 0.0, 10.0, 0.005, 90.0, 70.0);
        assert!(t.ticks.iter().all(|k| k.value.fract() == 0.0), "{:?}", t.ticks);
        // … and steps of 0.5 or 2.5 move to 1 and 5.
        let t = axis_ticks(&s, 0.0, 4.0, 0.004, 90.0, 70.0);
        assert_eq!(t.major_step, 1.0);
        let t = axis_ticks(&s, 0.0, 40.0, 0.025, 90.0, 70.0);
        assert_eq!(t.major_step, 5.0);
        let plain = axis_ticks(&AxisSettings::default(), 0.0, 40.0, 0.025, 90.0, 70.0);
        assert_eq!(plain.major_step, 2.5);
    }

    #[test]
    fn titles() {
        let mut s = AxisSettings { minor: MinorTicks::Auto, ..AxisSettings::default() };
        assert_eq!(s.title_or("Weeks"), "Weeks");
        s.title = Some("Time (weeks)".into());
        assert_eq!(s.title_or("Weeks"), "Time (weeks)");
        s.show_title = false;
        assert_eq!(s.title_or("Weeks"), "");
    }

    #[test]
    fn date_axes_tick_on_the_calendar() {
        use stat_data::{TemporalDisplay, TemporalFormat};
        let start = stat_data::temporal::seconds_from_civil(2026, 1, 1, 0, 0, 0.0);
        let settings =
            AxisSettings { time: Some(TemporalDisplay::new(TemporalFormat::Date)), ..AxisSettings::horizontal() };
        // A year across 600 points: months or quarters, all labelled.
        let span = 365.0 * 86_400.0;
        let t = axis_ticks(&settings, start, start + span, span / 600.0, 90.0, 70.0);
        assert!(t.ticks.iter().all(|k| k.kind == TickKind::Labelled));
        assert_eq!(t.time_labels.first().map(|l| l.1.as_str()), Some("Jan 2026"));
        assert_eq!(time_label(&t.time_labels, t.ticks[1].value, t.major_step), t.time_labels[1].1);
        assert_eq!(time_label(&t.time_labels, start + 1.0, t.major_step), "");
        // A fixed interval of one day still reads as dates.
        let fixed = AxisSettings { major: MajorTicks::Every(86_400.0), ..settings.clone() };
        let t = axis_ticks(&fixed, start, start + 3.0 * 86_400.0, 3.0 * 86_400.0 / 600.0, 90.0, 70.0);
        assert_eq!(t.time_labels[0].1, "2026-01-01");
    }
}
