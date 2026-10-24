//! The graph canvas: column shelves that assign data columns to visual
//! channels, a layer bar that switches drawing elements on and off, and a grid
//! of linked panels (small multiples) drawn from [`stat_viz`].
//!
//! Every panel follows the application's plot interaction model: no scroll
//! zoom; each axis is a draggable ruler (ends rescale, middle shifts); dragging
//! inside a panel selects the rows under the box in the shared row state, so
//! the data grid and every other plot highlight them at once. Panels showing
//! the same column share that axis's range, so moving one axis moves it in
//! every panel. Statistics labels (fit equations and R², ellipse
//! correlations) are the one exception to box selection: dragging one moves it.
//! Columns on the right-axis shelf are drawn against a second scale on the
//! right of each panel, whose ruler drags like the left one; an axis break
//! set in Plot settings leaves a stretch out of an axis.
//!
//! While a column is dragged, drop zones over the panels (edges for the
//! axes and splits, centre for colour) take it as the shelves would; the
//! row filter can sit in a panel beside the graph, with stepping through
//! levels.
//!
//! The shelf design follows the shelf-based view specification of Stolte,
//! Tang & Hanrahan ("Polaris", *IEEE TVCG* 8(1), 2002) and the panel grid the
//! trellis displays of Becker, Cleveland & Shyu (*JCGS* 5(2), 1996); labels
//! and layout are this application's own.

use std::collections::BTreeMap;

use app_model::brushing::{brush_click, nearest_row, selection_mode};
use app_model::plot_view::{AxisHandle, ViewBounds, drag_range, handle_at};
use eframe::egui::{self, PointerButton, RichText};
use egui_plot::{GridInput, GridMark, Line, Plot, PlotPoint, PlotPoints};
use stat_data::{DataTable, RowStateManager};
use stat_viz::render::{draw_layer_with, text_align};
use stat_viz::view::view_fraction;
use stat_viz::{
    AxisScale, AxisSpec, Frame, GraphForm, GraphSpec, Layer, LegendPlacement, Mark, Role, SharedRanges, place_labels,
    region_rows, rows_in_box, rows_in_polygon, scaled_ticks, tick_label,
};

use super::{
    CLICK_RADIUS, Gesture, PlotKey, PlotState, apply_bounds, axis_controls, draw_rubber_band, plot_gesture,
    plot_toolbar, size_grip,
};
use crate::plot_axes::{self, AxisInfo, PlotChrome, with_axes};
use crate::theme;

#[path = "graph_view/controls.rs"]
mod controls;
#[path = "graph_view/drop_zones.rs"]
mod drop_zones;
#[path = "graph_view/form_settings.rs"]
mod form_settings;
pub(crate) use controls::condition_editor;
#[path = "graph_view/layers.rs"]
mod layers;
#[path = "graph_view/legend.rs"]
mod legend;
#[path = "graph_view/markers.rs"]
mod markers;
#[path = "graph_view/notes.rs"]
mod notes;
#[path = "graph_view/output.rs"]
pub(super) mod output;
#[path = "graph_view/shelves.rs"]
mod shelves;

/// A column being dragged from the variables list or from another shelf.
#[derive(Debug, Clone)]
pub struct ColumnDrag {
    /// Column name.
    pub name: String,
    /// The shelf it is being moved from (`None`: from the variables list).
    pub from: Option<Role>,
}

/// A statistics label being dragged to a new place.
#[derive(Debug, Clone)]
struct LabelDrag {
    /// Panel index.
    panel: usize,
    /// The label's identifier.
    id: String,
    /// Pointer position minus the label's anchor point, in screen units.
    grab: egui::Vec2,
    /// Where the anchor point is now (plot coordinates).
    at: [f64; 2],
}

/// Everything the canvas draws, rebuilt when the data, the row states or the
/// graph's settings change.
struct Drawn {
    key: (u64, u64, usize),
    spec: GraphSpec,
    frame: Result<Frame, String>,
    layers: Vec<Vec<Layer>>,
    ranges: SharedRanges,
    /// What the right-axis shelf draws.
    right: Option<stat_viz::RightAxis>,
}

/// A drag of a panel's right-hand axis ruler in progress.
#[derive(Debug, Clone, Copy)]
struct RightDrag {
    panel: usize,
    handle: AxisHandle,
    /// The right axis's range when the drag began.
    start: (f64, f64),
    start_along: f64,
    length: f64,
}

/// View state of a graph canvas.
#[derive(Default)]
pub(super) struct GraphView {
    /// Settings shared by every panel (axes menu, size grip, reset).
    canvas: PlotState,
    /// Brush and axis-drag state of each panel.
    panels: Vec<PlotState>,
    /// Ranges set by dragging an axis, by column, shared across panels.
    x_views: BTreeMap<String, (f64, f64)>,
    y_views: BTreeMap<String, (f64, f64)>,
    drawn: Option<Drawn>,
    /// Shelves folded away (more room for the panels).
    hide_shelves: bool,
    /// Select with a freehand outline instead of a box.
    lasso: bool,
    /// The outline being drawn: panel and vertices (plot coordinates).
    lasso_path: Option<(usize, Vec<[f64; 2]>)>,
    /// Outcome of the last export or copy (`true`: success).
    status: Option<(bool, String)>,
    /// The statistics label being dragged.
    label_drag: Option<LabelDrag>,
    /// A label dropped this frame: panel, identifier, fraction of the view.
    label_dropped: Option<(usize, String, [f64; 2])>,
    /// The marker last right-clicked (its settings menu).
    marker_target: Option<markers::MarkerTarget>,
    /// The marker menu applies to every panel (by default only to the
    /// panel clicked).
    marker_all_panels: bool,
    /// Marker settings changed from a marker's menu this frame.
    style_edit: Option<stat_viz::MarkerStyle>,
    /// The right axis's range set by dragging its ruler (shared by panels).
    y2_view: Option<(f64, f64)>,
    /// A right-axis ruler drag in progress.
    right_drag: Option<RightDrag>,
    /// The row filter's stepping state.
    filter_panel: crate::filter_panel::FilterPanelState,
    /// The row filter sits in a panel beside the graph.
    filter_beside: bool,
    /// A column dropped on a drop zone over the panels this frame.
    dropped: Option<(Role, ColumnDrag)>,
    /// The graph's report (whose figure look changes it takes).
    id: u64,
    /// Annotations: selection, drag, editor.
    notes: notes::Notes,
    /// The key editor is open.
    key_editor: bool,
}

/// Title of a graph report: "Y by X", with any panel split.
pub(super) fn title(spec: &GraphSpec) -> String {
    app_model::graph_title(spec)
}

impl GraphView {
    /// Drop dragged axis ranges, so typed (or pasted) ranges show.
    pub(super) fn forget_views(&mut self) {
        self.x_views.clear();
        self.y_views.clear();
        self.y2_view = None;
    }

    /// Drop dragged or typed axis ranges that no longer apply: ranges are in
    /// drawn units, so a new axis scale (or a new chart form, which lays the
    /// axes out differently) makes them meaningless. Both axes are cleared,
    /// as some forms draw a column on either axis with the horizontal scale.
    fn forget_stale_ranges(&mut self, spec: &mut GraphSpec) {
        let Some(before) = self.drawn.as_ref().map(|d| &d.spec) else { return };
        let stale = std::mem::discriminant(&before.form) != std::mem::discriminant(&spec.form)
            || before.options.x_scale != spec.options.x_scale
            || before.options.y_scale != spec.options.y_scale;
        // Breaks change the drawn units too.
        let broken = before.axes.x_breaks != spec.axes.x_breaks || before.axes.y_breaks != spec.axes.y_breaks;
        if stale || broken {
            self.x_views.clear();
            self.y_views.clear();
            // Ranges typed in Plot settings too.
            if let Some(style) = spec.axes.style.as_mut() {
                (style.axes.x.range, style.axes.y.range) = (None, None);
            }
            if spec.axes.style.as_ref().is_some_and(|s| *s == app_model::plot_style::PlotStyle::default()) {
                spec.axes.style = None;
            }
        }
        if stale || before.encoding.y2 != spec.encoding.y2 {
            self.y2_view = None;
        }
    }

    /// Rebuild what is drawn if the data, row states or settings changed.
    fn refresh(&mut self, spec: &GraphSpec, table: &DataTable, rows: &RowStateManager) {
        let key = (table.revision(), rows.revision(), rows.len());
        if self.drawn.as_ref().is_some_and(|d| d.key == key && &d.spec == spec) {
            return;
        }
        // Moving a label, annotating or editing the key changes nothing
        // else: keep the layers.
        if let Some(d) = self.drawn.as_mut().filter(|d| d.key == key && same_but_decorations(&d.spec, spec)) {
            d.spec = spec.clone();
            return;
        }
        let frame = if rows.len() == table.n_rows() {
            spec.frame(table, rows).map_err(|e| e.to_string())
        } else {
            Err("the row states are out of step with the data".to_owned())
        };
        let layers = frame.as_ref().map(|f| spec.layers(f, rows)).unwrap_or_default();
        let ranges = frame.as_ref().map(|f| SharedRanges::of(f, &layers)).unwrap_or_default();
        let right = if frame.is_ok() { spec.right_axis(table, rows).ok().flatten() } else { None };
        self.drawn = Some(Drawn { key, spec: spec.clone(), frame, layers, ranges, right });
    }
}

/// `true` when `a` and `b` differ at most in moved labels, annotations and
/// key changes (which leave the layers as they are).
fn same_but_decorations(a: &GraphSpec, b: &GraphSpec) -> bool {
    let mut c = a.clone();
    c.label_positions.clone_from(&b.label_positions);
    let (notes, key) = (b.annotations().to_vec(), b.key_look());
    c.edit_style(|s| {
        s.annotations.items = notes;
        s.key = key;
    });
    c == *b
}

/// Copy the graph's plot style, titles and breaks into the Plot settings
/// shown for it, with what the view knows (breaks are available, the right
/// axis's default title). The graph is the record: an undo that changes it
/// changes the settings shown.
fn axes_from_spec(state: &mut PlotState, spec: &GraphSpec, right_title: Option<String>) {
    let g = &spec.axes;
    let style = g.style.clone().unwrap_or_default();
    state.reference = style.reference.clone();
    state.reference_supported = spec.form == GraphForm::Panels;
    state.reference_scopes = true;
    let axes = &mut state.axes;
    style.apply_to(axes);
    axes.title.clone_from(&g.title);
    axes.x.title.clone_from(&g.x_title);
    axes.y.title.clone_from(&g.y_title);
    axes.y2.title.clone_from(&g.y2_title);
    axes.x.breaks.clone_from(&g.x_breaks);
    axes.y.breaks.clone_from(&g.y_breaks);
    axes.breaks = [spec.form == GraphForm::Panels; 2];
    axes.y2_label = right_title;
}

/// Copy titles and breaks edited in Plot settings back into the graph
/// (where they are saved and exported).
fn axes_to_spec(state: &PlotState, spec: &mut GraphSpec) {
    let axes = &state.axes;
    let g = &mut spec.axes;
    g.title.clone_from(&axes.title);
    g.x_title.clone_from(&axes.x.title);
    g.y_title.clone_from(&axes.y.title);
    g.y2_title.clone_from(&axes.y2.title);
    g.x_breaks = axes.x.breaks.tidied();
    g.y_breaks = axes.y.breaks.tidied();
    // The rest of the settings, without the titles and breaks kept above.
    let mut style = g.style.clone().unwrap_or_default();
    style.axes = app_model::plot_style::user_axes(axes);
    style.reference.clone_from(&state.reference);
    style.axes.title = None;
    for a in [&mut style.axes.x, &mut style.axes.y, &mut style.axes.y2] {
        a.title = None;
        a.breaks = stat_viz::AxisBreaks::default();
    }
    g.style = (style != app_model::plot_style::PlotStyle::default()).then_some(style);
}

/// Where a panel's right-click menu asks for graph `id`'s Reference lines
/// window.
fn reference_open_id(id: u64) -> egui::Id {
    egui::Id::new(("graph_reference_lines_open", id))
}

/// The whole canvas: shelves, layer bar, panel grid, key and notes.
pub(super) fn show(
    ui: &mut egui::Ui,
    spec: &mut GraphSpec,
    view: &mut GraphView,
    table: &DataTable,
    rows: &mut RowStateManager,
    id: u64,
    reset: bool,
) {
    if !view.hide_shelves {
        shelves::shelves(ui, spec, table, id);
    }
    let drawn_frame = view.drawn.as_ref().and_then(|d| d.frame.as_ref().ok());
    let levels: Vec<String> =
        drawn_frame.and_then(|f| f.panels.first()).map(|p| p.x.levels().to_vec()).unwrap_or_default();
    layers::layer_bar(ui, spec, drawn_frame.map(|f| &f.legend), &levels);
    controls::view_bar(ui, spec, &mut view.lasso, &mut view.hide_shelves, &mut view.key_editor, id);
    form_settings::settings_bar(ui, spec, table);
    controls::filter_panel(ui, spec, table, rows, &mut view.filter_panel, &mut view.filter_beside, id);
    controls::switcher_bar(ui, spec, table, id);
    // Columns renamed away or deleted are dropped from the plan.
    for role in Role::ALL {
        let missing: Vec<String> =
            spec.encoding.get(role).into_iter().filter(|c| table.find(c).is_none()).map(str::to_owned).collect();
        for c in missing {
            spec.encoding.unassign(role, &c);
        }
    }
    spec.options.filter.conditions.retain(|c| table.find(c.column()).is_some());
    for w in &mut spec.switchers {
        w.choices.retain(|c| table.find(c).is_some());
    }
    let encoding = spec.encoding.clone();
    spec.switchers.retain(|w| !w.choices.is_empty() && w.current(&encoding).is_some());
    if spec.encoding.is_empty() {
        ui.add_space(6.0);
        ui.label(
            RichText::new(
                "Drag a column from the Variables list onto the Horizontal axis or Vertical axis shelf (or use +).",
            )
            .weak(),
        );
        return;
    }
    view.id = id;
    if let Some(look) = crate::plot_export::take_change(ui.ctx(), output::look_target(id)) {
        let mut style = spec.axes.style.clone().unwrap_or_default();
        style.export = look;
        spec.axes.style = (style != app_model::plot_style::PlotStyle::default()).then_some(style);
    }
    view.forget_stale_ranges(&mut *spec);
    view.refresh(spec, table, rows);
    let right_title = view.drawn.as_ref().and_then(|d| d.right.as_ref()).map(|r| r.title.clone());
    axes_from_spec(&mut view.canvas, spec, right_title);
    let open_id = reference_open_id(id);
    if ui.ctx().data_mut(|d| d.remove_temp::<bool>(open_id)).unwrap_or(false) {
        view.canvas.reference_open = true;
    }
    let Some(drawn) = view.drawn.take() else { return };
    match &drawn.frame {
        Err(message) => {
            ui.colored_label(theme::ERROR, message.as_str());
        }
        Ok(frame) => {
            controls::page_bar(ui, spec, &frame.pages, frame.page, id);
            let text = spec.axes.style.as_ref().map(|s| crate::plot_axes::ScreenText::set(ui.ctx(), &s.export));
            if view.filter_beside {
                // The filter's form on the left, the graph beside it.
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(FILTER_PANEL_WIDTH);
                        let GraphView { filter_panel, filter_beside, .. } = &mut *view;
                        controls::filter_body(ui, spec, table, rows, filter_panel, filter_beside, id);
                    });
                    ui.vertical(|ui| canvas(ui, spec, view, frame, &drawn, table, rows, id, reset));
                });
            } else {
                canvas(ui, spec, view, frame, &drawn, table, rows, id, reset);
            }
            drop(text);
            axes_to_spec(&view.canvas, spec);
            if let Some((role, drag)) = view.dropped.take() {
                drop_zones::apply_drop(&mut spec.encoding, role, &drag);
            }
            if let Some(style) = view.style_edit.take() {
                spec.options.style = style;
            }
            if let Some((panel, label, at)) = view.label_dropped.take() {
                spec.place_label(panel, &label, at);
            }
            view.notes.editor(ui.ctx(), spec, frame, id);
            if view.key_editor {
                let items = spec.key_items(frame, &drawn.layers, drawn.right.as_ref());
                legend::key_editor(ui.ctx(), spec, items, &mut view.key_editor, id);
            }
            if let Some(items) = view.notes.changed.take() {
                spec.edit_style(|s| s.annotations.items = items);
            }
        }
    }
    if let Some((ok, message)) = &view.status {
        let color = if *ok { ui.visuals().weak_text_color() } else { theme::ERROR };
        ui.colored_label(color, message.as_str());
    }
    view.drawn = Some(drawn);
}

/// How far the horizontal axis's ruler can reach below the panels' rows.
const AXIS_RULER_DEPTH: f32 = 28.0;
/// Width of the row filter's panel when it sits beside the graph.
const FILTER_PANEL_WIDTH: f32 = 250.0;
/// Width of the key when it sits right of the panels.
const LEGEND_WIDTH: f32 = 190.0;
/// Pixels reserved beside the grid for row headers.
const ROW_HEADER_WIDTH: f32 = 90.0;
/// Pixels each panel needs beyond its plot area for its own tick labels.
const PANEL_GAP: f32 = 6.0;

#[allow(clippy::too_many_arguments)]
fn canvas(
    ui: &mut egui::Ui,
    spec: &GraphSpec,
    view: &mut GraphView,
    frame: &Frame,
    drawn: &Drawn,
    table: &DataTable,
    rows: &mut RowStateManager,
    id: u64,
    reset: bool,
) {
    let first = frame.panels.first();
    let x_title = first.map(|p| p.x.title()).unwrap_or_default();
    let y_title = first.map(|p| spec.y_title(&p.y)).unwrap_or_default();
    let heading = match frame.page_title(&spec.encoding) {
        Some(page) => format!("{} · {page}", title(spec)),
        None => title(spec),
    };
    let info = AxisInfo { title: &heading, x_label: &x_title, y_label: &y_title, categorical_x: false, has_y: true };
    // The Plot settings menu shows (and edits) the first panel's ranges; they
    // count as set by the user when either of its axes has been dragged.
    let range_of = |views: &BTreeMap<String, (f64, f64)>, shared: &BTreeMap<String, (f64, f64)>, name: &str| {
        views.get(name).or_else(|| shared.get(name)).copied().unwrap_or((0.0, 1.0))
    };
    // Ranges typed in Plot settings (kept with the graph) apply to every
    // panel until an axis is dragged.
    for p in &frame.panels {
        if let Some([lo, hi]) = view.canvas.axes.x.range {
            view.x_views.entry(p.x.name.clone()).or_insert((lo, hi));
        }
        if let Some([lo, hi]) = view.canvas.axes.y.range {
            view.y_views.entry(p.y.name.clone()).or_insert((lo, hi));
        }
    }
    let first_view = first.map(|p| {
        let (x, y) =
            (range_of(&view.x_views, &drawn.ranges.x, &p.x.name), range_of(&view.y_views, &drawn.ranges.y, &p.y.name));
        ViewBounds { x_min: x.0, x_max: x.1, y_min: y.0, y_max: y.1 }
    });
    let user_set = first.is_some_and(|p| view.x_views.contains_key(&p.x.name) || view.y_views.contains_key(&p.y.name));
    view.canvas.last_view = first_view;
    view.canvas.bounds = if user_set { first_view } else { None };
    let before = view.canvas.bounds;
    let clicked = plot_toolbar(ui, &mut view.canvas, &info);
    let after = view.canvas.bounds.take();
    // "Fit view to data" and "Restore default settings" clear the ranges.
    let fit = before.is_some() && after.is_none();
    let reset = clicked || fit || reset || std::mem::take(&mut view.canvas.reset_pending);
    if reset {
        view.x_views.clear();
        view.y_views.clear();
        (view.canvas.axes.x.range, view.canvas.axes.y.range) = (None, None);
        view.y2_view = None;
        view.right_drag = None;
    } else if let Some(b) = after.filter(|b| Some(*b) != before) {
        // Ranges typed into the axes menu apply to every panel.
        for p in &frame.panels {
            view.x_views.insert(p.x.name.clone(), (b.x_min, b.x_max));
            view.y_views.insert(p.y.name.clone(), (b.y_min, b.y_max));
        }
    }
    let axes = view.canvas.axes.clone();
    let (n_rows, n_cols) = (frame.n_rows.max(1), frame.n_cols.max(1));
    let default_height = if n_rows == 1 { 340.0 } else { (230.0 * n_rows as f32).min(920.0) };
    let size = view.canvas.size.full(ui, default_height);
    super::plot_title(ui, &axes, &heading, size.x);
    // Headers that only repeat an axis title are left out.
    let axis_name = |row: Option<usize>, col: Option<usize>| {
        frame
            .panels
            .iter()
            .find(|p| row.is_none_or(|r| p.grid_row == r) && col.is_none_or(|c| p.grid_col == c))
            .map(|p| if row.is_some() { p.y.name.as_str() } else { p.x.name.as_str() })
    };
    let row_headers: Vec<String> = (0..frame.n_rows)
        .map(|r| {
            frame
                .row_headers
                .get(r)
                .filter(|h| axis_name(Some(r), None) != Some(h.as_str()))
                .cloned()
                .unwrap_or_default()
        })
        .collect();
    let column_headers: Vec<String> = (0..frame.n_cols)
        .map(|c| {
            frame
                .column_headers
                .get(c)
                .filter(|h| axis_name(None, Some(c)) != Some(h.as_str()))
                .cloned()
                .unwrap_or_default()
        })
        .collect();
    let has_row_headers = row_headers.iter().any(|h| !h.is_empty());
    let has_col_headers = column_headers.iter().any(|h| !h.is_empty());
    let has_titles = frame.panels.iter().any(|p| p.title.is_some());
    let header_h = if has_col_headers || has_titles { 18.0 } else { 0.0 };
    let legend_w = if spec.legend == LegendPlacement::Right && axes.show_key { LEGEND_WIDTH + PANEL_GAP } else { 0.0 };
    let usable_w = size.x - legend_w - if has_row_headers { ROW_HEADER_WIDTH } else { 0.0 };
    let panel_w = (usable_w / n_cols as f32 - PANEL_GAP).max(120.0);
    let panel_h = (size.y / n_rows as f32 - header_h).max(110.0);
    view.panels.resize_with(frame.panels.len(), PlotState::default);

    let placement = spec.legend;
    let show_key = axes.show_key && placement != LegendPlacement::Hidden;
    let mut draw_grid = |ui: &mut egui::Ui, rows: &mut RowStateManager| {
        let mut grid_rect: Option<egui::Rect> = None;
        for r in 0..n_rows {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = PANEL_GAP;
                for (i, panel) in frame.panels.iter().enumerate().filter(|(_, p)| p.grid_row == r) {
                    let header = match &panel.title {
                        Some(t) => t.clone(),
                        None if r == 0 => column_headers.get(panel.grid_col).cloned().unwrap_or_default(),
                        None => String::new(),
                    };
                    let rect = ui
                        .vertical(|ui| {
                            if header_h > 0.0 {
                                ui.allocate_ui_with_layout(
                                    egui::vec2(panel_w, header_h),
                                    egui::Layout::top_down(egui::Align::Center),
                                    |ui| {
                                        ui.set_width(panel_w);
                                        ui.add(egui::Label::new(RichText::new(header).strong().small()).truncate());
                                    },
                                );
                            }
                            // Named in the marker menu: its title, or its grid headers.
                            let name = panel.title.clone().unwrap_or_else(|| {
                                let parts: Vec<&str> =
                                    [frame.column_headers.get(panel.grid_col), frame.row_headers.get(panel.grid_row)]
                                        .into_iter()
                                        .flatten()
                                        .map(String::as_str)
                                        .filter(|h| !h.is_empty())
                                        .collect();
                                if parts.is_empty() { format!("panel {}", i + 1) } else { parts.join(" · ") }
                            });
                            let ctx = PanelCtx {
                                spec,
                                table,
                                panel_name: name,
                                n_panels: frame.panels.len(),
                                marker_target: &mut view.marker_target,
                                marker_all_panels: &mut view.marker_all_panels,
                                style_edit: &mut view.style_edit,
                                lasso: view.lasso,
                                lasso_path: &mut view.lasso_path,
                                label_drag: &mut view.label_drag,
                                label_dropped: &mut view.label_dropped,
                                notes: &mut view.notes,
                                view_x: &mut view.x_views,
                                view_y: &mut view.y_views,
                                canvas_axes: &mut view.canvas.axes,
                                canvas_reset: &mut view.canvas.reset_pending,
                                right: drawn.right.as_ref(),
                                y2_view: &mut view.y2_view,
                                right_drag: &mut view.right_drag,
                                ranges: &drawn.ranges,
                                axes: &axes,
                                info: &info,
                                id,
                                reset,
                                size: egui::vec2(panel_w, panel_h),
                                bottom: r + 1 == n_rows
                                    || !frame
                                        .panels
                                        .iter()
                                        .any(|q| q.grid_row == r + 1 && q.grid_col == panel.grid_col),
                                left: panel.grid_col == 0,
                                right_end: !frame
                                    .panels
                                    .iter()
                                    .any(|q| q.grid_row == panel.grid_row && q.grid_col > panel.grid_col),
                            };
                            panel_plot(
                                ui,
                                ctx,
                                i,
                                panel,
                                drawn.layers.get(i).map_or(&[][..], Vec::as_slice),
                                &mut view.panels[i],
                                rows,
                            )
                        })
                        .inner;
                    grid_rect = Some(grid_rect.map_or(rect, |g| g.union(rect)));
                }
                if has_row_headers {
                    let text = row_headers.get(r).cloned().unwrap_or_default();
                    ui.allocate_ui_with_layout(
                        egui::vec2(ROW_HEADER_WIDTH, panel_h + header_h),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.add(egui::Label::new(RichText::new(text).strong().small()).wrap());
                        },
                    );
                }
            });
        }
        if let Some(rect) = grid_rect {
            // Columns dragged over the panels can be dropped on them.
            // The rulers along the outer axes count as the panels' edges.
            let zones = egui::Rect::from_min_max(rect.min, rect.max + egui::vec2(0.0, AXIS_RULER_DEPTH));
            view.dropped = drop_zones::drop_zones(ui, zones, id).or(view.dropped.take());
            // The grip spans the panels; a key on the right keeps its own width.
            let before = view.canvas.size.width;
            size_grip(
                ui,
                &mut view.canvas.size,
                egui::vec2(size.x - legend_w, size.y),
                rect,
                egui::Id::new(("graph_size", id)),
            );
            if view.canvas.size.width != before {
                view.canvas.size.width = view.canvas.size.width.map(|w| w + legend_w);
            }
        }
    };
    if show_key && placement == LegendPlacement::Right {
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| draw_grid(ui, rows));
            ui.vertical(|ui| {
                ui.set_max_width(LEGEND_WIDTH);
                legend::legend(ui, true, spec, frame, &drawn.layers, rows, drawn.right.as_ref(), &mut view.key_editor);
            });
        });
    } else {
        draw_grid(ui, rows);
        if show_key {
            legend::legend(ui, false, spec, frame, &drawn.layers, rows, drawn.right.as_ref(), &mut view.key_editor);
        }
    }
    let mut notes: Vec<&str> = frame.notes.iter().map(String::as_str).collect();
    let right_notes = drawn
        .right
        .iter()
        .flat_map(|r| r.notes.iter().chain(r.layers.iter().flatten().flatten().flat_map(|l| l.notes.iter())));
    for n in drawn.layers.iter().flatten().flat_map(|l| l.notes.iter()).chain(right_notes) {
        if !notes.contains(&n.as_str()) {
            notes.push(n);
        }
    }
    for n in notes {
        ui.label(RichText::new(format!("Note: {n}")).weak().small());
    }
}

/// What one panel needs from the canvas.
struct PanelCtx<'a> {
    spec: &'a GraphSpec,
    table: &'a DataTable,
    /// How the panel is named in its marker menu.
    panel_name: String,
    n_panels: usize,
    marker_target: &'a mut Option<markers::MarkerTarget>,
    marker_all_panels: &'a mut bool,
    style_edit: &'a mut Option<stat_viz::MarkerStyle>,
    /// Select with a freehand outline.
    lasso: bool,
    lasso_path: &'a mut Option<(usize, Vec<[f64; 2]>)>,
    label_drag: &'a mut Option<LabelDrag>,
    label_dropped: &'a mut Option<(usize, String, [f64; 2])>,
    notes: &'a mut notes::Notes,
    view_x: &'a mut BTreeMap<String, (f64, f64)>,
    view_y: &'a mut BTreeMap<String, (f64, f64)>,
    canvas_axes: &'a mut app_model::axis_settings::PlotAxes,
    canvas_reset: &'a mut bool,
    /// The right axis, if the graph has one.
    right: Option<&'a stat_viz::RightAxis>,
    y2_view: &'a mut Option<(f64, f64)>,
    right_drag: &'a mut Option<RightDrag>,
    ranges: &'a SharedRanges,
    axes: &'a app_model::axis_settings::PlotAxes,
    info: &'a AxisInfo<'a>,
    id: u64,
    reset: bool,
    size: egui::Vec2,
    /// Lowest panel of its column (draws the horizontal axis title).
    bottom: bool,
    /// Leftmost panel of its row (draws the vertical axis title).
    left: bool,
    /// Rightmost panel of its row (draws the right axis's title).
    right_end: bool,
}

/// Grid marks at `ticks`, each strong enough to carry a label.
fn marks(ticks: Vec<f64>) -> Vec<GridMark> {
    ticks.into_iter().map(|value| GridMark { value, step_size: 1e6 }).collect()
}

/// Ticks of an axis that the shared spacer cannot place: level positions on a
/// categorical axis, nice data values on a transformed one, or each side of
/// an axis break (`brk`, drawn units) on its own.
fn special_ticks(
    axis: &AxisSpec,
    brk: Option<&stat_viz::BrokenAxis>,
    vertical: bool,
) -> Option<Box<dyn Fn(GridInput) -> Vec<GridMark>>> {
    if let Some(b) = brk.cloned() {
        return Some(plot_axes::broken_spacer(axis.clone(), b, vertical));
    }
    if axis.is_categorical() {
        let n = axis.levels().len();
        Some(Box::new(move |_| marks((1..=n).map(|k| k as f64).collect())))
    } else if axis.scale != AxisScale::Linear {
        let scale = axis.scale;
        Some(Box::new(move |input: GridInput| marks(scaled_ticks(scale, input.bounds.0, input.bounds.1))))
    } else {
        None
    }
}

/// One panel: its plot with linked axis rulers, brushing, and its layers.
/// Returns the screen rectangle the plot occupies.
#[allow(clippy::too_many_lines)]
fn panel_plot(
    ui: &mut egui::Ui,
    ctx: PanelCtx<'_>,
    index: usize,
    panel: &stat_viz::Panel,
    layers: &[Layer],
    state: &mut PlotState,
    rows: &mut RowStateManager,
) -> egui::Rect {
    let pal = theme::plot_palette(ui);
    let (xn, yn) = (panel.x.name.as_str(), panel.y.name.as_str());
    let (x_break, y_break) = ctx.spec.panel_breaks(panel);
    // Automatic ranges widened to the nearest major ticks when asked (plain
    // numeric axes only).
    let extend = |s: &app_model::axis_settings::AxisSettings, axis: &AxisSpec, broken: bool, r: (f64, f64), len: f32, gap| {
        if s.extend_to_ticks && !broken && !axis.is_categorical() && axis.scale == AxisScale::Linear {
            app_model::axis_settings::extended_to_ticks(s, r.0, r.1, f64::from(len), gap)
        } else {
            r
        }
    };
    let x_range = match ctx.view_x.get(xn) {
        Some(r) => *r,
        None => {
            let auto = ctx.ranges.x.get(xn).copied().unwrap_or((0.0, 1.0));
            extend(&ctx.axes.x, &panel.x, x_break.is_some(), auto, ctx.size.x, 90.0)
        }
    };
    let y_range = match ctx.view_y.get(yn) {
        Some(r) => *r,
        None => {
            let auto = ctx.ranges.y.get(yn).copied().unwrap_or((0.0, 1.0));
            extend(&ctx.axes.y, &panel.y, y_break.is_some(), auto, ctx.size.y, 45.0)
        }
    };
    let shown = ViewBounds { x_min: x_range.0, x_max: x_range.1, y_min: y_range.0, y_max: y_range.1 };
    state.bounds = Some(shown);
    state.axes = ctx.axes.clone();
    let markup = ctx.spec.axes.markup_labels;
    let right = ctx.right;
    let y2_range = right.map(|r| ctx.y2_view.unwrap_or(r.range));
    // Date columns get calendar ticks; the user's settings are otherwise kept.
    let mut timed = ctx.axes.clone();
    (timed.x.time, timed.y.time) = (panel.x.time, panel.y.time);
    // Round forms are drawn true to shape, with no numeric axes.
    let own_axes = ctx.spec.form.draws_own_axes() && !matches!(ctx.spec.form, GraphForm::Treemap(_));
    if ctx.spec.form.equal_aspect() {
        timed.equal_scale = true;
    }
    if own_axes {
        for a in [&mut timed.x, &mut timed.y] {
            (a.tick_labels, a.tick_marks, a.grid) = (false, false, app_model::axis_settings::GridLines::None);
        }
    }
    if right.is_some() {
        // The right side belongs to the second axis.
        timed.y.side = app_model::axis_settings::AxisSide::Standard;
    }
    // Broken axes draw their own tick marks, at both sides of the break.
    let mut marks_axes = timed.clone();
    marks_axes.x.tick_marks &= x_break.is_none();
    marks_axes.y.tick_marks &= y_break.is_none();
    let x_special = special_ticks(&panel.x, x_break.as_ref(), false);
    let categorical_x = x_special.is_some() && x_break.is_none();
    let special_x = x_special.is_some();
    let plot = interactive_plot!(("graph_panel", ctx.id, index), ctx.reset);
    // The right axis: room for its tick labels and title beside the plot
    // (egui_plot reserves it; the ticks are painted after, on its own scale).
    let chrome = if own_axes { PlotChrome::begin(ui).without_axes() } else { PlotChrome::begin(ui) };
    let tick_font = egui::TextStyle::Body.resolve(ui.style());
    let right_ticks: Vec<(f64, String)> = match (right, y2_range) {
        (Some(r), Some((lo, hi))) => {
            let target = ((f64::from(ctx.size.y) / 45.0).round() as usize).clamp(2, 12);
            stat_viz::export::axis_tick_labels(&r.axis, lo, hi, target)
        }
        _ => Vec::new(),
    };
    let plot = match right {
        Some(r) => {
            let widest = ui.fonts(|f| {
                right_ticks
                    .iter()
                    .map(|t| f.layout_no_wrap(t.1.clone(), tick_font.clone(), egui::Color32::WHITE).size().x)
                    .fold(0.0_f32, f32::max)
            });
            let title = if ctx.right_end {
                chrome.math_title(&ctx.axes.y2.title_text(&r.title, markup))
            } else {
                egui::WidgetText::default()
            };
            let hint = egui_plot::AxisHints::new_y()
                .placement(egui_plot::HPlacement::Right)
                .formatter(|_, _| String::new())
                .min_thickness(widest + 10.0)
                .label(title);
            plot.custom_y_axes(vec![egui_plot::AxisHints::new_y(), hint])
        }
        None => plot,
    };
    let mut plot = with_axes!(plot, &timed, special_x);
    if let Some(spacer) = x_special {
        let (axis, settings, brk) = (panel.x.clone(), timed.x.clone(), x_break.clone());
        plot = plot.x_grid_spacer(spacer).x_axis_formatter(move |mark, _| {
            let text = match &brk {
                Some(b) => b.label_at(&axis, mark.value),
                None => tick_label(&axis, mark.value),
            };
            plot_axes::pad_tick_label(text, &settings, false)
        });
    }
    if let Some(spacer) = special_ticks(&panel.y, y_break.as_ref(), true) {
        let (axis, settings, brk) = (panel.y.clone(), timed.y.clone(), y_break.clone());
        plot = plot.y_grid_spacer(spacer).y_axis_formatter(move |mark, _| {
            let text = match &brk {
                Some(b) => b.label_at(&axis, mark.value),
                None => tick_label(&axis, mark.value),
            };
            plot_axes::pad_tick_label(text, &settings, true)
        });
    }
    // Whole-panel forms draw their own axes (or none).
    plot = match ctx.spec.form {
        GraphForm::Treemap(_) => plot.show_axes([false, false]),
        GraphForm::Parallel => plot.show_axes([true, false]),
        form if form.draws_own_axes() => plot.show_axes([false, false]),
        _ => plot,
    };
    // Circles (chord diagrams, network nodes) stay round.
    if ctx.spec.form.equal_aspect() {
        plot = plot.data_aspect(1.0);
    }
    // Whole-panel forms have no grid; special vertical axes rule at their own ticks.
    let mut grid_axes = timed.clone();
    let whole = matches!(ctx.spec.form, GraphForm::Parallel) || ctx.spec.form.draws_own_axes();
    if whole {
        grid_axes.x.grid = app_model::axis_settings::GridLines::None;
        grid_axes.y.grid = app_model::axis_settings::GridLines::None;
    }
    let y_spacer = special_ticks(&panel.y, y_break.as_ref(), true);
    let x_label =
        if ctx.bottom { ctx.axes.x.title_text(&panel.x.title(), markup) } else { stat_viz::MathText::default() };
    let y_label = if ctx.left {
        ctx.axes.y.title_text(&ctx.spec.y_title(&panel.y), markup)
    } else {
        stat_viz::MathText::default()
    };
    let start = if ctx.lasso { None } else { state.brush_start };
    let outline: Option<Vec<[f64; 2]>> =
        ctx.lasso_path.as_ref().filter(|(i, _)| *i == index).map(|(_, path)| path.clone());
    // Statistics labels where the user put them (or where one is being dragged).
    let view_box = ([x_range.0, x_range.1], [y_range.0, y_range.1]);
    let mut placed = place_labels(layers, &ctx.spec.label_positions, index, view_box);
    if let Some(drag) = ctx.label_drag.as_ref().filter(|d| d.panel == index) {
        for mark in placed.iter_mut().flat_map(|l| l.marks.iter_mut()) {
            if let Mark::Text { at, movable: Some(id), .. } = mark {
                if *id == drag.id {
                    *at = drag.at;
                }
            }
        }
    }
    let (x_label, y_label) = (chrome.math_title(&x_label), chrome.math_title(&y_label));
    // The user's annotations, on top of everything.
    let place = ctx.spec.panel_place(panel, view_box);
    let note_layer = ctx.notes.layer(ctx.spec, index, &place);
    // Right-axis layers in the left axis's coordinates.
    let right_layers: Vec<Layer> = match (right, y2_range) {
        (Some(r), Some(r_range)) => r.panel_layers(index, y_range, r_range),
        _ => Vec::new(),
    };
    let response =
        plot.width(ctx.size.x).height(ctx.size.y).x_axis_label(x_label).y_axis_label(y_label).show(ui, |plot| {
            apply_bounds(plot, Some(shown));
            for layer in &placed {
                draw_layer_with(plot, layer, pal.dark, markup);
            }
            for layer in &right_layers {
                draw_layer_with(plot, layer, pal.dark, markup);
            }
            draw_layer_with(plot, &note_layer, pal.dark, true);
            draw_rubber_band(plot, start);
            if let Some(mut path) = outline {
                if let Some(first) = path.first().copied() {
                    path.push(first);
                }
                plot.line(Line::new(PlotPoints::from(path)).color(pal.selected).width(1.5_f32));
            }
        });
    let chrome = if whole { chrome.without_vertical_axis() } else { chrome };
    let chrome = if right.is_some() { chrome.with_right_axis() } else { chrome };
    chrome.finish(ui, &response.transform, &grid_axes, categorical_x, y_spacer.as_ref());
    axis_controls(ui, &response, state, egui::Id::new(("graph_panel_axes", ctx.id, index)), ctx.info);
    plot_axes::paint_tick_marks(ui, &response.transform, &marks_axes, categorical_x);
    plot_axes::paint_broken_tick_marks(
        ui,
        &response.transform,
        &timed,
        x_break.as_ref().map(|b| (&panel.x, b)),
        y_break.as_ref().map(|b| (&panel.y, b)),
    );
    plot_axes::paint_break_marks(ui, &response.transform, &timed, x_break.as_ref(), y_break.as_ref(), right.is_some());
    if let (Some(r), Some(r_range)) = (right, y2_range) {
        let to_left = stat_viz::RightAxis::to_left(y_range, r_range);
        paint_right_ticks(ui, &response.transform, &ctx.axes.y2, &right_ticks, &to_left, &tick_font, &pal);
        let ruler = RightRuler { panel: index, range: r_range, home: r.range };
        right_ruler(ui, &response, ruler, ctx.right_drag, ctx.y2_view, ctx.canvas_axes, ctx.id);
    }
    // Axis drags move this column's range in every panel showing it; only
    // the axes the drag changed are pinned, so the other still refits.
    if let Some(b) = state.bounds.filter(|b| *b != shown) {
        record_dragged_axes(b, shown, xn, yn, ctx.view_x, ctx.view_y);
    }
    // Annotations and labels take the pointer first (a double-click on one
    // edits it rather than re-fitting the view).
    let note_owned = ctx.notes.gesture(ui, &response, ctx.spec, index, &place, &placed, ctx.label_dropped);
    if note_owned {
        state.reset_pending = false;
    }
    ctx.notes.paint_selection(ui, &response.transform, ctx.spec, index, &place, &placed);
    if std::mem::take(&mut state.reset_pending) {
        *ctx.canvas_reset = true;
    }
    if state.axes != *ctx.axes {
        *ctx.canvas_axes = state.axes.clone();
    }
    // Right-click: the settings of the marker under the pointer.
    if response.response.secondary_clicked() {
        let row = response.response.interact_pointer_pos().and_then(|pos| {
            let screen = ctx.spec.row_positions(panel).into_iter().map(|(row, p)| {
                let s = response.transform.position_from_point(&PlotPoint::new(p[0], p[1]));
                (row, [s.x, s.y])
            });
            nearest_row(screen, [pos.x, pos.y], MARKER_REACH)
        });
        *ctx.marker_target = Some(markers::MarkerTarget { panel: index, row });
    }
    response.response.context_menu(|ui| {
        ctx.notes.menu(ui, ctx.spec, index, &place);
        let target =
            ctx.marker_target.filter(|t| t.panel == index).unwrap_or(markers::MarkerTarget { panel: index, row: None });
        let mut style = ctx.style_edit.clone().unwrap_or_else(|| ctx.spec.options.style.clone());
        let before = style.clone();
        markers::marker_context_menu(
            ui,
            ctx.spec,
            ctx.table,
            panel,
            &ctx.panel_name,
            ctx.n_panels,
            target,
            ctx.marker_all_panels,
            &mut style,
        );
        if style != before {
            *ctx.style_edit = Some(style);
        }
        if ctx.spec.form == GraphForm::Panels {
            ui.separator();
            if ui
                .button(crate::reference_lines_ui::OPEN_LABEL)
                .on_hover_text(crate::reference_lines_ui::OPEN_HINT)
                .clicked()
            {
                ui.ctx().data_mut(|d| d.insert_temp(reference_open_id(ctx.id), true));
                ui.close_menu();
            }
        }
        ui.separator();
        crate::plot_style_menu::canvas_menu_items(ui, ctx.id);
    });
    if note_owned || label_gesture(ui, &response, &placed, index, view_box, ctx.label_drag, ctx.label_dropped) {
        state.brush_start = None;
        return response.response.rect;
    }
    // Rows drawn on either vertical axis, in the left axis's coordinates.
    let mut positions = ctx.spec.row_positions(panel);
    if let (Some(r), Some(r_range)) = (right, y2_range) {
        positions.extend(r.row_positions(ctx.spec, index, y_range, r_range));
    }
    let all_layers: Vec<Layer> = layers.iter().cloned().chain(right_layers).collect();
    point_tooltip(ui, &response, &positions, ctx.table, [xn, yn], rows);
    if ctx.lasso {
        lasso_gesture(
            ui,
            &response,
            &positions,
            &all_layers,
            ctx.spec.form.selects_by_region(),
            index,
            ctx.lasso_path,
            rows,
        );
        return response.response.rect;
    }
    match plot_gesture(ui, &response, state) {
        Some((Gesture::Region(region), mode)) => {
            let inside = rows_in_box(&positions, [region.x_min, region.y_min], [region.x_max, region.y_max]);
            rows.select(inside, mode);
        }
        Some((Gesture::Click(pos), mode)) => {
            click_select(
                pos,
                &response.transform,
                &positions,
                &all_layers,
                ctx.spec.form.selects_by_region(),
                mode,
                rows,
            );
        }
        None => {}
    }
    response.response.rect
}

/// Pin the ranges a drag changed: `bounds` against what was drawn
/// (`shown`), each axis separately, keyed by its column (`xn`, `yn`).
fn record_dragged_axes(
    bounds: ViewBounds,
    shown: ViewBounds,
    xn: &str,
    yn: &str,
    view_x: &mut BTreeMap<String, (f64, f64)>,
    view_y: &mut BTreeMap<String, (f64, f64)>,
) {
    if (bounds.x_min, bounds.x_max) != (shown.x_min, shown.x_max) {
        view_x.insert(xn.to_owned(), (bounds.x_min, bounds.x_max));
    }
    if (bounds.y_min, bounds.y_max) != (shown.y_min, shown.y_max) {
        view_y.insert(yn.to_owned(), (bounds.y_min, bounds.y_max));
    }
}


/// The right axis's tick marks (inside the frame, like the left axis's) and
/// labels (in the room egui_plot reserved right of the frame), at the right
/// scale's own nice values mapped onto the panel.
fn paint_right_ticks(
    ui: &egui::Ui,
    transform: &egui_plot::PlotTransform,
    settings: &app_model::axis_settings::AxisSettings,
    ticks: &[(f64, String)],
    to_left: &dyn Fn(f64) -> f64,
    font: &egui::FontId,
    pal: &theme::PlotPalette,
) {
    let frame = *transform.frame();
    let painter = ui.painter_at(frame.expand2(egui::vec2(200.0, 8.0)));
    let stroke = egui::Stroke::new(1.0_f32, pal.axis_spine);
    for (t, label) in ticks {
        let y = transform.position_from_point_y(to_left(*t));
        if y < frame.top() - 0.5 || y > frame.bottom() + 0.5 {
            continue;
        }
        if settings.tick_marks {
            painter.line_segment([egui::pos2(frame.right() - 5.0, y), egui::pos2(frame.right(), y)], stroke);
        }
        if settings.tick_labels {
            painter.text(
                egui::pos2(frame.right() + 5.0, y),
                egui::Align2::LEFT_CENTER,
                label,
                font.clone(),
                pal.tick_text,
            );
        }
    }
}

/// Where a panel's right-axis ruler is and what it shows.
struct RightRuler {
    panel: usize,
    /// The range shown now.
    range: (f64, f64),
    /// The default range (what a reset returns to).
    home: (f64, f64),
}

/// The right axis as a draggable ruler, like the left one: near either end
/// a drag moves that limit only (the grabbed value follows the pointer),
/// elsewhere it shifts the axis; right-clicking opens its settings. The
/// range is shared by every panel.
fn right_ruler(
    ui: &egui::Ui,
    response: &egui_plot::PlotResponse<()>,
    ruler: RightRuler,
    drag: &mut Option<RightDrag>,
    y2_view: &mut Option<(f64, f64)>,
    axes: &mut app_model::axis_settings::PlotAxes,
    id: u64,
) {
    let frame = *response.transform.frame();
    let strip =
        egui::Rect::from_min_max(frame.right_top(), egui::pos2(frame.right() + super::Y_AXIS_STRIP, frame.bottom()));
    let resp = ui.interact(strip, egui::Id::new(("graph_right_axis", id, ruler.panel)), egui::Sense::click_and_drag());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, "Right axis ruler"));
    resp.context_menu(|ui| plot_axes::right_axis_menu(ui, axes));
    let length = f64::from(frame.height());
    let along = |p: egui::Pos2| f64::from(frame.bottom() - p.y);
    if resp.drag_started() {
        if let Some(p) = ui.input(|i| i.pointer.press_origin()) {
            let start_along = along(p);
            *drag = Some(RightDrag {
                panel: ruler.panel,
                handle: handle_at(start_along, length),
                start: ruler.range,
                start_along,
                length,
            });
        }
    }
    let active = drag.filter(|d| d.panel == ruler.panel);
    if resp.dragged() {
        if let (Some(d), Some(p)) = (active, resp.interact_pointer_pos()) {
            *y2_view = Some(drag_range(d.start.0, d.start.1, d.length, d.handle, d.start_along, along(p)));
            ui.ctx().request_repaint();
        }
    }
    if resp.drag_stopped() {
        *drag = None;
    }
    if resp.double_clicked() {
        *y2_view = None;
    }
    let _ = ruler.home;
    let hovered = resp.hover_pos().map(|p| handle_at(along(p), length));
    let shown = active.map(|d| d.handle).or(hovered);
    if let Some(handle) = shown {
        ui.ctx().set_cursor_icon(match (handle, active.is_some()) {
            (AxisHandle::Shift, true) => egui::CursorIcon::Grabbing,
            (AxisHandle::Shift, false) => egui::CursorIcon::Grab,
            _ => egui::CursorIcon::ResizeVertical,
        });
    }
    super::paint_axis_handles(ui, strip, app_model::plot_view::Axis::Y, shown, true);
}

/// How far (screen points) from a marker's centre a right-click still picks it.
const MARKER_REACH: f32 = 14.0;

/// Screen boxes of the movable labels in `layers`: identifier, box, anchor
/// point (plot coordinates).
fn label_boxes(
    ui: &egui::Ui,
    transform: &egui_plot::PlotTransform,
    layers: &[Layer],
) -> Vec<(String, egui::Rect, [f64; 2])> {
    // egui_plot writes text in the small text style.
    let font = egui::TextStyle::Small.resolve(ui.style());
    layers
        .iter()
        .flat_map(|l| l.marks.iter())
        .filter_map(|m| match m {
            Mark::Text { at, text, anchor, movable: Some(id), .. } => {
                let size = ui.fonts(|f| f.layout_no_wrap(text.clone(), font.clone(), egui::Color32::WHITE)).size();
                let point = transform.position_from_point(&PlotPoint::new(at[0], at[1]));
                Some((id.clone(), text_align(*anchor).anchor_size(point, size), *at))
            }
            _ => None,
        })
        .collect()
}

/// Dragging a statistics label moves it; dropping it records where (as a
/// fraction of the panel's view, so it stays put on the plot when the axes
/// move). Returns `true` while a label drag owns the pointer, so the drag
/// does not also select rows.
fn label_gesture(
    ui: &egui::Ui,
    response: &egui_plot::PlotResponse<()>,
    layers: &[Layer],
    index: usize,
    view: ([f64; 2], [f64; 2]),
    drag: &mut Option<LabelDrag>,
    dropped: &mut Option<(usize, String, [f64; 2])>,
) -> bool {
    let r = &response.response;
    let transform = &response.transform;
    let boxes = label_boxes(ui, transform, layers);
    let under = |p: egui::Pos2| boxes.iter().rev().find(|(_, rect, _)| rect.expand(3.0).contains(p));
    if r.drag_started_by(PointerButton::Primary) {
        if let Some((origin, (id, _, at))) =
            ui.input(|i| i.pointer.press_origin()).and_then(|o| under(o).map(|b| (o, b)))
        {
            let anchor = transform.position_from_point(&PlotPoint::new(at[0], at[1]));
            *drag = Some(LabelDrag { panel: index, id: id.clone(), grab: origin - anchor, at: *at });
        }
    }
    let Some(d) = drag.as_mut().filter(|d| d.panel == index) else {
        if r.hover_pos().is_some_and(|p| under(p).is_some()) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        return false;
    };
    if let Some(p) = r.interact_pointer_pos() {
        let v = transform.value_from_position(p - d.grab);
        d.at = [v.x, v.y];
    }
    ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    let released = r.drag_stopped() || !ui.input(|i| i.pointer.primary_down());
    if released {
        *dropped = Some((index, d.id.clone(), view_fraction(d.at, view)));
        *drag = None;
    } else {
        ui.ctx().request_repaint();
    }
    true
}

/// Freehand selection: dragging draws an outline (kept in plot coordinates);
/// releasing selects the rows drawn inside it. A click selects the nearest
/// row as in box mode.
#[allow(clippy::too_many_arguments)]
fn lasso_gesture(
    ui: &egui::Ui,
    response: &egui_plot::PlotResponse<()>,
    positions: &[(usize, [f64; 2])],
    layers: &[Layer],
    region_first: bool,
    index: usize,
    path: &mut Option<(usize, Vec<[f64; 2]>)>,
    rows: &mut RowStateManager,
) {
    let modifiers = ui.input(|i| i.modifiers);
    let mode = selection_mode(modifiers.shift, modifiers.command, modifiers.alt);
    let r = &response.response;
    let transform = &response.transform;
    let at = |p: egui::Pos2| {
        let v = transform.value_from_position(p);
        [v.x, v.y]
    };
    if r.drag_started_by(PointerButton::Primary) {
        if let Some(origin) = ui.input(|i| i.pointer.press_origin()) {
            *path = Some((index, vec![at(origin)]));
        }
    } else if r.dragged_by(PointerButton::Primary) {
        if let (Some((i, points)), Some(p)) = (path.as_mut(), r.interact_pointer_pos()) {
            // Add a vertex once the pointer has moved a few points.
            let far = points.last().is_none_or(|last| {
                let s = transform.position_from_point(&PlotPoint::new(last[0], last[1]));
                s.distance(p) > 3.0
            });
            if *i == index && far {
                points.push(at(p));
            }
            ui.ctx().request_repaint();
        }
    } else if r.drag_stopped_by(PointerButton::Primary) {
        if let Some((i, points)) = path.take() {
            if i == index {
                rows.select(rows_in_polygon(positions, &points), mode);
            }
        }
    } else if r.clicked_by(PointerButton::Primary) {
        if let Some(pos) = r.interact_pointer_pos() {
            click_select(pos, transform, positions, layers, region_first, mode, rows);
        }
    }
}

/// The tooltip of the point under the pointer: its row number, its values
/// of the panel's two columns, and the columns shown on hover.
fn point_tooltip(
    ui: &egui::Ui,
    response: &egui_plot::PlotResponse<()>,
    positions: &[(usize, [f64; 2])],
    table: &DataTable,
    columns: [&str; 2],
    rows: &RowStateManager,
) {
    if ui.input(|i| i.pointer.any_down()) {
        return;
    }
    let Some(pos) = response.response.hover_pos() else { return };
    let screen = positions.iter().map(|&(row, p)| {
        let s = response.transform.position_from_point(&PlotPoint::new(p[0], p[1]));
        (row, [s.x, s.y])
    });
    let Some(row) = nearest_row(screen, [pos.x, pos.y], CLICK_RADIUS).filter(|&r| r < table.n_rows()) else {
        return;
    };
    let mut text = format!("Row {}", row + 1);
    let mut named: Vec<&str> = Vec::new();
    for name in columns {
        if name.is_empty() || named.contains(&name) {
            continue;
        }
        named.push(name);
        if let Some(c) = table.find(name).and_then(|j| table.column(j)) {
            let v = c.level_text(row);
            text.push_str(&format!("\n{name}: {}", if v.is_empty() { "missing" } else { v.as_str() }));
        }
    }
    text.push_str(&crate::report_view::hover_lines(table, row));
    if row < rows.len() && rows.is_excluded(row) {
        text.push_str("\nExcluded from analyses");
    }
    egui::show_tooltip_at_pointer(ui.ctx(), ui.layer_id(), response.response.id.with("point"), |ui| {
        ui.label(text);
    });
}

/// A click: the nearest drawn row within reach; otherwise the rows of the
/// bar, cell or tile under the pointer; otherwise (plain click) nothing.
fn click_select(
    pos: egui::Pos2,
    transform: &egui_plot::PlotTransform,
    positions: &[(usize, [f64; 2])],
    layers: &[Layer],
    region_first: bool,
    mode: stat_data::SelectionMode,
    rows: &mut RowStateManager,
) {
    let screen = positions.iter().map(|&(row, p)| {
        let s = transform.position_from_point(&PlotPoint::new(p[0], p[1]));
        (row, [s.x, s.y])
    });
    let at = transform.value_from_position(pos);
    if region_first && let Some(region) = region_rows(layers, [at.x, at.y]) {
        rows.select(region, mode);
        return;
    }
    let hit = nearest_row(screen, [pos.x, pos.y], CLICK_RADIUS);
    if hit.is_none() {
        if let Some(region) = region_rows(layers, [at.x, at.y]) {
            rows.select(region, mode);
            return;
        }
    }
    brush_click(hit, mode, rows);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drag_of_one_axis_pins_only_that_column() {
        let shown = ViewBounds { x_min: 0.0, x_max: 10.0, y_min: 0.0, y_max: 5.0 };
        let dragged = ViewBounds { y_max: 8.0, ..shown };
        let (mut view_x, mut view_y) = (BTreeMap::new(), BTreeMap::new());
        record_dragged_axes(dragged, shown, "Height", "Weight", &mut view_x, &mut view_y);
        assert!(view_x.is_empty(), "the horizontal axis still refits: {view_x:?}");
        assert_eq!(view_y.get("Weight"), Some(&(0.0, 8.0)));
        // A drag at equal scale changes both, so both are kept.
        let both = ViewBounds { x_min: -3.0, x_max: 13.0, ..dragged };
        record_dragged_axes(both, shown, "Height", "Weight", &mut view_x, &mut view_y);
        assert_eq!(view_x.get("Height"), Some(&(-3.0, 13.0)));
    }

    fn drawn_with(spec: GraphSpec) -> Drawn {
        Drawn {
            key: (0, 0, 0),
            spec,
            frame: Err(String::new()),
            layers: Vec::new(),
            ranges: SharedRanges::default(),
            right: None,
        }
    }

    /// Dragged ranges are in drawn units: a new axis scale or chart form
    /// drops them, while other changes keep them.
    #[test]
    fn scale_change_forgets_dragged_ranges() {
        let mut view = GraphView::default();
        let spec = GraphSpec::default();
        view.drawn = Some(drawn_with(spec.clone()));
        view.x_views.insert("x".to_owned(), (0.0, 1000.0));
        view.y_views.insert("y".to_owned(), (5.0, 6.0));
        let mut same = spec.clone();
        view.forget_stale_ranges(&mut same);
        assert_eq!(view.x_views.len(), 1, "unchanged graph keeps its ranges");
        let mut log = spec.clone();
        log.options.x_scale = AxisScale::Log10;
        // A typed range is kept with the graph's style; it goes too.
        let mut style = app_model::plot_style::PlotStyle::default();
        style.axes.x.range = Some([0.0, 1000.0]);
        log.axes.style = Some(style);
        view.forget_stale_ranges(&mut log);
        assert!(view.x_views.is_empty() && view.y_views.is_empty());
        assert_eq!(log.axes.style, None, "a style holding only the range is dropped");
        view.drawn = Some(drawn_with(log.clone()));
        view.y_views.insert("y".to_owned(), (5.0, 6.0));
        let mut treemap = log;
        treemap.form = GraphForm::Treemap(stat_viz::TreemapSettings { size_by_sum: false, labels: true });
        view.forget_stale_ranges(&mut treemap);
        assert!(view.y_views.is_empty());
    }
}
