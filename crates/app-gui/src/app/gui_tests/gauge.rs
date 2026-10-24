//! The gauge R&R dialog's study types: nested (destructive) and expanded.

use super::*;

const GAUGE: &str = "Gauge R&R study";

/// Opens sample `name` showing its results, with its reports closed.
fn empty_sample(name: &str) -> (Gui, usize) {
    let mut gui = Gui::with_sample(name);
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui.results();
    (gui, active)
}

#[test]
fn gauge_dialog_runs_a_nested_study_with_a_run_order() {
    let (mut gui, active) = empty_sample("Weld pull test");
    gui.menu(&["Quality", "Gauge R&R…"]);
    // Study type, part, operator, measurement, run order.
    gui.choose(GAUGE, 0, "Nested (destructive: parts within operators)");
    assert!(!gui.shows("Pool interaction into error when p > 0.05"), "pooling applies to crossed studies only");
    assert!(gui.harness.get_by(main_run).is_disabled(), "no measurement column guessed");
    gui.choose(GAUGE, 3, "Strength");
    gui.choose(GAUGE, 4, "Run");
    gui.harness.get_by(main_run).simulate_click();
    gui.run();
    let reports = &gui.app().documents[active].reports;
    assert_eq!(reports.len(), 1);
    let app_model::SavedReport::Gauge { request, .. } = reports[0].saved() else { panic!("not a gauge report") };
    assert_eq!(request.design.kind, app_model::GaugeStudyKind::Nested);
    assert_eq!(request.design.run_order, gui.app().documents[active].table.find("Run"));
    assert!(gui.shows("Nested gauge R&R: Strength"));
    assert!(gui.shows("Analysis of Variance"));
    gui.shot("gauge-nested-report");
}

#[test]
fn gauge_dialog_runs_an_expanded_study_with_an_extra_factor() {
    let mut gui = Gui::with_sample("Bore gauges");
    gui.results();
    // The sample opens with the expanded study and its run chart.
    assert!(gui.shows("Expanded gauge R&R: Diameter"));
    assert!(gui.shows("Gauge Run Chart"));
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui.run();

    gui.menu(&["Quality", "Gauge R&R…"]);
    gui.choose(GAUGE, 0, "Expanded (extra factors, unbalanced data)");
    gui.choose(GAUGE, 3, "Diameter");
    gui.click("Add factor");
    assert!(gui.harness.get_by(main_run).is_disabled(), "the new factor has no column yet");
    assert!(gui.shows("⚠ Choose a column for each extra factor, or remove it"));
    gui.choose(GAUGE, 5, "Gauge");
    gui.choose(GAUGE, 6, "Fixed");
    gui.harness.get_by(main_run).simulate_click();
    gui.run();
    let reports = &gui.app().documents[active].reports;
    assert_eq!(reports.len(), 1);
    let app_model::SavedReport::Gauge { request, .. } = reports[0].saved() else { panic!("not a gauge report") };
    assert_eq!(request.design.kind, app_model::GaugeStudyKind::Expanded);
    let gauge = gui.app().documents[active].table.find("Gauge").unwrap();
    assert_eq!(
        request.design.factors,
        [app_model::GaugeExtraFactor { column: gauge, random: false, nesting: app_model::GaugeFactorNesting::Crossed }]
    );
    assert!(gui.shows("Expanded gauge R&R: Diameter"));
    assert!(gui.shows("Variance Component Intervals"));
    assert!(gui.shows("Tests of Fixed Factors"));
    gui.shot("gauge-expanded-report");
}

/// The dialog's Run button, not the "Run" entry of a column list.
fn main_run(n: &Node<'_>) -> bool {
    n.role() == Role::Button && n.toggled().is_none() && n.label().as_deref() == Some("Run")
}
