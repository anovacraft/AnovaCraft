//! The interactive parts of a several-responses report: the linked Response
//! Explorer (one row per response plus the overall desirability), the goals
//! editor, the constraints and best-settings search, and the simulation
//! setup with its specification region.
//!
//! The analysis ([`app_model::MultipleResponsesAnalysis`]) does the work;
//! this module keeps the results of the slower searches (best settings,
//! simulation) between frames and draws the controls between the report's
//! sections.

use crate::dialog_kit::ComboExt as _;
use app_model::format::format_number;
use app_model::multiple_responses::MAX_RUNS;
use app_model::surface_profiler::ProfileFactorKind;
use app_model::{
    Desirability, FactorVariation, Goal, MultipleResponsesAnalysis, MultipleResponsesRequest, ReportSection,
    SectionContent, SectionKind, Simulation, SimulationSetup, Solution, TermValue,
};
use eframe::egui::{self, RichText};
use stat_data::DataTable;

use crate::dialog_kit as kit;
use crate::{response_explorer, theme};

/// One simulation and what it was run with.
struct SimulationRun {
    settings: Vec<TermValue>,
    setup: SimulationSetup,
    result: Result<Simulation, String>,
}

/// View state of a several-responses report.
#[derive(Default)]
pub struct SeveralView {
    /// Result of the last best-settings search, and the request (without
    /// settings or simulation) and data revisions it was made for.
    best: Option<Result<Vec<Solution>, String>>,
    best_for: Option<(MultipleResponsesRequest, (u64, u64))>,
    simulation: Option<SimulationRun>,
    /// Goals being edited (committed to the request only when valid).
    goals: Vec<Desirability>,
    goal_error: Option<String>,
    /// Message from the last simulation action.
    message: Option<String>,
    /// Constraints being edited (committed to the request only when they
    /// parse), and why they do not.
    constraints: Option<String>,
    constraint_error: Option<String>,
    /// Report sections, and the settings and generation they were built at.
    cache: Option<(Vec<TermValue>, u64, Vec<ReportSection>)>,
    generation: u64,
}

/// The request without the parts that do not need a refit.
pub fn without_settings(request: &MultipleResponsesRequest) -> MultipleResponsesRequest {
    MultipleResponsesRequest { settings: Vec::new(), ..request.clone() }
}

fn search_key(request: &MultipleResponsesRequest) -> MultipleResponsesRequest {
    MultipleResponsesRequest { settings: Vec::new(), simulation: None, ..request.clone() }
}

impl SeveralView {
    /// After the models were (re)fitted at data revisions `revisions`: drop
    /// a best-settings search made for other goals or data, and reload the
    /// goals editor.
    pub fn refitted(&mut self, analysis: &Result<MultipleResponsesAnalysis, String>, revisions: (u64, u64)) {
        self.generation += 1;
        self.cache = None;
        let Ok(a) = analysis else {
            self.best = None;
            return;
        };
        let key = (search_key(a.request()), revisions);
        if self.best_for.as_ref() != Some(&key) {
            self.best = None;
            self.best_for = Some(key);
        }
        self.goals = a.goals().to_vec();
        self.goal_error = None;
        if self.constraint_error.is_none() {
            self.constraints = None;
        }
    }

    /// Run the simulation again if the settings or setup changed (not while
    /// a pointer button is held, so dragging stays smooth), and rebuild the
    /// sections if anything changed.
    pub fn prepare(&mut self, analysis: &MultipleResponsesAnalysis, pointer_down: bool) {
        match &analysis.request().simulation {
            None => {
                if self.simulation.take().is_some() {
                    self.generation += 1;
                }
            }
            Some(setup) => {
                let stale =
                    self.simulation.as_ref().is_none_or(|s| s.settings != analysis.settings() || &s.setup != setup);
                if stale && !pointer_down {
                    self.simulation = Some(SimulationRun {
                        settings: analysis.settings().to_vec(),
                        setup: setup.clone(),
                        result: analysis.simulate(analysis.settings()).map_err(|e| e.to_string()),
                    });
                    self.generation += 1;
                }
            }
        }
        let fresh = matches!(&self.cache, Some((s, g, _)) if s == analysis.settings() && *g == self.generation);
        if !fresh {
            let sections = self.build_sections(analysis);
            self.cache = Some((analysis.settings().to_vec(), self.generation, sections));
        }
    }

    fn build_sections(&self, analysis: &MultipleResponsesAnalysis) -> Vec<ReportSection> {
        let best = self.best.as_ref().map(|b| b.as_deref().map_err(String::as_str));
        let sim = self.simulation.as_ref().map(|s| s.result.as_ref().map_err(String::as_str));
        let mut sections: Vec<ReportSection> = analysis
            .sections_with(best, sim)
            .into_iter()
            // The explorer and the goals editor show these live.
            .filter(|s| !matches!(s.kind, SectionKind::CurrentSettings | SectionKind::DesirabilityTable))
            .collect();
        // Anchors for the controls drawn before them.
        let note = |kind, title: &str, text: &str| ReportSection {
            kind,
            title: title.to_owned(),
            content: SectionContent::Note(text.to_owned()),
        };
        if !sections.iter().any(|s| s.kind == SectionKind::BestSettings) {
            let text = if analysis.has_goal() {
                "Click “Find best settings” above to search the factor ranges for the highest overall desirability."
            } else {
                "Give at least one response a goal above to search for the best settings."
            };
            let at = sections.iter().position(|s| s.kind == SectionKind::SimulationSummary).unwrap_or(sections.len());
            sections.insert(at, note(SectionKind::BestSettings, "Best Settings", text));
        }
        if !sections.iter().any(|s| s.kind == SectionKind::SimulationSummary) {
            let text = if analysis.request().simulation.is_some() {
                "The simulation runs when you release the pointer."
            } else {
                "Turn on the simulation above to see how variation in the factors and noise in the responses \
                 spread the results, and how often they fall outside specification limits."
            };
            sections.push(note(SectionKind::SimulationSummary, "Simulation", text));
        }
        sections
    }

    /// Keep the result of a best-settings search.
    pub fn set_best(&mut self, best: Result<Vec<Solution>, String>) {
        self.best = Some(best);
        self.generation += 1;
    }

    /// The sections built by [`Self::prepare`].
    pub fn sections(&self) -> Vec<ReportSection> {
        self.cache.as_ref().map(|(_, _, s)| s.clone()).unwrap_or_default()
    }
}

/// Controls drawn before the section of kind `kind`: the explorer and goals
/// before the best settings, the simulation setup before its results.
pub fn before_section(
    ui: &mut egui::Ui,
    kind: SectionKind,
    request: &mut MultipleResponsesRequest,
    analysis: &mut MultipleResponsesAnalysis,
    view: &mut SeveralView,
    id: u64,
) {
    let header = |title: &str, salt: &str| {
        egui::CollapsingHeader::new(RichText::new(title).strong())
            .id_salt(("section", id, salt.to_owned()))
            .default_open(true)
    };
    match kind {
        SectionKind::BestSettings => {
            header("Linked Response Explorer", "linked_explorer")
                .show(ui, |ui| explorer(ui, request, analysis, view, id));
            header("Goals (desirability)", "goals").show(ui, |ui| goals_editor(ui, request, analysis, view, id));
            egui::CollapsingHeader::new(RichText::new("Constraints on the factors").strong())
                .id_salt(("section", id, "constraints".to_owned()))
                .default_open(!request.constraints.trim().is_empty())
                .show(ui, |ui| constraints_editor(ui, request, analysis, view));
        }
        SectionKind::SimulationSummary => {
            header("Simulation Setup", "simulation_setup").show(ui, |ui| simulation_editor(ui, request, analysis, id));
        }
        _ => {}
    }
}

/// The linear constraints and excluded combinations the best settings must
/// meet, committed to the request whenever they parse.
fn constraints_editor(
    ui: &mut egui::Ui,
    request: &mut MultipleResponsesRequest,
    analysis: &MultipleResponsesAnalysis,
    view: &mut SeveralView,
) {
    let text = view.constraints.get_or_insert_with(|| request.constraints.clone());
    let before = text.clone();
    crate::design_dialog::constraints_box(ui, text);
    kit::note(ui, "Find best settings and Find robust settings keep to these limits, written in the factors' units.");
    if *text != before {
        let continuous: Vec<stat_doe::Factor> = analysis
            .factors()
            .iter()
            .filter_map(|f| match f.kind {
                ProfileFactorKind::Continuous { range: (lo, hi) } => {
                    Some(stat_doe::Factor::new(f.name.clone(), lo, hi))
                }
                ProfileFactorKind::Categorical { .. } => None,
            })
            .collect();
        let parsed = if text.trim().is_empty() {
            Ok(())
        } else if continuous.is_empty() {
            Err("constraints need continuous factors".to_owned())
        } else {
            stat_doe::Region::parse(text, &continuous).map(|_| ()).map_err(|e| e.to_string())
        };
        match parsed {
            Ok(()) => {
                view.constraint_error = None;
                request.constraints = text.clone();
            }
            Err(e) => view.constraint_error = Some(e),
        }
    }
    if let Some(e) = &view.constraint_error {
        ui.colored_label(theme::ERROR, format!("⚠ {e} (not applied)"));
    }
}

fn set_settings(request: &mut MultipleResponsesRequest, analysis: &mut MultipleResponsesAnalysis, s: &[TermValue]) {
    if analysis.set_settings(s) {
        request.settings = analysis.request().settings.clone();
    }
}

fn explorer(
    ui: &mut egui::Ui,
    request: &mut MultipleResponsesRequest,
    analysis: &mut MultipleResponsesAnalysis,
    view: &mut SeveralView,
    id: u64,
) {
    ui.horizontal_wrapped(|ui| {
        if ui
            .button("Find best settings")
            .on_hover_text(
                "Maximize the overall desirability over the factor ranges: a simplex search from many starting \
                 points, trying every combination of categorical levels",
            )
            .clicked()
        {
            view.set_best(analysis.best_settings().map_err(|e| e.to_string()));
        }
        if let Some(Ok(best)) = &view.best {
            ui.label("Set to:");
            for (k, s) in best.iter().enumerate() {
                let label = format!("#{} (D = {})", k + 1, format_number(s.overall, 3));
                if ui.small_button(label).on_hover_text("Move every factor to this solution").clicked() {
                    set_settings(request, analysis, &s.settings);
                }
            }
        }
        if ui
            .small_button("Reset settings")
            .on_hover_text("Continuous factors at their means, categorical factors at their most common level")
            .clicked()
        {
            let d = analysis.default_settings().to_vec();
            set_settings(request, analysis, &d);
        }
    });
    let profiles = match analysis.profiles(analysis.settings()) {
        Ok(p) => p,
        Err(e) => {
            ui.colored_label(theme::ERROR, e.to_string());
            return;
        }
    };
    if let Some((j, v)) = response_explorer::linked(ui, &profiles, id) {
        let mut s = analysis.settings().to_vec();
        s[j] = v;
        set_settings(request, analysis, &s);
    }
}

const GOALS: [(&str, &str); 4] = [
    ("Maximize", "Larger is better: 0 at or below the lower limit, 1 at or above the upper limit"),
    ("Minimize", "Smaller is better: 1 at or below the lower limit, 0 at or above the upper limit"),
    ("Target", "Closest to the target is best: 1 at the target, 0 at or beyond either limit"),
    ("None", "Shown, but left out of the overall desirability"),
];

fn goal_index(goal: Goal) -> usize {
    match goal {
        Goal::Maximize => 0,
        Goal::Minimize => 1,
        Goal::Target(_) => 2,
        Goal::None => 3,
    }
}

fn goals_editor(
    ui: &mut egui::Ui,
    request: &mut MultipleResponsesRequest,
    analysis: &MultipleResponsesAnalysis,
    view: &mut SeveralView,
    id: u64,
) {
    let names = analysis.response_names();
    if view.goals.len() != names.len() {
        view.goals = analysis.goals().to_vec();
    }
    let predicted = analysis.predict(analysis.settings());
    let (d, overall) = analysis.desirability(&predicted);
    let mut changed = false;
    egui::ScrollArea::horizontal()
        .id_salt(("goals_scroll", id))
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
        .show(ui, |ui| {
            egui::Grid::new(("goals", id)).striped(true).spacing([10.0, 4.0]).show(ui, |ui| {
                for h in [
                    "Response",
                    "Goal",
                    "Lower limit",
                    "Target",
                    "Upper limit",
                    "Importance",
                    "Shape",
                    "Shape above target",
                    "Predicted",
                    "Desirability",
                ] {
                    ui.strong(h);
                }
                ui.end_row();
                for (i, g) in view.goals.iter_mut().enumerate() {
                    ui.label(&names[i]);
                    let mut choice = goal_index(g.goal);
                    egui::ComboBox::from_id_salt(("goal", id, i)).selected_text(GOALS[choice].0).show_list(ui, |ui| {
                        for (k, (label, help)) in GOALS.iter().enumerate() {
                            ui.selectable_value(&mut choice, k, *label).on_hover_text(*help);
                        }
                    });
                    if choice != goal_index(g.goal) {
                        g.goal = match choice {
                            0 => Goal::Maximize,
                            1 => Goal::Minimize,
                            2 => Goal::Target(0.5 * (g.low + g.high)),
                            _ => Goal::None,
                        };
                        changed = true;
                    }
                    let active = g.goal != Goal::None;
                    let speed = ((g.high - g.low).abs() / 200.0).max(1e-6);
                    // One grid cell per control: a group (`add_enabled_ui`) would
                    // put several controls in one cell and shift the columns.
                    changed |= ui.add_enabled(active, egui::DragValue::new(&mut g.low).speed(speed)).changed();
                    match &mut g.goal {
                        Goal::Target(t) => changed |= ui.add(egui::DragValue::new(t).speed(speed)).changed(),
                        _ => {
                            ui.label("");
                        }
                    }
                    changed |= ui.add_enabled(active, egui::DragValue::new(&mut g.high).speed(speed)).changed();
                    changed |= ui
                        .add_enabled(active, egui::DragValue::new(&mut g.importance).speed(0.05).range(0.05..=100.0))
                        .on_hover_text("Weight of this response in the overall desirability")
                        .changed();
                    changed |= ui
                        .add_enabled(active, egui::DragValue::new(&mut g.shape).speed(0.05).range(0.05..=20.0))
                        .on_hover_text("1: linear; above 1: only values near the best score well; below 1: lenient")
                        .changed();
                    if matches!(g.goal, Goal::Target(_)) {
                        changed |= ui
                            .add(egui::DragValue::new(&mut g.shape_above).speed(0.05).range(0.05..=20.0))
                            .on_hover_text("Shape between the target and the upper limit")
                            .changed();
                    } else {
                        ui.label("");
                    }
                    ui.monospace(format_number(predicted[i], 6));
                    ui.monospace(if d[i].is_nan() { "—".to_owned() } else { format_number(d[i], 4) });
                    ui.end_row();
                }
            });
        });
    ui.label(
        RichText::new(format!(
            "Overall desirability (weighted geometric mean): {}",
            if overall.is_nan() { "— (no goals)".to_owned() } else { format_number(overall, 4) }
        ))
        .strong(),
    );
    if changed {
        let invalid = view.goals.iter().zip(&names).find_map(|(g, n)| g.validate().err().map(|e| format!("{n}: {e}")));
        match invalid {
            Some(e) => view.goal_error = Some(e),
            None => {
                view.goal_error = None;
                request.goals = view.goals.iter().copied().map(Some).collect();
            }
        }
    }
    if let Some(e) = &view.goal_error {
        ui.colored_label(theme::ERROR, format!("⚠ {e} (not applied)"));
    }
}

const VARIATIONS: [(&str, &str); 4] = [
    ("fixed", "Fixed"),
    ("normal", "Normal (SD)"),
    ("uniform", "Uniform (± half-width)"),
    ("triangular", "Triangular (± half-width)"),
];

fn simulation_editor(
    ui: &mut egui::Ui,
    request: &mut MultipleResponsesRequest,
    analysis: &MultipleResponsesAnalysis,
    id: u64,
) {
    let mut on = request.simulation.is_some();
    if ui
        .checkbox(&mut on, "Simulate variation around the current settings")
        .on_hover_text("Draw factor values around their settings, add noise to each prediction, and count the runs outside specification limits")
        .changed()
    {
        request.simulation = on.then(|| SimulationSetup::new(request.predictors.len(), request.responses.len()));
    }
    let Some(setup) = &mut request.simulation else { return };
    ui.label(RichText::new("Factor variation").strong());
    egui::Grid::new(("sim_factors", id)).striped(true).spacing([10.0, 4.0]).show(ui, |ui| {
        ui.strong("Factor");
        ui.strong("Distribution");
        ui.strong("Spread");
        ui.end_row();
        for (j, (f, v)) in analysis.factors().iter().zip(setup.factors.iter_mut()).enumerate() {
            ui.label(&f.name);
            if !f.is_continuous() {
                ui.label(RichText::new("fixed at its level (categorical)").weak());
                ui.label("");
                ui.end_row();
                continue;
            }
            let mut key = v.key();
            let label = VARIATIONS.iter().find(|(k, _)| *k == key).map_or("", |(_, l)| *l);
            egui::ComboBox::from_id_salt(("variation", id, j)).selected_text(label).show_list(ui, |ui| {
                for (k, l) in VARIATIONS {
                    ui.selectable_value(&mut key, k, l);
                }
            });
            let mut spread = v.spread();
            if key != "fixed" && spread == 0.0 {
                // A starting spread: a twentieth of the factor's range.
                if let app_model::surface_profiler::ProfileFactorKind::Continuous { range: (lo, hi) } = f.kind {
                    spread = ((hi - lo) / 20.0).max(f64::EPSILON);
                }
            }
            ui.add_enabled_ui(key != "fixed", |ui| {
                let speed = (spread / 50.0).max(1e-4);
                ui.add(egui::DragValue::new(&mut spread).speed(speed).range(0.0..=f64::MAX));
            });
            if let Some(new) = FactorVariation::from_key(key, spread) {
                *v = new;
            }
            ui.end_row();
        }
    });
    ui.label(RichText::new("Responses").strong());
    let names = analysis.response_names();
    egui::Grid::new(("sim_responses", id)).striped(true).spacing([10.0, 4.0]).show(ui, |ui| {
        for h in ["Response", "Noise SD", "", "Lower spec limit", "Upper spec limit"] {
            ui.strong(h);
        }
        ui.end_row();
        for (i, name) in names.iter().enumerate() {
            ui.label(name);
            let noise = &mut setup.noise[i];
            let speed = (*noise / 50.0).max(1e-4);
            ui.add(egui::DragValue::new(noise).speed(speed).range(0.0..=f64::MAX));
            let rmse = analysis.models()[i].root_mean_square_error();
            if ui
                .small_button("Use RMSE")
                .on_hover_text(format!("The model's root mean square error, {}", format_number(rmse, 6)))
                .clicked()
            {
                *noise = rmse;
            }
            let predicted = analysis.predict(analysis.settings())[i];
            let (lo, hi) = &mut setup.limits[i];
            limit_input(ui, lo, predicted, ("lsl", id, i));
            limit_input(ui, hi, predicted, ("usl", id, i));
            ui.end_row();
        }
    });
    ui.horizontal(|ui| {
        ui.label("Runs:");
        ui.add(egui::DragValue::new(&mut setup.runs).speed(100.0).range(100..=MAX_RUNS));
        ui.label("Seed:");
        ui.add(crate::dialog_kit::seed_value(&mut setup.seed).speed(1.0));
    });
    region_editor(ui, setup, analysis, id);
}

/// The specification region: a low and high per continuous factor, whose
/// corners are each checked for the probability of meeting every limit.
fn region_editor(ui: &mut egui::Ui, setup: &mut SimulationSetup, analysis: &MultipleResponsesAnalysis, id: u64) {
    let factors = analysis.factors();
    // Each continuous factor's current setting ± a twentieth of its range.
    let around = |settings: &[TermValue]| -> Vec<(f64, f64)> {
        factors
            .iter()
            .zip(settings)
            .map(|(f, s)| match (&f.kind, s) {
                (ProfileFactorKind::Continuous { range: (lo, hi) }, TermValue::Number(x)) => {
                    let h = (hi - lo) / 20.0;
                    ((x - h).max(*lo), (x + h).min(*hi))
                }
                _ => (0.0, 0.0),
            })
            .collect()
    };
    let mut on = setup.region.is_some();
    if ui
        .checkbox(&mut on, "Check a specification region around the settings")
        .on_hover_text(
            "Probability that every response is within its specification limits at each corner of a box of factor \
             settings (the factors' operating ranges), with the worst corner marked",
        )
        .changed()
    {
        setup.region = on.then(|| around(analysis.settings()));
    }
    let Some(region) = &mut setup.region else { return };
    if region.len() != factors.len() {
        *region = around(analysis.settings());
    }
    egui::Grid::new(("spec_region", id)).striped(true).spacing([10.0, 4.0]).show(ui, |ui| {
        ui.strong("Factor");
        ui.strong("Region low");
        ui.strong("Region high");
        ui.end_row();
        for (j, (f, (lo, hi))) in factors.iter().zip(region.iter_mut()).enumerate() {
            ui.label(&f.name);
            let ProfileFactorKind::Continuous { range: (a, b) } = f.kind else {
                ui.label(RichText::new("at its current level (categorical)").weak());
                ui.label("");
                ui.end_row();
                continue;
            };
            let speed = ((b - a) / 200.0).max(1e-6);
            ui.add(egui::DragValue::new(lo).speed(speed)).widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, format!("Region low {}", j + 1))
            });
            ui.add(egui::DragValue::new(hi).speed(speed)).widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, format!("Region high {}", j + 1))
            });
            if *hi < *lo {
                std::mem::swap(lo, hi);
            }
            ui.end_row();
        }
    });
    ui.horizontal(|ui| {
        if ui
            .small_button("Center on the current settings")
            .on_hover_text("Each continuous factor's setting ± a twentieth of its range")
            .clicked()
        {
            *region = around(analysis.settings());
        }
        kit::note(ui, "A factor whose low equals its high stays at its current setting.");
    });
}

/// An optional specification limit: a checkbox and its value.
fn limit_input(ui: &mut egui::Ui, limit: &mut Option<f64>, near: f64, salt: (&str, u64, usize)) {
    ui.horizontal(|ui| {
        ui.push_id(salt, |ui| {
            let mut on = limit.is_some();
            if ui.checkbox(&mut on, "").changed() {
                *limit = on.then_some(if near.is_finite() { near } else { 0.0 });
            }
            if let Some(v) = limit {
                let speed = (v.abs() / 200.0).max(1e-4);
                ui.add(egui::DragValue::new(v).speed(speed));
            }
        });
    });
}

/// Buttons acting on the simulation, drawn after its results: robust
/// settings, and saving the runs as a table.
pub fn after_section(
    ui: &mut egui::Ui,
    kind: SectionKind,
    request: &mut MultipleResponsesRequest,
    analysis: &mut MultipleResponsesAnalysis,
    view: &mut SeveralView,
    new_tables: &mut Vec<(DataTable, bool)>,
) {
    if kind != SectionKind::SimulationSummary || request.simulation.is_none() {
        return;
    }
    ui.horizontal_wrapped(|ui| {
        if ui
            .button("Find robust settings")
            .on_hover_text(
                "Move the continuous factors to where the simulated share of runs outside the specification \
                 limits is lowest (the same random draws at every candidate)",
            )
            .clicked()
        {
            match analysis.robust_settings() {
                Ok(s) => {
                    view.message = None;
                    set_settings(request, analysis, &s);
                }
                Err(e) => view.message = Some(e.to_string()),
            }
        }
        let sim = view.simulation.as_ref().and_then(|s| s.result.as_ref().ok());
        if ui
            .add_enabled(sim.is_some(), egui::Button::new("Save simulated runs as a new table"))
            .on_hover_text("One row per simulated run: the factor values and every response")
            .clicked()
        {
            if let Some(sim) = sim {
                match analysis.simulation_table(sim) {
                    Ok(t) => new_tables.push((t, true)),
                    Err(e) => view.message = Some(e.to_string()),
                }
            }
        }
    });
    if let Some(m) = &view.message {
        ui.colored_label(theme::ERROR, format!("⚠ {m}"));
    }
}
