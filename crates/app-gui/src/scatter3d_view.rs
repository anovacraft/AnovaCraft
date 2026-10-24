//! The 3-D scatter plot: rows placed by three columns in a box that turns
//! and tilts as the plot is dragged, with colour, size and marker-shape
//! channels, optional drop lines to the floor, and linked brushing.
//!
//! Points are painted back to front (the painter's algorithm), each with a
//! thin outline in the contrasting ink so overlapping points stay apart.
//! Grouped colours always come with a distinct marker shape (or the shape
//! column's shapes, with every colour named in the key), and a continuous
//! colour column has a labelled colour bar, so colour is never the only cue.
//! Excluded rows are drawn hollow; hidden rows are left out.
//!
//! The rows, ranges and channels come from [`app_model::Scatter3dAnalysis`];
//! the projection is [`app_model::Camera`]; the box, axes and figure files
//! are shared with the 3-D surface views ([`crate::surface_profiler`]).

use app_model::axis_settings::nice_step;
use app_model::brushing::{brush_click, nearest_row, selection_mode};
use app_model::format::format_number;
use app_model::{Camera, ColorChannel, ColorValue, Scatter3dAnalysis, Scatter3dOptions, Scatter3dPoint};
use eframe::egui::{self, Align2, Color32, FontId, Pos2, RichText, Sense, Stroke, Vec2};
use egui_plot::MarkerShape;
use stat_data::RowStateManager;

use crate::plot_size::{GripFrame, PlotSize, shared_grip};
use crate::surface_profiler::{
    EDGES, Look, Prim, axes, box_projection, circle, contrast, paint, ramp, ring, scene_figure, shade,
};
use crate::theme::{self, PlotPalette};

/// Interaction state of one 3-D scatter plot (its angle and drop lines are
/// display options saved with the project).
#[derive(Debug, Clone, Default)]
pub struct Scatter3dView {
    /// Screen position where a modifier-drag (box selection) started.
    box_start: Option<Pos2>,
    /// Size chosen with the resize grip.
    pub(crate) size: PlotSize,
}

const PLOT_HEIGHT: f32 = 460.0;
/// Narrowest the plot is drawn: the box and its axis labels need the room.
const MIN_WIDTH: f32 = 360.0;
/// Marker radius range for the size channel (and the fixed radius without one).
const SMALL: f32 = 3.5;
const LARGE: f32 = 10.0;

/// Draw the plot and its controls, updating `options` (angle, drop lines)
/// and the shared row state.
pub fn show(
    ui: &mut egui::Ui,
    analysis: &Scatter3dAnalysis,
    options: &mut Scatter3dOptions,
    view: &mut Scatter3dView,
    rows: &mut RowStateManager,
    id: u64,
) {
    let pal = theme::plot_palette(ui);
    ui.horizontal_wrapped(|ui| {
        ui.checkbox(&mut options.drop_lines, "Drop lines")
            .on_hover_text("A line from each point down to the floor of the box, to read its position across");
        if ui.small_button("Reset view").clicked() {
            options.camera = Camera::default();
        }
        if ui.small_button("View from above").on_hover_text("Looks straight down the up axis").clicked() {
            options.camera = Camera::TOP;
        }
        if ui.small_button("Side view").on_hover_text("Looks along the depth axis").clicked() {
            options.camera = Camera { azimuth: 0.0, elevation: 0.0 };
        }
        ui.label(
            RichText::new(format!(
                "Turned {:.0}°, tilted {:.0}° · drag the plot to rotate · double-click to reset",
                options.camera.azimuth, options.camera.elevation
            ))
            .weak()
            .small(),
        );
    });
    let mut size = view.size.capped(ui, PLOT_HEIGHT);
    size.x = size.x.max(MIN_WIDTH);
    let (response, painter) = ui.allocate_painter(size, Sense::click_and_drag());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, "Scatter (drag to turn)"));
    let frame = GripFrame { size, area: response.rect, panels: [1, 1], min_width: MIN_WIDTH, what: "plot" };
    shared_grip(ui, &mut view.size, ui.make_persistent_id(("scatter3d_size", id)), &frame);
    let modifiers = ui.input(|i| i.modifiers);
    let mode = selection_mode(modifiers.shift, modifiers.command, modifiers.alt);
    let selecting = modifiers.shift || modifiers.command || modifiers.alt;
    if response.drag_started() {
        view.box_start = if selecting { response.interact_pointer_pos() } else { None };
    }
    if response.dragged() && view.box_start.is_none() {
        let d = response.drag_delta();
        options.camera.rotate(f64::from(d.x) * 0.5, f64::from(d.y) * 0.4);
    }
    if response.double_clicked() {
        options.camera = Camera::default();
    }
    if response.hovered() {
        let icon =
            if selecting || view.box_start.is_some() { egui::CursorIcon::Crosshair } else { egui::CursorIcon::Grab };
        ui.ctx().set_cursor_icon(icon);
    }
    let shown: Vec<&Scatter3dPoint> = analysis.points.iter().filter(|p| !rows.is_hidden(p.row)).collect();
    let state: Vec<(bool, bool)> = shown.iter().map(|p| (rows.is_selected(p.row), rows.is_excluded(p.row))).collect();
    let look = Look::of(ui);
    let measure = |text: &str| {
        ui.fonts(|f| f.layout_no_wrap(text.to_owned(), FontId::proportional(look.font), Color32::WHITE).size().x)
    };
    let rect = response.rect;
    let (prims, positions) = scene(rect, analysis, &shown, &state, *options, look, &pal, &measure);
    paint(&painter, &prims);
    response.context_menu(|ui| {
        if ui.checkbox(&mut options.drop_lines, "Drop lines").clicked() {
            ui.close_menu();
        }
        if ui.button("Reset view").clicked() {
            options.camera = Camera::default();
            ui.close_menu();
        }
        ui.separator();
        crate::report_view::save_figure_items(
            ui,
            id,
            "3-D Scatter Plot",
            "The plot at its on-screen size and angle.",
            &|format| {
                let look = Look::light();
                let pal = PlotPalette::light();
                let (prims, _) = scene(rect, analysis, &shown, &state, *options, look, &pal, &|t| look.text_width(t));
                Some(scene_figure(&prims, rect, "3-D Scatter Plot", format))
            },
        );
    });
    if let (Some(start), Some(now)) = (view.box_start, response.interact_pointer_pos()) {
        let region = egui::Rect::from_two_pos(start, now);
        painter.rect(region, 0.0, pal.brush_fill, Stroke::new(1.0_f32, pal.selected), egui::StrokeKind::Inside);
        if response.drag_stopped() {
            let inside = positions.iter().zip(&shown).filter(|(p, _)| region.contains(**p)).map(|(_, q)| q.row);
            rows.select(inside.collect::<Vec<_>>(), mode);
            view.box_start = None;
        }
    }
    if response.drag_stopped() {
        view.box_start = None;
    }
    if response.clicked() {
        let hit = response.interact_pointer_pos().and_then(|pointer| {
            nearest_row(shown.iter().zip(&positions).map(|(q, p)| (q.row, [p.x, p.y])), [pointer.x, pointer.y], 8.0)
        });
        brush_click(hit, mode, rows);
    }
    let hovered = response.hover_pos().and_then(|pointer| {
        positions
            .iter()
            .enumerate()
            .map(|(k, p)| (k, p.distance(pointer)))
            .filter(|&(_, d)| d <= 8.0)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(k, _)| k)
    });
    if let Some(k) = hovered {
        let p = shown[k];
        painter.circle_stroke(positions[k], LARGE + 1.0, Stroke::new(2.0_f32, pal.selected));
        let fmt = |v: f64| format_number(v, 6);
        response.on_hover_ui_at_pointer(|ui| {
            ui.label(RichText::new(format!("Row {}", p.row + 1)).strong());
            for (name, v) in analysis.names.iter().zip(p.at) {
                ui.label(format!("{name} = {}", fmt(v)));
            }
            match (&analysis.color, p.color) {
                (ColorChannel::Levels { name, levels }, ColorValue::Level(l)) => {
                    ui.label(format!("{name}: {}", levels[l]));
                }
                (ColorChannel::Ramp { name, .. }, ColorValue::Value(v)) => {
                    ui.label(format!("{name} = {}", fmt(v)));
                }
                _ => {}
            }
            if let (Some((name, _)), Some(v)) = (&analysis.size, p.size) {
                ui.label(format!("{name} = {}", fmt(v)));
            }
            if let (Some((name, levels)), Some(l)) = (&analysis.shape, p.shape) {
                ui.label(format!("{name}: {}", levels[l]));
            }
        });
    }
    ui.label(
        RichText::new(
            "Click a point to select its row (Shift adds, Command toggles, Option removes); hold one of those keys \
             and drag a box to select several. Selected rows are ringed in every view.",
        )
        .weak()
        .small(),
    );
}

/// Fill colour and marker shape of `p`.
fn style(analysis: &Scatter3dAnalysis, p: &Scatter3dPoint, pal: &PlotPalette) -> (Color32, MarkerShape) {
    let (color, color_shape) = match (&analysis.color, p.color) {
        (ColorChannel::Levels { .. }, ColorValue::Level(l)) => {
            let (c, s) = pal.group_style(l);
            (c, Some(s))
        }
        (ColorChannel::Ramp { range: (lo, hi), .. }, ColorValue::Value(v)) => {
            let t = if hi > lo { (v - lo) / (hi - lo) } else { 0.5 };
            (shade(ramp(t), 1.0), None)
        }
        _ => (pal.point, None),
    };
    let shape = match p.shape {
        Some(l) => pal.group_style(l).1,
        None => color_shape.unwrap_or(MarkerShape::Circle),
    };
    (color, shape)
}

/// Outline of a marker of `shape` and radius `r` at `at`, or `None` for the
/// line-drawn shapes.
fn marker_polygon(shape: MarkerShape, at: Pos2, r: f32) -> Option<Vec<Pos2>> {
    let p = |x: f32, y: f32| at + Vec2::new(x, y) * r;
    Some(match shape {
        MarkerShape::Circle => circle(at, r),
        MarkerShape::Square => vec![p(-0.85, -0.85), p(0.85, -0.85), p(0.85, 0.85), p(-0.85, 0.85)],
        MarkerShape::Diamond => vec![p(0.0, -1.15), p(1.15, 0.0), p(0.0, 1.15), p(-1.15, 0.0)],
        MarkerShape::Up => vec![p(0.0, -1.2), p(1.05, 0.65), p(-1.05, 0.65)],
        MarkerShape::Down => vec![p(0.0, 1.2), p(-1.05, -0.65), p(1.05, -0.65)],
        MarkerShape::Left => vec![p(-1.2, 0.0), p(0.65, -1.05), p(0.65, 1.05)],
        MarkerShape::Right => vec![p(1.2, 0.0), p(-0.65, 1.05), p(-0.65, -1.05)],
        MarkerShape::Asterisk | MarkerShape::Cross | MarkerShape::Plus => return None,
    })
}

/// A marker: filled with an outline in `ink`, or hollow (excluded rows).
fn marker(out: &mut Vec<Prim>, shape: MarkerShape, at: Pos2, r: f32, color: Color32, ink: Color32, hollow: bool) {
    match marker_polygon(shape, at, r) {
        Some(pts) => {
            if hollow {
                let mut closed = pts;
                closed.push(closed[0]);
                out.push(Prim::Line { pts: closed, width: 1.6, color, dash: None });
            } else {
                let grow: Vec<Pos2> = pts.iter().map(|q| at + (*q - at) * ((r + 1.0) / r)).collect();
                out.push(Prim::Fill { pts: grow, color: ink, seamless: false });
                out.push(Prim::Fill { pts, color, seamless: false });
            }
        }
        None => {
            let angles: &[f32] = match shape {
                MarkerShape::Plus => &[0.0, 90.0],
                MarkerShape::Cross => &[45.0, 135.0],
                _ => &[90.0, 30.0, 150.0],
            };
            for (width, c) in [(3.4, ink), (1.8, color)] {
                if hollow && c == ink {
                    continue;
                }
                for a in angles {
                    let d = Vec2::angled(a.to_radians()) * r * 1.15;
                    out.push(Prim::Line { pts: vec![at - d, at + d], width, color: c, dash: None });
                }
            }
        }
    }
}

/// The plot drawn into `rect`: primitives and the screen position of each
/// shown point. `state` holds each shown point's (selected, excluded).
#[allow(clippy::too_many_arguments)]
fn scene(
    rect: egui::Rect,
    a: &Scatter3dAnalysis,
    shown: &[&Scatter3dPoint],
    state: &[(bool, bool)],
    options: Scatter3dOptions,
    look: Look,
    pal: &PlotPalette,
    measure: &dyn Fn(&str) -> f32,
) -> (Vec<Prim>, Vec<Pos2>) {
    let mut out = Vec::new();
    let k = look.font / 11.0;
    let corners = |r: egui::Rect| vec![r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()];
    out.push(Prim::Fill { pts: corners(rect), color: look.background, seamless: false });
    let legend_width = legend_width(a, look, measure);
    let margin = 46.0 * k;
    let area = egui::Rect::from_min_max(
        rect.min + Vec2::splat(margin),
        Pos2::new(rect.max.x - legend_width - margin, rect.max.y - margin),
    );
    let cam = options.camera;
    let to_screen = box_projection(area, cam);
    let unit = |v: f64, (lo, hi): (f64, f64)| if hi > lo { 2.0 * (v - lo) / (hi - lo) - 1.0 } else { 0.0 };
    let place = |p: [f64; 3]| [unit(p[0], a.ranges[0]), unit(p[1], a.ranges[1]), unit(p[2], a.ranges[2])];
    let frame = look.weak.gamma_multiply(0.7);
    let line = |pts: Vec<Pos2>, width: f32, color: Color32| Prim::Line { pts, width, color, dash: None };

    // Back edges and the floor (with faint rules at the tick positions) when
    // seen from above; from below the floor is outlined last.
    let front =
        |p: [f64; 3], q: [f64; 3]| to_screen([(p[0] + q[0]) / 2.0, (p[1] + q[1]) / 2.0, (p[2] + q[2]) / 2.0]).1 > 0.0;
    let floor_edge = |p: [f64; 3], q: [f64; 3]| p[2] == -1.0 && q[2] == -1.0;
    let floor_behind = cam.sees_floor_from_above();
    let early = |p: [f64; 3], q: [f64; 3]| if floor_edge(p, q) { floor_behind } else { !front(p, q) };
    for (p, q) in EDGES {
        if early(p, q) {
            out.push(line(vec![to_screen(p).0, to_screen(q).0], 1.0, frame));
        }
    }
    if floor_behind {
        let floor = [[-1.0, -1.0, -1.0], [1.0, -1.0, -1.0], [1.0, 1.0, -1.0], [-1.0, 1.0, -1.0]];
        out.push(Prim::Fill { pts: floor.map(|p| to_screen(p).0).to_vec(), color: look.floor, seamless: false });
        let rule = look.weak.gamma_multiply(0.25);
        for (axis, range) in [(0, a.ranges[0]), (1, a.ranges[1])] {
            for v in ticks(range) {
                let u = unit(v, range);
                let (p, q) =
                    if axis == 0 { ([u, -1.0, -1.0], [u, 1.0, -1.0]) } else { ([-1.0, u, -1.0], [1.0, u, -1.0]) };
                out.push(line(vec![to_screen(p).0, to_screen(q).0], 0.8, rule));
            }
        }
    }

    // Points, farthest first, each after its drop line.
    let ink = contrast(look.dark).gamma_multiply(0.8);
    let mut order: Vec<(f64, usize)> = shown.iter().enumerate().map(|(i, p)| (to_screen(place(p.at)).1, i)).collect();
    order.sort_by(|x, y| x.0.total_cmp(&y.0));
    let mut positions = vec![Pos2::ZERO; shown.len()];
    for &(_, i) in &order {
        let p = shown[i];
        let (color, shape) = style(a, p, pal);
        let (selected, excluded) = state.get(i).copied().unwrap_or_default();
        let q = place(p.at);
        let at = to_screen(q).0;
        positions[i] = at;
        if options.drop_lines {
            let foot = to_screen([q[0], q[1], -1.0]).0;
            out.push(line(vec![foot, at], 1.0, color.gamma_multiply(0.55)));
            out.push(Prim::Fill { pts: circle(foot, 1.8), color: color.gamma_multiply(0.7), seamless: false });
        }
        let r = a.radius(p.size, SMALL, LARGE) * k.max(1.0);
        marker(&mut out, shape, at, r, if excluded { pal.excluded } else { color }, ink, excluded);
        if selected {
            out.push(ring(at, r + 4.0, 2.0, look.selected));
        }
    }
    if !floor_behind {
        for (p, q) in EDGES.iter().filter(|(p, q)| floor_edge(*p, *q)) {
            out.push(line(vec![to_screen(*p).0, to_screen(*q).0], 1.0, frame));
        }
    }
    for (p, q) in EDGES {
        if !early(p, q) && !(floor_edge(p, q) && !floor_behind) {
            out.push(line(vec![to_screen(p).0, to_screen(q).0], 1.0, frame));
        }
    }
    axes(&mut out, &to_screen, a.ranges, [&a.names[0], &a.names[1], &a.names[2]], look);
    legend(&mut out, rect, legend_width, a, look, pal, measure);
    (out, positions)
}

/// Round tick values inside `(lo, hi)`.
fn ticks((lo, hi): (f64, f64)) -> Vec<f64> {
    if hi <= lo {
        return Vec::new();
    }
    let step = nice_step((hi - lo) / 4.0);
    let mut out = Vec::new();
    let mut k = (lo / step).ceil();
    while k * step <= hi && out.len() < 12 {
        out.push(k * step);
        k += 1.0;
    }
    out
}

/// Longest text a key entry shows before it is shortened.
const KEY_TEXT: usize = 18;

fn shorten(text: &str) -> String {
    if text.chars().count() <= KEY_TEXT {
        text.to_owned()
    } else {
        format!("{}…", text.chars().take(KEY_TEXT - 1).collect::<String>())
    }
}

/// Width the key needs (none without channels).
fn legend_width(a: &Scatter3dAnalysis, look: Look, measure: &dyn Fn(&str) -> f32) -> f32 {
    let mut texts: Vec<String> = Vec::new();
    match &a.color {
        ColorChannel::None => {}
        ColorChannel::Levels { name, levels } => {
            texts.push(name.clone());
            texts.extend(levels.iter().map(|l| format!("    {l}")));
        }
        ColorChannel::Ramp { name, range: (lo, hi) } => {
            texts.push(name.clone());
            texts.extend([lo, hi].map(|v| format!("      {}", format_number(*v, 4))));
        }
    }
    if let Some((name, (lo, hi))) = &a.size {
        texts.push(name.clone());
        texts.extend([lo, hi].map(|v| format!("      {}", format_number(*v, 4))));
    }
    if let Some((name, levels)) = &a.shape {
        texts.push(name.clone());
        texts.extend(levels.iter().map(|l| format!("    {l}")));
    }
    if texts.is_empty() {
        return 0.0;
    }
    texts.iter().map(|t| measure(&shorten(t))).fold(0.0, f32::max) + 24.0 * look.font / 11.0
}

/// The key: colour levels (with their shapes) or a colour bar, marker sizes
/// and shapes, each under its column's name.
fn legend(
    out: &mut Vec<Prim>,
    rect: egui::Rect,
    width: f32,
    a: &Scatter3dAnalysis,
    look: Look,
    pal: &PlotPalette,
    measure: &dyn Fn(&str) -> f32,
) {
    if width <= 0.0 {
        return;
    }
    let row_h = look.font * 1.7;
    let x0 = rect.max.x - width;
    let mut y = rect.min.y + 18.0;
    let text = |out: &mut Vec<Prim>, at: Pos2, t: &str, strong: bool| {
        out.push(Prim::Text {
            at,
            align: Align2::LEFT_CENTER,
            text: shorten(t),
            size: look.font,
            color: if strong { look.text } else { look.weak },
        });
        let _ = measure;
    };
    let ink = contrast(look.dark).gamma_multiply(0.8);
    let shape_led = a.shape.is_some();
    match &a.color {
        ColorChannel::None => {}
        ColorChannel::Levels { name, levels } => {
            text(out, Pos2::new(x0, y), name, true);
            y += row_h;
            let max_rows = ((rect.max.y - y - 20.0) / row_h).max(1.0) as usize;
            for (l, level) in levels.iter().enumerate().take(max_rows) {
                let (c, s) = pal.group_style(l);
                // With a shape column the colours are named here, each as a circle.
                let s = if shape_led { MarkerShape::Circle } else { s };
                marker(out, s, Pos2::new(x0 + 6.0, y), 5.0, c, ink, false);
                text(out, Pos2::new(x0 + 16.0, y), level, false);
                y += row_h;
            }
            if levels.len() > max_rows {
                text(out, Pos2::new(x0 + 16.0, y), &format!("… {} more", levels.len() - max_rows), false);
                y += row_h;
            }
        }
        ColorChannel::Ramp { name, range: (lo, hi) } => {
            text(out, Pos2::new(x0, y), name, true);
            y += row_h * 0.7;
            let bar = egui::Rect::from_min_size(Pos2::new(x0 + 2.0, y), Vec2::new(12.0, 110.0));
            let steps = 32;
            for s in 0..steps {
                let (t0, t1) = (s as f32 / steps as f32, (s + 1) as f32 / steps as f32);
                let r = egui::Rect::from_min_max(
                    Pos2::new(bar.min.x, bar.max.y - bar.height() * t1),
                    Pos2::new(bar.max.x, bar.max.y - bar.height() * t0),
                );
                out.push(Prim::Fill {
                    pts: vec![r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()],
                    color: shade(ramp(f64::from(0.5 * (t0 + t1))), 1.0),
                    seamless: true,
                });
            }
            for v in ticks((*lo, *hi)).into_iter().chain([*lo, *hi]) {
                let t = if hi > lo { ((v - lo) / (hi - lo)) as f32 } else { 0.5 };
                let ty = bar.max.y - bar.height() * t;
                out.push(Prim::Line {
                    pts: vec![Pos2::new(bar.max.x, ty), Pos2::new(bar.max.x + 3.0, ty)],
                    width: 1.0,
                    color: look.weak,
                    dash: None,
                });
            }
            for (v, t) in [(*hi, 1.0_f32), (*lo, 0.0)] {
                text(out, Pos2::new(bar.max.x + 6.0, bar.max.y - bar.height() * t), &format_number(v, 4), false);
            }
            y = bar.max.y + row_h;
        }
    }
    if let Some((name, (lo, hi))) = &a.size {
        y += row_h * 0.3;
        text(out, Pos2::new(x0, y), name, true);
        y += row_h;
        for v in [*lo, *hi] {
            let r = a.radius(Some(v), SMALL, LARGE);
            marker(out, MarkerShape::Circle, Pos2::new(x0 + LARGE, y), r, look.weak.gamma_multiply(0.6), ink, false);
            text(out, Pos2::new(x0 + 2.0 * LARGE + 6.0, y), &format_number(v, 4), false);
            y += row_h.max(2.0 * LARGE + 4.0);
        }
    }
    if let Some((name, levels)) = &a.shape {
        y += row_h * 0.3;
        text(out, Pos2::new(x0, y), name, true);
        y += row_h;
        for (l, level) in levels.iter().enumerate() {
            if y > rect.max.y - 10.0 {
                break;
            }
            let s = pal.group_style(l).1;
            marker(out, s, Pos2::new(x0 + 6.0, y), 5.0, look.weak, ink, false);
            text(out, Pos2::new(x0 + 16.0, y), level, false);
            y += row_h;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use app_model::{SavedReport, Scatter3dRequest};
    use stat_viz::export::VectorFormat;

    /// The plot saves as a figure of the picture on screen, with its axis
    /// names and key.
    #[test]
    fn scatter_figures_match_the_screen() {
        let sample =
            (app_model::sample::catalog().into_iter().find(|s| s.name.starts_with("Hillside")).unwrap().build)();
        let Some(SavedReport::Scatter3d { request, options }) =
            sample.reports.iter().find(|r| matches!(r, SavedReport::Scatter3d { .. })).cloned()
        else {
            panic!("the sample has a 3-D scatter plot")
        };
        let rows = RowStateManager::new(sample.table.n_rows());
        let a = Scatter3dAnalysis::run(&sample.table, &rows, &request).unwrap();
        let shown: Vec<&Scatter3dPoint> = a.points.iter().collect();
        let state = vec![(false, false); shown.len()];
        let rect = egui::Rect::from_min_size(Pos2::new(0.0, 0.0), Vec2::new(900.0, PLOT_HEIGHT));
        let look = Look::light();
        let pal = PlotPalette::light();
        let (prims, positions) = scene(rect, &a, &shown, &state, options, look, &pal, &|t| look.text_width(t));
        assert_eq!(positions.len(), shown.len());
        assert!(positions.iter().all(|p| rect.contains(*p)), "every point inside the plot");
        let svg = String::from_utf8(scene_figure(&prims, rect, "3-D Scatter Plot", VectorFormat::Svg)).unwrap();
        for text in ["Easting (m)", "Northing (m)", "Elevation (m)", "Soil type", "Soil depth (cm)", "Clay"] {
            assert!(svg.contains(text), "{text}");
        }
        for format in [VectorFormat::Pdf, VectorFormat::Eps, VectorFormat::Png] {
            assert!(!scene_figure(&prims, rect, "3-D Scatter Plot", format).is_empty());
        }
        // Without channels there is no key, and the plot still draws.
        let plain = Scatter3dAnalysis::run(&sample.table, &rows, &Scatter3dRequest::new(0, 1, 2)).unwrap();
        assert_eq!(legend_width(&plain, look, &|t| look.text_width(t)), 0.0);
    }

    /// Turning the view moves the points but keeps them inside the plot.
    #[test]
    fn every_angle_keeps_points_inside() {
        let sample =
            (app_model::sample::catalog().into_iter().find(|s| s.name.starts_with("Hillside")).unwrap().build)();
        let rows = RowStateManager::new(sample.table.n_rows());
        let a = Scatter3dAnalysis::run(&sample.table, &rows, &Scatter3dRequest::new(0, 1, 2)).unwrap();
        let shown: Vec<&Scatter3dPoint> = a.points.iter().collect();
        let state = vec![(false, false); shown.len()];
        let rect = egui::Rect::from_min_size(Pos2::ZERO, Vec2::new(700.0, PLOT_HEIGHT));
        let look = Look::light();
        for (azimuth, elevation) in [(0.0, 0.0), (45.0, 90.0), (-120.0, -40.0), (170.0, 135.0)] {
            let options = Scatter3dOptions { drop_lines: true, camera: Camera { azimuth, elevation } };
            let (_, positions) =
                scene(rect, &a, &shown, &state, options, look, &PlotPalette::light(), &|t| look.text_width(t));
            assert!(positions.iter().all(|p| rect.contains(*p)), "{azimuth}, {elevation}");
        }
    }
}
