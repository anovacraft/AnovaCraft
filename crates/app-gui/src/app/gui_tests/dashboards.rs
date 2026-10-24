//! Dashboards: tiles added from reports, moved and resized with undo,
//! filters shared across tables, brushing within a table, saving and
//! reopening, HTML export and a window of its own.

use super::*;
use app_model::dashboard::{GridArea, TileContent, TileId};
use app_model::project::TableId;

/// Where `node` is on screen.
fn rect(node: &Node<'_>) -> egui::Rect {
    let b = node.raw_bounds().expect("the node has a position");
    egui::Rect::from_min_max(egui::pos2(b.x0 as f32, b.y0 as f32), egui::pos2(b.x1 as f32, b.y1 as f32))
}

/// The greenhouse sample project (two tables and a dashboard, which it
/// shows), in a window tall enough for all six tiles.
fn greenhouse() -> Gui {
    let mut gui = Gui::start();
    apply_color_scheme(&gui.harness.ctx.clone(), ColorScheme::Light);
    gui.harness.set_size(egui::vec2(1360.0, 1400.0));
    gui.perform(Action::OpenProjectSample(0));
    gui
}

/// The dashboard the main window shows.
fn shown_dashboard(gui: &Gui) -> u64 {
    gui.app().shown_dashboard().expect("a dashboard is shown")
}

fn open(gui: &Gui, id: u64) -> &crate::app::dashboards::OpenDashboard {
    gui.app().dashboard(id).expect("the dashboard exists")
}

fn area(gui: &Gui, id: u64, tile: u32) -> GridArea {
    open(gui, id).dashboard.tile(TileId(tile)).expect("the tile exists").area
}

/// The title of report tile `tile`.
fn tile_title(gui: &Gui, id: u64, tile: u32) -> String {
    open(gui, id).tile_report(TileId(tile)).expect("a report tile, drawn").title()
}

/// The document of the sample project's table `table` (`table-1`: plants, `table-2`: climate).
fn table_index(gui: &Gui, table: &str) -> usize {
    let project = gui.app().documents[gui.app().shown].project;
    gui.app()
        .documents
        .iter()
        .position(|d| d.project == project && d.table_id == TableId::from(table))
        .expect("the table is open")
}

/// Drag from `from` to `to` in steps, saving a shot named `shot` halfway.
fn drag_with_shot(gui: &mut Gui, from: egui::Pos2, to: egui::Pos2, shot: &str) {
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    gui.harness.input_mut().events.push(egui::Event::PointerMoved(from));
    gui.harness.step();
    gui.harness.input_mut().events.push(button(from, true));
    gui.harness.step();
    for i in 1..=6 {
        let at = from.lerp(to, i as f32 / 6.0);
        gui.harness.input_mut().events.push(egui::Event::PointerMoved(at));
        gui.harness.step();
        if i == 6 {
            gui.shot(shot);
        }
    }
    gui.harness.input_mut().events.push(button(to, false));
    gui.run();
}

/// The sample opens on its dashboard: a heading, a note and four report
/// tiles from two tables, with a tab after the project's tables.
#[test]
fn sample_project_opens_on_its_dashboard() {
    let mut gui = greenhouse();
    let id = shown_dashboard(&gui);
    let d = &open(&gui, id).dashboard;
    assert_eq!(d.name, "Greenhouse overview");
    assert_eq!(d.tiles.len(), 6);
    assert!(gui.shows("Move tile Greenhouse trial: plants and climate"));
    for tile in 3..=6 {
        let title = tile_title(&gui, id, tile);
        assert!(gui.shows(&format!("Move tile {title}")), "{title}");
    }
    assert!(gui.harness.query_all_by_label("Greenhouse overview").next().is_some(), "its tab");
    gui.shot("dashboard-sample");
    // Dark theme, for review.
    apply_color_scheme(&gui.harness.ctx.clone(), ColorScheme::Dark);
    gui.run();
    gui.shot("dashboard-sample-dark");
}

/// "Add to dashboard" in a report's ⋯ Options menu adds a copy of the
/// report to a dashboard of its project, or makes a new one.
#[test]
fn add_to_dashboard_from_a_report() {
    let mut gui = greenhouse();
    let id = shown_dashboard(&gui);
    let plants = table_index(&gui, "table-1");
    gui.perform(Action::Activate(plants));
    gui.results();
    assert!(gui.app().shown_dashboard().is_none(), "the table is shown");
    let before = open(&gui, id).dashboard.tiles.len();
    let report = gui.app().documents[plants].reports[0].saved();
    let options = |n: &Node<'_>| n.role() == Role::Button && n.label().as_deref() == Some("⋯ Options");
    gui.harness.get_all_by(options).next().expect("a report").simulate_click();
    gui.run();
    gui.widget("Add to dashboard").hover();
    gui.run();
    gui.shot("dashboard-add-from-report");
    gui.click("Greenhouse overview");
    let d = &open(&gui, id).dashboard;
    assert_eq!(d.tiles.len(), before + 1);
    let added = d.tiles.last().unwrap();
    assert_eq!(added.content, TileContent::Report { table: TableId::from("table-1"), report: Box::new(report) });
    assert!(gui.app().shown_dashboard().is_none(), "adding to an existing dashboard keeps the table shown");
    // Undo in the dashboard takes it off again.
    gui.perform(Action::DashboardUndo(id));
    assert_eq!(open(&gui, id).dashboard.tiles.len(), before);

    // "New dashboard" makes one in the project, starting with the report, and shows it.
    gui.harness.get_all_by(options).next().expect("a report").simulate_click();
    gui.run();
    gui.widget("Add to dashboard").hover();
    gui.run();
    gui.click("New dashboard");
    let project = gui.app().documents[plants].project;
    assert_eq!(gui.app().project(project).unwrap().dashboards.len(), 2);
    let new = shown_dashboard(&gui);
    assert_ne!(new, id);
    assert_eq!(open(&gui, new).dashboard.name, "Dashboard");
    assert_eq!(open(&gui, new).dashboard.tiles.len(), 1);
    gui.shot("dashboard-new-from-report");
}

/// A tile is resized by dragging its corner and moved by dragging its
/// strip, on whole grid cells; Undo (button and ⌘Z) puts it back.
#[test]
fn tiles_move_and_resize_with_undo() {
    let mut gui = greenhouse();
    let id = shown_dashboard(&gui);
    let title = tile_title(&gui, id, 3);
    assert_eq!(area(&gui, id, 3), GridArea::new(0, 2, 6, 6));
    let strip = rect(&gui.widget(&format!("Move tile {title}")));
    let grip = rect(&gui.widget(&format!("Resize tile {title}")));
    // Six columns span the strip, with gaps of 10 points between them.
    let step = (strip.width() + 10.0) / 6.0;
    drag_with_shot(&mut gui, grip.center(), grip.center() - egui::vec2(2.0 * step, 0.0), "dashboard-resizing");
    assert_eq!(area(&gui, id, 3), GridArea::new(0, 2, 4, 6), "two columns narrower");

    let strip = rect(&gui.widget(&format!("Move tile {title}")));
    let from = egui::pos2(strip.left() + 8.0, strip.center().y);
    drag_with_shot(&mut gui, from, from + egui::vec2(2.0 * step, 0.0), "dashboard-moving");
    assert_eq!(area(&gui, id, 3), GridArea::new(2, 2, 4, 6), "two columns to the right");

    // Onto another tile: refused, the tile stays.
    let strip = rect(&gui.widget(&format!("Move tile {title}")));
    let from = egui::pos2(strip.left() + 8.0, strip.center().y);
    gui.drag(from, from + egui::vec2(4.0 * step, 0.0));
    assert_eq!(area(&gui, id, 3), GridArea::new(2, 2, 4, 6), "overlapping moves are refused");

    gui.click("Undo");
    assert_eq!(area(&gui, id, 3), GridArea::new(0, 2, 4, 6), "the move undone");
    gui.harness.press_key_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    gui.run();
    assert_eq!(area(&gui, id, 3), GridArea::new(0, 2, 6, 6), "⌘Z undoes the resize");
    gui.click("Redo");
    assert_eq!(area(&gui, id, 3), GridArea::new(0, 2, 4, 6));
    // Removing a tile is undoable too.
    gui.click(&format!("Remove tile {title}"));
    assert!(open(&gui, id).dashboard.tile(TileId(3)).is_none());
    gui.click("Undo");
    assert!(open(&gui, id).dashboard.tile(TileId(3)).is_some());
}

/// A filter on a column applies to every tile whose table has it; a tile
/// whose table lacks it says so. Rows a filter leaves out are "filtered
/// out", and the tables' own row states never change.
#[test]
fn filters_apply_across_tables_and_leave_the_tables_alone() {
    let mut gui = greenhouse();
    let id = shown_dashboard(&gui);
    let (plants, climate) = (table_index(&gui, "table-1"), table_index(&gui, "table-2"));
    // Greenhouse ∈ {1, 2, 3} applies to both tables; Fertilizer only to the plants.
    let painted = gui.painted_text().join("\n");
    assert!(painted.contains("15 of 60 rows filtered out"), "plants: {painted}");
    assert!(painted.contains("12 of 48 rows filtered out"), "climate");
    assert!(painted.contains("not filtered by Fertilizer"), "the climate tiles say the filter skips them");
    assert!(!painted.contains("rows excluded"), "filtered rows are not called excluded");
    for doc in [plants, climate] {
        let c = gui.app().documents[doc].rows.counts();
        assert_eq!((c.excluded, c.hidden), (0, 0), "the tables' own rows are untouched");
    }
    gui.shot("dashboard-filtered");

    // Add greenhouse 4 back through the filter's chip.
    gui.click("Filter Greenhouse");
    gui.click("4");
    gui.shot("dashboard-filter-menu");
    gui.harness.press_key(egui::Key::Escape);
    gui.run();
    let filter = open(&gui, id).dashboard.filter("Greenhouse").cloned().expect("still filtered");
    assert!(matches!(filter, stat_viz::FilterCondition::Levels { ref levels, .. } if levels.len() == 4));
    let painted = gui.painted_text().join("\n");
    assert!(!painted.contains("rows filtered out"), "every row passes now");
    assert_eq!(open(&gui, id).history.next_undo(), Some("Filter Greenhouse"));

    // A filter on a column only the climate table has narrows only its tiles.
    gui.click("+ Add filter");
    gui.click("Humidity");
    assert!(open(&gui, id).dashboard.filter("Humidity").is_some());
    gui.perform(Action::DashboardUndo(id));
    gui.perform(Action::DashboardUndo(id));
    let painted = gui.painted_text().join("\n");
    assert!(painted.contains("15 of 60 rows filtered out"), "undo brings the filter back");
    for doc in [plants, climate] {
        assert_eq!(gui.app().documents[doc].rows.counts().excluded, 0);
    }
}

/// Selecting points in a tile selects those rows in its own table only.
#[test]
fn brushing_in_a_tile_stays_within_its_table() {
    let mut gui = greenhouse();
    let id = shown_dashboard(&gui);
    let (plants, climate) = (table_index(&gui, "table-1"), table_index(&gui, "table-2"));
    // Tile 5: humidity against temperature, from the climate table, made tall enough to show its plot.
    let resize = app_model::dashboard::DashboardEdit::ResizeTile { tile: TileId(5), width: 6, height: 14 };
    assert!(gui.app_mut().edit_dashboard(id, resize));
    gui.harness.set_size(egui::vec2(1360.0, 1800.0));
    gui.run();
    let title = tile_title(&gui, id, 5);
    let strip = rect(&gui.widget(&format!("Move tile {title}")));
    let tile = egui::Rect::from_min_max(strip.min, strip.max + egui::vec2(0.0, 14.0 * 74.0));
    let screen = gui.harness.ctx.screen_rect();
    let in_tile = |r: &egui::Rect| tile.contains(r.center()) && screen.contains(r.center());
    let rulers = |label: &'static str| {
        gui.harness
            .get_all_by_label(label)
            .map(|n| rect(&n))
            // The last one: plots of the tile above, clipped by it, can reach down here.
            .rfind(|r| in_tile(r))
            .unwrap_or_else(|| panic!("{label} in the tile"))
    };
    let (x, y) = (rulers("Horizontal axis ruler"), rulers("Vertical axis ruler"));
    let from = egui::pos2(y.right() + 4.0, y.top() + 4.0);
    let to = egui::pos2(x.right() - 4.0, x.top() - 4.0);
    gui.drag(from, to);
    let selected = gui.app().documents[climate].rows.counts().selected;
    assert!(selected > 0, "the box selected climate rows");
    assert_eq!(gui.app().documents[plants].rows.counts().selected, 0, "no plant rows");
    assert_eq!(gui.app().active, climate, "the tile's table is the command target");
    // Filtered-out rows cannot be selected: greenhouse 4 is not drawn.
    let greenhouse = gui.app().documents[climate].table.find("Greenhouse").unwrap();
    let col = gui.app().documents[climate].table.column(greenhouse).unwrap().as_numeric().unwrap().to_vec();
    let rows = &gui.app().documents[climate].rows;
    assert!((0..rows.len()).filter(|&r| rows.is_selected(r)).all(|r| col[r] != 4.0));
    gui.shot("dashboard-brushing");
    // The selection shows in the climate table's grid.
    gui.perform(Action::Activate(climate));
    assert_eq!(gui.app().documents[climate].rows.counts().selected, selected);
}

/// Dashboards, their tiles, filters and windows are saved in the project
/// and come back when it is reopened.
#[test]
fn dashboards_save_and_reopen_with_their_window() {
    let mut gui = greenhouse();
    let id = shown_dashboard(&gui);
    let edit = open(&gui, id).dashboard.add_tile_edit(
        TileContent::Text { text: "Notes for the review".into(), style: app_model::dashboard::TextStyle::Body },
        6,
        2,
    );
    assert!(gui.app_mut().edit_dashboard(id, edit));
    gui.perform(Action::DashboardWindow(id));
    assert!(gui.app().in_window_dashboard(id));
    let saved = open(&gui, id).dashboard.clone();
    let project = gui.app().dashboard_project(id).unwrap();
    assert!(gui.app().project_modified(project), "the edit is unsaved");

    let dir = temp_dir("dashboard-project");
    let path = dir.join("greenhouse.acraft");
    let file = gui.app().project_file(project, true);
    assert_eq!(file.dashboards, vec![saved.clone()]);
    assert_eq!(file.layout.dashboard_windows.len(), 1);
    file_io::write_project_file(&file, &path).expect("project written");
    gui.app_mut().close_project(project);
    gui.run();
    assert!(gui.app().windows.is_empty(), "its window closed with it");

    gui.perform(Action::OpenPaths(vec![path.clone()]));
    let reopened = gui.app().documents[gui.app().shown].project;
    let p = gui.app().project(reopened).unwrap();
    assert_eq!(p.dashboards.len(), 1);
    assert_eq!(p.dashboards[0].dashboard, saved, "tiles, filters and name");
    let back = p.dashboards[0].id;
    assert!(gui.app().in_window_dashboard(back), "in its window again");
    assert!(!gui.app().project_modified(reopened), "just opened");
    gui.shot("dashboard-reopened");
    let _ = std::fs::remove_dir_all(dir);
}

/// "Export as HTML…" writes one page: the tiles on the same grid, each
/// report's sections in its tile, plots as SVG.
#[test]
fn dashboard_exports_as_one_html_page() {
    let mut gui = greenhouse();
    let id = shown_dashboard(&gui);
    let dir = temp_dir("dashboard-html");
    let path = dir.join("overview.html");
    file_io::test_save_answer::set(Some(path.clone()));
    gui.click("Export as HTML…");
    assert!(!file_io::test_save_answer::pending(), "the save dialog was asked");
    let html = std::fs::read_to_string(&path).expect("the page was written");
    assert!(html.contains("<h1>Greenhouse overview</h1>"));
    assert!(html.contains("grid-template-columns: repeat(12, minmax(0, 1fr))"));
    assert!(html.contains("grid-column: 1 / span 6; grid-row: 3 / span 6;"), "tile 3's place");
    assert!(html.contains("<svg"), "plots as SVG");
    assert!(html.contains("Greenhouse ∈ {1, 2, 3}"), "the filters");
    for tile in 3..=6 {
        let title = tile_title(&gui, id, tile);
        assert!(html.contains(&app_model::html_export::escape(&title)), "{title}");
    }
    assert!(html.contains("not filtered by Fertilizer"));
    assert!(!html.contains("<script"), "nothing to load");
    assert!(gui.painted_text().iter().any(|t| t.contains("Saved dashboard “Greenhouse overview” (6 tiles)")));
    let _ = std::fs::remove_dir_all(dir);
}

/// A dashboard pops out into a window of its own and comes back; the main
/// window shows its table meanwhile, and the window's Undo acts on it.
#[test]
fn dashboard_pops_out_into_its_own_window() {
    let mut gui = greenhouse();
    let id = shown_dashboard(&gui);
    gui.harness.set_size(egui::vec2(1500.0, 1400.0));
    gui.click("New window");
    assert!(gui.app().in_window_dashboard(id));
    assert!(gui.app().shown_dashboard().is_none(), "the main window shows the project's table");
    let title = "Greenhouse overview · Dashboard · AnovaCraft";
    let _ = gui.window(title);
    gui.shot("dashboard-window");
    // Removing a tile in the window, then Undo there.
    let tile = tile_title(&gui, id, 4);
    let remove = format!("Remove tile {tile}");
    gui.window(title).get_by_label(&remove).simulate_click();
    gui.run();
    assert!(open(&gui, id).dashboard.tile(TileId(4)).is_none());
    gui.window(title).get_by_label("Undo").simulate_click();
    gui.run();
    assert!(open(&gui, id).dashboard.tile(TileId(4)).is_some());
    // Showing it again brings the window forward rather than moving it.
    gui.perform(Action::ShowDashboard(id));
    assert!(gui.app().in_window_dashboard(id));
    gui.window(title).get_by_label("Back to main window").simulate_click();
    gui.run();
    assert!(!gui.app().in_window_dashboard(id));
    assert_eq!(gui.app().shown_dashboard(), Some(id), "back in the main window");
}

/// A project of several tables is a framed group in the tab bar, headed by
/// its name: clicking the header folds its tabs away (the shown one stays)
/// and shows them again.
#[test]
fn project_tabs_fold_behind_their_header() {
    let mut gui = greenhouse();
    let tabs = |gui: &Gui, name: &str| gui.harness.query_all_by_label(name).count();
    let plants = tabs(&gui, "Plant growth (simulated)");
    // The header: the project's name after a fold triangle.
    let header = |n: &Node<'_>| {
        n.role() == Role::Button
            && n.label().is_some_and(|l| {
                l.ends_with(" Greenhouse trial (simulated)") || l.contains("Greenhouse trial (simulated) (")
            })
    };
    assert_eq!(gui.harness.get_all_by(header).count(), 1);
    gui.shot("project-tabs");
    gui.harness.get_by(header).simulate_click();
    gui.run();
    assert!(gui.app().projects.iter().any(|p| p.tabs_folded));
    assert_eq!(tabs(&gui, "Plant growth (simulated)"), plants - 1, "its tab is folded away");
    assert!(gui.shows("Greenhouse overview"), "the shown dashboard keeps its tab");
    gui.shot("project-tabs-folded");
    gui.harness.get_by(header).simulate_click();
    gui.run();
    assert_eq!(tabs(&gui, "Plant growth (simulated)"), plants);
}

/// A shared column picker shows another column in every tile that reads
/// its column; tiles it cannot switch say so, the tiles' own requests never
/// change, and Undo steps back.
#[test]
fn a_shared_switcher_changes_every_tile_that_reads_its_column() {
    let mut gui = greenhouse();
    let id = shown_dashboard(&gui);
    let tiles = open(&gui, id).dashboard.tiles.clone();
    let switcher = |gui: &Gui| open(gui, id).dashboard.switcher("Humidity").cloned().expect("the sample's picker");
    assert_eq!(switcher(&gui).choices, ["Humidity", "Temperature", "Week"]);
    let (scatter, humidity) = (tile_title(&gui, id, 5), tile_title(&gui, id, 6));
    let plant_titles = (tile_title(&gui, id, 3), tile_title(&gui, id, 4));
    assert!(humidity.contains("Humidity"), "{humidity}");

    // Temperature: the distribution switches; the scatter plot already uses Temperature.
    gui.click("Next Humidity");
    assert_eq!(switcher(&gui).current, "Temperature");
    let shown = tile_title(&gui, id, 6);
    assert!(shown.contains("Temperature") && !shown.contains("Humidity"), "{shown}");
    assert_eq!(tile_title(&gui, id, 5), scatter, "left as it is");
    let painted = gui.painted_text().join("\n");
    assert!(painted.contains("Humidity → Temperature"), "the switched tile says so: {painted}");
    assert!(painted.contains("not switched: Humidity"), "and so does the one left alone");
    gui.shot("dashboard-switcher");

    // Week: both climate tiles switch; the plant tiles do not read Humidity.
    gui.click("Next Humidity");
    assert_eq!(switcher(&gui).current, "Week");
    assert!(tile_title(&gui, id, 5).contains("Week"), "{}", tile_title(&gui, id, 5));
    assert!(tile_title(&gui, id, 6).contains("Week"));
    assert_eq!((tile_title(&gui, id, 3), tile_title(&gui, id, 4)), plant_titles);
    assert_eq!(open(&gui, id).dashboard.tiles, tiles, "the tiles keep their own requests");
    assert_eq!(open(&gui, id).history.next_undo(), Some("Switch Humidity"));

    // The export names the switched column.
    let dir = temp_dir("dashboard-switcher-html");
    let path = dir.join("overview.html");
    file_io::test_save_answer::set(Some(path.clone()));
    gui.click("Export as HTML…");
    let html = std::fs::read_to_string(&path).expect("the page was written");
    assert!(html.contains("columns switched: Humidity → Week"));
    let _ = std::fs::remove_dir_all(dir);

    // Undo steps back; choosing Humidity again shows the tiles as they were.
    gui.click("Undo");
    assert_eq!(switcher(&gui).current, "Temperature");
    gui.click("Previous Humidity");
    assert_eq!(switcher(&gui).current, "Humidity");
    assert_eq!((tile_title(&gui, id, 5), tile_title(&gui, id, 6)), (scatter, humidity));

    // Adding a switcher offers the project's other columns of the same type; × removes it.
    gui.click("+ Add picker");
    gui.click("Add picker on Height");
    let height = open(&gui, id).dashboard.switcher("Height").cloned().expect("added");
    assert_eq!(height.current, "Height");
    assert!(height.choices.contains(&"Light hours".to_owned()) && height.choices.contains(&"Temperature".to_owned()));
    assert!(!height.choices.contains(&"Fertilizer".to_owned()), "a text column is not offered for a numeric one");
    gui.click("Remove picker Height");
    assert!(open(&gui, id).dashboard.switcher("Height").is_none());
}
