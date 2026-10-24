//! Open projects: one project file holds one or more tables.
//!
//! Every open table (a [`Document`]) belongs to an [`OpenProject`], which
//! holds what the file holds besides the tables: its path, its scripts and
//! whether it has unsaved changes. A project with one table looks and works
//! as a single dataset always has; a project with several is listed in the
//! sidebar as a tree (the project, its tables, and tables made from another
//! table indented under it) and its bottom tabs sit together after the
//! project's name.
//!
//! Tables made by Data ▸ Rows, Reshape, Merge or Summarize, augmented
//! designs and tables saved from reports join the project of the table they
//! came from unless the dialog's "Keep in this project" box is cleared. A
//! table made by a table operation keeps its lineage: the operation, and the
//! id and a fingerprint of each source in the project
//! ([`app_model::project::TableLineage`]). When a source's data no longer
//! matches its fingerprint, the derived table is marked (⚠ in the sidebar
//! and its tab, a banner above its grid) and offers Update, which runs the
//! operation again ([`ProjectFile::rerun_origin`]) and puts the result in
//! the table as one undoable edit. Nothing is updated without being asked.
//!
//! Saving writes the whole project ([`app_model::project::save_project_file`]),
//! with its layout: the table shown, each table's view and the windows of
//! their own with their places. A project of one table with nothing to
//! record in its layout is still written in the one-table form older
//! builds read.

use app_model::project::{
    DashboardWindow, DataFingerprint, PoppedOutWindow, ProjectLayout, TableView, ViewMode, WindowContent,
    save_project_file,
};
use app_model::settings::{RecentContents, RecentDetails};
use app_model::table_ops::{SubsetRows, TableOperation, unique_name, update_edit};
use stat_data::{Column, ColumnData};

use super::renaming::{self, Place, RenameTarget};
use super::windows::clamp_onto_screen;
use super::*;

/// An open project: what its file holds besides the tables (which are the
/// documents whose `project` is its id).
pub(super) struct OpenProject {
    /// Unique for the life of the application.
    pub id: u64,
    /// The project file it was opened from or last saved to.
    pub path: Option<PathBuf>,
    /// Name to show when it has no file yet (a workbook's file name).
    pub title: Option<String>,
    /// The name the user gave it (Rename), shown before its file's name
    /// and saved with it.
    pub name: Option<String>,
    /// A built-in sample: saving always writes a new project file (the
    /// sample itself is part of the program and is never overwritten).
    pub sample: bool,
    /// Scripts kept with the project.
    pub scripts: Vec<ProjectScript>,
    /// What the project was as of the last open or save (`None`: never
    /// saved), to detect unsaved changes.
    pub saved_state: Option<SavedState>,
    /// Its tables are folded away in the sidebar.
    pub collapsed: bool,
    /// Its tabs along the bottom edge are folded behind its header.
    pub tabs_folded: bool,
    /// Its dashboards, in tab order.
    pub dashboards: Vec<super::dashboards::OpenDashboard>,
    /// Free-text notes saved with the project (the sidebar's Notes).
    pub notes: String,
    /// Script notebooks kept with the project.
    pub notebooks: Vec<app_model::notebook::Notebook>,
    /// Folders the user made in its tree.
    pub folders: Vec<super::folders::Folder>,
}

/// What decides whether a project has unsaved changes (see
/// [`AnovaCraftApp::saved_state`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SavedState {
    /// Data revision and saved row-state revision of each table, in order.
    revisions: Vec<(u64, u64)>,
    /// Hash of everything else the file holds: names, reports, scripts,
    /// notes, notebooks, dashboards, folders, lineage and the tables'
    /// columns (without their values).
    rest: u64,
}

/// `table`'s name and columns (with their levels, formulas and labels)
/// without any rows: what a project's saved form says about a table besides
/// its values, which the table's revision stands for.
fn columns_only(table: &DataTable) -> DataTable {
    let columns = table
        .columns()
        .iter()
        .map(|c| Column {
            name: c.name.clone(),
            data: match &c.data {
                ColumnData::Numeric(_) => ColumnData::Numeric(Vec::new()),
                ColumnData::Text(_) => ColumnData::Text(Vec::new()),
                ColumnData::DateTime { display, .. } => ColumnData::DateTime { values: Vec::new(), display: *display },
            },
            level: c.level,
            formula: c.formula.clone(),
            value_labels: c.value_labels.clone(),
            hover: c.hover,
        })
        .collect();
    DataTable::new(table.name(), columns).unwrap_or_else(|_| table.clone())
}

/// How a new project starts.
#[derive(Default)]
pub(super) struct NewProject {
    pub path: Option<PathBuf>,
    pub title: Option<String>,
    /// The name the user gave it, read with the project.
    pub name: Option<String>,
    pub sample: bool,
    pub scripts: Vec<ProjectScript>,
    /// It has never been saved (a new table of results, a paste, an import
    /// from a database), so it counts as having unsaved changes.
    pub unsaved: bool,
    /// Dashboards read with the project.
    pub dashboards: Vec<app_model::Dashboard>,
    /// Notes read with the project.
    pub notes: String,
    /// Notebooks read with the project.
    pub notebooks: Vec<app_model::notebook::Notebook>,
}

/// One thing read from a file the user opened.
pub(super) enum ReadItem {
    /// A project of one or more tables.
    Project(Box<ProjectFile>),
    /// A plain table, with notes on how its values were read.
    Table(DataTable, Vec<String>),
}

/// Whether a derived table's sources changed since it was made, and what
/// that was worked out from (so it is only worked out again when a source
/// changes).
#[derive(Clone, Debug)]
pub(super) struct StaleCheck {
    /// Each source in the project: document id, data revision, row-state
    /// revision (when the operation reads row states) and the fingerprint
    /// recorded when the table was made.
    key: Vec<(u64, u64, u64, u64)>,
    /// Names of the sources edited since (empty: up to date).
    pub changed: Vec<String>,
    /// What Update re-runs ("summary", "subset", …).
    pub operation: &'static str,
}

/// The operation as a noun, for "Re-runs the …".
fn operation_noun(op: &TableOperation) -> &'static str {
    match op {
        TableOperation::Sort { .. } => "sort",
        TableOperation::RemoveDuplicates { .. } => "duplicate removal",
        TableOperation::Subset { sample: Some(_), .. } => "sample",
        TableOperation::Subset { .. } => "subset",
        TableOperation::CombineRows { .. } => "combination of rows",
        TableOperation::Join { .. } => "join",
        TableOperation::Stack { .. } => "stack",
        TableOperation::Unstack { .. } => "unstack",
        TableOperation::Transpose { .. } => "transpose",
        TableOperation::Summarize { .. } => "summary",
        TableOperation::MultipleImputation { .. } => "multiple imputation",
        TableOperation::DistanceMatrix { .. } => "distance matrix",
    }
}

/// Whether `op` reads the row states of its primary source (which rows are
/// excluded or selected), so a change of row states can make it stale.
fn reads_row_states(op: &TableOperation) -> bool {
    matches!(
        op,
        TableOperation::Summarize { include_excluded: false, .. }
            | TableOperation::Subset { rows: SubsetRows::Included | SubsetRows::Selected, .. }
            | TableOperation::MultipleImputation { .. }
            | TableOperation::DistanceMatrix { .. }
    )
}

fn view_mode(workspace: Workspace) -> ViewMode {
    match workspace {
        Workspace::Data => ViewMode::Data,
        Workspace::Results => ViewMode::Results,
        Workspace::SideBySide => ViewMode::SideBySide,
    }
}

fn workspace_of(mode: ViewMode) -> Workspace {
    match mode {
        ViewMode::Data => Workspace::Data,
        ViewMode::Results => Workspace::Results,
        ViewMode::SideBySide => Workspace::SideBySide,
    }
}

/// `table` under another name.
fn renamed(table: DataTable, name: &str) -> DataTable {
    if table.name() == name {
        return table;
    }
    DataTable::new(name, table.columns().to_vec()).unwrap_or(table)
}

/// The inside of the "Source changed" banner of derived table `doc` (by id).
pub(super) fn banner_contents(ui: &mut egui::Ui, doc: u64, check: &StaleCheck, actions: &mut Vec<Action>) {
    let (color, _) = theme::stale_colors(ui.visuals().dark_mode);
    let sources = check.changed.iter().map(|n| format!("“{n}”")).collect::<Vec<_>>().join(", ");
    let verb = if check.changed.len() == 1 { "was" } else { "were" };
    ui.horizontal_wrapped(|ui| {
        ui.label(
            RichText::new(format!("⚠ Source changed: {sources} {verb} edited after this table was made.")).color(color),
        );
        if ui
            .button("Update")
            .on_hover_text("Make this table again from the current data of its source, as one step that Undo reverses")
            .clicked()
        {
            actions.push(Action::UpdateFromSource(doc));
        }
        ui.label(
            RichText::new(format!("Re-runs the {}; Undo puts these values back.", check.operation)).weak().small(),
        );
    });
}

impl StatsApp {
    /// Open project `id`.
    pub(super) fn project(&self, id: u64) -> Option<&OpenProject> {
        self.projects.iter().find(|p| p.id == id)
    }

    /// Open project `id`, to change.
    pub(super) fn project_mut(&mut self, id: u64) -> Option<&mut OpenProject> {
        self.projects.iter_mut().find(|p| p.id == id)
    }

    /// The documents (indices, in order) of project `id`.
    pub(super) fn members(&self, id: u64) -> Vec<usize> {
        self.documents.iter().enumerate().filter(|(_, d)| d.project == id).map(|(i, _)| i).collect()
    }

    /// The name shown for project `id`: the name the user gave it, its
    /// file's name, the name it opened with, or else its first table's name.
    pub(super) fn project_name(&self, id: u64) -> String {
        let Some(project) = self.project(id) else { return String::new() };
        if let Some(name) = &project.name {
            return name.clone();
        }
        if let Some(path) = &project.path {
            return app_model::file_formats::name_from_path(path);
        }
        if let Some(title) = &project.title {
            return title.clone();
        }
        self.documents.iter().find(|d| d.project == id).map_or_else(|| "Untitled".to_owned(), |d| d.name().to_owned())
    }

    /// Give `target` the name `name` typed in place. A table's new name is
    /// an undoable edit; a project's is kept with it and saved in its file
    /// (a blank name goes back to the file's name). Unchanged names do nothing.
    pub(super) fn rename(&mut self, target: RenameTarget, name: &str) {
        let name = name.trim();
        match target {
            RenameTarget::Table(doc) => {
                let Some(index) = self.documents.iter().position(|d| d.id == doc) else { return };
                if name.is_empty() || self.documents[index].name() == name {
                    return;
                }
                match self.edit_document(index, Edit::RenameTable { name: name.to_owned() }) {
                    Ok(()) => self.info(format!("Renamed the table “{name}”. Undo puts the old name back.")),
                    Err(message) => self.error(message),
                }
            }
            RenameTarget::Project(id) => {
                if self.project_name(id) == name {
                    return;
                }
                if let Some(project) = self.project_mut(id) {
                    project.name = (!name.is_empty()).then(|| name.to_owned());
                }
                self.modified_marks.remove(&id);
            }
            RenameTarget::Folder(project, folder) => self.rename_folder(project, folder, name),
        }
    }

    /// The first table id not used in project `id`: `table-1`, `table-2`, ….
    pub(super) fn unused_table_id(&self, id: u64) -> TableId {
        (1..)
            .map(|i| TableId(format!("table-{i}")))
            .find(|t| !self.documents.iter().any(|d| d.project == id && &d.table_id == t))
            .expect("fewer tables than ids")
    }

    /// Add `doc` to project `project`, after its last table, and return
    /// its index (what the main window shows does not change).
    pub(super) fn add_to_project(&mut self, mut doc: Document, project: u64) -> usize {
        doc.project = project;
        doc.table_id = self.unused_table_id(project);
        self.register_document(&mut doc);
        let at = self.documents.iter().rposition(|d| d.project == project).map_or(self.documents.len(), |i| i + 1);
        self.documents.insert(at, doc);
        if self.documents.len() > 1 {
            if self.active >= at {
                self.active += 1;
            }
            if self.shown >= at {
                self.shown += 1;
            }
        }
        at
    }

    /// An untouched empty "Untitled" table (a project of one empty table
    /// with no file, rows, reports or edits), if one is open.
    pub(super) fn pristine_document(&self) -> Option<usize> {
        self.documents.iter().position(|d| {
            self.project(d.project)
                .is_some_and(|p| p.path.is_none() && !p.sample && p.title.is_none() && p.name.is_none())
                && self.members(d.project).len() == 1
                && d.source_path.is_none()
                && d.external.is_none()
                && d.table.n_rows() == 0
                && d.reports.is_empty()
                && !self.project_modified(d.project)
        })
    }

    /// What the start page's recent projects say about file `path`, just
    /// opened or saved as project `id`: a project file lists its tables and
    /// the first line of its notes; a data file gives its size (a workbook
    /// of several sheets lists them as tables).
    pub(super) fn recent_details(&self, id: u64, path: &std::path::Path) -> RecentDetails {
        let when = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
        let members = self.members(id);
        let is_project =
            path.extension().is_some_and(|e| e.eq_ignore_ascii_case(app_model::project::PROJECT_EXTENSION));
        let contents = match members.as_slice() {
            [one] if !is_project => {
                let table = &self.documents[*one].table;
                RecentContents::Data { rows: table.n_rows(), columns: table.n_cols() }
            }
            _ => RecentContents::Project {
                tables: members.iter().map(|&i| self.documents[i].name().to_owned()).collect(),
                notes: self
                    .project(id)
                    .map(|p| app_model::settings::first_line(&p.notes).to_owned())
                    .unwrap_or_default(),
            },
        };
        RecentDetails { when, contents }
    }

    /// Project `id` as it is written to its file, with its layout when
    /// `with_layout`.
    pub(super) fn project_file(&self, id: u64, with_layout: bool) -> ProjectFile {
        self.project_file_with(id, with_layout, Document::saved)
    }

    /// [`project_file`](Self::project_file) with each table written by
    /// `table`.
    fn project_file_with(
        &self,
        id: u64,
        with_layout: bool,
        table: impl Fn(&Document) -> ProjectTable,
    ) -> ProjectFile {
        ProjectFile {
            tables: self.documents.iter().filter(|d| d.project == id).map(table).collect(),
            scripts: self.project(id).map(|p| p.scripts.clone()).unwrap_or_default(),
            layout: if with_layout { self.layout(id) } else { ProjectLayout::default() },
            dashboards: self
                .project(id)
                .map(|p| p.dashboards.iter().map(|d| d.dashboard.clone()).collect())
                .unwrap_or_default(),
            notes: self.project(id).map(|p| p.notes.clone()).unwrap_or_default(),
            notebooks: self.project(id).map(|p| p.notebooks.clone()).unwrap_or_default(),
            name: self.project(id).and_then(|p| p.name.clone()),
            folders: self.folders_for_file(id),
        }
    }

    /// What decides whether project `id` has unsaved changes, cheaply:
    /// each table's data and row-state revisions (unique in the process, so
    /// equal numbers mean equal contents; the selection does not count) and
    /// a hash of the rest of its saved form, written with the tables'
    /// columns but none of their rows. Row selections and the layout are too
    /// transient to count (both are still written to the file).
    pub(super) fn saved_state(&self, id: u64) -> SavedState {
        let members: Vec<&Document> = self.documents.iter().filter(|d| d.project == id).collect();
        let revisions = members.iter().map(|d| (d.table.revision(), d.rows.saved_revision())).collect();
        let file = self.project_file_with(id, false, |d| ProjectTable {
            id: d.table_id.clone(),
            table: columns_only(&d.table),
            rows: RowStateManager::new(0),
            reports: d.reports.iter().map(Report::saved).collect(),
            external: d.external.clone(),
            lineage: d.lineage.clone(),
        });
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        std::hash::Hash::hash(&save_project_file(&file), &mut hasher);
        SavedState { revisions, rest: std::hash::Hasher::finish(&hasher) }
    }

    /// Whether project `id` differs from what was last opened or saved.
    pub(super) fn project_modified(&self, id: u64) -> bool {
        self.project(id).is_some_and(|p| p.saved_state.as_ref() != Some(&self.saved_state(id)))
    }

    /// Project `id` as it is now counts as saved.
    pub(super) fn mark_project_saved(&mut self, id: u64) {
        let state = self.saved_state(id);
        if let Some(p) = self.project_mut(id) {
            p.saved_state = Some(state);
        }
        self.modified_marks.remove(&id);
        self.recovery_baseline(id);
    }

    /// The lineage of a table made by `origin` from document `primary` (by
    /// id) and, for combining and joining, other tables named in `origin`:
    /// sources in project `project` are linked by id with their current
    /// fingerprints; others are left unlinked.
    pub(super) fn record_lineage(&self, origin: TableOrigin, primary: u64, project: u64) -> TableLineage {
        let docs: Vec<Option<&Document>> = origin
            .sources
            .iter()
            .enumerate()
            .map(|(k, name)| {
                let doc = if k == 0 {
                    self.documents.iter().find(|d| d.id == primary)
                } else {
                    self.documents.iter().find(|d| d.project == project && d.name() == name)
                };
                doc.filter(|d| d.project == project)
            })
            .collect();
        let sources: Vec<Option<(&TableId, Source<'_>)>> =
            docs.iter().map(|d| d.map(|d| (&d.table_id, Source { table: &d.table, rows: &d.rows }))).collect();
        TableLineage::record(origin.clone(), &sources).unwrap_or_else(|_| TableLineage::unlinked(origin))
    }

    /// The tables (indices) of document `doc`'s project made from it.
    pub(super) fn dependents(&self, doc: u64) -> Vec<usize> {
        let Some(source) = self.documents.iter().find(|d| d.id == doc) else { return Vec::new() };
        self.documents
            .iter()
            .enumerate()
            .filter(|(_, d)| d.project == source.project && d.id != doc)
            .filter(|(_, d)| {
                d.lineage.as_ref().is_some_and(|l| l.sources.iter().flatten().any(|s| s.table == source.table_id))
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// Forget links to table `table` of project `project` (it is leaving
    /// the project): tables made from it keep their data but can no longer
    /// be updated from it.
    pub(super) fn unlink_table(&mut self, project: u64, table: &TableId) {
        for doc in self.documents.iter_mut().filter(|d| d.project == project) {
            let lineages = doc.lineage.iter_mut().chain(&mut doc.lineage_undo).chain(&mut doc.lineage_redo);
            for lineage in lineages {
                for snap in &mut lineage.sources {
                    if snap.as_ref().is_some_and(|s| &s.table == table) {
                        *snap = None;
                    }
                }
            }
        }
    }

    /// The sources of `doc` in its project ("Made from …"), for the sidebar.
    pub(super) fn made_from(&self, doc: &Document) -> Option<String> {
        let lineage = doc.lineage.as_ref()?;
        let names: Vec<String> = lineage
            .sources
            .iter()
            .flatten()
            .filter_map(|s| self.documents.iter().find(|d| d.project == doc.project && d.table_id == s.table))
            .map(|d| d.name().to_owned())
            .collect();
        (!names.is_empty()).then(|| format!("Made from {}", names.join(", ")))
    }

    /// Work out again, for each derived table whose sources are in its
    /// project, which sources changed since it was made. Fingerprints are
    /// only recomputed when a source's data (or the row states the
    /// operation reads) changed since the last check.
    pub(super) fn refresh_staleness(&mut self) {
        let mut checks = HashMap::new();
        for doc in &self.documents {
            let Some(lineage) = &doc.lineage else { continue };
            let op = &lineage.origin.operation;
            let mut key = Vec::new();
            let mut sources = Vec::new();
            for (k, snap) in lineage.sources.iter().enumerate() {
                let Some(snap) = snap else { continue };
                let Some(source) = self.documents.iter().find(|d| d.project == doc.project && d.table_id == snap.table)
                else {
                    continue;
                };
                let rows = if k == 0 && reads_row_states(op) { source.rows.revision() } else { 0 };
                key.push((source.id, source.table.revision(), rows, snap.fingerprint.0));
                sources.push((k, snap, source));
            }
            if sources.is_empty() {
                continue;
            }
            let check = match self.stale.get(&doc.id) {
                Some(check) if check.key == key => check.clone(),
                _ => {
                    let changed = sources
                        .iter()
                        .filter(|(k, snap, source)| {
                            let now = Source { table: &source.table, rows: &source.rows };
                            DataFingerprint::of_source(op, now, *k == 0) != snap.fingerprint
                        })
                        .map(|(_, _, source)| source.name().to_owned())
                        .collect();
                    StaleCheck { key, changed, operation: operation_noun(op) }
                }
            };
            checks.insert(doc.id, check);
        }
        self.stale = checks;
    }

    /// Make derived table `id` again from its sources' current data and put
    /// the result in it as one undoable edit (Undo puts the old values, and
    /// the old lineage, back).
    pub(super) fn update_from_source(&mut self, id: u64) {
        let Some(index) = self.documents.iter().position(|d| d.id == id) else { return };
        let doc = &self.documents[index];
        let name = doc.name().to_owned();
        let file = self.project_file(doc.project, false);
        let rerun = match file.rerun_origin(&doc.table_id) {
            Ok(rerun) => rerun,
            Err(e) => return self.error(format!("Could not update “{name}”: {e}")),
        };
        let in_use: Vec<usize> =
            (0..doc.table.n_cols()).filter(|&j| doc.reports.iter().any(|r| r.uses_column(j, &doc.table))).collect();
        let edit = match update_edit(&doc.table, &rerun.document.table, &in_use) {
            Ok(edit) => edit,
            Err(e) => return self.error(format!("Could not update “{name}”: {e}")),
        };
        let sources = self.stale.get(&id).map(|c| c.changed.join("”, “")).unwrap_or_default();
        if let Err(message) = self.edit_document(index, edit) {
            return self.error(format!("Could not update “{name}”: {message}"));
        }
        let doc = &mut self.documents[index];
        // Rows that come from one source row carry its exclusion, hiding and colour.
        let fresh = &rerun.document.rows;
        if fresh.len() == doc.rows.len() {
            for r in 0..fresh.len() {
                let state = fresh.state(r);
                doc.rows.set_excluded([r], state.excluded);
                doc.rows.set_hidden([r], state.hidden);
                doc.rows.set_color([r], state.color);
            }
        }
        doc.lineage_undo.extend(doc.lineage.replace(rerun.lineage));
        let from = if sources.is_empty() { String::new() } else { format!(" from “{sources}”") };
        self.info(format!("Updated “{name}”{from}. Undo puts the previous values back."));
    }

    /// How project `id` is laid out now: the table shown (when it has
    /// several), views other than each table's default, and windows of its
    /// own with their places.
    pub(super) fn layout(&self, id: u64) -> ProjectLayout {
        let members = self.members(id);
        let mut layout = ProjectLayout::default();
        if members.len() > 1 {
            let current = [self.shown, self.active].into_iter().find(|i| members.contains(i));
            layout.active_table = current.map(|i| self.documents[i].table_id.clone());
        }
        for &i in &members {
            let doc = &self.documents[i];
            if doc.workspace != doc.default_workspace() {
                layout.views.push(TableView { table: doc.table_id.clone(), mode: view_mode(doc.workspace) });
            }
        }
        for w in &self.windows {
            let Some(doc) = members.iter().map(|&i| &self.documents[i]).find(|d| d.id == w.document) else {
                continue;
            };
            let content = match w.view {
                View::Data => WindowContent::Data,
                View::Results => WindowContent::Results,
                View::Report(r) => match doc.reports.iter().position(|x| x.id() == r) {
                    Some(k) => WindowContent::Report(k),
                    None => continue,
                },
                View::Dashboard(_) => continue,
            };
            layout.windows.push(PoppedOutWindow {
                table: doc.table_id.clone(),
                content,
                position: w.rect.map(|r| [r.min.x, r.min.y]),
                size: w.rect.map(|r| [r.width(), r.height()]),
            });
        }
        let dashboards = self.project(id).map_or(&[][..], |p| p.dashboards.as_slice());
        if let Some(shown) = self.shown_dashboard().and_then(|s| dashboards.iter().find(|d| d.id == s)) {
            layout.active_dashboard = Some(shown.dashboard.id.clone());
        }
        for w in &self.windows {
            let View::Dashboard(d) = w.view else { continue };
            let Some(open) = dashboards.iter().find(|x| x.id == d) else { continue };
            layout.dashboard_windows.push(DashboardWindow {
                dashboard: open.dashboard.id.clone(),
                position: w.rect.map(|r| [r.min.x, r.min.y]),
                size: w.rect.map(|r| [r.width(), r.height()]),
            });
        }
        layout
    }

    /// Put back project `id`'s saved layout: views, windows (placed onto
    /// the screen when they are drawn) and the table shown.
    pub(super) fn apply_layout(&mut self, id: u64, layout: &ProjectLayout) {
        let find =
            |docs: &[Document], table: &TableId| docs.iter().position(|d| d.project == id && &d.table_id == table);
        for v in &layout.views {
            if let Some(i) = find(&self.documents, &v.table) {
                self.documents[i].workspace = workspace_of(v.mode);
            }
        }
        for w in &layout.windows {
            let Some(i) = find(&self.documents, &w.table) else { continue };
            let doc = &self.documents[i];
            let view = match w.content {
                WindowContent::Data => View::Data,
                WindowContent::Results => View::Results,
                WindowContent::Report(k) => match doc.reports.get(k) {
                    Some(r) => View::Report(r.id()),
                    None => continue,
                },
            };
            if self.in_window(doc.id, view) {
                continue;
            }
            let mut window = ViewWindow::new(doc.id, view);
            window.raise = false;
            if w.position.is_some() || w.size.is_some() {
                let [x, y] = w.position.unwrap_or([80.0, 80.0]);
                let [width, height] = w.size.unwrap_or([780.0, 640.0]);
                window.place = Some(egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(width, height)));
            }
            self.windows.push(window);
        }
        if let Some(i) = layout.active_table.as_ref().and_then(|t| find(&self.documents, t)) {
            self.set_active(i);
        }
        let gui_id = |app: &Self, d: &app_model::DashboardId| {
            app.project(id).and_then(|p| p.dashboards.iter().find(|x| &x.dashboard.id == d)).map(|x| x.id)
        };
        for w in &layout.dashboard_windows {
            let (Some(d), Some(home)) = (gui_id(self, &w.dashboard), self.documents.iter().find(|x| x.project == id))
            else {
                continue;
            };
            if self.in_window_dashboard(d) {
                continue;
            }
            let mut window = ViewWindow::new(home.id, View::Dashboard(d));
            window.raise = false;
            if w.position.is_some() || w.size.is_some() {
                let [x, y] = w.position.unwrap_or([80.0, 80.0]);
                let [width, height] = w.size.unwrap_or([1100.0, 760.0]);
                window.place = Some(egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(width, height)));
            }
            self.windows.push(window);
        }
        if let Some(d) = layout.active_dashboard.as_ref().and_then(|d| gui_id(self, d)) {
            if !self.in_window_dashboard(d) {
                self.show_dashboard(d);
            }
        }
    }

    /// Where windows of a saved layout may go: the main window's screen.
    pub(super) fn screen_size(ctx: &egui::Context) -> egui::Vec2 {
        ctx.input(|i| i.viewport().monitor_size).filter(|s| s.x > 0.0 && s.y > 0.0).unwrap_or(ctx.screen_rect().size())
    }

    /// The place of a window from a saved layout, fitted onto the screen.
    pub(super) fn fitted_place(ctx: &egui::Context, place: Option<egui::Rect>) -> Option<egui::Rect> {
        place.map(|r| clamp_onto_screen(r, Self::screen_size(ctx)))
    }

    /// Open a table a report made (`source`: that report's document, by
    /// id): in the same project when `keep`, else as a new document.
    pub(super) fn open_report_table(&mut self, source: u64, table: DataTable, keep: bool) {
        let project = self.documents.iter().find(|d| d.id == source).map(|d| d.project);
        let Some(project) = project.filter(|_| keep) else { return self.open_table(table, None) };
        let name = unique_name(table.name(), self.documents.iter().map(Document::name));
        let doc = self.imported_document(renamed(table, &name), None);
        let index = self.add_to_project(doc, project);
        self.set_active(index);
        self.info(format!("Added “{name}” to “{}”. Save the project to keep it.", self.project_name(project)));
    }

    /// Run `op` on document `index` (a table a report asked for, such as a
    /// multiple imputation's stacked table) and keep the result in its
    /// project, linked to it so Update can remake it.
    pub(super) fn make_derived(&mut self, index: usize, op: &TableOperation) {
        let Some(doc) = self.documents.get(index) else { return };
        let name = unique_name(&op.default_name(&[]), self.documents.iter().map(Document::name));
        match op.run(app_model::table_ops::Source { table: &doc.table, rows: &doc.rows }, &[], &name) {
            Ok(derived) => {
                self.set_active(index);
                self.open_derived(derived, true);
            }
            Err(e) => self.error(format!("Could not make the table: {e}")),
        }
    }

    /// The order the sidebar lists some of a project's tables in
    /// (`members`, document indices), with each one's depth: a table made
    /// from another of them comes right after it, one level in.
    pub(super) fn tree_order_of(&self, members: &[usize]) -> Vec<(usize, usize)> {
        let members = members.to_vec();
        let parent = |i: usize| -> Option<usize> {
            let snap = self.documents[i].lineage.as_ref()?.sources.first()?.as_ref()?;
            members.iter().copied().find(|&j| j != i && self.documents[j].table_id == snap.table)
        };
        let mut order = Vec::new();
        let mut placed = vec![false; self.documents.len()];
        fn visit(
            i: usize,
            depth: usize,
            members: &[usize],
            parent: &dyn Fn(usize) -> Option<usize>,
            placed: &mut [bool],
            order: &mut Vec<(usize, usize)>,
        ) {
            if placed[i] {
                return;
            }
            placed[i] = true;
            order.push((i, depth));
            for &child in members {
                if parent(child) == Some(i) {
                    visit(child, depth + 1, members, parent, placed, order);
                }
            }
        }
        for &i in &members {
            if parent(i).is_none() {
                visit(i, 0, &members, &parent, &mut placed, &mut order);
            }
        }
        // Anything left (a loop of links, which a valid file cannot hold) at the top level.
        for &i in &members {
            visit(i, 0, &members, &parent, &mut placed, &mut order);
        }
        order
    }

    /// The open projects' ids, in the order of their first tables.
    pub(super) fn project_order(&self) -> Vec<u64> {
        let mut order = Vec::new();
        for d in &self.documents {
            if !order.contains(&d.project) {
                order.push(d.project);
            }
        }
        order
    }

    /// The sidebar's list of open projects: a project of one table is one
    /// row, as a dataset always was; a project of several is a heading (fold
    /// it with its triangle, right-click for its file actions) over its
    /// tables, each made table indented under its source.
    pub(super) fn project_list(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        for id in self.project_order() {
            self.prune_folders(id);
            let members = self.members(id);
            let n_dashboards = self.project(id).map_or(0, |p| p.dashboards.len());
            let plain = self.project(id).is_some_and(|p| p.folders.is_empty() && p.notebooks.is_empty());
            if members.len() == 1 && n_dashboards == 0 && plain {
                self.table_row(ui, members[0], 0, None, false, actions);
                self.reports_of(ui, members[0], 1, &HashMap::new(), actions);
                continue;
            }
            let collapsed = self.project(id).is_some_and(|p| p.collapsed);
            let edited = self.project_shows_modified(id);
            let name = self.project_name(id);
            let mut toggle = false;
            ui.horizontal(|ui| {
                let arrow = if collapsed { "▸" } else { "▾" };
                let fold = if collapsed { "Show this project's tables" } else { "Fold this project's tables away" };
                if ui.add(egui::Button::new(arrow).frame(false)).on_hover_text(fold).clicked() {
                    toggle = true;
                }
                let label = format!("{name}{}", if edited { "\u{a0}•" } else { "" });
                let dashboards = match n_dashboards {
                    0 => String::new(),
                    n => format!(" and {}", count(n, "dashboard", "dashboards")),
                };
                let tip = edited_tooltip(
                    format!(
                        "Project of {}{dashboards}{}\nDouble-click to rename; right-click to save, rename or close the whole project, or add a \
                         dashboard or folder",
                        count(members.len(), "table", "tables"),
                        self.project(id)
                            .and_then(|p| p.path.as_ref())
                            .map(|p| format!(" · {}", p.display()))
                            .unwrap_or_default()
                    ),
                    edited,
                );
                let target = RenameTarget::Project(id);
                if renaming::field(ui, target, Place::Sidebar, ui.available_width().min(220.0), actions) {
                    return;
                }
                let response = ui
                    .add(egui::Label::new(RichText::new(label).strong()).sense(egui::Sense::click()))
                    .on_hover_text(tip);
                if response.double_clicked() {
                    renaming::start(ui.ctx(), target, Place::Sidebar, &name);
                } else if response.clicked() {
                    toggle = true;
                }
                // Dropped on the project's name: out of every folder.
                super::folders::drop_target(ui, &response, id, None, actions);
                response.context_menu(|ui| project_menu(ui, members[0], id, (&name, Place::Sidebar), actions));
            });
            if toggle {
                if let Some(p) = self.project_mut(id) {
                    p.collapsed = !p.collapsed;
                }
            }
            if !collapsed {
                let several = (members.len() > 1).then_some(id);
                self.project_tree(ui, id, several, actions);
            }
        }
    }

    /// One table's row in the sidebar's list, `depth` levels in. `several`:
    /// its project's id when it holds other tables too; `in_tree`: it is
    /// listed under its project's heading (which shows the unsaved mark).
    pub(super) fn table_row(
        &self,
        ui: &mut egui::Ui,
        i: usize,
        depth: usize,
        several: Option<u64>,
        in_tree: bool,
        actions: &mut Vec<Action>,
    ) {
        let doc = &self.documents[i];
        let edited = !in_tree && self.shows_modified(doc);
        let shown = i == self.shown && self.shown_dashboard().is_none();
        let marker = if shown { "▶" } else { "  " };
        // A non-breaking space keeps the unsaved mark with the name's last word.
        let label = format!("{marker} {}{}", doc.name(), if edited { "\u{a0}•" } else { "" });
        let tip = edited_tooltip(
            format!("{}\nDouble-click to rename; right-click for save, export, rename and close", doc.tooltip(self.project(doc.project))),
            edited,
        );
        let scripts = self.project(doc.project).map_or(&[][..], |p| p.scripts.as_slice());
        ui.horizontal(|ui| {
            if depth > 0 {
                ui.add_space(12.0 * depth as f32);
            }
            self.reports_fold_button(ui, doc);
            let target = RenameTarget::Table(doc.id);
            if renaming::field(ui, target, Place::Sidebar, ui.available_width().min(220.0), actions) {
                return;
            }
            let response =
                ui.selectable_label(shown, label).interact(egui::Sense::click_and_drag()).on_hover_text(tip);
            if response.double_clicked() {
                renaming::start(ui.ctx(), target, Place::Sidebar, doc.name());
            } else if response.clicked() {
                actions.push(Action::Activate(i));
            }
            self.table_drag(ui, &response, doc);
            response.context_menu(|ui| {
                renaming::menu_item(ui, target, Place::Sidebar, doc.name(), "this table");
                document_context_menu(ui, i, scripts, several, actions);
                ui.separator();
                self.move_to_folder_menu(ui, doc.project, super::folders::TreeItem::Table(doc.id), actions);
            });
            self.stale_mark(ui, doc.id);
            self.window_mark(ui, doc);
        });
    }

    /// A ⚠ after a derived table's name (sidebar and tab) when a source
    /// changed since it was made.
    pub(super) fn stale_mark(&self, ui: &mut egui::Ui, id: u64) {
        let Some(check) = self.stale.get(&id).filter(|c| !c.changed.is_empty()) else { return };
        let (color, _) = theme::stale_colors(ui.visuals().dark_mode);
        let sources = check.changed.iter().map(|n| format!("“{n}”")).collect::<Vec<_>>().join(", ");
        ui.label(RichText::new("⚠").color(color))
            .on_hover_text(format!(
                "Source changed: {sources} edited after this table was made. Show the table and choose Update to \
                 make it again; nothing changes until you do."
            ))
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, "Source changed"));
    }

    /// Project `id`'s tabs along the bottom edge. A project of one table
    /// is just that table's tab; a project of several tables, or with
    /// dashboards, is a framed group headed by the project's name, which
    /// folds the group's tabs away (the shown one stays) and has the
    /// project's save and close actions on right-click.
    pub(super) fn project_tabs(&mut self, ui: &mut egui::Ui, id: u64, actions: &mut Vec<Action>) {
        let members = self.members(id);
        let Some(project) = self.project(id) else { return };
        let n_dashboards = project.dashboards.len();
        if members.len() == 1 && n_dashboards == 0 {
            return self.document_tab(ui, members[0], actions);
        }
        let folded = project.tabs_folded;
        let edited = self.project_shows_modified(id);
        let shown_dashboard = self.shown_dashboard();
        let holds_shown = project.dashboards.iter().any(|d| Some(d.id) == shown_dashboard)
            || (shown_dashboard.is_none() && members.contains(&self.shown));
        let mut toggle = false;
        let visuals = ui.visuals().clone();
        let accent = if holds_shown { theme::DIVIDER_ACTIVE } else { visuals.widgets.noninteractive.fg_stroke.color };
        egui::Frame::new()
            .fill(visuals.extreme_bg_color)
            .stroke(egui::Stroke::new(1.0_f32, visuals.widgets.noninteractive.bg_stroke.color))
            .corner_radius(egui::CornerRadius { nw: 0, ne: 0, sw: 8, se: 8 })
            .inner_margin(egui::Margin { left: 4, right: 4, top: 0, bottom: 3 })
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let arrow = if folded { "▸" } else { "▾" };
                    let name = self.project_name(id);
                    let target = RenameTarget::Project(id);
                    let editing = renaming::field(ui, target, Place::Tabs, 160.0, actions);
                    let mut text = format!("{arrow} {name}");
                    if edited {
                        text.push_str("\u{a0}•");
                    }
                    if folded {
                        let n = members.len() + n_dashboards;
                        text.push_str(&format!(" ({})", count(n, "tab", "tabs")));
                    }
                    if !editing {
                        let tip = format!(
                            "Project of {}{}\nClick to {} its tabs; double-click to rename; right-click to save, \
                             rename or close the project",
                            count(members.len(), "table", "tables"),
                            match n_dashboards {
                                0 => String::new(),
                                n => format!(" and {}", count(n, "dashboard", "dashboards")),
                            },
                            if folded { "show" } else { "fold away" }
                        );
                        let header = ui
                            .add(egui::Button::new(RichText::new(text).strong().color(accent)).frame(false))
                            .on_hover_text(edited_tooltip(tip, edited));
                        if header.double_clicked() {
                            renaming::start(ui.ctx(), target, Place::Tabs, &name);
                        } else if header.clicked() {
                            toggle = true;
                        }
                        header.context_menu(|ui| project_menu(ui, members[0], id, (&name, Place::Tabs), actions));
                    }
                    for &i in &members {
                        if !folded || (shown_dashboard.is_none() && i == self.shown) {
                            self.document_tab(ui, i, actions);
                        }
                    }
                    self.dashboard_tabs(ui, id, folded, actions);
                });
            });
        if toggle {
            if let Some(p) = self.project_mut(id) {
                p.tabs_folded = !p.tabs_folded;
            }
        }
    }

    /// Document `i`'s tab along the bottom edge.
    pub(super) fn document_tab(&self, ui: &mut egui::Ui, i: usize, actions: &mut Vec<Action>) {
        let doc = &self.documents[i];
        let members = self.members(doc.project);
        let several = (members.len() > 1).then_some(doc.project);
        let has_dashboards = self.project(doc.project).is_some_and(|p| !p.dashboards.is_empty());
        let grouped = (members.len() > 1 || has_dashboards).then_some(doc.project);
        let selected = i == self.shown && self.shown_dashboard().is_none();
        let fill = if selected { ui.visuals().selection.bg_fill } else { ui.visuals().faint_bg_color };
        let stroke = if selected {
            egui::Stroke::new(1.5_f32, theme::DIVIDER_ACTIVE)
        } else {
            ui.visuals().widgets.noninteractive.bg_stroke
        };
        let scripts = self.project(doc.project).map_or(&[][..], |p| p.scripts.as_slice());
        egui::Frame::new()
            .fill(fill)
            .stroke(stroke)
            .corner_radius(egui::CornerRadius { nw: 0, ne: 0, sw: 6, se: 6 })
            .inner_margin(egui::Margin::symmetric(8, 3))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let edited = grouped.is_none() && self.shows_modified(doc);
                    let name = if edited { format!("{} •", doc.name()) } else { doc.name().to_owned() };
                    let text = RichText::new(name);
                    let text = if selected { text.strong() } else { text };
                    let target = RenameTarget::Table(doc.id);
                    if !renaming::field(ui, target, Place::Tabs, 140.0, actions) {
                        let tab = ui
                            .add(egui::Label::new(text).sense(egui::Sense::click()).truncate())
                            .on_hover_text(edited_tooltip(doc.tooltip(self.project(doc.project)), edited));
                        if tab.double_clicked() {
                            renaming::start(ui.ctx(), target, Place::Tabs, doc.name());
                        } else if tab.clicked() {
                            actions.push(Action::Activate(i));
                        }
                        if tab.middle_clicked() {
                            actions.push(Action::Close(i));
                        }
                        tab.context_menu(|ui| {
                            renaming::menu_item(ui, target, Place::Tabs, doc.name(), "this table");
                            document_context_menu(ui, i, scripts, several, actions);
                        });
                    }
                    self.stale_mark(ui, doc.id);
                    self.window_mark(ui, doc);
                    let close =
                        if several.is_some() { "Remove this table from the project" } else { "Close this dataset" };
                    if ui.small_button("×").on_hover_text(close).clicked() {
                        actions.push(Action::Close(i));
                    }
                });
            });
    }

    /// The "Source changed" banner above the shown table's data, when its
    /// sources changed since it was made.
    pub(super) fn source_banner(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let Some(doc) = self.documents.get(self.shown) else { return };
        let Some(check) = self.stale.get(&doc.id).filter(|c| !c.changed.is_empty()).cloned() else { return };
        let id = doc.id;
        let (_, fill) = theme::stale_colors(ctx.style().visuals.dark_mode);
        let frame = egui::Frame::side_top_panel(&ctx.style()).fill(fill).inner_margin(egui::Margin::symmetric(8, 4));
        egui::TopBottomPanel::top("source_banner").frame(frame).show(ctx, |ui| {
            banner_contents(ui, id, &check, actions);
        });
    }

    /// The question before removing a table that other tables of its
    /// project were made from.
    pub(super) fn remove_confirmation(&mut self, ctx: &egui::Context) {
        let Some(id) = self.confirm_remove else { return };
        let Some(index) = self.documents.iter().position(|d| d.id == id) else {
            self.confirm_remove = None;
            return;
        };
        let doc = &self.documents[index];
        let name = doc.name().to_owned();
        let project = self.project_name(doc.project);
        let made: Vec<String> =
            self.dependents(id).iter().map(|&i| format!("“{}”", self.documents[i].name())).collect();
        let emptied: Vec<String> = self.dashboards_emptied_by(index).iter().map(|d| format!("“{d}”")).collect();
        let mut choice = None;
        egui::Window::new("Remove table from project?")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                if !made.is_empty() {
                    let (tables, were, keep) = if made.len() == 1 {
                        ("Another table", "was", "keeps its")
                    } else {
                        ("Other tables", "were", "keep their")
                    };
                    ui.label(RichText::new(format!("{tables} of “{project}” {were} made from “{name}”.")).strong());
                    ui.label(format!("{} {keep} data but can no longer be updated from it.", made.join(", ")));
                }
                if !emptied.is_empty() {
                    let (dashboards, shows) =
                        if emptied.len() == 1 { ("Dashboard", "shows") } else { ("Dashboards", "show") };
                    ui.label(
                        RichText::new(format!(
                            "{dashboards} {} {shows} only “{name}”, so removing it leaves {} empty.",
                            emptied.join(", "),
                            if emptied.len() == 1 { "it" } else { "them" }
                        ))
                        .strong(),
                    );
                    ui.label("Their tiles, filters and layout are lost unless the project is saved without them.");
                }
                ui.label(
                    RichText::new("Save the project to keep the change; the file is not changed until then.")
                        .weak()
                        .small(),
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Remove table").clicked() {
                        choice = Some(true);
                    }
                    if ui.button("Cancel").clicked() {
                        choice = Some(false);
                    }
                });
            });
        let Some(remove) = choice else { return };
        self.confirm_remove = None;
        if remove {
            self.remove_table(index);
        }
    }
}

impl StatsApp {
    /// The box saying why columns asked to be deleted were kept, with a
    /// way to the reports that use them.
    pub(super) fn column_refusal_box(&mut self, ctx: &egui::Context) {
        let Some(message) = self.column_refusal.clone() else { return };
        let mut close = false;
        let mut show = false;
        egui::Window::new("Column not deleted")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.set_max_width(420.0);
                ui.label(message);
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Show the reports").clicked() {
                        show = true;
                    }
                    if ui.button("OK").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        close = true;
                    }
                });
            });
        if show {
            self.reveal(self.active, View::Results);
        }
        if show || close {
            self.column_refusal = None;
        }
    }
}

/// Right-click items for project `id` as a whole (`first`: its first table).
fn project_menu(ui: &mut egui::Ui, first: usize, id: u64, (name, place): (&str, Place), actions: &mut Vec<Action>) {
    renaming::menu_item(ui, RenameTarget::Project(id), place, name, "this project");
    ui.separator();
    if ui.button("Save project").on_hover_text("Every table, with its reports and the window layout").clicked() {
        actions.push(Action::Save(first));
        ui.close_menu();
    }
    if ui.button("Save project as…").clicked() {
        actions.push(Action::SaveAs(first));
        ui.close_menu();
    }
    ui.separator();
    if ui.button("New dashboard").on_hover_text("Reports and notes from this project's tables on one grid").clicked() {
        actions.push(Action::NewDashboard(id));
        ui.close_menu();
    }
    if ui
        .button("New folder")
        .on_hover_text("A folder in this project's tree; drag tables, reports and dashboards onto it")
        .clicked()
    {
        actions.push(Action::Folder(id, super::folders::FolderOp::New { parent: None, with: None }));
        ui.close_menu();
    }
    ui.separator();
    if ui.button("Close project").on_hover_text("Close every table of this project").clicked() {
        actions.push(Action::CloseProject(id));
        ui.close_menu();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_from_a_bigger_desk_come_back_onto_the_screen() {
        let screen = egui::vec2(1440.0, 900.0);
        // On a second monitor to the right.
        let r =
            clamp_onto_screen(egui::Rect::from_min_size(egui::pos2(2200.0, 300.0), egui::vec2(800.0, 600.0)), screen);
        assert_eq!(r, egui::Rect::from_min_size(egui::pos2(640.0, 300.0), egui::vec2(800.0, 600.0)));
        // On a monitor to the left (negative coordinates), taller than this screen.
        let r =
            clamp_onto_screen(egui::Rect::from_min_size(egui::pos2(-1500.0, -40.0), egui::vec2(900.0, 1400.0)), screen);
        assert_eq!(r, egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(900.0, 900.0)));
        // Already on screen: unchanged.
        let inside = egui::Rect::from_min_size(egui::pos2(100.0, 50.0), egui::vec2(700.0, 500.0));
        assert_eq!(clamp_onto_screen(inside, screen), inside);
    }

    #[test]
    fn view_modes_map_both_ways() {
        for w in [Workspace::Data, Workspace::Results, Workspace::SideBySide] {
            assert_eq!(workspace_of(view_mode(w)), w);
        }
    }
}
