//! Opening, switching and closing documents.

use super::*;

/// Opening a project that is already open brings its tab forward instead of
/// opening it twice (it used to show the last tab instead).
#[test]
fn reopening_an_open_project_brings_it_forward() {
    let mut gui = Gui::with_sample("Longley");
    let dir = temp_dir("reopen-project");
    let path = dir.join("longley.acraft");
    let project = gui.app().documents[gui.app().active].project(&[]);
    file_io::write_project(&project, &path).expect("project written");
    gui.perform(Action::OpenPaths(vec![path.clone()]));
    let project_tab = gui.app().active;
    let id = gui.app().documents[project_tab].project;
    assert_eq!(gui.app().project(id).unwrap().path.as_deref(), Some(path.as_path()));
    // Another document opens after it, so the project is not the last tab.
    gui.perform(Action::OpenSample(1));
    let count = gui.app().documents.len();
    assert_ne!(gui.app().active, project_tab);
    gui.perform(Action::OpenPaths(vec![path]));
    assert_eq!(gui.app().documents.len(), count, "not opened twice");
    assert_eq!(gui.app().active, project_tab);
    let _ = std::fs::remove_dir_all(dir);
}

/// The data grid shortens long computed numbers but shows whole numbers such
/// as identifiers in full (a 12-digit code used to show rounded to 8 digits,
/// 400638130000, as if that were the value).
#[test]
fn grid_shows_whole_numbers_in_full() {
    let mut gui = Gui::start();
    let table = DataTable::new(
        "Codes",
        vec![
            stat_data::Column::numeric("Code", vec![400_638_133_393.0, 12.0]),
            stat_data::Column::numeric("Mean", vec![2.0 / 3.0, 1.5]),
        ],
    )
    .unwrap();
    gui.app_mut().open_table(table, None);
    gui.run();
    let text = gui.painted_text();
    let numbers: Vec<&String> = text.iter().filter(|t| t.starts_with("4006") || t.starts_with("0.6")).collect();
    assert!(text.iter().any(|t| t == "400638133393"), "{numbers:?}");
    assert!(!text.iter().any(|t| t == "400638130000"), "not rounded");
    assert!(text.iter().any(|t| t == "0.66666667"), "a long fraction is still shortened");
}

/// A table of one row read "1 rows" in the status bar.
#[test]
fn one_row_is_singular() {
    let mut gui = Gui::with_sample("Longley");
    gui.app_mut().documents.iter_mut().for_each(|d| d.reports.clear());
    let n = gui.app().documents[gui.app().active].table.n_rows();
    gui.grid(vec![GridRequest::RemoveRows((1..n).collect())]);
    assert!(gui.shows("1 row · 0 selected"), "status bar wording");
}

/// File ▸ Open sample data lists the samples in one submenu per topic, so
/// no list runs off the screen (the single list of 61 samples did), and
/// choosing one opens it.
#[test]
fn sample_menu_groups_samples_by_topic() {
    let mut gui = Gui::start();
    gui.harness.set_size(egui::vec2(1280.0, 720.0));
    gui.run();
    let topics = app_model::sample::SampleTopic::ALL;
    let catalog = app_model::sample::catalog();
    for topic in topics {
        let n = catalog.iter().filter(|s| s.topic == topic).count();
        assert!((1..=16).contains(&n), "{} has {n} samples", topic.label());
    }
    let is_menu = |n: &Node<'_>| n.role() == Role::Button && n.label().as_deref() == Some("File");
    gui.harness.get_all_by(is_menu).next().expect("File menu").simulate_click();
    gui.run();
    gui.widget("Open sample data").hover();
    gui.run();
    for topic in topics {
        assert!(gui.shows(topic.label()), "no {} submenu", topic.label());
    }
    gui.widget(app_model::sample::SampleTopic::Model.label()).hover();
    gui.run();
    let screen = gui.harness.ctx.screen_rect();
    let entries: Vec<_> = gui
        .harness
        .get_all_by(|n| n.role() == Role::Button && n.label().is_some_and(|l| l.contains("  —  ")))
        .collect();
    assert!(entries.len() >= 10, "the Models submenu lists its samples");
    for entry in &entries {
        let b = entry.raw_bounds().expect("entry position");
        assert!(b.y1 as f32 <= screen.bottom() + 0.5, "{:?} runs off the screen", entry.label());
    }
    drop(entries);
    gui.widget(app_model::sample::SampleTopic::Graphs.label()).hover();
    gui.run();
    gui.harness
        .get_by(|n| n.role() == Role::Button && n.label().is_some_and(|l| l.starts_with("Hillside survey")))
        .simulate_click();
    gui.run();
    let doc = &gui.app().documents[gui.app().active];
    assert!(doc.table.name().starts_with("Hillside survey"), "{}", doc.table.name());
}
