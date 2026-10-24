//! Series and Signals ▸ Signals and spectra: the four dialogs fill their requests and refuse
//! incomplete choices, the reports draw their plots and time–frequency maps,
//! options menu items save columns, and a report reopens in its dialog.

use super::*;
use app_model::signal_analysis::{CorrelationOperation, FilterDesign};
use app_model::{SectionKind, SignalMethod};

const SAMPLE: &str = "Test bench signals";

fn clear_reports(gui: &mut Gui) {
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
}

fn tick(gui: &mut Gui, label: &str) {
    gui.harness.get_by(|n| n.role() == Role::CheckBox && n.label().as_deref() == Some(label)).simulate_click();
    gui.run();
}

fn request(gui: &Gui, i: usize) -> app_model::SignalProcessingRequest {
    let active = gui.app().active;
    match gui.app().documents[active].reports[i].saved() {
        app_model::SavedReport::SignalProcessing { request } => request,
        other => panic!("not a signal report: {other:?}"),
    }
}

fn plotted(gui: &Gui) -> Vec<SectionKind> {
    let active = gui.app().active;
    gui.app().documents[active].reports[0].plot_keys().into_iter().map(|(k, _)| k).collect()
}

fn painted(gui: &Gui, start: &str) -> bool {
    gui.painted_text().iter().any(|t| t.starts_with(start))
}

/// Fourier spectrum: the x and group columns set the sampling (500 per
/// second, shown in the dialog), the report draws the spectra and a
/// spectrogram with its colour bar.
#[test]
fn spectrum_dialog_reads_the_sampling_and_draws_a_spectrogram() {
    const TITLE: &str = "Fourier spectrum";
    let mut gui = Gui::with_sample(SAMPLE);
    clear_reports(&mut gui);
    gui.menu(&["Series and Signals", "Fourier spectrum…"]);
    assert!(gui.shows("⚠ Choose at least one signal column"));
    assert!(gui.widget("Run").is_disabled());
    tick(&mut gui, "Microphone");
    gui.choose(TITLE, 0, "Time (s)");
    gui.choose(TITLE, 1, "Run");
    assert!(painted(&gui, "Sampling rate 500; frequencies up to 250"), "{:?}", gui.painted_text());
    tick(&mut gui, "Spectrogram (spectrum over time)");
    gui.shot("dialog-fourier-spectrum");
    gui.click("Run");
    assert!(gui.harness.query_by_role_and_label(Role::Window, TITLE).is_none(), "the dialog closes");
    let r = request(&gui, 0);
    assert!(r.input.x.is_some() && r.input.group.is_some());
    let SignalMethod::Spectrum(s) = &r.method else { panic!("not a spectrum") };
    assert!(s.spectrogram && !s.welch);
    gui.harness.set_size(egui::vec2(1500.0, 4800.0));
    gui.results();
    for kind in [SectionKind::SignalTrace, SectionKind::AmplitudeSpectrum, SectionKind::PowerSpectrum] {
        assert!(plotted(&gui).contains(&kind), "{kind:?} not drawn");
    }
    assert!(plotted(&gui).contains(&SectionKind::Spectrogram), "spectrogram drawn");
    assert!(painted(&gui, "Spectrogram: A"));
    assert!(painted(&gui, "Dominant Frequencies"));
    assert!(gui.painted_text().iter().any(|t| t.contains("Power") && t.contains("dB")), "colour bar title");
    gui.shot("signal-spectrum");
}

/// Digital filter: without an x column the rate is one per row, so the
/// cut-off starts at a fifth of the Nyquist frequency; an elliptic filter
/// applied forwards adds a phase plot, and the options menu saves the
/// filtered signal.
#[test]
fn filter_dialog_designs_an_elliptic_filter_and_saves_the_output() {
    const TITLE: &str = "Digital filter";
    let mut gui = Gui::with_sample(SAMPLE);
    clear_reports(&mut gui);
    gui.menu(&["Series and Signals", "Digital filter…"]);
    assert!(gui.shows("— (choose a signal first)"), "no cut-off before the sampling rate is known");
    tick(&mut gui, "Microphone");
    assert!(painted(&gui, "Sampling rate 1; frequencies up to 0.5"), "{:?}", gui.painted_text());
    gui.choose(TITLE, 2, "Elliptic (sharpest cut-off)");
    assert!(gui.shows("Stop-band attenuation (dB):"));
    tick(&mut gui, "Zero phase (filter forwards and backwards)");
    gui.shot("dialog-digital-filter");
    gui.click("Run");
    let r = request(&gui, 0);
    let SignalMethod::Filter(f) = &r.method else { panic!("not a filter") };
    assert!(matches!(
        f.design,
        FilterDesign::Iir { family: stat_signal::filter::IirFamily::Elliptic { .. }, order: 4 }
    ));
    assert!((f.low - 0.1).abs() < 1e-12 && !f.zero_phase);
    gui.harness.set_size(egui::vec2(1500.0, 4200.0));
    gui.results();
    for kind in [SectionKind::FilteredSignal, SectionKind::FilterResponse, SectionKind::FilterPhase] {
        assert!(plotted(&gui).contains(&kind), "{kind:?} not drawn");
    }
    assert!(painted(&gui, "Second-Order Sections"));
    let active = gui.app().active;
    let before = gui.app().documents[active].table.n_cols();
    gui.click("⋯ Options");
    gui.click("Save filtered signal");
    let table = &gui.app().documents[active].table;
    assert_eq!(table.n_cols(), before + 1);
    assert!(table.find("Filtered Microphone").is_some());
}

/// Convolution and correlation: a cross-correlation needs a second signal;
/// the envelope saves three columns' worth of choices.
#[test]
fn correlation_dialog_needs_a_second_signal_and_traces_the_envelope() {
    const TITLE: &str = "Convolution and correlation";
    let mut gui = Gui::with_sample(SAMPLE);
    clear_reports(&mut gui);
    gui.menu(&["Series and Signals", "Convolution and correlation…"]);
    tick(&mut gui, "Accelerometer");
    assert!(gui.shows("⚠ Choose the second signal"));
    gui.choose(TITLE, 2, "Microphone");
    assert!(!gui.widget("Run").is_disabled());
    gui.click("Envelope and instantaneous phase (Hilbert transform)");
    gui.click("Run");
    let SignalMethod::Correlation(c) = request(&gui, 0).method else { panic!("not a correlation") };
    assert_eq!(c.operation, CorrelationOperation::Envelope);
    gui.harness.set_size(egui::vec2(1500.0, 4200.0));
    gui.results();
    for kind in [SectionKind::SignalEnvelope, SectionKind::InstantaneousFrequency, SectionKind::InstantaneousPhase] {
        assert!(plotted(&gui).contains(&kind), "{kind:?} not drawn");
    }
    let active = gui.app().active;
    gui.click("⋯ Options");
    gui.click("Save envelope");
    assert!(gui.app().documents[active].table.find("Envelope of Accelerometer").is_some());
}

/// Wavelets: the sample's report reopens in its dialog with its wavelet,
/// and switching to hard thresholding updates it; its scalograms are drawn.
#[test]
fn wavelet_report_reopens_in_its_dialog() {
    let mut gui = Gui::with_sample(SAMPLE);
    let active = gui.app().active;
    let reports = &gui.app().documents[active].reports;
    let i = (0..reports.len())
        .find(|&i| matches!(reports[i].saved(), app_model::SavedReport::SignalProcessing { request } if matches!(request.method, SignalMethod::Wavelet(_))))
        .expect("the sample has a wavelet report");
    let id = reports[i].id();
    gui.app_mut().edit_report(id);
    gui.run();
    assert!(gui.shows("Update"));
    assert!(painted(&gui, "sym4"), "the report's wavelet");
    gui.shot("dialog-wavelets");
    gui.click("Hard");
    gui.click("Update");
    let SignalMethod::Wavelet(w) = request(&gui, i).method else { panic!("not wavelets") };
    assert_eq!(w.kind, stat_signal::wavelet::ThresholdKind::Hard);
    assert!(w.scalogram, "other settings kept");
    assert!(gui.app().documents[active].reports[i].title().starts_with("Wavelet Analysis: Strain"));
}
