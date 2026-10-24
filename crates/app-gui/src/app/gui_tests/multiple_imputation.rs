//! Multiple imputation: the dialog's settings, the stacked table it makes,
//! and reports pooled across the imputations.

use super::*;
use app_model::SavedReport;
use app_model::report::SectionKind;

const COLUMNS: [&str; 3] = ["Temperature (°C)", "Humidity (%)", "Ozone (µg/m³)"];

/// The air-quality sample with no reports and the imputation dialog open on
/// chained equations with three columns ticked.
fn dialog_on_air_quality() -> (Gui, usize) {
    let mut gui = Gui::with_sample("Air quality");
    gui.harness.set_size(egui::vec2(1400.0, 1500.0));
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui.menu(&["Data", "Impute missing values…"]);
    for c in COLUMNS {
        gui.harness.get_by(|n| n.role() == Role::CheckBox && n.label().as_deref() == Some(c)).simulate_click();
        gui.run();
    }
    gui.click("Multiple imputation: chained equations");
    (gui, active)
}

fn imputation_request(gui: &Gui, doc: usize) -> app_model::ImputationRequest {
    let report =
        gui.app().documents[doc].reports.iter().find(|r| matches!(r.request(), SavedReport::Imputation { .. }));
    let Some(SavedReport::Imputation { request }) = report.map(Report::request) else { panic!("no imputation report") };
    request
}

/// Run makes the imputation report with its diagnostics and, ticked by
/// default, the stacked table in the same project, linked to its source. A
/// linear model added to the stacked table pools across the imputations;
/// unticking "Pool across imputations" turns that off.
#[test]
fn multiple_imputation_makes_a_stacked_table_whose_reports_pool() {
    let (mut gui, source) = dialog_on_air_quality();
    assert!(gui.shows("Make the stacked table"));
    let summary = "Multiple imputation: chained equations of 3 columns";
    assert!(gui.painted_text().iter().any(|t| t.starts_with(summary)), "{:?}", gui.painted_text());
    gui.shot("dialog-multiple-imputation");
    let before = gui.app().documents.len();
    gui.click("Run");

    let request = imputation_request(&gui, source);
    assert_eq!(request.method, app_model::ImputationMethod::ChainedEquations);
    assert_eq!(request.multiple.imputations, 20);
    let doc = &gui.app().documents[source];
    let sections = doc.reports[0].for_export().sections.unwrap();
    for kind in [SectionKind::ImputationModels, SectionKind::ImputationTrace, SectionKind::ImputationDensity] {
        assert!(sections.iter().any(|s| s.kind == kind), "{kind:?}");
    }
    assert_eq!(gui.app().documents.len(), before + 1, "the stacked table opened");
    let stacked = gui.app().active;
    let doc = &gui.app().documents[stacked];
    assert_eq!(doc.table.n_rows(), 21 * gui.app().documents[source].table.n_rows());
    assert_eq!(doc.project, gui.app().documents[source].project, "kept in the source's project");
    let lineage = doc.lineage.as_ref().expect("linked to its source");
    assert!(matches!(lineage.origin.operation, app_model::table_ops::TableOperation::MultipleImputation { .. }));
    assert_eq!(app_model::multiple_imputation::imputation_column(&doc.table), Some(0));

    // A model on the stacked table pools.
    let column = |name: &str| gui.app().documents[stacked].table.find(name).unwrap();
    let spec = app_model::LinearModelSpec {
        response: column(COLUMNS[2]),
        predictors: vec![column(COLUMNS[0]), column(COLUMNS[1])],
        effects: Vec::new(),
        splines: Vec::new(),
    };
    gui.app_mut().add_report(ReportRequest::Linear(spec));
    gui.results();
    let report = &gui.app().documents[stacked].reports[0];
    assert_eq!(report.scope().pool.as_deref(), Some("Imputation"));
    let sections = report.for_export().sections.unwrap();
    assert_eq!(sections[0].kind, SectionKind::PooledNote);
    assert!(gui.shows("Pooled Estimates"));
    assert!(gui.shows("Pooled Tests by Effect (D₁ Wald, D₃ likelihood ratio)"));
    gui.shot("report-pooled-linear-model");

    gui.click("⋯ Options");
    gui.click("Pool across imputations");
    let report = &gui.app().documents[stacked].reports[0];
    assert!(report.scope().pool.is_none());
    assert!(report.for_export().sections.unwrap().iter().all(|s| s.kind != SectionKind::PooledNote));
    assert!(!gui.shows("Pooled Estimates"));
}

/// The predictor matrix, a column's model and the stacked-table tick reach
/// the request; with the tick off no table is made, and "Make stacked
/// table" in the report's options makes it later.
#[test]
fn chained_equation_settings_reach_the_request() {
    let (mut gui, source) = dialog_on_air_quality();
    gui.click("Make the stacked table");
    gui.click("More options");
    gui.click("Humidity (%) predicts Ozone (µg/m³)");
    gui.shot("dialog-multiple-imputation-options");
    // Visit ozone first.
    let up = |n: &Node<'_>| n.role() == Role::Button && n.label().as_deref() == Some("▲");
    gui.harness.get_all_by(up).last().expect("ozone's button").simulate_click();
    gui.run();
    let before = gui.app().documents.len();
    gui.click("Run");
    assert_eq!(gui.app().documents.len(), before, "no stacked table");
    let mi = imputation_request(&gui, source).multiple;
    assert_eq!(
        mi.predictors,
        Some(vec![vec![false, true, true], vec![true, false, true], vec![true, false, false]]),
        "humidity no longer predicts ozone"
    );
    assert_eq!(mi.visit_order, Some(vec![0, 2, 1]));

    gui.results();
    gui.click("⋯ Options");
    gui.click("Make stacked table");
    assert_eq!(gui.app().documents.len(), before + 1);
    let stacked = &gui.app().documents[gui.app().active];
    assert!(stacked.lineage.is_some());
    // The same imputations as the report: same settings and seed.
    let Some(app_model::table_ops::TableOperation::MultipleImputation { settings, .. }) =
        stacked.lineage.as_ref().map(|l| l.origin.operation.clone())
    else {
        panic!("not a multiple imputation")
    };
    assert_eq!(settings.predictors, mi.predictors);
}

/// The clinical trial sample project opens with the trial, its imputation
/// report and the stacked table with two pooled reports.
#[test]
fn clinical_trial_sample_opens_with_pooled_reports() {
    let mut gui = Gui::start();
    gui.harness.set_size(egui::vec2(1400.0, 1500.0));
    let n = app_model::sample::project_samples()
        .iter()
        .position(|s| s.name.starts_with("Clinical trial with dropout"))
        .expect("the sample project");
    gui.perform(Action::OpenProjectSample(n));
    let docs = &gui.app().documents;
    let stacked = docs.iter().position(|d| d.table.name() == "20 imputations").expect("the stacked table");
    assert!(docs[stacked].reports.iter().all(|r| r.scope().pool.is_some()));
    gui.app_mut().set_active(stacked);
    gui.results();
    assert!(gui.shows("Pooled Across Imputations"));
    gui.shot("sample-clinical-trial-pooled");
}
