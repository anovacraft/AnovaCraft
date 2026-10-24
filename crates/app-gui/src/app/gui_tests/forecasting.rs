//! Series and Signals ▸ Exponential smoothing… and Series and Signals ▸ ARIMA…: the
//! dialogs fill requests from the series settings and model choices, the
//! reports draw the forecast plot, and "Save forecasts as data table" adds
//! the forecasts to the project.

use super::*;
use app_model::SectionKind;

fn clear_reports(gui: &mut Gui) {
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
}

fn is_open(gui: &Gui, title: &str) -> bool {
    gui.harness.query_by_role_and_label(Role::Window, title).is_some()
}

fn first_saved(gui: &Gui) -> app_model::SavedReport {
    gui.app().documents[gui.app().active].reports[0].saved()
}

fn drawn_plots(gui: &mut Gui) -> Vec<SectionKind> {
    gui.harness.set_size(egui::vec2(1500.0, 5200.0));
    gui.results();
    gui.app().documents[gui.app().active].reports[0].plot_keys().into_iter().map(|(k, _)| k).collect()
}

fn open_options(gui: &mut Gui) {
    let options = |n: &Node<'_>| n.role() == Role::Button && n.label().as_deref() == Some("⋯ Options");
    gui.harness.get_all_by(options).next().expect("a report").simulate_click();
    gui.run();
}

/// Exponential smoothing of the hotel occupancy: the month column is taken
/// as the time column, the period comes from it, automatic selection runs,
/// and Save forecasts adds a table with the forecast rows.
#[test]
fn smoothing_dialog_forecasts_hotel_occupancy() {
    const TITLE: &str = "Exponential smoothing";
    let mut gui = Gui::with_sample("Hotel occupancy");
    clear_reports(&mut gui);
    gui.menu(&["Series and Signals", "Exponential smoothing…"]);
    assert!(is_open(&gui, TITLE));
    assert!(gui.shows("⚠ Choose the series to forecast"));
    assert!(gui.widget("Run").is_disabled());
    assert!(gui.harness.query_all_by_label_contains("(12 per cycle)").next().is_some(), "{:?}", gui.painted_text());
    gui.click("All");
    assert!(!gui.widget("Run").is_disabled());
    gui.shot("dialog-exponential-smoothing");
    gui.click("Run");
    assert!(!is_open(&gui, TITLE), "the dialog closes");
    let app_model::SavedReport::ExponentialSmoothing { request } = first_saved(&gui) else {
        panic!("not an exponential smoothing report")
    };
    assert_eq!(request.model, app_model::SmoothingModel::Automatic);
    assert!(request.series.time.is_some() && request.series.period.is_none());
    let kinds = drawn_plots(&mut gui);
    for kind in [SectionKind::ForecastPlot, SectionKind::ForecastResidualPlot, SectionKind::ForecastResidualAcf] {
        assert!(kinds.contains(&kind), "{kind:?} not drawn: {kinds:?}");
    }
    let text = gui.painted_text();
    assert!(text.iter().any(|t| t.starts_with("Model Selection")), "{text:?}");
    assert!(text.iter().any(|t| t.starts_with("Ljung–Box Test")), "{text:?}");
    gui.shot("exponential-smoothing-report");
    let before = gui.app().documents.len();
    open_options(&mut gui);
    gui.click("Save forecasts as data table");
    gui.run();
    assert_eq!(gui.app().documents.len(), before + 1, "a forecast table is added");
    let saved = &gui.app().documents.last().unwrap().table;
    assert_eq!(saved.n_rows(), 108 + 12);
    assert!(saved.find("Upper 95%").is_some());
}

/// ARIMA with chosen orders on the reactor temperatures: the coefficient
/// table names the AR and MA terms, and Edit analysis… reopens the orders.
#[test]
fn arima_dialog_fits_chosen_orders() {
    const TITLE: &str = "ARIMA";
    let mut gui = Gui::with_sample("Reactor temperature");
    clear_reports(&mut gui);
    gui.menu(&["Series and Signals", "ARIMA…"]);
    assert!(is_open(&gui, TITLE));
    gui.click("All");
    gui.click("Choose");
    assert!(gui.shows("Include a drift (trend per observation)"));
    gui.click("Run");
    assert!(!is_open(&gui, TITLE), "the dialog closes");
    let app_model::SavedReport::Arima { request } = first_saved(&gui) else { panic!("not an ARIMA report") };
    assert_eq!(
        request.orders,
        app_model::ArimaOrders::Chosen { regular: (1, 1, 1), seasonal: (0, 0, 0), constant: false }
    );
    let kinds = drawn_plots(&mut gui);
    assert!(kinds.contains(&SectionKind::ForecastPlot), "{kinds:?}");
    let text = gui.painted_text();
    assert!(text.iter().any(|t| t.starts_with("Coefficients")), "{text:?}");
    assert!(text.iter().any(|t| t == "ar1") && text.iter().any(|t| t == "ma1"), "{text:?}");
    gui.shot("arima-report");
    open_options(&mut gui);
    gui.click("Edit analysis…");
    assert!(is_open(&gui, TITLE), "Edit analysis… reopens the dialog");
    let summary = "ARIMA(1,1,1) of Temperature (°C), 12 steps ahead";
    assert!(gui.harness.query_all_by_label_contains(summary).next().is_some(), "{:?}", gui.painted_text());
    gui.click("Update");
    assert_eq!(gui.app().documents[gui.app().active].reports.len(), 1, "Update keeps one report");
}

/// ARIMA with an input series on the kiln data: the input section adds the
/// fuel feed as a transfer-function input (delay from the data), the report
/// draws the impulse response and the residual cross-correlation check, and
/// Edit analysis… reopens with the input chosen.
#[test]
fn arima_dialog_adds_a_transfer_function_input() {
    const TITLE: &str = "ARIMA";
    let mut gui = Gui::with_sample("Kiln fuel feed");
    clear_reports(&mut gui);
    gui.menu(&["Series and Signals", "ARIMA…"]);
    assert!(is_open(&gui, TITLE));
    let check = |gui: &mut Gui, label: &str, last: bool| {
        let is_box = |n: &Node<'_>| n.role() == Role::CheckBox && n.label().as_deref() == Some(label);
        let mut all: Vec<_> = gui.harness.get_all_by(is_box).collect();
        let node = if last { all.pop() } else { all.into_iter().next() }.expect("a check box");
        node.simulate_click();
        gui.run();
    };
    check(&mut gui, "Burning-zone temperature (°C)", false);
    gui.click("Input series (transfer functions)");
    check(&mut gui, "Fuel feed (t/h)", true);
    assert!(gui.shows("From the data"), "{:?}", gui.painted_text());
    let summary = "Best ARIMA model of Burning-zone temperature (°C) with input Fuel feed (t/h), 12 steps ahead";
    assert!(gui.harness.query_all_by_label_contains(summary).next().is_some(), "{:?}", gui.painted_text());
    gui.shot("dialog-arima-input");
    gui.click("Run");
    assert!(!is_open(&gui, TITLE), "the dialog closes");
    let app_model::SavedReport::Arima { request } = first_saved(&gui) else { panic!("not an ARIMA report") };
    let feed = gui.app().documents[gui.app().active].table.find("Fuel feed (t/h)").unwrap();
    assert_eq!(request.inputs, vec![app_model::TransferRole::new(feed)]);
    let kinds = drawn_plots(&mut gui);
    for kind in [SectionKind::ForecastImpulseResponse, SectionKind::ForecastInputResidualCcf, SectionKind::ForecastPlot]
    {
        assert!(kinds.contains(&kind), "{kind:?} not drawn: {kinds:?}");
    }
    let text = gui.painted_text();
    assert!(text.iter().any(|t| t.starts_with("Transfer Functions")), "{text:?}");
    assert!(text.iter().any(|t| t == "Fuel feed (t/h) δ1"), "{text:?}");
    gui.shot("arima-input-report");
    open_options(&mut gui);
    gui.click("Edit analysis…");
    assert!(is_open(&gui, TITLE), "Edit analysis… reopens the dialog");
    assert!(gui.harness.query_all_by_label_contains("with input Fuel feed (t/h)").next().is_some());
}
