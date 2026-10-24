//! Compare ▸ Factorial repeated measures… and Compare ▸ Nested
//! comparison…: dialogs filled, run and reopened; the number-at-risk table
//! and log-rank trend test of survival reports; and the number-of-clusters
//! section of k-means reports.

use super::*;

fn shows_part(gui: &Gui, part: &str) -> bool {
    gui.harness.query_all_by_label_contains(part).next().is_some()
        || gui.painted_text().iter().any(|t| t.contains(part))
}

/// Tick the `n`th check box labelled `label` (the within-subject list comes
/// before the between-subject list).
fn tick_nth(gui: &mut Gui, label: &str, n: usize) {
    let is_box = |node: &Node<'_>| node.role() == Role::CheckBox && node.label().as_deref() == Some(label);
    gui.harness.get_all_by(is_box).nth(n).unwrap_or_else(|| panic!("no check box {label} #{n}")).simulate_click();
    gui.run();
}

/// Open a sample with no reports.
fn empty_sample(name: &str) -> Gui {
    let mut gui = Gui::with_sample(name);
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui
}

/// Open the top report's ⋯ Options menu.
fn top_options(gui: &mut Gui) {
    gui.results();
    let options = |n: &Node<'_>| n.role() == Role::Button && n.label().as_deref() == Some("⋯ Options");
    gui.harness.get_all_by(options).next().expect("a report").simulate_click();
    gui.run();
}

/// Reopen the top report with Edit analysis…, check its summary and Update.
fn edit_and_update(gui: &mut Gui, summary: &str) {
    top_options(gui);
    gui.click("Edit analysis…");
    assert!(shows_part(gui, summary), "filled from the report: {:?}", gui.painted_text());
    gui.click("Update");
    assert_eq!(gui.app().documents[gui.app().active].reports.len(), 1, "Update keeps one report");
}

/// The dialog says what is missing, takes within- and between-subject
/// factors from two lists (a column ticked in one leaves the other), asks
/// for both simple-effect factors, and the report shows the sphericity and
/// simple-effects sections; Edit analysis… reopens it filled in.
#[test]
fn factorial_repeated_measures_dialog_runs_and_edits() {
    const TITLE: &str = "Factorial repeated measures";
    let mut gui = empty_sample("Reaction times by task and cue");
    gui.menu(&["Compare", "Factorial repeated measures…"]);
    assert!(gui.shows("⚠ Choose a response column"));
    assert!(gui.widget("Run").is_disabled());
    gui.choose(TITLE, 0, "Reaction time");
    gui.choose(TITLE, 1, "Person");
    assert!(gui.shows("⚠ Choose at least one within-subject factor"));
    tick_nth(&mut gui, "Task", 0);
    tick_nth(&mut gui, "Cue", 0);
    let is_box =
        |label: &'static str| move |n: &Node<'_>| n.role() == Role::CheckBox && n.label().as_deref() == Some(label);
    assert_eq!(gui.harness.get_all_by(is_box("Task")).count(), 1, "a within factor leaves the between list");
    // Age group is in both lists; the second is the between-subject one.
    tick_nth(&mut gui, "Age group", 1);
    tick(&mut gui, "Simple effects of one factor at each level of another");
    assert!(gui.shows("⚠ Choose the two factors for simple effects"));
    gui.choose(TITLE, 2, "Task");
    gui.choose(TITLE, 3, "Age group");
    let summary = "Reaction time by Task × Cue × Age group (subjects: Person) · 120 rows";
    assert!(shows_part(&gui, summary), "{:?}", gui.painted_text());
    gui.shot("dialog-factorial-repeated-measures");
    gui.click("Run");
    let active = gui.app().active;
    let app_model::SavedReport::FactorialRepeated { request } = gui.app().documents[active].reports[0].saved() else {
        panic!("not a factorial repeated-measures report")
    };
    assert_eq!((request.within.len(), request.between.len()), (2, 1));
    assert!(request.simple.is_some());
    gui.harness.set_size(egui::vec2(1500.0, 3600.0));
    gui.results();
    for heading in ["Sphericity (Mauchly's test", "Simple Effects of Task at Each Level of Age group", "Profile Plot"] {
        assert!(shows_part(&gui, heading), "{heading} missing: {:?}", gui.painted_text());
    }
    gui.shot("report-factorial-repeated-measures");
    edit_and_update(&mut gui, summary);
}

/// Tick the first check box labelled `label`.
fn tick(gui: &mut Gui, label: &str) {
    tick_nth(gui, label, 0);
}

/// The nested comparison needs three different columns, runs on the
/// dendrite sample (subject labels repeat across groups but count
/// separately), shows the REML tests, variance components and the
/// classical nested ANOVA, and its confidence level is an in-report option.
#[test]
fn nested_comparison_dialog_runs_and_edits() {
    const TITLE: &str = "Nested comparison";
    let mut gui = empty_sample("Dendrite lengths per mouse");
    gui.menu(&["Compare", "Nested comparison…"]);
    assert!(gui.shows("⚠ Choose a response column"));
    gui.choose(TITLE, 0, "Length");
    gui.choose(TITLE, 1, "Treatment");
    assert!(gui.shows("⚠ Choose a subject column"));
    gui.choose(TITLE, 2, "Treatment");
    assert!(gui.shows("⚠ Choose three different columns"));
    gui.choose(TITLE, 2, "Mouse");
    let summary = "Length by Treatment, subjects Mouse nested in groups · 72 rows";
    assert!(shows_part(&gui, summary), "{:?}", gui.painted_text());
    gui.shot("dialog-nested-comparison");
    gui.click("Run");
    gui.harness.set_size(egui::vec2(1500.0, 3000.0));
    gui.results();
    for heading in [
        "Group Means (REML fit with Mouse as a random effect",
        "Tests (Mouse as the experimental unit",
        "Variance Components (REML; intraclass correlation",
        "Nested Analysis of Variance (balanced data",
    ] {
        assert!(shows_part(&gui, heading), "{heading} missing: {:?}", gui.painted_text());
    }
    assert!(shows_part(&gui, "2, 9"), "F test on 2 and 9 DF: 12 mice in 3 groups");
    gui.shot("report-nested-comparison");
    edit_and_update(&mut gui, summary);
}

/// The bearing sample's survival report draws the number-at-risk rows
/// under the curves; the options menu turns on the censored counts and the
/// log-rank test for trend.
#[test]
fn survival_number_at_risk_and_trend_test() {
    let mut gui = Gui::with_sample("Bearing life test");
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.retain(|r| matches!(r.saved(), app_model::SavedReport::Survival { .. }));
    assert_eq!(gui.app().documents[active].reports.len(), 1, "the sample has a survival report");
    gui.harness.set_size(egui::vec2(1500.0, 3000.0));
    gui.results();
    assert!(shows_part(&gui, "Number at risk."), "{:?}", gui.painted_text());
    assert!(!shows_part(&gui, "Trend χ²"));
    top_options(&mut gui);
    tick(&mut gui, "Also count censored rows");
    tick(&mut gui, "Log-rank test for trend (groups in order)");
    gui.results();
    let app_model::SavedReport::Survival { options, .. } = gui.app().documents[active].reports[0].saved() else {
        panic!("not a survival report")
    };
    assert!(options.risk_table && options.risk_censored && options.trend);
    assert!(shows_part(&gui, "Trend χ²"), "{:?}", gui.painted_text());
    assert!(shows_part(&gui, "Censored"), "censored counts in the table");
    gui.shot("survival-number-at-risk");
}

/// The k-means dialog's "Compare numbers of clusters" option adds the gap
/// statistic table and plots to the report.
#[test]
fn k_means_compares_numbers_of_clusters() {
    let mut gui = empty_sample("Flower measurements");
    gui.menu(&["Multivariate", "Clustering…"]);
    for c in ["Sepal length", "Sepal width", "Petal length", "Petal width"] {
        if gui.harness.query_all_by_label(c).next().is_some() {
            tick(&mut gui, c);
        }
    }
    tick(&mut gui, "Compare numbers of clusters (gap statistic and silhouettes)");
    gui.click("Run");
    let active = gui.app().active;
    let app_model::SavedReport::Cluster { request } = gui.app().documents[active].reports[0].saved() else {
        panic!("not a clustering report")
    };
    assert_eq!(request.choose, Some(app_model::ChooseClusters::default()));
    gui.harness.set_size(egui::vec2(1500.0, 3600.0));
    gui.results();
    assert!(shows_part(&gui, "Number of Clusters"), "{:?}", gui.painted_text());
    assert!(shows_part(&gui, "Average silhouette"));
}
