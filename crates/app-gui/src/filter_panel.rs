//! The filter panel: a form for a row filter's conditions (a level list per
//! categorical column, "from" and "to" sliders per continuous one) with
//! stepping through levels. Reports and graph canvases share it.
//!
//! Stepping shows one level of a condition's column at a time (for a
//! continuous column, a window of fixed width moved along it), by hand with
//! Previous and Next or automatically with Play, at a chosen number of
//! seconds per step. It is the conditioning-plot idea of Cleveland
//! (*Visualizing Data*, 1993) put in time: the same view drawn for each
//! slice of a conditioning variable in turn.

use eframe::egui::{self, RichText};
use stat_data::DataTable;
use stat_viz::FilterCondition;
use stat_viz::filter::column_levels;

/// Seconds each step stays on screen unless changed.
const DEFAULT_SECONDS: f32 = 1.5;

/// On-screen state of one filter panel (not saved).
#[derive(Debug, Clone, Default)]
pub struct FilterPanelState {
    /// The column whose condition is being stepped through.
    pub stepping: Option<String>,
    /// Stepping on its own, one step every [`Self::seconds`].
    pub playing: bool,
    /// Seconds per automatic step (`0`: the default).
    pub seconds: f32,
    /// Time of the last automatic step.
    last_step: f64,
}

impl FilterPanelState {
    /// Seconds per automatic step.
    pub fn seconds(&self) -> f32 {
        if self.seconds > 0.0 { self.seconds } else { DEFAULT_SECONDS }
    }
}

/// The panel's contents: one block per condition (its column, Step through,
/// ×, then its levels or sliders, and the stepping controls while stepping)
/// and "+ Add condition". Edits `conditions` in place; returns `true` when
/// they changed.
pub fn filter_panel(
    ui: &mut egui::Ui,
    conditions: &mut Vec<FilterCondition>,
    table: &DataTable,
    state: &mut FilterPanelState,
    id: impl std::hash::Hash + Copy,
) -> bool {
    let before = conditions.clone();
    if state.stepping.as_ref().is_some_and(|c| !conditions.iter().any(|f| f.column() == c)) {
        state.stepping = None;
        state.playing = false;
    }
    let mut remove = None;
    for (i, c) in conditions.iter_mut().enumerate() {
        let column = c.column().to_owned();
        let levels = column_levels(table, &column);
        let range = data_range(table, &column);
        egui::Frame::group(ui.style()).inner_margin(egui::Margin::symmetric(6, 4)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(&column).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let close = ui.small_button("×").on_hover_text("Remove this condition");
                    close.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Remove {column}"))
                    });
                    if close.clicked() {
                        remove = Some(i);
                    }
                    let mut on = state.stepping.as_deref() == Some(column.as_str());
                    let hint = match c {
                        FilterCondition::Levels { .. } => "Show one level at a time, stepping through them",
                        FilterCondition::Range { .. } => {
                            "Show a window of this column's values at a time, moving it along"
                        }
                    };
                    let toggle = ui.toggle_value(&mut on, "Step through").on_hover_text(hint);
                    toggle.widget_info(|| {
                        egui::WidgetInfo::selected(
                            egui::WidgetType::Checkbox,
                            true,
                            on,
                            format!("Step through {column}"),
                        )
                    });
                    if toggle.changed() {
                        state.playing = false;
                        if on {
                            state.stepping = Some(column.clone());
                            // Start at the first level or window.
                            *c = c.stepped(&levels, range, 1);
                        } else {
                            state.stepping = None;
                        }
                    }
                });
            });
            condition_form(ui, c, &levels, range, (id, i));
            if state.stepping.as_deref() == Some(column.as_str()) {
                step_controls(ui, c, &levels, range, state);
            }
        });
    }
    if let Some(i) = remove {
        conditions.remove(i);
    }
    crate::menus::menu_button(ui, "+ Add condition", |ui| {
        ui.set_min_width(200.0);
        egui::ScrollArea::vertical().id_salt(("filter_panel_columns", id)).max_height(320.0).show(ui, |ui| {
            for col in table.columns() {
                let used = conditions.iter().any(|c| c.column() == col.name);
                if ui.add_enabled(!used, egui::Button::new(&col.name)).clicked() {
                    if let Some(c) = FilterCondition::for_column(table, &col.name) {
                        conditions.push(c);
                    }
                    ui.close_menu();
                }
            }
        });
    })
    .response
    .on_hover_text("Add a condition on another column");
    if conditions.is_empty() {
        ui.label(RichText::new("Add a condition on any column to narrow what is shown").weak().small());
    }
    *conditions != before
}

/// A condition as a form: a scrolling list of level check boxes with "all"
/// and "none", or a "from" and a "to" slider, each on its own line.
fn condition_form(
    ui: &mut egui::Ui,
    c: &mut FilterCondition,
    levels: &[String],
    range: (f64, f64),
    id: impl std::hash::Hash,
) {
    match c {
        FilterCondition::Levels { levels: chosen, .. } => {
            ui.horizontal(|ui| {
                if ui.small_button("all").clicked() {
                    *chosen = levels.to_vec();
                }
                if ui.small_button("none").clicked() {
                    chosen.clear();
                }
                ui.label(RichText::new(format!("{} of {} levels", chosen.len(), levels.len())).weak().small());
            });
            egui::ScrollArea::vertical().id_salt(("filter_panel_levels", id)).max_height(160.0).show(ui, |ui| {
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
            });
        }
        FilterCondition::Range { min, max, .. } => {
            let (lo, hi) = range;
            let speed = ((hi - lo) / 200.0).max(1e-9);
            let width = (ui.available_width() - 60.0).max(80.0);
            for (label, value) in [("from", &mut *min), ("to", &mut *max)] {
                ui.horizontal(|ui| {
                    ui.add_sized([32.0, 18.0], egui::Label::new(label));
                    ui.spacing_mut().slider_width = width - 70.0;
                    ui.add(
                        egui::Slider::new(value, lo..=hi).drag_value_speed(speed).clamping(egui::SliderClamping::Never),
                    );
                });
            }
            if *min > *max {
                std::mem::swap(min, max);
            }
        }
    }
}

/// Previous, Play or Pause, Next, the seconds per step and where the
/// stepping stands. Takes the automatic step when one is due.
fn step_controls(
    ui: &mut egui::Ui,
    c: &mut FilterCondition,
    levels: &[String],
    range: (f64, f64),
    state: &mut FilterPanelState,
) {
    let mut step = 0;
    ui.horizontal_wrapped(|ui| {
        if ui.small_button("◀ Previous").on_hover_text("Show the previous level (or window)").clicked() {
            step = -1;
            state.playing = false;
        }
        let (label, hint) = if state.playing {
            ("⏸ Pause", "Stop stepping")
        } else {
            ("▶ Play", "Step on by itself, one step at a time")
        };
        if ui.small_button(label).on_hover_text(hint).clicked() {
            state.playing = !state.playing;
            state.last_step = ui.input(|i| i.time);
        }
        if ui.small_button("Next ▶").on_hover_text("Show the next level (or window)").clicked() {
            step = 1;
            state.playing = false;
        }
        let mut seconds = state.seconds();
        let speed = ui
            .add(egui::DragValue::new(&mut seconds).range(0.2..=10.0).speed(0.05).suffix(" s").max_decimals(1))
            .on_hover_text("Seconds each step stays on screen while playing");
        speed.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, "Seconds per step"));
        if speed.changed() {
            state.seconds = seconds;
        }
    });
    if state.playing {
        let now = ui.input(|i| i.time);
        let due = state.last_step + f64::from(state.seconds());
        if now >= due {
            step = 1;
            state.last_step = now;
        }
        ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64((due - now).max(0.02)));
    }
    if step != 0 {
        *c = c.stepped(levels, range, step);
    }
    let position = c.step_position(levels, range);
    let text = match (position, &*c) {
        (Some((i, n)), FilterCondition::Levels { levels: chosen, .. }) => {
            format!("Level {i} of {n}: {}", chosen.first().map_or("", String::as_str))
        }
        (Some((i, n)), FilterCondition::Range { column, min, max }) => {
            format!("Window {i} of {n}: {} ≤ {column} ≤ {}", number(*min), number(*max))
        }
        (None, _) => "Press Next or Play to start".to_owned(),
    };
    ui.label(RichText::new(text).weak().small());
}

/// A window limit to six decimals at most, without trailing zeros (so the
/// half-unit steps of a column of whole numbers read as such).
fn number(v: f64) -> String {
    let rounded = (v * 1e6).round() / 1e6;
    if rounded == 0.0 { "0".to_owned() } else { rounded.to_string() }
}

/// Smallest and largest finite value of a numeric column (`(0, 1)` without
/// any; a single value is widened to one unit).
pub fn data_range(table: &DataTable, column: &str) -> (f64, f64) {
    let values = table.find(column).and_then(|i| table.column(i)).and_then(|c| c.as_numeric()).unwrap_or(&[]);
    let (lo, hi) = values
        .iter()
        .filter(|v| v.is_finite())
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &v| (a.min(v), b.max(v)));
    if lo.is_finite() { (lo, if hi > lo { hi } else { lo + 1.0 }) } else { (0.0, 1.0) }
}
