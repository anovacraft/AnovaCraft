//! Crash safety: plots with more points than the point budget drawn as a
//! density picture (still selectable), the offer to restore work saved for
//! recovery after a run that ended abnormally, and Help ▸ Show crash reports.

use super::*;
use app_model::recovery::{RecoveryInfo, RecoverySession, orphaned_sessions};

/// Rows in the dense table: above the 20,000-point budget.
const DENSE: usize = 30_000;

/// The app with one table of [`DENSE`] rows (x from 0 to 299 across, y
/// from 0 to 99.9 up) and its reports cleared, showing its results.
fn dense() -> Gui {
    let mut gui = Gui::start();
    let x: Vec<f64> = (0..DENSE).map(|i| (i % 300) as f64).collect();
    let y: Vec<f64> = (0..DENSE).map(|i| (i / 300) as f64 + ((i * 37) % 10) as f64 / 10.0).collect();
    let table =
        DataTable::new("Dense", vec![stat_data::Column::numeric("x", x), stat_data::Column::numeric("y", y)]).unwrap();
    let doc = Document::new(table, RowStateManager::new(DENSE), Vec::new(), Workspace::Data);
    gui.app_mut().push_document(doc);
    gui.harness.set_size(egui::vec2(1400.0, 1100.0));
    gui.run();
    gui
}

/// Circles painted in the last frame (each plot marker is one).
fn painted_circles(gui: &Gui) -> usize {
    fn count(shape: &egui::Shape) -> usize {
        match shape {
            egui::Shape::Circle(_) => 1,
            egui::Shape::Vec(v) => v.iter().map(count).sum(),
            _ => 0,
        }
    }
    gui.harness.output().shapes.iter().map(|c| count(&c.shape)).sum()
}

/// The note under a plot drawn as a picture.
fn dense_note(n: usize) -> String {
    let grouped = |n: usize| {
        let digits = n.to_string();
        let mut out = String::new();
        for (i, ch) in digits.chars().enumerate() {
            if i > 0 && (digits.len() - i) % 3 == 0 {
                out.push(',');
            }
            out.push(ch);
        }
        out
    };
    format!(
        "{} points, drawn as a density picture (more than {}): darker where points overlap",
        grouped(n),
        grouped(stat_viz::point_raster::POINT_BUDGET)
    )
}

/// A scatter plot of 30,000 rows is drawn as one picture instead of a
/// circle per row (a million rows overflowed the GPU's buffers), says so
/// under the plot, and a box dragged over its left part still selects
/// exactly those rows.
#[test]
fn dense_scatter_plot_is_a_picture_and_box_selection_still_works() {
    let mut gui = dense();
    gui.app_mut().add_report(ReportRequest::Bivariate(app_model::BivariateRequest::new(0, 1)));
    gui.results();
    assert!(gui.shows(&dense_note(DENSE)), "the plot says it is a picture");
    assert!(painted_circles(&gui) < 200, "no marker per row: {} circles", painted_circles(&gui));
    gui.shot("dense-scatter-picture");

    let b = gui.harness.get_all_by_label("Horizontal axis ruler").next().unwrap().raw_bounds().unwrap();
    let v = gui.harness.get_all_by_label("Vertical axis ruler").next().unwrap().raw_bounds().unwrap();
    let (left, right, top, bottom) = (b.x0 as f32, b.x1 as f32, v.y0 as f32, v.y1 as f32);
    gui.drag(egui::pos2(left + 2.0, top + 2.0), egui::pos2(left + 0.3 * (right - left), bottom - 2.0));
    let active = gui.app().active;
    let selected = gui.app().documents[active].rows.selected_rows();
    let x = gui.app().documents[active].table.column(0).unwrap().as_numeric().unwrap().to_vec();
    assert!(selected.len() > 1000, "the box covers many rows: {}", selected.len());
    let edge = selected.iter().map(|&r| x[r]).fold(f64::NEG_INFINITY, f64::max);
    assert!(edge < 150.0, "only rows of the left part (up to x = {edge})");
    // Every row left of the box's right edge is in it (the box spans the height).
    assert!((0..DENSE).filter(|&r| x[r] < edge).all(|r| selected.contains(&r)));
    // The selection is drawn into the picture too: still no markers.
    assert!(painted_circles(&gui) < 200);
    gui.shot("dense-scatter-selection");
}

/// A few hundred points keep their markers.
#[test]
fn ordinary_scatter_plots_keep_one_marker_per_row() {
    let mut gui = Gui::with_sample("Longley");
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui.app_mut().add_report(ReportRequest::Bivariate(app_model::BivariateRequest::new(2, 0)));
    gui.results();
    assert!(!gui.painted_text().iter().any(|t| t.contains("density picture")));
    assert!(gui.harness.query_all_by(|n| n.label().is_some_and(|l| l.contains("density picture"))).next().is_none());
    assert!(painted_circles(&gui) >= 16, "Longley's 16 years each drawn as a marker");
}

/// The graph canvas's point layer and the box plot's ringed selection go
/// over to a picture the same way.
#[test]
fn dense_graph_canvas_and_box_plot_selection_are_pictures() {
    let mut gui = dense();
    let spec = stat_viz::GraphSpec {
        encoding: stat_viz::EncodingPlan { x: vec!["x".into()], y: vec!["y".into()], ..Default::default() },
        points: Some(stat_viz::PointsElement::default()),
        ..stat_viz::GraphSpec::default()
    };
    gui.app_mut().add_report(ReportRequest::Graph(Box::new(spec)));
    gui.results();
    assert!(painted_circles(&gui) < 200, "graph canvas: {} circles", painted_circles(&gui));
    gui.shot("dense-graph-picture");

    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui.app_mut().add_report(ReportRequest::Distribution(app_model::DistributionRequest::new(1)));
    gui.run();
    gui.app_mut().documents[active].rows.select(0..DENSE, stat_data::SelectionMode::Replace);
    gui.run();
    assert!(gui.shows(&dense_note(DENSE)), "the box plot's selection rings became a picture");
    assert!(painted_circles(&gui) < 200, "distribution: {} circles", painted_circles(&gui));
}

/// A recovery folder under `root` left by a run that stopped: one copy of a
/// project named "Field trial" that came from `original`, and a crash report.
fn crashed_run(root: &std::path::Path, original: &std::path::Path) -> std::path::PathBuf {
    let mut session = RecoverySession::start(root, "run-crashed").unwrap();
    let table = DataTable::new("Plots", vec![stat_data::Column::numeric("yield", vec![4.1, 4.4, 5.0])]).unwrap();
    let mut file = app_model::project::ProjectFile::default();
    file.add_table(table, RowStateManager::new(3));
    let info = RecoveryInfo { name: "Field trial".to_owned(), original: Some(original.to_path_buf()) };
    assert!(session.save(1, &info, &file).unwrap());
    app_model::recovery::note_crash(session.dir(), &root.join("crash-1.txt")).unwrap();
    let dir = session.dir().to_path_buf();
    std::fs::write(dir.join("heartbeat"), "0").unwrap();
    dir
}

/// Offer what is under `root` as the app would at launch.
fn offer(gui: &mut Gui, root: &std::path::Path) {
    let sessions = orphaned_sessions(root, "this-run", app_model::recovery::now_secs());
    assert_eq!(sessions.len(), 1);
    gui.app_mut().recovery_offer = Some(recovery::RecoveryOffer { sessions });
    gui.run();
}

/// After a crash the next launch lists the projects kept for recovery;
/// Restore opens them with unsaved changes (saving writes to the file they
/// came from) and deletes the copies.
#[test]
fn restore_offer_reopens_recovered_projects_as_unsaved() {
    let root = temp_dir("recovery-restore");
    let original = root.join("field trial.acraft");
    let session = crashed_run(&root, &original);
    let mut gui = Gui::start();
    offer(&mut gui, &root);
    assert!(gui.harness.query_by_role_and_label(Role::Window, "Restore unsaved work").is_some());
    assert!(gui.shows("AnovaCraft stopped unexpectedly the last time it ran."));
    assert!(gui.shows("•  Field trial"));
    assert!(gui.shows("Show crash report"));
    gui.shot("recovery-offer");
    gui.click("Restore");
    assert!(gui.harness.query_by_role_and_label(Role::Window, "Restore unsaved work").is_none());
    assert_eq!(gui.app().documents.len(), 1);
    let doc = &gui.app().documents[0];
    assert_eq!(doc.table.column(0).unwrap().as_numeric().unwrap(), &[4.1, 4.4, 5.0]);
    let project = gui.app().project(doc.project).unwrap();
    assert_eq!(project.path.as_deref(), Some(original.as_path()), "saving writes where it came from");
    assert!(gui.app().project_modified(doc.project), "the restored project has unsaved changes");
    assert!(!session.exists(), "the copies are deleted once restored");
}

/// Discard deletes the copies; Decide later keeps them for the next launch.
#[test]
fn restore_offer_can_discard_or_wait() {
    let root = temp_dir("recovery-discard");
    let session = crashed_run(&root, &root.join("a.acraft"));
    let mut gui = Gui::start();
    offer(&mut gui, &root);
    gui.click("Decide later");
    assert!(gui.app().recovery_offer.is_none() && gui.app().documents.is_empty());
    assert!(session.exists(), "kept for the next launch");
    offer(&mut gui, &root);
    gui.click("Discard");
    assert!(gui.app().documents.is_empty());
    assert!(!session.exists(), "deleted");
}

/// Help ▸ Show crash reports opens the reports folder in the system's file
/// viewer; the restore offer's button opens the report itself.
#[test]
fn crash_reports_can_be_opened_from_help_and_the_offer() {
    let mut gui = Gui::start();
    super::super::manuals::LAUNCHED.with(|l| l.borrow_mut().clear());
    gui.menu(&["Help", "Show crash reports"]);
    let launched = super::super::manuals::LAUNCHED.with(|l| l.borrow().clone());
    assert_eq!(launched, vec![app_model::recovery::log_dir().unwrap()]);

    let root = temp_dir("recovery-report");
    crashed_run(&root, &root.join("a.acraft"));
    offer(&mut gui, &root);
    gui.click("Show crash report");
    let launched = super::super::manuals::LAUNCHED.with(|l| l.borrow().clone());
    assert_eq!(launched.last(), Some(&root.join("crash-1.txt")));
    assert!(gui.app().recovery_offer.is_some(), "the offer stays open");
}
