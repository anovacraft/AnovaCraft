//! Options added to existing analyses: distribution-free tolerance sample
//! size, rank columns with tie rules, k-means from given centers, gauge
//! hysteresis and drift, capability by group, and control-chart limit and
//! display options with saved limits read back as standard values.

use super::*;

/// Opens sample `name` showing its results, with its reports closed.
fn empty_sample(name: &str) -> (Gui, usize) {
    let mut gui = Gui::with_sample(name);
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui.results();
    (gui, active)
}

/// Whether any painted text or widget label contains `part`.
fn painted_part(gui: &Gui, part: &str) -> bool {
    gui.painted_text().iter().any(|t| t.contains(part))
        || gui.harness.query_all_by(|n| n.label().is_some_and(|l| l.contains(part))).next().is_some()
}

/// The Run button of a dialog, not a column or entry named "Run".
fn run_button(n: &Node<'_>) -> bool {
    n.role() == Role::Button && n.toggled().is_none() && n.label().as_deref() == Some("Run")
}

#[test]
fn power_dialog_sizes_a_distribution_free_tolerance_interval() {
    let (mut gui, active) = empty_sample("Fill weights");
    let request = app_model::PowerRequest {
        design: app_model::PowerDesign::DistributionFreeTolerance { proportion: 0.95, removed: 2, two_sided: true },
        solve: app_model::PowerSolve::SampleSize { target: 0.95 },
        ..app_model::PowerRequest::default()
    };
    gui.app_mut().add_report(ReportRequest::Comparison(app_model::SavedReport::Power { request }));
    gui.results();
    assert!(painted_part(&gui, "Sample Size: Tolerance interval, distribution-free"), "{:?}", gui.painted_text());
    // Edit analysis… reopens the design with its confidence target.
    gui.click("⋯ Options");
    gui.click("Edit analysis…");
    assert!(gui.shows("Tolerance interval, distribution-free"));
    assert!(gui.shows("Confidence:"), "the target is a confidence, not a power");
    assert!(gui.shows("Largest proportion covered p"));
    gui.click("Update");
    let reports = &gui.app().documents[active].reports;
    assert_eq!(reports.len(), 1);
    let app_model::SavedReport::Power { request } = reports[0].saved() else { panic!("not a power report") };
    assert!(matches!(request.design, app_model::PowerDesign::DistributionFreeTolerance { removed: 2, .. }));
}

#[test]
fn rank_column_dialog_adds_a_formula_with_the_tie_rule() {
    let (mut gui, active) = empty_sample("Fill weights");
    let n_cols = gui.app().documents[active].table.n_cols();
    gui.menu(&["Data", "New rank column…"]);
    gui.choose("New rank column", 0, "Weight (g)");
    gui.choose("New rank column", 1, "Filler setting");
    gui.click("Dense (1, 2, 2, 3)");
    gui.click("Add column");
    let table = &gui.app().documents[active].table;
    assert_eq!(table.n_cols(), n_cols + 1);
    let added = table.column(n_cols).unwrap();
    assert_eq!(added.formula.as_deref(), Some(r#"rank_ties("Weight (g)", "dense", "Filler setting")"#));
    let ranks = added.as_numeric().unwrap();
    assert!(ranks.iter().all(|r| *r >= 1.0 && r.fract() == 0.0), "dense ranks are whole numbers");
}

#[test]
fn k_means_starts_from_typed_centers() {
    let (mut gui, active) = empty_sample("Flower measurements");
    gui.menu(&["Multivariate", "Clustering…"]);
    for c in ["Petal length", "Petal width"] {
        gui.click(c);
    }
    gui.click("More options");
    gui.click("Given centers");
    assert!(gui.shows("Center 1 1"), "a grid of starting centers");
    let field = gui.widget("Center 1 1").id();
    gui.set_number(field, 1.5);
    gui.harness.get_by(run_button).simulate_click();
    gui.run();
    let app_model::SavedReport::Cluster { request } = gui.app().documents[active].reports[0].saved() else {
        panic!("not a clustering report")
    };
    let centers = request.centers.as_ref().expect("given centers");
    assert_eq!(centers.centers[0][0], 1.5);
    assert!(!centers.also_seeded);
    gui.harness.set_size(egui::vec2(1500.0, 3000.0));
    gui.results();
    assert!(painted_part(&gui, "Starting Centers"), "{:?}", gui.painted_text());
}

#[test]
fn gauge_repeatability_shows_hysteresis_and_drift() {
    let mut gui = Gui::with_sample("Gauge repeatability");
    gui.harness.set_size(egui::vec2(1500.0, 3000.0));
    gui.results();
    // The sample's report already uses the Approach and Order columns.
    assert!(painted_part(&gui, "Hysteresis"), "{:?}", gui.painted_text());
    assert!(painted_part(&gui, "Drift over Order"));
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui.run();
    const T: &str = "Single-gauge repeatability (Cg, Cgk)";
    gui.menu(&["Quality", "Gauge repeatability (Cg, Cgk)…"]);
    gui.choose(T, 0, "Measurement");
    gui.choose(T, 1, "Approach");
    gui.click("More options");
    gui.click("Drift over time or order");
    gui.click("Median");
    gui.harness.get_by(run_button).simulate_click();
    gui.run();
    let app_model::SavedReport::GaugeRepeatability { request } = gui.app().documents[active].reports[0].saved() else {
        panic!("not a repeatability report")
    };
    let table = &gui.app().documents[active].table;
    assert_eq!(request.direction, table.find("Approach"));
    assert_eq!(request.order, None);
    assert!(!request.show_drift);
    assert!(request.summary.contains(app_model::SummaryStat::Median));
    gui.results();
    assert!(painted_part(&gui, "Hysteresis"));
    assert!(!painted_part(&gui, "Drift over"));
}

#[test]
fn capability_by_group_adds_the_group_table_and_ppk_chart() {
    let mut gui = Gui::with_sample("Fill weights");
    gui.harness.set_size(egui::vec2(1500.0, 4000.0));
    gui.results();
    assert!(painted_part(&gui, "Capability by Filler setting"), "{:?}", gui.painted_text());
    assert!(painted_part(&gui, "Ppk by Filler setting"));
    assert!(painted_part(&gui, "Normal Probability Plot"));
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui.run();
    gui.menu(&["Quality", "Process capability…"]);
    gui.choose("Process capability", 0, "Weight (g)");
    gui.choose("Process capability", 1, "Filler setting");
    gui.harness.get_by(run_button).simulate_click();
    gui.run();
    let app_model::SavedReport::Capability { request } = gui.app().documents[active].reports[0].saved() else {
        panic!("not a capability report")
    };
    assert_eq!(request.by, gui.app().documents[active].table.find("Filler setting"));
    // The report offers the in-specification indicator column.
    let n_cols = gui.app().documents[active].table.n_cols();
    gui.results();
    gui.click("⋯ Options");
    gui.click("Save in-specification indicator");
    gui.run();
    let table = &gui.app().documents[active].table;
    assert_eq!(table.n_cols(), n_cols + 1);
    assert!(table.column(n_cols).unwrap().formula.as_deref().is_some_and(|f| f.contains("is_missing")));
}

#[test]
fn control_chart_options_last_subgroups_and_saved_standard_values() {
    const T: &str = "Control chart";
    let (mut gui, active) = empty_sample("Fill weights");
    gui.menu(&["Quality", "Control chart…"]);
    gui.choose(T, 0, "Weight (g)");
    gui.choose(T, 1, "Subgroup");
    gui.click("More options");
    let k = gui.widget("Limits at σ").id();
    gui.set_number(k, 2.5);
    gui.click("Show only the last");
    let last = gui.widget("Last subgroups").id();
    gui.set_number(last, 10.0);
    gui.harness.get_by(run_button).simulate_click();
    gui.run();
    let app_model::SavedReport::ControlChart { request, .. } = gui.app().documents[active].reports[0].saved() else {
        panic!("not a control chart")
    };
    assert_eq!(request.sigma_multiple, 2.5);
    assert_eq!(request.last, Some(app_model::LastSubgroups { count: 10, limits_from_last: false }));
    gui.harness.set_size(egui::vec2(1500.0, 3000.0));
    gui.results();
    assert!(painted_part(&gui, "last 10 of 30 subgroups"), "{:?}", gui.painted_text());

    // Run chart, then save the limits as a table.
    gui.click("⋯ Options");
    gui.click("Run chart of every value");
    gui.run();
    assert!(painted_part(&gui, "Run Chart of Weight (g)"));
    let before = gui.app().documents.len();
    // The options menu stays open after a check box.
    if !gui.shows("Save limits as data table") {
        gui.click("⋯ Options");
    }
    gui.press("Save limits as data table");
    gui.run();
    assert_eq!(gui.app().documents.len(), before + 1, "a limits table is added");
    let saved = &gui.app().documents.last().unwrap().table;
    let sv = app_model::control_chart::standard_from_table(saved, "Weight (g)").expect("process center and σ");

    // A new chart reads them back as standard values.
    gui.app_mut().active = active;
    gui.app_mut().documents[active].reports.clear();
    gui.run();
    gui.menu(&["Quality", "Control chart…"]);
    gui.choose(T, 0, "Weight (g)");
    gui.choose(T, 1, "Subgroup");
    // More options stays open from the first chart.
    if !gui.shows("Limits from standard (historical) values") {
        gui.click("More options");
    }
    gui.click("Limits from standard (historical) values");
    let read =
        gui.harness.get_by(|n| n.role() == Role::Button && n.label().is_some_and(|l| l.starts_with("Read from "))).id();
    gui.harness.get_by(move |n| n.id() == read).simulate_click();
    gui.run();
    gui.harness.get_by(run_button).simulate_click();
    gui.run();
    let app_model::SavedReport::ControlChart { request, .. } = gui.app().documents[active].reports[0].saved() else {
        panic!("not a control chart")
    };
    assert_eq!(request.standard, Some(sv));
}
