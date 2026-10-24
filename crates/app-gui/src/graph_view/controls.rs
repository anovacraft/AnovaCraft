//! The canvas's controls: axis scales, the graph's row
//! filter, column pickers and the page chooser.

use crate::dialog_kit::ComboExt as _;
use eframe::egui::{self, RichText};
use stat_data::{DataTable, RowStateManager, SelectionMode};
use crate::filter_panel::FilterPanelState;
use stat_viz::{
    AngleUnit, AxisScale, FilterCondition, GraphForm, GraphSpec, PieSettings, PolarSettings, RadarSettings,
    RidgelineSettings, TreemapSettings, VectorSettings, WaterfallSettings, WindRoseSettings,
};

use super::shelves::shelf;

/// Axis scales, selection tool and the shelves toggle.
pub(super) fn view_bar(
    ui: &mut egui::Ui,
    spec: &mut GraphSpec,
    lasso: &mut bool,
    hide_shelves: &mut bool,
    key_editor: &mut bool,
    id: u64,
) {
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Chart:").strong());
        let treemap = match spec.form {
            GraphForm::Treemap(t) => t,
            _ => TreemapSettings { size_by_sum: false, labels: true },
        };
        egui::ComboBox::from_id_salt("graph_form").selected_text(spec.form.label()).show_list(ui, |ui| {
            for (form, hint) in [
                (GraphForm::Panels, "Axes and layers, split into panels by the panel shelves"),
                (GraphForm::Treemap(treemap), "Nested rectangles: one level per horizontal-axis column, outer first"),
                (GraphForm::Parallel, "One vertical axis per column on the axis shelves; a line per row"),
                (
                    GraphForm::MultiVari,
                    "The vertical column by up to four horizontal-axis factors (outer first): values, and each \
                     factor's means joined within the factor above",
                ),
                (
                    GraphForm::ScatterMatrix,
                    "Every pair of the continuous columns on the axis shelves in a grid; histograms on the \
                     diagonal, the other layers (points, density ellipses, fits) off it",
                ),
                (
                    keep(&spec.form, GraphForm::Pie(PieSettings::default())),
                    "Slices by the levels of the horizontal-axis column, sized by row count or by the vertical \
                     column's sum; click a slice to select its rows",
                ),
                (
                    keep(&spec.form, GraphForm::Radar(RadarSettings::default())),
                    "One spoke per numeric column on the axis shelves; a polygon per row, or per colour group's \
                     means",
                ),
                (
                    keep(&spec.form, GraphForm::Polar(PolarSettings::default())),
                    "Points at the horizontal column's angle and the vertical column's radius",
                ),
                (
                    keep(&spec.form, GraphForm::WindRose(WindRoseSettings::default())),
                    "Rows counted by direction sector (horizontal column, compass degrees), stacked by bins of \
                     the vertical column",
                ),
                (
                    GraphForm::Ternary,
                    "Three component columns on the axis shelves, each row divided by its total, in a triangle",
                ),
                (
                    keep(&spec.form, GraphForm::Waterfall(WaterfallSettings::default())),
                    "A running total of the vertical column's sums by horizontal-axis category, in table order, \
                     then the total",
                ),
                (
                    keep(&spec.form, GraphForm::Ridgeline(RidgelineSettings::default())),
                    "Kernel densities of the horizontal column, one per level of the vertical (or colour) \
                     column, stacked",
                ),
                (
                    keep(&spec.form, GraphForm::VectorField(VectorSettings::default())),
                    "An arrow per row: x and its horizontal component on the horizontal shelf, y and its \
                     vertical component on the vertical shelf",
                ),
            ]
            .into_iter()
            .chain(super::form_settings::choices())
            {
                let chosen = std::mem::discriminant(&spec.form) == std::mem::discriminant(&form);
                if ui.selectable_label(chosen, form.label()).on_hover_text(hint).clicked() && !chosen {
                    spec.form = form;
                    if form == GraphForm::ScatterMatrix && spec.ellipse.is_none() {
                        spec.ellipse = Some(stat_viz::EllipseElement::default());
                    }
                    super::form_settings::on_chosen(spec);
                }
            }
        });
        if let GraphForm::Treemap(t) = &mut spec.form {
            ui.checkbox(&mut t.size_by_sum, "size by the vertical column's sum")
                .on_hover_text("Otherwise each tile's area is its number of rows");
            ui.checkbox(&mut t.labels, "labels");
        }
        form_settings(ui, &mut spec.form);
        ui.separator();
        ui.label(RichText::new("Scales:").strong());
        scale_choice(ui, "Horizontal", &mut spec.options.x_scale);
        scale_choice(ui, "Vertical", &mut spec.options.y_scale);
        ui.separator();
        ui.label(RichText::new("Select by:").strong());
        ui.radio_value(lasso, false, "box").on_hover_text("Drag a rectangle in a panel to select the rows inside it");
        ui.radio_value(lasso, true, "lasso")
            .on_hover_text("Draw a freehand outline in a panel to select the rows inside it");
        ui.separator();
        ui.toggle_value(hide_shelves, "Hide shelves").on_hover_text("Fold the column shelves away");
        ui.separator();
        if ui.button("Edit key…").on_hover_text("Rename, hide or reorder the key's entries").clicked() {
            *key_editor = true;
        }
        if spec.form == GraphForm::Panels {
            ui.separator();
            let n = spec.axes.style.as_ref().map_or(0, |s| s.reference.lines.len() + s.reference.bands.len());
            let label = if n == 0 {
                crate::reference_lines_ui::OPEN_LABEL.to_owned()
            } else {
                format!("{} ({n})", crate::reference_lines_ui::OPEN_LABEL)
            };
            if ui.button(label).on_hover_text(crate::reference_lines_ui::OPEN_HINT).clicked() {
                ui.ctx().data_mut(|d| d.insert_temp(super::reference_open_id(id), true));
            }
        }
    });
}

/// `form` with the settings of `current` when it is the same kind of chart
/// (so reopening the chooser keeps them).
fn keep(current: &GraphForm, form: GraphForm) -> GraphForm {
    if std::mem::discriminant(current) == std::mem::discriminant(&form) { *current } else { form }
}

/// The settings of the pie, radar, polar, wind-rose, waterfall, ridgeline
/// and vector-field forms, beside the chooser.
fn form_settings(ui: &mut egui::Ui, form: &mut GraphForm) {
    match form {
        GraphForm::Pie(p) => {
            ui.checkbox(&mut p.donut, "donut").on_hover_text("Leave the centre open and write the total there");
            ui.checkbox(&mut p.size_by_sum, "size by the vertical column's sum")
                .on_hover_text("Otherwise each slice's angle is its number of rows");
            ui.checkbox(&mut p.labels, "labels").on_hover_text("Each slice's level, share and value beside it");
        }
        GraphForm::Radar(r) => {
            ui.checkbox(&mut r.group_means, "group means")
                .on_hover_text("One polygon through each colour or subgroup level's means, instead of one per row");
            ui.checkbox(&mut r.fill, "fill");
        }
        GraphForm::Polar(p) => {
            egui::ComboBox::from_id_salt("polar_angle").selected_text(p.angle.label()).show_list(ui, |ui| {
                for u in AngleUnit::ALL {
                    ui.selectable_value(&mut p.angle, u, u.label());
                }
            });
            ui.checkbox(&mut p.connect, "join in row order");
        }
        GraphForm::WindRose(w) => {
            ui.label("sectors");
            for k in [8, 16, 36] {
                ui.radio_value(&mut w.sectors, k, k.to_string());
            }
            ui.checkbox(&mut w.percent, "rings in %").on_hover_text("Otherwise the rings count rows");
        }
        GraphForm::Waterfall(w) => {
            ui.checkbox(&mut w.total, "total bar");
            ui.checkbox(&mut w.labels, "labels").on_hover_text("Each bar's signed value (+12, −5)");
        }
        GraphForm::Ridgeline(r) => {
            ui.add(egui::Slider::new(&mut r.height, 50..=300).text("height %"))
                .on_hover_text("Height of the tallest peak, in percent of the gap between ridges");
            ui.checkbox(&mut r.rug, "rug").on_hover_text("A tick for each row under its ridge");
        }
        GraphForm::VectorField(v) => {
            ui.add(egui::Slider::new(&mut v.scale, 10..=500).text("arrow length %"))
                .on_hover_text("Percent of the automatic length (the longest arrow about the points' spacing)");
        }
        _ => {}
    }
}

fn scale_choice(ui: &mut egui::Ui, label: &str, scale: &mut AxisScale) {
    ui.label(label);
    egui::ComboBox::from_id_salt(("graph_scale", label)).selected_text(scale.label()).show_list(ui, |ui| {
        for s in AxisScale::ALL {
            ui.selectable_value(scale, s, s.label());
        }
    });
}

/// The graph's own row filter, folded under a "Row filter" heading unless
/// it is shown beside the graph (`beside`).
pub(super) fn filter_panel(
    ui: &mut egui::Ui,
    spec: &mut GraphSpec,
    table: &DataTable,
    rows: &mut RowStateManager,
    panel: &mut FilterPanelState,
    beside: &mut bool,
    id: u64,
) {
    if *beside {
        return;
    }
    let filter = &spec.options.filter;
    let heading = if filter.is_active() {
        format!(
            "Row filter ({} condition{})",
            filter.conditions.len(),
            if filter.conditions.len() == 1 { "" } else { "s" }
        )
    } else {
        "Row filter".to_owned()
    };
    egui::CollapsingHeader::new(RichText::new(heading).strong())
        .id_salt(("graph_filter", id))
        .default_open(false)
        .show(ui, |ui| filter_body(ui, spec, table, rows, panel, beside, id));
}

/// The row filter's form: a condition per column (levels to keep, or an
/// interval, with stepping through levels) and what to do with rows
/// outside it. When "select matching rows" is on, the matching rows become
/// the shared selection whenever the filter changes.
pub(super) fn filter_body(
    ui: &mut egui::Ui,
    spec: &mut GraphSpec,
    table: &DataTable,
    rows: &mut RowStateManager,
    panel: &mut FilterPanelState,
    beside: &mut bool,
    id: u64,
) {
    let before = spec.options.filter.clone();
    let filter = &mut spec.options.filter;
    ui.horizontal_wrapped(|ui| {
        if *beside {
            ui.label(RichText::new("Row filter").strong());
        }
        let label = if *beside { "Move above the graph" } else { "Show beside the graph" };
        let hint = if *beside {
            "Fold the filter back under the Row filter heading above the graph"
        } else {
            "Keep the filter in a panel beside the graph, so its sliders and stepping sit next to the panels"
        };
        if ui.small_button(label).on_hover_text(hint).clicked() {
            *beside = !*beside;
        }
    });
    crate::filter_panel::filter_panel(ui, &mut filter.conditions, table, panel, ("graph_filter_panel", id));
    ui.horizontal_wrapped(|ui| {
        ui.checkbox(&mut filter.show_only, "Show only matching rows")
            .on_hover_text("Rows outside the filter are not drawn");
        ui.checkbox(&mut filter.include_only, "Analyse only matching rows")
            .on_hover_text("Fits, smoothers and summaries use matching rows only; others drawn are hollow");
        ui.checkbox(&mut filter.select, "Select matching rows")
            .on_hover_text("Make the matching rows the selection in every view whenever the filter changes");
        if ui
            .add_enabled(filter.is_active(), egui::Button::new("Select them now"))
            .on_hover_text("Select the matching rows in every view")
            .clicked()
        {
            rows.select(filter.matching_rows(table), SelectionMode::Replace);
        }
    });
    if filter.is_active() {
        let n = filter.matching_rows(table).len();
        ui.label(RichText::new(format!("{n} of {} rows match: {}", table.n_rows(), filter.describe())).weak().small());
    }
    let filter = &spec.options.filter;
    if filter.select && filter.is_active() && *filter != before {
        rows.select(filter.matching_rows(table), SelectionMode::Replace);
    }
}

/// The settings of one filter condition, laid out in a row: a check box
/// per level with "all" and "none", or "from" and "to" sliders. `levels`
/// and `range` are what the data offer (for a dashboard filter, across the
/// tables that have the column). Returns `true` when the condition changed.
pub(crate) fn condition_editor(
    ui: &mut egui::Ui,
    c: &mut FilterCondition,
    levels: &[String],
    range: (f64, f64),
) -> bool {
    let before = c.clone();
    match c {
        FilterCondition::Levels { levels: chosen, .. } => {
            if ui.small_button("all").clicked() {
                *chosen = levels.to_vec();
            }
            if ui.small_button("none").clicked() {
                chosen.clear();
            }
            for level in levels {
                let mut on = chosen.contains(level);
                if ui.checkbox(&mut on, level.as_str()).changed() {
                    if on {
                        chosen.push(level.clone());
                    } else {
                        chosen.retain(|l| l != level);
                    }
                }
            }
        }
        FilterCondition::Range { min, max, .. } => {
            let (lo, hi) = range;
            let speed = ((hi - lo) / 200.0).max(1e-9);
            ui.label("from");
            ui.add(egui::Slider::new(min, lo..=hi).drag_value_speed(speed).clamping(egui::SliderClamping::Never));
            ui.label("to");
            ui.add(egui::Slider::new(max, lo..=hi).drag_value_speed(speed).clamping(egui::SliderClamping::Never));
            if *min > *max {
                std::mem::swap(min, max);
            }
        }
    }
    *c != before
}

/// One chooser per column picker: the column now shown, with previous and
/// next buttons, a menu to edit the choices, and × to remove the switcher.
pub(super) fn switcher_bar(ui: &mut egui::Ui, spec: &mut GraphSpec, table: &DataTable, id: u64) {
    if spec.switchers.is_empty() {
        return;
    }
    let mut remove = None;
    let mut steps: Vec<(usize, isize)> = Vec::new();
    let mut pick: Option<(usize, String)> = None;
    for (k, w) in spec.switchers.iter_mut().enumerate() {
        let current = w.current(&spec.encoding).unwrap_or("—").to_owned();
        ui.horizontal_wrapped(|ui| {
            let (label, _) = shelf(w.role);
            let label = if w.role.is_multi() && spec.encoding.get(w.role).len() > 1 {
                format!("{label} ({})", w.slot + 1)
            } else {
                label.to_owned()
            };
            ui.label(RichText::new(format!("{label}:")).strong());
            if ui.small_button("Previous").on_hover_text("Show the previous column in the list").clicked() {
                steps.push((k, -1));
            }
            egui::ComboBox::from_id_salt(("graph_switcher", id, k)).selected_text(current.as_str()).show_list(
                ui,
                |ui| {
                    for c in &w.choices {
                        if ui.selectable_label(*c == current, c.as_str()).clicked() {
                            pick = Some((k, c.clone()));
                        }
                    }
                },
            );
            if ui.small_button("Next").on_hover_text("Show the next column in the list").clicked() {
                steps.push((k, 1));
            }
            crate::menus::menu_button(ui, "choices…", |ui| {
                egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                    for col in table.columns() {
                        let mut on = w.choices.contains(&col.name);
                        if ui.checkbox(&mut on, col.name.as_str()).changed() {
                            if on {
                                w.choices.push(col.name.clone());
                            } else {
                                w.choices.retain(|c| *c != col.name);
                            }
                        }
                    }
                });
            });
            if ui.small_button("×").on_hover_text("Remove this column picker").clicked() {
                remove = Some(k);
            }
        });
    }
    for (k, s) in steps {
        let w = spec.switchers[k].clone();
        w.step(&mut spec.encoding, s);
    }
    if let Some((k, c)) = pick {
        let (role, slot) = (spec.switchers[k].role, spec.switchers[k].slot);
        spec.encoding.replace(role, slot, &c);
    }
    if let Some(k) = remove {
        spec.switchers.remove(k);
    }
}

/// The page chooser: previous / next buttons and a list of every page.
pub(super) fn page_bar(ui: &mut egui::Ui, spec: &mut GraphSpec, pages: &[String], page: usize, id: u64) {
    if pages.is_empty() {
        return;
    }
    let column = spec.encoding.page.clone().unwrap_or_default();
    ui.horizontal(|ui| {
        ui.label(RichText::new("Page:").strong());
        if ui.add_enabled(page > 0, egui::Button::new("Previous").small()).clicked() {
            spec.options.page = page - 1;
        }
        egui::ComboBox::from_id_salt(("graph_page", id)).selected_text(format!("{column}: {}", pages[page])).show_list(
            ui,
            |ui| {
                for (k, p) in pages.iter().enumerate() {
                    if ui.selectable_label(k == page, format!("{column}: {p}")).clicked() {
                        spec.options.page = k;
                    }
                }
            },
        );
        if ui.add_enabled(page + 1 < pages.len(), egui::Button::new("Next").small()).clicked() {
            spec.options.page = page + 1;
        }
        ui.label(RichText::new(format!("{} of {}", page + 1, pages.len())).weak());
    });
}
