//! Data plots in three dimensions: Visualize ▸ 3-D scatter plot… and
//! Visualize ▸ Contour and surface plot….

use super::*;

/// The centre of the control labelled `label`.
fn centre(gui: &Gui, label: &str) -> egui::Pos2 {
    let b = gui.widget(label).raw_bounds().expect("the control has a position");
    egui::pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32)
}

/// The 3-D scatter dialog plots three columns with colour, size and shape
/// channels; dragging the plot turns it, and the drop-line toggle and the
/// angle are kept in the report.
#[test]
fn scatter_dialog_runs_and_the_plot_turns() {
    const TITLE: &str = "3-D scatter plot";
    let mut gui = Gui::with_sample("Hillside survey");
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui.menu(&["Visualize", "3-D scatter plot…"]);
    gui.choose(TITLE, 0, "Easting (m)");
    gui.choose(TITLE, 1, "Easting (m)");
    gui.choose(TITLE, 2, "Elevation (m)");
    assert!(gui.widget("Run").is_disabled(), "a column chosen twice");
    assert!(gui.shows("⚠ Choose three different columns"));
    gui.choose(TITLE, 1, "Northing (m)");
    gui.choose(TITLE, 3, "Soil type");
    gui.choose(TITLE, 4, "Soil depth (cm)");
    gui.click("Run");
    assert!(gui.harness.query_by_role_and_label(Role::Window, TITLE).is_none(), "the dialog closes");
    let reports = &gui.app().documents[active].reports;
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].title(), "3-D Scatter Plot: Elevation (m) by Easting (m) and Northing (m)");
    let app_model::SavedReport::Scatter3d { request, options } = reports[0].saved() else {
        panic!("not a 3-D scatter plot")
    };
    assert!(request.color.is_some() && request.size.is_some() && request.shape.is_none());
    assert!(!options.drop_lines);
    let before = options.camera;

    gui.harness.set_size(egui::vec2(1400.0, 1600.0));
    gui.results();
    let text = gui.painted_text();
    for level in ["Sandy loam", "Clay"] {
        assert!(text.iter().any(|t| t == level), "the colour key names {level}: {text:?}");
    }
    let at = centre(&gui, "Scatter (drag to turn)");
    gui.drag(at, at + egui::vec2(120.0, 40.0));
    gui.click("Drop lines");
    let app_model::SavedReport::Scatter3d { options, .. } = gui.app().documents[active].reports[0].saved() else {
        panic!("not a 3-D scatter plot")
    };
    assert!(options.drop_lines, "the toggle is kept");
    assert!(
        (options.camera.azimuth - before.azimuth).abs() > 5.0
            || (options.camera.elevation - before.elevation).abs() > 5.0,
        "dragging turns the plot: {:?} → {:?}",
        before,
        options.camera
    );
    gui.shot("scatter-3d-report");

    gui.click("View from above");
    let app_model::SavedReport::Scatter3d { options, .. } = gui.app().documents[active].reports[0].saved() else {
        panic!("not a 3-D scatter plot")
    };
    assert!((options.camera.elevation - 90.0).abs() < 1.0, "{:?}", options.camera);
}

/// The contour and surface dialog fits the chosen method; the report shows
/// the fit table, the contour plot and the turnable surface, whose look can
/// be switched to a wireframe.
#[test]
fn surface_dialog_runs_and_switches_to_a_wireframe() {
    const TITLE: &str = "Contour and surface plot";
    let mut gui = Gui::with_sample("Hillside survey");
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui.menu(&["Visualize", "Contour and surface plot…"]);
    gui.choose(TITLE, 0, "Easting (m)");
    gui.choose(TITLE, 1, "Northing (m)");
    gui.choose(TITLE, 2, "Soil depth (cm)");
    gui.choose(TITLE, 3, app_model::SurfaceFit::SmoothingSpline.label());
    gui.click("Run");
    let reports = &gui.app().documents[active].reports;
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].title(), "Contour and Surface: Soil depth (cm) over Easting (m) and Northing (m)");
    let app_model::SavedReport::DataSurface { request, options } = reports[0].saved() else {
        panic!("not a surface of data")
    };
    assert_eq!(request.method, app_model::SurfaceFit::SmoothingSpline);
    assert!(options.filled);

    gui.harness.set_size(egui::vec2(1400.0, 2600.0));
    gui.results();
    assert!(gui.shows("Surface Fit"));
    assert!(gui.shows("3-D Surface"));
    let text = gui.painted_text();
    assert!(text.iter().any(|t| t == "Effective degrees of freedom"), "the fit table: {text:?}");
    gui.click("Wireframe");
    let app_model::SavedReport::DataSurface { options, .. } = gui.app().documents[active].reports[0].saved() else {
        panic!("not a surface of data")
    };
    assert!(!options.filled, "the wireframe choice is kept");
    let before = options.camera;
    let at = centre(&gui, "Surface (drag to turn)");
    gui.drag(at, at + egui::vec2(-100.0, 30.0));
    let app_model::SavedReport::DataSurface { options, .. } = gui.app().documents[active].reports[0].saved() else {
        panic!("not a surface of data")
    };
    assert_ne!(options.camera, before, "dragging turns the surface");
    gui.shot("data-surface-report");
}

/// The sample opens with its three plots, and Edit analysis brings back the
/// dialog with the saved columns and method.
#[test]
fn hillside_sample_opens_its_plots_and_edits_them() {
    let mut gui = Gui::with_sample("Hillside survey");
    let active = gui.app().active;
    let titles: Vec<String> = gui.app().documents[active].reports.iter().map(|r| r.title()).collect();
    assert_eq!(titles.len(), 3, "{titles:?}");
    assert!(titles.iter().any(|t| t.starts_with("3-D Scatter Plot")));
    assert_eq!(titles.iter().filter(|t| t.starts_with("Contour and Surface")).count(), 2);
    gui.harness.set_size(egui::vec2(1400.0, 4000.0));
    gui.results();
    assert!(gui.shows("Scatter (drag to turn)"));
    assert!(gui.shows("Surface (drag to turn)"));
    gui.shot("hillside-survey-reports");

    let saved = gui.app().documents[active].reports[2].saved();
    let table = &gui.app().documents[active].table;
    let dialog = crate::plot3d_dialog::Plot3dDialog::from_saved(&saved, table).expect("the dialog reopens");
    let app_model::SavedReport::DataSurface { request, .. } = saved else { panic!("not a surface of data") };
    assert_eq!(request.method, app_model::SurfaceFit::SmoothingSpline);
    let _ = dialog;
}
