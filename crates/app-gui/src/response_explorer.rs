//! The Response Explorer of a fitted linear model: one panel per factor showing
//! the predicted response (with its confidence band) as that factor varies and
//! every other factor stays at its current setting.
//!
//! Each continuous panel has a vertical line at the factor's setting; dragging
//! or clicking in the panel moves it, and every panel and the predicted value
//! update at once. Categorical panels show the prediction at each level with its
//! confidence interval; clicking a level selects it. Exact values can be typed
//! below each panel. All panels share one vertical scale, so the size of each
//! factor's effect can be compared directly. The panels do not pan or zoom.
//!
//! One grip under the panels resizes them all together (the strip changes
//! every panel's height, its corner and the strip beside the last panel the
//! width); double-clicking it restores the default size. The size is kept for
//! each report while the application runs.
//!
//! Any fitted model implementing [`app_model::SurfaceModel`] can be explored:
//! linear models, response surfaces, logistic and Poisson models, penalized
//! fits (no bands) and mixtures (moving one component rescales the others).
//! The model logic (predictions, intervals, ranges) is
//! [`app_model::surface_profiler::profile`]; this module only draws it.

use crate::dialog_kit::ComboExt as _;
use std::sync::Arc;

use app_model::axis_settings::PlotAxes;
use app_model::format::format_number;
use app_model::report_figure::{FigureLayout, linked_profile_figure, profile_figure};
use app_model::surface_profiler::{profile, settings_fit};
use app_model::{Profile, ProfilePanel, SurfaceModel, TermValue};
use eframe::egui::{self, CursorIcon, RichText};
use egui_plot::{
    AxisHints, GridMark, HLine, Line, LineStyle, MarkerShape, Plot, PlotBounds, PlotPoints, Points, VLine,
};

use crate::figure_page_window::{PanelMaker, PanelRequest};
use crate::plot_axes::{PlotChrome, Spacer};
use crate::plot_size::{GripFrame, PlotSize, shared_grip, visible_plot_width};
use crate::theme;

const PANEL_HEIGHT: f32 = 200.0;
const MIN_PANEL_WIDTH: f32 = 170.0;

/// Where the size of explorer `id`'s panels is kept (`linked` tells the
/// linked explorer of several responses from the single one).
fn size_id(id: u64, linked: bool) -> egui::Id {
    egui::Id::new(("response_explorer_size", id, linked))
}

/// Width the panels may take: the available width, but no more than the
/// visible part of the pane, so the shared grip's corner and width handle
/// stay in view.
fn room(ui: &egui::Ui) -> f32 {
    ui.available_width().min(visible_plot_width(ui))
}

/// Lay out panels side by side within [`room`], wrapping onto more rows.
fn panel_rows(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    let room = room(ui);
    ui.scope(|ui| {
        ui.set_max_width(room);
        ui.horizontal_wrapped(add);
    });
}

/// Width and height of each panel: the size chosen with the shared grip,
/// otherwise `count` panels filling the [`room`] at `default_height`. Never
/// wider than the room.
fn panel_size(ui: &egui::Ui, size: &PlotSize, count: usize, default_height: f32) -> (f32, f32) {
    let n = count.max(1) as f32;
    let gap = ui.spacing().item_spacing.x;
    let available = room(ui);
    let fill = ((available - gap * (n - 1.0)) / n).floor();
    let width = size.width.map_or(fill, |w| w.min(available)).max(MIN_PANEL_WIDTH);
    (width, size.height.unwrap_or(default_height))
}

/// Panels of `width` that fit across the [`room`] (at least one, at
/// most `count`).
fn columns_that_fit(ui: &egui::Ui, width: f32, count: usize) -> usize {
    let gap = ui.spacing().item_spacing.x;
    let columns = ((room(ui) + gap) / (width + gap)).floor().max(1.0) as usize;
    columns.min(count.max(1))
}

/// The grip under every panel (which together cover `area`, `panels`
/// across and down), shared so a drag resizes them all alike.
fn panels_grip(
    ui: &mut egui::Ui,
    size: &mut PlotSize,
    key: egui::Id,
    panel: (f32, f32),
    area: egui::Rect,
    panels: [usize; 2],
) {
    let frame =
        GripFrame { size: egui::vec2(panel.0, panel.1), area, panels, min_width: MIN_PANEL_WIDTH, what: "panels" };
    // The handles are widgets of this window (a report can move between
    // windows); the size is kept per report.
    let grip_id = ui.make_persistent_id(key);
    shared_grip(ui, size, grip_id, &frame);
    size.store(ui.ctx(), key);
}

/// Draw the explorer for `analysis`, reading and updating `settings` (one
/// value per model term; reset to the defaults when they no longer fit).
pub fn show(ui: &mut egui::Ui, analysis: &dyn SurfaceModel, settings: &mut Vec<TermValue>, id: u64) {
    if !settings_fit(analysis, settings) {
        *settings = analysis.default_profile_settings();
    }
    let profile = match profile(analysis, settings) {
        Ok(p) => p,
        Err(e) => {
            ui.colored_label(theme::ERROR, e.to_string());
            return;
        }
    };
    let fmt = |v: f64| format_number(v, 6);
    ui.horizontal_wrapped(|ui| {
        ui.label(
            RichText::new(format!("Predicted {} = {}", profile.response, fmt(profile.predicted)))
                .strong()
                .size(15.0 * theme::text_scale(ui)),
        );
        if let (Some((lo, hi)), Some(level)) = (profile.confidence_interval, profile.confidence_level) {
            ui.label(format!("{}% confidence interval [{}, {}]", (level * 100.0).round(), fmt(lo), fmt(hi)));
        }
        if ui
            .small_button("Reset settings")
            .on_hover_text(
                "Back to the starting settings: continuous factors at their means, categorical factors at their \
                 most common level (a mixture at the average blend)",
            )
            .clicked()
        {
            *settings = analysis.default_profile_settings();
        }
    });
    if let Some(note) = &profile.note {
        ui.label(RichText::new(note).weak().small());
    }
    ui.label(
        RichText::new(
            "Drag or click in a panel to change that factor (or type a value below it); the others stay where they are.",
        )
        .weak()
        .small(),
    );

    let key = size_id(id, false);
    let mut size = PlotSize::load(ui.ctx(), key);
    let (width, height) = panel_size(ui, &size, profile.panels.len(), PANEL_HEIGHT);
    let (y0, y1) = profile.y_range;
    // Right-clicking any panel saves the whole explorer as a figure, at the
    // panels' on-screen size and layout.
    let columns = columns_that_fit(ui, width, profile.panels.len());
    let rows_of_panels = profile.panels.len().div_ceil(columns).max(1);
    let layout = FigureLayout::matching_screen(
        f64::from(width) * columns as f64,
        f64::from(height) * rows_of_panels as f64,
        None,
    );
    let menu = |ui: &mut egui::Ui| {
        let maker = || -> PanelMaker {
            let profile = profile.clone();
            Arc::new(move |format, r: PanelRequest| {
                let title = if r.titled { "Response Explorer" } else { "" };
                let layout = FigureLayout { width: r.width, height: r.height, ..FigureLayout::default() };
                Some(profile_figure(&profile, title, columns, format, layout))
            })
        };
        crate::report_view::save_figure_items_with(
            ui,
            id,
            "Response Explorer",
            "Every panel at its current settings, laid out as on screen.",
            &|format| Some(profile_figure(&profile, "Response Explorer", columns, format, layout.clone())),
            Some(&maker),
        );
    };
    let mut area = egui::Rect::NOTHING;
    panel_rows(ui, |ui| {
        for (j, panel) in profile.panels.iter().enumerate() {
            let cell = ui.vertical(|ui| {
                ui.set_width(width);
                let new_value = match panel {
                    ProfilePanel::Continuous { name, curve, band, range, value } => continuous_panel(
                        ui,
                        (id, j),
                        name,
                        &profile.response,
                        curve,
                        band,
                        *range,
                        *value,
                        profile.predicted,
                        (y0, y1),
                        (width, height),
                        true,
                        &menu,
                    ),
                    ProfilePanel::Categorical { name, levels, predictions, selected } => categorical_panel(
                        ui,
                        (id, j),
                        name,
                        &profile.response,
                        levels,
                        predictions,
                        *selected,
                        profile.predicted,
                        (y0, y1),
                        (width, height),
                        true,
                        &menu,
                    ),
                };
                if let Some(v) = new_value {
                    *settings = analysis.with_setting(settings, j, v);
                }
            });
            area = area.union(cell.response.rect);
        }
    });
    panels_grip(ui, &mut size, key, (width, height), area, [columns, rows_of_panels]);
}

/// Height of each panel in the linked explorer, where several rows stack.
const LINKED_PANEL_HEIGHT: f32 = 150.0;

/// The linked explorer of several responses over the same factors: one row
/// of panels per profile (each on its own vertical scale), every row at the
/// same settings, so moving a factor in any row moves it in all of them.
/// Values can be typed below the last row. Returns the factor that was
/// changed and its new value. Right-clicking a panel saves every row as one
/// figure.
pub fn linked(ui: &mut egui::Ui, profiles: &[Profile], id: u64) -> Option<(usize, TermValue)> {
    let fmt = |v: f64| format_number(v, 6);
    ui.horizontal_wrapped(|ui| {
        for (i, p) in profiles.iter().enumerate() {
            if i > 0 {
                ui.label(RichText::new("·").weak());
            }
            let text = match p.confidence_interval {
                Some((lo, hi)) => format!("{} = {} [{}, {}]", p.response, fmt(p.predicted), fmt(lo), fmt(hi)),
                None => format!("{} = {}", p.response, fmt(p.predicted)),
            };
            ui.label(RichText::new(text).strong());
        }
    });
    if let Some(level) = profiles.iter().find_map(|p| p.confidence_level) {
        ui.label(
            RichText::new(format!(
                "Brackets: {}% confidence intervals. Drag or click in any panel to change that factor in every row.",
                (level * 100.0).round()
            ))
            .weak()
            .small(),
        );
    }
    let n = profiles.first().map_or(1, |p| p.panels.len()).max(1);
    let key = size_id(id, true);
    let mut size = PlotSize::load(ui.ctx(), key);
    let (width, height) = panel_size(ui, &size, n, LINKED_PANEL_HEIGHT);
    let columns = columns_that_fit(ui, width, n);
    let wraps = columns < n;
    let layout =
        FigureLayout::matching_screen(f64::from(width) * n as f64, f64::from(height) * profiles.len() as f64, None);
    let menu = |ui: &mut egui::Ui| {
        let maker = || -> PanelMaker {
            let profiles = profiles.to_vec();
            Arc::new(move |format, r: PanelRequest| {
                let title = if r.titled { "Linked Response Explorer" } else { "" };
                let layout = FigureLayout { width: r.width, height: r.height, ..FigureLayout::default() };
                Some(linked_profile_figure(&profiles, title, format, layout))
            })
        };
        crate::report_view::save_figure_items_with(
            ui,
            id,
            "Linked Response Explorer",
            "Every response's row of panels at the current settings.",
            &|format| Some(linked_profile_figure(profiles, "Linked Response Explorer", format, layout.clone())),
            Some(&maker),
        );
    };
    let mut change = None;
    let mut area = egui::Rect::NOTHING;
    let last = profiles.len().saturating_sub(1);
    for (row, profile) in profiles.iter().enumerate() {
        let (y0, y1) = profile.y_range;
        // Typed inputs under the last row only (under every row if the row wraps).
        let inputs = row == last || wraps;
        panel_rows(ui, |ui| {
            for (j, panel) in profile.panels.iter().enumerate() {
                let key = (id, row * 1000 + j);
                let cell = ui.vertical(|ui| {
                    ui.set_width(width);
                    let size = (width, height);
                    let new_value = match panel {
                        ProfilePanel::Continuous { name, curve, band, range, value } => continuous_panel(
                            ui,
                            key,
                            name,
                            &profile.response,
                            curve,
                            band,
                            *range,
                            *value,
                            profile.predicted,
                            (y0, y1),
                            size,
                            inputs,
                            &menu,
                        ),
                        ProfilePanel::Categorical { name, levels, predictions, selected } => categorical_panel(
                            ui,
                            key,
                            name,
                            &profile.response,
                            levels,
                            predictions,
                            *selected,
                            profile.predicted,
                            (y0, y1),
                            size,
                            inputs,
                            &menu,
                        ),
                    };
                    if let Some(v) = new_value {
                        change = Some((j, v));
                    }
                });
                area = area.union(cell.response.rect);
            }
        });
    }
    let down = profiles.len() * n.div_ceil(columns);
    panels_grip(ui, &mut size, key, (width, height), area, [columns, down.max(1)]);
    change
}

/// A plot with every built-in navigation turned off. (A macro, so the plot's
/// type never has to be written out.)
macro_rules! fixed_plot {
    ($id:expr, $width:expr, $height:expr, $x_label:expr, $y_label:expr) => {
        Plot::new($id)
            .width($width)
            .height($height)
            .allow_drag(false)
            .allow_zoom(false)
            .allow_scroll(false)
            .allow_boxed_zoom(false)
            .allow_double_click_reset(false)
            // Denser tick labels than the defaults: the panels are small.
            .custom_x_axes(vec![AxisHints::new_x().label($x_label).label_spacing(36.0..=48.0)])
            .custom_y_axes(vec![AxisHints::new_y().label($y_label).label_spacing(14.0..=20.0)])
            // Grid lines are painted behind the plot by `PlotChrome`.
            .show_grid([false, false])
            .x_grid_spacer(x_marks)
            .y_grid_spacer(nice_marks)
            .x_axis_formatter(|mark, _| tick_label(mark.value))
            .y_axis_formatter(|mark, _| tick_label(mark.value))
    };
}

/// A tick label: up to six significant digits, no "−0".
fn tick_label(v: f64) -> String {
    format_number(if v.abs() < 1e-12 { 0.0 } else { v }, 6)
}

/// Lower and upper curves of a band.
type Band = (Vec<[f64; 2]>, Vec<[f64; 2]>);

/// At most five round (1-2-5) grid lines across the visible range, all
/// labelled: fewer when the labels would not fit between them (egui_plot
/// leaves out a label wider than its spacing, which on a narrow panel would
/// drop them all).
fn nice_marks(input: egui_plot::GridInput) -> Vec<GridMark> {
    fitted_marks(input, |_| 20.0)
}

/// Marks along the horizontal axis, spaced for their labels' widths.
fn x_marks(input: egui_plot::GridInput) -> Vec<GridMark> {
    // Tick labels are about 7.5 points a character, plus a gap.
    fitted_marks(input, |label| label.chars().count() as f64 * 7.5 + 12.0)
}

/// Round marks at least `room(label)` points apart for every label shown.
fn fitted_marks(input: egui_plot::GridInput, room: impl Fn(&str) -> f64) -> Vec<GridMark> {
    let (lo, hi) = input.bounds;
    if hi.is_nan() || lo.is_nan() || hi <= lo {
        return Vec::new();
    }
    // egui_plot's base step is the span of its smallest grid spacing (8 points).
    let units_per_point = input.base_step_size / 8.0;
    let marks = |step: f64| -> Vec<f64> {
        let mut k = (lo / step).ceil();
        let mut out = Vec::new();
        while k * step <= hi && out.len() < 20 {
            // `+ 0.0` turns −0 into 0.
            out.push(k * step + 0.0);
            k += 1.0;
        }
        out
    };
    let mut step = app_model::axis_settings::nice_step((hi - lo) / 5.0);
    for _ in 0..12 {
        let need = marks(step).iter().map(|&v| room(&tick_label(v))).fold(0.0_f64, f64::max);
        if units_per_point.is_nan() || units_per_point <= 0.0 || step / units_per_point >= need {
            break;
        }
        step = app_model::axis_settings::nice_step(step * 1.0001);
    }
    marks(step).into_iter().map(|value| GridMark { value, step_size: step }).collect()
}

/// Name a panel for screen readers (and tests): "Profile of <response> by
/// <factor>".
fn panel_name(panel: &egui::Response, response: &str, factor: &str) {
    panel.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Other, true, format!("Profile of {response} by {factor}"))
    });
}

#[allow(clippy::too_many_arguments)]
fn continuous_panel(
    ui: &mut egui::Ui,
    id: (u64, usize),
    name: &str,
    response: &str,
    curve: &[[f64; 2]],
    band: &Option<Band>,
    (lo, hi): (f64, f64),
    value: f64,
    predicted: f64,
    (y0, y1): (f64, f64),
    size: (f32, f32),
    inputs: bool,
    menu: &dyn Fn(&mut egui::Ui),
) -> Option<TermValue> {
    let pal = theme::plot_palette(ui);
    let pad = ((hi - lo) * 0.02).max(1e-9);
    let chrome = PlotChrome::begin(ui);
    let plot = fixed_plot!(("profile", id.0, id.1), size.0, size.1, chrome.title(name), chrome.title(response)).show(
        ui,
        |plot| {
            plot.set_plot_bounds(PlotBounds::from_min_max([lo - pad, y0], [hi + pad, y1]));
            for edge in band.iter().flat_map(|(lower, upper)| [lower, upper]) {
                plot.line(
                    Line::new(PlotPoints::from(edge.clone()))
                        .color(pal.band)
                        .style(LineStyle::dashed_loose())
                        .name("Confidence band"),
                );
            }
            plot.line(
                Line::new(PlotPoints::from(curve.to_vec())).color(pal.fit_line).width(2.0_f32).name("Prediction"),
            );
            plot.hline(
                HLine::new(predicted).color(pal.divider).style(LineStyle::dashed_dense()).name("Current prediction"),
            );
            plot.vline(VLine::new(value).color(pal.selected).width(2.0_f32).name("Setting"));
        },
    );
    chrome.finish(ui, &plot.transform, &PlotAxes::default(), false, Some(&(Box::new(nice_marks) as Spacer)));
    panel_name(&plot.response, response, name);
    plot.response.context_menu(|ui| menu(ui));
    let mut out = None;
    let r = &plot.response;
    if r.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
    }
    if r.dragged() || r.clicked() {
        if let Some(pos) = r.interact_pointer_pos() {
            let x = plot.transform.value_from_position(pos).x.clamp(lo, hi);
            out = Some(TermValue::Number(x));
        }
    }
    if !inputs {
        return out;
    }
    ui.horizontal(|ui| {
        ui.label(format!("{name}:"));
        let mut v = value;
        let speed = ((hi - lo) / 200.0).max(f64::EPSILON);
        if ui.add(egui::DragValue::new(&mut v).speed(speed).range(lo..=hi)).changed() {
            out = Some(TermValue::Number(v));
        }
    });
    out
}

#[allow(clippy::too_many_arguments)]
fn categorical_panel(
    ui: &mut egui::Ui,
    id: (u64, usize),
    name: &str,
    response: &str,
    levels: &[String],
    predictions: &[(f64, Option<(f64, f64)>)],
    selected: usize,
    predicted: f64,
    (y0, y1): (f64, f64),
    size: (f32, f32),
    inputs: bool,
    menu: &dyn Fn(&mut egui::Ui),
) -> Option<TermValue> {
    let pal = theme::plot_palette(ui);
    let k = levels.len();
    let positions: Vec<f64> = (1..=k).map(|i| i as f64).collect();
    let labels: Vec<String> = levels.to_vec();
    let ticks = positions.clone();
    let chrome = PlotChrome::begin(ui);
    let plot = fixed_plot!(("profile", id.0, id.1), size.0, size.1, chrome.title(name), chrome.title(response))
        .x_grid_spacer(move |_input| ticks.iter().map(|&value| GridMark { value, step_size: 1e6 }).collect())
        .x_axis_formatter(move |mark, _range| {
            let i = mark.value.round() as usize;
            if (mark.value - i as f64).abs() < 1e-9 && (1..=labels.len()).contains(&i) {
                labels[i - 1].clone()
            } else {
                String::new()
            }
        })
        .show(ui, |plot| {
            plot.set_plot_bounds(PlotBounds::from_min_max([0.5, y0], [k as f64 + 0.5, y1]));
            plot.hline(
                HLine::new(predicted).color(pal.divider).style(LineStyle::dashed_dense()).name("Current prediction"),
            );
            plot.vline(VLine::new((selected + 1) as f64).color(pal.selected).width(1.0_f32).name("Setting"));
            for (i, &(_, limits)) in predictions.iter().enumerate() {
                let Some((lo, hi)) = limits else { continue };
                let x = (i + 1) as f64;
                plot.line(
                    Line::new(PlotPoints::from(vec![[x, lo], [x, hi]]))
                        .color(pal.band)
                        .width(2.0_f32)
                        .name("Confidence interval"),
                );
            }
            let others: Vec<[f64; 2]> = predictions
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != selected)
                .map(|(i, p)| [(i + 1) as f64, p.0])
                .collect();
            plot.points(Points::new(PlotPoints::from(others)).radius(4.0_f32).color(pal.fit_line).name("Prediction"));
            if let Some(p) = predictions.get(selected) {
                plot.points(
                    Points::new(PlotPoints::from(vec![[(selected + 1) as f64, p.0]]))
                        .shape(MarkerShape::Diamond)
                        .radius(6.0_f32)
                        .color(pal.selected)
                        .name("Setting"),
                );
            }
        });
    chrome.finish(ui, &plot.transform, &PlotAxes::default(), true, Some(&(Box::new(nice_marks) as Spacer)));
    panel_name(&plot.response, response, name);
    plot.response.context_menu(|ui| menu(ui));
    let mut out = None;
    let r = &plot.response;
    if r.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    if r.dragged() || r.clicked() {
        if let Some(pos) = r.interact_pointer_pos() {
            let x = plot.transform.value_from_position(pos).x.round().clamp(1.0, k as f64);
            let level = x as usize - 1;
            if level != selected {
                out = Some(TermValue::Level(level));
            }
        }
    }
    if !inputs {
        return out;
    }
    ui.horizontal(|ui| {
        ui.label(format!("{name}:"));
        let mut choice = selected;
        egui::ComboBox::from_id_salt(("profile_level", id.0, id.1))
            .selected_text(levels.get(selected).map_or("", String::as_str))
            .show_list(ui, |ui| {
                for (i, l) in levels.iter().enumerate() {
                    ui.selectable_value(&mut choice, i, l);
                }
            });
        if choice != selected {
            out = Some(TermValue::Level(choice));
        }
    });
    out
}
