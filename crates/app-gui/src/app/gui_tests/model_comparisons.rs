//! Model ▸ Screen many responses…, spline terms and comparisons within
//! slices in the linear and generalized linear model dialogs and reports,
//! Fieller's ratio of two means, the false-discovery-rate adjustment of
//! stratified tables, the contingency-table differences and residual
//! glyphs, and repeated cross-validation in the model search.

use super::*;

fn tick(gui: &mut Gui, label: &str) {
    gui.harness.get_by(|n| n.role() == Role::CheckBox && n.label().as_deref() == Some(label)).simulate_click();
    gui.run();
}

/// Tick the first check box labelled `label` (left-hand list first).
fn tick_first(gui: &mut Gui, label: &str) {
    let is_box = |n: &Node<'_>| n.role() == Role::CheckBox && n.label().as_deref() == Some(label);
    gui.harness.get_all_by(is_box).next().unwrap_or_else(|| panic!("no check box {label}")).simulate_click();
    gui.run();
}

/// Whether a control or heading whose label starts with `start` is shown.
fn shows_start(gui: &Gui, start: &str) -> bool {
    gui.harness.query_all_by(|n| n.label().is_some_and(|l| l.starts_with(start))).count() > 0
}

/// Whether text containing `part` was painted (dialog summaries, cells).
fn paints(gui: &Gui, part: &str) -> bool {
    gui.painted_text().iter().any(|t| t.contains(part))
}

/// Open a sample with no reports; returns the app and its document.
fn empty_sample(name: &str) -> (Gui, usize) {
    let mut gui = Gui::with_sample(name);
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    (gui, active)
}

/// Open the last drop-down list of window `window` and choose `option`.
fn choose_last(gui: &mut Gui, window: &str, option: &str) {
    let n = gui.window(window).get_all_by_role(Role::ComboBox).count();
    gui.choose(window, n - 1, option);
}

/// Open the top report's ⋯ Options menu.
fn top_options(gui: &mut Gui) {
    let options = |n: &Node<'_>| n.role() == Role::Button && n.label().as_deref() == Some("⋯ Options");
    gui.harness.get_all_by(options).next().expect("a report").simulate_click();
    gui.run();
}

fn column(gui: &Gui, doc: usize, name: &str) -> usize {
    gui.app().documents[doc].table.find(name).unwrap()
}

/// The screening dialog takes numeric responses on the left and factors on
/// the right, says what is missing, and runs a negative binomial screen
/// whose options menu opens one response's full report.
#[test]
fn screening_dialog_runs_and_opens_a_response() {
    const TITLE: &str = "Screen many responses";
    let (mut gui, active) = empty_sample("Transcript counts");
    gui.menu(&["Model", "Screen many responses…"]);
    gui.shot("dialog-screening-first-open");
    assert!(gui.shows("⚠ Choose at least one response"), "{:?}", gui.painted_text());
    for name in ["T01", "T02", "T03", "T04"] {
        tick_first(&mut gui, name);
    }
    assert!(gui.shows("⚠ Choose at least one factor"));
    tick(&mut gui, "Treatment");
    choose_last(&mut gui, TITLE, "Negative binomial regression (overdispersed counts)");
    gui.click("Storey q-values");
    assert!(paints(&gui, "Negative binomial regression of 4 responses on Treatment"), "{:?}", gui.painted_text());
    gui.shot("dialog-screening-ready");
    gui.click("Run");
    let doc = &gui.app().documents[active];
    assert_eq!(doc.reports.len(), 1);
    let app_model::SavedReport::Screening { request } = doc.reports[0].saved() else { panic!("not a screen") };
    assert_eq!(request.responses.len(), 4);
    assert_eq!(request.factors, vec![column(&gui, active, "Treatment")]);
    assert_eq!(request.model, app_model::ScreeningModel::NegativeBinomial);
    assert_eq!(request.fdr, app_model::FdrMethod::Storey);
    gui.harness.set_size(egui::vec2(1500.0, 2400.0));
    gui.results();
    gui.shot("report-screening");
    assert!(shows_start(&gui, "Treatment by Response"), "{:?}", gui.painted_text());
    assert!(shows_start(&gui, "Volcano Plot of Treatment"));
    assert!(shows_start(&gui, "Forest Plot of Treatment"));
    top_options(&mut gui);
    gui.widget("Open a response's full report").hover();
    gui.run();
    let first = |n: &Node<'_>| n.role() == Role::Button && n.label().is_some_and(|l| l.starts_with("T01 (q = "));
    gui.harness.get_by(first).simulate_click();
    gui.run();
    let reports = &gui.app().documents[active].reports;
    assert_eq!(reports.len(), 2, "the response's report opens");
    assert!(reports.iter().any(|r| match r.saved() {
        app_model::SavedReport::Screening { request } => request.responses.len() == 1,
        _ => false,
    }));
}

/// Kruskal–Wallis needs exactly one categorical factor.
#[test]
fn screening_dialog_explains_the_rank_test_rule() {
    const TITLE: &str = "Screen many responses";
    let (mut gui, _) = empty_sample("Transcript counts");
    gui.menu(&["Model", "Screen many responses…"]);
    tick_first(&mut gui, "T01");
    tick(&mut gui, "Treatment");
    tick(&mut gui, "Batch");
    choose_last(&mut gui, TITLE, "Kruskal–Wallis (ranks, one factor)");
    assert!(gui.shows("⚠ Kruskal–Wallis takes exactly one categorical factor"), "{:?}", gui.painted_text());
    assert!(gui.widget("Run").is_disabled());
    tick(&mut gui, "Batch");
    assert!(!gui.widget("Run").is_disabled());
}

/// The linear model dialog enters a continuous predictor as a spline; the
/// report's options add comparisons within slices of an interaction.
#[test]
fn linear_model_spline_and_slices() {
    const LINEAR: &str = "Linear model";
    let (mut gui, active) = empty_sample("Longley");
    gui.menu(&["Model", "Linear model…"]);
    gui.choose(LINEAR, 0, "Employed");
    tick(&mut gui, "GNP");
    gui.click("More options");
    assert!(paints(&gui, "Curved effects of continuous predictors (spline terms):"), "{:?}", gui.painted_text());
    choose_last(&mut gui, LINEAR, "Natural cubic spline");
    gui.shot("dialog-linear-spline");
    gui.click("Run");
    let app_model::SavedReport::Linear { spec, .. } = gui.app().documents[active].reports[0].saved() else {
        panic!("not a linear model")
    };
    assert_eq!(spec.splines, vec![Some(app_model::SplineSpec::natural(3))]);

    let (mut gui, active) = empty_sample("Plastic film");
    let (rate, additive, tear) = (
        column(&gui, active, "Extrusion rate"),
        column(&gui, active, "Additive"),
        column(&gui, active, "Tear resistance"),
    );
    gui.app_mut().add_report(ReportRequest::Linear(app_model::LinearModelSpec {
        response: tear,
        predictors: vec![rate, additive],
        effects: vec![vec![(0, 1)], vec![(1, 1)], vec![(0, 1), (1, 1)]],
        splines: Vec::new(),
    }));
    gui.harness.set_size(egui::vec2(1500.0, 2400.0));
    gui.results();
    assert!(!shows_start(&gui, "Slices of"));
    top_options(&mut gui);
    gui.widget("Comparisons within slices").hover();
    gui.run();
    gui.click("Holm");
    gui.harness.press_key(egui::Key::Escape);
    gui.run();
    let app_model::SavedReport::Linear { options, .. } = gui.app().documents[active].reports[0].saved() else {
        panic!()
    };
    assert_eq!(options.slices, Some(app_model::PairAdjust::Holm), "kept with the report");
    assert!(shows_start(&gui, "Slices of Extrusion rate × Additive"), "{:?}", gui.painted_text());
    gui.shot("report-linear-slices");
}

/// The generalized linear model dialog adds a two-factor interaction and
/// comparisons within its slices.
#[test]
fn glm_dialog_interaction_and_slices() {
    const GLM: &str = "Generalized linear model";
    let (mut gui, active) = empty_sample("Plastic film");
    gui.menu(&["Model", "Generalized linear model…"]);
    gui.choose(GLM, 0, "Tear resistance");
    tick(&mut gui, "Extrusion rate");
    tick(&mut gui, "Additive");
    gui.click("More options");
    tick(&mut gui, "Extrusion rate × Additive");
    let n = gui.window(GLM).get_all_by_role(Role::ComboBox).count();
    // The slice choice comes before the slope choice.
    gui.choose(GLM, n - 2, "Tukey");
    gui.shot("dialog-glm-slices");
    gui.click("Run");
    let app_model::SavedReport::Glm { request, .. } = gui.app().documents[active].reports[0].saved() else {
        panic!("not a GLM")
    };
    assert_eq!(request.effects.len(), 3, "two main effects and their interaction");
    assert_eq!(request.slices, Some(app_model::PairAdjust::Tukey));
    gui.harness.set_size(egui::vec2(1500.0, 2400.0));
    gui.results();
    assert!(shows_start(&gui, "Slices of Extrusion rate × Additive"), "{:?}", gui.painted_text());
}

/// The mixed model dialog offers the same comparisons.
#[test]
fn mixed_dialog_offers_comparisons() {
    let (mut gui, _) = empty_sample("Plastic film");
    gui.menu(&["Model", "Mixed model…"]);
    gui.click("More options");
    assert!(paints(&gui, "Comparisons within slices:"), "{:?}", gui.painted_text());
    assert!(paints(&gui, "Slopes by group:"));
}

/// Two groups: the ratio of means with Fieller's interval.
#[test]
fn group_comparison_adds_the_ratio_of_means() {
    let (mut gui, active) = empty_sample("Sleep drug trial");
    gui.menu(&["Compare", "Two or more groups…"]);
    gui.choose("Compare groups", 0, "Extra sleep (h)");
    gui.choose("Compare groups", 1, "Drug");
    gui.click("More options");
    tick(&mut gui, "Ratio of the two means with Fieller's interval (two groups)");
    gui.click("Run");
    let app_model::SavedReport::Groups { request } = gui.app().documents[active].reports[0].saved() else { panic!() };
    assert!(request.ratio_of_means);
    gui.harness.set_size(egui::vec2(1500.0, 2400.0));
    gui.results();
    assert!(shows_start(&gui, "Ratio of Means, A / B (Fieller interval"), "{:?}", gui.painted_text());
    assert!(paints(&gui, "Unequal (Welch)"));
}

/// Stratified tables adjust each stratum's p-values for the false
/// discovery rate when asked.
#[test]
fn stratified_tables_adjust_for_the_false_discovery_rate() {
    const TITLE: &str = "Stratified tables and exact tests";
    let (mut gui, active) = empty_sample("Multi-centre trial");
    gui.menu(&["Compare", "Stratified tables and exact tests…"]);
    gui.choose(TITLE, 0, "Treatment");
    gui.choose(TITLE, 1, "Outcome");
    gui.click("Strata: Centre");
    gui.click("More options");
    choose_last(&mut gui, TITLE, "Benjamini–Yekutieli");
    gui.click("Run");
    let app_model::SavedReport::StratifiedTables { request } = gui.app().documents[active].reports[0].saved() else {
        panic!()
    };
    assert_eq!(request.fdr, Some(app_model::FdrMethod::BenjaminiYekutieli));
    gui.harness.set_size(egui::vec2(1500.0, 2400.0));
    gui.results();
    assert!(shows_start(&gui, "Tests by Stratum Adjusted for the False Discovery Rate (Benjamini–Yekutieli"));
}

/// A two-way table shows the differences of proportions with their forest
/// plot, and adjusted residuals marked ▲ or ▼ beyond ±1.96.
#[test]
fn contingency_table_differences_and_residual_glyphs() {
    let (mut gui, active) = empty_sample("Multi-centre trial");
    let (x, y) = (column(&gui, active, "Treatment"), column(&gui, active, "Outcome"));
    gui.app_mut().add_report(ReportRequest::Bivariate(app_model::BivariateRequest::new(x, y)));
    gui.harness.set_size(egui::vec2(1500.0, 3000.0));
    gui.results();
    assert!(shows_start(&gui, "Differences of Proportions (Newcombe"), "{:?}", gui.painted_text());
    assert!(shows_start(&gui, "Forest Plot of the Differences of Proportions"));
    assert!(shows_start(&gui, "Adjusted Residuals (▲ above"));
    assert!(gui.painted_text().iter().any(|t| t.ends_with(" ▲") || t.ends_with(" ▼")), "{:?}", gui.painted_text());
    gui.shot("report-contingency-differences");
}

/// The model search repeats k-fold cross-validation with fresh folds and
/// reports each model's error across the repeats.
#[test]
fn model_search_repeats_cross_validation() {
    const SEARCH: &str = "Model search";
    let (mut gui, active) = empty_sample("Polymer batches");
    gui.menu(&["Model", "Model search…"]);
    gui.choose(SEARCH, 0, "Tensile strength (MPa)");
    tick(&mut gui, "Cure temperature (°C)");
    tick(&mut gui, "Pressure (bar)");
    gui.click("More options");
    let repeats = gui.widget("Repeat k-fold cross-validation").id();
    gui.set_number(repeats, 3.0);
    assert!(paints(&gui, "3 repeats"), "{:?}", gui.painted_text());
    gui.click("Run");
    let app_model::SavedReport::ModelSearch { request } = gui.app().documents[active].reports[0].saved() else {
        panic!("search first")
    };
    assert_eq!(request.repeats, 3);
    assert!(!request.nested);
    gui.harness.set_size(egui::vec2(1500.0, 2400.0));
    gui.results();
    assert!(paints(&gui, "across 3 repeats of"), "{:?}", gui.painted_text());
    gui.shot("report-model-search-repeats");
}
