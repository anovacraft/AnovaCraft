//! Reliability ▸ Proportional hazards… and Reliability ▸ Parametric
//! survival regression…: dialogs filled and run, and their reports.

use super::*;

/// Tick the first check box labelled `label` (the predictor list comes
/// before the effect check boxes of the same columns).
fn tick(gui: &mut Gui, label: &str) {
    let is_box = |n: &Node<'_>| n.role() == Role::CheckBox && n.label().as_deref() == Some(label);
    gui.harness.get_all_by(is_box).next().unwrap_or_else(|| panic!("no check box {label}")).simulate_click();
    gui.run();
}

/// Section kinds drawn as plots by report `index` of the active document.
fn plot_kinds(gui: &Gui, index: usize) -> Vec<app_model::SectionKind> {
    let active = gui.app().active;
    gui.app().documents[active].reports[index].plot_keys().into_iter().map(|(k, _)| k).collect()
}

/// A Cox model of the leukemia remissions: time, event coding, two
/// predictors and their interaction, curves by treatment; the report shows
/// hazard ratios, the proportional-hazards test, the Schoenfeld and survival
/// plots and the Response Explorer.
#[test]
fn proportional_hazards_dialog_runs_a_cox_model() {
    const PH: &str = "Proportional hazards";
    let mut gui = Gui::with_sample("Leukemia with covariates");
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui.menu(&["Reliability", "Proportional hazards…"]);
    assert!(gui.widget("Run").is_disabled(), "no time column chosen yet");
    gui.choose(PH, 0, "Weeks");
    gui.choose(PH, 1, "Relapse");
    gui.click("Non-zero = failure (event)");
    assert!(gui.shows("⚠ Choose at least one predictor"));
    tick(&mut gui, "Treatment");
    tick(&mut gui, "log WBC");
    gui.click("Treatment × log WBC");
    // Drop-downs: time, censor, strata, then "Survival curves for".
    gui.choose(PH, 3, "Treatment");
    gui.click("Run");
    assert!(gui.harness.query_by_role_and_label(Role::Window, PH).is_none(), "the dialog closes");
    let reports = &gui.app().documents[active].reports;
    assert_eq!(reports.len(), 1);
    let app_model::SavedReport::ProportionalHazards { request, .. } = reports[0].saved() else {
        panic!("not a proportional hazards report")
    };
    assert_eq!(request.coding, app_model::StatusCoding::NonzeroIsEvent);
    assert_eq!(request.predictors.len(), 2);
    assert_eq!(request.effects.len(), 3, "two main effects and their interaction");
    assert!(request.curve_by.is_some());
    gui.harness.set_size(egui::vec2(1500.0, 4200.0));
    gui.results();
    let kinds = plot_kinds(&gui, 0);
    for kind in [
        app_model::SectionKind::HazardSchoenfeldPlot,
        app_model::SectionKind::HazardSurvivalPlot,
        app_model::SectionKind::HazardResiduals,
    ] {
        assert!(kinds.contains(&kind), "{kind:?} not drawn: {kinds:?}");
    }
    let text = gui.painted_text();
    for start in ["Effect Pareto", "Coefficients and Hazard Ratios", "Proportional Hazards Check", "Response Explorer"]
    {
        assert!(text.iter().any(|t| t.starts_with(start)), "no {start}: {text:?}");
    }
    assert!(text.iter().any(|t| t == "6-MP"), "a curve per treatment");
    gui.shot("proportional-hazards-report");
}

/// The capacitor inspections fitted as interval-censored Weibull lives on
/// temperature and lot, then switched to lognormal through Edit analysis:
/// the report shows the distribution comparison, time ratios, the fitted
/// survival and the Cox–Snell check.
#[test]
fn survival_regression_dialog_fits_interval_data() {
    const AFT: &str = "Parametric survival regression";
    let mut gui = Gui::with_sample("Capacitor life test");
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui.menu(&["Reliability", "Parametric survival regression…"]);
    assert!(gui.widget("Run").is_disabled(), "no time column chosen yet");
    gui.click("Start and end of the failure interval");
    gui.choose(AFT, 0, "Last pass (h)");
    gui.choose(AFT, 2, "Units");
    tick(&mut gui, "Temperature (°C)");
    tick(&mut gui, "Lot");
    gui.choose(AFT, 1, "Last pass (h)");
    assert!(gui.shows("⚠ A column is chosen twice"));
    gui.choose(AFT, 1, "Found failed (h)");
    assert!(!gui.shows("⚠ A column is chosen twice"));
    gui.click("Lognormal");
    gui.click("Run");
    assert!(gui.harness.query_by_role_and_label(Role::Window, AFT).is_none(), "the dialog closes");
    let reports = &gui.app().documents[active].reports;
    assert_eq!(reports.len(), 1);
    let app_model::SavedReport::SurvivalRegression { request, .. } = reports[0].saved() else {
        panic!("not a survival regression report")
    };
    assert!(matches!(request.layout, app_model::LifeDataLayout::Intervals { .. }));
    assert_eq!(request.distribution, app_model::AftDistribution::Lognormal);
    assert!(request.frequency.is_some());
    assert_eq!(request.predictors.len(), 2);
    gui.harness.set_size(egui::vec2(1500.0, 4200.0));
    gui.results();
    let kinds = plot_kinds(&gui, 0);
    for kind in
        [app_model::SectionKind::SurvivalRegressionSurvivalPlot, app_model::SectionKind::SurvivalRegressionResiduals]
    {
        assert!(kinds.contains(&kind), "{kind:?} not drawn: {kinds:?}");
    }
    let text = gui.painted_text();
    for start in ["Effect Pareto", "Distribution Comparison", "Time Ratios", "Response Explorer"] {
        assert!(text.iter().any(|t| t.starts_with(start)), "no {start}: {text:?}");
    }
    gui.shot("survival-regression-report");
}

/// The capacitor sample opens with its Weibull regression already run, and
/// its dialog reopens filled from the report.
#[test]
fn survival_regression_sample_reopens_in_its_dialog() {
    let mut gui = Gui::with_sample("Capacitor life test");
    let active = gui.app().active;
    let reports = &gui.app().documents[active].reports;
    assert_eq!(reports.len(), 1);
    let saved = reports[0].saved();
    assert!(matches!(saved, app_model::SavedReport::SurvivalRegression { .. }));
    let id = reports[0].id();
    gui.app_mut().edit_report(id);
    gui.run();
    assert!(gui.harness.query_by_role_and_label(Role::Window, "Parametric survival regression").is_some());
    let text = gui.painted_text();
    assert!(text.iter().any(|t| t == "Update"), "the dialog edits the report");
    assert!(!text.iter().any(|t| t.starts_with("⚠ ")), "the dialog reopens complete");
}
