//! Analysis and data dialogs.

use super::*;

const DOSE: &str = "Dose–response (probit) analysis";

fn numeric_column(at: usize) -> GridRequest {
    GridRequest::InsertColumn { at, kind: stat_data::ColumnKind::Numeric, count: 1, name: String::new() }
}

/// A column cannot be both the dose and the response.
#[test]
fn dose_response_refuses_a_column_in_two_roles() {
    let mut gui = Gui::with_sample("Longley");
    gui.menu(&["Model", "Dose–response (probit)…"]);
    gui.choose(DOSE, 0, "GNP");
    gui.choose(DOSE, 1, "GNP");
    assert!(gui.shows("⚠ Dose, response and trials must be different columns"));
    assert!(gui.widget("Run").is_disabled());
    gui.choose(DOSE, 1, "Unemployed");
    assert!(!gui.shows("⚠ Dose, response and trials must be different columns"));
    assert!(!gui.widget("Run").is_disabled());
}

/// The Distribution dialog lists a column added while it is open (it used
/// to panic drawing a check box it had no state for).
#[test]
fn distribution_dialog_lists_a_column_added_while_open() {
    let mut gui = Gui::with_sample("Longley");
    gui.menu(&["Describe", "Distribution…"]);
    let boxes = |gui: &Gui| gui.window("Distribution").get_all_by_role(Role::CheckBox).count();
    assert_eq!(boxes(&gui), 7);
    gui.menu(&["Edit", "Add column", "Numbers"]);
    assert_eq!(gui.app().documents[gui.app().active].table.n_cols(), 8);
    assert_eq!(boxes(&gui), 8, "the dialog stays open, listing the new column");
    gui.shot("distribution-dialog-new-column");
}

/// Analysis dialogs hold column positions: one inserted before them closes
/// the dialog rather than leaving it pointing at different columns.
#[test]
fn analysis_dialog_closes_when_columns_move() {
    let mut gui = Gui::with_sample("Longley");
    gui.menu(&["Model", "Dose–response (probit)…"]);
    gui.choose(DOSE, 0, "GNP");
    // Appending at the end leaves every chosen column where it was.
    let end = gui.app().documents[gui.app().active].table.n_cols();
    gui.grid(vec![numeric_column(end)]);
    assert!(gui.harness.query_by_role_and_label(Role::Window, DOSE).is_some());
    gui.grid(vec![numeric_column(0)]);
    assert!(gui.harness.query_by_role_and_label(Role::Window, DOSE).is_none());
}

/// The Recode dialog stays on its column when another is inserted before it.
#[test]
fn recode_dialog_follows_its_column() {
    let mut gui = Gui::with_sample("Longley");
    gui.grid(vec![GridRequest::Recode(1)]);
    // The value list starts with the column's smallest value.
    let first_value =
        |gui: &Gui| gui.window("Recode column").get_all_by_role(Role::TextInput).next().and_then(|n| n.value());
    let before = first_value(&gui);
    assert!(before.is_some());
    gui.grid(vec![numeric_column(0)]);
    assert_eq!(first_value(&gui), before, "still recoding the same column, now the third");
    // Undo removes the inserted column again.
    gui.grid(vec![GridRequest::Undo]);
    assert_eq!(first_value(&gui), before, "back in second place");
}

/// The design dialog's buttons sat inside its scrolling settings and were
/// cut off at the window's bottom edge (Plackett–Burman with seven factors).
#[test]
fn design_dialog_buttons_stay_in_view() {
    let mut gui = Gui::with_sample("Longley");
    gui.menu(&["Design", "Plackett–Burman…"]);
    let window = gui.window("Designed experiment").raw_bounds().expect("window position");
    let button = gui.widget("Create").raw_bounds().expect("button position");
    assert!(button.y1 <= window.y1, "button bottom {} below window bottom {}", button.y1, window.y1);
    assert!(gui.harness.query_all_by_label_contains("— 8 runs").next().is_none(), "run count said twice");
    assert_eq!(gui.harness.query_all_by_label_contains("8 runs").count(), 1, "run count said once");
    gui.shot("design-dialog-plackett-burman");
}

/// Analysis of means refuses the same column as response and groups.
#[test]
fn analysis_of_means_refuses_a_column_in_two_roles() {
    const ANOM: &str = "Analysis of means";
    let mut gui = Gui::with_sample("Longley");
    gui.menu(&["Quality", "Analysis of means…"]);
    gui.choose(ANOM, 0, "GNP");
    gui.choose(ANOM, 1, "GNP");
    assert!(gui.shows("⚠ A column is chosen twice"));
    assert!(gui.widget("Run").is_disabled());
    gui.choose(ANOM, 1, "Year");
    assert!(!gui.shows("⚠ A column is chosen twice"));
    assert!(!gui.widget("Run").is_disabled());
}

/// Attribute capability offers only whole-number columns as counts: Longley's
/// Year is offered, its fractional GNP deflator is not.
#[test]
fn attribute_capability_offers_only_count_columns() {
    let mut gui = Gui::with_sample("Longley");
    gui.menu(&["Quality", "Attribute capability…"]);
    gui.window("Attribute capability").get_all_by_role(Role::ComboBox).next().expect("count list").simulate_click();
    gui.run();
    let offered = |label: &str| {
        gui.harness.query_all_by(|n| n.role() != Role::Label && n.label().as_deref() == Some(label)).count() > 0
    };
    assert!(offered("Year"));
    assert!(!offered("GNP deflator"));
}

/// Reliability ▸ Life distributions… fits interval-censored inspection
/// counts: the start and end columns cannot be the same, and the report
/// draws the probability, survival and hazard plots.
#[test]
fn life_distributions_fit_interval_data() {
    const LIFE: &str = "Life distributions";
    let mut gui = Gui::with_sample("Seal inspections");
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui.menu(&["Reliability", "Life distributions…"]);
    assert!(gui.widget("Run").is_disabled(), "no time column chosen yet");
    gui.click("Start and end of the failure interval");
    gui.choose(LIFE, 0, "Last pass (h)");
    gui.choose(LIFE, 1, "Last pass (h)");
    assert!(gui.shows("⚠ A column is chosen twice"));
    assert!(gui.widget("Run").is_disabled());
    gui.choose(LIFE, 1, "Found failed (h)");
    gui.choose(LIFE, 2, "Units");
    assert!(!gui.shows("⚠ A column is chosen twice"));
    gui.click("Run");
    assert!(gui.harness.query_by_role_and_label(Role::Window, LIFE).is_none(), "the dialog closes");
    let reports = &gui.app().documents[active].reports;
    assert_eq!(reports.len(), 1);
    let app_model::SavedReport::LifeDistribution { request } = reports[0].saved() else {
        panic!("not a life-distribution report")
    };
    assert!(matches!(request.layout, app_model::LifeDataLayout::Intervals { .. }));
    assert!(request.frequency.is_some() && request.group.is_none());
    gui.harness.set_size(egui::vec2(1500.0, 3200.0));
    gui.results();
    let kinds: Vec<app_model::SectionKind> =
        gui.app().documents[active].reports[0].plot_keys().into_iter().map(|(k, _)| k).collect();
    for kind in [
        app_model::SectionKind::LifeProbabilityPlot,
        app_model::SectionKind::LifeSurvivalPlot,
        app_model::SectionKind::LifeHazardPlot,
    ] {
        assert!(kinds.contains(&kind), "{kind:?} not drawn: {kinds:?}");
    }
    let text = gui.painted_text();
    assert!(text.iter().any(|t| t == "50%"), "the probability axis is labelled in percent");
    assert!(text.iter().any(|t| t.starts_with("Model Comparison")), "{text:?}");
    gui.shot("life-distributions-report");
}

/// The ordinal or nominal logistic regression dialog: an ordinal response
/// starts in its natural order and can be reordered; a nominal one takes a
/// reference category; each fit adds its report.
#[test]
fn categorical_logistic_dialog_fits_ordinal_and_nominal_models() {
    const TITLE: &str = "Ordinal or nominal logistic regression";
    let mut gui = Gui::with_sample("Service ratings");
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui.menu(&["Model", "Ordinal or nominal logistic regression…"]);
    gui.choose(TITLE, 0, "Rating");
    assert!(gui.shows("1. Excellent"), "natural order first");
    for _ in 0..3 {
        gui.click("Move Poor lower");
    }
    assert!(gui.shows("1. Poor") && gui.shows("2. Excellent"));
    assert!(gui.widget("Run").is_disabled(), "no predictors yet");
    gui.click("Wait (min)");
    gui.click("Channel");
    gui.shot("categorical-logistic-dialog-ordinal");
    gui.click("Run");
    let reports = &gui.app().documents[active].reports;
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].title(), "Ordinal Logistic Regression: Rating ~ Wait (min) + Channel");
    let app_model::project::SavedReport::CategoricalLogistic { request } = reports[0].saved() else {
        panic!("not a categorical report")
    };
    assert_eq!(request.order[..2], ["Poor".to_owned(), "Excellent".to_owned()]);
    assert!(request.reference.is_none());
    gui.results();
    assert!(gui.shows("Parallel-Lines Test (small p: the effects differ between cumulative splits)"));

    // A nominal response (Channel) with a chosen reference category.
    gui.menu(&["Model", "Ordinal or nominal logistic regression…"]);
    gui.click("Reset");
    gui.choose(TITLE, 0, "Channel");
    assert!(gui.shows("Reference category:"), "a nominal column starts as a nominal model");
    gui.choose(TITLE, 1, "Phone");
    gui.click("Wait (min)");
    gui.click("Run");
    let reports = &gui.app().documents[active].reports;
    assert_eq!(reports.len(), 2);
    let nominal = reports
        .iter()
        .find_map(|r| match r.saved() {
            app_model::project::SavedReport::CategoricalLogistic { request }
                if request.model == app_model::CategoricalModel::Nominal =>
            {
                Some((request, r.title()))
            }
            _ => None,
        })
        .expect("a nominal report");
    assert_eq!(nominal.0.reference.as_deref(), Some("Phone"));
    assert_eq!(nominal.1, "Nominal Logistic Regression: Channel ~ Wait (min)");
    gui.run();
    assert!(gui.shows("Odds Ratios (odds of each category against Phone)"));
    gui.shot("categorical-logistic-nominal-report");
}
/// Model ▸ Nonlinear regression…: a library curve with automatic starting
/// values, then a formula with typed parameters; both reports open with
/// their estimates and fitted curve.
#[test]
fn nonlinear_regression_dialog_runs_both_kinds_of_model() {
    const NONLINEAR: &str = "Nonlinear regression";
    let mut gui = Gui::with_sample("Seedling growth");
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui.menu(&["Model", "Nonlinear regression…"]);
    assert!(gui.widget("Run").is_disabled());
    assert!(gui.shows("⚠ Choose a response"));
    gui.choose(NONLINEAR, 0, "Height (cm)");
    gui.choose(NONLINEAR, 1, "Gompertz");
    gui.choose(NONLINEAR, 2, "Day");
    assert!(gui.shows("y = Asymptote * exp(-exp(-Rate * (x - Inflection)))"));
    gui.widget("Asymptote lower bound").type_text("0");
    gui.run();
    gui.click("Run");
    gui.results();
    let reports = &gui.app().documents[active].reports;
    assert_eq!(reports.len(), 1);
    let app_model::SavedReport::Nonlinear { request, .. } = reports[0].saved() else {
        panic!("not a nonlinear report")
    };
    assert_eq!(request.parameters[0].lower, Some(0.0));
    assert!(gui.shows("Nonlinear Regression: Height (cm) by Day (Gompertz)"));
    assert!(gui.shows("Parameter Estimates"));
    assert!(gui.shows("Correlation of Estimates"));

    // A formula with two parameters typed in the dialog.
    gui.menu(&["Model", "Nonlinear regression…"]);
    gui.choose(NONLINEAR, 0, "Height (cm)");
    gui.click("Formula with parameters");
    gui.window(NONLINEAR).get_by_role(Role::MultilineTextInput).type_text("a * (1 - exp(-b * Day))");
    gui.run();
    assert!(gui.shows("⚠ Enter a starting value for a"));
    gui.widget("a start").type_text("50");
    gui.widget("b start").type_text("0.05");
    gui.run();
    assert!(!gui.widget("Run").is_disabled());
    gui.click("Run");
    let reports = &gui.app().documents[active].reports;
    assert_eq!(reports.len(), 2);
    // New reports open at the top.
    let app_model::SavedReport::Nonlinear { request, .. } = reports[0].saved() else {
        panic!("not a nonlinear report")
    };
    let app_model::NonlinearModel::Formula { formula, .. } = &request.model else { panic!("not a formula model") };
    assert_eq!(formula, "a * (1 - exp(-b * Day))");
    assert_eq!(request.parameters[0].start, Some(50.0));
    assert!(gui.shows("Nonlinear Regression: Height (cm)"));
    gui.shot("nonlinear-regression-report");
}

/// Model ▸ Nonlinear regression…: one curve fitted to two compounds at once
/// with every parameter but the potency shared, outliers removed first; the
/// report tests whether the potency differs.
#[test]
fn nonlinear_regression_dialog_fits_data_sets_together() {
    const NONLINEAR: &str = "Nonlinear regression";
    let mut gui = Gui::with_sample("Compound potency");
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui.menu(&["Model", "Nonlinear regression…"]);
    gui.choose(NONLINEAR, 0, "Response (%)");
    gui.choose(NONLINEAR, 1, "Dose–response, log dose (4 parameters)");
    gui.choose(NONLINEAR, 2, "Log concentration (M)");
    gui.choose(NONLINEAR, 3, "Compound");
    assert!(gui.shows("Shared by all data sets"));
    gui.click("LogEC50 shared");
    gui.choose(NONLINEAR, 5, "Least squares after removing outliers");
    assert!(gui.shows("Q (false discovery rate, %):"));
    gui.click("Run");
    gui.results();
    let reports = &gui.app().documents[active].reports;
    assert_eq!(reports.len(), 1);
    let app_model::SavedReport::Nonlinear { request, .. } = reports[0].saved() else {
        panic!("not a nonlinear report")
    };
    assert!(request.group.is_some());
    let shared: Vec<bool> = request.parameters.iter().map(|p| p.shared).collect();
    assert_eq!(shared, [true, true, false, true]);
    assert!(matches!(request.method, app_model::FitMethod::RemoveOutliers { .. }));
    assert!(gui.shows("Does LogEC50 differ between data sets?"));
}

/// Model ▸ Nonlinear regression…: a differential equation with the starting
/// value as a parameter.
#[test]
fn nonlinear_regression_dialog_fits_a_differential_equation() {
    const NONLINEAR: &str = "Nonlinear regression";
    let mut gui = Gui::with_sample("Substrate depletion");
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui.menu(&["Model", "Nonlinear regression…"]);
    gui.choose(NONLINEAR, 0, "Substrate");
    gui.click("Differential equation");
    gui.choose(NONLINEAR, 1, "Time");
    gui.window(NONLINEAR).get_by_role(Role::MultilineTextInput).type_text("-a * Substrate / (b + Substrate)");
    gui.run();
    gui.widget("a start").type_text("2");
    gui.widget("b start").type_text("5");
    // The new parameter (named p3) is the substrate at time zero.
    gui.click("Add parameter");
    gui.widget("p3 start").type_text("50");
    gui.run();
    gui.choose(NONLINEAR, 2, "p3");
    gui.widget("Starting predictor value").type_text("0");
    gui.run();
    assert!(!gui.widget("Run").is_disabled());
    gui.click("Run");
    gui.results();
    let reports = &gui.app().documents[active].reports;
    assert_eq!(reports.len(), 1);
    let app_model::SavedReport::Nonlinear { request, .. } = reports[0].saved() else {
        panic!("not a nonlinear report")
    };
    let app_model::NonlinearModel::Differential { initial, .. } = &request.model else { panic!("not an ODE model") };
    assert_eq!(initial, "p3");
    assert!(gui.shows("Parameter Estimates"));
}

// Group C: quality dialogs

fn c_reports(gui: &Gui) -> usize {
    gui.app().documents[gui.app().active].reports.len()
}

fn c_clear_reports(gui: &mut Gui) {
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
}

/// Whether any label (such as the dialog's note) contains `text`.
fn c_says(gui: &Gui, text: &str) -> bool {
    gui.harness.query_all_by_label_contains(text).next().is_some()
}

/// The label starting with `start` (the dialog's summary note).
fn c_note(gui: &Gui, start: &str) -> String {
    gui.harness
        .query_all_by_label_contains(start)
        .filter(|n| n.role() == Role::Label)
        .filter_map(|n| n.label().or_else(|| n.value()))
        .find(|l| l.starts_with(start))
        .unwrap_or_else(|| panic!("no text starting {start:?}"))
}

/// Tick the first check box labelled `label`.
fn c_tick(gui: &mut Gui, label: &str) {
    let tick = |n: &Node<'_>| n.role() == Role::CheckBox && n.label().as_deref() == Some(label);
    gui.harness.get_all_by(tick).next().expect("a check box").simulate_click();
    gui.run();
}

/// "Edit analysis…" on the top report.
fn c_edit_first(gui: &mut Gui) {
    gui.results();
    let options = |n: &Node<'_>| n.role() == Role::Button && n.label().as_deref() == Some("⋯ Options");
    gui.harness.get_all_by(options).next().expect("a report").simulate_click();
    gui.run();
    gui.click("Edit analysis…");
}

/// Run the open dialog, reopen its report with "Edit analysis…", check it
/// came back saying `summary`, and Update: still one report, the same one.
fn c_run_edit_update(gui: &mut Gui, summary: &str) {
    assert!(c_says(gui, summary), "{summary:?} not in {:?}", gui.painted_text());
    // The button, not a check box for a column called Run.
    let run = |n: &Node<'_>| n.role() == Role::Button && n.label().as_deref() == Some("Run");
    gui.harness.get_by(run).simulate_click();
    gui.run();
    assert_eq!(c_reports(gui), 1);
    let id = gui.app().documents[gui.app().active].reports[0].id();
    c_edit_first(gui);
    assert!(c_says(gui, summary), "filled from the report: {:?}", gui.painted_text());
    gui.click("Update");
    assert_eq!(c_reports(gui), 1);
    assert_eq!(gui.app().documents[gui.app().active].reports[0].id(), id, "updated in place");
}

#[test]
fn c_control_chart_dialog() {
    const T: &str = "Control chart";
    let mut gui = Gui::with_sample("Fill weights");
    c_clear_reports(&mut gui);
    gui.menu(&["Quality", "Control chart…"]);
    gui.shot("dialog-control-chart-first-open");
    assert!(gui.shows("⚠ Choose a measurement column"));
    assert!(gui.widget("Run").is_disabled());
    gui.choose(T, 0, "Weight (g)");
    gui.choose(T, 1, "Subgroup");
    assert!(c_says(&gui, "· 150 rows"));
    c_run_edit_update(&mut gui, "X̄ and R chart of Weight (g) in subgroups by Subgroup");
    // A short-run chart needs a part number.
    c_edit_first(&mut gui);
    gui.click("Short-Run Z and Moving Range");
    assert!(gui.shows("⚠ Choose a part number for a short-run chart"));
}

#[test]
fn c_count_chart_dialog() {
    const T: &str = "Count and rare-event chart";
    let mut gui = Gui::with_sample("Daily inspection");
    c_clear_reports(&mut gui);
    gui.menu(&["Quality", "Count and rare-event chart (p, np, c, u, p′, u′, g, t)…"]);
    gui.shot("dialog-count-chart-first-open");
    assert!(gui.shows("⚠ Choose a count column"));
    gui.choose(T, 0, "Defective units");
    assert!(gui.shows("⚠ Choose the column of units inspected"));
    gui.choose(T, 1, "Units inspected");
    c_run_edit_update(&mut gui, "p chart of Defective units, units inspected from Units inspected");
}

#[test]
fn c_time_weighted_dialog() {
    let mut gui = Gui::with_sample("Fill weights");
    c_clear_reports(&mut gui);
    gui.menu(&["Quality", "EWMA chart…"]);
    gui.shot("dialog-ewma-first-open");
    assert!(gui.shows("⚠ Choose a measurement column"));
    gui.choose("EWMA chart", 0, "Weight (g)");
    c_run_edit_update(&mut gui, "EWMA chart of Weight (g) (λ = 0.2, L = 3)");
    // Switching to CUSUM renames the window.
    c_edit_first(&mut gui);
    gui.click("CUSUM");
    assert!(gui.harness.query_by_role_and_label(Role::Window, "CUSUM chart").is_some());
    assert!(c_says(&gui, "CUSUM chart of Weight (g) (k = 0.5, h = 5)"));
}

#[test]
fn c_capability_dialog() {
    let mut gui = Gui::with_sample("Fill weights");
    c_clear_reports(&mut gui);
    gui.menu(&["Quality", "Process capability…"]);
    gui.shot("dialog-capability-first-open");
    assert!(gui.shows("⚠ Choose a measurement column"));
    gui.choose("Process capability", 0, "Weight (g)");
    c_tick(&mut gui, "Lower limit (LSL)");
    c_tick(&mut gui, "Upper limit (USL)");
    assert!(gui.shows("⚠ Set a lower or an upper specification limit"));
    c_tick(&mut gui, "Upper limit (USL)");
    c_run_edit_update(&mut gui, "Capability of Weight (g) against USL 1 (Normal)");
    // The summary page mode has its own title and remembered settings.
    gui.menu(&["Quality", "Capability summary page…"]);
    assert!(gui.harness.query_by_role_and_label(Role::Window, "Capability summary page").is_some());
    gui.shot("dialog-capability-summary-first-open");
    assert!(gui.shows("⚠ Choose a measurement column"));
}

#[test]
fn c_multivariate_chart_dialog() {
    let mut gui = Gui::with_sample("Bottle filling");
    c_clear_reports(&mut gui);
    gui.menu(&["Quality", "Multivariate control chart…"]);
    gui.shot("dialog-multivariate-chart-first-open");
    assert!(gui.shows("⚠ Choose at least two measurements"));
    c_tick(&mut gui, "Fill volume (mL)");
    assert!(gui.shows("⚠ Choose at least two measurements"));
    c_tick(&mut gui, "Cap torque (N·m)");
    c_run_edit_update(&mut gui, "Hotelling T² chart of 2 measurements");
}

/// The MEWMA in-control run length is capped at 2000, so a typed 100 000
/// (which would take minutes to simulate) is held at the cap.
#[test]
fn c_multivariate_mewma_run_length_is_capped() {
    const T: &str = "Multivariate control chart";
    let mut gui = Gui::with_sample("Bottle filling");
    c_clear_reports(&mut gui);
    gui.menu(&["Quality", "Multivariate control chart…"]);
    c_tick(&mut gui, "Fill volume (mL)");
    c_tick(&mut gui, "Cap torque (N·m)");
    gui.click("MEWMA");
    gui.click("More options");
    let arl = gui
        .window(T)
        .get_all_by_role(Role::SpinButton)
        .find(|n| n.value().as_deref() == Some("200"))
        .expect("the run-length field")
        .id();
    gui.set_number(arl, 100_000.0);
    gui.click("Run");
    let reports = &gui.app().documents[gui.app().active].reports;
    assert_eq!(reports.len(), 1);
    let app_model::SavedReport::MultivariateChart { request } = reports[0].saved() else {
        panic!("not a multivariate chart")
    };
    let app_model::MewmaLimit::InControlArl(arl) = request.mewma_limit else { panic!("{:?}", request.mewma_limit) };
    assert!((arl - 2000.0).abs() < 1e-9, "{arl}");
}

#[test]
fn c_gauge_dialog() {
    const T: &str = "Gauge R&R study";
    let mut gui = Gui::with_sample("Gauge study");
    c_clear_reports(&mut gui);
    gui.menu(&["Quality", "Gauge R&R…"]);
    gui.shot("dialog-gauge-first-open");
    assert!(c_says(&gui, "Gauge R&R of Measurement by Part and Operator"), "roles guessed from the names");
    gui.click("Reset");
    assert!(gui.shows("⚠ Choose a part column"));
    gui.choose(T, 1, "Part");
    assert!(gui.shows("⚠ Choose an operator column"));
    gui.choose(T, 2, "Operator");
    gui.choose(T, 3, "Measurement");
    c_run_edit_update(&mut gui, "Gauge R&R of Measurement by Part and Operator");
}

#[test]
fn c_quality_tool_dialogs() {
    const T: &str = "Attribute capability";
    let mut gui = Gui::with_sample("Supplier lots");
    c_clear_reports(&mut gui);
    gui.menu(&["Quality", "Attribute capability…"]);
    gui.shot("dialog-attribute-capability-first-open");
    assert!(gui.shows("⚠ Choose a count column"));
    gui.choose(T, 0, "Defective units");
    assert!(gui.shows("⚠ Choose the column of units inspected"));
    gui.choose(T, 1, "Units inspected");
    c_run_edit_update(&mut gui, "Binomial capability of Defective units, units inspected from Units inspected");

    c_clear_reports(&mut gui);
    gui.menu(&["Quality", "Between/within capability…"]);
    gui.shot("dialog-between-within-first-open");
    assert!(gui.shows("⚠ Choose a measurement column"));
    gui.click("Cancel");

    gui.menu(&["Quality", "Acceptance sampling plan…"]);
    gui.shot("dialog-acceptance-first-open");
    assert!(!c_note(&gui, "Single attribute plan").contains(" rows"), "uses no rows");
    c_run_edit_update(&mut gui, "Single attribute plan (n, c) for AQL");
}

#[test]
fn c_study_dialogs() {
    const T: &str = "Gauge bias and linearity";
    let mut gui = Gui::with_sample("Gauge linearity");
    c_clear_reports(&mut gui);
    gui.menu(&["Quality", "Bias and linearity…"]);
    gui.shot("dialog-gauge-linearity-first-open");
    gui.click("Reset");
    assert!(gui.shows("⚠ Choose the reference-value column"));
    gui.choose(T, 0, "Reference");
    assert!(gui.shows("⚠ Choose a measurement column"));
    gui.choose(T, 1, "Measurement");
    c_run_edit_update(&mut gui, "Bias and linearity of Measurement against Reference");

    // The worksheet dialog creates a table.
    let documents = gui.app().documents.len();
    gui.menu(&["Quality", "Gauge study worksheet…"]);
    gui.shot("dialog-gauge-worksheet-first-open");
    assert!(c_says(&gui, "Gauge study worksheet (crossed, 3 operators), "));
    assert!(!c_note(&gui, "Gauge study worksheet (").contains(" rows"), "a new table: no row count");
    gui.click("Create");
    assert_eq!(gui.app().documents.len(), documents + 1);
}

/// Dialogs opened from the Design menu come back with their settings too.
#[test]
fn c_design_menu_study_dialogs_remember_settings() {
    const T: &str = "Signal-to-noise analysis";
    let mut gui = Gui::with_sample("Moulding shrinkage");
    c_clear_reports(&mut gui);
    gui.menu(&["Design", "Signal-to-noise analysis…"]);
    assert!(gui.harness.query_by_role_and_label(Role::Window, T).is_some());
    gui.shot("dialog-signal-to-noise-first-open");
    assert!(gui.shows("⚠ Choose at least one control factor"));
    // Control factors are the right-hand list of the two.
    let factor = |n: &Node<'_>| n.role() == Role::CheckBox && n.label().as_deref() == Some("Temperature");
    gui.harness.get_all_by(factor).last().expect("a factor check box").simulate_click();
    gui.run();
    assert!(gui.shows("⚠ Choose at least one response"), "{:?}", gui.painted_text());
    gui.click("Cancel");
    gui.menu(&["Design", "Signal-to-noise analysis…"]);
    assert!(gui.shows("⚠ Choose at least one response"), "remembered");
    // The first list of the two: responses.
    let response =
        |n: &Node<'_>| n.role() == Role::CheckBox && n.label().as_deref() == Some("Shrinkage (Humidity=low, Age=new)");
    gui.harness.get_all_by(response).next().expect("a response check box").simulate_click();
    gui.run();
    c_run_edit_update(&mut gui, "Signal-to-noise (Smaller the better) of 1 response on 1 factor");
    gui.menu(&["Design", "Robust design (orthogonal arrays)…"]);
    gui.shot("dialog-robust-design-first-open");
    assert!(c_says(&gui, " runs"));
}

// Group A: Compare, multivariate and mixed-model dialogs

/// The active document's report count.
fn a_reports(gui: &Gui) -> usize {
    gui.app().documents[gui.app().active].reports.len()
}

/// " · 20 rows" for the active table.
fn a_rows(gui: &Gui) -> String {
    format!(" · {} rows", gui.app().documents[gui.app().active].table.n_rows())
}

/// Tick the check box labelled `label`.
fn a_tick(gui: &mut Gui, label: &str) {
    gui.harness.get_by(|n| n.role() == Role::CheckBox && n.label().as_deref() == Some(label)).simulate_click();
    gui.run();
}

/// Open a sample with no reports.
fn a_sample(name: &str) -> Gui {
    let mut gui = Gui::with_sample(name);
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui
}

/// Reopen the top report in its dialog ("Edit analysis…"), check the dialog
/// says `summary`, and Update it: one report, the same one.
fn a_edit_and_update(gui: &mut Gui, summary: &str) {
    gui.results();
    let id = gui.app().documents[gui.app().active].reports[0].id();
    let options = |n: &Node<'_>| n.role() == Role::Button && n.label().as_deref() == Some("⋯ Options");
    gui.harness.get_all_by(options).next().expect("a report").simulate_click();
    gui.run();
    gui.click("Edit analysis…");
    assert!(gui.shows(summary), "filled from the report: {:?}", gui.painted_text());
    gui.click("Update");
    let doc = &gui.app().documents[gui.app().active];
    assert_eq!(doc.reports.len(), 1, "Update keeps one report");
    assert_eq!(doc.reports[0].id(), id, "the same report");
}

/// Compare ▸ Two or more groups: what is missing, the summary, Run and
/// Edit analysis; the calculator reads no rows; a random column is created.
#[test]
fn a_compare_dialogs_say_what_is_missing_and_edit_in_place() {
    const GROUPS: &str = "Compare groups";
    let mut gui = a_sample("Sleep drug trial");
    gui.menu(&["Compare", "Two or more groups…"]);
    gui.shot("dialog-compare-groups-first-open");
    assert!(gui.shows("⚠ Choose a data column"));
    assert!(gui.widget("Run").is_disabled());
    gui.choose(GROUPS, 0, "Extra sleep (h)");
    assert!(gui.shows("⚠ Choose a group column"));
    gui.choose(GROUPS, 1, "Drug");
    let summary = format!("Compare Extra sleep (h) across groups of Drug{}", a_rows(&gui));
    assert!(gui.shows(&summary), "{:?}", gui.painted_text());
    gui.click("More options");
    gui.shot("dialog-compare-groups-more-options");
    gui.click("Run");
    assert_eq!(a_reports(&gui), 1);
    assert!(matches!(gui.app().documents[gui.app().active].reports[0].saved(), app_model::SavedReport::Groups { .. }));
    a_edit_and_update(&mut gui, &summary);

    // The calculator's summary has no row count.
    gui.menu(&["Describe", "Distribution calculator…"]);
    gui.shot("dialog-distribution-calculator-first-open");
    assert!(gui.shows("Probabilities and quantiles of Normal(0, 1)"), "{:?}", gui.painted_text());
    gui.click("Cancel");

    // A random column: Create adds it to the table.
    let n_cols = gui.app().documents[gui.app().active].table.n_cols();
    gui.menu(&["Data", "New random column…"]);
    gui.shot("dialog-random-column-first-open");
    let text = gui.painted_text();
    assert!(
        text.iter().any(|t| t.starts_with("Column “Random") && t.ends_with("” of 20 draws from Normal(0, 1)")),
        "{text:?}"
    );
    gui.click("Create");
    assert_eq!(gui.app().documents[gui.app().active].table.n_cols(), n_cols + 1);
}

/// Multivariate ▸ Principal components (phase-2 dialogs).
#[test]
fn a_principal_components_dialog_runs_and_edits() {
    let mut gui = a_sample("Flower measurements");
    gui.menu(&["Multivariate", "Principal components…"]);
    gui.shot("dialog-principal-components-first-open");
    assert!(gui.shows("⚠ Choose at least two columns"));
    a_tick(&mut gui, "Sepal length");
    assert!(gui.shows("⚠ Choose at least two columns"));
    a_tick(&mut gui, "Petal length");
    let summary = format!("Principal components of 2 columns, on correlations{}", a_rows(&gui));
    assert!(gui.shows(&summary), "{:?}", gui.painted_text());
    gui.click("Run");
    assert_eq!(a_reports(&gui), 1);
    a_edit_and_update(&mut gui, &summary);
    let app_model::SavedReport::Pca { request } = gui.app().documents[gui.app().active].reports[0].saved() else {
        panic!("not a PCA report")
    };
    assert_eq!(request.columns.len(), 2);

    // The generalized linear model names its response first.
    gui.menu(&["Model", "Generalized linear model…"]);
    gui.shot("dialog-glm-first-open");
    assert!(gui.shows("⚠ Choose a response"));
    gui.click("Cancel");
}

/// Multivariate ▸ Correlations (column-set methods).
#[test]
fn a_correlations_dialog_runs_and_edits() {
    const TITLE: &str = "Correlations";
    let mut gui = a_sample("Flower measurements");
    gui.menu(&["Multivariate", "Correlations…"]);
    gui.shot("dialog-correlations-first-open");
    assert!(gui.shows("⚠ Choose at least two columns"));
    a_tick(&mut gui, "Sepal length");
    a_tick(&mut gui, "Sepal width");
    a_tick(&mut gui, "Petal width");
    let summary = format!("Correlations of 3 columns{}", a_rows(&gui));
    assert!(gui.shows(&summary), "{:?}", gui.painted_text());
    gui.click("Run");
    assert!(gui.harness.query_by_role_and_label(Role::Window, TITLE).is_none(), "the dialog closes");
    assert_eq!(a_reports(&gui), 1);
    a_edit_and_update(&mut gui, &summary);

    gui.menu(&["Multivariate", "Item analysis…"]);
    gui.shot("dialog-item-analysis-first-open");
    assert!(gui.shows("⚠ Choose at least two items"));
    gui.click("Cancel");
}

/// Multivariate ▸ Multivariate analysis of variance: two lists of columns
/// (responses, then predictors), a contrast built from level weights, Run
/// and Edit analysis.
#[test]
fn a_manova_dialog_runs_with_a_contrast_and_edits() {
    // The `n`th check box labelled `label` (the responses list comes first).
    fn tick(gui: &mut Gui, label: &str, last: bool) {
        let boxes = |n: &Node<'_>| n.role() == Role::CheckBox && n.label().as_deref() == Some(label);
        let mut all: Vec<_> = gui.harness.get_all_by(boxes).collect();
        let node = if last { all.pop() } else { all.into_iter().next() };
        node.expect("a check box").simulate_click();
        gui.run();
    }
    let mut gui = a_sample("Flower measurements");
    gui.menu(&["Multivariate", "Multivariate analysis of variance…"]);
    gui.shot("dialog-manova-first-open");
    assert!(gui.shows("⚠ Choose at least two responses"), "{:?}", gui.painted_text());
    tick(&mut gui, "Sepal length", false);
    tick(&mut gui, "Petal length", false);
    assert!(gui.shows("⚠ Choose at least one predictor"));
    tick(&mut gui, "Variety", true);
    let summary = format!("MANOVA of 2 responses on 1 effect{}", a_rows(&gui));
    assert!(gui.shows(&summary), "{:?}", gui.painted_text());
    gui.click("More options");
    assert!(gui.widget("Add contrast").is_disabled(), "no weights yet");
    let a = gui.widget("Weight of A").id();
    gui.set_number(a, 1.0);
    let b = gui.widget("Weight of B").id();
    gui.set_number(b, -1.0);
    gui.click("Add contrast");
    let summary = format!("MANOVA of 2 responses on 1 effect, 1 contrast{}", a_rows(&gui));
    assert!(gui.shows(&summary), "{:?}", gui.painted_text());
    gui.shot("dialog-manova-contrast");
    gui.click("Run");
    assert_eq!(a_reports(&gui), 1);
    let app_model::SavedReport::Manova { request } = gui.app().documents[gui.app().active].reports[0].saved() else {
        panic!("not a MANOVA report")
    };
    assert_eq!(request.responses.len(), 2);
    assert_eq!(request.contrasts.len(), 1);
    assert_eq!(request.contrasts[0].name, "A vs B");
    gui.results();
    gui.shot("manova-report");
    assert!(gui.shows("Multivariate Tests"), "{:?}", gui.painted_text());
    assert!(gui.shows("Canonical Plot: Variety"));
    a_edit_and_update(&mut gui, &summary);
}

/// Model ▸ Mixed model.
#[test]
fn a_mixed_model_dialog_runs_and_edits() {
    const MIXED: &str = "Mixed model";
    let mut gui = a_sample("Plant growth");
    gui.menu(&["Model", "Mixed model…"]);
    gui.shot("dialog-mixed-model-first-open");
    assert!(gui.shows("⚠ Choose a response"));
    assert!(gui.widget("Run").is_disabled());
    gui.choose(MIXED, 0, "Height");
    assert!(gui.shows("⚠ Choose a grouping column"));
    gui.choose(MIXED, 1, "Greenhouse");
    a_tick(&mut gui, "Fertilizer");
    a_tick(&mut gui, "Light hours");
    let summary = format!("Mixed model of Height on 2 fixed effects, random effects by Greenhouse{}", a_rows(&gui));
    assert!(gui.shows(&summary), "{:?}", gui.painted_text());
    gui.shot("dialog-mixed-model-ready");
    gui.click("Run");
    assert_eq!(a_reports(&gui), 1);
    a_edit_and_update(&mut gui, &summary);
    let app_model::SavedReport::Mixed { request, .. } = gui.app().documents[gui.app().active].reports[0].saved() else {
        panic!("not a mixed model")
    };
    assert_eq!(request.predictors.len(), 2);
    assert_eq!(request.random.len(), 1);
}

/// Every sample report made by a group A dialog reopens in it complete:
/// its dialog shows a summary, not a "⚠" note.
#[test]
fn a_sample_reports_reopen_complete_in_their_dialogs() {
    use app_model::SavedReport as S;
    let mine = |s: &S| {
        matches!(
            s,
            S::OneSample { .. }
                | S::Groups { .. }
                | S::Paired { .. }
                | S::Proportions { .. }
                | S::Rates { .. }
                | S::Power { .. }
                | S::GoodnessOfFit { .. }
                | S::Crossover { .. }
                | S::FactorialRepeated { .. }
                | S::Nested { .. }
                | S::DistributionCalculator { .. }
                | S::AdjustPValues { .. }
                | S::AreaUnderCurve { .. }
                | S::MultipleResponseItems { .. }
                | S::ManyTests { .. }
                | S::Pca { .. }
                | S::Cluster { .. }
                | S::Glm { .. }
                | S::Regularized { .. }
                | S::Correspondence { .. }
                | S::MultipleResponses { .. }
                | S::Correlations { .. }
                | S::OutlierDistances { .. }
                | S::ItemAnalysis { .. }
                | S::Manova { .. }
                | S::FactorAnalysis { .. }
                | S::DiscriminantAnalysis { .. }
                | S::GaussianMixture { .. }
                | S::LatentClass { .. }
                | S::TextExploration { .. }
                | S::TopicModel { .. }
                | S::Sentiment { .. }
                | S::Embedding { .. }
                | S::Imputation { .. }
                | S::Mixed { .. }
                | S::Bayesian { .. }
                | S::Signal { .. }
        )
    };
    let mut checked = 0;
    for (n, sample) in app_model::sample::catalog().iter().enumerate() {
        let mut gui = Gui::start();
        gui.perform(Action::OpenSample(n));
        let active = gui.app().active;
        let reports: Vec<(u64, S)> =
            gui.app().documents[active].reports.iter().map(|r| (r.id(), r.saved())).filter(|(_, s)| mine(s)).collect();
        for (id, saved) in reports {
            gui.app_mut().edit_report(id);
            gui.run();
            let text = gui.painted_text();
            assert!(text.iter().any(|t| t == "Update"), "{}: {saved:?} did not reopen", sample.name);
            let warning = text.iter().find(|t| t.starts_with("⚠ "));
            assert!(warning.is_none(), "{}: {saved:?} reopened incomplete: {warning:?}", sample.name);
            gui.click("Cancel");
            checked += 1;
        }
    }
    assert!(checked >= 10, "only {checked} reports checked");
}

// Group D: reliability, design and table dialogs

fn d_reports(gui: &Gui) -> usize {
    gui.app().documents[gui.app().active].reports.len()
}

fn d_clear_reports(gui: &mut Gui) {
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
}

/// Whether some text on screen contains `part`.
fn d_shows_part(gui: &Gui, part: &str) -> bool {
    gui.harness.query_all_by_label_contains(part).next().is_some()
}

fn d_is_open(gui: &Gui, title: &str) -> bool {
    gui.harness.query_by_role_and_label(Role::Window, title).is_some()
}

fn d_tick(gui: &mut Gui, label: &str) {
    gui.harness.get_by(|n| n.role() == Role::CheckBox && n.label().as_deref() == Some(label)).simulate_click();
    gui.run();
}

/// `Gui::choose` for an option whose name a menu also has ("Design"): the
/// drop-down list's entry is drawn last.
fn d_choose_last(gui: &mut Gui, window: &str, n: usize, option: &str) {
    gui.window(window).get_all_by_role(Role::ComboBox).nth(n).expect("no such drop-down list").simulate_click();
    gui.run();
    let entry =
        |n: &Node<'_>| !matches!(n.role(), Role::Label | Role::CheckBox) && n.label().as_deref() == Some(option);
    gui.harness.get_all_by(entry).last().expect("no such entry").simulate_click();
    gui.run();
}

/// The first number field of window `title`, and its value.
fn d_first_number(gui: &Gui, title: &str) -> (egui::accesskit::NodeId, Option<f64>) {
    let node = gui.window(title).get_all_by_role(Role::SpinButton).next().expect("a number field");
    (node.id(), node.numeric_value())
}

fn d_escape(gui: &mut Gui) {
    gui.harness.input_mut().events.push(egui::Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    });
    gui.run();
}

/// Open "Edit analysis…" from the top report's ⋯ Options menu.
fn d_edit_first_report(gui: &mut Gui) {
    gui.results();
    let options = |n: &Node<'_>| n.role() == Role::Button && n.label().as_deref() == Some("⋯ Options");
    gui.harness.get_all_by(options).next().expect("a report").simulate_click();
    gui.run();
    gui.click("Edit analysis…");
}

/// Run the dialog, reopen its report with "Edit analysis…", check the
/// summary came back, and Update it in place.
fn d_run_edit_update(gui: &mut Gui, title: &str, summary: &str) {
    let before = d_reports(gui);
    gui.click("Run");
    assert!(!d_is_open(gui, title), "the dialog closes");
    assert_eq!(d_reports(gui), before + 1);
    let id = gui.app().documents[gui.app().active].reports[0].id();
    d_edit_first_report(gui);
    assert!(d_is_open(gui, title), "Edit analysis… reopens {title}");
    assert!(d_shows_part(gui, summary), "filled from the report: {:?}", gui.painted_text());
    gui.click("Update");
    assert_eq!(d_reports(gui), before + 1, "Update keeps one report");
    assert_eq!(gui.app().documents[gui.app().active].reports[0].id(), id, "the same report");
}

/// Survival curves: the time column is required; the summary names the
/// roles; Edit analysis… brings them back.
#[test]
fn d_survival_dialog_runs_and_edits() {
    const TITLE: &str = "Survival analysis";
    let mut gui = Gui::with_sample("Leukemia");
    d_clear_reports(&mut gui);
    gui.menu(&["Reliability", "Survival curves…"]);
    gui.shot("dialog-survival-first-open");
    assert!(gui.shows("⚠ Choose a time-to-event column"));
    assert!(gui.widget("Run").is_disabled());
    gui.choose(TITLE, 0, "Weeks");
    gui.choose(TITLE, 1, "Censored");
    gui.choose(TITLE, 2, "Treatment");
    let summary = "Survival curves of Weeks by Treatment, censoring from Censored";
    assert!(d_shows_part(&gui, summary), "{:?}", gui.painted_text());
    d_run_edit_update(&mut gui, TITLE, summary);
}

/// Reliability growth, test plans and repairable systems run from their
/// menus and reopen filled in.
#[test]
fn d_reliability_dialogs_run_and_edit() {
    const GROWTH: &str = "Reliability growth";
    let mut gui = Gui::with_sample("Prototype failures");
    d_clear_reports(&mut gui);
    gui.menu(&["Reliability", "Reliability growth…"]);
    gui.shot("dialog-reliability-growth-first-open");
    assert!(gui.shows("⚠ Choose a cumulative time column"));
    gui.choose(GROWTH, 0, "Test hours");
    gui.choose(GROWTH, 1, "Test hours");
    assert!(gui.shows("⚠ Event and time must be different columns"));
    gui.choose(GROWTH, 1, "Failure");
    gui.choose(GROWTH, 2, "Phase");
    d_run_edit_update(&mut gui, GROWTH, "Reliability growth of Test hours by Phase");

    const PLAN: &str = "Reliability test plan";
    gui.menu(&["Reliability", "Test plans…"]);
    gui.shot("dialog-reliability-test-plan-first-open");
    assert!(d_shows_part(&gui, "Demonstration plan: units needed"), "{:?}", gui.painted_text());
    gui.click("Estimation");
    d_run_edit_update(&mut gui, PLAN, "Estimation plan: units to estimate B10 life");

    const REPAIRS: &str = "Repairable systems";
    let mut gui = Gui::with_sample("Pump repairs");
    d_clear_reports(&mut gui);
    gui.menu(&["Reliability", "Repairable systems…"]);
    gui.shot("dialog-repairable-systems-first-open");
    assert!(gui.shows("⚠ Choose the system column"));
    gui.choose(REPAIRS, 0, "Unit");
    assert!(gui.shows("⚠ Choose the age column"));
    gui.choose(REPAIRS, 1, "Age (days)");
    gui.choose(REPAIRS, 2, "Repair");
    d_choose_last(&mut gui, REPAIRS, 3, "Design");
    d_run_edit_update(&mut gui, REPAIRS, "Mean cumulative repairs per Unit by Age (days), populations of Design");
}

/// Life distributions: right-censored times, run, and Edit analysis….
#[test]
fn d_life_distribution_dialog_runs_and_edits() {
    const LIFE: &str = "Life distributions";
    let mut gui = Gui::with_sample("Bearing life test");
    d_clear_reports(&mut gui);
    gui.menu(&["Reliability", "Life distributions…"]);
    gui.shot("dialog-life-distributions-first-open");
    assert!(gui.shows("⚠ Choose a time column"));
    gui.choose(LIFE, 0, "Hours");
    gui.choose(LIFE, 1, "Suspended");
    gui.choose(LIFE, 3, "Supplier");
    let summary = "6 distributions fitted to Hours by Supplier";
    assert!(d_shows_part(&gui, summary), "{:?}", gui.painted_text());
    d_run_edit_update(&mut gui, LIFE, summary);
}

/// The design dialog: a design that cannot be built says why; Create opens
/// the design as a new document; settings come back on reopening.
#[test]
fn d_design_dialog_creates_and_remembers() {
    const TITLE: &str = "Designed experiment";
    let mut gui = Gui::with_sample("Longley");
    let docs = gui.app().documents.len();
    gui.menu(&["Design", "Fractional factorial…"]);
    gui.shot("dialog-design-first-open");
    assert!(!gui.widget("Create").is_disabled());
    let (factors, _) = d_first_number(&gui, TITLE);
    gui.set_number(factors, 2.0);
    assert!(d_shows_part(&gui, "⚠ "), "a fraction of two factors cannot be made: {:?}", gui.painted_text());
    assert!(gui.widget("Create").is_disabled());
    gui.set_number(factors, 5.0);
    assert!(!gui.widget("Create").is_disabled());
    assert!(d_shows_part(&gui, "runs"));
    gui.click("Create");
    assert!(!d_is_open(&gui, TITLE));
    assert_eq!(gui.app().documents.len(), docs + 1, "the design opens as a new document");
    gui.menu(&["Design", "Fractional factorial…"]);
    assert_eq!(d_first_number(&gui, TITLE).1, Some(5.0), "five factors remembered");
    gui.click("Reset");
    assert_eq!(d_first_number(&gui, TITLE).1, Some(3.0), "Reset starts again");
}

/// The mixture design dialog: Escape cancels, settings are remembered,
/// Create opens the design.
#[test]
fn d_mixture_design_dialog_creates_and_remembers() {
    const TITLE: &str = "Mixture experiment";
    let mut gui = Gui::with_sample("Longley");
    let docs = gui.app().documents.len();
    gui.menu(&["Design", "Simplex lattice…"]);
    gui.shot("dialog-mixture-design-first-open");
    assert!(d_shows_part(&gui, "runs"), "{:?}", gui.painted_text());
    let (components, _) = d_first_number(&gui, TITLE);
    gui.set_number(components, 4.0);
    d_escape(&mut gui);
    assert!(!d_is_open(&gui, TITLE), "Escape cancels");
    gui.menu(&["Design", "Simplex lattice…"]);
    assert_eq!(d_first_number(&gui, TITLE).1, Some(4.0), "four components remembered");
    gui.click("Create");
    assert_eq!(gui.app().documents.len(), docs + 1);
}

/// Table operations: the summary names the table to be made; Sort sorts in
/// place under its own verb; settings are remembered per operation.
#[test]
fn d_table_op_dialogs_create_and_remember() {
    const STACK: &str = "Stack columns";
    let mut gui = Gui::with_sample("Longley");
    let docs = gui.app().documents.len();
    gui.menu(&["Data", "Reshape", "Stack columns…"]);
    gui.shot("dialog-stack-first-open");
    assert!(gui.shows("⚠ Choose at least one column to stack"));
    assert!(gui.widget("Create").is_disabled());
    d_tick(&mut gui, "GNP");
    d_tick(&mut gui, "Employed");
    assert!(d_shows_part(&gui, "New table “Stacked Employed, GNP” in Longley (1947–1962)"), "{:?}", gui.painted_text());
    assert!(d_shows_part(&gui, ": 32 rows × "), "{:?}", gui.painted_text());
    gui.click("Create");
    assert!(!d_is_open(&gui, STACK));
    assert_eq!(gui.app().documents.len(), docs + 1, "the result opens as a new document");

    const SORT: &str = "Sort rows";
    let mut gui = Gui::with_sample("Longley");
    gui.menu(&["Data", "Rows", "Sort…"]);
    gui.shot("dialog-sort-first-open");
    assert!(gui.shows("⚠ Choose a column to sort by"));
    gui.choose(SORT, 0, "GNP");
    gui.click("Descending");
    assert!(d_shows_part(&gui, "by GNP (descending)"), "{:?}", gui.painted_text());
    gui.click("Sort");
    assert!(!d_is_open(&gui, SORT));
    let doc = &gui.app().documents[gui.app().active];
    let gnp = doc.table.column(doc.table.find("GNP").unwrap()).unwrap().as_numeric().unwrap().to_vec();
    assert!(gnp.windows(2).all(|w| w[0] >= w[1]), "sorted descending: {gnp:?}");
    gui.menu(&["Data", "Rows", "Sort…"]);
    assert!(d_shows_part(&gui, "by GNP (descending)"), "settings remembered");
}

/// The formula editor uses the shared window and action row: it says what
/// is missing, and Apply adds the column.
#[test]
fn d_formula_dialog_says_why_apply_is_unavailable() {
    let mut gui = Gui::with_sample("Longley");
    let at = gui.app().documents[gui.app().active].table.n_cols();
    gui.grid(vec![GridRequest::Formula { column: None, at }]);
    gui.shot("dialog-formula-first-open");
    assert!(gui.shows("⚠ Write a formula"));
    assert!(gui.widget("Apply").is_disabled());
    gui.harness.input_mut().events.push(egui::Event::Text("GNP * 2".to_owned()));
    gui.run();
    assert!(d_shows_part(&gui, "Add the formula column “Formula"), "{:?}", gui.painted_text());
    gui.click("Apply");
    assert!(!d_is_open(&gui, "New formula column"));
    assert_eq!(gui.app().documents[gui.app().active].table.n_cols(), at + 1);
}

/// Under the formula editor, a single-expression formula is drawn as
/// mathematics (a root, a fraction, a power); a formula with steps has no
/// typeset view.
#[test]
fn d_formula_dialog_typesets_the_formula() {
    let mut gui = Gui::with_sample("Longley");
    let at = gui.app().documents[gui.app().active].table.n_cols();
    gui.grid(vec![GridRequest::Formula { column: None, at }]);
    assert!(!d_shows_part(&gui, "Typeset formula"), "nothing typeset before a formula is written");
    gui.harness.input_mut().events.push(egui::Event::Text("sqrt(GNP) / Unemployed ** 2".to_owned()));
    gui.run();
    gui.shot("dialog-formula-typeset");
    assert!(gui.shows("Reads as"));
    assert!(d_shows_part(&gui, " = (√(GNP))/(Unemployed^2)"), "{:?}", gui.painted_text());
    // The radical sign is drawn as part of the typeset formula.
    assert!(gui.painted_text().iter().any(|t| t.contains('√')), "{:?}", gui.painted_text());
    gui.harness.input_mut().events.push(egui::Event::Text("; GNP".to_owned()));
    gui.run();
    assert!(!d_shows_part(&gui, "Typeset formula"), "statements are not typeset");
    assert!(d_shows_part(&gui, "Add the formula column"), "the formula itself is still fine");
}

/// The Recode dialog says what it will make, and why Recode is unavailable.
#[test]
fn d_recode_dialog_says_why_recode_is_unavailable() {
    let mut gui = Gui::with_sample("Longley");
    gui.grid(vec![GridRequest::Recode(5)]);
    gui.shot("dialog-recode-first-open");
    assert!(d_shows_part(&gui, "from “Population 14+”: 0 values changed"), "{:?}", gui.painted_text());
    gui.click("Replace the values in this column (undoable)");
    assert!(gui.shows("⚠ Type a new value for at least one value"));
    assert!(gui.widget("Recode").is_disabled());
}

// Group B: predictive, regression and diagram dialogs

fn b_reports(gui: &Gui) -> usize {
    gui.app().documents[gui.app().active].reports.len()
}

fn b_clear_reports(gui: &mut Gui) {
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
}

fn b_tick(gui: &mut Gui, label: &str) {
    gui.harness.get_by(|n| n.role() == Role::CheckBox && n.label().as_deref() == Some(label)).simulate_click();
    gui.run();
}

/// Whether a label containing `part` is on screen (the summary line ends
/// with the row count).
fn b_shows_part(gui: &Gui, part: &str) -> bool {
    gui.harness.query_all_by_label_contains(part).next().is_some()
}

/// Open the ⋯ Options menu of the top report and choose "Edit analysis…".
fn b_edit_top_report(gui: &mut Gui) {
    let options = |n: &Node<'_>| n.role() == Role::Button && n.label().as_deref() == Some("⋯ Options");
    gui.harness.get_all_by(options).next().expect("a report").simulate_click();
    gui.run();
    gui.click("Edit analysis…");
}

/// Run the dialog, reopen it with "Edit analysis…" from the new report's
/// options menu, check it came back filled (`summary`) and Update it: still
/// one report, the same one.
fn b_run_edit_update(gui: &mut Gui, title: &str, summary: &str) {
    assert!(b_shows_part(gui, summary), "{:?}", gui.painted_text());
    gui.click("Run");
    assert!(gui.harness.query_by_role_and_label(Role::Window, title).is_none(), "the dialog closes");
    assert_eq!(b_reports(gui), 1);
    gui.results();
    let id = gui.app().documents[gui.app().active].reports[0].id();
    b_edit_top_report(gui);
    assert!(gui.harness.query_by_role_and_label(Role::Window, title).is_some(), "the dialog reopens");
    assert!(b_shows_part(gui, summary), "filled from the report: {:?}", gui.painted_text());
    gui.click("Update");
    assert_eq!(b_reports(gui), 1, "updated in place");
    assert_eq!(gui.app().documents[gui.app().active].reports[0].id(), id);
}

#[test]
fn b_decision_tree_dialog() {
    const TITLE: &str = "Decision tree";
    let mut gui = Gui::with_sample("Loan applications");
    b_clear_reports(&mut gui);
    gui.menu(&["Model", "Decision tree…"]);
    gui.shot("dialog-decision-tree-first-open");
    assert!(gui.shows("⚠ Choose a response"));
    gui.choose(TITLE, 0, "Outcome");
    assert!(gui.shows("⚠ Choose at least one predictor"));
    b_tick(&mut gui, "Debt ratio");
    b_tick(&mut gui, "Age");
    b_run_edit_update(&mut gui, TITLE, "Classification tree of Outcome on 2 predictors");
}

#[test]
fn b_random_forest_dialog() {
    const TITLE: &str = "Random forest";
    let mut gui = Gui::with_sample("Loan applications");
    b_clear_reports(&mut gui);
    gui.menu(&["Model", "Random forest…"]);
    gui.shot("dialog-random-forest-first-open");
    assert!(gui.shows("⚠ Choose a response"));
    gui.choose(TITLE, 0, "Outcome");
    assert!(gui.shows("⚠ Choose at least one predictor"));
    b_tick(&mut gui, "Debt ratio");
    b_tick(&mut gui, "Age");
    b_run_edit_update(&mut gui, TITLE, "Random forest of Outcome on 2 predictors");
}

#[test]
fn b_boosted_trees_dialog() {
    const TITLE: &str = "Boosted trees";
    let mut gui = Gui::with_sample("Loan applications");
    b_clear_reports(&mut gui);
    gui.menu(&["Model", "Boosted trees…"]);
    gui.shot("dialog-boosted-trees-first-open");
    assert!(gui.shows("⚠ Choose a response"));
    gui.choose(TITLE, 0, "Outcome");
    b_tick(&mut gui, "Debt ratio");
    // Roles from a column need the column named.
    gui.click("Roles from a column");
    assert!(gui.shows("⚠ Choose the column of validation roles"));
    gui.choose(TITLE, 1, "Validation");
    b_run_edit_update(&mut gui, TITLE, "Boosted trees for Outcome on 1 predictor");
    let app_model::SavedReport::BoostedTrees { request, .. } = gui.app().documents[gui.app().active].reports[0].saved()
    else {
        panic!("not boosted trees")
    };
    assert!(matches!(request.validation, app_model::ValidationMethod::Column(_)), "kept through the edit");
}

#[test]
fn b_partial_least_squares_dialog() {
    const TITLE: &str = "Partial least squares";
    let mut gui = Gui::with_sample("Fuel spectra");
    b_clear_reports(&mut gui);
    gui.menu(&["Model", "Partial least squares…"]);
    gui.shot("dialog-partial-least-squares-first-open");
    assert!(gui.shows("⚠ Choose a response"));
    gui.choose(TITLE, 0, "Octane");
    assert!(gui.shows("⚠ Choose at least one predictor"));
    gui.click("All");
    b_run_edit_update(&mut gui, TITLE, "Partial least squares of Octane on 40 predictors");
}

#[test]
fn b_support_vector_machine_dialog() {
    const TITLE: &str = "Support vector machine";
    let mut gui = Gui::with_sample("Pump sensors");
    b_clear_reports(&mut gui);
    gui.menu(&["Model", "Support vector machine…"]);
    gui.shot("dialog-support-vector-machine-first-open");
    assert!(gui.shows("⚠ Choose a response"));
    gui.choose(TITLE, 0, "Status");
    assert!(gui.shows("⚠ Choose at least one predictor"));
    b_tick(&mut gui, "Temperature");
    b_tick(&mut gui, "Vibration");
    gui.click("Linear");
    b_run_edit_update(&mut gui, TITLE, "Support vector machine for Status on 2 predictors, linear kernel");
}

#[test]
fn b_compare_models_dialog() {
    const TITLE: &str = "Compare models";
    let mut gui = Gui::with_sample("Loan applications");
    b_clear_reports(&mut gui);
    gui.menu(&["Model", "Compare models…"]);
    gui.shot("dialog-compare-models-first-open");
    assert!(gui.shows("⚠ Choose a response"));
    gui.choose(TITLE, 0, "Outcome");
    assert!(gui.shows("⚠ Choose at least one prediction column"));
    b_tick(&mut gui, "Prob Default (tree)");
    b_tick(&mut gui, "Prob Default (forest)");
    b_run_edit_update(&mut gui, TITLE, "Comparison of 2 models of Outcome");
}

#[test]
fn b_nonlinear_regression_dialog() {
    const TITLE: &str = "Nonlinear regression";
    let mut gui = Gui::with_sample("Seedling growth");
    b_clear_reports(&mut gui);
    gui.menu(&["Model", "Nonlinear regression…"]);
    gui.shot("dialog-nonlinear-regression-first-open");
    assert!(gui.shows("⚠ Choose a response"));
    gui.choose(TITLE, 0, "Height (cm)");
    assert!(gui.shows("⚠ Choose a predictor"));
    gui.choose(TITLE, 1, "Gompertz");
    gui.choose(TITLE, 2, "Day");
    gui.widget("Asymptote lower bound").type_text("0");
    gui.run();
    b_run_edit_update(&mut gui, TITLE, "curve of Height (cm) on Day");
    let app_model::SavedReport::Nonlinear { request, .. } = gui.app().documents[gui.app().active].reports[0].saved()
    else {
        panic!("not a nonlinear report")
    };
    assert_eq!(request.parameters[0].lower, Some(0.0), "the bound came back into the dialog");
}

#[test]
fn b_dose_response_dialog() {
    let mut gui = Gui::with_sample("Rotenone");
    b_clear_reports(&mut gui);
    gui.menu(&["Model", "Dose–response (probit)…"]);
    gui.shot("dialog-dose-response-first-open");
    assert!(gui.shows("⚠ Choose the dose column"));
    gui.choose(DOSE, 0, "Rotenone (mg/l)");
    assert!(gui.shows("⚠ Choose the number responding"));
    gui.choose(DOSE, 1, "Killed");
    gui.choose(DOSE, 2, "Aphids");
    b_run_edit_update(&mut gui, DOSE, "Dose–response of Killed on Rotenone (mg/l), probit link");
}

#[test]
fn b_method_comparison_and_shelf_life_dialogs() {
    const METHOD: &str = "Method comparison";
    let mut gui = Gui::with_sample("Glucose methods");
    b_clear_reports(&mut gui);
    gui.menu(&["Model", "Method comparison…"]);
    gui.shot("dialog-method-comparison-first-open");
    assert!(gui.shows("⚠ Choose the comparison method (X)"));
    gui.choose(METHOD, 0, "Analyzer (mg/dL)");
    gui.choose(METHOD, 1, "Analyzer (mg/dL)");
    assert!(gui.shows("⚠ The two methods must be different columns"));
    gui.choose(METHOD, 1, "Meter (mg/dL)");
    b_run_edit_update(&mut gui, METHOD, "Deming and Passing–Bablok fits of Meter (mg/dL) on Analyzer (mg/dL)");

    const SHELF: &str = "Shelf life";
    let mut gui = Gui::with_sample("Tablet stability");
    b_clear_reports(&mut gui);
    gui.menu(&["Model", "Shelf life…"]);
    gui.shot("dialog-shelf-life-first-open");
    assert!(gui.shows("⚠ Choose the time column"));
    gui.choose(SHELF, 0, "Month");
    gui.choose(SHELF, 1, "Assay (% label claim)");
    gui.choose(SHELF, 2, "Batch");
    b_tick(&mut gui, "Lower limit:");
    assert!(gui.shows("⚠ Set a lower or an upper limit"));
    b_tick(&mut gui, "Lower limit:");
    b_run_edit_update(&mut gui, SHELF, "Shelf life of Assay (% label claim) over Month, by Batch");
}

#[test]
fn b_mixture_model_dialog() {
    const TITLE: &str = "Mixture model";
    let mut gui = Gui::with_sample("Yarn elongation");
    b_clear_reports(&mut gui);
    gui.menu(&["Model", "Mixture model…"]);
    gui.shot("dialog-mixture-model-first-open");
    assert!(gui.shows("⚠ Choose a response"));
    gui.choose(TITLE, 0, "Elongation");
    assert!(gui.shows("⚠ Choose at least two components"));
    b_tick(&mut gui, "Polyethylene");
    b_tick(&mut gui, "Polystyrene");
    b_tick(&mut gui, "Polypropylene");
    b_run_edit_update(&mut gui, TITLE, "Quadratic mixture model of Elongation in 3 components");
}

#[test]
fn b_analysis_of_means_dialog() {
    const TITLE: &str = "Analysis of means";
    let mut gui = Gui::with_sample("Heat treatment");
    b_clear_reports(&mut gui);
    gui.menu(&["Quality", "Analysis of means…"]);
    gui.shot("dialog-analysis-of-means-first-open");
    assert!(gui.shows("⚠ Choose a response"));
    gui.choose(TITLE, 0, "Hardness (HRC)");
    assert!(gui.shows("⚠ Choose the group column"));
    gui.choose(TITLE, 1, "Furnace");
    b_run_edit_update(&mut gui, TITLE, "Analysis of means of Hardness (HRC) by Furnace");
}

/// The Pareto chart's options travel with the request: changing them in
/// "Edit analysis…" changes the chart.
#[test]
fn b_pareto_chart_dialog() {
    const TITLE: &str = "Pareto chart";
    let mut gui = Gui::with_sample("Solder defects");
    b_clear_reports(&mut gui);
    gui.menu(&["Quality", "Pareto chart…"]);
    gui.shot("dialog-pareto-chart-first-open");
    assert!(gui.shows("⚠ Choose the category column"));
    gui.choose(TITLE, 0, "Category");
    gui.choose(TITLE, 1, "Defects");
    b_run_edit_update(&mut gui, TITLE, "Pareto chart of Category weighted by Defects");
    b_edit_top_report(&mut gui);
    gui.click("More options");
    b_tick(&mut gui, "Bar heights as percentages of the total");
    gui.click("Update");
    let app_model::SavedReport::Pareto { options, .. } = gui.app().documents[gui.app().active].reports[0].saved()
    else {
        panic!("not a Pareto chart")
    };
    assert!(options.percent_scale, "the dialog's options replace the report's");
}

#[test]
fn b_cause_effect_and_multi_vari_dialogs() {
    const FISHBONE: &str = "Cause-and-effect diagram";
    let mut gui = Gui::with_sample("Solder defects");
    b_clear_reports(&mut gui);
    gui.menu(&["Quality", "Cause-and-effect diagram…"]);
    gui.shot("dialog-cause-and-effect-first-open");
    assert!(gui.shows("⚠ Choose the column of main branches"));
    gui.choose(FISHBONE, 0, "Category");
    gui.choose(FISHBONE, 1, "Category");
    assert!(gui.shows("⚠ Branches, causes and sub-causes must be different columns"));
    gui.choose(FISHBONE, 1, "Cause");
    b_run_edit_update(&mut gui, FISHBONE, "Cause-and-effect diagram: Category › Cause");

    const MULTI: &str = "Multi-vari chart";
    let mut gui = Gui::with_sample("Shaft diameters");
    b_clear_reports(&mut gui);
    gui.menu(&["Quality", "Multi-vari chart…"]);
    gui.shot("dialog-multi-vari-first-open");
    assert!(gui.shows("⚠ Choose a response"));
    gui.choose(MULTI, 0, "Diameter (mm)");
    assert!(gui.shows("⚠ Choose the first factor"));
    gui.choose(MULTI, 1, "Batch");
    gui.choose(MULTI, 2, "Position");
    b_run_edit_update(&mut gui, MULTI, "Multi-vari chart of Diameter (mm) by Batch, Position");
}

/// The shelf-life dialog asks for the unit of time (months by default) so
/// that the report can apply ICH Q1E's 12-month extrapolation limit; "Not
/// stated" leaves only the twice-the-data limit.
#[test]
fn shelf_life_dialog_states_the_time_unit() {
    let mut gui = Gui::with_sample("Tablet stability");
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui.menu(&["Model", "Shelf life…"]);
    gui.choose("Shelf life", 0, "Month");
    gui.choose("Shelf life", 1, "Assay (% label claim)");
    gui.choose("Shelf life", 2, "Batch");
    gui.click("Run");
    gui.results();
    assert!(gui.shows("36 months (12 months beyond the longest time)"));
    let saved = gui.app().documents[active].reports[0].saved();
    let app_model::SavedReport::ShelfLife { request } = saved else { panic!() };
    assert_eq!(request.time_unit, Some(app_model::StabilityTimeUnit::Months));
    gui.shot("shelf-life-time-unit");

    gui.app_mut().documents[active].reports.clear();
    gui.menu(&["Model", "Shelf life…"]);
    gui.choose("Shelf life", 0, "Month");
    gui.choose("Shelf life", 1, "Assay (% label claim)");
    gui.choose("Shelf life", 3, "Not stated");
    gui.click("Run");
    gui.results();
    let saved = gui.app().documents[active].reports[0].saved();
    let app_model::SavedReport::ShelfLife { request } = saved else { panic!() };
    assert_eq!(request.time_unit, None);
    assert!(gui.shows("48 (twice the longest time; time unit not stated, so 12 months not checked)"));
}

#[test]
fn c_attribute_agreement_dialog() {
    let mut gui = Gui::with_sample("Visual inspection");
    c_clear_reports(&mut gui);
    gui.menu(&["Quality", "Attribute agreement…"]);
    gui.shot("dialog-attribute-agreement-first-open");
    c_run_edit_update(&mut gui, "Agreement of Result ratings of Part by Inspector");
}

#[test]
fn c_multiway_table_dialog() {
    let mut gui = Gui::with_sample("Multi-centre trial");
    c_clear_reports(&mut gui);
    gui.menu(&["Describe", "Multi-way table…"]);
    assert!(gui.shows("⚠ Choose at least one row or column classification"));
    gui.click("Rows: Centre");
    gui.click("Columns: Outcome");
    c_run_edit_update(&mut gui, "Multi-way table of Centre by Outcome");
}

#[test]
fn c_permutation_test_dialog() {
    const T: &str = "Permutation test";
    let mut gui = Gui::with_sample("Repair times");
    c_clear_reports(&mut gui);
    gui.menu(&["Compare", "Permutation test…"]);
    assert!(gui.shows("⚠ Choose the response"));
    gui.choose(T, 0, "Repair time (h)");
    gui.choose(T, 1, "Crew");
    c_run_edit_update(&mut gui, "Permutation test of Repair time (h) between the groups of Crew");
}

/// Data ▸ New random column draws its column once per change of settings,
/// not on every frame, and caps the seed at 2⁵³ − 1 so it saves exactly.
#[test]
fn random_column_dialog_draws_once_per_setting() {
    const RANDOM: &str = "New random column";
    let mut gui = a_sample("Longley");
    gui.menu(&["Data", "New random column…"]);
    let draws = |gui: &Gui| gui.app().compare_dialog.as_ref().expect("the dialog is open").random_draws();
    assert_eq!(draws(&gui), 1);
    for _ in 0..5 {
        gui.harness.step();
    }
    assert_eq!(draws(&gui), 1, "nothing changed, nothing drawn again");
    gui.click("More options");
    assert_eq!(draws(&gui), 1);
    let seed = gui.window(RANDOM).get_all_by_role(Role::SpinButton).last().expect("the seed field").id();
    gui.set_number(seed, 1e19);
    assert_eq!(draws(&gui), 2, "a new seed draws again");
    gui.run();
    assert_eq!(draws(&gui), 2);
    gui.click("Create");
    let table = &gui.app().documents[gui.app().active].table;
    let added = table.column(table.n_cols() - 1).and_then(stat_data::Column::as_numeric).expect("the new column");
    let normal = app_model::CalculatorRequest::new(stat_core::catalog::Family::Normal);
    let max = app_model::seed::MAX_SEED;
    let expected = app_model::random_column("x", normal.family, &normal.params, table.n_rows(), max).expect("draws");
    assert_eq!(Some(added), expected.as_numeric(), "drawn with the seed capped at 2⁵³ − 1");
}
