//! Axis settings for plots: the grid and tick-label functions handed to
//! egui_plot, tick marks along the axes, and the settings menu (opened from a
//! plot's "Plot settings" button or by right-clicking an axis). Titles typed
//! there are read as math markup (`stat_viz::math_text`); axis breaks and
//! the right-hand axis appear where the plot supports them.

use crate::dialog_kit::ComboExt as _;
use std::cell::{Cell, RefCell};
use std::ops::RangeInclusive;
use std::rc::Rc;

use app_model::axis_settings::{
    AxisSettings, AxisSide, GridLines, MajorTicks, MinorTicks, PlotAxes, TickFormat, TickKind,
    TickMarkSize, axis_ticks, custom_tolerance, format_tick, time_label,
};
use app_model::plot_view::ViewBounds;
use eframe::egui::{self, RichText};
use egui_plot::{GridInput, GridMark, PlotTransform};

use crate::theme::{self, PlotPalette};

/// Preferred spacing of automatic major ticks, and the least spacing between
/// labels, in screen points (horizontal labels are wider than vertical ones).
const X_GAPS: (f64, f64) = (90.0, 70.0);
const Y_GAPS: (f64, f64) = (45.0, 26.0);
/// Least distance, in points, between a tick and the top of the vertical axis
/// or the left end of the horizontal axis for its label to be shown (about
/// half the height of a label).
const END_LABEL_ROOM: f64 = 6.0;
/// egui_plot's smallest grid spacing (its default), which scales `GridInput::base_step_size`.
const GRID_MIN_POINTS: f64 = 8.0;

pub type Spacer = Box<dyn Fn(GridInput) -> Vec<GridMark>>;
pub type Formatter = Box<dyn Fn(GridMark, &RangeInclusive<f64>) -> String>;

/// What the axes of one plot show, for the settings menu.
pub struct AxisInfo<'a> {
    /// Default plot title (the section heading, before any subtitle split).
    pub title: &'a str,
    pub x_label: &'a str,
    pub y_label: &'a str,
    /// The horizontal axis shows categories (its ticks follow them).
    pub categorical_x: bool,
    /// The plot has a numeric vertical axis.
    pub has_y: bool,
}

/// Grid spacer and tick-label formatter for an axis with `settings`.
///
/// Majors get full-strength grid lines and minors faint ones. Labels are
/// written only at the labelled majors, which the spacer shares with the
/// formatter for the frame being drawn (on a date axis, with their labels).
pub fn spacer_and_formatter(settings: &AxisSettings, vertical: bool) -> (Spacer, Formatter) {
    let (target, label_gap) = if vertical { Y_GAPS } else { X_GAPS };
    let shared = Rc::new(Cell::new((0.0_f64, 1.0_f64, 1.0_f64)));
    // Bounds and units per point of the frame being drawn.
    let frame = Rc::new(Cell::new((0.0_f64, 0.0_f64, 0.0_f64)));
    let frame_for_spacer = Rc::clone(&frame);
    let time_labels: Rc<RefCell<Vec<(f64, String)>>> = Rc::default();
    let s = settings.clone();
    let for_spacer = Rc::clone(&shared);
    let labels_for_spacer = Rc::clone(&time_labels);
    let spacer: Spacer = Box::new(move |input: GridInput| {
        let units_per_point = input.base_step_size / GRID_MIN_POINTS;
        frame_for_spacer.set((input.bounds.0, input.bounds.1, units_per_point));
        let mut t = axis_ticks(&s, input.bounds.0, input.bounds.1, units_per_point, target, label_gap);
        for_spacer.set((s.origin, t.label_step, t.major_step));
        *labels_for_spacer.borrow_mut() = std::mem::take(&mut t.time_labels);
        let strong = input.base_step_size * 300.0 / GRID_MIN_POINTS;
        let faint = input.base_step_size * 60.0 / GRID_MIN_POINTS;
        t.ticks
            .iter()
            .filter_map(|tick| match tick.kind {
                TickKind::Minor => {
                    (s.grid == GridLines::MajorAndMinor).then_some(GridMark { value: tick.value, step_size: faint })
                }
                _ => Some(GridMark { value: tick.value, step_size: strong }),
            })
            .collect()
    });
    let (format, show, time) = (settings.format, settings.tick_labels, settings.time.is_some());
    let labels = settings.clone();
    let formatter: Formatter = Box::new(move |mark: GridMark, _range: &RangeInclusive<f64>| {
        if !show {
            return String::new();
        }
        let (origin, label_step, major) = shared.get();
        // The user's own labels (as plain text; markup is set after the plot is drawn).
        if let Some(custom) = labels.custom_label(mark.value, custom_tolerance(major)) {
            return pad_tick_label(stat_viz::MathText::parse(custom).plain_text(), &labels, vertical);
        }
        if time {
            return pad_tick_label(time_label(&time_labels.borrow(), mark.value, major), &labels, vertical);
        }
        let k = (mark.value - origin) / label_step;
        if (k - k.round()).abs() > 1e-6 {
            return String::new();
        }
        // A label centred on a tick at the very top of the vertical axis would
        // be cut in half by the plot's edge; leave it out. (At the bottom the
        // horizontal axis's strip leaves room, so a zero baseline keeps its label.)
        let (lo, hi, units_per_point) = frame.get();
        if vertical && units_per_point > 0.0 && mark.value > lo.max(hi) - END_LABEL_ROOM * units_per_point {
            return String::new();
        }
        // Likewise a label at the very left of the horizontal axis would run
        // into the vertical axis's lowest label.
        if !vertical && units_per_point > 0.0 && mark.value < lo.min(hi) + END_LABEL_ROOM * units_per_point {
            return String::new();
        }
        pad_tick_label(format_tick(format, mark.value, major), &labels, vertical)
    });
    (spacer, formatter)
}

/// Rough width of a space and height of a line of tick-label text, in
/// points: tick labels moved out by [`AxisSettings::label_gap`] are padded
/// with at least that much blank text, so egui_plot leaves room for them,
/// and [`PlotChrome::finish`] then moves them out exactly.
const PAD_SPACE: f64 = 3.0;
const PAD_LINE: f64 = 12.0;

/// `text` padded on the side facing the plot when the axis's labels sit
/// further out than usual (offset axis line, tick marks outside): blank
/// lines above or below a horizontal axis's label, spaces beside a vertical
/// one's. Unpadded when the labels sit in their usual place.
#[must_use]
pub fn pad_tick_label(text: String, settings: &AxisSettings, vertical: bool) -> String {
    let gap = settings.label_gap();
    if text.is_empty() || gap <= 0.0 {
        return text;
    }
    let opposite = settings.side == AxisSide::Opposite;
    if vertical {
        let pad = " ".repeat((gap / PAD_SPACE).ceil() as usize);
        if opposite { pad + text.as_str() } else { text + pad.as_str() }
    } else {
        let pad = "\n".repeat((gap / PAD_LINE).ceil() as usize);
        if opposite { pad + text.as_str() } else { text + pad.as_str() }
    }
}

/// Apply `PlotAxes` to an egui_plot `Plot` builder (a macro, so the plot's
/// lifetime-carrying type never has to be written out).
macro_rules! with_axes {
    ($plot:expr, $axes:expr, $categorical_x:expr) => {{
        let axes: &app_model::axis_settings::PlotAxes = $axes;
        let mut plot = $plot;
        if !$categorical_x {
            let (spacer, formatter) = crate::plot_axes::spacer_and_formatter(&axes.x, false);
            plot = plot.x_grid_spacer(spacer).x_axis_formatter(formatter);
        }
        let (spacer, formatter) = crate::plot_axes::spacer_and_formatter(&axes.y, true);
        plot = plot.y_grid_spacer(spacer).y_axis_formatter(formatter);
        use app_model::axis_settings::AxisSide;
        // Grid lines are painted behind the plot by `PlotChrome`.
        plot = plot.show_grid([false, false]);
        if axes.x.side == AxisSide::Opposite {
            plot = plot.x_axis_position(egui_plot::VPlacement::Top);
        }
        if axes.y.side == AxisSide::Opposite {
            plot = plot.y_axis_position(egui_plot::HPlacement::Right);
        }
        if let Some(aspect) = axes.data_aspect() {
            plot = plot.data_aspect(aspect as f32);
        }
        if axes.hover_coordinates && (axes.x.time.is_some() || axes.y.time.is_some()) {
            // Pointer coordinates on a date-time axis read as dates, not seconds.
            let (xt, yt) = (axes.x.time, axes.y.time);
            plot = plot.label_formatter(move |name: &str, p: &egui_plot::PlotPoint| {
                let show =
                    |v: f64, t: Option<stat_data::TemporalDisplay>| t.map_or_else(|| format!("{v:.4}"), |t| t.text(v));
                let head = if name.is_empty() { String::new() } else { format!("{name}\n") };
                format!("{head}x = {}\ny = {}", show(p.x, xt), show(p.y, yt))
            });
        }
        plot.show_x(axes.hover_coordinates).show_y(axes.hover_coordinates)
    }};
}
pub(crate) use with_axes;

/// The plot font and text size of the plot being drawn (its figure look,
/// `stat_viz::plot_style::ExportLook`), which [`PlotChrome::begin`] uses for
/// its titles and tick labels. Set for a plot with [`ScreenText::set`]
/// before drawing it; plots drawn without one use the interface font.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenText {
    /// Titles in the serif plot font.
    pub serif: bool,
    /// Scale of titles and tick labels.
    pub scale: f32,
}

impl Default for ScreenText {
    fn default() -> Self {
        Self { serif: false, scale: 1.0 }
    }
}

impl ScreenText {
    fn id() -> egui::Id {
        egui::Id::new("plot_screen_text")
    }

    /// The setting in force.
    #[must_use]
    pub fn current(ctx: &egui::Context) -> Self {
        ctx.data(|d| d.get_temp(Self::id())).unwrap_or_default()
    }

    /// Draw plots with `look`'s font and text size until the returned guard
    /// is dropped.
    #[must_use]
    pub fn set(ctx: &egui::Context, look: &stat_viz::plot_style::ExportLook) -> ScreenTextGuard {
        let text = Self {
            serif: look.font == stat_viz::plot_style::PlotFont::Serif,
            scale: look.text_scale() as f32,
        };
        ctx.data_mut(|d| d.insert_temp(Self::id(), text));
        ScreenTextGuard(ctx.clone())
    }
}

/// Puts plots back in the interface font when dropped (see [`ScreenText::set`]).
pub struct ScreenTextGuard(egui::Context);

impl Drop for ScreenTextGuard {
    fn drop(&mut self) {
        self.0.data_mut(|d| d.remove_temp::<ScreenText>(ScreenText::id()));
    }
}

/// The look shared by every plot: canvas, grid, frame and axis lines in the
/// light or dark palette.
///
/// Call [`PlotChrome::begin`] just before `Plot::show` and
/// [`PlotChrome::finish`] just after. `begin` reserves a place in the paint
/// order behind the plot; `finish` paints the canvas and the grid there, at the
/// ticks of the frame actually drawn, and the axis lines on top.
pub struct PlotChrome {
    slot: egui::layers::ShapeIdx,
    saved: std::sync::Arc<egui::Style>,
    scale: f32,
    /// Font family of the titles (the plot font, see [`ScreenText`]).
    family: egui::FontFamily,
    vertical_axis: bool,
    spines: bool,
    /// Draw a spine on the right too (a second vertical axis).
    right_spine: bool,
    ctx: egui::Context,
    /// The colors of this plot.
    pub pal: PlotPalette,
}

impl PlotChrome {
    /// Start a plot in `ui`'s appearance.
    pub fn begin(ui: &mut egui::Ui) -> Self {
        let pal = theme::plot_palette(ui);
        let text = ScreenText::current(ui.ctx());
        let scale = theme::text_scale(ui) * text.scale;
        let family = if text.serif { theme::plot_serif() } else { egui::FontFamily::Proportional };
        let slot = ui.painter().add(egui::Shape::Noop);
        let saved = theme::begin_plot(ui, &pal, scale);
        Self {
            slot,
            saved,
            scale,
            family,
            vertical_axis: true,
            spines: true,
            right_spine: false,
            ctx: ui.ctx().clone(),
            pal,
        }
    }

    /// The plot has a second vertical axis on the right: a spine there too.
    #[must_use]
    pub fn with_right_axis(mut self) -> Self {
        self.right_spine = true;
        self
    }

    /// The plot has no vertical axis: no horizontal rules or vertical axis line.
    #[must_use]
    pub fn without_vertical_axis(mut self) -> Self {
        self.vertical_axis = false;
        self
    }

    /// The plot has no numeric axes at all (a ternary plot): no axis lines.
    #[must_use]
    pub fn without_axes(mut self) -> Self {
        self.spines = false;
        self
    }

    /// An axis title in the plot style.
    #[must_use]
    pub fn title(&self, text: impl Into<String>) -> RichText {
        theme::axis_title(text, self.scale, &self.family, &self.pal)
    }

    /// An axis title in math markup (plain text exactly as [`Self::title`]).
    #[must_use]
    pub fn math_title(&self, text: &stat_viz::MathText) -> egui::WidgetText {
        theme::axis_title_math(&self.ctx, text, self.scale, &self.family, &self.pal)
    }

    /// The title of an axis with `settings`: the user's title read as math
    /// markup, else `default` as written (nothing when the title is hidden).
    #[must_use]
    pub fn axis_title(&self, settings: &AxisSettings, default: &str) -> egui::WidgetText {
        self.math_title(&settings.title_text(default, false))
    }

    /// Finish a plot drawn with `transform`: restore the style, paint the
    /// canvas and grid behind the plot, and the axis lines.
    ///
    /// Horizontal rules follow the vertical axis's ticks (from `y_spacer` when
    /// the axis has its own, else from `axes`); vertical rules are drawn only
    /// when turned on for a numeric horizontal axis.
    pub fn finish(
        self,
        ui: &mut egui::Ui,
        transform: &PlotTransform,
        axes: &PlotAxes,
        categorical_x: bool,
        y_spacer: Option<&Spacer>,
    ) {
        ui.set_style(self.saved);
        let pal = self.pal;
        let frame = *transform.frame();
        let mut shapes = vec![egui::Shape::rect_filled(frame, 2.0, pal.plot_bg)];
        let ppp = ui.ctx().pixels_per_point();
        let crisp = |v: f32| ((v * ppp).floor() + 0.5) / ppp;
        let per_point = transform.dvalue_dpos();
        let bounds = transform.bounds();
        if self.vertical_axis && axes.y.grid != GridLines::None {
            let input = GridInput {
                bounds: (bounds.min()[1], bounds.max()[1]),
                base_step_size: per_point[1].abs() * GRID_MIN_POINTS,
            };
            let marks = match y_spacer {
                Some(spacer) => spacer(input),
                None => spacer_and_formatter(&axes.y, true).0(input),
            };
            for (value, major) in classify(&marks) {
                let y = crisp(transform.position_from_point_y(value));
                if y < frame.top() + 0.5 || y > frame.bottom() - 0.5 {
                    continue;
                }
                let color = if major { pal.grid_major } else { pal.grid_major.gamma_multiply(0.5) };
                shapes.push(egui::Shape::line_segment(
                    [egui::pos2(frame.left(), y), egui::pos2(frame.right(), y)],
                    egui::Stroke::new(1.0_f32, color),
                ));
            }
        }
        if !categorical_x && axes.x.grid != GridLines::None {
            let input = GridInput {
                bounds: (bounds.min()[0], bounds.max()[0]),
                base_step_size: per_point[0].abs() * GRID_MIN_POINTS,
            };
            for (value, major) in classify(&spacer_and_formatter(&axes.x, false).0(input)) {
                let x = crisp(transform.position_from_point_x(value));
                if x < frame.left() + 0.5 || x > frame.right() - 0.5 {
                    continue;
                }
                let color = if major { pal.grid_vertical } else { pal.grid_vertical.gamma_multiply(0.5) };
                shapes.push(egui::Shape::line_segment(
                    [egui::pos2(x, frame.top()), egui::pos2(x, frame.bottom())],
                    egui::Stroke::new(1.0_f32, color),
                ));
            }
        }
        ui.painter().set(self.slot, egui::Shape::Vec(shapes));
        place_tick_labels(ui, self.slot, frame, axes);
        if !self.spines {
            return;
        }
        // Axis lines along the sides the axes are drawn on, moved out by
        // their offsets.
        let spine = egui::Stroke::new(1.0_f32, pal.axis_spine);
        let (ox, oy) = (axis_offset(&axes.x), axis_offset(&axes.y));
        let painter = ui.painter_at(frame.expand(1.0 + ox.max(oy)));
        let y = match axes.x.side {
            AxisSide::Standard => crisp(frame.bottom() - 0.5 + ox),
            AxisSide::Opposite => crisp(frame.top() - ox),
        };
        painter.line_segment([egui::pos2(frame.left(), y), egui::pos2(frame.right(), y)], spine);
        let x = match axes.y.side {
            AxisSide::Standard => crisp(frame.left() - oy),
            AxisSide::Opposite => crisp(frame.right() - 0.5 + oy),
        };
        if self.vertical_axis {
            painter.line_segment([egui::pos2(x, frame.top()), egui::pos2(x, frame.bottom())], spine);
        }
        if self.right_spine {
            let x = crisp(frame.right() - 0.5);
            painter.line_segment([egui::pos2(x, frame.top()), egui::pos2(x, frame.bottom())], spine);
        }
    }
}

/// The offset of an axis line from the data area, in screen points.
#[must_use]
pub fn axis_offset(settings: &AxisSettings) -> f32 {
    settings.offset.clamp(0.0, app_model::axis_settings::MAX_AXIS_OFFSET) as f32
}

/// Move the tick labels egui_plot drew (the shapes after `from`) out by
/// their axis's [`AxisSettings::label_gap`], and set the user's labels
/// written in math markup as markup. Labels padded by [`pad_tick_label`]
/// are moved so the text sits `gap` beyond its usual place.
fn place_tick_labels(ui: &egui::Ui, from: egui::layers::ShapeIdx, frame: egui::Rect, axes: &PlotAxes) {
    let (gap_x, gap_y) = (axes.x.label_gap() as f32, axes.y.label_gap() as f32);
    let markup: Vec<(String, stat_viz::MathText)> = [&axes.x, &axes.y]
        .iter()
        .flat_map(|a| a.custom_ticks.iter())
        .map(|c| stat_viz::MathText::parse(&c.label))
        .filter(|m| m.as_plain().is_none())
        .map(|m| (m.plain_text(), m))
        .collect();
    if gap_x <= 0.0 && gap_y <= 0.0 && markup.is_empty() {
        return;
    }
    let ctx = ui.ctx().clone();
    let layer = ui.layer_id();
    let end = ctx.graphics(|g| g.get(layer).map(|l| l.next_idx().0)).unwrap_or(0);
    for idx in from.0 + 1..end {
        let mut swap: Option<(String, egui::Pos2, egui::Align2, egui::Color32, egui::FontId)> = None;
        ctx.graphics_mut(|g| {
            g.entry(layer).mutate_shape(egui::layers::ShapeIdx(idx), |clipped| {
                let egui::Shape::Text(t) = &mut clipped.shape else { return };
                let text = t.galley.job.text.as_str();
                let trimmed = text.trim_matches(|c| c == ' ' || c == '\n');
                if trimmed.is_empty() {
                    return;
                }
                let size = t.galley.size();
                let (glyph_left, glyph_right) = t
                    .galley
                    .rows
                    .iter()
                    .flat_map(|r| r.glyphs.iter())
                    .filter(|g| !g.chr.is_whitespace())
                    .fold((f32::INFINITY, f32::NEG_INFINITY), |(l, r), g| {
                        (l.min(g.pos.x), r.max(g.pos.x + g.advance_width))
                    });
                if !glyph_left.is_finite() {
                    return;
                }
                let rect = egui::Rect::from_min_size(t.pos, size);
                let row_h = t.galley.rows.first().map_or(size.y, |r| r.rect.height());
                // Where the label is and how far it moves.
                let delta = if rect.min.y >= frame.bottom() - 1.0 && text.ends_with('\n') {
                    egui::vec2(0.0, gap_x)
                } else if rect.max.y <= frame.top() + 1.0 && text.starts_with('\n') {
                    egui::vec2(0.0, -gap_x)
                } else if rect.max.x <= frame.left() + 1.0 && text.ends_with(' ') {
                    egui::vec2(size.x - gap_y - glyph_right, 0.0)
                } else if rect.min.x >= frame.right() - 1.0 && text.starts_with(' ') {
                    egui::vec2(gap_y - glyph_left, 0.0)
                } else {
                    let outside = rect.min.y >= frame.bottom() - 1.0
                        || rect.max.y <= frame.top() + 1.0
                        || rect.max.x <= frame.left() + 1.0
                        || rect.min.x >= frame.right() - 1.0;
                    if !outside {
                        return;
                    }
                    egui::Vec2::ZERO
                };
                t.pos += delta;
                if let Some((_, m)) = markup.iter().find(|(plain, _)| plain == trimmed) {
                    // The visible text's box after the move.
                    let first_text_row = t.galley.rows.iter().find(|r| r.glyphs.iter().any(|g| !g.chr.is_whitespace()));
                    let top = t.pos.y + first_text_row.map_or(0.0, |r| r.rect.min.y);
                    let (left, right) = (t.pos.x + glyph_left, t.pos.x + glyph_right);
                    let (anchor, at) = if rect.max.x <= frame.left() + 1.0 {
                        (egui::Align2::RIGHT_CENTER, egui::pos2(right, top + row_h / 2.0))
                    } else if rect.min.x >= frame.right() - 1.0 {
                        (egui::Align2::LEFT_CENTER, egui::pos2(left, top + row_h / 2.0))
                    } else if rect.max.y <= frame.top() + 1.0 {
                        (egui::Align2::CENTER_BOTTOM, egui::pos2((left + right) / 2.0, top + row_h))
                    } else {
                        (egui::Align2::CENTER_TOP, egui::pos2((left + right) / 2.0, top))
                    };
                    let font = t
                        .galley
                        .job
                        .sections
                        .first()
                        .map_or_else(|| egui::FontId::monospace(12.0), |s| s.format.font_id.clone());
                    swap = Some((m.plain_text(), at, anchor, t.fallback_color, font));
                    clipped.shape = egui::Shape::Noop;
                }
            });
        });
        if let Some((plain, at, anchor, color, font)) = swap {
            let Some((_, m)) = markup.iter().find(|(p, _)| *p == plain) else { continue };
            let galley = stat_viz::math_text::egui_text::galley(&ctx, m, &font, color);
            let pos = anchor.anchor_size(at, galley.size()).min;
            let shape = egui::Shape::galley(pos, galley, color);
            ctx.graphics_mut(|g| g.entry(layer).mutate_shape(egui::layers::ShapeIdx(idx), |c| c.shape = shape));
        }
    }
}

/// Grid marks at a broken axis's ticks: each piece's own nice values,
/// about one labelled tick per 45 points up the axis or 90 across.
pub fn broken_spacer(axis: stat_viz::AxisSpec, b: stat_viz::BrokenAxis, vertical: bool) -> Spacer {
    let spacing = if vertical { 45.0 } else { 90.0 };
    Box::new(move |input: GridInput| {
        let length = (input.bounds.1 - input.bounds.0) / input.base_step_size * 8.0;
        let target = ((length / spacing).round() as usize).clamp(2, 12);
        let ticks = stat_viz::export::broken_axis_ticks(&axis, input.bounds.0, input.bounds.1, &b, target);
        ticks.into_iter().map(|(value, _)| GridMark { value, step_size: 1e6 }).collect()
    })
}

/// A plain continuous axis (report plots draw data units on a linear scale).
pub fn linear_axis() -> stat_viz::AxisSpec {
    stat_viz::AxisSpec {
        name: String::new(),
        kind: stat_viz::frame::AxisKind::Continuous,
        scale: stat_viz::AxisScale::Linear,
        time: None,
    }
}

/// `plot` with the ticks and tick labels of its broken axes (`[x, y]`,
/// linear axes in data units): each piece labelled on its own scale.
pub fn with_broken_axes<'a>(
    mut plot: egui_plot::Plot<'a>,
    breaks: &[Option<stat_viz::BrokenAxis>; 2],
    axes: &PlotAxes,
) -> egui_plot::Plot<'a> {
    if let Some(b) = breaks[0].clone() {
        let (settings, labels) = (axes.x.clone(), b.clone());
        plot = plot.x_grid_spacer(broken_spacer(linear_axis(), b, false)).x_axis_formatter(move |mark, _| {
            let text = if settings.tick_labels { labels.label_at(&linear_axis(), mark.value) } else { String::new() };
            pad_tick_label(text, &settings, false)
        });
    }
    if let Some(b) = breaks[1].clone() {
        let (settings, labels) = (axes.y.clone(), b.clone());
        plot = plot.y_grid_spacer(broken_spacer(linear_axis(), b, true)).y_axis_formatter(move |mark, _| {
            let text = if settings.tick_labels { labels.label_at(&linear_axis(), mark.value) } else { String::new() };
            pad_tick_label(text, &settings, true)
        });
    }
    plot
}

/// Tick marks of broken axes (`(axis, map)` for each broken one), at each
/// piece's own ticks.
pub fn paint_broken_tick_marks(
    ui: &egui::Ui,
    transform: &PlotTransform,
    axes: &PlotAxes,
    x: Option<(&stat_viz::AxisSpec, &stat_viz::BrokenAxis)>,
    y: Option<(&stat_viz::AxisSpec, &stat_viz::BrokenAxis)>,
) {
    let frame = *transform.frame();
    let bounds = transform.bounds();
    if let Some((axis, b)) = x.filter(|_| axes.x.tick_marks) {
        let target = ((frame.width() / 90.0).round() as usize).clamp(2, 12);
        let ticks = stat_viz::export::broken_axis_ticks(axis, bounds.min()[0], bounds.max()[0], b, target);
        paint_axis_marks(ui, transform, &axes.x, false, ticks.into_iter().map(|(t, _)| (t, false)));
    }
    if let Some((axis, b)) = y.filter(|_| axes.y.tick_marks) {
        let target = ((frame.height() / 45.0).round() as usize).clamp(2, 12);
        let ticks = stat_viz::export::broken_axis_ticks(axis, bounds.min()[1], bounds.max()[1], b, target);
        paint_axis_marks(ui, transform, &axes.y, true, ticks.into_iter().map(|(t, _)| (t, false)));
    }
}

/// The marks where axes are broken (slashes or a zigzag, as each axis's
/// breaks say), across the spine at every cut of the horizontal axis
/// `x` (bottom spine, or top when the axis is drawn there) and of the
/// vertical axis `y` (left spine, and the right one when `right` is set);
/// the spine under each mark is painted in the plot's background so it
/// reads as cut.
pub fn paint_break_marks(
    ui: &egui::Ui,
    transform: &PlotTransform,
    axes: &PlotAxes,
    x: Option<&stat_viz::BrokenAxis>,
    y: Option<&stat_viz::BrokenAxis>,
    right: bool,
) {
    let pal = theme::plot_palette(ui);
    let frame = *transform.frame();
    let painter = ui.painter_at(frame.expand(8.0));
    let stroke = egui::Stroke::new(1.2_f32, pal.axis_spine);
    let (half, gap, slant) = (6.0, 2.0, 2.5);
    let mark = |c: egui::Pos2, vertical_spine: bool, style: stat_viz::BreakMark| {
        let at = [f64::from(c.x), f64::from(c.y)];
        let pos = |p: &[f64; 2]| egui::pos2(p[0] as f32, p[1] as f32);
        let cover = style.cover(at, vertical_spine, half, gap, slant).iter().map(pos).collect();
        painter.add(egui::Shape::convex_polygon(cover, pal.plot_bg, egui::Stroke::NONE));
        for line in style.strokes(at, vertical_spine, half, gap, slant) {
            painter.add(egui::Shape::line(line.iter().map(pos).collect(), stroke));
        }
    };
    if let Some(b) = x {
        for v in b.cuts() {
            let px = transform.position_from_point_x(v);
            if px > frame.left() && px < frame.right() {
                let py = if axes.x.side == AxisSide::Opposite { frame.top() } else { frame.bottom() };
                mark(egui::pos2(px, py), false, b.mark);
            }
        }
    }
    if let Some(b) = y {
        for v in b.cuts() {
            let py = transform.position_from_point_y(v);
            if py > frame.top() && py < frame.bottom() {
                let px = if axes.y.side == AxisSide::Opposite { frame.right() } else { frame.left() };
                mark(egui::pos2(px, py), true, b.mark);
                if right {
                    mark(egui::pos2(frame.right(), py), true, b.mark);
                }
            }
        }
    }
}

/// Grid marks as (value, is major): the marks with the largest step are the
/// majors.
fn classify(marks: &[GridMark]) -> Vec<(f64, bool)> {
    let top = marks.iter().map(|m| m.step_size).fold(0.0_f64, f64::max);
    marks.iter().map(|m| (m.value, m.step_size >= top * 0.99)).collect()
}

/// Category labels under (or over) a categorical horizontal axis, turned as
/// the plot's Plot settings say (see [`stat_viz::category_labels`]).
///
/// Level labels are written by egui_plot itself. Slanted and upright labels
/// are painted here, after the plot, in a strip whose height is reserved
/// from the layout the previous frame needed (egui_plot sizes its axes the
/// same way); a change of layout repaints at once.
pub struct CategoryAxis {
    key: egui::Id,
    setting: stat_viz::CategoryLabels,
    font: egui::FontId,
    widest: f32,
    line: f32,
    title_height: f32,
    /// The layout the axis strip is sized for this frame.
    layout: stat_viz::CategoryLayout,
}

impl CategoryAxis {
    /// The labels `ticks` of the plot keyed `key`, turned per `setting`.
    pub fn new(ui: &egui::Ui, key: egui::Id, ticks: &[(f64, String)], setting: stat_viz::CategoryLabels) -> Self {
        let scale = theme::text_scale(ui);
        let font = egui::FontId::monospace(theme::TICK_LABEL_SIZE * scale);
        let (widest, line) = ui.fonts(|f| {
            let widest = ticks
                .iter()
                .map(|t| f.layout_no_wrap(t.1.clone(), font.clone(), egui::Color32::WHITE).size().x)
                .fold(0.0_f32, f32::max);
            (widest, f.row_height(&font))
        });
        let level = stat_viz::CategoryLayout { turn: stat_viz::LabelTurn::Horizontal, step: 1 };
        let layout = ui.data(|d| d.get_temp::<stat_viz::CategoryLayout>(key)).unwrap_or(level);
        let title_height = theme::AXIS_TITLE_SIZE * scale * 1.6;
        Self { key, setting, font, widest, line, title_height, layout }
    }

    /// Whether the labels are turned (so painted here, not by egui_plot).
    pub fn turned(&self) -> bool {
        self.layout.turn != stat_viz::LabelTurn::Horizontal
    }

    /// Height of the horizontal axis's strip: the turned labels and the
    /// axis title below them.
    pub fn axis_thickness(&self) -> f32 {
        let depth =
            stat_viz::category_labels::label_depth(self.layout.turn, f64::from(self.widest), f64::from(self.line));
        6.0 + depth as f32 + self.title_height
    }

    /// How much taller the plot is drawn than its set height, so turned
    /// labels take room below the plot rather than from its data area.
    pub fn extra_height(&self) -> f32 {
        if self.turned() {
            let depth =
                stat_viz::category_labels::label_depth(self.layout.turn, f64::from(self.widest), f64::from(self.line));
            (depth as f32 - self.line).max(0.0)
        } else {
            0.0
        }
    }

    /// Choose the layout from the frame drawn (remembered for the next
    /// frame's strip) and, when the labels are turned, paint them: each ends
    /// at its tick below the plot (starts at it above, on an axis drawn on
    /// the opposite side).
    pub fn paint(&self, ui: &egui::Ui, transform: &PlotTransform, ticks: &[(f64, String)], side: AxisSide) {
        let positions: Vec<f64> = ticks.iter().map(|t| t.0).collect();
        let spacing = stat_viz::category_labels::least_spacing(&positions) * transform.dpos_dvalue_x().abs();
        let layout = stat_viz::category_labels::category_layout(
            self.setting,
            spacing,
            f64::from(self.widest),
            f64::from(self.line),
        );
        if layout != self.layout {
            ui.data_mut(|d| d.insert_temp(self.key, layout));
            ui.ctx().request_repaint();
        }
        if !self.turned() {
            return;
        }
        let frame = *transform.frame();
        let color = theme::plot_palette(ui).tick_text;
        let angle = match self.layout.turn {
            stat_viz::LabelTurn::Slanted => -std::f32::consts::FRAC_PI_4,
            _ => -std::f32::consts::FRAC_PI_2,
        };
        // Along the text, and down across it, once turned.
        let (along, across) = (egui::Vec2::angled(angle), egui::Vec2::angled(angle + std::f32::consts::FRAC_PI_2));
        let painter = ui.painter();
        for (k, (value, text)) in ticks.iter().enumerate() {
            if k % self.layout.step.max(1) != 0 {
                continue;
            }
            let x = transform.position_from_point_x(*value);
            if !(frame.left() - 0.5..=frame.right() + 0.5).contains(&x) {
                continue;
            }
            let galley = painter.layout_no_wrap(text.clone(), self.font.clone(), color);
            let (w, h) = (galley.size().x, galley.size().y);
            let pos = match side {
                AxisSide::Standard => egui::pos2(x, frame.bottom() + 4.0) - along * w - across * (h / 2.0),
                AxisSide::Opposite => egui::pos2(x, frame.top() - 4.0) - across * (h / 2.0),
            };
            painter.add(egui::epaint::TextShape::new(pos, galley, color).with_angle(angle));
        }
    }
}

/// Tick marks along the axes as their settings say: inside the plot frame,
/// outside it or across the axis line, at the axis line's offset; majors 5
/// points long and minors 3 unless set otherwise.
pub fn paint_tick_marks(ui: &egui::Ui, transform: &PlotTransform, axes: &PlotAxes, categorical_x: bool) {
    let bounds = transform.bounds();
    let per_point = transform.dvalue_dpos();
    for (vertical, settings) in [(false, &axes.x), (true, &axes.y)] {
        if !settings.tick_marks || (!vertical && categorical_x) {
            continue;
        }
        let i = usize::from(vertical);
        let (target, gap) = if vertical { Y_GAPS } else { X_GAPS };
        let t = axis_ticks(settings, bounds.min()[i], bounds.max()[i], per_point[i].abs(), target, gap);
        let values = t.ticks.iter().map(|k| (k.value, k.kind == TickKind::Minor));
        paint_axis_marks(ui, transform, settings, vertical, values);
    }
}

/// Marks at `values` (data units, each with whether it is a minor tick)
/// along one axis drawn with `settings`: their direction, length,
/// thickness and the axis line's offset.
pub fn paint_axis_marks(
    ui: &egui::Ui,
    transform: &PlotTransform,
    settings: &AxisSettings,
    vertical: bool,
    values: impl Iterator<Item = (f64, bool)>,
) {
    let frame = *transform.frame();
    let offset = axis_offset(settings);
    let reach = offset + 31.0;
    let painter = ui.painter_at(frame.expand(reach));
    let color = theme::plot_palette(ui).axis_spine;
    for (value, minor) in values {
        let size: TickMarkSize = if minor { settings.minor_mark } else { settings.major_mark }.clamped();
        let (inside, outside) = settings.tick_direction.extent(size.length);
        let (inside, outside) = (inside as f32, outside as f32);
        let stroke = egui::Stroke::new(size.width as f32, color);
        if vertical {
            let y = transform.position_from_point_y(value);
            if y < frame.top() - 0.5 || y > frame.bottom() + 0.5 {
                continue;
            }
            let (x0, x1) = match settings.side {
                AxisSide::Standard => (frame.left() - offset - outside, frame.left() - offset + inside),
                AxisSide::Opposite => (frame.right() + offset - inside, frame.right() + offset + outside),
            };
            painter.line_segment([egui::pos2(x0, y), egui::pos2(x1, y)], stroke);
        } else {
            let x = transform.position_from_point_x(value);
            if x < frame.left() - 0.5 || x > frame.right() + 0.5 {
                continue;
            }
            let (y0, y1) = match settings.side {
                AxisSide::Standard => (frame.bottom() + offset - inside, frame.bottom() + offset + outside),
                AxisSide::Opposite => (frame.top() - offset - outside, frame.top() - offset + inside),
            };
            painter.line_segment([egui::pos2(x, y0), egui::pos2(x, y1)], stroke);
        }
    }
}

/// A tick label being written in place, on the plot.
#[derive(Debug, Clone, PartialEq)]
pub struct TickEdit {
    /// On the vertical axis.
    pub vertical: bool,
    /// The tick's value (data units).
    pub value: f64,
    /// The label as typed (math markup).
    pub text: String,
    /// Where the editor opens (screen).
    pub at: egui::Pos2,
    /// The text field has been given the keyboard focus.
    focused: bool,
}

impl TickEdit {
    /// Edit the label of the labelled tick nearest `pos` along the axis
    /// (within 30 points), else of a new tick at the value under `pos`.
    pub fn at(transform: &PlotTransform, axes: &PlotAxes, vertical: bool, pos: egui::Pos2) -> Self {
        let settings = if vertical { &axes.y } else { &axes.x };
        let i = usize::from(vertical);
        let bounds = transform.bounds();
        let (target, gap) = if vertical { Y_GAPS } else { X_GAPS };
        let per_point = transform.dvalue_dpos()[i].abs();
        let t = axis_ticks(settings, bounds.min()[i], bounds.max()[i], per_point, target, gap);
        let screen = |v: f64| {
            if vertical { transform.position_from_point_y(v) } else { transform.position_from_point_x(v) }
        };
        let along = if vertical { pos.y } else { pos.x };
        let nearest = t
            .ticks
            .iter()
            .filter(|k| k.kind == TickKind::Labelled)
            .map(|k| (k.value, (screen(k.value) - along).abs()))
            .filter(|(_, d)| *d <= 30.0)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        let tolerance = custom_tolerance(t.major_step);
        let (value, text) = match nearest {
            Some((v, _)) => {
                let text = settings.custom_label(v, tolerance).map_or_else(
                    || if settings.time.is_some() { time_label(&t.time_labels, v, t.major_step) } else { format_tick(settings.format, v, t.major_step) },
                    str::to_owned,
                );
                (v, text)
            }
            None => {
                let v = if vertical {
                    transform.value_from_position(pos).y
                } else {
                    transform.value_from_position(pos).x
                };
                // Rounded to a tenth of the major step.
                let step = t.major_step / 10.0;
                let v = if step > 0.0 { (v / step).round() * step } else { v };
                (v, format_tick(settings.format, v, step.max(f64::MIN_POSITIVE)))
            }
        };
        Self { vertical, value, text, at: pos + egui::vec2(8.0, 8.0), focused: false }
    }
}

/// The in-place editor of a tick label: the label (math markup), Apply,
/// "Use the number" (drops the user's label) and Cancel; Enter applies and
/// Escape cancels. Returns `false` once closed.
pub fn tick_label_editor(ui: &egui::Ui, id: egui::Id, edit: &mut TickEdit, axes: &mut PlotAxes) -> bool {
    let mut open = true;
    let mut apply: Option<Option<String>> = None;
    egui::Area::new(id.with("tick_label_editor")).order(egui::Order::Foreground).fixed_pos(edit.at).show(
        ui.ctx(),
        |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                let at = app_model::axis_settings::format_tick(TickFormat::Auto, edit.value, edit.value.abs().max(1e-12) / 1e6);
                ui.label(RichText::new(format!("Tick label at {at}")).strong());
                let field = ui
                    .add(egui::TextEdit::singleline(&mut edit.text).desired_width(180.0).hint_text("no label"))
                    .on_hover_text(MARKUP_HINT);
                field.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Tick label"));
                if !edit.focused {
                    field.request_focus();
                    edit.focused = true;
                }
                if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    apply = Some(Some(edit.text.clone()));
                }
                ui.horizontal(|ui| {
                    if ui.button("Apply").clicked() {
                        apply = Some(Some(edit.text.clone()));
                    }
                    if ui.button("Use the number").on_hover_text("Drop your label at this tick").clicked() {
                        apply = Some(None);
                    }
                    if ui.button("Cancel").clicked() {
                        open = false;
                    }
                });
            });
        },
    );
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        open = false;
    }
    if let Some(label) = apply {
        let s = if edit.vertical { &mut axes.y } else { &mut axes.x };
        let tolerance = custom_tolerance(edit.value.abs().max(1.0));
        s.set_custom_label(edit.value, label, tolerance);
        open = false;
    }
    open
}

/// The view a report plot shows: the user's dragged or typed limits
/// (`bounds`), else its default view with each axis's typed range
/// ([`AxisSettings::range`]) or, when asked, its automatic range widened
/// to the nearest major ticks, for a frame `frame_size` points large.
#[must_use]
pub fn view_to_show(
    bounds: Option<ViewBounds>,
    default_view: Option<ViewBounds>,
    axes: &PlotAxes,
    frame_size: egui::Vec2,
    categorical_x: bool,
) -> Option<ViewBounds> {
    if bounds.is_some() {
        return bounds;
    }
    let mut v = default_view?;
    let axis = |s: &AxisSettings, lo: f64, hi: f64, length: f32, target: f64, categorical: bool| match s.range {
        Some([a, b]) if a < b => (a, b),
        _ if s.extend_to_ticks && !categorical => {
            app_model::axis_settings::extended_to_ticks(s, lo, hi, f64::from(length), target)
        }
        _ => (lo, hi),
    };
    (v.x_min, v.x_max) = axis(&axes.x, v.x_min, v.x_max, frame_size.x, X_GAPS.0, categorical_x);
    (v.y_min, v.y_max) = axis(&axes.y, v.y_min, v.y_max, frame_size.y, Y_GAPS.0, false);
    Some(v)
}

/// The labelled ticks of the horizontal axis as drawn: `(value, screen x)`,
/// for tables aligned under a plot (such as a number-at-risk table).
#[must_use]
pub fn labelled_x_ticks(transform: &PlotTransform, axes: &PlotAxes) -> Vec<(f64, f32)> {
    let bounds = transform.bounds();
    let per_point = transform.dvalue_dpos();
    let (target, gap) = X_GAPS;
    axis_ticks(&axes.x, bounds.min()[0], bounds.max()[0], per_point[0].abs(), target, gap)
        .ticks
        .into_iter()
        .filter(|t| t.kind == TickKind::Labelled)
        .map(|t| (t.value, transform.position_from_point_x(t.value)))
        .collect()
}

/// Text labels painted over a plot in screen space, each moved the least
/// distance up or down needed to clear the labels placed before it (and any
/// `obstacles`, such as the points they label), and kept inside `frame`.
pub struct LabelLayer {
    frame: egui::Rect,
    placed: Vec<egui::Rect>,
    obstacles: Vec<egui::Rect>,
    shapes: Vec<egui::Shape>,
}

impl LabelLayer {
    /// Labels inside `frame` (the plot area, in screen points).
    #[must_use]
    pub fn new(frame: egui::Rect) -> Self {
        Self { frame, placed: Vec::new(), obstacles: Vec::new(), shapes: Vec::new() }
    }

    /// `r` moved the least distance that brings it inside the frame.
    fn inside(&self, r: egui::Rect) -> egui::Rect {
        let dx = (self.frame.left() - r.left()).max(0.0) - (r.right() - self.frame.right()).max(0.0);
        let dy = (self.frame.top() - r.top()).max(0.0) - (r.bottom() - self.frame.bottom()).max(0.0);
        r.translate(egui::vec2(dx, dy))
    }

    /// Keep labels off `rect` (a marker, say).
    pub fn avoid(&mut self, rect: egui::Rect) {
        self.obstacles.push(rect);
    }

    /// Where a label of `size`, anchored at `pos` by `anchor`, goes: its own
    /// place if free, else the nearest free place above or below it, else
    /// its own place (inside the frame either way).
    pub fn place(&mut self, pos: egui::Pos2, anchor: egui::Align2, size: egui::Vec2) -> egui::Rect {
        let home = self.inside(anchor.anchor_size(pos, size));
        let free = |r: &egui::Rect| {
            self.frame.expand(0.5).contains_rect(*r)
                && !self.placed.iter().chain(&self.obstacles).any(|o| o.shrink(0.5).intersects(*r))
        };
        let step = (size.y * 0.5).max(1.0);
        let rect = (0..60)
            .flat_map(|k| [-(k as f32) * step, k as f32 * step])
            .map(|dy| home.translate(egui::vec2(0.0, dy)))
            .find(|r| free(r))
            .unwrap_or(home);
        self.placed.push(rect);
        rect
    }

    /// Place and paint `text` only if its own place is free; `None` (and
    /// nothing painted) if it would cover a label already placed.
    pub fn text_if_free(
        &mut self,
        ui: &egui::Ui,
        pos: egui::Pos2,
        anchor: egui::Align2,
        text: impl Into<String>,
        font: egui::FontId,
        color: egui::Color32,
    ) -> Option<egui::Rect> {
        let galley = ui.fonts(|f| f.layout_no_wrap(text.into(), font, color));
        let rect = self.inside(anchor.anchor_size(pos, galley.size()));
        if self.placed.iter().any(|o| o.expand(2.0).intersects(rect)) {
            return None;
        }
        self.placed.push(rect);
        self.shapes.push(egui::Shape::galley(rect.min, galley, color));
        Some(rect)
    }

    /// Place and paint `text` in `font` and `color`.
    pub fn text(
        &mut self,
        ui: &egui::Ui,
        pos: egui::Pos2,
        anchor: egui::Align2,
        text: impl Into<String>,
        font: egui::FontId,
        color: egui::Color32,
    ) -> egui::Rect {
        let galley = ui.fonts(|f| f.layout_no_wrap(text.into(), font, color));
        let rect = self.place(pos, anchor, galley.size());
        self.shapes.push(egui::Shape::galley(rect.min, galley, color));
        rect
    }

    /// Paint the labels, clipped to the frame.
    pub fn paint(self, ui: &egui::Ui) {
        ui.painter_at(self.frame).extend(self.shapes);
    }
}

/// The full settings menu of a plot: its title, both axes and the plot area.
pub fn axes_menu(
    ui: &mut egui::Ui,
    axes: &mut PlotAxes,
    bounds: &mut Option<ViewBounds>,
    view: Option<ViewBounds>,
    info: &AxisInfo<'_>,
) {
    ui.set_min_width(330.0);
    egui::ScrollArea::vertical().max_height(560.0).show(ui, |ui| {
        title_row(ui, axes, info.title);
        ui.separator();
        let [x_breaks, y_breaks] = axes.breaks;
        egui::CollapsingHeader::new(RichText::new("Horizontal axis (x)").strong())
            .id_salt("axes_menu_x")
            .default_open(true)
            .show(ui, |ui| {
                axis_section(ui, "x", &mut axes.x, bounds, view, info.x_label, info.categorical_x, x_breaks);
                if info.categorical_x {
                    category_labels_row(ui, &mut axes.category_labels);
                }
            });
        if info.has_y {
            egui::CollapsingHeader::new(RichText::new("Vertical axis (y)").strong())
                .id_salt("axes_menu_y")
                .default_open(true)
                .show(ui, |ui| axis_section(ui, "y", &mut axes.y, bounds, view, info.y_label, false, y_breaks));
        }
        if let Some(label) = axes.y2_label.clone() {
            egui::CollapsingHeader::new(RichText::new("Right axis (y2)").strong())
                .id_salt("axes_menu_y2")
                .default_open(true)
                .show(ui, |ui| right_axis_section(ui, &mut axes.y2, &label));
        }
        egui::CollapsingHeader::new(RichText::new("Plot area").strong())
            .id_salt("axes_menu_plot")
            .default_open(true)
            .show(ui, |ui| plot_section(ui, axes, bounds));
    });
}

/// The choice of how category labels along the horizontal axis are turned.
fn category_labels_row(ui: &mut egui::Ui, choice: &mut stat_viz::CategoryLabels) {
    ui.horizontal(|ui| {
        ui.label("Category labels:").on_hover_text(
            "Automatic keeps the labels level when they fit, slants them when they would overlap, and stands \
             them upright when even slanted labels would",
        );
        egui::ComboBox::from_id_salt("category_labels").selected_text(choice.label()).width(140.0).show_list(
            ui,
            |ui| {
                for option in stat_viz::CategoryLabels::ALL {
                    ui.selectable_value(choice, option, option.label());
                }
            },
        );
    });
}

/// The settings of one axis only (from right-clicking that axis).
pub fn single_axis_menu(
    ui: &mut egui::Ui,
    vertical: bool,
    axes: &mut PlotAxes,
    bounds: &mut Option<ViewBounds>,
    view: Option<ViewBounds>,
    info: &AxisInfo<'_>,
) {
    ui.set_min_width(330.0);
    ui.label(RichText::new(if vertical { "Vertical axis (y)" } else { "Horizontal axis (x)" }).strong());
    let [x_breaks, y_breaks] = axes.breaks;
    if vertical {
        axis_section(ui, "y", &mut axes.y, bounds, view, info.y_label, false, y_breaks);
    } else {
        axis_section(ui, "x", &mut axes.x, bounds, view, info.x_label, info.categorical_x, x_breaks);
        if info.categorical_x {
            category_labels_row(ui, &mut axes.category_labels);
        }
    }
    ui.label(
        RichText::new("Title, both axes and the plot area: the Plot settings button above the plot.").weak().small(),
    );
}

/// The settings of the right-hand vertical axis (from right-clicking it).
pub fn right_axis_menu(ui: &mut egui::Ui, axes: &mut PlotAxes) {
    ui.set_min_width(330.0);
    ui.label(RichText::new("Right axis (y2)").strong());
    let label = axes.y2_label.clone().unwrap_or_default();
    right_axis_section(ui, &mut axes.y2, &label);
    ui.label(
        RichText::new("Its range follows dragging this axis; double-click the plot to fit it again.").weak().small(),
    );
}

/// What a title field's tooltip says about math markup.
pub const MARKUP_HINT: &str = "Math markup: x_1 or x_{ij} for subscripts, x^2 for superscripts, \\alpha \\mu \\Delta \\sigma \
     for Greek letters, \\frac{a}{b} (or $a/b$) for a fraction; \\_ \\^ \\$ \\\\ for the characters themselves";

/// Title, tick labels and tick marks of the right-hand axis.
fn right_axis_section(ui: &mut egui::Ui, s: &mut AxisSettings, default_title: &str) {
    egui::Grid::new("axis_settings_y2").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
        ui.label("Title");
        ui.horizontal(|ui| {
            ui.checkbox(&mut s.show_title, "");
            let mut text = s.title.clone().unwrap_or_default();
            let edit = egui::TextEdit::singleline(&mut text).hint_text(default_title).desired_width(200.0);
            if ui.add_enabled(s.show_title, edit).on_hover_text(MARKUP_HINT).changed() {
                s.title = if text.trim().is_empty() { None } else { Some(text) };
            }
        });
        ui.end_row();
        ui.label("Show");
        ui.horizontal(|ui| {
            ui.checkbox(&mut s.tick_marks, "Tick marks");
            ui.checkbox(&mut s.tick_labels, "Tick labels");
        });
        ui.end_row();
    });
    if ui.small_button("Reset the right axis").clicked() {
        *s = AxisSettings::default();
    }
}

/// The plot title: shown or hidden, and its text (empty for the default).
fn title_row(ui: &mut egui::Ui, axes: &mut PlotAxes, heading: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Plot title").strong());
        ui.checkbox(&mut axes.show_title, "Show");
    });
    let mut text = axes.title.clone().unwrap_or_default();
    let hint = app_model::axis_settings::split_title(heading).0;
    let edit = egui::TextEdit::singleline(&mut text).hint_text(hint).desired_width(f32::INFINITY);
    let hint = format!("Leave empty for the default title. {MARKUP_HINT}");
    if ui.add_enabled(axes.show_title, edit).on_hover_text(hint).changed() {
        axes.title = if text.trim().is_empty() { None } else { Some(text) };
    }
}

fn plot_section(ui: &mut egui::Ui, axes: &mut PlotAxes, bounds: &mut Option<ViewBounds>) {
    ui.checkbox(&mut axes.equal_scale, "Equal scale (one unit is the same length on both axes)");
    ui.add_enabled_ui(axes.equal_scale, |ui| {
        ui.horizontal(|ui| {
            ui.label("Scale ratio: one x unit as long as");
            ui.add(egui::DragValue::new(&mut axes.scale_ratio).range(0.001..=1000.0).speed(0.01).suffix(" y units"));
        });
    });
    ui.horizontal(|ui| {
        let mut on = axes.print_scale.is_some();
        if ui
            .checkbox(&mut on, "Print scale")
            .on_hover_text("Saved figures draw one horizontal unit this many millimetres long")
            .changed()
        {
            axes.print_scale = on.then_some(10.0);
        }
        if let Some(mm) = &mut axes.print_scale {
            ui.add(egui::DragValue::new(mm).range(0.001..=10_000.0).speed(0.1).prefix("1 x unit = ").suffix(" mm"));
        }
    });
    ui.checkbox(&mut axes.hover_coordinates, "Show pointer coordinates");
    ui.checkbox(&mut axes.show_key, "Show key below the plot");
    ui.horizontal(|ui| {
        if ui.button("Fit view to data").on_hover_text("Automatic range on both axes").clicked() {
            *bounds = None;
            axes.x.range = None;
            axes.y.range = None;
        }
        if ui.button("Restore default settings").clicked() {
            *axes = PlotAxes::default();
            *bounds = None;
        }
    });
}

#[allow(clippy::too_many_lines, clippy::too_many_arguments)]
fn axis_section(
    ui: &mut egui::Ui,
    id: &str,
    s: &mut AxisSettings,
    bounds: &mut Option<ViewBounds>,
    view: Option<ViewBounds>,
    default_title: &str,
    categorical: bool,
    breaks: bool,
) {
    let vertical = id == "y";
    egui::Grid::new(("axis_settings", id)).num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
        // Title.
        ui.label("Title");
        ui.horizontal(|ui| {
            ui.checkbox(&mut s.show_title, "");
            let mut text = s.title.clone().unwrap_or_default();
            let edit = egui::TextEdit::singleline(&mut text).hint_text(default_title).desired_width(200.0);
            if ui.add_enabled(s.show_title, edit).on_hover_text(MARKUP_HINT).changed() {
                s.title = if text.trim().is_empty() { None } else { Some(text) };
            }
        });
        ui.end_row();

        // Range.
        ui.label("Range");
        if categorical {
            ui.label(RichText::new("follows the categories").weak());
        } else {
            range_row(ui, vertical, s, bounds, view);
        }
        ui.end_row();

        if !categorical {
            // Major ticks.
            ui.label("Major ticks");
            ui.horizontal(|ui| {
                let mut fixed = matches!(s.major, MajorTicks::Every(_));
                let mut step = match s.major {
                    MajorTicks::Every(v) => v,
                    MajorTicks::Auto => view.map_or(1.0, |v| default_step(v, vertical)),
                };
                ui.radio_value(&mut fixed, false, "Automatic");
                ui.radio_value(&mut fixed, true, "Every");
                let speed = step.abs().max(1e-6) * 0.01;
                let drag = egui::DragValue::new(&mut step).speed(speed).range(1e-12..=f64::MAX);
                ui.add_enabled(fixed, drag);
                s.major = if fixed { MajorTicks::Every(step) } else { MajorTicks::Auto };
            });
            ui.end_row();

            // Minor ticks.
            ui.label("Minor ticks");
            ui.horizontal(|ui| {
                let mut kind = match s.minor {
                    MinorTicks::None => 0,
                    MinorTicks::Auto => 1,
                    MinorTicks::Subdivisions(_) => 2,
                    MinorTicks::Every(_) => 3,
                };
                let names = ["None", "Automatic", "Subdivisions per major", "Every"];
                egui::ComboBox::from_id_salt(("minor_kind", id)).selected_text(names[kind]).width(170.0).show_list(
                    ui,
                    |ui| {
                        for (i, n) in names.iter().enumerate() {
                            ui.selectable_value(&mut kind, i, *n);
                        }
                    },
                );
                s.minor = match (kind, s.minor) {
                    (0, _) => MinorTicks::None,
                    (1, _) => MinorTicks::Auto,
                    (2, MinorTicks::Subdivisions(n)) => {
                        let mut n = n;
                        ui.add(egui::DragValue::new(&mut n).range(2..=100));
                        MinorTicks::Subdivisions(n)
                    }
                    (2, _) => MinorTicks::Subdivisions(5),
                    (_, MinorTicks::Every(v)) => {
                        let mut v = v;
                        let speed = v.abs().max(1e-6) * 0.01;
                        ui.add(egui::DragValue::new(&mut v).speed(speed).range(1e-12..=f64::MAX));
                        MinorTicks::Every(v)
                    }
                    _ => MinorTicks::Every(match s.major {
                        MajorTicks::Every(m) => m / 5.0,
                        MajorTicks::Auto => view.map_or(0.2, |v| default_step(v, vertical) / 5.0),
                    }),
                };
            });
            ui.end_row();

            ui.label("Tick origin");
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut s.origin).speed(0.01));
                ui.label(RichText::new("a value that falls on a major tick").weak().small());
            });
            ui.end_row();

            if breaks {
                ui.label("Axis breaks");
                breaks_editor(ui, id, s, view, vertical);
                ui.end_row();
            }
        }

        // Grid and marks.
        ui.label("Grid lines");
        ui.horizontal(|ui| {
            ui.radio_value(&mut s.grid, GridLines::MajorAndMinor, "Major + minor");
            ui.radio_value(&mut s.grid, GridLines::MajorOnly, "Major");
            ui.radio_value(&mut s.grid, GridLines::None, "None");
        });
        ui.end_row();

        ui.label("Show");
        ui.horizontal(|ui| {
            ui.add_enabled(!categorical, egui::Checkbox::new(&mut s.tick_marks, "Tick marks"));
            ui.checkbox(&mut s.tick_labels, "Tick labels");
        });
        ui.end_row();

        if !categorical {
            ui.label("Number format");
            ui.horizontal(|ui| {
                let mut kind = match s.format {
                    TickFormat::Auto => 0,
                    TickFormat::Decimals(_) => 1,
                    TickFormat::Scientific(_) => 2,
                    TickFormat::Percent(_) => 3,
                };
                let mut digits = match s.format {
                    TickFormat::Auto => 2,
                    TickFormat::Decimals(d) | TickFormat::Scientific(d) | TickFormat::Percent(d) => d,
                };
                let names = ["Automatic", "Fixed decimals", "Scientific", "Percent"];
                egui::ComboBox::from_id_salt(("format_kind", id)).selected_text(names[kind]).width(140.0).show_list(
                    ui,
                    |ui| {
                        for (i, n) in names.iter().enumerate() {
                            ui.selectable_value(&mut kind, i, *n);
                        }
                    },
                );
                if kind != 0 {
                    ui.add(egui::DragValue::new(&mut digits).range(0..=10).suffix(" decimals"));
                }
                s.format = match kind {
                    0 => TickFormat::Auto,
                    1 => TickFormat::Decimals(digits),
                    2 => TickFormat::Scientific(digits),
                    _ => TickFormat::Percent(digits),
                };
            });
            ui.end_row();
        }

        ui.label("Position");
        ui.horizontal(|ui| {
            let (a, b) = if vertical { ("Left", "Right") } else { ("Bottom", "Top") };
            ui.radio_value(&mut s.side, AxisSide::Standard, a);
            ui.radio_value(&mut s.side, AxisSide::Opposite, b);
        });
        ui.end_row();
    });
    ticks_group(ui, id, s, view, vertical, categorical);
    spine_group(ui, id, s, categorical);
    if ui.small_button("Reset this axis").clicked() {
        *s = AxisSettings::default();
    }
}

/// The "Ticks" group of an axis: which way the marks point, their length
/// and thickness, and the user's own tick labels.
fn ticks_group(
    ui: &mut egui::Ui,
    id: &str,
    s: &mut AxisSettings,
    view: Option<ViewBounds>,
    vertical: bool,
    categorical: bool,
) {
    egui::CollapsingHeader::new("Ticks").id_salt(("ticks_group", id)).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label("Tick direction");
            for d in app_model::axis_settings::TickDirection::ALL {
                ui.radio_value(&mut s.tick_direction, d, d.label());
            }
        });
        for (name, size, minor) in [("Major marks", &mut s.major_mark, false), ("Minor marks", &mut s.minor_mark, true)] {
            if minor && (categorical || s.minor == MinorTicks::None) {
                continue;
            }
            ui.horizontal(|ui| {
                ui.label(name);
                ui.add(egui::DragValue::new(&mut size.length).range(0.0..=30.0).speed(0.1).prefix("length ").suffix(" pt"));
                ui.add(egui::DragValue::new(&mut size.width).range(0.25..=5.0).speed(0.05).prefix("thickness ").suffix(" pt"));
            });
        }
        if categorical {
            return;
        }
        ui.label("Tick labels").on_hover_text(
            "Write your own label at any value: it replaces the number of a tick there or adds a tick of its own. \
             Labels read math markup (\\pi for π). Double-click a tick label on the plot to edit it there.",
        );
        let tolerance = |v: f64| app_model::axis_settings::custom_tolerance(v.abs().max(1.0));
        let mut remove = None;
        egui::Grid::new(("custom_ticks", id)).num_columns(3).show(ui, |ui| {
            for (i, c) in s.custom_ticks.iter_mut().enumerate() {
                let speed = (c.value.abs() * 0.005).max(1e-6);
                ui.add(egui::DragValue::new(&mut c.value).speed(speed).prefix("at "));
                ui.add(egui::TextEdit::singleline(&mut c.label).desired_width(120.0).hint_text("no label"))
                    .on_hover_text(MARKUP_HINT);
                if ui.small_button("✕").on_hover_text("Remove this label").clicked() {
                    remove = Some(i);
                }
                ui.end_row();
            }
        });
        if let Some(i) = remove {
            s.custom_ticks.remove(i);
        }
        if ui.button("Add a tick at…").on_hover_text("Add a labelled tick (set its value and label above)").clicked() {
            let mid = view.map_or(0.0, |v| if vertical { (v.y_min + v.y_max) / 2.0 } else { (v.x_min + v.x_max) / 2.0 });
            // The middle of the view, rounded to a step of a fiftieth of it.
            let span = view.map_or(1.0, |v| if vertical { v.y_max - v.y_min } else { v.x_max - v.x_min });
            let step = app_model::axis_settings::nice_step(span.abs() / 50.0);
            let value = if mid.is_finite() { (mid / step).round() * step } else { 0.0 };
            s.set_custom_label(value, Some(format_tick(s.format, value, step)), tolerance(value));
        }
    });
}

/// The "Spine" group of an axis: the axis line's offset from the data area
/// and whether an automatic range ends on major ticks.
fn spine_group(ui: &mut egui::Ui, id: &str, s: &mut AxisSettings, categorical: bool) {
    egui::CollapsingHeader::new("Spine").id_salt(("spine_group", id)).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label("Offset from data").on_hover_text(
                "Move the axis line (with its marks and labels) away from the data area by this many points",
            );
            let max = app_model::axis_settings::MAX_AXIS_OFFSET;
            ui.add(egui::DragValue::new(&mut s.offset).range(0.0..=max).speed(0.2).prefix("offset ").suffix(" pt"));
        });
        ui.add_enabled(!categorical, egui::Checkbox::new(&mut s.extend_to_ticks, "Extend to nearest ticks"))
            .on_hover_text("Widen the automatic range so the axis starts and ends on a major tick");
    });
}

/// The stretches left out of an axis: a check box with the first one's two
/// ends (data units; turning it on proposes the middle third of the visible
/// range), a row for each further one, *Add a break* (the middle third of
/// the widest piece left), and a group with each piece's scale and share of
/// the axis and the style of the break marks.
fn breaks_editor(ui: &mut egui::Ui, id: &str, s: &mut AxisSettings, view: Option<ViewBounds>, vertical: bool) {
    let shown = view.map_or((0.0, 1.0), |v| if vertical { (v.y_min, v.y_max) } else { (v.x_min, v.x_max) });
    let b = &mut s.breaks;
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            let mut on = !b.breaks.is_empty();
            let toggled = ui
                .checkbox(&mut on, "Leave out")
                .on_hover_text("Remove a stretch of the axis (drawn with break marks), to show values far apart")
                .changed();
            if toggled {
                if on {
                    let third = (shown.1 - shown.0) / 3.0;
                    b.breaks = vec![stat_viz::AxisBreak { from: shown.0 + third, to: shown.1 - third }];
                } else {
                    *b = stat_viz::AxisBreaks::default();
                }
            }
            match b.breaks.first_mut() {
                Some(first) => break_ends(ui, first),
                None => {
                    ui.label(RichText::new("no break").weak().small());
                }
            }
        });
        let mut remove = None;
        for (i, brk) in b.breaks.iter_mut().enumerate().skip(1) {
            ui.horizontal(|ui| {
                ui.label(format!("Break {}", i + 1));
                break_ends(ui, brk);
                let name = format!("Remove break {}", i + 1);
                let r = ui.small_button("✕").on_hover_text(&name);
                r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name));
                if r.clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            b.breaks.remove(i);
            if i + 1 < b.segments.len() {
                b.segments.remove(i + 1);
            }
        }
        if !b.breaks.is_empty() {
            let add = ui
                .button("Add a break")
                .on_hover_text("Leave out another stretch: the middle third of the widest piece of the axis left");
            if add.clicked() {
                if let Some(new) = next_break(b, shown) {
                    b.breaks.push(new);
                }
            }
            // Keep the list in axis order (segments follow the breaks).
            b.breaks.sort_by(|p, q| p.from.total_cmp(&q.from));
            segments_group(ui, id, b);
        }
    });
}

/// The two ends of one break (data units), kept in order.
fn break_ends(ui: &mut egui::Ui, brk: &mut stat_viz::AxisBreak) {
    let speed = ((brk.to - brk.from).abs() * 0.01).max(1e-9);
    ui.add(egui::DragValue::new(&mut brk.from).speed(speed).prefix("from "));
    ui.add(egui::DragValue::new(&mut brk.to).speed(speed).prefix("to "));
    if brk.to < brk.from {
        std::mem::swap(&mut brk.from, &mut brk.to);
    }
}

/// A further break: the middle third of the widest piece of `shown` the
/// breaks leave, or `None` when nothing of it is left.
fn next_break(b: &stat_viz::AxisBreaks, shown: (f64, f64)) -> Option<stat_viz::AxisBreak> {
    let t = b.tidied();
    let mut edges = vec![shown.0];
    for brk in &t.breaks {
        edges.push(brk.from);
        edges.push(brk.to);
    }
    edges.push(shown.1);
    edges
        .chunks(2)
        .map(|w| (w[0].max(shown.0), w[1].min(shown.1)))
        .filter(|(lo, hi)| hi > lo)
        .max_by(|p, q| (p.1 - p.0).total_cmp(&(q.1 - q.0)))
        .map(|(lo, hi)| {
            let third = (hi - lo) / 3.0;
            stat_viz::AxisBreak { from: lo + third, to: hi - third }
        })
}

/// "Segments and marks": each piece of a broken axis with its scale (the
/// axis's own by default) and its share of the axis (automatic by default),
/// and the style of the break marks.
fn segments_group(ui: &mut egui::Ui, id: &str, b: &mut stat_viz::AxisBreaks) {
    egui::CollapsingHeader::new("Segments and marks").id_salt(("break_segments", id)).show(ui, |ui| {
        let n = b.breaks.len() + 1;
        b.segments.resize(n.max(b.segments.len()), stat_viz::AxisSegment::default());
        let num = |v: f64| format_tick(TickFormat::Auto, v, (v.abs() / 100.0).max(1e-9));
        egui::Grid::new(("break_segments_grid", id)).num_columns(4).spacing([8.0, 4.0]).show(ui, |ui| {
            for k in 0..n {
                let place = match k {
                    0 => format!("Below {}", num(b.breaks[0].from)),
                    k if k == n - 1 => format!("Above {}", num(b.breaks[k - 1].to)),
                    k => format!("{} to {}", num(b.breaks[k - 1].to), num(b.breaks[k].from)),
                };
                ui.label(place);
                let seg = &mut b.segments[k];
                let name = format!("Scale of segment {}", k + 1);
                let shown = seg.scale.map_or("Same as axis", stat_viz::AxisScale::label);
                let combo = egui::ComboBox::from_id_salt(("segment_scale", id, k)).selected_text(shown).width(130.0);
                let r = combo.show_list(ui, |ui| {
                    ui.selectable_value(&mut seg.scale, None, "Same as axis");
                    for sc in stat_viz::AxisScale::ALL {
                        ui.selectable_value(&mut seg.scale, Some(sc), sc.label());
                    }
                });
                r.response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, &name));
                let mut auto = seg.share.is_none();
                if ui
                    .checkbox(&mut auto, "Automatic share")
                    .on_hover_text("Off: give this segment a fixed share of the axis length")
                    .changed()
                {
                    seg.share = if auto { None } else { Some((100.0 / n as f64).round()) };
                }
                match seg.share.as_mut() {
                    Some(share) => {
                        ui.add(egui::DragValue::new(share).range(1.0..=99.0).speed(0.5).prefix("share ").suffix(" %"));
                    }
                    None => {
                        ui.label("");
                    }
                }
                ui.end_row();
            }
        });
        ui.horizontal(|ui| {
            ui.label("Break marks");
            for mark in stat_viz::BreakMark::ALL {
                ui.radio_value(&mut b.mark, mark, mark.label());
            }
        });
        while b.segments.last().is_some_and(stat_viz::AxisSegment::is_default) {
            b.segments.pop();
        }
    });
}

/// A reasonable starting interval when switching to fixed ticks.
fn default_step(v: ViewBounds, vertical: bool) -> f64 {
    let span = if vertical { v.y_max - v.y_min } else { v.x_max - v.x_min };
    app_model::axis_settings::nice_step(span.abs() / 6.0)
}

/// Minimum and maximum of one axis; editing them fixes the view and keeps
/// the range with the plot ([`AxisSettings::range`], saved with it).
fn range_row(
    ui: &mut egui::Ui,
    vertical: bool,
    s: &mut AxisSettings,
    bounds: &mut Option<ViewBounds>,
    view: Option<ViewBounds>,
) {
    ui.horizontal(|ui| {
        let Some(current) = bounds.or(view) else {
            ui.label(RichText::new("shown after the plot is drawn").weak());
            return;
        };
        let (mut lo, mut hi) = if vertical { (current.y_min, current.y_max) } else { (current.x_min, current.x_max) };
        let speed = ((hi - lo).abs() * 0.005).max(1e-9);
        let a = ui.add(egui::DragValue::new(&mut lo).speed(speed).prefix("min "));
        let b = ui.add(egui::DragValue::new(&mut hi).speed(speed).prefix("max "));
        if (a.changed() || b.changed()) && hi > lo {
            let mut v = current;
            if vertical {
                v.y_min = lo;
                v.y_max = hi;
            } else {
                v.x_min = lo;
                v.x_max = hi;
            }
            *bounds = Some(v);
            s.range = Some([lo, hi]);
        }
        let auto = bounds.is_none() && s.range.is_none();
        ui.label(RichText::new(if auto { "automatic" } else { "fixed" }).weak().small());
        if s.range.is_some()
            && ui.small_button("Automatic").on_hover_text("Let this axis fit the data again").clicked()
        {
            s.range = None;
            *bounds = None;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_marks_split_into_majors_and_minors() {
        let marks = [
            GridMark { value: 0.0, step_size: 10.0 },
            GridMark { value: 2.0, step_size: 2.0 },
            GridMark { value: 10.0, step_size: 10.0 },
        ];
        assert_eq!(classify(&marks), vec![(0.0, true), (2.0, false), (10.0, true)]);
        assert!(classify(&[]).is_empty());
    }

    #[test]
    fn default_spacer_gives_majors_only() {
        let (spacer, _) = spacer_and_formatter(&AxisSettings::default(), true);
        // 0..100 over 400 points.
        let marks = spacer(GridInput { bounds: (0.0, 100.0), base_step_size: 0.25 * GRID_MIN_POINTS });
        assert!(!marks.is_empty());
        assert!(classify(&marks).iter().all(|m| m.1), "no minor rules by default");
        assert!(marks.iter().any(|m| m.value == 0.0));
    }
}

#[cfg(test)]
mod label_tests {
    use super::LabelLayer;
    use eframe::egui::{Align2, Rect, pos2, vec2};

    #[test]
    fn labels_move_apart_and_stay_inside() {
        let frame = Rect::from_min_max(pos2(0.0, 0.0), pos2(200.0, 100.0));
        let mut layer = LabelLayer::new(frame);
        let size = vec2(40.0, 10.0);
        let a = layer.place(pos2(100.0, 50.0), Align2::LEFT_CENTER, size);
        let b = layer.place(pos2(100.0, 52.0), Align2::LEFT_CENTER, size);
        assert!(!a.intersects(b.shrink(0.5)), "{a:?} {b:?}");
        assert!((b.center().y - 52.0).abs() <= 12.0, "moved the least distance: {b:?}");
        // A label at the bottom edge is pulled inside the frame.
        let c = layer.place(pos2(10.0, 100.0), Align2::LEFT_CENTER, size);
        assert!(frame.contains_rect(c), "{c:?}");
        // One past the right edge moves left.
        let d = layer.place(pos2(190.0, 20.0), Align2::LEFT_CENTER, size);
        assert!(frame.contains_rect(d), "{d:?}");
    }
}
