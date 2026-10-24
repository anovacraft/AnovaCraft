//! The column shelves: one drop zone per visual channel. Assigned columns are
//! chips that can be dragged to another shelf, removed with ×, or (from their
//! context menu) given a column picker.

use eframe::egui::{self, RichText};
use stat_data::DataTable;
use stat_viz::{ColumnSwitcher, GraphSpec, Role};

use super::ColumnDrag;
use crate::theme;

/// Shelf label and tooltip of each role.
pub(super) fn shelf(role: Role) -> (&'static str, &'static str) {
    match role {
        Role::X => ("Horizontal axis", "Columns along the horizontal axis; several give side-by-side panels"),
        Role::Y => ("Vertical axis", "Columns up the vertical axis; several give stacked panels"),
        Role::GroupX => ("Split across", "One column of panels per level of this column"),
        Role::GroupY => ("Split down", "One row of panels per level of this column"),
        Role::Wrap => ("Wrap panels by", "One panel per level, wrapped into a grid (instead of the splits)"),
        Role::Page => ("Pages by", "One page per level, shown one at a time with a page chooser"),
        Role::Color => ("Color", "Marker color: a palette for categories, a ramp for continuous values"),
        Role::Size => ("Marker size", "Marker area grows with this numeric column"),
        Role::Shape => ("Marker shape", "One marker shape per level"),
        Role::Overlay => ("Subgroups", "Separate fit lines, smoothers and boxes for each level within a panel"),
        Role::Freq => ("Frequency", "How many observations each row stands for (numeric); used by every statistic"),
        Role::Weight => ("Weight", "Relative precision of each row (numeric) in fits and smoothers"),
        Role::Label => ("Point labels", "Text written beside each point (all rows, or only selected ones)"),
        Role::Y2 => {
            ("Right axis", "Numeric columns drawn against a second vertical axis on the right, with its own scale")
        }
    }
}

/// Columns that can go on `role`'s shelf.
fn fits_role(table: &DataTable, column: &str, role: Role) -> bool {
    !role.needs_numeric() || table.find(column).and_then(|i| table.column(i)).is_some_and(|c| c.as_numeric().is_some())
}

/// The shelves, in rows: position, panels, marker style, statistics.
pub(super) fn shelves(ui: &mut egui::Ui, spec: &mut GraphSpec, table: &DataTable, id: u64) {
    let layout: [&[Role]; 4] = [
        &[Role::X, Role::Y, Role::Y2],
        &[Role::GroupX, Role::GroupY, Role::Wrap, Role::Page],
        &[Role::Color, Role::Size, Role::Shape, Role::Overlay],
        &[Role::Freq, Role::Weight, Role::Label],
    ];
    let dragging = egui::DragAndDrop::has_payload_of_type::<ColumnDrag>(ui.ctx());
    for roles in layout {
        ui.horizontal_wrapped(|ui| {
            for &role in roles {
                shelf_zone(ui, spec, table, id, role, dragging);
            }
        });
    }
}

fn shelf_zone(ui: &mut egui::Ui, spec: &mut GraphSpec, table: &DataTable, id: u64, role: Role, dragging: bool) {
    let (label, hint) = shelf(role);
    let assigned: Vec<String> = spec.encoding.get(role).into_iter().map(str::to_owned).collect();
    let stroke_color =
        if dragging { theme::DIVIDER_ACTIVE } else { ui.visuals().widgets.noninteractive.bg_stroke.color };
    let frame = egui::Frame::new()
        .stroke(egui::Stroke::new(1.0_f32, stroke_color))
        .corner_radius(4.0_f32)
        .inner_margin(egui::Margin::symmetric(6, 3));
    let mut remove: Option<String> = None;
    let mut add: Option<String> = None;
    let mut add_switcher: Option<usize> = None;
    let (response, dropped) = ui.dnd_drop_zone::<ColumnDrag, ()>(frame, |ui| {
        ui.set_min_width(150.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new(label).strong().small()).on_hover_text(hint);
            if assigned.is_empty() {
                ui.label(RichText::new(if dragging { "drop here" } else { "—" }).weak().small());
            }
            for (slot, name) in assigned.iter().enumerate() {
                let drag_id = egui::Id::new(("graph_chip", id, role.key(), name.as_str()));
                let has_switcher = spec.switchers.iter().any(|w| w.role == role && w.slot == slot);
                ui.dnd_drag_source(drag_id, ColumnDrag { name: name.clone(), from: Some(role) }, |ui| {
                    egui::Frame::new()
                        .fill(ui.visuals().faint_bg_color)
                        .corner_radius(3.0_f32)
                        .inner_margin(egui::Margin::symmetric(4, 1))
                        .show(ui, |ui| {
                            ui.label(RichText::new(name.as_str()).small());
                        });
                })
                .response
                .on_hover_text("Drag to another shelf to move this column; right-click for a column picker")
                .context_menu(|ui| {
                    if ui
                        .add_enabled(!has_switcher, egui::Button::new("Add a column picker for this column"))
                        .on_hover_text("A chooser above the graph that swaps this column for another")
                        .clicked()
                    {
                        add_switcher = Some(slot);
                        ui.close_menu();
                    }
                    if ui.button(format!("Remove from {label}")).clicked() {
                        remove = Some(name.clone());
                        ui.close_menu();
                    }
                });
                if ui.small_button("×").on_hover_text(format!("Remove {name} from {label}")).clicked() {
                    remove = Some(name.clone());
                }
            }
            if role.is_multi() || assigned.is_empty() {
                crate::menus::menu_button(ui, "+", |ui| {
                    egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                        for col in table.columns() {
                            let enabled = !assigned.contains(&col.name) && fits_role(table, &col.name, role);
                            if ui.add_enabled(enabled, egui::Button::new(&col.name)).clicked() {
                                add = Some(col.name.clone());
                                ui.close_menu();
                            }
                        }
                    });
                })
                .response
                .on_hover_text(format!("Choose a column for {label}"));
            }
        });
    });
    response.response.on_hover_text(hint);
    if let Some(name) = remove {
        let slot = assigned.iter().position(|c| *c == name);
        spec.encoding.unassign(role, &name);
        // Switchers follow the slots that remain.
        spec.switchers.retain(|w| !(w.role == role && Some(w.slot) == slot));
        for w in spec.switchers.iter_mut().filter(|w| w.role == role) {
            if slot.is_some_and(|s| w.slot > s) {
                w.slot -= 1;
            }
        }
    }
    if let Some(name) = add {
        spec.encoding.assign(role, &name);
    }
    if let Some(slot) = add_switcher {
        // Every column that fits the role, starting from the one shown.
        let choices = table.columns().iter().map(|c| c.name.clone()).filter(|c| fits_role(table, c, role)).collect();
        spec.switchers.push(ColumnSwitcher { role, slot, choices });
    }
    if let Some(payload) = dropped {
        if payload.from != Some(role) && fits_role(table, &payload.name, role) {
            if let Some(from) = payload.from {
                spec.encoding.unassign(from, &payload.name);
            }
            spec.encoding.assign(role, &payload.name);
        }
    }
}
