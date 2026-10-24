//! The size of a plot and the grip that changes it.
//!
//! Every plot in a report opens at a default size and can be resized by
//! dragging: a strip under it changes the height, a strip along its right
//! edge changes the width, and the corner where they meet changes both.
//! Double-clicking either restores the default. The width never exceeds the
//! visible part of the pane, so the whole plot stays in view.
//!
//! A row of small plots (the panels of a Response Explorer) shares one grip:
//! dragging it resizes every panel together, by the same amount, so the
//! grabbed corner follows the pointer whatever the number of panels.

use eframe::egui;

use crate::theme;

/// Size chosen with a plot's resize grip, kept across frames.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct PlotSize {
    /// Height chosen with the grip (`None`: the plot's default).
    pub(crate) height: Option<f32>,
    /// Width chosen with the grip (`None`: the default width). Never more
    /// than the visible width, so the whole plot stays in view.
    pub(crate) width: Option<f32>,
    /// The current grip drag started on the corner (resizing width too).
    grip_corner: bool,
}

impl PlotSize {
    /// Size to draw a plot at: the user's grip size, capped to the visible
    /// width; full visible width and `default_height` until resized.
    pub(crate) fn full(&self, ui: &egui::Ui, default_height: f32) -> egui::Vec2 {
        let visible = visible_plot_width(ui);
        let width = self.width.map_or(visible, |w| w.min(visible)).max(MIN_PLOT_WIDTH);
        egui::vec2(width, self.height.unwrap_or(default_height))
    }

    /// [`PlotSize::full`] for x–y plots and other drawings that need no
    /// more than a moderate width: until the user sets a width, at most
    /// [`MAX_DEFAULT_PLOT_WIDTH`] wide (more with larger text). Charts read
    /// along a long sequence (control charts, Pareto charts) keep the full
    /// width.
    pub(crate) fn capped(&self, ui: &egui::Ui, default_height: f32) -> egui::Vec2 {
        let mut size = self.full(ui, default_height);
        if self.width.is_none() {
            size.x = size.x.min(MAX_DEFAULT_PLOT_WIDTH * theme::text_scale(ui));
        }
        size
    }

    /// Back to the default size.
    pub(crate) fn reset(&mut self) {
        self.height = None;
        self.width = None;
    }

    /// The size remembered under `id` (default until set), for views that
    /// keep no state of their own across frames.
    pub(crate) fn load(ctx: &egui::Context, id: egui::Id) -> Self {
        ctx.data(|d| d.get_temp(id)).unwrap_or_default()
    }

    /// Remember this size under `id` (see [`PlotSize::load`]).
    pub(crate) fn store(self, ctx: &egui::Context, id: egui::Id) {
        ctx.data_mut(|d| d.insert_temp(id, self));
    }
}

/// Narrowest a plot is drawn.
pub(crate) const MIN_PLOT_WIDTH: f32 = 180.0;
/// Shortest and tallest a plot can be made with its grip.
pub(crate) const PLOT_HEIGHT_RANGE: std::ops::RangeInclusive<f32> = 100.0..=1400.0;
/// Widest a plot opens before the user widens it (see
/// [`PlotSize::capped`]), at the standard text size, so a wide pane does
/// not flatten it into a strip.
pub(crate) const MAX_DEFAULT_PLOT_WIDTH: f32 = 760.0;
/// Width of the drag handle on a plot's right edge.
const EDGE_GRIP: f32 = 8.0;

/// Key of the report pane's `(content left edge, viewport width)` for this frame.
pub(crate) fn report_frame_id() -> egui::Id {
    egui::Id::new("report_pane_frame")
}

/// Width available to a plot without leaving the visible part of the pane:
/// from the layout cursor to the right edge of the clip rectangle (the scroll
/// viewport), whatever width other content has stretched the layout to.
pub(crate) fn visible_plot_width(ui: &egui::Ui) -> f32 {
    // The pane's vertical scroll bar is drawn over the right edge of the
    // content, and the clip rectangle extends a few points past the viewport:
    // stay clear of both, with room for the plot's own width handle.
    let bar = ui.spacing().scroll;
    let reserve =
        bar.bar_width + bar.bar_inner_margin + bar.bar_outer_margin + ui.visuals().clip_rect_margin + EDGE_GRIP + 6.0;
    // Measured from the content's left edge (which moves with horizontal
    // scrolling) rather than the screen, so scrolling never resizes a plot.
    let available = match ui.ctx().data(|d| d.get_temp::<(f32, f32)>(report_frame_id())) {
        Some((content_left, viewport_width)) => viewport_width - (ui.cursor().left() - content_left),
        None => ui.clip_rect().right() - ui.cursor().left(),
    };
    (available - reserve).max(MIN_PLOT_WIDTH)
}

/// What a resize grip frames and how its drags map to the chosen size.
pub(crate) struct GripFrame {
    /// The size of one plot (one panel of a row) as drawn this frame.
    pub(crate) size: egui::Vec2,
    /// The area the handles frame: the plot, or every panel together. The
    /// width handle runs down its right edge; the height strip is laid out
    /// under it (at the layout cursor), as wide as the area.
    pub(crate) area: egui::Rect,
    /// Panels across and down in the area, among which a drag is shared
    /// (`[1, 1]` for a single plot).
    pub(crate) panels: [usize; 2],
    /// Narrowest a plot (or panel) can be made.
    pub(crate) min_width: f32,
    /// What is resized, for the accessible names and tooltips: "plot" or
    /// "panels".
    pub(crate) what: &'static str,
}

/// Resize handles of a plot (whose widget occupies `plot_rect`, drawn at
/// `size`): a strip under it changes the height, a strip along its right
/// edge changes the width, and the corner where they meet changes both.
/// Width never exceeds the visible pane, so the whole plot stays in view.
/// Double-click either to restore the default size.
pub(crate) fn size_grip(
    ui: &mut egui::Ui,
    state: &mut PlotSize,
    size: egui::Vec2,
    plot_rect: egui::Rect,
    id: egui::Id,
) {
    // The height strip spans the plot's drawn width (which can differ from
    // its widget's rectangle, for example when a key sits beside it).
    let area = egui::Rect::from_min_size(plot_rect.min, egui::vec2(size.x, plot_rect.height()));
    let edge = plot_rect.right() - area.right();
    grip(ui, state, id, &GripFrame { size, area, panels: [1, 1], min_width: MIN_PLOT_WIDTH, what: "plot" }, edge);
}

/// [`size_grip`] for any [`GripFrame`]: a single plot, or a block of panels
/// that share one size (a drag of `d` points changes each panel by `d`
/// divided by the panels in that direction, so the grabbed corner follows
/// the pointer).
pub(crate) fn shared_grip(ui: &mut egui::Ui, state: &mut PlotSize, id: egui::Id, frame: &GripFrame) {
    grip(ui, state, id, frame, 0.0);
}

/// The handles; the width strip sits `edge_shift` points right of the area.
fn grip(ui: &mut egui::Ui, state: &mut PlotSize, id: egui::Id, frame: &GripFrame, edge_shift: f32) {
    let GripFrame { size, area, panels, min_width, what } = *frame;
    let across = panels[0].max(1) as f32;
    let down = panels[1].max(1) as f32;
    let pal = theme::plot_palette(ui);
    let visible = visible_plot_width(ui);
    let set_width = |state: &mut PlotSize, w: f32| state.width = Some(w.clamp(min_width, visible.max(min_width)));
    let set_height = |state: &mut PlotSize, h: f32| {
        state.height = Some(h.clamp(*PLOT_HEIGHT_RANGE.start(), *PLOT_HEIGHT_RANGE.end()))
    };

    // Right edge: registered after the plot, so it wins presses on the edge.
    let right = area.right() + edge_shift;
    let edge = egui::Rect::from_x_y_ranges(right - 2.0..=right + EDGE_GRIP, area.y_range());
    let edge_response = ui.interact(edge, id.with("width_grip"), egui::Sense::click_and_drag());
    edge_response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, format!("Width of {what}")));
    if edge_response.dragged() {
        set_width(state, size.x + edge_response.drag_delta().x / across);
    }
    let edge_active = edge_response.hovered() || edge_response.dragged();

    // Bottom strip; its right corner resizes both ways.
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(area.width() + EDGE_GRIP, 10.0), egui::Sense::click_and_drag());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, format!("Size of {what}")));
    let corner = egui::Rect::from_min_max(egui::pos2(rect.right() - 24.0, rect.top()), rect.right_bottom());
    if response.drag_started() {
        state.grip_corner = ui.input(|i| i.pointer.press_origin()).is_some_and(|p| corner.contains(p));
    }
    if response.dragged() {
        let d = response.drag_delta();
        set_height(state, size.y + d.y / down);
        if state.grip_corner {
            set_width(state, size.x + d.x / across);
        }
    }
    if response.double_clicked() || edge_response.double_clicked() {
        state.reset();
    }
    let on_corner =
        if response.dragged() { state.grip_corner } else { response.hover_pos().is_some_and(|p| corner.contains(p)) };
    let bottom_active = response.hovered() || response.dragged();
    if edge_active {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    } else if bottom_active {
        ui.ctx().set_cursor_icon(if on_corner {
            egui::CursorIcon::ResizeNwSe
        } else {
            egui::CursorIcon::ResizeVertical
        });
    }

    let rest = ui.visuals().weak_text_color().gamma_multiply(0.4);
    let painter = ui.painter();
    // Width handle: a faint bar with grip dots, mid-height on the right edge.
    let edge_color = if edge_active { pal.point } else { rest };
    let x = right + EDGE_GRIP * 0.5;
    painter.vline(
        x,
        area.y_range().shrink(area.height() * 0.3),
        egui::Stroke::new(1.0_f32, edge_color.gamma_multiply(0.6)),
    );
    for dy in [-8.0, 0.0, 8.0] {
        painter.circle_filled(egui::pos2(x, area.center().y + dy), 1.5, edge_color);
    }
    // Height handle and corner.
    let color = if bottom_active { pal.point } else { rest };
    let y = rect.center().y;
    painter.hline(rect.x_range().shrink(rect.width() * 0.3), y, egui::Stroke::new(1.0_f32, color.gamma_multiply(0.6)));
    for dx in [-8.0, 0.0, 8.0] {
        painter.circle_filled(egui::pos2(rect.center().x + dx, y), 1.5, color);
    }
    let corner_color = if bottom_active && on_corner { pal.point } else { rest };
    for k in [4.0, 8.0, 12.0] {
        painter.line_segment(
            [egui::pos2(rect.right() - k, rect.bottom() - 1.0), egui::pos2(rect.right() - 1.0, rect.bottom() - k)],
            egui::Stroke::new(1.0_f32, corner_color),
        );
    }
    let (one, both) = if panels == [1, 1] { ("the plot's", "") } else { ("every panel's", " together") };
    edge_response.on_hover_text(format!("Drag to change {one} width{both}; double-click to restore"));
    response
        .on_hover_text(format!("Drag to change {one} height{both} (the corner changes both); double-click to restore"));
}
