//! Series and Signals ▸ Images: the three dialogs find the pixel columns, show the
//! image size, refuse incomplete choices and fill their requests; the
//! reports draw their pictures and save columns; a report reopens in its
//! dialog.

use super::*;
use app_model::ImageMethod;
use app_model::image_analysis::{BandChoice, RestoreOperation};

const SCAN: &str = "Machined surface scan";
const SCENE: &str = "Blurred and shifted scene";

fn clear_reports(gui: &mut Gui) {
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
}

fn tick(gui: &mut Gui, label: &str) {
    gui.harness.get_by(|n| n.role() == Role::CheckBox && n.label().as_deref() == Some(label)).simulate_click();
    gui.run();
}

fn request(gui: &Gui, i: usize) -> app_model::ImageAnalysisRequest {
    let active = gui.app().active;
    match gui.app().documents[active].reports[i].saved() {
        app_model::SavedReport::ImageAnalysis { request } => request,
        other => panic!("not an image report: {other:?}"),
    }
}

fn painted(gui: &Gui, start: &str) -> bool {
    gui.painted_text().iter().any(|t| t.starts_with(start))
}

/// Image spectrum: the dialog picks the x and y columns by name, shows the
/// image size and Nyquist frequency once a value column is chosen, and the
/// report lists the dominant frequencies.
#[test]
fn spectrum_dialog_finds_the_pixels_and_lists_dominant_frequencies() {
    let mut gui = Gui::with_sample(SCAN);
    clear_reports(&mut gui);
    gui.menu(&["Series and Signals", "Image spectrum…"]);
    assert!(gui.shows("⚠ Choose at least one value column"));
    assert!(gui.widget("Run").is_disabled());
    tick(&mut gui, "Height (µm)");
    assert!(painted(&gui, "256 × 256 pixels; spatial frequencies up to 0.25"), "{:?}", gui.painted_text());
    tick(&mut gui, "Phase spectrum");
    gui.shot("dialog-image-spectrum");
    gui.click("Run");
    let r = request(&gui, 0);
    let ImageMethod::Spectrum(s) = &r.method else { panic!("not a spectrum") };
    assert!(s.phase);
    let active = gui.app().active;
    let t = &gui.app().documents[active].table;
    assert_eq!((t.column(r.input.x).unwrap().name.as_str(), t.column(r.input.y).unwrap().name.as_str()), ("x (µm)", "y (µm)"));
    gui.harness.set_size(egui::vec2(1500.0, 5200.0));
    gui.results();
    assert!(painted(&gui, "Log-Magnitude Spectrum") || painted(&gui, "Log-magnitude spectrum"), "{:?}", gui.painted_text());
    assert!(painted(&gui, "Dominant Frequencies") || painted(&gui, "Dominant frequencies"));
    assert!(gui.app().documents[active].reports[0].title().starts_with("Image Spectrum: Height"));
    gui.shot("image-spectrum");
}

/// Image frequency filter: with no band and no notch the dialog refuses;
/// the sample's notch filter saves the filtered image as a column.
#[test]
fn filter_needs_a_band_or_notch_and_saves_the_filtered_image() {
    let mut gui = Gui::with_sample(SCAN);
    let active = gui.app().active;
    let reports = &gui.app().documents[active].reports;
    let i = (0..reports.len())
        .find(|&i| matches!(reports[i].saved(), app_model::SavedReport::ImageAnalysis { request } if matches!(request.method, ImageMethod::Filter(_))))
        .expect("the sample has a filter report");
    let ImageMethod::Filter(f) = request(&gui, i).method else { panic!() };
    assert_eq!((f.band, f.auto_notches), (BandChoice::None, 2));
    // Keep only the filter report, so its options menu is the only one.
    let keep = gui.app().documents[active].reports[i].id();
    gui.app_mut().documents[active].reports.retain(|r| r.id() == keep);
    let before = gui.app().documents[active].table.n_cols();
    gui.harness.set_size(egui::vec2(1500.0, 5200.0));
    gui.results();
    gui.click("⋯ Options");
    gui.click("Save filtered image");
    let table = &gui.app().documents[active].table;
    assert_eq!(table.n_cols(), before + 1);
    assert!(table.find("Filtered Height (µm)").is_some());

    clear_reports(&mut gui);
    gui.menu(&["Series and Signals", "Image frequency filter…"]);
    tick(&mut gui, "Height (µm)");
    gui.choose("Image frequency filter", 3, "None (notches only)");
    assert!(gui.shows("⚠ Choose a band filter or at least one notch"));
    gui.click("Add a notch");
    assert!(!gui.widget("Run").is_disabled());
    gui.shot("dialog-image-filter");
    gui.click("Run");
    let ImageMethod::Filter(f) = request(&gui, 0).method else { panic!("not a filter") };
    assert_eq!(f.notches.len(), 1);
}

/// Registration needs the image to align; the sample's registration report
/// reopens in its dialog with its column.
#[test]
fn registration_needs_a_second_image_and_reopens_in_its_dialog() {
    let mut gui = Gui::with_sample(SCENE);
    let active = gui.app().active;
    let saved = std::mem::take(&mut gui.app_mut().documents[active].reports);
    gui.menu(&["Series and Signals", "Image registration and deblurring…"]);
    tick(&mut gui, "Sharp");
    gui.click("Align a second image with this one");
    assert!(gui.shows("⚠ Choose the image to align"));
    gui.shot("dialog-image-registration");
    gui.click("Cancel");
    gui.app_mut().documents[active].reports = saved;
    gui.run();
    let reports = &gui.app().documents[active].reports;
    let i = (0..reports.len())
        .find(|&i| matches!(reports[i].saved(), app_model::SavedReport::ImageAnalysis { request } if matches!(request.method, ImageMethod::Restore(ref r) if matches!(r.operation, RestoreOperation::Register { .. }))))
        .expect("the sample has a registration report");
    assert!(gui.app().documents[active].reports[i].title().starts_with("Image Registration: Shifted to Sharp"));
    let id = gui.app().documents[active].reports[i].id();
    gui.app_mut().edit_report(id);
    gui.run();
    assert!(gui.shows("Update"));
    assert!(painted(&gui, "Shifted"), "the image to align");
    gui.click("Update");
    assert!(matches!(request(&gui, i).method, ImageMethod::Restore(ref r) if matches!(r.operation, RestoreOperation::Register { upsample: 20, .. })));
}

/// Image spectrum: radial profiles take the typed directions, refuse text
/// that is not degrees, and draw the profiles with their values table.
#[test]
fn spectrum_profiles_take_typed_directions() {
    let mut gui = Gui::with_sample(SCAN);
    clear_reports(&mut gui);
    gui.menu(&["Series and Signals", "Image spectrum…"]);
    tick(&mut gui, "Height (µm)");
    gui.widget("Profile directions").type_text(", up");
    gui.run();
    assert!(painted(&gui, "⚠ \"up\" is not a direction"), "{:?}", gui.painted_text());
    assert!(gui.widget("Run").is_disabled());
    for _ in 0..4 {
        gui.widget("Profile directions").key_press(egui_kittest::kittest::Key::Backspace);
        gui.run();
    }
    gui.widget("Profile directions").type_text(", 20°");
    gui.run();
    tick(&mut gui, "Also along each dominant frequency's direction");
    gui.click("Run");
    let ImageMethod::Spectrum(s) = request(&gui, 0).method else { panic!("not a spectrum") };
    assert_eq!(s.profile_directions, [0.0, 90.0, 20.0]);
    assert!(s.profile_peaks);
    gui.harness.set_size(egui::vec2(1500.0, 6400.0));
    gui.results();
    assert!(painted(&gui, "Radial Profiles"), "{:?}", gui.painted_text());
    assert!(painted(&gui, "Radial Profile Values"));
}
