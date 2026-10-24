//! The attribute measurement study dialogs (Quality menu).

use super::*;

/// Quality ▸ Attribute agreement…: the dialog guesses the part, inspector,
/// trial and standard columns; an ordinal rating turns on the ordered
/// analysis with its category order; a column in two roles is refused; the
/// report shows every comparison and the agreement plot.
#[test]
fn attribute_agreement_dialog_runs_the_ordinal_study() {
    const TITLE: &str = "Attribute agreement";
    let mut gui = Gui::with_sample("Visual inspection");
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui.menu(&["Quality", "Attribute agreement…"]);
    assert!(!gui.widget("Run").is_disabled(), "the columns are guessed from their names");
    gui.choose(TITLE, 2, "Grade");
    gui.choose(TITLE, 4, "Grade");
    assert!(gui.shows("⚠ A column is chosen twice"));
    assert!(gui.widget("Run").is_disabled());
    gui.choose(TITLE, 4, "Standard grade");
    assert!(!gui.shows("⚠ A column is chosen twice"));
    assert!(gui.shows("1. 1") && gui.shows("5. 5"), "an ordinal rating lists its categories in order");
    gui.shot("attribute-agreement-dialog");
    gui.click("Run");
    assert!(gui.harness.query_by_role_and_label(Role::Window, TITLE).is_none(), "the dialog closes");
    let reports = &gui.app().documents[active].reports;
    assert_eq!(reports.len(), 1);
    let app_model::SavedReport::AttributeAgreement { request } = reports[0].saved() else {
        panic!("not an attribute agreement report")
    };
    assert!(request.ordinal);
    assert_eq!(request.trial, gui.app().documents[active].table.find("Trial"));
    assert_eq!(request.order.len(), 5);
    gui.harness.set_size(egui::vec2(1500.0, 3600.0));
    gui.results();
    assert_eq!(reports_title(&gui), "Attribute Agreement: Grade");
    for heading in [
        "Within Each Appraiser (repeat trials)",
        "Each Appraiser Against the Standard",
        "Between Appraisers",
        "Ordered Categories Against the Standard",
    ] {
        assert!(gui.shows(heading), "{heading} missing");
    }
    let kinds: Vec<app_model::SectionKind> =
        gui.app().documents[active].reports[0].plot_keys().into_iter().map(|(k, _)| k).collect();
    assert!(kinds.contains(&app_model::SectionKind::AgreementPlot), "{kinds:?}");
    let text = gui.painted_text();
    assert!(text.iter().any(|t| t == "Within appraiser"), "the plot's key names both series");
    gui.shot("attribute-agreement-report");
}

fn reports_title(gui: &Gui) -> String {
    let active = gui.app().active;
    gui.app().documents[active].reports[0].title()
}

/// Quality ▸ Attribute gauge bias and repeatability…: counts per row with a
/// typed limit give the bias report and the gauge curve.
#[test]
fn attribute_gauge_dialog_reports_bias() {
    const TITLE: &str = "Attribute gauge bias and repeatability";
    let mut gui = Gui::with_sample("Go/no-go gauge");
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui.menu(&["Quality", "Attribute gauge bias and repeatability…"]);
    let limit = gui.window(TITLE).get_all_by_role(Role::SpinButton).next().expect("the limit field").id();
    gui.set_number(limit, 12.0);
    gui.click("Run");
    let reports = &gui.app().documents[active].reports;
    assert_eq!(reports.len(), 1);
    let app_model::SavedReport::AttributeGauge { request } = reports[0].saved() else {
        panic!("not an attribute gauge report")
    };
    assert!((request.limit - 12.0).abs() < 1e-12);
    assert!(request.checks.is_some());
    gui.results();
    assert!(gui.shows("Bias and Repeatability"));
    assert!(gui.shows("Gauge Performance Curve"));
    gui.shot("attribute-gauge-report");
}
