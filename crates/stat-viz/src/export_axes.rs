//! Axes of exported report figures drawn with the plot's own settings
//! (`crate::axis_settings`), so a saved figure shows what the screen shows:
//! the user's tick intervals, minor ticks, number format and tick labels,
//! grid lines, tick marks pointing in, out or across the axis line with
//! their length and thickness, axis lines moved away from the data area,
//! and either axis on its opposite side.
//!
//! Lengths set in screen points are drawn at [`PT`] page points each, the
//! scale at which a figure matches the plot's size on screen.

use crate::axis_settings::{
    AxisSettings, AxisSide, GridLines, PlotAxes, TickKind, axis_ticks, custom_tolerance, format_tick, time_label,
};
use crate::category_labels::{CategoryLayout, LabelTurn};
use crate::export::{Canvas, ChartPanel, HAlign};
use crate::layer::Rgba;
use crate::math_text::MathText;

/// Page points per screen point (a plot drawn at 96 pixels per inch).
pub const PT: f64 = 0.75;

/// Preferred spacing of automatic major ticks and least spacing of labels,
/// in screen points, as on screen.
const X_GAPS: (f64, f64) = (90.0, 70.0);
const Y_GAPS: (f64, f64) = (45.0, 26.0);

const GRID: Rgba = Rgba([220, 222, 228, 255]);
const GRID_MINOR: Rgba = Rgba([236, 237, 241, 255]);
const FRAME: Rgba = Rgba([140, 144, 156, 255]);
const INK: Rgba = Rgba([30, 32, 40, 255]);
const MUTED: Rgba = Rgba([90, 94, 106, 255]);
const TICK_FONT: f64 = 7.0;

/// One tick of an exported axis.
#[derive(Debug, Clone, PartialEq)]
pub struct StyledTick {
    /// Position in data units.
    pub value: f64,
    /// A minor tick (no label, shorter mark).
    pub minor: bool,
    /// Label (`None` when unlabelled).
    pub label: Option<MathText>,
}

/// Ticks of an axis `length` page points long showing `[lo, hi]`, placed and
/// labelled as on screen: the user's intervals and format, labels thinned
/// so they never crowd, and the user's own labels (read as markup).
#[must_use]
pub fn styled_ticks(s: &AxisSettings, lo: f64, hi: f64, length: f64, vertical: bool) -> Vec<StyledTick> {
    if !(length > 0.0 && hi > lo) {
        return Vec::new();
    }
    let (target, gap) = if vertical { Y_GAPS } else { X_GAPS };
    let t = axis_ticks(s, lo, hi, (hi - lo) / (length / PT), target, gap);
    let tolerance = custom_tolerance(t.major_step);
    t.ticks
        .iter()
        .map(|k| {
            let label = (k.kind == TickKind::Labelled && s.tick_labels).then(|| {
                if let Some(custom) = s.custom_label(k.value, tolerance) {
                    MathText::parse(custom)
                } else if s.time.is_some() {
                    MathText::literal(&time_label(&t.time_labels, k.value, t.major_step))
                } else {
                    MathText::literal(&format_tick(s.format, k.value, t.major_step))
                }
            });
            StyledTick { value: k.value, minor: k.kind == TickKind::Minor, label: label.filter(|l| !l.is_empty()) }
        })
        .collect()
}

/// How far an axis's tick labels sit beyond their usual place, in page
/// points (offset plus tick marks pointing out).
#[must_use]
pub(crate) fn label_gap(s: &AxisSettings) -> f64 {
    s.label_gap() * PT
}

pub(crate) fn offset(s: &AxisSettings) -> f64 {
    s.offset.clamp(0.0, crate::axis_settings::MAX_AXIS_OFFSET) * PT
}

/// A tick mark at page position `at` (across the axis) of the axis drawn
/// with `s` beside `plot` (`[left, top, right, bottom]`).
pub(crate) fn mark(c: &mut dyn Canvas, s: &AxisSettings, at: f64, plot: [f64; 4], vertical: bool, minor: bool) {
    if !s.tick_marks {
        return;
    }
    let size = if minor { s.minor_mark } else { s.major_mark }.clamped();
    let (inside, outside) = s.tick_direction.extent(size.length * PT);
    let off = offset(s);
    let width = size.width * PT;
    let opposite = s.side == AxisSide::Opposite;
    let seg = if vertical {
        let x = if opposite { plot[2] + off } else { plot[0] - off };
        let (into, out) = if opposite { (-inside, outside) } else { (inside, -outside) };
        [[x + out, at], [x + into, at]]
    } else {
        let y = if opposite { plot[1] - off } else { plot[3] + off };
        let (into, out) = if opposite { (inside, -outside) } else { (-inside, outside) };
        [[at, y + out], [at, y + into]]
    };
    if seg[0] != seg[1] {
        c.polyline(&seg, FRAME, width, &[], false);
    }
}

/// The page rectangle of a chart panel's plot inside its cell
/// (`[left, top, right, bottom]` of the cell), leaving room for tick labels
/// and titles on the sides the axes are drawn on. `header` is the height of
/// the panel's heading, `bar` the width of a colour bar on the right and
/// `strip` the depth of the horizontal axis's labels.
#[must_use]
pub(crate) fn plot_rect(axes: &PlotAxes, cell: [f64; 4], header: f64, bar: f64, strip: f64) -> [f64; 4] {
    let (gx, gy) = (label_gap(&axes.x), label_gap(&axes.y));
    let x_top = axes.x.side == AxisSide::Opposite;
    let y_right = axes.y.side == AxisSide::Opposite;
    let below = strip + 23.0 + gx;
    [
        cell[0] + if y_right { 12.0 } else { 48.0 + gy },
        cell[1] + header + 6.0 + if x_top { below } else { 0.0 },
        cell[2] - 8.0 - bar - if y_right { 40.0 + gy } else { 0.0 },
        cell[3] - if x_top { 6.0 } else { below },
    ]
}

/// Draw both axes of `panel` beside `plot` with `axes`: grid lines, tick
/// marks, tick labels, the axis lines and the axis titles. `x_ticks` are the
/// panel's category ticks (used when it has them), laid out by `x_layout`
/// in a strip `strip` deep; `map` turns data into page coordinates and
/// `left` is how far left the vertical title may go.
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_chart_axes(
    c: &mut dyn Canvas,
    panel: &ChartPanel,
    axes: &PlotAxes,
    plot: [f64; 4],
    x_ticks: &[(f64, String)],
    x_layout: CategoryLayout,
    strip: f64,
    map: &dyn Fn([f64; 2]) -> [f64; 2],
    left: f64,
) {
    let ((x0, x1), (y0, y1)) = (panel.x_range, panel.y_range);
    let mut xs = axes.x.clone();
    xs.time = panel.x_time;
    let mut ys = axes.y.clone();
    ys.time = panel.y_time;
    // Horizontal axis.
    let categorical = panel.x_ticks.is_some();
    let ticks: Vec<StyledTick> = if categorical {
        x_ticks
            .iter()
            .map(|(v, l)| StyledTick { value: *v, minor: false, label: xs.tick_labels.then(|| MathText::literal(l)) })
            .collect()
    } else if let Some(b) = &panel.breaks[0] {
        graph_ticks(Some(&xs), None, false, b.ticks(&crate::export::plain_axis(), x0, x1, 6), (x0, x1), 0.0, false)
    } else {
        styled_ticks(&xs, x0, x1, plot[2] - plot[0], false)
    };
    let x_top = xs.side == AxisSide::Opposite;
    let gx = label_gap(&xs);
    for (k, t) in ticks.iter().enumerate() {
        let px = map([t.value, y0])[0];
        if px < plot[0] - 0.01 || px > plot[2] + 0.01 {
            continue;
        }
        if !categorical && (xs.grid == GridLines::MajorAndMinor || (xs.grid == GridLines::MajorOnly && !t.minor)) {
            let color = if t.minor { GRID_MINOR } else { GRID };
            c.polyline(&[[px, plot[1]], [px, plot[3]]], color, 0.5, &[], false);
        }
        mark(c, &xs, px, plot, false, t.minor);
        let Some(label) = &t.label else { continue };
        if k % x_layout.step.max(1) != 0 {
            continue;
        }
        let across = 0.35 * TICK_FONT;
        match (x_layout.turn, label.as_plain()) {
            (LabelTurn::Slanted, Some(text)) if !x_top => {
                let d = across * std::f64::consts::FRAC_1_SQRT_2;
                c.slanted_text([px + d, plot[3] + gx + 5.0 + d], text, TICK_FONT, MUTED, 1.0);
            }
            (LabelTurn::Vertical, Some(text)) if !x_top => {
                c.vertical_text([px + across, plot[3] + gx + 5.0], text, TICK_FONT, MUTED, 1.0);
            }
            _ => {
                let y = if x_top { plot[1] - gx - 4.0 } else { plot[3] + gx + 11.0 };
                crate::export::draw_math(c, [px, y], label, TICK_FONT, MUTED, 0.5);
            }
        }
    }
    // Vertical axis.
    let ticks: Vec<StyledTick> = match &panel.y_ticks {
        Some(explicit) => explicit
            .iter()
            .filter(|(v, _)| *v >= y0 - 1e-9 && *v <= y1 + 1e-9)
            .map(|(v, l)| StyledTick { value: *v, minor: false, label: ys.tick_labels.then(|| MathText::literal(l)) })
            .collect(),
        None => match &panel.breaks[1] {
            Some(b) => graph_ticks(Some(&ys), None, false, b.ticks(&crate::export::plain_axis(), y0, y1, 6), (y0, y1), 0.0, true),
            None => styled_ticks(&ys, y0, y1, plot[3] - plot[1], true),
        },
    };
    let y_right = ys.side == AxisSide::Opposite;
    let gy = label_gap(&ys);
    let mut tick_width: f64 = 0.0;
    for t in &ticks {
        let py = map([x0, t.value])[1];
        if py < plot[1] - 0.01 || py > plot[3] + 0.01 {
            continue;
        }
        if ys.grid == GridLines::MajorAndMinor || (ys.grid == GridLines::MajorOnly && !t.minor) {
            let color = if t.minor { GRID_MINOR } else { GRID };
            c.polyline(&[[plot[0], py], [plot[2], py]], color, 0.5, &[], false);
        }
        mark(c, &ys, py, plot, true, t.minor);
        if let Some(label) = &t.label {
            tick_width = tick_width.max(crate::export::markup_width(label, TICK_FONT));
            let (x, align): (f64, HAlign) = if y_right { (plot[2] + gy + 5.0, 0.0) } else { (plot[0] - gy - 5.0, 1.0) };
            crate::export::draw_math(c, [x, py + 2.5], label, TICK_FONT, MUTED, align);
        }
    }
    // Axis lines: the frame as before, or each line apart when moved out.
    if offset(&xs) > 0.0 || offset(&ys) > 0.0 {
        let y = if x_top { plot[1] - offset(&xs) } else { plot[3] + offset(&xs) };
        c.polyline(&[[plot[0], y], [plot[2], y]], FRAME, 0.8, &[], false);
        let x = if y_right { plot[2] + offset(&ys) } else { plot[0] - offset(&ys) };
        c.polyline(&[[x, plot[1]], [x, plot[3]]], FRAME, 0.8, &[], false);
    } else {
        let rect = [[plot[0], plot[1]], [plot[2], plot[1]], [plot[2], plot[3]], [plot[0], plot[3]]];
        c.polyline(&rect, FRAME, 0.8, &[], true);
    }
    // Titles beside their axes.
    if !panel.x_title.is_empty() {
        let y = if x_top { plot[1] - gx - 15.0 } else { plot[3] + strip + gx + 13.0 };
        c.text([(plot[0] + plot[2]) / 2.0, y], &panel.x_title, 8.5, INK, 0.5, false);
    }
    if !panel.y_title.is_empty() {
        let title = MathText::literal(&panel.y_title);
        if y_right {
            crate::export::right_axis_title(c, &title, plot, tick_width + gy);
        } else {
            crate::export::left_axis_title(c, &title, plot, tick_width + gy, left);
        }
    }
}

/// A graph axis's ticks for an exported canvas panel: the user's settings
/// (`style`) on a plain linear axis, as on screen, else `standard` (the
/// axis's own ticks: categories, transformed scales, breaks).
#[must_use]
pub(crate) fn graph_ticks(
    style: Option<&AxisSettings>,
    time: Option<stat_data::TemporalDisplay>,
    plain: bool,
    standard: Vec<(f64, String)>,
    range: (f64, f64),
    length: f64,
    vertical: bool,
) -> Vec<StyledTick> {
    match style {
        Some(s) if plain => {
            let s = AxisSettings { time, ..s.clone() };
            styled_ticks(&s, range.0, range.1, length, vertical)
        }
        _ => standard
            .into_iter()
            .map(|(value, l)| StyledTick {
                value,
                minor: false,
                label: style.is_none_or(|s| s.tick_labels).then(|| MathText::literal(&l)),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::axis_settings::{CustomTick, MajorTicks, MinorTicks};

    #[test]
    fn ticks_follow_the_settings() {
        let s = AxisSettings {
            major: MajorTicks::Every(2.0),
            minor: MinorTicks::Subdivisions(2),
            custom_ticks: vec![CustomTick { value: 3.0, label: "\\pi".into() }],
            ..AxisSettings::default()
        };
        // 0..10 over 300 page points (400 screen points).
        let t = styled_ticks(&s, 0.0, 10.0, 300.0, true);
        let majors: Vec<f64> = t.iter().filter(|t| !t.minor && t.value.fract() == 0.0).map(|t| t.value).collect();
        assert!(majors.contains(&0.0) && majors.contains(&10.0));
        assert!(t.iter().any(|t| t.minor && (t.value - 1.0).abs() < 1e-9));
        let pi = t.iter().find(|t| (t.value - 3.0).abs() < 1e-9).expect("the user's tick");
        assert_eq!(pi.label.as_ref().map(MathText::plain_text).as_deref(), Some("π"));
        assert_eq!(t.iter().find(|t| t.value == 4.0).and_then(|t| t.label.clone()), Some(MathText::literal("4")));
    }

    #[test]
    fn room_is_left_for_labels_moved_out() {
        let mut axes = PlotAxes::default();
        let plain = plot_rect(&axes, [0.0, 0.0, 400.0, 300.0], 0.0, 0.0, 11.0);
        axes.x.offset = 10.0;
        axes.y.tick_direction = crate::axis_settings::TickDirection::Outside;
        let moved = plot_rect(&axes, [0.0, 0.0, 400.0, 300.0], 0.0, 0.0, 11.0);
        assert!((plain[3] - moved[3] - 7.5).abs() < 1e-9, "10 screen points below");
        assert!((moved[0] - plain[0] - 3.75).abs() < 1e-9, "a 5-point mark outside");
        axes.y.side = AxisSide::Opposite;
        let right = plot_rect(&axes, [0.0, 0.0, 400.0, 300.0], 0.0, 0.0, 11.0);
        assert!(right[0] < plain[0] && right[2] < plain[2]);
    }

    /// A report figure drawn with axis settings: each setting shows in every
    /// format, and SVG puts the marks where the settings say.
    #[test]
    fn exports_draw_the_axis_settings() {
        use crate::axis_settings::{TickDirection, TickMarkSize};
        use crate::export::{Figure, VectorFormat, export_chart};
        let panel = |axes: Option<PlotAxes>| ChartPanel {
            x_range: (0.0, 10.0),
            y_range: (0.0, 5.0),
            axes,
            ..ChartPanel::default()
        };
        let figure = |format| Figure { title: "", format, width: 400.0, height: 300.0 };
        let mut axes = PlotAxes::default();
        axes.x.tick_direction = TickDirection::Outside;
        axes.x.major_mark = TickMarkSize { length: 8.0, width: 2.0 };
        axes.x.offset = 10.0;
        axes.x.custom_ticks = vec![crate::axis_settings::CustomTick { value: 3.0, label: "\\pi".into() }];
        axes.y.minor = MinorTicks::Auto;
        axes.y.grid = GridLines::None;
        let text = |b: Vec<u8>| String::from_utf8_lossy(&b).into_owned();
        let styled = text(export_chart(&[panel(Some(axes.clone()))], figure(VectorFormat::Svg)));
        let plain = text(export_chart(&[panel(None)], figure(VectorFormat::Svg)));
        assert!(styled.contains(">π</text>"), "the user's label in markup");
        // Outside marks 8 × 0.75 = 6 points long and 1.5 thick, below a line 7.5 points down.
        assert!(styled.contains("stroke-width=\"1.5\""), "{styled}");
        assert!(!styled.contains("<polygon points"), "axis lines drawn apart, not as a frame");
        assert!(plain.contains("<polygon points"), "the standard frame");
        // No horizontal grid lines: none run the plot's width at light grey.
        assert!(!styled.contains("stroke=\"#dcdee4\""), "{styled}");
        assert!(plain.contains("stroke=\"#dcdee4\""));
        for format in [VectorFormat::Pdf, VectorFormat::Eps, VectorFormat::Png] {
            let a = export_chart(&[panel(Some(axes.clone()))], figure(format));
            let b = export_chart(&[panel(None)], figure(format));
            assert!(!a.is_empty() && a != b, "{format:?} draws the settings");
        }
        let pdf = text(export_chart(&[panel(Some(axes))], figure(VectorFormat::Pdf)));
        assert!(pdf.contains("/BaseFont /Symbol"), "π set in the Symbol font");
    }
}
