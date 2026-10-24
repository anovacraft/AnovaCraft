//! Dropping a column on a report plot's axis: when the axis shows the
//! report's switchable column (a model's response, a distribution's or a
//! chart's column; see [`app_model::SavedReport::axis_drop`]), a column
//! dragged from the variables list highlights the axis and, let go there,
//! re-runs the report with that column in its place, as the column
//! switcher would.
//!
//! While a report is drawn its drop target sits in the context's temporary
//! data, where the shared axis code ([`axis_drop_zone`]) finds it; a drop
//! is left there for the report to take after drawing.

use eframe::egui;
use stat_data::DataTable;

use crate::report_view::{ColumnDrag, Report};
use crate::theme;

/// The report being drawn, as a target for columns dropped on its axes.
#[derive(Debug, Clone)]
struct Target {
    report: u64,
    /// Name of the column now in the report's switch slot.
    column: String,
    /// Columns that can take its place, by name.
    candidates: Vec<(String, usize)>,
}

fn target_id() -> egui::Id {
    egui::Id::new("report_axis_drop_target")
}

fn dropped_id(report: u64) -> egui::Id {
    egui::Id::new(("report_axis_dropped", report))
}

/// Make `report` the target of axis drops while it is drawn (only while a
/// column is being dragged, so nothing is computed otherwise).
pub fn begin(ctx: &egui::Context, report: &Report, table: &DataTable) {
    let target = egui::DragAndDrop::has_payload_of_type::<ColumnDrag>(ctx)
        .then(|| {
            let saved = report.saved();
            let slot = saved.switch_slot()?;
            let column = table.column(slot.column)?.name.clone();
            let candidates = table
                .columns()
                .iter()
                .filter_map(|c| saved.axis_drop(table, &column, &c.name).map(|i| (c.name.clone(), i)))
                .collect();
            Some(Target { report: report.id(), column, candidates })
        })
        .flatten();
    ctx.data_mut(|d| d.insert_temp(target_id(), target));
}

/// Stop taking axis drops for the report drawn, and return the column
/// dropped on one of its axes, if any.
pub fn end(ctx: &egui::Context, report: u64) -> Option<usize> {
    ctx.data_mut(|d| {
        d.remove::<Option<Target>>(target_id());
        d.remove_temp::<usize>(dropped_id(report))
    })
}

/// The drop zone of one axis: its ruler `strip`, which shows the column
/// named `title`. While a column is dragged over an axis showing the
/// report's switchable column, the strip is outlined and a note beside the
/// pointer says what letting go does (or why the column cannot go there).
pub fn axis_drop_zone(ui: &egui::Ui, strip: egui::Rect, title: &str) {
    let Some(drag) = egui::DragAndDrop::payload::<ColumnDrag>(ui.ctx()) else { return };
    let Some(target) = ui.ctx().data(|d| d.get_temp::<Option<Target>>(target_id())).flatten() else { return };
    if title.trim() != target.column {
        return;
    }
    let Some(pos) = ui.ctx().pointer_latest_pos().filter(|p| ui.clip_rect().contains(*p)) else { return };
    let over = strip.contains(pos);
    let to = target.candidates.iter().find(|(n, _)| *n == drag.name).map(|(_, i)| *i);
    let painter = ui
        .ctx()
        .layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("report_axis_drop")))
        .with_clip_rect(ui.clip_rect());
    let (accent, note) = match to {
        Some(_) => (theme::DIVIDER_ACTIVE, format!("Analyse {} instead of {}", drag.name, target.column)),
        None if drag.name == target.column => {
            (ui.visuals().weak_text_color(), format!("{} is shown already", drag.name))
        }
        None => (ui.visuals().weak_text_color(), format!("{} cannot take the place of {}", drag.name, target.column)),
    };
    let fill = if over { accent.gamma_multiply(0.30) } else { accent.gamma_multiply(0.08) };
    painter.rect(
        strip,
        3.0,
        fill,
        egui::Stroke::new(if over { 2.0_f32 } else { 1.0_f32 }, accent),
        egui::StrokeKind::Inside,
    );
    if !over {
        ui.ctx().request_repaint();
        return;
    }
    let galley = painter.layout_no_wrap(note, egui::FontId::proportional(12.0), ui.visuals().strong_text_color());
    let at = pos + egui::vec2(14.0, 14.0);
    let back = egui::Rect::from_min_size(at, galley.size()).expand2(egui::vec2(5.0, 3.0));
    painter.rect(back, 3.0, ui.visuals().panel_fill, egui::Stroke::new(1.0_f32, accent), egui::StrokeKind::Outside);
    painter.galley(at, galley, ui.visuals().strong_text_color());
    if let Some(column) = to.filter(|_| ui.input(|i| i.pointer.any_released())) {
        egui::DragAndDrop::clear_payload(ui.ctx());
        ui.ctx().data_mut(|d| d.insert_temp(dropped_id(target.report), column));
    }
    ui.ctx().request_repaint();
}
