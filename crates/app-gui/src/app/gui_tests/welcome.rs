//! The start page, project notes and the results pane's Export reports button.

use super::*;
use app_model::settings::{RecentContents, RecentDetails};

/// The start page with recent files whose details are known, in light or dark.
fn start_with_recent(scheme: ColorScheme) -> (Gui, std::path::PathBuf) {
    let mut gui = Gui::start();
    apply_color_scheme(&gui.harness.ctx.clone(), scheme);
    let project = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/data/plant_growth.acraft");
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
    let settings = &mut gui.app_mut().settings;
    settings.remember_file_with(
        std::path::Path::new("/data/line4-sensors.csv"),
        RecentDetails { when: now - 3 * 86_400, contents: RecentContents::Data { rows: 2400, columns: 9 } },
    );
    settings.remember_file_with(
        &project,
        RecentDetails {
            when: now - 60,
            contents: RecentContents::Project {
                tables: vec!["Plant growth".into()],
                notes: "Second batch after the pump was replaced".into(),
            },
        },
    );
    gui.run();
    (gui, project)
}

/// The app starts on the start page, not an empty table, and the page shows
/// recent projects with what they hold and their notes; clicking one opens it.
#[test]
fn startup_shows_the_start_page_with_recent_projects() {
    let (mut gui, project) = start_with_recent(ColorScheme::Light);
    assert!(gui.app().documents.is_empty(), "no table at startup");
    gui.shot("start-page-light");
    for label in ["New data table", "Open a file or project…", "Paste from clipboard", "Describe a column"] {
        assert!(gui.shows(label), "start page lacks {label}");
    }
    let text = gui.painted_text();
    assert!(text.iter().any(|t| t == "1 table: Plant growth"), "{text:?}");
    assert!(text.iter().any(|t| t == "“Second batch after the pump was replaced”"), "{text:?}");
    assert!(text.iter().any(|t| t == "Data file: 2,400 rows, 9 columns"), "{text:?}");
    assert!(text.iter().any(|t| t.starts_with("Today, ")), "{text:?}");
    gui.click("plant_growth");
    assert_eq!(gui.app().documents.len(), 1, "the recent project opened");
    assert_eq!(gui.app().project(gui.app().documents[0].project).unwrap().path.as_deref(), Some(project.as_path()));
}

/// The start page in the dark appearance (for the screenshot).
#[test]
fn start_page_dark() {
    let (mut gui, _) = start_with_recent(ColorScheme::Dark);
    gui.shot("start-page-dark");
    assert!(gui.shows("Sample data"));
}

/// A data task chosen with no table open brings up "Open data for …" in
/// front of the start page; a table opened from there opens the task's
/// dialog. Cancel drops the task, and so does opening data another way, so
/// no dialog waits out of sight.
#[test]
fn a_task_without_data_asks_for_data_in_front() {
    let mut gui = Gui::start();
    gui.click("Describe a column");
    assert!(gui.app().distribution_dialog.is_none());
    assert_eq!(gui.app().pending_task, Some(crate::app::welcome::Task::Describe));
    assert!(gui.shows("Open data for “Describe a column”"), "{:?}", gui.painted_text());
    gui.shot("start-page-task-asks-for-data");
    gui.click("Cancel");
    assert_eq!(gui.app().pending_task, None);
    assert!(!gui.shows("Open data for “Describe a column”"));

    // A table started from the small dialog opens the task's dialog on it.
    gui.click("Compare groups");
    gui.click("Start an empty table");
    assert_eq!(gui.app().documents.len(), 1);
    assert!(gui.app().compare_dialog.is_some(), "the dialog opened once the table did");
    assert_eq!(gui.app().pending_task, None);
    assert!(gui.app().message.as_ref().is_some_and(|(m, _)| m.contains("from the start page")));

    // Opening data some other way drops the waiting task.
    gui.perform(Action::Close(0));
    gui.click("Fit a model");
    assert!(gui.shows("Open data for “Fit a model”"));
    gui.perform(Action::OpenSample(0));
    assert_eq!(gui.app().pending_task, None);
    assert!(gui.app().dialog.is_none(), "no model dialog opens out of the blue");
}

/// "Design an experiment" needs no data: it lists the designs, and one opens
/// its dialog straight away.
#[test]
fn design_task_lists_designs() {
    let mut gui = Gui::start();
    gui.click("Design an experiment");
    gui.shot("start-page-design-list");
    gui.click("Plackett–Burman…");
    assert!(gui.app().design_dialog.is_some());
    assert_eq!(gui.app().pending_task, None);
}

/// Sample data lists the samples by topic; one opens with a click.
#[test]
fn sample_data_lists_samples_by_topic() {
    let mut gui = Gui::start();
    gui.click("Sample data");
    let topic = app_model::sample::SampleTopic::Graphs;
    let n = app_model::sample::catalog().iter().filter(|s| s.topic == topic).count();
    gui.click(&format!("{} ({n})", topic.label()));
    gui.shot("start-page-samples");
    gui.harness
        .get_by(|n| n.role() == Role::Button && n.label().is_some_and(|l| l.starts_with("Hillside survey")))
        .simulate_click();
    gui.run();
    assert_eq!(gui.app().documents.len(), 1);
    assert!(gui.app().documents[0].table.name().starts_with("Hillside survey"));
}

/// Next shows another tip.
#[test]
fn next_tip() {
    let mut gui = Gui::start();
    let tip = |gui: &Gui| gui.painted_text().into_iter().find(|t| crate::app::welcome::TIPS.contains(&t.as_str()));
    let first = tip(&gui).expect("a tip is shown");
    gui.click("Next ›");
    let second = tip(&gui).expect("a tip is shown");
    assert_ne!(first, second);
}

/// Project notes: typed in the sidebar, they make the project unsaved, are
/// saved in its file and read back; the sidebar shows the notes of the
/// project being worked in.
#[test]
fn project_notes_follow_the_project_and_are_saved() {
    let mut gui = Gui::with_sample("Longley");
    let longley = gui.app().documents[gui.app().active].project;
    gui.perform(Action::OpenSample(1));
    let other = gui.app().documents[gui.app().active].project;
    assert_ne!(longley, other);
    let notes = |gui: &Gui| gui.harness.get_by_role(Role::MultilineTextInput).value().unwrap_or_default();
    gui.harness.get_by_role(Role::MultilineTextInput).type_text("Check run 14");
    gui.run();
    gui.shot("project-notes");
    assert_eq!(gui.app().project(other).unwrap().notes, "Check run 14");
    assert!(gui.app().project_modified(other), "notes are an unsaved change");
    assert_eq!(notes(&gui), "Check run 14");
    // Working in the other project shows its (empty) notes.
    let first = gui.app().documents.iter().position(|d| d.project == longley).unwrap();
    gui.perform(Action::Activate(first));
    assert_eq!(notes(&gui), "");
    // Saved in the project file and read back.
    let dir = temp_dir("project_notes");
    let path = dir.join("notes.acraft");
    let second = gui.app().documents.iter().position(|d| d.project == other).unwrap();
    let file = gui.app().project_file(other, true);
    file_io::write_project_file(&file, &path).unwrap();
    let _ = second;
    let mut reopened = Gui::start();
    reopened.perform(Action::OpenPaths(vec![path.clone()]));
    let p = reopened.app().documents[0].project;
    assert_eq!(reopened.app().project(p).unwrap().notes, "Check run 14");
    assert!(!reopened.app().project_modified(p));
    let _ = std::fs::remove_dir_all(dir);
}

/// The notes move into a window of their own and back.
#[test]
fn project_notes_in_their_own_window() {
    let mut gui = Gui::with_sample("Longley");
    gui.click("Notes in their own window");
    assert!(gui.app().notes_window);
    assert!(gui.shows("Back to sidebar"));
    gui.shot("project-notes-window");
    gui.harness.get_by_role(Role::MultilineTextInput).type_text("From the window");
    gui.run();
    let project = gui.app().documents[gui.app().active].project;
    assert_eq!(gui.app().project(project).unwrap().notes, "From the window");
    gui.harness.get_all_by_label("Back to sidebar").next().unwrap().simulate_click();
    gui.run();
    assert!(!gui.app().notes_window);
}

/// Export all… at the top of the results works with every report
/// collapsed, and exports all of the table's reports.
#[test]
fn export_reports_from_the_results_toolbar() {
    let mut gui = Gui::with_sample("Longley");
    let active = gui.app().active;
    gui.app_mut().documents[active].workspace = Workspace::Results;
    gui.run();
    gui.click("Collapse all");
    gui.click("Export all…");
    let dialog = gui.app().export_dialog.as_ref().expect("the export dialog opened");
    assert_eq!(dialog.report, None, "every report of the table");
}

/// The task cards keep one height per row at any window width, the tip
/// keeps a readable width, and the "no data needed" badge stays on one line (it used to wrap a letter or two
/// per line in a narrow box, stretching its card).
#[test]
fn start_page_task_cards_line_up_at_any_width() {
    let mut gui = Gui::start();
    let tasks = [
        "Describe a column",
        "Compare groups",
        "Fit a model",
        "Watch a process",
        "Design an experiment",
        "Forecast a series",
    ];
    for (width, name) in [(1360.0, "start-page-cards-wide"), (980.0, "start-page-cards-narrow"), (760.0, "")] {
        gui.harness.set_size(egui::vec2(width, 900.0));
        gui.run();
        if !name.is_empty() {
            gui.shot(name);
        }
        let cards: Vec<_> = tasks
            .iter()
            .map(|t| gui.widget(t).raw_bounds().unwrap_or_else(|| panic!("{t} has a position")))
            .collect();
        for a in &cards {
            for b in &cards {
                if (a.y0 - b.y0).abs() < 1.0 {
                    assert!((a.height() - b.height()).abs() < 1.0, "cards in one row differ at {width}: {cards:?}");
                }
            }
        }
        let badge = gui
            .painted_text_rects()
            .into_iter()
            .find(|(t, ..)| t == "no data needed")
            .unwrap_or_else(|| panic!("no badge at {width}"));
        let line = gui.painted_text_rects().into_iter().find(|(t, ..)| t == "Fit a model").unwrap().1.height();
        assert!(badge.1.height() < line * 1.2, "the badge wrapped at {width}: {:?}", badge.1);
        // The tip moves under the Learn buttons rather than squeezing into a
        // narrow column that runs off the window's edge.
        let tip = gui
            .painted_text_rects()
            .into_iter()
            .find(|(t, ..)| crate::app::welcome::TIPS.contains(&t.as_str()))
            .expect("a tip");
        assert!(tip.1.right() <= width, "the tip runs off the window at {width}: {:?}", tip.1);
        assert!(tip.1.width() > 150.0, "the tip is squeezed at {width}: {:?}", tip.1);
    }
}
