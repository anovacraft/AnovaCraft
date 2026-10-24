//! A report's own row filter strip and column picker bar (see
//! [`app_model::report_scope`]).
//!
//! The filter strip sits under a report's title line: one chip per
//! condition (click to change its levels or interval, or remove it),
//! "+ Add condition" and "Clear". Changes made while a slider is held apply
//! when it is let go, so the analysis is not refitted on every frame of a
//! drag. "Filter panel" opens the same conditions as a form under the strip
//! (level lists, sliders, stepping through levels; see
//! [`crate::filter_panel`]). The switcher bar steps the report's response
//! or process column through a list of columns.

use crate::dialog_kit::ComboExt as _;
use app_model::{FilterCondition, ReportScope, SavedReport};
use eframe::egui::{self, RichText};
use stat_data::DataTable;
use stat_quality::SpecLimits;

/// On-screen state of a report's filter strip and switcher.
#[derive(Debug, Default)]
pub struct ScopeUi {
    /// The filter strip is shown although the filter has no condition yet.
    pub filter_open: bool,
    /// A condition being changed (applied once nothing is held down).
    pending: Option<FilterCondition>,
    /// A column picked in the switcher (taken by the report).
    pub switch_to: Option<usize>,
    /// The filter panel is open under the strip.
    pub panel_open: bool,
    /// The filter panel's stepping state.
    pub panel: crate::filter_panel::FilterPanelState,
    /// Conditions changed in the panel while something is held down.
    panel_pending: Option<Vec<FilterCondition>>,
}

/// The report options menu's items for the filter and switcher. `request`
/// is the report's analysis request (without its scope).
pub fn menu_items(
    ui: &mut egui::Ui,
    request: &SavedReport,
    table: &DataTable,
    scope: &mut ReportScope,
    state: &mut ScopeUi,
) {
    if scope.is_filtered() {
        if ui.button("Remove row filter").on_hover_text("Use every row the table includes again").clicked() {
            scope.filter.clear();
            state.filter_open = false;
            state.pending = None;
            state.panel_pending = None;
            ui.close_menu();
        }
    } else if ui
        .button("Filter rows…")
        .on_hover_text(
            "Narrow this report to rows with chosen levels or values, without excluding them from the table or \
             its other reports",
        )
        .clicked()
    {
        state.filter_open = true;
        ui.close_menu();
    }
    let imputations = app_model::multiple_imputation::imputation_column(table);
    if scope.pool.is_some() || (imputations.is_some() && app_model::pooling::can_pool(request)) {
        let mut on = scope.pool.is_some();
        let tip = "Fit the analysis to each completed copy of the data and combine the results by Rubin's rules; \
                   the report's other sections then use the original rows (Imputation 0) only";
        if ui.checkbox(&mut on, "Pool across imputations").on_hover_text(tip).changed() {
            scope.pool = on
                .then(|| imputations.and_then(|c| table.column(c)).map(|c| c.name.clone()))
                .flatten()
                .or_else(|| on.then(|| app_model::multiple_imputation::IMPUTATION_COLUMN.to_owned()));
            ui.close_menu();
        }
    }
    if let Some(slot) = request.switch_slot() {
        let mut on = scope.switcher.is_some();
        let tip = format!("Step this report's {} through other columns, one at a time", slot.role.to_lowercase());
        if ui.checkbox(&mut on, "Column picker").on_hover_text(tip).changed() {
            scope.switcher = on.then(|| names(table, &request.switch_candidates(table)));
            ui.close_menu();
        }
    }
}

fn names(table: &DataTable, columns: &[usize]) -> Vec<String> {
    columns.iter().filter_map(|&c| table.column(c).map(|c| c.name.clone())).collect()
}

/// The filter strip, shown while the report has a filter or one is being
/// added.
pub fn filter_strip(ui: &mut egui::Ui, scope: &mut ReportScope, state: &mut ScopeUi, table: &DataTable, id: u64) {
    if !scope.is_filtered() && !state.filter_open {
        return;
    }
    // A changed condition is applied once nothing is held down.
    let held = ui.input(|i| i.pointer.any_down());
    if !held {
        if let Some(c) = state.pending.take() {
            if let Some(slot) = scope.filter.iter_mut().find(|f| f.column() == c.column()) {
                *slot = c;
            }
        }
        if let Some(conditions) = state.panel_pending.take() {
            scope.filter = conditions;
        }
    }
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Row filter").strong()).on_hover_text(
            "Only rows passing every condition enter this report. The table's own row states are not changed, so \
             its grid and other reports still use every row.",
        );
        let mut remove = None;
        for f in scope.filter.clone() {
            let column = f.column().to_owned();
            let shown = state.pending.clone().filter(|p| p.column() == column).unwrap_or_else(|| f.clone());
            let chip = crate::menus::menu_button(ui, format!("{} ▾", short(&shown.describe(), 44)), |ui| {
                ui.set_min_width(280.0);
                ui.label(RichText::new(&column).strong());
                let mut edited = shown.clone();
                let levels = stat_viz::filter::column_levels(table, &column);
                let range = crate::filter_panel::data_range(table, &column);
                if crate::report_view::condition_editor(ui, &mut edited, &levels, range) {
                    state.pending = Some(edited);
                }
                ui.separator();
                if ui.button("Remove condition").clicked() {
                    remove = Some(column.clone());
                    ui.close_menu();
                }
            });
            chip.response
                .on_hover_text(format!("{}\nClick to change it", shown.describe()))
                .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Filter {column}")));
        }
        if let Some(column) = remove {
            scope.filter.retain(|f| f.column() != column);
            state.pending = None;
            state.filter_open = true;
        }
        // The panel has its own "+ Add condition".
        if !state.panel_open {
            crate::menus::menu_button(ui, "+ Add condition", |ui| {
                ui.set_min_width(200.0);
                egui::ScrollArea::vertical().id_salt(("report_filter_columns", id)).max_height(320.0).show(
                    ui,
                    |ui| {
                        for col in table.columns() {
                            let used = scope.condition(&col.name).is_some();
                            if ui.add_enabled(!used, egui::Button::new(&col.name)).clicked() {
                                if let Some(c) = FilterCondition::for_column(table, &col.name) {
                                    scope.filter.push(c);
                                }
                                ui.close_menu();
                            }
                        }
                    },
                );
            });
        }
        ui.toggle_value(&mut state.panel_open, "Filter panel").on_hover_text(
            "Show the conditions as a form under this line: level lists, sliders, and stepping through one level \
             at a time",
        );
        if ui.small_button("Clear").on_hover_text("Remove the row filter").clicked() {
            scope.filter.clear();
            state.pending = None;
            state.panel_pending = None;
            state.filter_open = false;
            state.panel_open = false;
            state.panel = Default::default();
        }
        if scope.filter.is_empty() && !state.panel_open {
            ui.label(RichText::new("Add a condition on any column to narrow this report").weak().small());
        }
    });
    if state.panel_open {
        // Edits apply once nothing is held down, so a slider drag does not
        // refit the analysis on every frame.
        let mut working = state.panel_pending.clone().unwrap_or_else(|| scope.filter.clone());
        ui.allocate_ui(egui::vec2(ui.available_width().min(420.0), 0.0), |ui| {
            crate::filter_panel::filter_panel(ui, &mut working, table, &mut state.panel, ("report_filter_panel", id));
        });
        if working != scope.filter {
            if held {
                state.panel_pending = Some(working);
            } else {
                scope.filter = working;
                state.panel_pending = None;
                state.pending = None;
            }
        } else {
            state.panel_pending = None;
        }
        if scope.filter.is_empty() {
            state.filter_open = true;
        }
    }
}

/// The column picker bar: the column now in the report's switch slot,
/// with previous and next buttons, a list of the choices, a menu to change
/// them and × to remove the switcher. A picked column goes to
/// [`ScopeUi::switch_to`].
pub fn switcher_bar(
    ui: &mut egui::Ui,
    request: &SavedReport,
    scope: &mut ReportScope,
    state: &mut ScopeUi,
    table: &DataTable,
    id: u64,
) {
    let (Some(choices), Some(slot)) = (scope.switcher.clone(), request.switch_slot()) else { return };
    let current = table.column(slot.column).map(|c| c.name.clone()).unwrap_or_default();
    let candidates = request.switch_candidates(table);
    // Choices still usable in this role (a column may have been removed or
    // become a predictor since).
    let usable: Vec<(usize, &String)> =
        choices.iter().filter_map(|n| table.find(n).filter(|c| candidates.contains(c)).map(|c| (c, n))).collect();
    let at = usable.iter().position(|(c, _)| *c == slot.column);
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(format!("{}:", slot.role)).strong());
        let n = usable.len();
        let step = |k: isize| -> Option<usize> {
            if n == 0 {
                return None;
            }
            let i = match at {
                Some(i) => (i as isize + k).rem_euclid(n as isize) as usize,
                None => 0,
            };
            Some(usable[i].0)
        };
        if ui.small_button("Previous").on_hover_text("Show the previous column in the list").clicked() {
            state.switch_to = step(-1);
        }
        egui::ComboBox::from_id_salt(("report_switcher", id)).selected_text(current.as_str()).show_list(ui, |ui| {
            for (c, name) in &usable {
                if ui.selectable_label(*c == slot.column, name.as_str()).clicked() {
                    state.switch_to = Some(*c);
                }
            }
        });
        if ui.small_button("Next").on_hover_text("Show the next column in the list").clicked() {
            state.switch_to = step(1);
        }
        if let Some(i) = at {
            ui.label(RichText::new(format!("{} of {n}", i + 1)).weak().small());
        }
        crate::menus::menu_button(ui, "choices…", |ui| {
            egui::ScrollArea::vertical().id_salt(("report_switcher_choices", id)).max_height(300.0).show(ui, |ui| {
                let mut list = choices.clone();
                for &c in &candidates {
                    let Some(name) = table.column(c).map(|c| c.name.clone()) else { continue };
                    let mut on = list.contains(&name);
                    if ui.checkbox(&mut on, name.as_str()).changed() {
                        if on {
                            list.push(name);
                        } else {
                            list.retain(|l| *l != name);
                        }
                    }
                }
                // Keep the choices in table order.
                list.sort_by_key(|n| table.find(n).unwrap_or(usize::MAX));
                if list != choices {
                    scope.switcher = Some(list);
                }
            });
        });
        if ui.small_button("×").on_hover_text("Remove the column picker").clicked() {
            scope.switcher = None;
        }
    });
}

/// Specification limits of a capability study, edited in place beside its
/// column picker (each column keeps its own limits). Returns `true` when
/// they changed.
pub fn limits_editor(ui: &mut egui::Ui, limits: &mut SpecLimits, column: &str, id: u64) -> bool {
    let before = *limits;
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(format!("Specification limits for {column}:")).weak());
        for (label, value) in
            [("Lower", &mut limits.lower), ("Target", &mut limits.target), ("Upper", &mut limits.upper)]
        {
            ui.label(label);
            optional_number(ui, value, egui::Id::new(("spec_limit", id, column, label)), label);
        }
    });
    *limits != before
}

/// A text box for an optional number: empty means none. The value follows
/// the text as it is typed whenever the text is a number (or empty); text
/// that is not a number is kept while being edited and goes back to the
/// value when the box loses focus.
fn optional_number(ui: &mut egui::Ui, value: &mut Option<f64>, key: egui::Id, label: &str) {
    let shown = value.map(|v| v.to_string()).unwrap_or_default();
    let mut text = ui.data(|d| d.get_temp::<String>(key)).unwrap_or_else(|| shown.clone());
    let response = ui.add(egui::TextEdit::singleline(&mut text).desired_width(70.0).hint_text("none"));
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, label));
    if response.changed() {
        let t = text.trim();
        if t.is_empty() {
            *value = None;
        } else if let Some(v) = t.parse::<f64>().ok().filter(|v| v.is_finite()) {
            *value = Some(v);
        }
        ui.data_mut(|d| d.insert_temp(key, text));
    }
    if response.lost_focus() {
        ui.data_mut(|d| d.remove::<String>(key));
    }
}

/// `text`, shortened to `max` characters with "…".
fn short(text: &str, max: usize) -> String {
    if text.chars().count() > max { text.chars().take(max - 1).chain(['…']).collect() } else { text.to_owned() }
}
