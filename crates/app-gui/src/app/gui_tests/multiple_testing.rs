//! Many tests and multiple comparisons: Compare ▸ Two groups, many
//! variables…, Compare ▸ Adjust and combine p-values… (with its saved
//! columns), the added pairwise methods, the linear-trend test and effect
//! sizes in model reports, and Describe ▸ Area under a curve….

use super::*;

/// Tick the check box labelled `label`.
fn tick(gui: &mut Gui, label: &str) {
    gui.harness.get_by(|n| n.role() == Role::CheckBox && n.label().as_deref() == Some(label)).simulate_click();
    gui.run();
}

/// Whether a control or heading whose label starts with `start` is shown.
fn shows_start(gui: &Gui, start: &str) -> bool {
    gui.harness.query_all_by(|n| n.label().is_some_and(|l| l.starts_with(start))).count() > 0
}

/// Whether text starting with `start` was painted (dialog summaries, table
/// cells).
fn paints_start(gui: &Gui, start: &str) -> bool {
    gui.painted_text().iter().any(|t| t.starts_with(start))
}

/// The many-tests dialog takes variables on the left and a two-valued group
/// column on the right, says what is missing, and opens a report with the
/// table and the volcano plot.
#[test]
fn many_tests_dialog_runs_tests_by_variable() {
    const TITLE: &str = "Two groups, many variables";
    let mut gui = Gui::with_sample("Gene screen");
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui.menu(&["Compare", "Two groups, many variables…"]);
    assert!(gui.harness.query_by_role_and_label(Role::Window, TITLE).is_some(), "the dialog opens");
    assert!(gui.shows("⚠ Choose a group column"), "{:?}", gui.painted_text());
    assert!(gui.widget("Run").is_disabled());
    gui.choose(TITLE, 0, "Treatment");
    assert!(gui.shows("⚠ Choose at least one variable"), "{:?}", gui.painted_text());
    for g in ["G01", "G02", "G03"] {
        tick(&mut gui, g);
    }
    gui.choose(TITLE, 0, "Sample");
    assert!(gui.shows("⚠ The group column must have two values; Sample has 16"), "{:?}", gui.painted_text());
    gui.choose(TITLE, 0, "Treatment");
    assert!(paints_start(&gui, "Welch t of 3 columns by Treatment"), "{:?}", gui.painted_text());
    gui.click("Smallest p-value first");
    gui.shot("many-tests-dialog");
    gui.click("Run");
    assert!(gui.harness.query_by_role_and_label(Role::Window, TITLE).is_none(), "the dialog closes");
    let reports = &gui.app().documents[active].reports;
    assert_eq!(reports.len(), 1);
    let app_model::SavedReport::ManyTests { request } = reports[0].saved() else { panic!("not a many-tests report") };
    assert_eq!(request.columns.len(), 3);
    assert_eq!(request.order, app_model::ManyTestsOrder::PValue);
    assert_eq!(request.adjust, app_model::AdjustMethod::BenjaminiHochberg);
    gui.harness.set_size(egui::vec2(1500.0, 2400.0));
    gui.results();
    assert!(shows_start(&gui, "Tests by Variable"));
    assert!(shows_start(&gui, "Volcano Plot"));
    // The report's options switch the test in place.
    gui.click("⋯ Options");
    gui.click("Rank-sum (Wilcoxon / Mann–Whitney)");
    gui.harness.press_key(egui::Key::Escape);
    gui.run();
    let app_model::SavedReport::ManyTests { request } = gui.app().documents[active].reports[0].saved() else {
        panic!()
    };
    assert_eq!(request.test, app_model::ManyTestKind::RankSum, "kept with the report");
    gui.shot("many-tests-report");
}

/// The p-value dialog adjusts a column of p-values; the report's options
/// save an adjusted column to the table.
#[test]
fn adjust_p_values_dialog_runs_and_saves_columns() {
    const TITLE: &str = "Adjust and combine p-values";
    let mut gui = Gui::with_sample("Trial endpoints");
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui.menu(&["Compare", "Adjust and combine p-values…"]);
    assert!(gui.harness.query_by_role_and_label(Role::Window, TITLE).is_some(), "the dialog opens");
    gui.choose(TITLE, 0, "p-value");
    gui.choose(TITLE, 1, "Endpoint");
    assert!(paints_start(&gui, "Adjust and combine the p-values in p-value"), "{:?}", gui.painted_text());
    gui.shot("adjust-p-values-dialog");
    gui.click("Run");
    let reports = &gui.app().documents[active].reports;
    assert_eq!(reports.len(), 1);
    let app_model::SavedReport::AdjustPValues { request } = reports[0].saved() else { panic!("not a p-value report") };
    assert_eq!(request.label, gui.app().documents[active].table.find("Endpoint"));
    assert_eq!(request.weight, None);
    gui.harness.set_size(egui::vec2(1500.0, 2400.0));
    gui.results();
    assert!(shows_start(&gui, "Multiple Testing Summary (12 tests"));
    assert!(shows_start(&gui, "Combined Tests"));
    assert!(paints_start(&gui, "Systolic pressure"), "tests named by the label column");
    let n_cols = gui.app().documents[active].table.n_cols();
    gui.click("⋯ Options");
    gui.click("Save Benjamini–Hochberg p-values");
    let table = &gui.app().documents[active].table;
    assert_eq!(table.n_cols(), n_cols + 1, "a column was added");
    let saved = table.find("Benjamini–Hochberg p-value").expect("the saved column");
    let values = table.column(saved).unwrap().as_numeric().unwrap();
    // R: p.adjust(p, "BH") for the smallest p-value (0.0008 of 12) is 0.0096.
    let row = table.column(1).unwrap().as_numeric().unwrap().iter().position(|&p| p == 0.0008).unwrap();
    assert!((values[row] - 0.0096).abs() < 1e-12, "{}", values[row]);
    gui.shot("adjust-p-values-report");
}

/// A one-way linear model's options offer the added pairwise methods and the
/// effect sizes; each choice changes the report in place.
#[test]
fn linear_model_offers_new_pairwise_methods_and_effect_sizes() {
    let mut gui = Gui::with_sample("Silicon resistivity");
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui.app_mut().add_report(ReportRequest::Linear(app_model::LinearModelSpec {
        response: 1,
        predictors: vec![0],
        effects: Vec::new(),
        splines: Vec::new(),
    }));
    gui.harness.set_size(egui::vec2(1500.0, 2400.0));
    gui.results();
    assert!(shows_start(&gui, "All Pairs: Tukey–Kramer"));
    gui.click("⋯ Options");
    gui.widget("Pairwise comparisons: Tukey–Kramer").hover();
    gui.run();
    gui.click("Tamhane T2");
    gui.harness.press_key(egui::Key::Escape);
    gui.run();
    assert!(shows_start(&gui, "All Pairs: Tamhane T2 (unequal variances"), "the section follows the menu");
    gui.click("⋯ Options");
    tick(&mut gui, "Effect sizes (η², partial η², ω², Cohen's f)");
    tick(&mut gui, "Linear trend across ordered levels");
    gui.harness.press_key(egui::Key::Escape);
    gui.run();
    let app_model::SavedReport::Linear { options, .. } = gui.app().documents[active].reports[0].saved() else {
        panic!()
    };
    assert_eq!(options.pairwise, Some(app_model::ComparisonMethod::TamhaneT2));
    assert!(options.show_effect_sizes && options.linear_trend, "kept with the report");
    assert!(shows_start(&gui, "Effect Sizes (95% intervals"), "{:?}", gui.painted_text());
    assert!(shows_start(&gui, "Linear Trend across Ordered Groups"));
    gui.shot("linear-model-effect-sizes");
}

/// The two-or-more-groups report's options add the linear-trend test, and
/// the report shows the ANOVA effect sizes and the Kolmogorov–Smirnov test
/// with two groups.
#[test]
fn group_comparison_adds_a_linear_trend_and_effect_sizes() {
    let mut gui = Gui::with_sample("Silicon resistivity");
    let active = gui.app().active;
    // Keep only the group comparison.
    gui.app_mut().documents[active].reports.retain(|r| matches!(r.saved(), app_model::SavedReport::Groups { .. }));
    gui.harness.set_size(egui::vec2(1500.0, 2400.0));
    gui.results();
    assert!(shows_start(&gui, "Effect Sizes (95% intervals"), "η², ω² and f with three or more groups");
    assert!(!shows_start(&gui, "Linear Trend"));
    gui.click("⋯ Options");
    tick(&mut gui, "Test for a linear trend across the groups in their order (three or more)");
    gui.harness.press_key(egui::Key::Escape);
    gui.run();
    let app_model::SavedReport::Groups { request } = gui.app().documents[active].reports[0].saved() else { panic!() };
    assert!(request.linear_trend);
    assert!(shows_start(&gui, "Linear Trend across Ordered Groups"));

    // Two groups: the Kolmogorov–Smirnov test and Glass's Δ.
    let mut gui = Gui::with_sample("Sleep drug trial");
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui.menu(&["Compare", "Two or more groups…"]);
    gui.choose("Compare groups", 0, "Extra sleep (h)");
    gui.choose("Compare groups", 1, "Drug");
    gui.click("Run");
    gui.harness.set_size(egui::vec2(1500.0, 2400.0));
    gui.results();
    // R: ks.test(A, B, exact = FALSE) with ties: D = 0.4, p = 0.400474.
    let text = gui.painted_text();
    let ks = text.iter().position(|t| t == "Kolmogorov–Smirnov (asymptotic)").expect("the KS row");
    assert_eq!(text[ks + 3], "D = 0.4", "{text:?}");
    assert_eq!(text[ks + 5], "0.4005", "{text:?}");
    assert!(paints_start(&gui, "Glass's Δ (scaled by the SD of B)"), "{text:?}");
}

/// The area dialog takes x, response and an optional group column; the
/// report gives one area per curve and saves the response as a percentage
/// of its curve's range.
#[test]
fn area_under_curve_dialog_runs_and_saves_transforms() {
    const TITLE: &str = "Area under a curve";
    let mut gui = Gui::with_sample("Plasma concentration");
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui.menu(&["Describe", "Area under a curve…"]);
    assert!(gui.shows("⚠ Choose the x column"), "{:?}", gui.painted_text());
    assert!(gui.widget("Run").is_disabled());
    gui.choose(TITLE, 0, "Time (h)");
    assert!(gui.shows("⚠ Choose the response column"));
    gui.choose(TITLE, 1, "Concentration (mg/L)");
    gui.choose(TITLE, 2, "Formulation");
    assert!(
        paints_start(&gui, "Area under Concentration (mg/L) against Time (h), one curve per Formulation"),
        "{:?}",
        gui.painted_text()
    );
    gui.click("Mean of the first points");
    gui.shot("area-under-curve-dialog");
    gui.click("Run");
    let reports = &gui.app().documents[active].reports;
    assert_eq!(reports.len(), 1);
    let app_model::SavedReport::AreaUnderCurve { request } = reports[0].saved() else { panic!("not an area report") };
    assert_eq!(request.baseline, app_model::AreaBaseline::FirstPoints(1));
    assert!(request.group.is_some());
    gui.harness.set_size(egui::vec2(1500.0, 2400.0));
    gui.results();
    assert!(shows_start(&gui, "Area under the Curve (trapezoid rule"));
    assert!(shows_start(&gui, "Peaks above the Baseline"));
    assert!(shows_start(&gui, "Curve: Formulation = A"));
    let n_cols = gui.app().documents[active].table.n_cols();
    gui.click("⋯ Options");
    gui.click("Save response as % of its range");
    let table = &gui.app().documents[active].table;
    assert_eq!(table.n_cols(), n_cols + 1, "a column was added");
    let saved = table.find("Concentration (mg/L) (% of range)").expect("the saved column");
    let values = table.column(saved).unwrap().as_numeric().unwrap();
    let largest = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    assert!((largest - 100.0).abs() < 1e-12 && values.iter().all(|v| (0.0..=100.0).contains(v)));
    gui.shot("area-under-curve-report");
}
