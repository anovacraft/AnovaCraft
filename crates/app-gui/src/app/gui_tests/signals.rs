//! Series and Signals ▸ Signals and spectra: the smoothing, derivatives and
//! integrals, baseline correction, peak finding and peak fitting dialogs
//! fill their requests, the reports draw their plots, and the in-place
//! settings change the request.

use super::*;
use app_model::{SavedReport, SectionKind, SignalRequest};

const SAMPLE_RAMAN: &str = "Raman bands";
const SAMPLE_VIBRATION: &str = "Machine vibration";

fn empty_sample(name: &str) -> Gui {
    let mut gui = Gui::with_sample(name);
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui
}

fn first_saved(gui: &Gui) -> SavedReport {
    gui.app().documents[gui.app().active].reports[0].saved()
}

fn is_open(gui: &Gui, title: &str) -> bool {
    gui.harness.query_by_role_and_label(Role::Window, title).is_some()
}

fn drawn_plots(gui: &mut Gui) -> Vec<SectionKind> {
    gui.harness.set_size(egui::vec2(1500.0, 6000.0));
    gui.results();
    gui.app().documents[gui.app().active].reports[0].plot_keys().into_iter().map(|(k, _)| k).collect()
}

fn open_options(gui: &mut Gui) {
    let options = |n: &Node<'_>| n.role() == Role::Button && n.label().as_deref() == Some("⋯ Options");
    gui.harness.get_all_by(options).next().expect("a report").simulate_click();
    gui.run();
}

/// Peak fitting of the Raman spectra: the dialog asks for the columns, fits
/// every sample, draws the fit and the comparison of peak counts, tabulates
/// the parameters by sample; a setting changes in place and Edit analysis…
/// reopens the dialog.
#[test]
fn peak_fitting_dialog_fits_every_spectrum() {
    const TITLE: &str = "Peak fitting";
    let mut gui = empty_sample(SAMPLE_RAMAN);
    gui.menu(&["Series and Signals", "Peak fitting…"]);
    assert!(is_open(&gui, TITLE));
    assert!(gui.shows("⚠ Choose the y column"));
    assert!(gui.widget("Run").is_disabled());
    gui.choose(TITLE, 0, "Intensity (counts)");
    assert!(gui.shows("⚠ Choose the x column"));
    gui.choose(TITLE, 1, "Raman shift (1/cm)");
    gui.choose(TITLE, 2, "Sample");
    assert!(gui.harness.query_all_by_label_contains("Gaussian peak fit of Intensity (counts)").next().is_some());
    gui.shot("dialog-peak-fitting");
    gui.click("Run");
    assert!(!is_open(&gui, TITLE), "the dialog closes");
    let SavedReport::Signal { request: SignalRequest::Spectrum(r) } = first_saved(&gui) else {
        panic!("not a spectrum report")
    };
    assert!(r.fit.is_some() && r.curve.is_some());
    let kinds = drawn_plots(&mut gui);
    for kind in [SectionKind::SpectrumBaselinePlot, SectionKind::SpectrumFitPlot] {
        assert!(kinds.contains(&kind), "{kind:?} not drawn: {kinds:?}");
    }
    let text = gui.painted_text();
    assert!(text.iter().any(|t| t.starts_with("Peak Parameters by Curve")), "{text:?}");
    assert!(text.iter().any(|t| t.starts_with("Numbers of Peaks")), "{text:?}");
    gui.shot("peak-fitting-report");
    open_options(&mut gui);
    gui.click("One width for every peak");
    gui.run();
    let SavedReport::Signal { request: SignalRequest::Spectrum(r) } = first_saved(&gui) else { panic!() };
    assert!(r.fit.unwrap().common_width, "the setting changes the request");
    // The menu stays open after a setting changes.
    gui.click("Edit analysis…");
    assert!(is_open(&gui, TITLE), "Edit analysis… reopens the dialog");
    gui.click("Update");
    assert_eq!(gui.app().documents[gui.app().active].reports.len(), 1, "Update keeps one report");
}

/// Baseline correction opens without a peak fit and takes a known baseline
/// column.
#[test]
fn baseline_correction_subtracts_a_known_baseline() {
    const TITLE: &str = "Baseline correction";
    let mut gui = empty_sample(SAMPLE_RAMAN);
    gui.menu(&["Series and Signals", "Baseline correction…"]);
    gui.choose(TITLE, 0, "Intensity (counts)");
    gui.choose(TITLE, 1, "Raman shift (1/cm)");
    gui.choose(TITLE, 2, "Sample");
    gui.click("Known (a column)");
    assert!(gui.shows("⚠ Choose the baseline column"));
    gui.choose(TITLE, 3, "Background (true)");
    gui.click("Run");
    let SavedReport::Signal { request: SignalRequest::Spectrum(r) } = first_saved(&gui) else { panic!() };
    assert!(r.fit.is_none(), "no fit unless asked");
    let background = gui.app().documents[gui.app().active].table.find("Background (true)").unwrap();
    assert_eq!(r.baseline, app_model::SpectrumBaseline::Column(background));
    assert!(drawn_plots(&mut gui).contains(&SectionKind::SpectrumBaselinePlot));
}

/// Smoothing the vibration signal with a running median, then changing the
/// window in place.
#[test]
fn smoothing_dialog_runs_a_running_median() {
    const TITLE: &str = "Signal smoothing";
    let mut gui = empty_sample(SAMPLE_VIBRATION);
    gui.menu(&["Series and Signals", "Smoothing…"]);
    assert!(gui.shows("⚠ Choose the y column"));
    gui.choose(TITLE, 0, "Acceleration (m/s²)");
    gui.click("Running median");
    gui.shot("dialog-signal-smoothing");
    gui.click("Run");
    let SavedReport::Signal { request: SignalRequest::Smoothing(r) } = first_saved(&gui) else { panic!() };
    assert_eq!(r.method, stat_signal::Smoother::RunningMedian { window: 11 });
    assert!(drawn_plots(&mut gui).contains(&SectionKind::SignalPlot));
    assert!(gui.shows("Smoothing: Acceleration (m/s²) — Running median (11 points)"), "{:?}", gui.painted_text());
}

/// Derivatives and integrals with Simpson's rule and resampling.
#[test]
fn calculus_dialog_integrates_and_resamples() {
    const TITLE: &str = "Derivatives and integrals";
    let mut gui = empty_sample(SAMPLE_VIBRATION);
    gui.menu(&["Series and Signals", "Derivatives and integrals…"]);
    gui.choose(TITLE, 0, "Acceleration (m/s²)");
    gui.choose(TITLE, 1, "Time (s)");
    gui.click("Simpson's rule");
    gui.click("Resample onto evenly spaced x");
    gui.click("Run");
    let SavedReport::Signal { request: SignalRequest::Calculus(r) } = first_saved(&gui) else { panic!() };
    assert_eq!(r.integral, stat_signal::Integration::Simpson);
    assert_eq!(r.resample.map(|s| s.points), Some(101));
    drawn_plots(&mut gui);
    let text = gui.painted_text();
    assert!(text.iter().any(|t| t.starts_with("Resampled Values")), "{text:?}");
    assert!(text.iter().any(|t| t.starts_with("Running Integral")), "{text:?}");
}
