//! Series and Signals ▸ Explore: the four dialogs fill their requests, refuse
//! incomplete choices, and their reports draw their plots; options menu
//! items add sections and save columns.

use super::*;
use app_model::{ExploreMethod, SectionKind};

fn clear_reports(gui: &mut Gui) {
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
}

fn tick(gui: &mut Gui, label: &str) {
    gui.harness.get_by(|n| n.role() == Role::CheckBox && n.label().as_deref() == Some(label)).simulate_click();
    gui.run();
}

fn request(gui: &Gui, i: usize) -> app_model::SeriesExploreRequest {
    let active = gui.app().active;
    match gui.app().documents[active].reports[i].saved() {
        app_model::SavedReport::SeriesExplore { request } => request,
        other => panic!("not a time series report: {other:?}"),
    }
}

fn plotted(gui: &Gui) -> Vec<SectionKind> {
    let active = gui.app().active;
    gui.app().documents[active].reports[0].plot_keys().into_iter().map(|(k, _)| k).collect()
}

fn painted(gui: &Gui, start: &str) -> bool {
    gui.painted_text().iter().any(|t| t.starts_with(start))
}

/// Series description: the date column is taken as the time axis (period
/// 12), the report draws the series, ACF and PACF and tests for a unit
/// root, the options menu adds the spectrum, and the report reopens in its
/// dialog.
#[test]
fn series_description_dialog_runs_and_adds_the_spectrum() {
    const TITLE: &str = "Series description";
    let mut gui = Gui::with_sample("Monthly electricity demand");
    clear_reports(&mut gui);
    gui.menu(&["Series and Signals", "Series description…"]);
    assert!(gui.shows("⚠ Choose at least one series column"));
    assert!(gui.widget("Run").is_disabled());
    assert!(gui.shows("From the time column (12)"), "monthly dates give 12 per cycle");
    tick(&mut gui, "Demand (GWh)");
    assert!(painted(&gui, "Series Description of Demand (GWh)"), "{:?}", gui.painted_text());
    gui.shot("dialog-series-description");
    gui.click("Run");
    assert!(gui.harness.query_by_role_and_label(Role::Window, TITLE).is_none(), "the dialog closes");
    let r = request(&gui, 0);
    assert!(r.series.time.is_some() && r.series.period.is_none());
    let ExploreMethod::Describe(d) = &r.method else { panic!("not a description") };
    assert!(d.unit_root && !d.spectrum);
    gui.harness.set_size(egui::vec2(1500.0, 4200.0));
    gui.results();
    for kind in [SectionKind::SeriesPlot, SectionKind::SeriesAcfPlot, SectionKind::SeriesPacfPlot] {
        assert!(plotted(&gui).contains(&kind), "{kind:?} not drawn");
    }
    assert!(painted(&gui, "Unit-Root and Stationarity Tests"));
    assert!(painted(&gui, "Ljung–Box Tests"));
    // The options menu adds the spectrum.
    gui.click("⋯ Options");
    tick(&mut gui, "Spectral density");
    gui.run();
    let ExploreMethod::Describe(d) = &request(&gui, 0).method else { panic!() };
    assert!(d.spectrum);
    assert!(plotted(&gui).contains(&SectionKind::SeriesSpectrumPlot), "spectrum drawn after the option");
    assert!(painted(&gui, "Largest Spectral Peaks"));
    gui.shot("time-series-description");
    // Edit analysis… reopens the dialog with the report's settings.
    let active = gui.app().active;
    let id = gui.app().documents[active].reports[0].id();
    gui.app_mut().edit_report(id);
    gui.run();
    assert!(gui.shows("Update"));
    gui.click("Box–Cox, λ by Guerrero's method");
    gui.click("Update");
    let ExploreMethod::Describe(d) = &request(&gui, 0).method else { panic!() };
    assert_eq!(d.box_cox, app_model::BoxCoxChoice::Guerrero);
    assert!(d.spectrum, "display settings kept");
}

/// Decomposition by robust STL draws the four panels; the options menu
/// saves the trend as a column.
#[test]
fn decomposition_dialog_runs_stl_and_saves_the_trend() {
    let mut gui = Gui::with_sample("Monthly electricity demand");
    clear_reports(&mut gui);
    gui.menu(&["Series and Signals", "Decomposition…"]);
    tick(&mut gui, "Demand (GWh)");
    gui.click("STL (LOESS)");
    tick(&mut gui, "Robust to outliers (downweight unusual values)");
    gui.click("Run");
    let r = request(&gui, 0);
    let ExploreMethod::Decompose(d) = r.method else { panic!("not a decomposition") };
    assert_eq!(d.method, app_model::DecomposeMethod::Stl);
    assert!(d.robust && d.seasonal_window.is_none());
    gui.harness.set_size(egui::vec2(1500.0, 3600.0));
    gui.results();
    for kind in [
        SectionKind::DecompositionObserved,
        SectionKind::DecompositionTrend,
        SectionKind::DecompositionSeasonal,
        SectionKind::DecompositionRemainder,
    ] {
        assert!(plotted(&gui).contains(&kind), "{kind:?} not drawn");
    }
    assert!(painted(&gui, "Decomposition Summary"));
    gui.shot("time-series-decomposition");
    let active = gui.app().active;
    let before = gui.app().documents[active].table.n_cols();
    gui.click("⋯ Options");
    gui.click("Save trend component");
    let table = &gui.app().documents[active].table;
    assert_eq!(table.n_cols(), before + 1);
    assert!(table.find("Trend Demand (GWh)").is_some());
}

/// Trend models: a logistic S-curve with forecasts.
#[test]
fn trend_models_dialog_fits_a_logistic_curve() {
    let mut gui = Gui::with_sample("Product adoption");
    clear_reports(&mut gui);
    gui.menu(&["Series and Signals", "Trend models…"]);
    tick(&mut gui, "Customers");
    gui.click("Logistic S-curve");
    assert!(painted(&gui, "Logistic S-curve trend of Customers"), "{:?}", gui.painted_text());
    gui.click("Run");
    let ExploreMethod::Trend(t) = request(&gui, 0).method else { panic!("not a trend model") };
    assert_eq!(t.model, app_model::TrendChoice::Logistic);
    gui.harness.set_size(egui::vec2(1500.0, 2600.0));
    gui.results();
    assert!(plotted(&gui).contains(&SectionKind::TrendModelPlot));
    assert!(painted(&gui, "Fitted Equation"));
    assert!(painted(&gui, "Accuracy of the Fitted Values"));
    assert!(gui.shows("Midpoint t = ln(a)/b"));
    gui.shot("time-series-trend");
}

/// Cross-correlation needs an input series different from the outputs;
/// prewhitened, the report explains the lags and lists the filter.
#[test]
fn cross_correlation_dialog_prewhitens_the_input() {
    const TITLE: &str = "Cross-correlation";
    let mut gui = Gui::with_sample("Advertising and sales");
    clear_reports(&mut gui);
    gui.menu(&["Series and Signals", "Cross-correlation…"]);
    assert!(gui.shows("⚠ Choose the input series"));
    gui.choose(TITLE, 0, "Advertising (k$)");
    tick(&mut gui, "Advertising (k$)");
    assert!(gui.shows("⚠ The input series cannot also be an output series"));
    tick(&mut gui, "Advertising (k$)");
    tick(&mut gui, "Sales (k$)");
    gui.click("Autoregression on the input, order by AIC");
    assert!(!gui.widget("Run").is_disabled());
    gui.click("Run");
    let r = request(&gui, 0);
    let ExploreMethod::CrossCorrelation(c) = r.method else { panic!("not a cross-correlation") };
    assert_eq!(c.prewhiten, app_model::Prewhiten::Aic);
    let active = gui.app().active;
    assert_eq!(gui.app().documents[active].table.find("Advertising (k$)"), Some(c.input));
    gui.harness.set_size(egui::vec2(1500.0, 3000.0));
    gui.results();
    assert!(plotted(&gui).contains(&SectionKind::CrossCorrelationPlot));
    assert!(painted(&gui, "How to Read the Lags"));
    assert!(painted(&gui, "Prewhitening Filter"));
    gui.shot("time-series-cross-correlation");
}
