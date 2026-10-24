//! Dashboards: reports and notes from a project's tables arranged on a grid,
//! with local filters shared by every tile (roadmap §10; the engine is
//! [`app_model::dashboard`]).
//!
//! A dashboard belongs to an open project and uses only that project's
//! tables. It is listed under the project in the sidebar and has a tab after
//! the project's tables; showing it fills the main window's workspace (or a
//! window of its own) with:
//!
//! * a toolbar: the dashboard's name, Undo and Redo of dashboard edits, "Add
//!   tile" (a copy of a report of one of the project's tables, a graph
//!   canvas, a heading or a note), "Export as HTML…" and "New window";
//! * the filter strip: one condition per column name, offered for every
//!   column of the project's tables, with the graph canvas's level and range
//!   choices. A filter applies to each tile whose table has that column; a
//!   tile whose table lacks it says so in its strip. Filters never change
//!   the tables' own row states: each tile runs on a copy with the rows
//!   outside the filters excluded and hidden, and its report says how many
//!   rows were "filtered out" (rather than "excluded");
//! * the switcher strip: shared column pickers, one per column name. A
//!   switcher steps through its choices (previous, list, next); every report
//!   tile that reads its column shows the chosen column in its place, and a
//!   tile it cannot switch (its table lacks the column, or it already reads
//!   it) says so in its strip. Tiles keep their own requests: a display
//!   option changed in a switched tile is stored against the switcher's own
//!   column ([`Dashboard::unswitched_report`]);
//! * the grid: tiles on the engine's 12 columns, each moved by dragging its
//!   strip, resized by dragging its corner, and removed with its ×.
//!
//! A tile holds its own copy of a report request bound to one table (made by
//! the report's "Add to dashboard" or by "Add tile"); changing the tile's
//! display options changes that copy only. Selecting rows in a tile selects
//! them in its table (so in that table's grid and every other view of it),
//! never in other tables. Every change to the dashboard (adding, moving,
//! resizing and removing tiles, filters, switchers, display options, its name) is one
//! [`DashboardEdit`] in the dashboard's [`DashboardHistory`], so Undo and
//! Redo work. Dashboards, their windows and the dashboard shown are saved in
//! the project file.

use crate::dialog_kit::ComboExt as _;
use std::collections::HashMap;
use std::sync::Arc;

use app_model::dashboard::{
    Dashboard, DashboardEdit, DashboardHistory, GridArea, SharedSwitcher, TableFilter, TextStyle, TileContent, TileId,
};
use app_model::project::{SavedReport, TableId};
use stat_viz::FilterCondition;

use super::*;

/// Height of one grid row, in points at the standard text size.
const ROW_HEIGHT: f32 = 64.0;
/// Space between tiles, in points.
const GAP: f32 = 10.0;
/// Height of a tile's strip (its handle for moving), in points.
const STRIP: f32 = 24.0;
/// Size of the resize grip in a tile's lower right corner, in points.
const GRIP: f32 = 14.0;
/// Span of a new report tile (columns, rows).
pub(super) const REPORT_TILE: (u32, u32) = (6, 6);
/// Span of a new heading tile.
const HEADING_TILE: (u32, u32) = (12, 1);
/// Span of a new note tile.
const NOTE_TILE: (u32, u32) = (6, 2);

/// A dashboard taken out of its project, kept so Edit ▸ Undo can put it back.
pub(super) struct RemovedDashboard {
    /// The project it was in (by id).
    pub project: u64,
    /// Its place in the project's list of dashboards.
    pub position: usize,
    /// The dashboard with its tiles, filters and undo history.
    pub open: OpenDashboard,
    /// It was shown in the main window.
    pub shown: bool,
}

/// An open dashboard: the engine's dashboard, its undo history and what is
/// drawn for it.
pub(super) struct OpenDashboard {
    /// Unique for the life of the application (egui ids, windows).
    pub id: u64,
    pub dashboard: Dashboard,
    pub history: DashboardHistory,
    /// The report shown in each report tile.
    tiles: HashMap<TileId, TileState>,
    /// Each table's filter, kept until the table or the filters change.
    filters: HashMap<TableId, CachedFilter>,
    /// A tile being moved or resized.
    gesture: Option<Gesture>,
    /// A filter being changed (a slider held down): applied as one edit on release.
    pending_filter: Option<FilterCondition>,
    /// A text tile being edited, with its text so far.
    editing: Option<(TileId, String)>,
    /// The name typed in the Rename box.
    rename: String,
    /// Shown by the web viewer: filters, switchers and display options
    /// change; tiles cannot be added, moved, resized, removed or edited.
    pub read_only: bool,
}

impl OpenDashboard {
    pub fn new(id: u64, dashboard: Dashboard) -> Self {
        let rename = dashboard.name.clone();
        Self {
            id,
            dashboard,
            history: DashboardHistory::default(),
            tiles: HashMap::new(),
            filters: HashMap::new(),
            gesture: None,
            pending_filter: None,
            editing: None,
            read_only: false,
            rename,
        }
    }

    /// The report shown in tile `tile`, if it is a report tile that has been drawn.
    #[cfg(test)]
    pub fn tile_report(&self, tile: TileId) -> Option<&Report> {
        self.tiles.get(&tile).map(|t| &t.report)
    }
}

/// A report tile's report and what it was made from.
struct TileState {
    report: Report,
    /// The request the report was computed from: the tile's content then, as
    /// the switchers made it.
    built_from: SavedReport,
    /// Switchers that changed the request, as (switcher column, column shown).
    applied: Vec<(String, String)>,
    /// Switchers on a column the request reads that could not change it.
    unswitched: Vec<String>,
    table: TableId,
    /// The dashboard's filters when it was computed.
    filters: Vec<FilterCondition>,
}

/// A table's filter as of a data revision and a set of filters.
struct CachedFilter {
    revision: u64,
    n_rows: usize,
    filters: Vec<FilterCondition>,
    filter: TableFilter,
    /// Rows outside the filters that the table itself does not exclude:
    /// the ones reports call "filtered out".
    outside: Arc<Vec<bool>>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum GestureKind {
    Move,
    Resize,
}

/// A move or resize in progress: the tile, and where it would go now.
#[derive(Clone, Copy, Debug)]
struct Gesture {
    tile: TileId,
    kind: GestureKind,
    /// Pointer offset from the tile's top-left corner when the drag began.
    grab: egui::Vec2,
    target: GridArea,
}

/// Where the grid's cells are on screen.
#[derive(Clone, Copy)]
struct GridGeometry {
    origin: egui::Pos2,
    column_width: f32,
    row_height: f32,
    columns: u32,
}

impl GridGeometry {
    fn rect(&self, area: GridArea) -> egui::Rect {
        let x = self.origin.x + area.column as f32 * (self.column_width + GAP);
        let y = self.origin.y + area.row as f32 * (self.row_height + GAP);
        let w = area.width as f32 * (self.column_width + GAP) - GAP;
        let h = area.height as f32 * (self.row_height + GAP) - GAP;
        egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w.max(1.0), h.max(1.0)))
    }

    /// The cell nearest to `pos` (top-left corner of a tile).
    fn cell_at(&self, pos: egui::Pos2) -> (u32, u32) {
        let col = ((pos.x - self.origin.x) / (self.column_width + GAP)).round().max(0.0) as u32;
        let row = ((pos.y - self.origin.y) / (self.row_height + GAP)).round().max(0.0) as u32;
        (col, row)
    }

    /// The span reaching from `area`'s top-left cell to `pos` (bottom-right corner).
    fn span_to(&self, area: GridArea, pos: egui::Pos2) -> (u32, u32) {
        let left = self.origin.x + area.column as f32 * (self.column_width + GAP);
        let top = self.origin.y + area.row as f32 * (self.row_height + GAP);
        let w = ((pos.x - left + GAP) / (self.column_width + GAP)).round().max(1.0) as u32;
        let h = ((pos.y - top + GAP) / (self.row_height + GAP)).round().max(1.0) as u32;
        (w.min(self.columns.saturating_sub(area.column).max(1)), h)
    }
}

/// What a dashboard's view asked for this frame, carried out once it is drawn.
#[derive(Default)]
pub(super) struct DashboardOutcome {
    pub edits: Vec<DashboardEdit>,
    /// The document (index) whose tile was used: it becomes the command target.
    used_table: Option<usize>,
    /// Columns, reports and tables asked for by tile reports, per document index.
    new_columns: Vec<(usize, report_view::NewColumn)>,
    new_reports: Vec<(usize, ReportRequest)>,
    pub new_tables: Vec<(usize, DataTable, bool)>,
    undo: bool,
    redo: bool,
    pub export: bool,
    publish: bool,
    pub message: Option<String>,
}

impl DashboardOutcome {
    /// Keep only what a read-only dashboard allows: filter, switcher and
    /// tile display changes, undo and redo of them, and the command target.
    /// Anything that would change the layout or the project is dropped,
    /// with a message when a report asked for it.
    pub(super) fn keep_view_changes(&mut self) {
        self.edits.retain(|e| {
            matches!(
                e,
                DashboardEdit::SetFilter { .. }
                    | DashboardEdit::RemoveFilter { .. }
                    | DashboardEdit::SetSwitcher { .. }
                    | DashboardEdit::RemoveSwitcher { .. }
                    | DashboardEdit::SetTileContent { content: TileContent::Report { .. }, .. }
            )
        });
        if !(self.new_columns.is_empty() && self.new_reports.is_empty() && self.new_tables.is_empty()) {
            self.message =
                Some("A published dashboard cannot add columns, reports or tables; open the project in AnovaCraft to do that".to_owned());
        }
        self.new_columns.clear();
        self.new_reports.clear();
        self.new_tables.clear();
        self.export = false;
        self.publish = false;
    }
}

/// A column offered for filtering: its name, its levels and value range
/// across the tables that have it, and whether it is continuous in all of them.
type FilterColumn = (String, Vec<String>, (f64, f64), bool);

/// The column names of the project's tables, in first-seen order, with the
/// levels and value range each column has across the tables that have it.
fn project_columns(tables: &[&DataTable]) -> Vec<FilterColumn> {
    let mut out: Vec<FilterColumn> = Vec::new();
    for t in tables {
        for c in t.columns() {
            let continuous =
                FilterCondition::for_column(t, &c.name).is_some_and(|f| matches!(f, FilterCondition::Range { .. }));
            let levels = stat_viz::filter::column_levels(t, &c.name);
            let range = c
                .as_numeric()
                .unwrap_or(&[])
                .iter()
                .filter(|v| v.is_finite())
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &v| (a.min(v), b.max(v)));
            match out.iter_mut().find(|(n, ..)| *n == c.name) {
                Some((_, lv, r, cont)) => {
                    for l in levels {
                        if !lv.contains(&l) {
                            lv.push(l);
                        }
                    }
                    lv.sort_by(|a, b| stat_data::natural_order(a, b));
                    *r = (r.0.min(range.0), r.1.max(range.1));
                    *cont = *cont && continuous;
                }
                None => out.push((c.name.clone(), levels, range, continuous)),
            }
        }
    }
    for (_, _, r, _) in &mut out {
        if !r.0.is_finite() {
            *r = (0.0, 1.0);
        } else if r.1 <= r.0 {
            r.1 = r.0 + 1.0;
        }
    }
    out
}

/// A new filter on `column` that every row passes.
fn open_filter(column: &str, levels: &[String], range: (f64, f64), continuous: bool) -> FilterCondition {
    if continuous {
        FilterCondition::Range { column: column.to_owned(), min: range.0, max: range.1 }
    } else {
        FilterCondition::Levels { column: column.to_owned(), levels: levels.to_vec() }
    }
}

/// `text`, shortened to `max` characters with "…".
fn short(text: &str, max: usize) -> String {
    if text.chars().count() > max { text.chars().take(max - 1).chain(['…']).collect() } else { text.to_owned() }
}

/// What a tile is called in its strip and in screen-reader labels.
fn tile_title(content: &TileContent, state: Option<&TileState>) -> String {
    match content {
        TileContent::Report { .. } => state.map_or_else(|| "Report".to_owned(), |s| s.report.title()),
        TileContent::Text { text, style } => {
            let first = text.lines().next().unwrap_or("").trim();
            let kind = if *style == TextStyle::Heading { "Heading" } else { "Note" };
            if first.is_empty() { kind.to_owned() } else { short(first, 40) }
        }
    }
}

/// Six dots in two columns: the handle of a tile's strip.
fn paint_grip_dots(painter: &egui::Painter, center: egui::Pos2, color: egui::Color32) {
    for dx in [-2.5_f32, 2.5] {
        for dy in [-4.0_f32, 0.0, 4.0] {
            painter.circle_filled(center + egui::vec2(dx, dy), 1.2, color);
        }
    }
}

/// Three short diagonal lines in a corner: the resize grip.
fn paint_resize_grip(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let stroke = egui::Stroke::new(1.2_f32, color);
    for k in [3.0_f32, 7.0, 11.0] {
        painter.line_segment(
            [egui::pos2(rect.right() - k, rect.bottom() - 2.0), egui::pos2(rect.right() - 2.0, rect.bottom() - k)],
            stroke,
        );
    }
}

impl StatsApp {
    /// The (project index, dashboard index) of dashboard `id`.
    pub(super) fn find_dashboard(&self, id: u64) -> Option<(usize, usize)> {
        self.projects
            .iter()
            .enumerate()
            .find_map(|(p, project)| project.dashboards.iter().position(|d| d.id == id).map(|d| (p, d)))
    }

    /// Dashboard `id`.
    pub(super) fn dashboard(&self, id: u64) -> Option<&OpenDashboard> {
        self.find_dashboard(id).map(|(p, d)| &self.projects[p].dashboards[d])
    }

    /// The open project dashboard `id` belongs to.
    pub(super) fn dashboard_project(&self, id: u64) -> Option<u64> {
        self.find_dashboard(id).map(|(p, _)| self.projects[p].id)
    }

    /// The table a dashboard's window and commands belong to: its project's
    /// first table (by index).
    pub(super) fn dashboard_home(&self, id: u64) -> Option<usize> {
        let project = self.dashboard_project(id)?;
        self.documents.iter().position(|d| d.project == project)
    }

    /// The dashboard the main window shows, if it still exists.
    pub(super) fn shown_dashboard(&self) -> Option<u64> {
        self.shown_dashboard.filter(|&id| self.find_dashboard(id).is_some())
    }

    /// The document (index) of table `table` of project `project`.
    fn table_index(&self, project: u64, table: &TableId) -> Option<usize> {
        self.documents.iter().position(|d| d.project == project && &d.table_id == table)
    }

    /// A new, empty dashboard in project `project` named "Dashboard",
    /// "Dashboard 2", …; returns its id.
    pub(super) fn new_dashboard(&mut self, project: u64) -> Option<u64> {
        let id = self.next_dashboard_id;
        let p = self.projects.iter().position(|p| p.id == project)?;
        self.next_dashboard_id += 1;
        let names: Vec<String> = self.projects[p].dashboards.iter().map(|d| d.dashboard.name.clone()).collect();
        let name = app_model::table_ops::unique_name("Dashboard", names.iter().map(String::as_str));
        let file = ProjectFile {
            dashboards: self.projects[p].dashboards.iter().map(|d| d.dashboard.clone()).collect(),
            ..ProjectFile::default()
        };
        let dashboard_id = file.unused_dashboard_id();
        let dashboard = app_model::Dashboard::new(dashboard_id, name);
        self.projects[p].dashboards.push(OpenDashboard::new(id, dashboard));
        self.modified_marks.remove(&project);
        Some(id)
    }

    /// Show dashboard `id` in the main window (or bring its window to the
    /// front), making its project's first table the command target.
    pub(super) fn show_dashboard(&mut self, id: u64) {
        if self.in_window_dashboard(id) {
            self.raise_dashboard_window(id);
            return;
        }
        let Some(home) = self.dashboard_home(id) else { return };
        let keeps_target = self.documents.get(self.active).map(|d| d.project) == self.dashboard_project(id);
        if !keeps_target {
            self.set_target(home);
        }
        self.shown = self.active;
        self.shown_dashboard = Some(id);
    }

    /// Apply `edit` to dashboard `id` through its history (so Undo reverts it).
    pub(super) fn edit_dashboard(&mut self, id: u64, edit: DashboardEdit) -> bool {
        let Some((p, d)) = self.find_dashboard(id) else { return false };
        let project = self.projects[p].id;
        let open = &mut self.projects[p].dashboards[d];
        match open.history.apply(edit, &mut open.dashboard) {
            Ok(()) => {
                self.modified_marks.remove(&project);
                true
            }
            Err(e) => {
                self.error(format!("Could not change the dashboard: {e}"));
                false
            }
        }
    }

    /// Undo (or redo) the latest edit of dashboard `id`.
    pub(super) fn dashboard_undo_redo(&mut self, id: u64, undo: bool) {
        let Some((p, d)) = self.find_dashboard(id) else { return };
        let project = self.projects[p].id;
        let open = &mut self.projects[p].dashboards[d];
        let label = if undo { open.history.next_undo() } else { open.history.next_redo() }.map(str::to_owned);
        let result = if undo { open.history.undo(&mut open.dashboard) } else { open.history.redo(&mut open.dashboard) };
        open.rename = open.dashboard.name.clone();
        match (result, label) {
            (Some(Ok(())), Some(label)) => {
                self.modified_marks.remove(&project);
                self.info(format!("{} {}", if undo { "Undid" } else { "Redid" }, label.to_lowercase()));
            }
            (Some(Err(e)), _) => self.error(format!("Could not {}: {e}", if undo { "undo" } else { "redo" })),
            _ => {}
        }
    }

    /// Add a tile showing a copy of `saved` (a report of document
    /// `doc_index`'s table) to dashboard `target`, or to a new dashboard of
    /// the table's project (which is then shown).
    pub(super) fn add_report_tile(&mut self, doc_index: usize, saved: SavedReport, title: &str, target: Option<u64>) {
        let Some(doc) = self.documents.get(doc_index) else { return };
        let (project, table) = (doc.project, doc.table_id.clone());
        let (id, created) = match target.filter(|&t| self.dashboard_project(t) == Some(project)) {
            Some(t) => (t, false),
            None => match self.new_dashboard(project) {
                Some(t) => (t, true),
                None => return,
            },
        };
        let Some(open) = self.dashboard(id) else { return };
        let (w, h) = REPORT_TILE;
        let edit = open.dashboard.add_tile_edit(TileContent::Report { table, report: Box::new(saved) }, w, h);
        let name = open.dashboard.name.clone();
        if self.edit_dashboard(id, edit) {
            if created {
                self.show_dashboard(id);
                self.info(format!("Made dashboard “{name}” with “{title}”. Save the project to keep it."));
            } else {
                self.info(format!("Added “{title}” to dashboard “{name}”"));
            }
        }
    }

    /// Remove dashboard `id` from its project. Edit ▸ Undo puts it back
    /// (see [`RemovedDashboard`]).
    pub(super) fn remove_dashboard(&mut self, id: u64) {
        let Some((p, d)) = self.find_dashboard(id) else { return };
        let open = self.projects[p].dashboards.remove(d);
        let project = self.projects[p].id;
        self.modified_marks.remove(&project);
        self.windows.retain(|w| w.view != View::Dashboard(id));
        let shown = self.shown_dashboard == Some(id);
        if shown {
            self.shown_dashboard = None;
        }
        self.info(format!(
            "Removed dashboard “{}”. Edit ▸ Undo brings it back; save the project to keep the change.",
            open.dashboard.name
        ));
        self.removed_dashboard = Some(RemovedDashboard { project, position: d, open, shown });
    }

    /// The name of the dashboard Edit ▸ Undo would bring back into the
    /// active table's project, if any.
    pub(super) fn restorable_dashboard(&self) -> Option<String> {
        let project = self.documents.get(self.active)?.project;
        let removed = self.removed_dashboard.as_ref().filter(|r| r.project == project)?;
        self.project(project)?;
        Some(removed.open.dashboard.name.clone())
    }

    /// Put the dashboard removed last back into its project, where it was,
    /// with its tiles, filters and history. Returns whether one came back.
    pub(super) fn restore_removed_dashboard(&mut self) -> bool {
        if self.restorable_dashboard().is_none() {
            return false;
        }
        let Some(removed) = self.removed_dashboard.take() else { return false };
        let Some(p) = self.projects.iter().position(|p| p.id == removed.project) else { return false };
        let dashboards = &mut self.projects[p].dashboards;
        let id = removed.open.id;
        let name = removed.open.dashboard.name.clone();
        dashboards.insert(removed.position.min(dashboards.len()), removed.open);
        self.modified_marks.remove(&removed.project);
        if removed.shown {
            self.show_dashboard(id);
        }
        self.info(format!("Brought back dashboard “{name}”"));
        true
    }

    /// Names of the dashboards of document `index`'s project whose tiles all
    /// show that table, so removing it from the project would leave them empty.
    pub(super) fn dashboards_emptied_by(&self, index: usize) -> Vec<String> {
        let Some(doc) = self.documents.get(index) else { return Vec::new() };
        let Some(project) = self.project(doc.project) else { return Vec::new() };
        project
            .dashboards
            .iter()
            .filter(|d| {
                !d.dashboard.tiles.is_empty()
                    && d.dashboard.tiles.iter().all(|t| t.content.table() == Some(&doc.table_id))
            })
            .map(|d| d.dashboard.name.clone())
            .collect()
    }

    /// The question before removing a dashboard from its project.
    pub(super) fn dashboard_removal_confirmation(&mut self, ctx: &egui::Context) {
        let Some(id) = self.confirm_dashboard_removal else { return };
        let Some(name) = self.dashboard(id).map(|d| d.dashboard.name.clone()) else {
            self.confirm_dashboard_removal = None;
            return;
        };
        let mut choice = None;
        egui::Window::new("Remove dashboard?")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                ui.label(RichText::new(format!("Remove dashboard “{name}” from the project?")).strong());
                ui.label(
                    "Its tiles, filters and layout are lost unless the project is saved without it. Edit ▸ Undo \
                     brings it back.",
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let keep = ui.button("Keep");
                    if ui.memory(|m| m.focused().is_none()) {
                        keep.request_focus();
                    }
                    if keep.clicked() {
                        choice = Some(false);
                    }
                    if ui.button("Remove").clicked() {
                        choice = Some(true);
                    }
                });
            });
        // Enter and Escape keep it (Keep is the default).
        if choice.is_none() && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            choice = Some(false);
        }
        let Some(remove) = choice else { return };
        self.confirm_dashboard_removal = None;
        if remove {
            self.remove_dashboard(id);
        }
    }

    /// Take tiles bound to table `table` out of project `project`'s
    /// dashboards (the table is leaving the project). Their undo histories
    /// are cleared, since they could bring such tiles back.
    pub(super) fn drop_tiles_of_table(&mut self, project: u64, table: &TableId) -> usize {
        let Some(p) = self.projects.iter().position(|p| p.id == project) else { return 0 };
        let mut n = 0;
        for open in &mut self.projects[p].dashboards {
            let before = open.dashboard.tiles.len();
            open.dashboard.tiles.retain(|t| t.content.table() != Some(table));
            if open.dashboard.tiles.len() != before {
                n += before - open.dashboard.tiles.len();
                open.history.clear();
            }
        }
        n
    }

    /// If removing column `column` of document `index` would break a
    /// dashboard tile, the message saying so.
    pub(super) fn tile_column_use(&self, index: usize, column: usize) -> Option<String> {
        let doc = self.documents.get(index)?;
        let project = self.project(doc.project)?;
        let name = doc.table.column(column).map_or("", |c| c.name.as_str());
        let users = project
            .dashboards
            .iter()
            .flat_map(|d| d.dashboard.reports_on(&doc.table_id))
            .filter(|r| r.columns().contains(&column) || r.column_names().contains(&name))
            .count();
        (users > 0).then(|| {
            format!(
                "Column “{name}” is used by {} on a dashboard; remove those tiles before deleting it",
                count(users, "tile", "tiles")
            )
        })
    }

    /// Keep dashboard tiles bound to document `index`'s table on their
    /// columns after an edit inserted, removed or renamed columns (`before`:
    /// the column names before it). Not an undoable dashboard edit: undoing
    /// the data edit moves the tiles back the same way.
    pub(super) fn dashboards_follow_columns(&mut self, index: usize, shifts: &[ColumnShift], before: &[String]) {
        let Some(doc) = self.documents.get(index) else { return };
        let (project, table_id) = (doc.project, doc.table_id.clone());
        let after: Vec<String> = column_names(&doc.table);
        let Some(p) = self.projects.iter().position(|p| p.id == project) else { return };
        for open in &mut self.projects[p].dashboards {
            for tile in &mut open.dashboard.tiles {
                let TileContent::Report { table, report } = &mut tile.content else { continue };
                if *table != table_id {
                    continue;
                }
                if !shifts.is_empty() {
                    if !matches!(**report, SavedReport::Graph { .. }) {
                        for shift in shifts {
                            **report = report.clone().map_columns(|c| shift.map(c).unwrap_or(c));
                        }
                    }
                } else if before.len() == after.len() {
                    for (old, new) in before.iter().zip(&after) {
                        if old != new {
                            report.rename_column(old, new);
                        }
                    }
                }
            }
        }
    }

    /// The dashboards of document `index`'s project as (id, name), for a
    /// report's "Add to dashboard" menu.
    pub(super) fn dashboard_choices(&self, index: usize) -> Vec<(u64, String)> {
        self.documents
            .get(index)
            .and_then(|d| self.project(d.project))
            .map(|p| p.dashboards.iter().map(|d| (d.id, d.dashboard.name.clone())).collect())
            .unwrap_or_default()
    }

    /// Bring every tile's report up to date with its table, its request and
    /// the filters, and return each table's filtered row states (by table id).
    fn prepare_tiles(&mut self, p: usize, d: usize) -> HashMap<TableId, RowStateManager> {
        let project = self.projects[p].id;
        let documents = &self.documents;
        let open = &mut self.projects[p].dashboards[d];
        let find = |t: &TableId| documents.iter().find(|doc| doc.project == project && &doc.table_id == t);
        let mut rows: HashMap<TableId, RowStateManager> = HashMap::new();
        let tables: Vec<TableId> = open.dashboard.tables().into_iter().cloned().collect();
        open.filters.retain(|t, _| tables.contains(t));
        for t in &tables {
            let Some(doc) = find(t) else { continue };
            let stale = open.filters.get(t).is_none_or(|c| {
                c.revision != doc.table.revision()
                    || c.n_rows != doc.table.n_rows()
                    || c.filters != open.dashboard.filters
            });
            if stale {
                let filter = open.dashboard.table_filter(&doc.table);
                let outside: Vec<bool> = filter.mask.iter().map(|m| !m).collect();
                open.filters.insert(
                    t.clone(),
                    CachedFilter {
                        revision: doc.table.revision(),
                        n_rows: doc.table.n_rows(),
                        filters: open.dashboard.filters.clone(),
                        filter,
                        outside: Arc::new(outside),
                    },
                );
            }
            let cached = &open.filters[t];
            let mut copy = doc.rows.clone();
            let out: Vec<usize> = cached.outside.iter().enumerate().filter(|(_, o)| **o).map(|(r, _)| r).collect();
            if !out.is_empty() {
                copy.set_excluded(out.iter().copied(), true);
                copy.set_hidden(out, true);
            }
            rows.insert(t.clone(), copy);
        }
        // Rows the reports call "filtered out": outside the filters and not excluded by the table itself.
        let mut filtered_out: HashMap<TableId, Arc<Vec<bool>>> = HashMap::new();
        for t in &tables {
            let (Some(doc), Some(c)) = (find(t), open.filters.get(t)) else { continue };
            let mask: Vec<bool> = c.outside.iter().enumerate().map(|(r, o)| *o && !doc.rows.is_excluded(r)).collect();
            filtered_out.insert(t.clone(), Arc::new(mask));
        }
        let filters = open.dashboard.filters.clone();
        let mut next_id = self.next_report_id;
        let present: Vec<TileId> = open.dashboard.tiles.iter().map(|t| t.id).collect();
        open.tiles.retain(|id, _| present.contains(id));
        for tile in &open.dashboard.tiles {
            let TileContent::Report { table, report: saved } = &tile.content else {
                open.tiles.remove(&tile.id);
                continue;
            };
            let (Some(doc), Some(filtered)) = (find(table), rows.get(table)) else { continue };
            let switched = open.dashboard.switched_report(saved, &doc.table);
            let shown = &switched.report;
            match open.tiles.get_mut(&tile.id) {
                Some(state) if &state.built_from == shown && &state.table == table => {
                    if state.filters != filters {
                        state.report.replace_all(shown.clone(), &doc.table, filtered);
                        state.filters = filters.clone();
                    } else {
                        state.report.refresh_if_stale(&doc.table, filtered);
                    }
                }
                Some(state) if &state.table == table => {
                    // Undo, redo, a switcher or a column change gave the tile another request.
                    state.report.replace_all(shown.clone(), &doc.table, filtered);
                    state.built_from = shown.clone();
                    state.filters = filters.clone();
                }
                _ => {
                    let mut report = Report::new(next_id, shown.clone(), &doc.table, filtered);
                    next_id += 1;
                    report.in_dashboard = true;
                    open.tiles.insert(
                        tile.id,
                        TileState {
                            report,
                            built_from: shown.clone(),
                            applied: Vec::new(),
                            unswitched: Vec::new(),
                            table: table.clone(),
                            filters: filters.clone(),
                        },
                    );
                }
            }
            if let Some(state) = open.tiles.get_mut(&tile.id) {
                state.report.filtered_out = filtered_out.get(table).cloned();
                state.applied = switched.applied;
                state.unswitched = switched.skipped;
            }
        }
        self.next_report_id = next_id;
        rows
    }

    /// Draw dashboard `id` (toolbar, filters and tiles) in `ui`, and carry
    /// out what was asked for there. `window`: it is drawn in a window of
    /// its own.
    pub(super) fn dashboard_view(&mut self, ui: &mut egui::Ui, id: u64, window: bool, actions: &mut Vec<Action>) {
        let Some((p, d)) = self.find_dashboard(id) else {
            ui.label(RichText::new("This dashboard was removed.").weak());
            return;
        };
        let mut rows = self.prepare_tiles(p, d);
        let before: HashMap<TableId, u64> = rows.iter().map(|(t, r)| (t.clone(), r.revision())).collect();
        let project = self.projects[p].id;
        let project_name = self.project_name(project);
        let members: Vec<usize> = self.members(project);
        let mut out = DashboardOutcome::default();
        {
            let documents = &self.documents;
            let open = &mut self.projects[p].dashboards[d];
            toolbar(ui, open, documents, &members, window, &project_name, &mut out, actions);
            ui.separator();
            let tables: Vec<&DataTable> = members.iter().map(|&i| &*documents[i].table).collect();
            filter_strip(ui, open, &tables, &mut out);
            switcher_strip(ui, open, &tables, &mut out);
            ui.add_space(4.0);
            grid(ui, open, documents, project, &mut rows, &mut out);
        }
        // Selections made in tiles go to their tables' own row states.
        for (table, filtered) in &rows {
            if before.get(table) != Some(&filtered.revision()) {
                if let Some(i) = self.table_index(project, table) {
                    let selected = filtered.selected_rows();
                    self.documents[i].rows.select(selected, SelectionMode::Replace);
                }
            }
        }
        if self.projects[p].dashboards[d].read_only {
            out.keep_view_changes();
        }
        let applied = out.edits.len();
        for edit in std::mem::take(&mut out.edits) {
            self.edit_dashboard(id, edit);
        }
        if applied > 0 && out.message.is_none() {
            self.message = None;
        }
        if let Some(index) = out.used_table {
            if index != self.active {
                self.set_target(index);
            }
        }
        for (index, column) in out.new_columns {
            self.add_columns(index, vec![column]);
        }
        for (index, request) in out.new_reports {
            self.add_report_to(index, request);
        }
        for (index, table, keep) in out.new_tables {
            if let Some(doc) = self.documents.get(index).map(|d| d.id) {
                self.open_report_table(doc, table, keep);
            }
        }
        if out.undo {
            self.dashboard_undo_redo(id, true);
        }
        if out.redo {
            self.dashboard_undo_redo(id, false);
        }
        if out.export {
            self.export_dashboard(id);
        }
        if out.publish {
            self.publish_dashboard(id);
        }
        if let Some(m) = out.message {
            self.info(m);
        }
    }

    /// Ask where to save dashboard `id` as one HTML page and write it: the
    /// tiles in the same grid (CSS grid), report sections in their tiles
    /// and plots as SVG.
    pub(super) fn export_dashboard(&mut self, id: u64) {
        let Some((p, d)) = self.find_dashboard(id) else { return };
        let rows = self.prepare_tiles(p, d);
        let project = self.projects[p].id;
        let project_name = self.project_name(project);
        let open = &self.projects[p].dashboards[d];
        let name = open.dashboard.name.clone();
        let Some(path) = file_io::pick_save_file("Export dashboard as HTML", "Web page", "html", &name) else {
            return;
        };
        let date = app_model::html_export::export_date();
        let exports: Vec<Option<(crate::report_export_ui::ReportForExport, String)>> = open
            .dashboard
            .tiles
            .iter()
            .map(|tile| {
                let TileContent::Report { table, .. } = &tile.content else { return None };
                let state = open.tiles.get(&tile.id)?;
                let doc = self.documents.iter().find(|doc| doc.project == project && &doc.table_id == table)?;
                Some((state.report.for_export(), tile_meta(open, doc, Some(state))))
            })
            .collect();
        let mut tiles = Vec::new();
        for (tile, export) in open.dashboard.tiles.iter().zip(&exports) {
            let content = match (&tile.content, export) {
                (TileContent::Text { text, style }, _) => {
                    app_model::html_export::HtmlTileContent::Text { text, heading: *style == TextStyle::Heading }
                }
                (TileContent::Report { table, .. }, Some((report, meta))) => {
                    let Some(doc) = self.documents.iter().find(|doc| doc.project == project && &doc.table_id == table)
                    else {
                        continue;
                    };
                    let Some(filtered) = rows.get(table) else { continue };
                    app_model::html_export::HtmlTileContent::Report {
                        report: report.page(),
                        source: app_model::html_export::HtmlSource {
                            table_name: doc.table.name(),
                            rows: filtered,
                            data: Some(&doc.table),
                            date: &date,
                        },
                        meta,
                    }
                }
                (TileContent::Report { .. }, None) => continue,
            };
            tiles.push(app_model::html_export::HtmlTile { area: tile.area, content });
        }
        let filters: Vec<String> = open.dashboard.filters.iter().map(FilterCondition::describe).collect();
        let switchers: Vec<String> =
            open.dashboard.switchers.iter().filter(|s| s.is_switched()).map(SharedSwitcher::describe).collect();
        let html = app_model::html_export::dashboard_html(
            app_model::html_export::HtmlDashboard {
                title: &name,
                project: &project_name,
                filters: &filters,
                switchers: &switchers,
                grid_columns: open.dashboard.grid_columns,
                date: &date,
            },
            &tiles,
        );
        let n = tiles.len();
        match std::fs::write(&path, html) {
            Ok(()) => {
                self.info(format!("Saved dashboard “{name}” ({}) as {}", count(n, "tile", "tiles"), path.display()))
            }
            Err(e) => self.error(format!("could not write {}: {e}", path.display())),
        }
    }
}

/// Where the built web viewer is: `ANOVACRAFT_WEB_VIEWER`, else a
/// `web-viewer` folder beside the program, one level up (a Cargo build's
/// `target/web-viewer`, written by `scripts/build-web-viewer.sh`) or in a
/// macOS bundle's Resources. A folder counts only when it holds every
/// viewer file.
pub(super) fn viewer_bundle() -> Option<std::path::PathBuf> {
    // Tests choose the bundle (none unless a test sets one).
    #[cfg(test)]
    {
        test_bundle::get()
    }
    #[cfg(not(test))]
    {
        installed_viewer_bundle()
    }
}

#[cfg_attr(test, allow(dead_code))]
fn installed_viewer_bundle() -> Option<std::path::PathBuf> {
    use app_model::web_publish::VIEWER_FILES;
    let complete = |dir: &std::path::Path| VIEWER_FILES.iter().all(|f| dir.join(f).is_file());
    if let Some(dir) = std::env::var_os("ANOVACRAFT_WEB_VIEWER").map(std::path::PathBuf::from) {
        return complete(&dir).then_some(dir);
    }
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    [dir.join("web-viewer"), dir.join("../web-viewer"), dir.join("../Resources/web-viewer")]
        .into_iter()
        .find(|d| complete(d))
}

/// Write the published folder for `file` (already in its published form)
/// titled `title` into `folder`: the project, the viewer copied from
/// `bundle`, and the page. Refuses a folder whose `index.html` AnovaCraft
/// did not write.
pub(super) fn write_published_folder(
    folder: &std::path::Path,
    title: &str,
    file: &ProjectFile,
    bundle: &std::path::Path,
) -> Result<(), String> {
    use app_model::web_publish::{NOTICES_FILE, PROJECT_FILE, VIEWER_FILES, index_html};
    let index = folder.join("index.html");
    if let Ok(existing) = std::fs::read_to_string(&index) {
        if !existing.contains(r#"<meta name="generator" content="AnovaCraft">"#) {
            return Err(format!(
                "{} already has an index.html that AnovaCraft did not write; choose an empty folder",
                folder.display()
            ));
        }
    }
    let write = |name: &str, bytes: &[u8]| {
        std::fs::write(folder.join(name), bytes)
            .map_err(|e| format!("could not write {}: {e}", folder.join(name).display()))
    };
    write(PROJECT_FILE, app_model::project::save_project_file(file).as_bytes())?;
    write(NOTICES_FILE, super::notices::THIRD_PARTY_NOTICES.as_bytes())?;
    for name in VIEWER_FILES {
        std::fs::copy(bundle.join(name), folder.join(name))
            .map_err(|e| format!("could not copy {} into {}: {e}", name, folder.display()))?;
    }
    // The page last, so a folder with a page always has what it loads.
    write("index.html", index_html(title).as_bytes())
}

impl StatsApp {
    /// Ask for a folder and publish dashboard `id` there as a web app (see
    /// [`app_model::web_publish`]). Without a built web viewer nothing is
    /// written: the "Build the web viewer" window opens instead.
    pub(super) fn publish_dashboard(&mut self, id: u64) {
        let Some((p, d)) = self.find_dashboard(id) else { return };
        let Some(bundle) = viewer_bundle() else {
            self.viewer_build = Some(super::viewer_build::ViewerBuild::new(id));
            return;
        };
        let project = self.projects[p].id;
        let dashboard = self.projects[p].dashboards[d].dashboard.clone();
        let Some(folder) = file_io::pick_folder("Publish dashboard as web app: choose a folder") else { return };
        let title = format!("{} · {}", dashboard.name, self.project_name(project));
        let file = app_model::web_publish::published_project(self.project_file(project, false), &dashboard.id);
        match write_published_folder(&folder, &title, &file, &bundle) {
            Ok(()) => self.info(format!(
                "Published “{}” to {} (index.html, project.acraft and the viewer). Put the folder on any web \
                 server to share it (to try it here, run python3 -m http.server in the folder and open \
                 http://localhost:8000)",
                dashboard.name,
                folder.display()
            )),
            Err(e) => self.error(e),
        }
    }
}

/// "Plants · 45 of 60 rows · not filtered by Fertilizer · Humidity → Week":
/// the tile's table, what the filters do to it and what the switchers
/// changed in its report (`state`).
fn tile_meta(open: &OpenDashboard, doc: &Document, state: Option<&TileState>) -> String {
    let mut parts = vec![doc.name().to_owned()];
    if let Some(c) = open.filters.get(&doc.table_id) {
        if c.filter.is_active() {
            parts.push(format!("{} of {}", c.filter.rows_kept(), count(c.filter.mask.len(), "row", "rows")));
        }
        if !c.filter.skipped.is_empty() {
            parts.push(format!("not filtered by {}", c.filter.skipped.join(", ")));
        }
    }
    if let Some(state) = state {
        parts.extend(state.applied.iter().map(|(from, to)| format!("{from} → {to}")));
        if !state.unswitched.is_empty() {
            parts.push(format!("not switched: {}", state.unswitched.join(", ")));
        }
    }
    parts.join(" · ")
}

/// The toolbar: name, Undo and Redo, Add tile, Export, New window and the
/// dashboard's menu.
#[allow(clippy::too_many_arguments)]
fn toolbar(
    ui: &mut egui::Ui,
    open: &mut OpenDashboard,
    documents: &[Document],
    members: &[usize],
    window: bool,
    project_name: &str,
    out: &mut DashboardOutcome,
    actions: &mut Vec<Action>,
) {
    let id = open.id;
    ui.horizontal_wrapped(|ui| {
        theme::dashboard_glyph(ui, ui.visuals().text_color());
        ui.label(RichText::new(&open.dashboard.name).strong().size(15.0 * theme::text_scale(ui)));
        ui.label(RichText::new(format!("Dashboard of {project_name}")).weak().small());
        ui.separator();
        let undo = open.history.next_undo().map(str::to_owned);
        let redo = open.history.next_redo().map(str::to_owned);
        let b = ui
            .add_enabled(undo.is_some(), egui::Button::new("Undo"))
            .on_hover_text(undo.as_deref().map_or("Nothing to undo".to_owned(), |l| format!("Undo: {l}")));
        if b.clicked() {
            out.undo = true;
        }
        let b = ui
            .add_enabled(redo.is_some(), egui::Button::new("Redo"))
            .on_hover_text(redo.as_deref().map_or("Nothing to redo".to_owned(), |l| format!("Redo: {l}")));
        if b.clicked() {
            out.redo = true;
        }
        if open.read_only {
            return;
        }
        ui.separator();
        crate::menus::menu_button(ui, "+ Add tile", |ui| {
            for &i in members {
                let doc = &documents[i];
                ui.menu_button(format!("From “{}”", doc.name()), |ui| {
                    ui.set_min_width(260.0);
                    if doc.reports.is_empty() {
                        ui.label(RichText::new("No reports yet: run an analysis on this table first").weak());
                    }
                    for report in &doc.reports {
                        let title = report.title();
                        if ui
                            .button(short(&title, 60))
                            .on_hover_text("A copy of this report, on this dashboard's filtered rows")
                            .clicked()
                        {
                            let (w, h) = REPORT_TILE;
                            let content =
                                TileContent::Report { table: doc.table_id.clone(), report: Box::new(report.saved()) };
                            out.edits.push(open.dashboard.add_tile_edit(content, w, h));
                            ui.close_menu();
                        }
                    }
                    ui.separator();
                    if ui
                        .button("New graph canvas")
                        .on_hover_text("An empty graph: drag columns onto its shelves")
                        .clicked()
                    {
                        let (w, h) = REPORT_TILE;
                        let content = TileContent::Report {
                            table: doc.table_id.clone(),
                            report: Box::new(ReportRequest::Graph(Box::default()).into()),
                        };
                        out.edits.push(open.dashboard.add_tile_edit(content, w, h));
                        ui.close_menu();
                    }
                });
            }
            ui.separator();
            if ui.button("Heading").on_hover_text("A line of large text across the dashboard").clicked() {
                let (w, h) = HEADING_TILE;
                let content = TileContent::Text { text: open.dashboard.name.clone(), style: TextStyle::Heading };
                out.edits.push(open.dashboard.add_tile_edit(content, w, h));
                ui.close_menu();
            }
            if ui.button("Note").on_hover_text("A paragraph of text; double-click it to edit").clicked() {
                let (w, h) = NOTE_TILE;
                let content =
                    TileContent::Text { text: "Double-click to write a note.".to_owned(), style: TextStyle::Body };
                out.edits.push(open.dashboard.add_tile_edit(content, w, h));
                ui.close_menu();
            }
        });
        if ui
            .button("Export as HTML…")
            .on_hover_text("Save the dashboard as one web page: the same grid, its reports' sections and plots")
            .clicked()
        {
            out.export = true;
        }
        if ui
            .button("Publish as web app…")
            .on_hover_text(
                "Write a folder that shows this dashboard in a web browser, still interactive: filters, switchers \
                 and selections recompute it there",
            )
            .clicked()
        {
            out.publish = true;
        }
        if !window
            && windows::new_window_button(ui).on_hover_text("Show this dashboard in a window of its own").clicked()
        {
            actions.push(Action::DashboardWindow(id));
        }
        crate::menus::menu_button(ui, "⋯", |ui| {
            ui.label("Name");
            let edit = ui.add(egui::TextEdit::singleline(&mut open.rename).desired_width(200.0));
            let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            let name = open.rename.trim().to_owned();
            let can = !name.is_empty() && name != open.dashboard.name;
            if (ui.add_enabled(can, egui::Button::new("Rename")).clicked() || (enter && can)) && can {
                out.edits.push(DashboardEdit::Rename { name });
                ui.close_menu();
            }
            ui.separator();
            if ui.button("Remove dashboard").on_hover_text("Take this dashboard out of its project").clicked() {
                actions.push(Action::RemoveDashboard(id));
                ui.close_menu();
            }
        })
        .response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Dashboard menu"));
    });
}

/// The filter strip: a chip per filter (click to change it), and "+ Add filter".
fn filter_strip(ui: &mut egui::Ui, open: &mut OpenDashboard, tables: &[&DataTable], out: &mut DashboardOutcome) {
    let columns = project_columns(tables);
    let pointer_down = ui.input(|i| i.pointer.any_down());
    // A changed filter is applied (one edit) once nothing is held down.
    if !pointer_down {
        if let Some(condition) = open.pending_filter.take() {
            if open.dashboard.filter(condition.column()) != Some(&condition) {
                out.edits.push(DashboardEdit::SetFilter { condition });
            }
        }
    }
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Filters").strong()).on_hover_text(
            "Each filter narrows every tile whose table has that column; the tables' own rows are not changed",
        );
        let mut remove = None;
        for f in open.dashboard.filters.clone() {
            let column = f.column().to_owned();
            let shown = open.pending_filter.clone().filter(|p| p.column() == column).unwrap_or_else(|| f.clone());
            let text = short(&shown.describe(), 44);
            let chip = crate::menus::menu_button(ui, format!("{text} ▾"), |ui| {
                ui.set_min_width(280.0);
                let Some((_, levels, range, _)) = columns.iter().find(|(n, ..)| *n == column) else {
                    ui.label(RichText::new("No table of this project has this column now.").weak());
                    return;
                };
                let mut edited = shown.clone();
                ui.label(RichText::new(&column).strong());
                if crate::report_view::condition_editor(ui, &mut edited, levels, *range) {
                    open.pending_filter = Some(edited);
                }
                ui.separator();
                if ui.button("Remove filter").clicked() {
                    remove = Some(column.clone());
                    ui.close_menu();
                }
            });
            chip.response
                .on_hover_text(format!("{}\nClick to change it", shown.describe()))
                .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Filter {column}")));
        }
        if let Some(column) = remove {
            open.pending_filter = None;
            out.edits.push(DashboardEdit::RemoveFilter { column });
        }
        crate::menus::menu_button(ui, "+ Add filter", |ui| {
            ui.set_min_width(200.0);
            egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                for (name, levels, range, continuous) in &columns {
                    let used = open.dashboard.filter(name).is_some();
                    let n = tables.iter().filter(|t| t.find(name).is_some()).count();
                    let tip = format!("In {} of the project's {}", n, count(tables.len(), "table", "tables"));
                    if ui.add_enabled(!used, egui::Button::new(name)).on_hover_text(tip).clicked() {
                        out.edits.push(DashboardEdit::SetFilter {
                            condition: open_filter(name, levels, *range, *continuous),
                        });
                        ui.close_menu();
                    }
                }
            });
        });
        if open.dashboard.filters.is_empty() {
            ui.label(
                RichText::new("None: add one on any column of this project's tables to narrow every tile that has it")
                    .weak()
                    .small(),
            );
        }
    });
}

/// The column names of the project's tables, in first-seen order, each with
/// its storage kind in the first table that has it.
fn project_column_kinds(tables: &[&DataTable]) -> Vec<(String, stat_data::ColumnKind)> {
    let mut out: Vec<(String, stat_data::ColumnKind)> = Vec::new();
    for t in tables {
        for c in t.columns() {
            if !out.iter().any(|(n, _)| *n == c.name) {
                out.push((c.name.clone(), c.kind()));
            }
        }
    }
    out
}

/// The switcher strip: a control per shared column picker (previous, the
/// list of choices, next, a menu of choices and ×), and "+ Add picker".
fn switcher_strip(ui: &mut egui::Ui, open: &mut OpenDashboard, tables: &[&DataTable], out: &mut DashboardOutcome) {
    let columns = project_column_kinds(tables);
    let id = open.id;
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Switchers").strong()).on_hover_text(
            "Each switcher shows another column in place of its own in every tile that reads it; \
             the tiles keep their own reports",
        );
        for s in open.dashboard.switchers.clone() {
            let column = s.column.clone();
            ui.label(RichText::new(format!("{column}:")).strong());
            let n = s.choices.len();
            let at = s.choices.iter().position(|c| *c == s.current).unwrap_or(0);
            let step = |k: isize| s.choices[(at as isize + k).rem_euclid(n.max(1) as isize) as usize].clone();
            let mut pick = None;
            let prev = ui.small_button("◀").on_hover_text("Show the previous column in the list");
            prev.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Previous {column}"))
            });
            if prev.clicked() && n > 1 {
                pick = Some(step(-1));
            }
            let combo = egui::ComboBox::from_id_salt(("dashboard_switcher", id, column.as_str()))
                .selected_text(s.current.as_str())
                .show_list(ui, |ui| {
                    for c in &s.choices {
                        if ui.selectable_label(*c == s.current, c.as_str()).clicked() {
                            pick = Some(c.clone());
                        }
                    }
                });
            combo.response.on_hover_text(format!("The column every tile reading {column} shows now")).widget_info(
                || egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, format!("Picker {column}")),
            );
            let next = ui.small_button("▶").on_hover_text("Show the next column in the list");
            next.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Next {column}")));
            if next.clicked() && n > 1 {
                pick = Some(step(1));
            }
            ui.label(RichText::new(format!("{} of {n}", at + 1)).weak().small());
            let kind = columns.iter().find(|(c, _)| *c == column).map(|(_, k)| *k);
            let mut choices = None;
            crate::menus::menu_button(ui, "choices…", |ui| {
                ui.set_min_width(200.0);
                egui::ScrollArea::vertical()
                    .id_salt(("dashboard_switcher_choices", id, column.as_str()))
                    .max_height(300.0)
                    .show(ui, |ui| {
                        let mut list = s.choices.clone();
                        for (name, _) in columns.iter().filter(|(_, k)| Some(*k) == kind) {
                            let mut on = list.contains(name);
                            // The switcher's own column and the one shown stay offered.
                            let fixed = *name == column || *name == s.current;
                            let b = ui.add_enabled(!fixed, egui::Checkbox::new(&mut on, name.as_str()));
                            b.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::WidgetType::Checkbox,
                                    !fixed,
                                    format!("Offer {name} for {column}"),
                                )
                            });
                            if b.changed() {
                                if on {
                                    list.push(name.clone());
                                } else {
                                    list.retain(|l| l != name);
                                }
                            }
                        }
                        // Keep the project's column order.
                        list.sort_by_key(|l| columns.iter().position(|(c, _)| c == l).unwrap_or(usize::MAX));
                        if list != s.choices {
                            choices = Some(list);
                        }
                    });
            });
            if let Some(list) = choices {
                out.edits.push(DashboardEdit::SetSwitcher { switcher: SharedSwitcher { choices: list, ..s.clone() } });
            }
            if let Some(c) = pick.filter(|c| *c != s.current) {
                out.edits.push(DashboardEdit::SetSwitcher { switcher: s.showing(c) });
            }
            let remove =
                ui.small_button("×").on_hover_text("Remove this picker; every tile shows its own column again");
            remove.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Remove picker {column}"))
            });
            if remove.clicked() {
                out.edits.push(DashboardEdit::RemoveSwitcher { column: column.clone() });
            }
            ui.separator();
        }
        crate::menus::menu_button(ui, "+ Add picker", |ui| {
            ui.set_min_width(200.0);
            egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                for (name, kind) in &columns {
                    let used = open.dashboard.switcher(name).is_some();
                    let tip = "Offers the project's other columns of the same type; change them under choices…";
                    let b = ui.add_enabled(!used, egui::Button::new(name)).on_hover_text(tip);
                    b.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::Button, !used, format!("Add picker on {name}"))
                    });
                    if b.clicked() {
                        let others = columns.iter().filter(|(c, k)| k == kind && c != name).map(|(c, _)| c.clone());
                        out.edits
                            .push(DashboardEdit::SetSwitcher { switcher: SharedSwitcher::new(name.clone(), others) });
                        ui.close_menu();
                    }
                }
            });
        });
        if open.dashboard.switchers.is_empty() {
            ui.label(
                RichText::new("None: add one to show another column in every tile that reads a column").weak().small(),
            );
        }
    });
}

/// The grid of tiles, with moving and resizing.
fn grid(
    ui: &mut egui::Ui,
    open: &mut OpenDashboard,
    documents: &[Document],
    project: u64,
    rows: &mut HashMap<TableId, RowStateManager>,
    out: &mut DashboardOutcome,
) {
    let id = open.id;
    let scale = theme::text_scale(ui);
    // No drag-to-scroll: dragging is for moving and resizing tiles (and selecting in their plots).
    let scroll = egui::ScrollArea::vertical().id_salt(("dashboard_scroll", id)).auto_shrink([false, false]);
    scroll.drag_to_scroll(false).show(ui, |ui| {
        let columns = open.dashboard.grid_columns.max(1);
        let width = ui.available_width().max(200.0);
        let column_width = ((width - GAP * (columns as f32 - 1.0)) / columns as f32).max(8.0);
        let row_height = ROW_HEIGHT * scale;
        let geometry = GridGeometry { origin: ui.cursor().min, column_width, row_height, columns };
        let last = open
            .dashboard
            .tiles
            .iter()
            .map(|t| t.area.bottom())
            .chain(open.gesture.map(|g| g.target.bottom()))
            .max()
            .unwrap_or(0) as u32;
        // Room below the last tile to move tiles into.
        let shown_rows = (last + 3).max(8);
        let height = shown_rows as f32 * (row_height + GAP);
        let (grid_rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
        let pal = theme::plot_palette(ui);
        if open.dashboard.tiles.is_empty() {
            let text = if open.read_only {
                "This dashboard is empty."
            } else {
                "This dashboard is empty. Choose + Add tile above, or Add to dashboard from a report's ⋯ Options menu."
            };
            ui.painter().text(
                grid_rect.center_top() + egui::vec2(0.0, 60.0),
                egui::Align2::CENTER_CENTER,
                text,
                egui::TextStyle::Body.resolve(ui.style()),
                ui.visuals().weak_text_color(),
            );
        }
        if open.gesture.is_some() {
            // Faint column guides while a tile is moved or resized.
            for c in 0..columns {
                let r = geometry.rect(GridArea::new(c, 0, 1, shown_rows));
                ui.painter().rect_filled(r, 4.0, pal.grid_vertical);
            }
        }
        let tiles: Vec<(TileId, GridArea, TileContent)> =
            open.dashboard.tiles.iter().map(|t| (t.id, t.area, t.content.clone())).collect();
        for (tile, area, content) in tiles {
            let rect = geometry.rect(area);
            tile_view(ui, open, documents, project, rows, tile, rect, &content, out, &geometry, area);
        }
        if let Some(g) = open.gesture {
            let ghost = geometry.rect(g.target);
            let fits = open.dashboard.check_area(g.target, Some(g.tile)).is_ok();
            let painter = ui
                .ctx()
                .layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new(("dashboard_ghost", id))));
            let painter = painter.with_clip_rect(ui.clip_rect());
            let (stroke, fill) =
                if fits { (theme::DIVIDER_ACTIVE, pal.brush_fill) } else { (theme::ERROR, egui::Color32::TRANSPARENT) };
            painter.rect(ghost, 8.0, fill, egui::Stroke::new(2.0_f32, stroke), egui::StrokeKind::Inside);
            let what = match (g.kind, fits) {
                (_, false) => "Overlaps another tile: it stays where it was".to_owned(),
                (GestureKind::Move, true) => format!("Column {}, row {}", g.target.column + 1, g.target.row + 1),
                (GestureKind::Resize, true) => format!("{} × {} cells", g.target.width, g.target.height),
            };
            painter.text(
                ghost.center_top() + egui::vec2(0.0, 14.0),
                egui::Align2::CENTER_CENTER,
                what,
                egui::TextStyle::Small.resolve(ui.style()),
                stroke,
            );
        }
    });
}

/// One tile: its strip (move handle, title, table and filter note, ×), its
/// body and its resize grip.
#[allow(clippy::too_many_arguments)]
fn tile_view(
    ui: &mut egui::Ui,
    open: &mut OpenDashboard,
    documents: &[Document],
    project: u64,
    rows: &mut HashMap<TableId, RowStateManager>,
    tile: TileId,
    rect: egui::Rect,
    content: &TileContent,
    out: &mut DashboardOutcome,
    geometry: &GridGeometry,
    area: GridArea,
) {
    let dash = open.id;
    let pal = theme::plot_palette(ui);
    let visuals = ui.visuals().clone();
    let heading = matches!(content, TileContent::Text { style: TextStyle::Heading, .. });
    let fill = if heading { egui::Color32::TRANSPARENT } else { pal.plot_bg };
    let border = if heading { egui::Stroke::NONE } else { egui::Stroke::new(1.0_f32, pal.container_border) };
    let moving = open.gesture.is_some_and(|g| g.tile == tile);
    let border = if moving { egui::Stroke::new(1.5_f32, theme::DIVIDER_ACTIVE) } else { border };
    ui.painter().rect(rect, 8.0, fill, border, egui::StrokeKind::Inside);
    let strip = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), STRIP));
    let title = tile_title(content, open.tiles.get(&tile));
    // The strip is the move handle (widgets drawn on it later take their own clicks).
    let read_only = open.read_only;
    let sense = if read_only { egui::Sense::hover() } else { egui::Sense::click_and_drag() };
    let mut handle = ui.interact(strip, egui::Id::new(("tile_strip", dash, tile)), sense);
    if !read_only {
        handle = handle.on_hover_cursor(egui::CursorIcon::Grab);
        handle.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, format!("Move tile {title}")));
    }
    if (handle.hovered() && !read_only) || moving {
        ui.painter().rect_filled(
            strip.shrink(1.0),
            egui::CornerRadius { nw: 8, ne: 8, sw: 0, se: 0 },
            visuals.faint_bg_color,
        );
    }
    if !read_only {
        paint_grip_dots(ui.painter(), egui::pos2(strip.left() + 10.0, strip.center().y), visuals.weak_text_color());
    }
    gesture_from(ui, open, &handle, (tile, area, rect), GestureKind::Move, geometry, out);
    let table_doc =
        content.table().and_then(|t| documents.iter().position(|d| d.project == project && &d.table_id == t));
    let mut strip_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(strip.shrink2(egui::vec2(6.0, 2.0)).with_min_x(strip.left() + 20.0))
            .layout(egui::Layout::left_to_right(egui::Align::Center))
            .id_salt(("tile_strip_ui", dash, tile)),
    );
    strip_ui.set_clip_rect(strip.intersect(ui.clip_rect()));
    strip_ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if read_only {
            // No tile buttons: only the table and filter note.
        } else {
            let remove = ui.small_button("×").on_hover_text("Remove this tile (Undo puts it back)");
            remove.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Remove tile {title}"))
            });
            if remove.clicked() {
                out.edits.push(DashboardEdit::RemoveTile { tile });
            }
            if let TileContent::Text { text, style } = content {
                let editing = open.editing.as_ref().is_some_and(|(t, _)| *t == tile);
                if editing {
                    if ui.small_button("Done").clicked() {
                        if let Some((_, new)) = open.editing.take() {
                            if new != *text {
                                out.edits.push(DashboardEdit::SetTileContent {
                                    tile,
                                    content: TileContent::Text { text: new, style: *style },
                                });
                            }
                        }
                    }
                    if ui.small_button("Cancel").clicked() {
                        open.editing = None;
                    }
                } else {
                    if ui.small_button("Edit").on_hover_text("Change the text (or double-click it)").clicked() {
                        open.editing = Some((tile, text.clone()));
                    }
                    let other = if *style == TextStyle::Heading { TextStyle::Body } else { TextStyle::Heading };
                    let label = if other == TextStyle::Heading { "As heading" } else { "As note" };
                    if ui.small_button(label).clicked() {
                        out.edits.push(DashboardEdit::SetTileContent {
                            tile,
                            content: TileContent::Text { text: text.clone(), style: other },
                        });
                    }
                }
            }
        }
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| match (content, table_doc) {
            (TileContent::Report { .. }, Some(i)) => {
                let state = open.tiles.get(&tile);
                let meta = tile_meta(open, &documents[i], state);
                let skipped =
                    open.filters.get(&documents[i].table_id).map(|c| c.filter.skipped.clone()).unwrap_or_default();
                let label = ui.add(egui::Label::new(RichText::new(meta).weak().small()).truncate());
                let mut tip = if skipped.is_empty() {
                    format!("A copy of a report of “{}”", documents[i].name())
                } else {
                    format!(
                        "“{}” has no column named {}, so {} not apply to this tile",
                        documents[i].name(),
                        skipped.iter().map(|s| format!("“{s}”")).collect::<Vec<_>>().join(" or "),
                        if skipped.len() == 1 { "that filter does" } else { "those filters do" }
                    )
                };
                for (from, to) in state.map(|s| s.applied.as_slice()).unwrap_or_default() {
                    tip.push_str(&format!(".\nThe {from} picker shows {to} in place of {from}"));
                }
                if let Some(state) = state.filter(|s| !s.unswitched.is_empty()) {
                    tip.push_str(&format!(
                        ".\nNot switched by {}: the table lacks the column chosen, or the report already uses it",
                        state.unswitched.join(", ")
                    ));
                }
                if !read_only {
                    tip.push_str(". Drag this strip to move the tile.");
                }
                label.on_hover_text(tip);
            }
            (TileContent::Report { table, .. }, None) => {
                ui.colored_label(theme::ERROR, format!("⚠ Table {table} is no longer in this project"));
            }
            (TileContent::Text { style, .. }, _) => {
                let kind = if *style == TextStyle::Heading { "Heading" } else { "Note" };
                ui.label(RichText::new(kind).weak().small());
            }
        });
    });
    // The body stops above the resize grip, so its scroll bars never cover it.
    let body =
        egui::Rect::from_min_max(egui::pos2(rect.left() + 6.0, strip.bottom()), rect.max - egui::vec2(6.0, GRIP));
    let mut body_ui =
        ui.new_child(egui::UiBuilder::new().max_rect(body).layout(egui::Layout::top_down(egui::Align::Min)).id_salt((
            "tile_body",
            dash,
            tile,
        )));
    body_ui.set_clip_rect(body.intersect(ui.clip_rect()));
    match content {
        TileContent::Report { table, .. } => {
            let (Some(i), Some(state), Some(filtered)) = (table_doc, open.tiles.get_mut(&tile), rows.get_mut(table))
            else {
                body_ui.label(RichText::new("This tile's table is not open in this project.").weak());
                return finish_tile(ui, open, tile, area, rect, geometry, out);
            };
            let doc = &documents[i];
            report_view::show_single_report(&mut body_ui, &mut state.report, &doc.table, filtered);
            // A press inside the tile makes its table the command target.
            let press = ui.input(|inp| if inp.pointer.any_pressed() { inp.pointer.press_origin() } else { None });
            if press.is_some_and(|p| rect.contains(p) && ui.ctx().layer_id_at(p) == Some(ui.layer_id())) {
                out.used_table = Some(i);
            }
            let report = &mut state.report;
            if std::mem::take(&mut report.remove_requested) {
                out.edits.push(DashboardEdit::RemoveTile { tile });
            }
            for c in std::mem::take(&mut report.new_columns) {
                out.new_columns.push((i, c));
            }
            for r in std::mem::take(&mut report.new_reports) {
                out.new_reports.push((i, r));
            }
            for (t, keep) in std::mem::take(&mut report.new_tables) {
                out.new_tables.push((i, t, keep));
            }
            report.edit_requested = false;
            report.window_requested = false;
            report.dashboard_requested = None;
            // Display options changed in the tile: the tile's own copy of the request changes,
            // on the switchers' own columns.
            let saved = report.saved();
            if saved != state.built_from {
                let stored = Dashboard::unswitched_report(&saved, &doc.table, &state.applied);
                state.built_from = saved;
                out.edits.push(DashboardEdit::SetTileContent {
                    tile,
                    content: TileContent::Report { table: table.clone(), report: Box::new(stored) },
                });
            }
        }
        TileContent::Text { text, style } => {
            if let Some((_, draft)) = open.editing.as_mut().filter(|(t, _)| *t == tile) {
                body_ui.add_sized(
                    body.size(),
                    egui::TextEdit::multiline(draft).desired_width(f32::INFINITY).hint_text("Text"),
                );
            } else {
                let scale = theme::text_scale(&body_ui);
                let text_widget = match style {
                    TextStyle::Heading => egui::Label::new(RichText::new(text).strong().size(22.0 * scale)),
                    TextStyle::Body => egui::Label::new(RichText::new(text)),
                };
                let response = body_ui.add(text_widget.wrap().sense(egui::Sense::click()));
                if response.double_clicked() && !read_only {
                    open.editing = Some((tile, text.clone()));
                }
            }
        }
    }
    finish_tile(ui, open, tile, area, rect, geometry, out);
}

/// The resize grip in a tile's corner.
fn finish_tile(
    ui: &mut egui::Ui,
    open: &mut OpenDashboard,
    tile: TileId,
    area: GridArea,
    rect: egui::Rect,
    geometry: &GridGeometry,
    out: &mut DashboardOutcome,
) {
    if open.read_only {
        return;
    }
    let grip = egui::Rect::from_min_max(rect.max - egui::vec2(GRIP, GRIP), rect.max);
    let title = open.dashboard.tile(tile).map_or_else(String::new, |t| tile_title(&t.content, open.tiles.get(&tile)));
    let response = ui
        .interact(grip, egui::Id::new(("tile_grip", open.id, tile)), egui::Sense::drag())
        .on_hover_cursor(egui::CursorIcon::ResizeNwSe)
        .on_hover_text("Drag to resize the tile");
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, format!("Resize tile {title}")));
    let color =
        if response.hovered() || response.dragged() { theme::DIVIDER_ACTIVE } else { ui.visuals().weak_text_color() };
    paint_resize_grip(ui.painter(), grip, color);
    gesture_from(ui, open, &response, (tile, area, rect), GestureKind::Resize, geometry, out);
}

/// Follow a drag of a tile's strip (`Move`) or grip (`Resize`): where it
/// would go while the button is held, and the edit when it is released.
fn gesture_from(
    ui: &egui::Ui,
    open: &mut OpenDashboard,
    response: &egui::Response,
    (tile, area, rect): (TileId, GridArea, egui::Rect),
    kind: GestureKind,
    geometry: &GridGeometry,
    out: &mut DashboardOutcome,
) {
    if response.drag_started() {
        let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(rect.min);
        let grab = match kind {
            GestureKind::Move => origin - rect.min,
            GestureKind::Resize => origin - rect.max,
        };
        open.gesture = Some(Gesture { tile, kind, grab, target: area });
    }
    let Some(mut g) = open.gesture.filter(|g| g.tile == tile && g.kind == kind) else { return };
    if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
        g.target = match kind {
            GestureKind::Move => {
                let (column, row) = geometry.cell_at(pos - g.grab);
                let column = column.min(geometry.columns.saturating_sub(area.width));
                GridArea { column, row, ..area }
            }
            GestureKind::Resize => {
                let (width, height) = geometry.span_to(area, pos - g.grab);
                GridArea { width, height, ..area }
            }
        };
        open.gesture = Some(g);
    }
    if response.drag_stopped() {
        open.gesture = None;
        if g.target != area && open.dashboard.check_area(g.target, Some(tile)).is_ok() {
            let edit = match kind {
                GestureKind::Move => DashboardEdit::MoveTile { tile, column: g.target.column, row: g.target.row },
                GestureKind::Resize => {
                    DashboardEdit::ResizeTile { tile, width: g.target.width, height: g.target.height }
                }
            };
            out.edits.push(edit);
        }
    }
}

/// Right-click items for dashboard `id` (sidebar row and tab).
fn dashboard_menu(ui: &mut egui::Ui, id: u64, actions: &mut Vec<Action>) {
    if ui.button("Show").clicked() {
        actions.push(Action::ShowDashboard(id));
        ui.close_menu();
    }
    if ui.button("Open in new window").on_hover_text("Show this dashboard in a window of its own").clicked() {
        actions.push(Action::DashboardWindow(id));
        ui.close_menu();
    }
    if ui.button("Export dashboard as HTML…").on_hover_text("One web page with the tiles on the same grid").clicked()
    {
        actions.push(Action::ExportDashboard(id));
        ui.close_menu();
    }
    ui.separator();
    if ui.button("Remove dashboard").on_hover_text("Take this dashboard out of its project").clicked() {
        actions.push(Action::RemoveDashboard(id));
        ui.close_menu();
    }
}

impl StatsApp {
    /// `true` if dashboard `id` has a window of its own.
    pub(super) fn in_window_dashboard(&self, id: u64) -> bool {
        self.windows.iter().any(|w| w.view == View::Dashboard(id))
    }

    /// Bring dashboard `id`'s window to the front.
    pub(super) fn raise_dashboard_window(&mut self, id: u64) {
        for w in self.windows.iter_mut().filter(|w| w.view == View::Dashboard(id)) {
            w.raise = true;
        }
    }

    /// Show dashboard `id` in a window of its own (or bring that window to
    /// the front); the main window goes back to its project's table.
    pub(super) fn pop_out_dashboard(&mut self, id: u64) {
        let Some(home) = self.dashboard_home(id).map(|i| self.documents[i].id) else { return };
        if self.in_window_dashboard(id) {
            return self.raise_dashboard_window(id);
        }
        self.windows.push(ViewWindow::new(home, View::Dashboard(id)));
        if self.shown_dashboard == Some(id) {
            self.shown_dashboard = None;
        }
    }

    /// Close dashboard `id`'s window and show it in the main window again.
    pub(super) fn bring_back_dashboard(&mut self, id: u64) {
        self.windows.retain(|w| w.view != View::Dashboard(id));
        if matches!(self.last_window, Some((_, View::Dashboard(d))) if d == id) {
            self.last_window = None;
        }
        self.show_dashboard(id);
    }

    /// Keep dashboard windows on a table of their project (the first one),
    /// and drop those whose dashboard has gone.
    pub(super) fn sync_dashboard_windows(&mut self) {
        let homes: Vec<Option<u64>> = self
            .windows
            .iter()
            .map(|w| match w.view {
                View::Dashboard(d) => self.dashboard_home(d).map(|i| self.documents[i].id),
                _ => Some(w.document),
            })
            .collect();
        let mut homes = homes.into_iter();
        self.windows.retain_mut(|w| match homes.next().flatten() {
            Some(home) => {
                w.document = home;
                true
            }
            None => false,
        });
        if self.shown_dashboard.is_some() && self.shown_dashboard().is_none() {
            self.shown_dashboard = None;
        }
    }

    /// The dashboard the menus' Undo and Redo act on: the one in the window
    /// used last, or the one the main window shows when it was used last.
    pub(super) fn command_dashboard(&self) -> Option<u64> {
        match self.last_window {
            Some((_, View::Dashboard(d))) => Some(d),
            Some(_) => None,
            None => self.shown_dashboard(),
        }
    }

    /// The sidebar row of project `id`'s dashboard number `k`, `depth` levels in.
    pub(super) fn dashboard_row(&self, ui: &mut egui::Ui, id: u64, k: usize, depth: usize, actions: &mut Vec<Action>) {
        let Some(d) = self.project(id).and_then(|p| p.dashboards.get(k)) else { return };
        let shown = self.shown_dashboard();
        ui.horizontal(|ui| {
            ui.add_space(12.0 * depth as f32);
            let is_shown = shown == Some(d.id);
            let marker = if is_shown { "▶" } else { "  " };
            let response = ui
                .selectable_label(is_shown, format!("{marker} {}", d.dashboard.name))
                .interact(egui::Sense::click_and_drag())
                .on_hover_text(format!(
                    "Dashboard of {} · {} · {}\nClick to show it; drag it onto a folder; right-click to open it in a \
                     window, export it or remove it",
                    self.project_name(id),
                    count(d.dashboard.tiles.len(), "tile", "tiles"),
                    count(d.dashboard.filters.len(), "filter", "filters"),
                ));
            if response.clicked() {
                actions.push(Action::ShowDashboard(d.id));
            }
            self.dashboard_drag(ui, &response, id, d.id, &d.dashboard.name);
            response.context_menu(|ui| {
                dashboard_menu(ui, d.id, actions);
                ui.separator();
                self.move_to_folder_menu(ui, id, super::folders::TreeItem::Dashboard(d.id), actions);
            });
            theme::dashboard_glyph(ui, ui.visuals().weak_text_color())
                .on_hover_text("A dashboard")
                .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, "Dashboard"));
            if self.in_window_dashboard(d.id) {
                theme::window_glyph(ui, ui.visuals().text_color())
                    .on_hover_text("In a window of its own. Click the dashboard to bring the window to the front.")
                    .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, "In its own window"));
            }
        });
    }

    /// The tabs of project `id`'s dashboards, after its tables' tabs.
    pub(super) fn dashboard_tabs(&self, ui: &mut egui::Ui, id: u64, folded: bool, actions: &mut Vec<Action>) {
        let Some(project) = self.project(id) else { return };
        let shown = self.shown_dashboard();
        for d in &project.dashboards {
            let selected = shown == Some(d.id);
            // A folded project still shows the tab on screen.
            if folded && !selected {
                continue;
            }
            let fill = if selected { ui.visuals().selection.bg_fill } else { ui.visuals().faint_bg_color };
            let stroke = if selected {
                egui::Stroke::new(1.5_f32, theme::DIVIDER_ACTIVE)
            } else {
                ui.visuals().widgets.noninteractive.bg_stroke
            };
            egui::Frame::new()
                .fill(fill)
                .stroke(stroke)
                .corner_radius(egui::CornerRadius { nw: 0, ne: 0, sw: 6, se: 6 })
                .inner_margin(egui::Margin::symmetric(8, 3))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        theme::dashboard_glyph(ui, ui.visuals().text_color());
                        let text = RichText::new(&d.dashboard.name);
                        let text = if selected { text.strong() } else { text };
                        let tab = ui.add(egui::Label::new(text).sense(egui::Sense::click()).truncate()).on_hover_text(
                            format!(
                                "Dashboard of {}; right-click for its window, HTML export and removal",
                                self.project_name(id)
                            ),
                        );
                        if tab.clicked() {
                            actions.push(Action::ShowDashboard(d.id));
                        }
                        tab.context_menu(|ui| dashboard_menu(ui, d.id, actions));
                        if self.in_window_dashboard(d.id) {
                            theme::window_glyph(ui, ui.visuals().text_color())
                                .on_hover_text("In a window of its own")
                                .widget_info(|| {
                                    egui::WidgetInfo::labeled(egui::WidgetType::Label, true, "In its own window")
                                });
                        }
                    });
                });
        }
    }

    /// The inside of dashboard `id`'s own window: "Back to main window" and
    /// the dashboard.
    pub(super) fn dashboard_window_contents(
        &mut self,
        ui: &mut egui::Ui,
        id: u64,
        back: &mut bool,
        actions: &mut Vec<Action>,
    ) {
        egui::TopBottomPanel::top(egui::Id::new(("dashboard_window_bar", id))).show_inside(ui, |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .button("Back to main window")
                    .on_hover_text("Close this window and show the dashboard in the main window again")
                    .clicked()
                {
                    *back = true;
                }
            });
        });
        egui::CentralPanel::default().show_inside(ui, |ui| {
            ui.push_id(("dashboard_window", id), |ui| self.dashboard_view(ui, id, true, actions));
        });
    }
}

/// The viewer bundle tests publish with (`None`: no viewer built).
#[cfg(test)]
pub(super) mod test_bundle {
    use std::cell::RefCell;
    use std::path::PathBuf;

    thread_local! {
        static BUNDLE: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
    }

    pub fn set(dir: Option<PathBuf>) {
        BUNDLE.with(|b| *b.borrow_mut() = dir);
    }

    pub(in crate::app) fn get() -> Option<PathBuf> {
        BUNDLE.with(|b| b.borrow().clone())
    }
}
