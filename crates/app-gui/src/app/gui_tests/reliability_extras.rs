//! Reliability ▸ Competing risks… and Reliability ▸ Warranty returns….

use super::*;

/// The competing-risks dialog guesses the censored value from the cause
/// column, refuses a column in two roles, and runs a grouped analysis with
/// failure-mode fits whose report draws the incidence and system plots.
#[test]
fn competing_risks_dialog_runs_grouped_analysis() {
    const TITLE: &str = "Competing risks";
    let mut gui = Gui::with_sample("Pump failure modes");
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui.menu(&["Reliability", "Competing risks…"]);
    assert!(gui.widget("Run").is_disabled(), "no columns chosen yet");
    gui.choose(TITLE, 0, "Hours");
    gui.choose(TITLE, 1, "Hours");
    assert!(gui.widget("Run").is_disabled());
    gui.choose(TITLE, 1, "Failure mode");
    assert!(gui.shows("Running"), "the censored value is guessed from the cause column");
    gui.choose(TITLE, 3, "Site");
    gui.click("Fit a life distribution to each failure mode:");
    assert!(!gui.widget("Run").is_disabled());
    gui.click("Run");
    assert!(gui.harness.query_by_role_and_label(Role::Window, TITLE).is_none(), "the dialog closes");
    let reports = &gui.app().documents[active].reports;
    assert_eq!(reports.len(), 1);
    let app_model::SavedReport::CompetingRisks { request } = reports[0].saved() else {
        panic!("not a competing-risks report")
    };
    assert_eq!(request.censor_value.as_deref(), Some("Running"));
    assert!(request.group.is_some() && request.mode_family.is_some() && !request.stacked);
    gui.harness.set_size(egui::vec2(1500.0, 3600.0));
    gui.results();
    let kinds: Vec<app_model::SectionKind> =
        gui.app().documents[active].reports[0].plot_keys().into_iter().map(|(k, _)| k).collect();
    for kind in [app_model::SectionKind::CumulativeIncidencePlot, app_model::SectionKind::SeriesSystemPlot] {
        assert!(kinds.contains(&kind), "{kind:?} not drawn: {kinds:?}");
    }
    let text = gui.painted_text();
    assert!(text.iter().any(|t| t.starts_with("Gray's Test")), "{text:?}");
    gui.shot("competing-risks-report");
}

/// The warranty dialog takes the shipped column and the return columns of a
/// triangular table and forecasts returns: the report shows the fit and the
/// forecast plot.
#[test]
fn warranty_dialog_forecasts_returns() {
    const TITLE: &str = "Warranty returns";
    let mut gui = Gui::with_sample("Monthly warranty returns");
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui.menu(&["Reliability", "Warranty returns…"]);
    assert!(gui.widget("Run").is_disabled(), "no columns chosen yet");
    gui.choose(TITLE, 0, "Shipped");
    gui.choose(TITLE, 1, "Ship month");
    assert!(gui.shows("⚠ Choose the return columns, one per calendar period"));
    gui.click("All");
    assert!(!gui.widget("Run").is_disabled());
    gui.click("Run");
    assert!(gui.harness.query_by_role_and_label(Role::Window, TITLE).is_none(), "the dialog closes");
    let reports = &gui.app().documents[active].reports;
    assert_eq!(reports.len(), 1);
    let app_model::SavedReport::Warranty { request } = reports[0].saved() else { panic!("not a warranty report") };
    let app_model::WarrantyLayout::Triangular { returns, label, .. } = &request.layout else {
        panic!("not the triangular layout")
    };
    assert_eq!(returns.len(), 12);
    assert!(label.is_some());
    gui.harness.set_size(egui::vec2(1500.0, 3600.0));
    gui.results();
    let kinds: Vec<app_model::SectionKind> =
        gui.app().documents[active].reports[0].plot_keys().into_iter().map(|(k, _)| k).collect();
    for kind in [app_model::SectionKind::LifeProbabilityPlot, app_model::SectionKind::WarrantyForecastPlot] {
        assert!(kinds.contains(&kind), "{kind:?} not drawn: {kinds:?}");
    }
    let text = gui.painted_text();
    assert!(text.iter().any(|t| t.starts_with("Forecast Returns per Period")), "{text:?}");
    gui.shot("warranty-report");
}
