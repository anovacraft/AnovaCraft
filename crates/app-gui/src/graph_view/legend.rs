//! The key below the graph's panels.

use eframe::egui::{self, RichText};
use stat_data::RowStateManager;
use stat_viz::render::{color32, marker};
use stat_viz::{ColorKey, Frame, GraphSpec, KeyGlyph, Layer, LinePattern, Ramp, RightAxis};

use super::PlotKey;
use crate::theme;

/// The key below the panels: color (levels with their shapes, or a ramp),
/// shape, size and subgroups, then each layer's entries and color ramps.
/// With a right axis the key starts with which series uses which axis, and
/// includes the right-axis layers' entries (labelled "(right axis)").
/// The entries are shown as the user edited them (renamed, hidden,
/// reordered: [`GraphSpec::key_shown`]); right-clicking the key offers the
/// key editor (`editor`). Labels are read as math markup when the graph
/// says so.
#[allow(clippy::too_many_arguments)]
pub(super) fn legend(
    ui: &mut egui::Ui,
    column: bool,
    spec: &GraphSpec,
    frame: &Frame,
    layers: &[Vec<Layer>],
    rows: &RowStateManager,
    right: Option<&RightAxis>,
    editor: &mut bool,
) {
    let mut key = PlotKey::default();
    key.read_markup(spec.axes.markup_labels);
    let legend = &frame.legend;
    let text_color = ui.visuals().text_color();
    for (id, label, color, glyph) in spec.key_shown(frame, layers, right) {
        // Shape levels are drawn in the text color.
        let color = if id.starts_with("shape/") { text_color } else { color32(color) };
        match glyph {
            KeyGlyph::Line(pattern) => key.line(label, color, line_pattern(pattern)),
            KeyGlyph::Area(fill) => key.area(label, color32(fill), color),
            KeyGlyph::Marker(shape, filled) => key.marker(label, color, marker(shape), filled),
        }
    }
    if rows.counts().selected > 0 {
        key.marker("Selected", theme::SELECTED, egui_plot::MarkerShape::Circle, false);
    }
    let shown = ui
        .scope(|ui| {
            if column {
                key.show_column(ui);
            } else {
                key.show(ui);
            }
        })
        .response;
    let menu = ui.interact(shown.rect, ui.id().with("graph_key_menu"), egui::Sense::click());
    menu.context_menu(|ui| {
        if ui.button("Edit key…").clicked() {
            *editor = true;
            ui.close_menu();
        }
    });
    let mut ramps: Vec<&Ramp> = Vec::new();
    for ramp in layers.iter().flatten().filter_map(|l| l.ramp.as_ref()) {
        if !ramps.iter().any(|r| r.title == ramp.title) {
            ramps.push(ramp);
        }
    }
    // A heat map shading by the color column replaces that column's own ramp.
    let color_title = match &legend.color {
        Some(ColorKey::Gradient { title, .. }) => Some(title.clone()),
        _ => None,
    };
    let mut merged_color = false;
    for ramp in ramps {
        let title = match (&color_title, ramp.title.strip_suffix("of the color column")) {
            (Some(c), Some(stat)) => {
                merged_color = true;
                format!("{stat}of {c}")
            }
            _ => ramp.title.clone(),
        };
        // Ranges differ between panels: show the widest.
        let (min, max) = layers
            .iter()
            .flatten()
            .filter_map(|l| l.ramp.as_ref())
            .filter(|r| r.title == ramp.title)
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), r| (a.min(r.min), b.max(r.max)));
        gradient_bar(ui, &title, min, max, ramp.reversed);
    }
    if let (Some(ColorKey::Gradient { title, min, max }), false) = (&legend.color, merged_color) {
        gradient_bar(ui, title, *min, *max, false);
    }
    if let Some((title, lo, hi)) = &legend.size {
        ui.label(
            RichText::new(format!(
                "Marker size: {title}, from {} (smallest) to {} (largest)",
                stat_viz::view::format_value(*lo),
                stat_viz::view::format_value(*hi)
            ))
            .small(),
        );
    }
}

/// The key editor: every generated entry in the key's order, each with a
/// check box showing or hiding it, its text, and buttons moving it up or
/// down; Reset key undoes every change. `items`: the generated entries
/// ([`GraphSpec::key_items`]).
pub(super) fn key_editor(
    ctx: &egui::Context,
    spec: &mut GraphSpec,
    items: Vec<(String, String, stat_viz::Rgba, KeyGlyph)>,
    open: &mut bool,
    id: u64,
) {
    let mut look = spec.key_look();
    let before = look.clone();
    let ordered = stat_viz::plot_style::reorder_slots(items, |e| look.order.iter().position(|o| *o == e.0));
    let ids: Vec<String> = ordered.iter().map(|e| e.0.clone()).collect();
    let mut done = false;
    egui::Window::new("Edit key")
        .id(egui::Id::new(("graph_key_editor", id)))
        .collapsible(false)
        .resizable(false)
        .open(open)
        .show(ctx, |ui| {
            ui.set_min_width(380.0);
            if ordered.is_empty() {
                ui.label(RichText::new("The key has no entries.").weak());
            }
            egui::Grid::new(("graph_key_editor_rows", id)).num_columns(4).show(ui, |ui| {
                for (k, (entry, generated, ..)) in ordered.iter().enumerate() {
                    let mut shown = !look.is_hidden(entry);
                    let check = ui.checkbox(&mut shown, "");
                    check.widget_info(|| {
                        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, shown, format!("Show {generated}"))
                    });
                    if check.on_hover_text("Show this entry").changed() {
                        look.set_hidden(entry, !shown);
                    }
                    let mut text = look.renamed(entry).unwrap_or(generated).to_owned();
                    let field = ui.add_sized([240.0, 20.0], egui::TextEdit::singleline(&mut text));
                    field.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, format!("Text of {generated}"))
                    });
                    if field.changed() {
                        look.rename(entry, &text, generated);
                    }
                    let up = ui.add_enabled(k > 0, egui::Button::new("▲").small());
                    up.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Move {generated} up"))
                    });
                    if up.clicked() {
                        look.move_entry(&ids, entry, true);
                    }
                    let down = ui.add_enabled(k + 1 < ordered.len(), egui::Button::new("▼").small());
                    down.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Move {generated} down"))
                    });
                    if down.clicked() {
                        look.move_entry(&ids, entry, false);
                    }
                    ui.end_row();
                }
            });
            ui.label(
                RichText::new("Entries keep their changes while the data change; new levels are added.").weak(),
            );
            ui.horizontal(|ui| {
                if ui.button("Reset key").on_hover_text("Undo every change to the key").clicked() {
                    look = stat_viz::plot_style::KeyLook::default();
                }
                done = ui.button("Done").clicked();
            });
        });
    if done {
        *open = false;
    }
    if look != before {
        spec.edit_style(|s| s.key = look);
    }
}

/// The app's line pattern for a layer's.
fn line_pattern(p: LinePattern) -> app_model::LinePattern {
    app_model::LinePattern::from_viz(p)
}

/// A horizontal color ramp labelled with what it shows and its end values
/// (`reversed`: light at `min`, dark at `max`).
fn gradient_bar(ui: &mut egui::Ui, title: &str, min: f64, max: f64, reversed: bool) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("{title}:")).small());
        ui.label(RichText::new(stat_viz::view::format_value(min)).small());
        let (rect, _) = ui.allocate_exact_size(egui::vec2(140.0, 10.0), egui::Sense::hover());
        let steps = 28;
        let w = rect.width() / steps as f32;
        for k in 0..steps {
            let t = (k as f64 + 0.5) / steps as f64;
            let t = if reversed { 1.0 - t } else { t };
            let r = egui::Rect::from_min_size(
                egui::pos2(rect.left() + k as f32 * w, rect.top()),
                egui::vec2(w + 0.5, rect.height()),
            );
            ui.painter().rect_filled(r, 0.0_f32, color32(stat_viz::palette::ramp(t)));
        }
        ui.label(RichText::new(stat_viz::view::format_value(max)).small());
    });
}
