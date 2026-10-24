//! The read-only dashboard viewer (what the web build runs) and "Publish as
//! web app…", which writes the folder it is served from.

use super::*;
use app_model::dashboard::TileId;

/// The viewer showing the Greenhouse sample project, in a window tall
/// enough for all six tiles.
fn viewer() -> Gui {
    let mut builder = Harness::builder().with_size([1360.0, 1400.0]);
    if std::env::var_os(SHOTS).is_some() {
        builder = builder.wgpu();
    }
    let file = app_model::sample::greenhouse_project();
    let mut gui = Gui {
        harness: builder.build_eframe(move |cc| {
            apply_color_scheme(&cc.egui_ctx, ColorScheme::Light);
            StatsApp::new_viewer(cc, file.clone(), "Greenhouse overview · Greenhouse trial".to_owned())
        }),
    };
    gui.run();
    gui
}

/// The viewer shows the dashboard full window: its tiles, filters and
/// switchers, with no menus, sidebar or document tabs, and no controls
/// that add, move, resize, remove or edit tiles.
#[test]
fn viewer_shows_the_dashboard_without_editing_controls() {
    let mut gui = viewer();
    let id = gui.app().shown_dashboard().expect("a dashboard is shown");
    let painted = gui.painted_text().join("\n");
    assert!(painted.contains("Greenhouse overview · Greenhouse trial"), "the title bar");
    assert!(painted.contains("changes are not saved"));
    // Every report tile was computed.
    for tile in 3..=6 {
        assert!(gui.app().dashboard(id).unwrap().tile_report(TileId(tile)).is_some(), "tile {tile}");
    }
    assert!(gui.shows("Filter Greenhouse"), "filters stay");
    assert!(gui.shows("Undo"));
    for absent in ["File", "Describe", "+ Add tile", "Export as HTML…", "Publish as web app…", "Dashboard menu"] {
        assert!(!gui.shows(absent), "{absent} is not offered");
    }
    let labels: Vec<String> = gui.harness.query_all_by(|_| true).filter_map(|n| n.label()).collect();
    for prefix in ["Move tile", "Resize tile", "Remove tile"] {
        assert!(!labels.iter().any(|l| l.starts_with(prefix)), "no {prefix} control");
    }
    assert!(!gui.shows("×"), "reports cannot be removed");
    gui.shot("viewer");
}

/// Filters work in the viewer as in the app (with Undo), and the tables'
/// own rows stay as published.
#[test]
fn viewer_filters_recompute_the_tiles() {
    let mut gui = viewer();
    let id = gui.app().shown_dashboard().expect("a dashboard is shown");
    let before = gui.app().dashboard(id).unwrap().dashboard.filters.len();
    assert!(gui.app().dashboard(id).unwrap().dashboard.filter("Greenhouse").is_some());
    gui.click("Filter Greenhouse");
    gui.click("Remove filter");
    let open = gui.app().dashboard(id).unwrap();
    assert_eq!(open.dashboard.filters.len(), before - 1);
    assert!(open.dashboard.filter("Greenhouse").is_none());
    assert_eq!(open.history.next_undo(), Some("Remove filter on Greenhouse"));
    gui.click("Undo");
    assert!(gui.app().dashboard(id).unwrap().dashboard.filter("Greenhouse").is_some());
    for doc in &gui.app().documents {
        let c = doc.rows.counts();
        assert_eq!((c.excluded, c.hidden), (0, 0), "the tables' own rows are untouched");
    }
}

/// A tile's report asking for a new column, or a layout edit reaching the
/// dashboard, changes nothing in the viewer.
#[test]
fn viewer_ignores_layout_and_project_changes() {
    let mut gui = viewer();
    let id = gui.app().shown_dashboard().expect("a dashboard is shown");
    let tiles = gui.app().dashboard(id).unwrap().dashboard.tiles.len();
    let columns: Vec<usize> = gui.app().documents.iter().map(|d| d.table.n_cols()).collect();
    let mut keep = crate::app::dashboards::DashboardOutcome::default();
    keep.edits.push(app_model::dashboard::DashboardEdit::RemoveTile { tile: TileId(3) });
    keep.edits.push(app_model::dashboard::DashboardEdit::MoveTile { tile: TileId(3), column: 0, row: 40 });
    keep.edits.push(app_model::dashboard::DashboardEdit::RemoveFilter { column: "Greenhouse".to_owned() });
    keep.new_tables.push((0, stat_data::DataTable::new("x", Vec::new()).unwrap(), false));
    keep.export = true;
    keep.keep_view_changes();
    assert_eq!(keep.edits.len(), 1, "only the filter change is kept");
    assert!(keep.new_tables.is_empty() && !keep.export);
    assert!(keep.message.as_deref().is_some_and(|m| m.contains("cannot add columns, reports or tables")));
    gui.run();
    assert_eq!(gui.app().dashboard(id).unwrap().dashboard.tiles.len(), tiles);
    let after: Vec<usize> = gui.app().documents.iter().map(|d| d.table.n_cols()).collect();
    assert_eq!(after, columns);
}

/// Two stand-in viewer files, as `scripts/build-web-viewer.sh` would write.
fn stand_in_bundle(test: &str) -> std::path::PathBuf {
    let bundle = temp_dir(test);
    for name in app_model::web_publish::VIEWER_FILES {
        std::fs::write(bundle.join(name), name).unwrap();
    }
    bundle
}

/// Without a built web viewer, "Publish as web app…" writes nothing (it
/// does not even ask for a folder) and opens a window that says what is
/// missing, with the command that builds it.
#[test]
fn publish_without_the_viewer_explains_how_to_build_it() {
    let mut gui = Gui::start();
    gui.harness.set_size(egui::vec2(1500.0, 1400.0));
    gui.perform(Action::OpenProjectSample(0));
    crate::app::dashboards::test_bundle::set(None);
    let dir = temp_dir("publish-no-viewer");
    file_io::test_save_answer::set(Some(dir.clone()));
    gui.click("Publish as web app…");
    // (The unused answer stays with this test's thread.)
    assert!(file_io::test_save_answer::pending(), "no folder was asked for");
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0, "nothing written");
    let painted = gui.painted_text().join("\n");
    assert!(painted.contains("nothing has been published yet"), "{painted}");
    assert!(painted.contains("build-web-viewer.sh"), "the command");
    // This checkout has the script, so the build can run from here.
    assert!(gui.shows("Build it now"));
    gui.shot("publish-build-viewer");
    gui.click("Close");
    assert!(!gui.shows("Build it now"));
    let _ = std::fs::remove_dir_all(dir);
}

/// With the viewer built, "Publish as web app…" writes the page, the
/// project (dashboard first, no layout or scripts) and the viewer files
/// into the chosen folder. It will not overwrite a page it did not write.
#[test]
fn publish_writes_the_web_app_folder() {
    let mut gui = Gui::start();
    apply_color_scheme(&gui.harness.ctx.clone(), ColorScheme::Light);
    gui.harness.set_size(egui::vec2(1500.0, 1400.0));
    gui.perform(Action::OpenProjectSample(0));
    let bundle = stand_in_bundle("publish-web-app-bundle");
    crate::app::dashboards::test_bundle::set(Some(bundle.clone()));
    let dir = temp_dir("publish-web-app");
    file_io::test_save_answer::set(Some(dir.clone()));
    gui.click("Publish as web app…");
    assert!(!file_io::test_save_answer::pending(), "the folder was asked for");
    let html = std::fs::read_to_string(dir.join("index.html")).expect("the page was written");
    assert!(html.contains("<title>Greenhouse overview · Greenhouse trial (simulated)</title>"), "{html}");
    assert!(html.contains("anovacraft_viewer.js"));
    let notices = app_model::web_publish::NOTICES_FILE;
    assert!(html.contains(&format!(r#"<link rel="license" href="{notices}">"#)), "the page links the licences");
    let written = std::fs::read_to_string(dir.join(notices)).expect("the licences were written");
    assert!(written.starts_with("AnovaCraft: third-party notices"));
    for name in app_model::web_publish::VIEWER_FILES {
        assert_eq!(std::fs::read_to_string(dir.join(name)).unwrap(), name, "the viewer was copied");
    }
    let text = std::fs::read_to_string(dir.join(app_model::web_publish::PROJECT_FILE)).expect("the project");
    let file = app_model::project::load_project_file(&text).expect("a project file");
    assert_eq!(file.tables.len(), 2);
    assert_eq!(file.dashboards[0].name, "Greenhouse overview");
    let painted = gui.painted_text().join("\n");
    assert!(painted.contains("Published “Greenhouse overview”"), "{painted}");

    // A folder holding someone else's index.html is refused.
    let other = temp_dir("publish-web-app-other");
    std::fs::write(other.join("index.html"), "<html>mine</html>").unwrap();
    file_io::test_save_answer::set(Some(other.clone()));
    gui.click("Publish as web app…");
    assert_eq!(std::fs::read_to_string(other.join("index.html")).unwrap(), "<html>mine</html>");
    assert!(!other.join(app_model::web_publish::PROJECT_FILE).exists());
    assert!(gui.painted_text().join("\n").contains("AnovaCraft did not write"));
    crate::app::dashboards::test_bundle::set(None);
    for d in [dir, other, bundle] {
        let _ = std::fs::remove_dir_all(d);
    }
}

/// With a built viewer (here two stand-in files), publishing copies it in,
/// and the published folder opens in the viewer as the dashboard.
#[test]
fn published_folder_opens_in_the_viewer() {
    let bundle = stand_in_bundle("publish-bundle");
    let dir = temp_dir("publish-with-bundle");
    let file = app_model::web_publish::published_project(
        app_model::sample::greenhouse_project(),
        &app_model::sample::greenhouse_project().dashboards[0].id,
    );
    crate::app::dashboards::write_published_folder(&dir, "Greenhouse", &file, &bundle).unwrap();
    for name in app_model::web_publish::VIEWER_FILES {
        assert_eq!(std::fs::read_to_string(dir.join(name)).unwrap(), name);
    }
    // Publishing again into the same folder replaces what AnovaCraft wrote there.
    assert!(crate::app::dashboards::write_published_folder(&dir, "Greenhouse", &file, &bundle).is_ok());
    let text = std::fs::read_to_string(dir.join(app_model::web_publish::PROJECT_FILE)).unwrap();
    let file = app_model::project::load_project_file(&text).unwrap();
    let mut gui = viewer();
    gui.app_mut().projects.clear();
    gui.app_mut().documents.clear();
    gui.app_mut().load_viewer(file, "Greenhouse".to_owned());
    gui.run();
    assert!(gui.app().shown_dashboard().is_some());
    assert!(gui.shows("Filter Greenhouse"));
    let _ = std::fs::remove_dir_all(bundle);
    let _ = std::fs::remove_dir_all(dir);
}
