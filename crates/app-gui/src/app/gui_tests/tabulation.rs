//! Describe ▸ Multi-way table… and Compare ▸ Stratified tables and exact tests….

use super::*;

/// The multi-way table dialog nests the ticked columns in the order they
/// were ticked, needs something to show in the cells, and opens the report.
#[test]
fn multiway_table_dialog_nests_columns_and_runs() {
    const TITLE: &str = "Multi-way table";
    let mut gui = Gui::with_sample("Multi-centre trial");
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui.menu(&["Describe", "Multi-way table…"]);
    assert!(gui.harness.query_by_role_and_label(Role::Window, TITLE).is_some());
    assert!(gui.widget("Run").is_disabled(), "no classification yet");
    gui.click("Rows: Centre");
    gui.click("Rows: Treatment");
    gui.click("Columns: Outcome");
    assert!(gui.widget("Columns: Centre").is_disabled(), "a column has one role");
    assert!(gui.shows("Rows: Centre › Treatment"));
    gui.click("Count");
    assert!(gui.shows("⚠ Choose a count or percentage, or a continuous column and a statistic"), "nothing to show");
    assert!(gui.widget("Run").is_disabled());
    gui.click("Continuous columns: Age");
    assert!(!gui.widget("Run").is_disabled(), "the mean of Age is something to show");
    gui.click("Count");
    gui.click("Row %");
    gui.click("Subtotals of outer levels");
    gui.shot("multiway-table-dialog");
    gui.click("Run");
    assert!(gui.harness.query_by_role_and_label(Role::Window, TITLE).is_none(), "the dialog closes");
    let t = &gui.app().documents[active].table;
    let col = |n: &str| t.find(n).unwrap();
    let reports = &gui.app().documents[active].reports;
    assert_eq!(reports.len(), 1);
    let app_model::SavedReport::MultiwayTable { request } = reports[0].saved() else { panic!("not a multi-way table") };
    assert_eq!(request.rows, vec![col("Centre"), col("Treatment")]);
    assert_eq!(request.columns, vec![col("Outcome")]);
    assert_eq!(request.measures, vec![col("Age")]);
    assert_eq!(request.frequencies, vec![app_model::CellFrequency::Count, app_model::CellFrequency::RowPercent]);
    assert!(request.subtotals && request.margins);
    assert_eq!(reports[0].title(), "Multi-way Table: Centre, Treatment by Outcome");
    gui.harness.set_size(egui::vec2(1500.0, 1600.0));
    gui.results();
    let text = gui.painted_text();
    for heading in ["Improved · Count", "Not improved · Row %", "All · Mean Age"] {
        assert!(text.iter().any(|t| t == heading), "{heading} not drawn: {text:?}");
    }
    gui.shot("multiway-table-report");
}

/// The stratified dialog refuses a column in two roles and opens the
/// stratified analysis with the CMH, odds-ratio and homogeneity sections.
#[test]
fn stratified_dialog_refuses_repeated_columns_and_runs() {
    const TITLE: &str = "Stratified tables and exact tests";
    let mut gui = Gui::with_sample("Multi-centre trial");
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui.menu(&["Compare", "Stratified tables and exact tests…"]);
    assert!(gui.widget("Run").is_disabled());
    gui.choose(TITLE, 0, "Treatment");
    gui.choose(TITLE, 1, "Treatment");
    assert!(gui.shows("⚠ Rows, columns and strata must be different columns"));
    assert!(gui.widget("Run").is_disabled());
    gui.choose(TITLE, 1, "Outcome");
    assert!(!gui.shows("⚠ Rows, columns and strata must be different columns"));
    assert!(gui.widget("Strata: Treatment").is_disabled(), "the row column cannot be a stratum");
    gui.click("Strata: Centre");
    gui.shot("stratified-dialog");
    gui.click("Run");
    let reports = &gui.app().documents[active].reports;
    assert_eq!(reports.len(), 1);
    let app_model::SavedReport::StratifiedTables { request } = reports[0].saved() else { panic!("not stratified") };
    let t = &gui.app().documents[active].table;
    assert_eq!(request.strata, vec![t.find("Centre").unwrap()]);
    assert!(request.exact);
    gui.harness.set_size(egui::vec2(1500.0, 2000.0));
    gui.results();
    for title in [
        "Counts by Stratum",
        "Tests by Stratum",
        "Cochran–Mantel–Haenszel Tests",
        "Common Odds Ratio",
        "Homogeneity of Odds Ratios",
    ] {
        assert!(gui.shows(title), "{title} missing");
    }
    let text = gui.painted_text();
    assert!(text.iter().any(|t| t == "Mantel–Haenszel common odds ratio"), "{text:?}");
    gui.shot("stratified-report");
}
