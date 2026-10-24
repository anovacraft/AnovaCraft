//! Model ▸ Curve templates…: assay templates with their own report sections.

use super::*;

const TEMPLATES: &str = "Curve templates";

fn says(gui: &Gui, text: &str) -> bool {
    gui.painted_text().iter().any(|t| t.contains(text))
}

/// A 4PL standard curve with a dilution column: the report checks the
/// standards and reads the samples off the curve, and "Edit analysis…"
/// reopens this dialog (not the general nonlinear one) filled from it.
#[test]
fn standard_curve_template_reads_samples_and_reopens() {
    let mut gui = Gui::with_sample("ELISA plate");
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui.menu(&["Model", "Curve templates…"]);
    assert!(gui.widget("Run").is_disabled());
    assert!(gui.shows("⚠ Choose a response"));
    assert!(gui.shows("Y = D + (A − D) / (1 + (X/C)^B)"));
    gui.choose(TEMPLATES, 2, "OD 450");
    gui.choose(TEMPLATES, 3, "Concentration (pg/ml)");
    gui.choose(TEMPLATES, 5, "Dilution factor");
    gui.run();
    let summary = "Four-parameter logistic (4PL) standard curve of OD 450 on Concentration (pg/ml)";
    assert!(says(&gui, summary), "{:?}", gui.painted_text());
    gui.click("Run");
    gui.results();
    let reports = &gui.app().documents[active].reports;
    assert_eq!(reports.len(), 1);
    let app_model::SavedReport::Nonlinear { request, .. } = reports[0].saved() else {
        panic!("not a nonlinear report")
    };
    let template = request.template.as_ref().expect("a template report");
    assert_eq!(template.template, app_model::curve_templates::CurveTemplate::FourPl);
    assert!(template.dilution.is_some());
    assert_eq!(request.weighting, app_model::nonlinear_analysis::Weighting::InverseResponseSquared);
    assert!(gui.shows("Standard Curve Acceptance (ICH M10)"));
    assert!(gui.shows("Back-Calculated Standards"));
    let id = gui.app().documents[active].reports[0].id();

    let options = |n: &Node<'_>| n.role() == Role::Button && n.label().as_deref() == Some("⋯ Options");
    gui.harness.get_all_by(options).next().expect("a report").simulate_click();
    gui.run();
    gui.click("Edit analysis…");
    assert!(gui.harness.query_all_by_label(TEMPLATES).next().is_some(), "the template dialog opened");
    assert!(says(&gui, summary), "filled from the report: {:?}", gui.painted_text());
    gui.click("Update");
    assert_eq!(gui.app().documents[active].reports.len(), 1);
    assert_eq!(gui.app().documents[active].reports[0].id(), id, "updated in place");
}

/// Switching the experiment type lists its templates; an inhibition
/// template asks for the inhibitor column and compares the mechanisms.
#[test]
fn inhibition_template_needs_the_inhibitor_and_compares_mechanisms() {
    let mut gui = Gui::with_sample("Enzyme inhibition");
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui.menu(&["Model", "Curve templates…"]);
    gui.choose(TEMPLATES, 0, "Enzyme kinetics");
    gui.choose(TEMPLATES, 1, "Mixed inhibition");
    gui.choose(TEMPLATES, 2, "Rate (nmol/min)");
    gui.choose(TEMPLATES, 3, "Substrate (µM)");
    gui.run();
    assert!(gui.widget("Run").is_disabled());
    assert!(says(&gui, "Choose the inhibitor"), "{:?}", gui.painted_text());
    gui.choose(TEMPLATES, 4, "Inhibitor (µM)");
    gui.run();
    gui.click("Run");
    gui.results();
    assert_eq!(gui.app().documents[active].reports.len(), 1);
    // The mechanism table, with mixed inhibition best supported.
    assert!(says(&gui, "Akaike weight (%)"), "{:?}", gui.painted_text());
    assert!(says(&gui, "Mixed inhibition: Rate (nmol/min) by Substrate (µM)"));
    gui.shot("curve-template-inhibition");
}
