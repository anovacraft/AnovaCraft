//! The 3-D surface profiler: a shaded, rotatable surface of a fitted model's
//! predicted response over two continuous factors, with contour lines drawn
//! on the surface, a contour map on the floor of the box, and the current
//! setting marked. The other factors are held at settings chosen above the
//! plot (shared with the Response Explorer for linear models).
//!
//! Height is shown three ways — position against the vertical axis, contour
//! lines with a labelled legend, and a sequential colour ramp — so colour is
//! never the only cue. Dragging the plot turns and tilts the view (there are
//! no data points to select in this view); double-click or "Reset view"
//! restores the default angle. The view never responds to scrolling.
//!
//! The grid, contours, projection and painting order come from
//! [`app_model::surface_profiler`]; this module only draws them.

use crate::dialog_kit::ComboExt as _;
use app_model::brushing::{brush_click, nearest_row, selection_mode};
use app_model::format::format_number;
use app_model::surface_profiler::{
    Camera, MAX_RESOLUTION, ProfileFactorKind, SurfaceMesh, SurfaceModel, add_observations, settings_fit, surface_mesh,
};
use app_model::{TermValue, axis_settings::nice_step};
use eframe::egui::{self, Align2, Color32, FontId, Pos2, RichText, Sense, Shape, Stroke, Vec2};
use stat_data::RowStateManager;
use stat_viz::export::{ChartPanel, Figure, Label, VectorFormat, export_chart};
use stat_viz::layer::{LinePattern, Mark, Rgba};

use crate::plot_size::{GripFrame, PlotSize, shared_grip};
use crate::theme;

/// View and display settings of one surface profiler (not saved with the project).
#[derive(Debug, Clone)]
pub struct SurfaceProfilerState {
    /// Factor across (index into the model's factors), once chosen.
    x: Option<usize>,
    /// Factor in depth.
    y: Option<usize>,
    pub(crate) camera: Camera,
    resolution: usize,
    pub(crate) levels: usize,
    pub(crate) surface_contours: bool,
    pub(crate) floor_map: bool,
    pub(crate) mesh_lines: bool,
    /// Shaded cells; a wireframe of the grid when `false`.
    pub(crate) filled: bool,
    /// Colour scale of the surface, its floor map and colour bar, and
    /// whether it runs from high to low.
    pub(crate) colors: (stat_viz::palette::ColorScale, bool),
    /// Screen position where a modifier-drag (box selection) started.
    pub(crate) box_start: Option<Pos2>,
    /// Size chosen with the resize grip.
    pub(crate) size: PlotSize,
}

/// What a surface shows, for its wording and point markers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SurfaceKind {
    /// A fitted model's prediction (the 3-D surface profiler).
    Model,
    /// A surface made from data; `exact` when it passes through the points.
    Data { exact: bool },
}

impl SurfaceKind {
    fn title(self) -> &'static str {
        match self {
            Self::Model => "3-D Response Surface",
            Self::Data { .. } => "3-D Surface",
        }
    }
}

impl Default for SurfaceProfilerState {
    fn default() -> Self {
        Self {
            x: None,
            y: None,
            camera: Camera::default(),
            resolution: 30,
            levels: 8,
            surface_contours: true,
            floor_map: true,
            mesh_lines: false,
            filled: true,
            colors: (stat_viz::palette::ColorScale::Sequential, false),
            box_start: None,
            size: PlotSize::default(),
        }
    }
}

const PLOT_HEIGHT: f32 = 430.0;
/// Narrowest the plot is drawn: the box and its axis labels need the room.
const MIN_WIDTH: f32 = 320.0;

/// Draw the profiler for `model`, reading and updating `settings` (one value
/// per factor; reset to the model's defaults when they no longer fit).
pub fn show(
    ui: &mut egui::Ui,
    model: &dyn SurfaceModel,
    settings: &mut Vec<TermValue>,
    show_observed: &mut bool,
    state: &mut SurfaceProfilerState,
    rows: &mut RowStateManager,
    id: u64,
) {
    if !model.independent_factors() {
        ui.label(RichText::new("The 3-D profiler needs factors that can be varied independently.").weak());
        return;
    }
    if !settings_fit(model, settings) {
        *settings = model.default_profile_settings();
    }
    let factors = model.profile_factors();
    let continuous: Vec<usize> = (0..factors.len()).filter(|&j| factors[j].is_continuous()).collect();
    if continuous.len() < 2 {
        ui.label(
            RichText::new("The 3-D profiler needs two continuous factors in the model; this model has fewer.").weak(),
        );
        return;
    }
    // Keep the axes valid and distinct.
    let x = state.x.filter(|j| continuous.contains(j)).unwrap_or(continuous[0]);
    let y = state
        .y
        .filter(|j| continuous.contains(j) && *j != x)
        .unwrap_or_else(|| *continuous.iter().find(|&&j| j != x).expect("two continuous factors"));
    let (mut x, mut y) = (x, y);

    // Axis choice.
    ui.horizontal_wrapped(|ui| {
        let name = |j: usize| factors[j].name.as_str();
        ui.label("Across:");
        egui::ComboBox::from_id_salt(("surface_x", id)).selected_text(name(x)).show_list(ui, |ui| {
            for &j in &continuous {
                if j != y {
                    ui.selectable_value(&mut x, j, name(j));
                }
            }
        });
        ui.label("Depth:");
        egui::ComboBox::from_id_salt(("surface_y", id)).selected_text(name(y)).show_list(ui, |ui| {
            for &j in &continuous {
                if j != x {
                    ui.selectable_value(&mut y, j, name(j));
                }
            }
        });
        if ui.small_button("Swap axes").on_hover_text("Exchange the two axes").clicked() {
            std::mem::swap(&mut x, &mut y);
        }
    });
    state.x = Some(x);
    state.y = Some(y);

    // Factor settings: the axis factors set the marked point, the others are held.
    ui.horizontal_wrapped(|ui| {
        for (j, f) in factors.iter().enumerate() {
            let role = if j == x || j == y { " (marked point)" } else { " (held)" };
            ui.horizontal(|ui| {
                ui.label(format!("{}{role}:", f.name));
                match (&f.kind, &mut settings[j]) {
                    (ProfileFactorKind::Continuous { range: (lo, hi) }, TermValue::Number(v)) => {
                        let speed = ((hi - lo) / 200.0).max(f64::EPSILON);
                        ui.add(egui::DragValue::new(v).range(*lo..=*hi).speed(speed).max_decimals(6));
                    }
                    (ProfileFactorKind::Categorical { levels }, TermValue::Level(l)) => {
                        egui::ComboBox::from_id_salt(("surface_level", id, j))
                            .selected_text(levels.get(*l).map_or("", String::as_str))
                            .show_list(ui, |ui| {
                                for (k, name) in levels.iter().enumerate() {
                                    ui.selectable_value(l, k, name.as_str());
                                }
                            });
                    }
                    _ => {}
                }
            });
            ui.add_space(8.0);
        }
        if ui.small_button("Reset settings").clicked() {
            *settings = model.default_profile_settings();
        }
    });

    // Display options.
    ui.horizontal_wrapped(|ui| {
        ui.label("Grid divisions:");
        ui.add(egui::Slider::new(&mut state.resolution, 8..=MAX_RESOLUTION.min(60)))
            .on_hover_text("Divisions along each axis: finer is smoother but slower");
        ui.label("Contour levels:");
        ui.add(egui::Slider::new(&mut state.levels, 0..=20)).on_hover_text("About this many, at round values");
        ui.checkbox(&mut state.surface_contours, "Contours on surface");
        ui.checkbox(&mut state.floor_map, "Contour map on floor");
        ui.checkbox(&mut state.mesh_lines, "Grid lines");
        ui.checkbox(show_observed, "Show observed data")
            .on_hover_text("Plot each row used in the fit at its observed response; click points to select their rows");
    });
    ui.horizontal_wrapped(|ui| {
        if ui.small_button("Reset view").clicked() {
            state.camera = Camera::default();
        }
        if ui.small_button("View from above").on_hover_text("Looks straight down: a contour map").clicked() {
            state.camera = Camera::TOP;
        }
        if ui.small_button("Side view").on_hover_text("Looks along the depth axis").clicked() {
            state.camera = Camera { azimuth: 0.0, elevation: 0.0 };
        }
        ui.label(
            RichText::new(format!(
                "Turned {:.0}°, tilted {:.0}° · drag the plot to rotate · double-click to reset",
                state.camera.azimuth, state.camera.elevation
            ))
            .weak()
            .small(),
        );
    });

    let mut mesh = match surface_mesh(model, settings, x, y, state.resolution, state.levels) {
        Ok(m) => m,
        Err(e) => {
            ui.colored_label(theme::ERROR, e.to_string());
            return;
        }
    };
    if *show_observed {
        if let Err(e) = add_observations(&mut mesh, model, settings, x, y) {
            ui.colored_label(theme::ERROR, e.to_string());
        }
        // Hidden rows are not drawn, so they cannot be selected here either.
        mesh.points.retain(|p| !rows.is_hidden(p.row));
    }
    let fmt = |v: f64| format_number(v, 5);
    ui.label(
        RichText::new(format!(
            "Predicted {} at the marked point ({} = {}, {} = {}): {}",
            mesh.response,
            mesh.x_name,
            fmt(mesh.current[0]),
            mesh.y_name,
            fmt(mesh.current[1]),
            fmt(mesh.current[2])
        ))
        .strong(),
    );

    plot(ui, &mesh, state, *show_observed, rows, id, SurfaceKind::Model);
}

/// Draw `mesh` (with its observations when `show_points`) and handle its
/// gestures: drag to turn, modifier-drag or click to select rows, hover for
/// values, right-click to save the figure.
pub(crate) fn plot(
    ui: &mut egui::Ui,
    mesh: &SurfaceMesh,
    state: &mut SurfaceProfilerState,
    show_points: bool,
    rows: &mut RowStateManager,
    id: u64,
    kind: SurfaceKind,
) {
    let pal = theme::plot_palette(ui);
    let fmt = |v: f64| format_number(v, 5);
    let empty = SurfaceMesh { points: Vec::new(), ..mesh.clone() };
    let mesh = if show_points { mesh } else { &empty };
    let show_observed = &show_points;
    let mut size = state.size.capped(ui, PLOT_HEIGHT);
    size.x = size.x.max(MIN_WIDTH);
    let (response, painter) = ui.allocate_painter(size, Sense::click_and_drag());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, "Surface (drag to turn)"));
    let frame = GripFrame { size, area: response.rect, panels: [1, 1], min_width: MIN_WIDTH, what: "plot" };
    shared_grip(ui, &mut state.size, ui.make_persistent_id(("surface3d_size", id, kind.title())), &frame);
    // Gestures: drag rotates; with observed data shown, shift/⌘/⌥-drag draws a
    // selection box and a click selects the nearest point (the modifier sets
    // how the selection combines, as in the other plots).
    let modifiers = ui.input(|i| i.modifiers);
    let mode = selection_mode(modifiers.shift, modifiers.command, modifiers.alt);
    let selecting = *show_observed && (modifiers.shift || modifiers.command || modifiers.alt);
    if response.drag_started() {
        state.box_start = if selecting { response.interact_pointer_pos() } else { None };
    }
    if response.dragged() && state.box_start.is_none() {
        let d = response.drag_delta();
        state.camera.rotate(f64::from(d.x) * 0.5, f64::from(d.y) * 0.4);
    }
    if response.double_clicked() {
        state.camera = Camera::default();
    }
    if response.hovered() {
        let icon =
            if selecting || state.box_start.is_some() { egui::CursorIcon::Crosshair } else { egui::CursorIcon::Grab };
        ui.ctx().set_cursor_icon(icon);
    }
    let selected: Vec<bool> = mesh.points.iter().map(|p| rows.is_selected(p.row)).collect();
    let look = Look::of(ui);
    let measure = |text: &str| {
        ui.fonts(|f| f.layout_no_wrap(text.to_owned(), FontId::proportional(look.font), Color32::WHITE).size().x)
    };
    let (prims, positions) = scene(response.rect, mesh, &selected, state, look, &measure, kind);
    paint(&painter, &prims);
    // Right-click saves the plot as drawn (same size, angle and settings), in
    // the light appearance.
    let rect = response.rect;
    response.context_menu(|ui| {
        crate::report_view::save_figure_items(
            ui,
            id,
            kind.title(),
            "The surface at its on-screen size, angle and settings.",
            &|format| {
                let look = Look::light();
                let (prims, _) = scene(rect, mesh, &selected, state, look, &|t| look.text_width(t), kind);
                Some(scene_figure(&prims, rect, kind.title(), format))
            },
        );
    });
    if let (Some(start), Some(now)) = (state.box_start, response.interact_pointer_pos()) {
        let region = egui::Rect::from_two_pos(start, now);
        painter.rect(region, 0.0, pal.brush_fill, Stroke::new(1.0_f32, pal.selected), egui::StrokeKind::Inside);
        if response.drag_stopped() {
            let inside = positions.iter().zip(&mesh.points).filter(|(p, _)| region.contains(**p)).map(|(_, o)| o.row);
            rows.select(inside.collect::<Vec<_>>(), mode);
            state.box_start = None;
        }
    }
    if response.drag_stopped() {
        state.box_start = None;
    }
    if *show_observed && response.clicked() {
        let hit = response.interact_pointer_pos().and_then(|pointer| {
            nearest_row(
                mesh.points.iter().zip(&positions).map(|(o, p)| (o.row, [p.x, p.y])),
                [pointer.x, pointer.y],
                7.0,
            )
        });
        brush_click(hit, mode, rows);
    }
    // Hovering an observation shows its row and values.
    let hovered = response.hover_pos().and_then(|pointer| {
        positions
            .iter()
            .enumerate()
            .map(|(k, p)| (k, p.distance(pointer)))
            .filter(|&(_, d)| d <= 7.0)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(k, _)| k)
    });
    if let Some(k) = hovered {
        let p = mesh.points[k];
        painter.circle_stroke(positions[k], 7.0, Stroke::new(2.0_f32, pal.selected));
        response.on_hover_ui_at_pointer(|ui| {
            ui.label(RichText::new(format!("Row {}", p.row + 1)).strong());
            ui.label(format!("{} = {}", mesh.x_name, fmt(p.x)));
            ui.label(format!("{} = {}", mesh.y_name, fmt(p.y)));
            ui.label(format!("Observed {} = {}", mesh.response, fmt(p.observed)));
            match kind {
                SurfaceKind::Model => {
                    ui.label(format!(
                        "Surface here = {} ({} it)",
                        fmt(p.surface),
                        if p.above_surface() { "above" } else { "below" }
                    ));
                    ui.label(format!(
                        "Fitted for this row = {}  (residual {})",
                        fmt(p.fitted),
                        fmt(p.observed - p.fitted)
                    ));
                }
                SurfaceKind::Data { exact: true } => {}
                SurfaceKind::Data { exact: false } => {
                    ui.label(format!("Surface here = {}  (residual {})", fmt(p.surface), fmt(p.observed - p.surface)));
                }
            }
        });
    }
    if *show_observed && !mesh.points.is_empty() {
        let text = match kind {
            SurfaceKind::Model => {
                "Observed rows: triangles point up above the drawn surface and down below it. The surface holds the \
                 other factors at the settings above, so a row whose own settings differ can sit away from it even \
                 when the model fits it well (hover for the row's own fitted value and residual). Click a point to \
                 select its row (Shift adds, Command toggles, Option removes); hold one of those keys and drag a box to \
                 select several."
            }
            SurfaceKind::Data { exact: true } => {
                "Observations sit on the surface. Click a point to select its row (Shift adds, Command toggles, \
                 Option removes); hold one of those keys and drag a box to select several."
            }
            SurfaceKind::Data { exact: false } => {
                "Observations: triangles point up above the smoothed surface and down below it. Click a point to \
                 select its row (Shift adds, Command toggles, Option removes); hold one of those keys and drag a box \
                 to select several."
            }
        };
        ui.label(RichText::new(text).weak().small());
    }
}

/// Colour at `t ∈ [0, 1]` of `scale` (reversed when asked), as components
/// for [`shade`].
pub(crate) fn scale_rgb(colors: (stat_viz::palette::ColorScale, bool), t: f64) -> [f32; 3] {
    let t = if colors.1 { 1.0 - t.clamp(0.0, 1.0) } else { t };
    let c = colors.0.color(t, false).0;
    [f32::from(c[0]), f32::from(c[1]), f32::from(c[2])]
}

/// A sequential ramp (deep blue → teal → yellow), light enough at the top to
/// keep dark contour lines readable.
pub(crate) fn ramp(t: f64) -> [f32; 3] {
    const STOPS: [[f32; 3]; 5] =
        [[46.0, 52.0, 128.0], [38.0, 110.0, 170.0], [36.0, 158.0, 146.0], [124.0, 196.0, 92.0], [244.0, 222.0, 92.0]];
    let t = t.clamp(0.0, 1.0) as f32 * (STOPS.len() - 1) as f32;
    let k = (t.floor() as usize).min(STOPS.len() - 2);
    let f = t - k as f32;
    let (a, b) = (STOPS[k], STOPS[k + 1]);
    [a[0] + f * (b[0] - a[0]), a[1] + f * (b[1] - a[1]), a[2] + f * (b[2] - a[2])]
}

pub(crate) fn shade(rgb: [f32; 3], light: f32) -> Color32 {
    let c = |v: f32| (v * light).clamp(0.0, 255.0) as u8;
    Color32::from_rgb(c(rgb[0]), c(rgb[1]), c(rgb[2]))
}

/// Unit-cube corners of the box edges: (start, end).
pub(crate) const EDGES: [([f64; 3], [f64; 3]); 12] = [
    ([-1.0, -1.0, -1.0], [1.0, -1.0, -1.0]),
    ([-1.0, 1.0, -1.0], [1.0, 1.0, -1.0]),
    ([-1.0, -1.0, 1.0], [1.0, -1.0, 1.0]),
    ([-1.0, 1.0, 1.0], [1.0, 1.0, 1.0]),
    ([-1.0, -1.0, -1.0], [-1.0, 1.0, -1.0]),
    ([1.0, -1.0, -1.0], [1.0, 1.0, -1.0]),
    ([-1.0, -1.0, 1.0], [-1.0, 1.0, 1.0]),
    ([1.0, -1.0, 1.0], [1.0, 1.0, 1.0]),
    ([-1.0, -1.0, -1.0], [-1.0, -1.0, 1.0]),
    ([1.0, -1.0, -1.0], [1.0, -1.0, 1.0]),
    ([-1.0, 1.0, -1.0], [-1.0, 1.0, 1.0]),
    ([1.0, 1.0, -1.0], [1.0, 1.0, 1.0]),
];

/// One drawing primitive of the profiler, in screen points. The same list
/// is painted on screen and written to figure files, so both show the same
/// picture.
#[derive(Debug, Clone)]
pub(crate) enum Prim {
    /// A filled convex polygon. `seamless` ones tile with their neighbours
    /// (surface cells, legend steps), so files outline them in their own
    /// colour to hide anti-aliasing seams.
    Fill { pts: Vec<Pos2>, color: Color32, seamless: bool },
    /// A polyline; `dash` is (dash, gap) lengths.
    Line { pts: Vec<Pos2>, width: f32, color: Color32, dash: Option<(f32, f32)> },
    /// One line of text at `at`, aligned by `align`.
    Text { at: Pos2, align: Align2, text: String, size: f32, color: Color32 },
}

/// Colours and text sizes the profiler is drawn with.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Look {
    pub(crate) dark: bool,
    pub(crate) text: Color32,
    pub(crate) weak: Color32,
    /// Plot background.
    pub(crate) background: Color32,
    /// Floor fill.
    pub(crate) floor: Color32,
    pub(crate) above: Color32,
    pub(crate) below: Color32,
    pub(crate) selected: Color32,
    /// Text size in points.
    pub(crate) font: f32,
}

impl Look {
    pub(crate) fn of(ui: &egui::Ui) -> Self {
        let pal = theme::plot_palette(ui);
        let v = ui.visuals();
        Self {
            dark: v.dark_mode,
            text: v.text_color(),
            weak: v.weak_text_color(),
            background: v.extreme_bg_color,
            floor: v.faint_bg_color,
            above: pal.point,
            below: pal.fit_line,
            selected: pal.selected,
            font: 11.0 * theme::text_scale(ui),
        }
    }

    /// The light appearance, for figure files.
    pub(crate) fn light() -> Self {
        let pal = theme::PlotPalette::light();
        let v = egui::Visuals::light();
        Self {
            dark: false,
            text: v.text_color(),
            weak: v.weak_text_color(),
            background: Color32::WHITE,
            floor: v.faint_bg_color,
            above: pal.point,
            below: pal.fit_line,
            selected: pal.selected,
            font: 11.0,
        }
    }

    /// Approximate width of `text` (for label backings in files; the screen
    /// measures exactly).
    pub(crate) fn text_width(self, text: &str) -> f32 {
        text.chars().count() as f32 * self.font * 0.56
    }
}

/// Points per screen point in figure files (as for report plots).
const POINTS_PER_PIXEL: f64 = 0.75;

/// `prims`, drawn in `rect` on screen, as a figure file titled `title`: the
/// same picture, at the plot's on-screen proportions.
pub(crate) fn scene_figure(prims: &[Prim], rect: egui::Rect, title: &str, format: VectorFormat) -> Vec<u8> {
    let k = POINTS_PER_PIXEL;
    let (w, h) = (f64::from(rect.width()) * k, f64::from(rect.height()) * k);
    // Page coordinates: x right, y up, origin at the plot's bottom-left.
    let at = |p: Pos2| [f64::from(p.x - rect.min.x) * k, -f64::from(p.y - rect.min.y) * k + h];
    let rgba = |c: Color32| Rgba(c.to_srgba_unmultiplied());
    let mut panel = ChartPanel { bare: true, x_range: (0.0, w), y_range: (0.0, h), ..ChartPanel::default() };
    for prim in prims {
        match prim {
            Prim::Fill { pts, color, seamless } => panel.marks.push(Mark::Polygon {
                points: pts.iter().map(|p| at(*p)).collect(),
                fill: rgba(*color),
                // Outlining tiles in their own colour hides the hairline
                // seams anti-aliasing leaves between neighbours.
                stroke: if *seamless { rgba(*color) } else { Rgba([0, 0, 0, 0]) },
                name: String::new(),
            }),
            Prim::Line { pts, width, color, dash } => panel.marks.push(Mark::Line {
                points: pts.iter().map(|p| at(*p)).collect(),
                color: rgba(*color),
                // Files draw line marks at 0.8 of their width.
                width: (f64::from(*width) * k / 0.8) as f32,
                pattern: if dash.is_some() { LinePattern::Dashed } else { LinePattern::Solid },
                name: String::new(),
            }),
            Prim::Text { at: p, align, text, size, color } => panel.labels.push(Label {
                at: at(*p),
                text: text.clone(),
                size: f64::from(*size) * k,
                color: rgba(*color),
                align: f64::from(align.x().to_factor()),
                valign: f64::from(align.y().to_factor()),
                bold: false,
            }),
        }
    }
    // The page adds a margin, the title and the bare panel's inset (see
    // `export_chart_grid`).
    let figure = Figure { title, format, width: w + 44.0, height: h + 66.0 };
    export_chart(&[panel], figure)
}

/// Paint `prims` on screen. Fills and thin solid lines go into one mesh (no
/// anti-aliasing seams between surface cells); other lines and text are
/// painted in order between mesh batches.
pub(crate) fn paint(painter: &egui::Painter, prims: &[Prim]) {
    let mut mesh = egui::Mesh::default();
    let flush = |mesh: &mut egui::Mesh| {
        if !mesh.is_empty() {
            painter.add(Shape::mesh(std::mem::take(mesh)));
        }
    };
    for prim in prims {
        match prim {
            Prim::Fill { pts, color, .. } => {
                let base = mesh.vertices.len() as u32;
                for p in pts {
                    mesh.colored_vertex(*p, *color);
                }
                for k in 1..pts.len().saturating_sub(1) as u32 {
                    mesh.add_triangle(base, base + k, base + k + 1);
                }
            }
            Prim::Line { pts, width, color, dash: None } if *width <= 1.2 => {
                for w in pts.windows(2) {
                    thin_quad(&mut mesh, w[0], w[1], *width, *color);
                }
            }
            Prim::Line { pts, width, color, dash } => {
                flush(&mut mesh);
                let stroke = Stroke::new(*width, *color);
                match dash {
                    Some((d, g)) => {
                        painter.extend(Shape::dashed_line(pts, stroke, *d, *g));
                    }
                    None => {
                        painter.add(Shape::line(pts.clone(), stroke));
                    }
                }
            }
            Prim::Text { at, align, text, size, color } => {
                flush(&mut mesh);
                painter.text(*at, *align, text, FontId::proportional(*size), *color);
            }
        }
    }
    flush(&mut mesh);
}

/// The profiler drawn into `rect`: its primitives and the screen position of
/// each observed point. `measure` gives a text's width at the look's size.
pub(crate) fn scene(
    rect: egui::Rect,
    mesh: &SurfaceMesh,
    selected: &[bool],
    state: &SurfaceProfilerState,
    look: Look,
    measure: &dyn Fn(&str) -> f32,
    kind: SurfaceKind,
) -> (Vec<Prim>, Vec<Pos2>) {
    let mut out = Vec::new();
    let dark = look.dark;
    let scale_text = look.font / 11.0;
    let fill = |pts: Vec<Pos2>, color: Color32, seamless: bool| Prim::Fill { pts, color, seamless };
    let line = |pts: Vec<Pos2>, width: f32, color: Color32| Prim::Line { pts, width, color, dash: None };
    out.push(fill(
        vec![rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom()],
        look.background,
        false,
    ));

    // Fit the projected box into the plot, leaving room for labels and the legend.
    let legend_width = 70.0 * scale_text;
    let margin = 46.0 * scale_text;
    let area = egui::Rect::from_min_max(
        rect.min + Vec2::splat(margin),
        Pos2::new(rect.max.x - legend_width - margin, rect.max.y - margin),
    );
    let cam = state.camera;
    let to_screen = box_projection(area, cam);
    let data = |p: [f64; 3]| to_screen(mesh.normalise(p));
    let squash = |p: [f64; 3]| [p[0], p[1], p[2] * BOX_HEIGHT];
    let (z_lo, z_hi) = mesh.z_range;
    let t_of = |z: f64| if z_hi > z_lo { (z - z_lo) / (z_hi - z_lo) } else { 0.5 };
    let frame = look.weak.gamma_multiply(0.7);

    // 1. Back edges, and the floor when it is behind the surface (seen from
    //    above); from below it is drawn last, without its fill.
    let front =
        |a: [f64; 3], b: [f64; 3]| to_screen([(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0, (a[2] + b[2]) / 2.0]).1 > 0.0;
    let floor_edge = |a: [f64; 3], b: [f64; 3]| a[2] == -1.0 && b[2] == -1.0;
    let floor_behind = cam.sees_floor_from_above();
    let early = |a: [f64; 3], b: [f64; 3]| if floor_edge(a, b) { floor_behind } else { !front(a, b) };
    for (a, b) in EDGES {
        if early(a, b) {
            out.push(line(vec![to_screen(a).0, to_screen(b).0], 1.0, frame));
        }
    }
    let marker = look.selected;
    let [cx, cy, cz] = mesh.current;
    let marked = cx.is_finite() && cy.is_finite() && cz.is_finite();
    let draw_floor = |out: &mut Vec<Prim>, filled: bool| {
        if filled {
            let floor = [[-1.0, -1.0, -1.0], [1.0, -1.0, -1.0], [1.0, 1.0, -1.0], [-1.0, 1.0, -1.0]]
                .map(|p| to_screen(p).0)
                .to_vec();
            out.push(fill(floor, look.floor, false));
        }
        let alpha = if filled { 1.0 } else { 0.7 };
        if state.floor_map {
            for (k, contour) in &mesh.floor_contours {
                let rgb = scale_rgb(state.colors, t_of(mesh.levels[*k]));
                let pts: Vec<Pos2> = contour.iter().map(|&[u, v]| data([u, v, z_lo]).0).collect();
                let color = shade(rgb, if dark { 1.0 } else { 0.85 }).gamma_multiply(alpha);
                out.push(Prim::Line { pts, width: 1.3, color, dash: None });
            }
        }
        if marked {
            // Crosshair on the floor at the marked point.
            let pts = vec![data([mesh.xs[0], cy, z_lo]).0, data([cx, cy, z_lo]).0, data([cx, mesh.ys[0], z_lo]).0];
            out.push(line(pts, 1.0, marker.gamma_multiply(0.8 * alpha)));
        }
    };
    if floor_behind {
        draw_floor(&mut out, true);
    }

    // 2. The surface, back to front, each cell followed by its contour segments.
    let light = normalise([-0.35, -0.45, 0.82]);
    let contour_color =
        if dark { Color32::from_rgba_unmultiplied(250, 250, 250, 200) } else { Color32::from_rgb(30, 30, 40) };
    let grid_color = Color32::from_rgba_unmultiplied(0, 0, 0, if dark { 90 } else { 60 });
    // Observations, farthest first, painted in among the cells by depth.
    let outline = contrast(dark);
    let mut positions = vec![Pos2::ZERO; mesh.points.len()];
    let mut queue: Vec<(f64, usize)> =
        mesh.points.iter().enumerate().map(|(k, p)| (data([p.x, p.y, p.observed]).1, k)).collect();
    queue.sort_by(|a, b| b.0.total_cmp(&a.0)); // nearest first, popped last
    let mut paint_point = |out: &mut Vec<Prim>, k: usize| {
        let p = mesh.points[k];
        let at = data([p.x, p.y, p.observed]).0;
        positions[k] = at;
        if kind == (SurfaceKind::Data { exact: true }) {
            out.push(Prim::Fill { pts: circle(at, 5.0), color: outline, seamless: false });
            out.push(Prim::Fill { pts: circle(at, 3.8), color: look.above, seamless: false });
        } else {
            let up = p.above_surface();
            triangle_marker(out, at, 5.5, up, if up { look.above } else { look.below }, outline);
        }
        if selected.get(k).copied().unwrap_or(false) {
            out.push(ring(at, 9.0, 2.0, look.selected));
        }
    };
    for (depth, i, j) in mesh.cells_with_depth(&cam) {
        while queue.last().is_some_and(|q| q.0 <= depth) {
            let (_, k) = queue.pop().expect("checked");
            paint_point(&mut out, k);
        }
        let c = mesh.cell_corners(i, j);
        let n: Vec<[f64; 3]> = c.iter().map(|&p| squash(mesh.normalise(p))).collect();
        let normal = normalise(cross(sub(n[1], n[0]), sub(n[3], n[0])));
        let lambert = (normal[0] * light[0] + normal[1] * light[1] + normal[2] * light[2]).abs() as f32;
        let brightness = 0.62 + 0.38 * lambert;
        // One colour per cell (the mean height), so files can fill it flat.
        let mean = c.iter().map(|p| p[2]).sum::<f64>() / 4.0;
        if !state.filled {
            // Wireframe: the cell's edges in its height colour, nothing hidden.
            let color = shade(scale_rgb(state.colors, t_of(mean)), if dark { 1.0 } else { 0.8 });
            let mut pts: Vec<Pos2> = c.iter().map(|p| data(*p).0).collect();
            pts.push(pts[0]);
            out.push(line(pts, 1.0, color));
        } else {
            out.push(fill(
                c.iter().map(|p| data(*p).0).collect(),
                shade(scale_rgb(state.colors, t_of(mean)), brightness),
                true,
            ));
        }
        if state.mesh_lines && state.filled {
            for e in 0..4 {
                out.push(line(vec![data(c[e]).0, data(c[(e + 1) % 4]).0], 0.6, grid_color));
            }
        }
        if state.surface_contours {
            for (_, a, b) in &mesh.cell_contours[j * mesh.resolution() + i] {
                out.push(line(vec![data(*a).0, data(*b).0], 1.1, contour_color));
            }
        }
    }
    while let Some((_, k)) = queue.pop() {
        paint_point(&mut out, k);
    }

    // 3. The floor if seen from below, front edges, the marked point, axes and legend.
    if !floor_behind {
        draw_floor(&mut out, false);
    }
    for (a, b) in EDGES {
        if !early(a, b) {
            out.push(line(vec![to_screen(a).0, to_screen(b).0], 1.0, frame));
        }
    }
    if marked {
        let top = data([cx, cy, cz]).0;
        let foot = data([cx, cy, z_lo]).0;
        out.push(Prim::Line { pts: vec![foot, top], width: 1.2, color: marker, dash: Some((4.0, 3.0)) });
        out.push(fill(circle(top, 6.5), contrast(dark), false));
        out.push(fill(circle(top, 5.0), marker, false));
        // Value label on a backing so it reads over any part of the surface.
        let label = format_number(cz, 5);
        let size = Vec2::new(measure(&label), look.font * 1.25);
        let at = top + Vec2::new(10.0, -10.0 - size.y);
        let back = egui::Rect::from_min_size(at, size).expand(3.0);
        out.push(fill(
            vec![back.left_top(), back.right_top(), back.right_bottom(), back.left_bottom()],
            look.background.gamma_multiply(0.85),
            false,
        ));
        out.push(Prim::Text { at, align: Align2::LEFT_TOP, text: label, size: look.font, color: look.text });
    }
    let ranges = [(mesh.xs[0], mesh.xs[mesh.xs.len() - 1]), (mesh.ys[0], mesh.ys[mesh.ys.len() - 1]), mesh.z_range];
    axes(&mut out, &to_screen, ranges, [&mesh.x_name, &mesh.y_name, &mesh.response], look);
    let legend_title = if kind == SurfaceKind::Model { "Predicted" } else { "Surface" };
    legend(&mut out, rect, legend_width, mesh, look, &t_of, legend_title, state.colors);
    if kind == (SurfaceKind::Data { exact: true }) {
        // The points lie on the surface: no above/below key.
    } else if !mesh.points.is_empty() {
        // Key for the observation markers.
        let base = rect.min + Vec2::new(14.0, 16.0);
        triangle_marker(&mut out, base, 5.5, true, look.above, outline);
        triangle_marker(&mut out, base + Vec2::new(0.0, 18.0), 5.5, false, look.below, outline);
        for (dy, text) in [(0.0, "Above surface"), (18.0, "Below surface")] {
            out.push(Prim::Text {
                at: base + Vec2::new(10.0, dy),
                align: Align2::LEFT_CENTER,
                text: text.to_owned(),
                size: look.font,
                color: look.text,
            });
        }
    }
    (out, positions)
}

/// Height of the box relative to its floor, so a surface reads as a landscape.
pub(crate) const BOX_HEIGHT: f64 = 0.7;

/// The screen projection of the unit cube `[−1, 1]³` (vertical axis drawn
/// [`BOX_HEIGHT`] tall) seen by `cam`, fitted into `area`: each point's
/// screen position and depth (larger is nearer).
pub(crate) fn box_projection(area: egui::Rect, cam: Camera) -> impl Fn([f64; 3]) -> (Pos2, f64) {
    let squash = |p: [f64; 3]| [p[0], p[1], p[2] * BOX_HEIGHT];
    let corners: Vec<[f64; 3]> = EDGES.iter().flat_map(|(a, b)| [*a, *b]).map(|p| cam.project(squash(p))).collect();
    let (min_x, max_x) = corners.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| (lo.min(p[0]), hi.max(p[0])));
    let (min_y, max_y) = corners.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| (lo.min(p[1]), hi.max(p[1])));
    let s = (f64::from(area.width()) / (max_x - min_x)).min(f64::from(area.height()) / (max_y - min_y));
    let centre = area.center();
    let (mid_x, mid_y) = (0.5 * (min_x + max_x), 0.5 * (min_y + max_y));
    move |p: [f64; 3]| -> (Pos2, f64) {
        let q = cam.project(squash(p));
        (Pos2::new(centre.x + ((q[0] - mid_x) * s) as f32, centre.y - ((q[1] - mid_y) * s) as f32), q[2])
    }
}

/// Points around a circle of radius `r` at `at`.
pub(crate) fn circle(at: Pos2, r: f32) -> Vec<Pos2> {
    let n = 20;
    (0..n)
        .map(|k| {
            let a = std::f32::consts::TAU * k as f32 / n as f32;
            at + Vec2::new(a.cos(), a.sin()) * r
        })
        .collect()
}

/// A ring (selection mark) of radius `r` around `at`.
pub(crate) fn ring(at: Pos2, r: f32, width: f32, color: Color32) -> Prim {
    let mut pts = circle(at, r);
    pts.push(pts[0]);
    Prim::Line { pts, width, color, dash: None }
}

/// A filled triangle pointing up (`up`) or down, centred on `at`, with an outline.
pub(crate) fn triangle_marker(out: &mut Vec<Prim>, at: Pos2, r: f32, up: bool, fill: Color32, outline: Color32) {
    let s = if up { -1.0 } else { 1.0 };
    let pts =
        [at + Vec2::new(0.0, s * r), at + Vec2::new(-0.87 * r, -s * 0.5 * r), at + Vec2::new(0.87 * r, -s * 0.5 * r)];
    let grow = |p: Pos2, by: f32| at + (p - at) * by;
    for (scale, color) in [(1.35, outline), (1.0, fill)] {
        out.push(Prim::Fill { pts: pts.iter().map(|p| grow(*p, scale)).collect(), color, seamless: false });
    }
}

/// Tick labels and titles along the nearest floor edges and a left vertical
/// edge, for a box spanning `ranges` (x, y, z) with axes named `names`.
pub(crate) fn axes(
    out: &mut Vec<Prim>,
    to_screen: &dyn Fn([f64; 3]) -> (Pos2, f64),
    ranges: [(f64, f64); 3],
    names: [&str; 3],
    look: Look,
) {
    let text =
        |at: Pos2, align: Align2, text: String, color: Color32| Prim::Text { at, align, text, size: look.font, color };
    let scale = |v: f64, lo: f64, hi: f64| if hi > lo { 2.0 * (v - lo) / (hi - lo) - 1.0 } else { 0.0 };
    let ticks = |lo: f64, hi: f64| -> Vec<f64> {
        if hi <= lo {
            return vec![lo];
        }
        let step = nice_step((hi - lo) / 4.0);
        let mut out = Vec::new();
        let mut k = (lo / step).ceil();
        while k * step <= hi + 1e-9 * step.abs() && out.len() < 12 {
            out.push(k * step);
            k += 1.0;
        }
        out
    };
    let centre = to_screen([0.0, 0.0, -1.0]).0;
    let outward = |p: Pos2| {
        let d = p - centre;
        if d.length() > 0.0 { d.normalized() } else { Vec2::Y }
    };
    // Each floor axis on whichever of its two parallel edges is nearer the viewer.
    let [(x0, x1), (y0, y1), (z_lo, z_hi)] = ranges;
    let nearer = |a: [f64; 3], b: [f64; 3]| if to_screen(a).1 >= to_screen(b).1 { a } else { b };
    let x_edge = nearer([0.0, -1.0, -1.0], [0.0, 1.0, -1.0])[1];
    let y_edge = nearer([-1.0, 0.0, -1.0], [1.0, 0.0, -1.0])[0];
    for (axis, lo, hi, name) in [(0, x0, x1, names[0]), (1, y0, y1, names[1])] {
        let at = |v: f64| if axis == 0 { [scale(v, lo, hi), x_edge, -1.0] } else { [y_edge, scale(v, lo, hi), -1.0] };
        let (a, b) = (to_screen(at(lo)).0, to_screen(at(hi)).0);
        if (b - a).length() < 30.0 {
            continue; // seen end-on
        }
        // Half the extent of a centred label along direction `o`, from an
        // estimate of its size (about 0.6 em a character, 1.2 em a line).
        let half_extent = |label: &str, o: Vec2| {
            let (w, h) = (0.6 * look.font * label.chars().count() as f32, 1.2 * look.font);
            0.5 * (o.x.abs() * w + o.y.abs() * h)
        };
        let mut reach = 0.0_f32;
        for v in ticks(lo, hi) {
            let p = to_screen(at(v)).0;
            let o = outward(p);
            let label = format_number(v, 4);
            let off = 8.0 + half_extent(&label, o);
            reach = reach.max(off + half_extent(&label, o));
            out.push(Prim::Line { pts: vec![p, p + o * 4.0], width: 1.0, color: look.weak, dash: None });
            out.push(text(p + o * off, Align2::CENTER_CENTER, label, look.weak));
        }
        // The title beyond the tick labels, however the edge is turned.
        let mid = to_screen(at(0.5 * (lo + hi))).0;
        let o = outward(mid);
        let off = reach + 6.0 + half_extent(name, o);
        out.push(text(mid + o * off, Align2::CENTER_CENTER, name.to_owned(), look.text));
    }
    // Response axis on the leftmost vertical edge.
    let posts = [[-1.0, -1.0], [1.0, -1.0], [-1.0, 1.0], [1.0, 1.0]];
    let post =
        posts.iter().min_by(|a, b| to_screen([a[0], a[1], 0.0]).0.x.total_cmp(&to_screen([b[0], b[1], 0.0]).0.x));
    let Some(&[px, py]) = post else { return };
    for v in ticks(z_lo, z_hi) {
        let p = to_screen([px, py, scale(v, z_lo, z_hi)]).0;
        out.push(Prim::Line { pts: vec![p, p - Vec2::new(4.0, 0.0)], width: 1.0, color: look.weak, dash: None });
        out.push(text(p - Vec2::new(7.0, 0.0), Align2::RIGHT_CENTER, format_number(v, 4), look.weak));
    }
    let top = to_screen([px, py, 1.0]).0;
    out.push(text(top - Vec2::new(0.0, 12.0), Align2::CENTER_BOTTOM, names[2].to_owned(), look.text));
}

/// Colour bar with the response scale and a tick at every contour level.
#[allow(clippy::too_many_arguments)]
fn legend(
    out: &mut Vec<Prim>,
    rect: egui::Rect,
    width: f32,
    mesh: &SurfaceMesh,
    look: Look,
    t_of: &dyn Fn(f64) -> f64,
    title: &str,
    colors: (stat_viz::palette::ColorScale, bool),
) {
    let bar = egui::Rect::from_min_size(
        Pos2::new(rect.max.x - width + 8.0, rect.min.y + 40.0),
        Vec2::new(14.0, (rect.height() - 80.0).max(40.0)),
    );
    let corners = |r: egui::Rect| vec![r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()];
    let steps = 48;
    for k in 0..steps {
        let (t0, t1) = (k as f64 / steps as f64, (k + 1) as f64 / steps as f64);
        let y0 = bar.max.y - bar.height() * t0 as f32;
        let y1 = bar.max.y - bar.height() * t1 as f32;
        let r = egui::Rect::from_min_max(Pos2::new(bar.min.x, y1), Pos2::new(bar.max.x, y0));
        out.push(Prim::Fill { pts: corners(r), color: shade(scale_rgb(colors, 0.5 * (t0 + t1)), 1.0), seamless: true });
    }
    let mut outline = corners(bar.expand(0.5));
    outline.push(outline[0]);
    out.push(Prim::Line { pts: outline, width: 1.0, color: look.text.gamma_multiply(0.5), dash: None });
    let y_of = |z: f64| bar.max.y - bar.height() * t_of(z) as f32;
    let mut last = f32::INFINITY;
    for &level in &mesh.levels {
        let y = y_of(level);
        out.push(Prim::Line {
            pts: vec![Pos2::new(bar.min.x - 3.0, y), Pos2::new(bar.max.x + 3.0, y)],
            width: 1.0,
            color: contrast(look.dark),
            dash: None,
        });
        if (last - y).abs() >= look.font + 2.0 {
            out.push(Prim::Text {
                at: Pos2::new(bar.max.x + 6.0, y),
                align: Align2::LEFT_CENTER,
                text: format_number(level, 4),
                size: look.font,
                color: look.text,
            });
            last = y;
        }
    }
    out.push(Prim::Text {
        at: Pos2::new(bar.center().x, bar.min.y - 6.0),
        align: Align2::CENTER_BOTTOM,
        text: title.to_owned(),
        size: look.font,
        color: look.text,
    });
}

pub(crate) fn contrast(dark: bool) -> Color32 {
    if dark { Color32::from_gray(240) } else { Color32::from_gray(25) }
}

/// A line from `a` to `b` of `width` pixels as two triangles in `mesh`.
fn thin_quad(mesh: &mut egui::Mesh, a: Pos2, b: Pos2, width: f32, color: Color32) {
    let d = b - a;
    if d.length() < 1e-3 {
        return;
    }
    let n = Vec2::new(-d.y, d.x).normalized() * (width / 2.0);
    let base = mesh.vertices.len() as u32;
    for p in [a + n, b + n, b - n, a - n] {
        mesh.colored_vertex(p, color);
    }
    mesh.add_triangle(base, base + 1, base + 2);
    mesh.add_triangle(base, base + 2, base + 3);
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn normalise(v: [f64; 3]) -> [f64; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l > 0.0 { [v[0] / l, v[1] / l, v[2] / l] } else { [0.0, 0.0, 1.0] }
}

#[cfg(test)]
mod tests {
    use super::*;
    use app_model::{SurfaceAnalysis, SurfaceRequest};

    /// The profiler saves as a figure of the picture on screen: the page
    /// keeps the plot's proportions, and the axis names, legend and surface
    /// all reach the file.
    #[test]
    fn surface_figures_match_the_screen() {
        let t = app_model::sample::reaction_yield();
        let rows = RowStateManager::new(t.n_rows());
        let a = SurfaceAnalysis::run(&t, &rows, &SurfaceRequest { response: 2, factors: vec![0, 1] }).unwrap();
        let settings = a.default_profile_settings();
        let state = SurfaceProfilerState::default();
        let mut mesh = surface_mesh(&a, &settings, 0, 1, state.resolution, state.levels).unwrap();
        add_observations(&mut mesh, &a, &settings, 0, 1).unwrap();
        let selected = vec![false; mesh.points.len()];
        let rect = egui::Rect::from_min_size(Pos2::new(10.0, 20.0), Vec2::new(800.0, PLOT_HEIGHT));
        let look = Look::light();
        let (prims, positions) =
            scene(rect, &mesh, &selected, &state, look, &|t| look.text_width(t), SurfaceKind::Model);
        assert_eq!(positions.len(), mesh.points.len());
        assert!(prims.iter().any(|p| matches!(p, Prim::Fill { seamless: true, .. })), "surface cells");
        let svg = String::from_utf8(scene_figure(&prims, rect, "3-D Response Surface", VectorFormat::Svg)).unwrap();
        assert!(svg.contains("viewBox=\"0 0 644 388.5\""), "{}", &svg[..200]);
        for name in ["3-D Response Surface", "Time (min)", &mesh.response] {
            assert!(svg.contains(name), "{name}");
        }
        let png = scene_figure(&prims, rect, "3-D Response Surface", VectorFormat::Png);
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        if let Ok(dir) = std::env::var("ANOVACRAFT_FIGURE_DIR") {
            std::fs::write(std::path::Path::new(&dir).join("surface.png"), &png).unwrap();
        }
    }
}
