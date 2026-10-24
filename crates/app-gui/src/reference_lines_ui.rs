//! The Reference lines window: the reference lines and shaded bands of one
//! plot (a report plot or a graph canvas), each placed at a typed value or
//! at a statistic of the plotted data (`stat_viz::element::reference`).

use crate::dialog_kit::ComboExt as _;
use eframe::egui::{self, RichText};
use stat_viz::element::reference::{
    ReferenceBand, ReferenceLine, ReferenceOrientation, ReferenceScope, ReferenceStat, ReferenceValue,
};
use stat_viz::plot_style::ReferenceLook;
use stat_viz::{LinePattern, Rgba};

/// Label of the menu item and button that open the window.
pub const OPEN_LABEL: &str = "Reference lines…";

/// Hover text of [`OPEN_LABEL`].
pub const OPEN_HINT: &str =
    "Add horizontal or vertical reference lines and shaded bands, at a typed value or a statistic of the data";

/// What a value can be in the window's choice: a typed value or one of the
/// statistics.
fn value_label(v: ReferenceValue) -> String {
    match v {
        ReferenceValue::Fixed(_) => "Typed value".to_owned(),
        ReferenceValue::Statistic(s) => s.kind_label().to_owned(),
    }
}

/// The choice of a value: typed (with its number) or a statistic (with its
/// quantile or multiple). `data` is whether statistics are offered.
fn value_row(ui: &mut egui::Ui, id: impl std::hash::Hash, value: &mut ReferenceValue, data: bool) -> bool {
    let mut changed = false;
    let id = ui.make_persistent_id(id);
    egui::ComboBox::from_id_salt(id).selected_text(value_label(*value)).width(200.0).show_list(ui, |ui| {
        let typed = !value.is_statistic();
        if ui.selectable_label(typed, "Typed value").clicked() && !typed {
            *value = ReferenceValue::Fixed(0.0);
            changed = true;
        }
        if data {
            for choice in ReferenceStat::CHOICES {
                let chosen = matches!(value, ReferenceValue::Statistic(s) if s.same_kind(choice));
                if ui.selectable_label(chosen, choice.kind_label()).clicked() && !chosen {
                    *value = ReferenceValue::Statistic(choice);
                    changed = true;
                }
            }
        }
    });
    match value {
        ReferenceValue::Fixed(v) => {
            changed |= ui.add(egui::DragValue::new(v).speed(0.1).max_decimals(6)).on_hover_text("The value").changed();
        }
        ReferenceValue::Statistic(ReferenceStat::Quantile(p)) => {
            let mut pct = *p * 100.0;
            if ui.add(egui::DragValue::new(&mut pct).range(0.0..=100.0).speed(0.5).suffix("%")).changed() {
                *p = pct / 100.0;
                changed = true;
            }
        }
        ReferenceValue::Statistic(ReferenceStat::MeanSd(k)) => {
            changed |= ui
                .add(egui::DragValue::new(k).range(-10.0..=10.0).speed(0.1).prefix("k = "))
                .on_hover_text("Mean + k standard deviations (negative k: below the mean)")
                .changed();
        }
        ReferenceValue::Statistic(_) => {}
    }
    changed
}

/// A colour choice: the default (`None`) or one picked.
fn color_row(ui: &mut egui::Ui, color: &mut Option<Rgba>, default: Rgba, hint: &str) {
    let mut own = color.is_some();
    if ui.checkbox(&mut own, "Own colour").on_hover_text(hint).changed() {
        *color = own.then_some(default);
    }
    if let Some(c) = color.as_mut() {
        let [r, g, b, a] = c.0;
        let mut c32 = egui::Color32::from_rgba_unmultiplied(r, g, b, a);
        if ui.color_edit_button_srgba(&mut c32).changed() {
            *c = Rgba(c32.to_srgba_unmultiplied());
        }
    }
}

fn orientation_row(ui: &mut egui::Ui, id: impl std::hash::Hash, o: &mut ReferenceOrientation) {
    egui::ComboBox::from_id_salt(ui.make_persistent_id(id)).selected_text(o.label()).width(170.0).show_list(ui, |ui| {
        for choice in ReferenceOrientation::ALL {
            ui.selectable_value(o, choice, choice.label());
        }
    });
}

fn scope_row(ui: &mut egui::Ui, id: impl std::hash::Hash, scope: &mut ReferenceScope) {
    ui.label("Statistic of:");
    egui::ComboBox::from_id_salt(ui.make_persistent_id(id)).selected_text(scope.label()).width(120.0).show_list(
        ui,
        |ui| {
            for choice in ReferenceScope::ALL {
                ui.selectable_value(scope, choice, choice.label());
            }
        },
    );
}

fn line_editor(ui: &mut egui::Ui, k: usize, line: &mut ReferenceLine, canvas: bool) -> bool {
    let mut remove = false;
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(format!("Line {}", k + 1)).strong());
        orientation_row(ui, ("reference_line_orientation", k), &mut line.orientation);
        ui.label("at");
        value_row(ui, ("reference_line_value", k), &mut line.value, true);
        if ui.small_button("Remove").on_hover_text("Remove this line").clicked() {
            remove = true;
        }
    });
    ui.horizontal_wrapped(|ui| {
        ui.label("Label:");
        let hint = line.name();
        ui.add(egui::TextEdit::singleline(&mut line.label).desired_width(140.0).hint_text(hint))
            .on_hover_text("Its name on the plot and in the key (empty: the value or statistic)");
        ui.checkbox(&mut line.show_value, "Show the value");
    });
    ui.horizontal_wrapped(|ui| {
        color_row(
            ui,
            &mut line.color,
            stat_viz::palette::REFERENCE,
            "Otherwise the reference colour, or each subgroup's",
        );
        ui.add(egui::DragValue::new(&mut line.width).range(0.25..=12.0).speed(0.1).prefix("width "));
        egui::ComboBox::from_id_salt(ui.make_persistent_id(("reference_line_pattern", k)))
            .selected_text(line.pattern.label())
            .width(90.0)
            .show_list(ui, |ui| {
                for p in LinePattern::ALL {
                    ui.selectable_value(&mut line.pattern, p, p.label());
                }
            });
        if canvas && line.value.is_statistic() {
            scope_row(ui, ("reference_line_scope", k), &mut line.scope);
        }
    });
    remove
}

fn band_editor(ui: &mut egui::Ui, k: usize, band: &mut ReferenceBand, canvas: bool) -> bool {
    let mut remove = false;
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(format!("Band {}", k + 1)).strong());
        orientation_row(ui, ("reference_band_orientation", k), &mut band.orientation);
        if ui.small_button("Remove").on_hover_text("Remove this band").clicked() {
            remove = true;
        }
    });
    ui.horizontal_wrapped(|ui| {
        ui.label("From");
        value_row(ui, ("reference_band_from", k), &mut band.from, true);
    });
    ui.horizontal_wrapped(|ui| {
        ui.label("to");
        value_row(ui, ("reference_band_to", k), &mut band.to, true);
    });
    ui.horizontal_wrapped(|ui| {
        ui.label("Label:");
        let hint = band.name();
        ui.add(egui::TextEdit::singleline(&mut band.label).desired_width(140.0).hint_text(hint))
            .on_hover_text("Its name in the key (empty: its two edges)");
        ui.checkbox(&mut band.edges, "Edge lines");
        ui.checkbox(&mut band.edge_labels, "Edge values");
    });
    ui.horizontal_wrapped(|ui| {
        color_row(
            ui,
            &mut band.color,
            stat_viz::palette::REFERENCE_BAND,
            "Otherwise a neutral grey, or each subgroup's",
        );
        ui.add(egui::Slider::new(&mut band.opacity, 0.02..=1.0).text("opacity"));
        if canvas && (band.from.is_statistic() || band.to.is_statistic()) {
            scope_row(ui, ("reference_band_scope", k), &mut band.scope);
        }
    });
    remove
}

/// The editor: every line and band with its settings, and buttons to add
/// more. `canvas` offers the choice of rows a statistic is computed from
/// (a graph canvas has panels and subgroups; a report plot is one set).
pub fn editor(ui: &mut egui::Ui, look: &mut ReferenceLook, canvas: bool) {
    let mut remove_line = None;
    for (k, line) in look.lines.iter_mut().enumerate() {
        ui.push_id(("reference_line", k), |ui| {
            if line_editor(ui, k, line, canvas) {
                remove_line = Some(k);
            }
        });
        ui.separator();
    }
    if let Some(k) = remove_line {
        look.lines.remove(k);
    }
    let mut remove_band = None;
    for (k, band) in look.bands.iter_mut().enumerate() {
        ui.push_id(("reference_band", k), |ui| {
            if band_editor(ui, k, band, canvas) {
                remove_band = Some(k);
            }
        });
        ui.separator();
    }
    if let Some(k) = remove_band {
        look.bands.remove(k);
    }
    ui.horizontal(|ui| {
        if ui.button("+ Add line").on_hover_text("A dashed line at a value you type, to change as you like").clicked() {
            look.lines.push(ReferenceLine::default());
        }
        if ui.button("+ Add band").on_hover_text("A shaded stretch between two values, behind the data").clicked() {
            look.bands.push(ReferenceBand::default());
        }
        if !look.is_empty() && ui.button("Remove all").clicked() {
            *look = ReferenceLook::default();
        }
    });
    crate::dialog_kit::note(
        ui,
        "Statistics are of the plotted values on the line's axis (rows excluded from analyses are left out) and \
         follow every change to the data or row states. Individuals limits are x̄ ± 3·MR̄/d₂ from the values in row \
         order.",
    );
}

/// The window, shown while `open` is set.
pub fn window(ctx: &egui::Context, id: egui::Id, title: &str, look: &mut ReferenceLook, open: &mut bool, canvas: bool) {
    if !*open {
        return;
    }
    let heading = if title.is_empty() { "Reference lines".to_owned() } else { format!("Reference lines: {title}") };
    egui::Window::new(heading).id(id).open(open).resizable(true).default_width(470.0).collapsible(false).show(
        ctx,
        |ui| {
            egui::ScrollArea::vertical().max_height(520.0).show(ui, |ui| editor(ui, look, canvas));
        },
    );
}
