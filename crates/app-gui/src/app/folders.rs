//! Folders in a project's tree in the sidebar.
//!
//! A project's tree lists its tables (each with its reports and graph
//! canvases under it), dashboards and notebooks. The user can add folders
//! (right-click ▸ New folder, or Move to folder ▸ New folder on an item),
//! rename them in place, nest them, and move items into them by dragging an
//! item onto a folder (or onto the project's name to take it out of every
//! folder) or with right-click ▸ Move to folder. A folder only groups: a
//! report in a folder still belongs to its table.
//!
//! Clicking a folder's name brings the windows of everything in it to the
//! front; when none of it is in a window, the main window shows its first
//! table or dashboard. Right-click ▸ Open each in a window gives every item
//! a window of its own.
//!
//! Folders are saved with the project ([`app_model::project::ProjectFolder`]):
//! here items are held by their ids for this session (documents, reports and
//! dashboards keep their ids however they move), and turned into the file's
//! table ids and report positions when the project is saved.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use app_model::project::{FolderItem, ProjectFolder};

use super::renaming::{self, Place, RenameTarget};
use super::*;
use crate::icons::{self, Icon};

/// Ids of folders, unique for the life of the application.
static NEXT_FOLDER: AtomicU64 = AtomicU64::new(1);

fn next_folder_id() -> u64 {
    NEXT_FOLDER.fetch_add(1, Ordering::Relaxed)
}

/// Something listed in a project's tree that a folder can hold.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) enum TreeItem {
    /// A table (by document id).
    Table(u64),
    /// A report or graph canvas (by document and report id).
    Report { doc: u64, report: u64 },
    /// A dashboard (by its id in this session).
    Dashboard(u64),
    /// A notebook, by name.
    Notebook(String),
}

/// A folder of an open project.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Folder {
    /// Unique for the life of the application.
    pub id: u64,
    pub name: String,
    /// Folders inside it.
    pub folders: Vec<Folder>,
    /// What it holds.
    pub items: Vec<TreeItem>,
    /// Its contents are folded away in the tree.
    pub collapsed: bool,
}

impl Folder {
    pub fn new(name: impl Into<String>) -> Self {
        Self { id: next_folder_id(), name: name.into(), folders: Vec::new(), items: Vec::new(), collapsed: false }
    }

    /// Whether folder `id` is this one or inside it.
    fn holds_folder(&self, id: u64) -> bool {
        self.id == id || self.folders.iter().any(|f| f.holds_folder(id))
    }

    /// Every item in it and in the folders inside it, in order.
    pub fn all_items(&self) -> Vec<TreeItem> {
        let mut out = self.items.clone();
        for f in &self.folders {
            out.extend(f.all_items());
        }
        out
    }
}

/// What is dragged in the tree.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Dragged {
    Item(TreeItem),
    Folder(u64),
}

/// The payload of a drag in the tree: what, and in which project.
#[derive(Clone, Debug)]
pub(super) struct TreeDrag {
    project: u64,
    what: Dragged,
    /// Shown beside the pointer while dragging.
    label: String,
}

/// A change to a project's folders, asked for in the tree.
#[derive(Clone, Debug)]
pub(super) enum FolderOp {
    /// Add a folder inside `parent` (top level: `None`), holding `with`,
    /// and start renaming it.
    New { parent: Option<u64>, with: Option<Dragged> },
    /// Put an item or folder into folder `to` (`None`: out of every folder).
    Move { what: Dragged, to: Option<u64> },
    /// Remove a folder; what it holds moves to the folder it was in.
    Delete(u64),
    /// Fold or unfold a folder.
    Fold(u64),
    /// Bring the windows of what it holds to the front (or show its first table).
    Show(u64),
    /// Give everything it holds a window of its own.
    OpenEach(u64),
}

/// The folder with id `id` among `folders` (at any depth).
pub(super) fn find(folders: &[Folder], id: u64) -> Option<&Folder> {
    folders.iter().find_map(|f| if f.id == id { Some(f) } else { find(&f.folders, id) })
}

fn find_mut(folders: &mut [Folder], id: u64) -> Option<&mut Folder> {
    for f in folders {
        if f.id == id {
            return Some(f);
        }
        if let Some(found) = find_mut(&mut f.folders, id) {
            return Some(found);
        }
    }
    None
}

/// Where each item of `folders` is: item → folder id.
pub(super) fn placement(folders: &[Folder]) -> HashMap<TreeItem, u64> {
    fn walk(folders: &[Folder], out: &mut HashMap<TreeItem, u64>) {
        for f in folders {
            for item in &f.items {
                out.entry(item.clone()).or_insert(f.id);
            }
            walk(&f.folders, out);
        }
    }
    let mut out = HashMap::new();
    walk(folders, &mut out);
    out
}

/// Take `item` out of every folder.
fn remove_item(folders: &mut [Folder], item: &TreeItem) {
    for f in folders {
        f.items.retain(|i| i != item);
        remove_item(&mut f.folders, item);
    }
}

/// Take folder `id` out of the tree.
fn take_folder(folders: &mut Vec<Folder>, id: u64) -> Option<Folder> {
    if let Some(i) = folders.iter().position(|f| f.id == id) {
        return Some(folders.remove(i));
    }
    folders.iter_mut().find_map(|f| take_folder(&mut f.folders, id))
}

/// The id of the folder holding folder `id` (`None`: top level or unknown).
fn parent_of(folders: &[Folder], id: u64) -> Option<u64> {
    folders.iter().find_map(
        |f| {
            if f.folders.iter().any(|g| g.id == id) { Some(f.id) } else { parent_of(&f.folders, id) }
        },
    )
}

/// Carry out a move. Returns `false` (nothing changed) when `to` is not a
/// folder of the tree, or a folder would go into itself or a folder inside it.
pub(super) fn apply_move(folders: &mut Vec<Folder>, what: &Dragged, to: Option<u64>) -> bool {
    if to.is_some_and(|t| find(folders, t).is_none()) {
        return false;
    }
    match what {
        Dragged::Item(item) => {
            remove_item(folders, item);
            if let Some(f) = to.and_then(|t| find_mut(folders, t)) {
                f.items.push(item.clone());
            }
            true
        }
        Dragged::Folder(id) => {
            let Some(moving) = find(folders, *id) else { return false };
            if to.is_some_and(|t| moving.holds_folder(t)) || parent_of(folders, *id) == to && to.is_some() {
                return false;
            }
            if to.is_none() && folders.iter().any(|f| f.id == *id) {
                return false;
            }
            let Some(moving) = take_folder(folders, *id) else { return false };
            match to.and_then(|t| find_mut(folders, t)) {
                Some(f) => f.folders.push(moving),
                None => folders.push(moving),
            }
            true
        }
    }
}

/// Remove folder `id`, moving what it holds (items and folders) into the
/// folder it was in, or to the top level.
pub(super) fn delete_folder(folders: &mut Vec<Folder>, id: u64) -> Option<Folder> {
    let parent = parent_of(folders, id);
    let gone = take_folder(folders, id)?;
    match parent.and_then(|p| find_mut(folders, p)) {
        Some(p) => {
            p.items.extend(gone.items.iter().cloned());
            p.folders.extend(gone.folders.iter().cloned());
        }
        None => folders.extend(gone.folders.iter().cloned()),
    }
    Some(gone)
}

/// Drop items `keep` says no longer exist (a closed report or table).
pub(super) fn prune(folders: &mut [Folder], keep: &dyn Fn(&TreeItem) -> bool) {
    for f in folders {
        f.items.retain(|i| keep(i));
        prune(&mut f.folders, keep);
    }
}

/// `name`, or `name 2`, `name 3`, … so it differs from every name in `taken`.
fn unused_name(name: &str, taken: &[String]) -> String {
    if !taken.iter().any(|t| t == name) {
        return name.to_owned();
    }
    (2..).map(|i| format!("{name} {i}")).find(|n| !taken.iter().any(|t| t == n)).expect("a free name")
}

fn all_names(folders: &[Folder], out: &mut Vec<String>) {
    for f in folders {
        out.push(f.name.clone());
        all_names(&f.folders, out);
    }
}

/// Make `response` (a row of the tree) a drag source for `drag`, with a
/// label following the pointer while it is dragged.
fn drag_source(ui: &egui::Ui, response: &egui::Response, drag: TreeDrag) {
    if response.drag_started() || response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        if let Some(pos) = ui.ctx().pointer_interact_pos() {
            egui::Area::new(response.id.with("tree_drag"))
                .order(egui::Order::Tooltip)
                .fixed_pos(pos + egui::vec2(14.0, 6.0))
                .interactable(false)
                .show(ui.ctx(), |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| ui.label(&drag.label));
                });
        }
        response.dnd_set_drag_payload(drag);
    }
}

/// Make `response` a drop target of project `project`'s tree that puts what
/// is dropped into folder `to` (`None`: out of every folder). Draws a frame
/// around it while something of the project is dragged over it.
pub(super) fn drop_target(
    ui: &egui::Ui,
    response: &egui::Response,
    project: u64,
    to: Option<u64>,
    actions: &mut Vec<Action>,
) {
    let fits = |p: &TreeDrag| p.project == project && p.what != Dragged::Folder(to.unwrap_or(u64::MAX));
    if response.dnd_hover_payload::<TreeDrag>().is_some_and(|p| fits(&p)) {
        let stroke = egui::Stroke::new(1.5_f32, theme::DIVIDER_ACTIVE);
        ui.painter().rect_stroke(response.rect.expand(2.0), 3.0, stroke, egui::StrokeKind::Outside);
    }
    if let Some(payload) = response.dnd_release_payload::<TreeDrag>() {
        if fits(&payload) {
            actions.push(Action::Folder(project, FolderOp::Move { what: payload.what.clone(), to }));
        }
    }
}

/// The items of a "Move to folder" submenu for `what` in project
/// `project`: a new folder, every folder (indented by depth) and, when it is
/// in one, the top level.
fn move_menu(ui: &mut egui::Ui, project: u64, folders: &[Folder], what: &Dragged, actions: &mut Vec<Action>) {
    let place = placement(folders);
    let current = match what {
        Dragged::Item(item) => place.get(item).copied(),
        Dragged::Folder(id) => parent_of(folders, *id),
    };
    let at_top = match what {
        Dragged::Item(_) => current.is_none(),
        Dragged::Folder(id) => folders.iter().any(|f| f.id == *id),
    };
    ui.menu_button("Move to folder", |ui| {
        if ui.button("New folder").on_hover_text("Make a folder in this project and move this into it").clicked() {
            actions.push(Action::Folder(project, FolderOp::New { parent: None, with: Some(what.clone()) }));
            ui.close_menu();
        }
        fn list(
            ui: &mut egui::Ui,
            folders: &[Folder],
            depth: usize,
            ctx: (u64, &Dragged, Option<u64>),
            actions: &mut Vec<Action>,
        ) {
            let (project, what, current) = ctx;
            for f in folders {
                if *what == Dragged::Folder(f.id) {
                    continue;
                }
                let text = format!("{}{}", "    ".repeat(depth), f.name);
                if ui.add_enabled(current != Some(f.id), egui::Button::new(text)).clicked() {
                    actions.push(Action::Folder(project, FolderOp::Move { what: what.clone(), to: Some(f.id) }));
                    ui.close_menu();
                }
                list(ui, &f.folders, depth + 1, ctx, actions);
            }
        }
        if !folders.is_empty() {
            ui.separator();
            list(ui, folders, 0, (project, what, current), actions);
        }
        if !at_top {
            ui.separator();
            if ui.button("Out of folders").on_hover_text("List it at the top of the project again").clicked() {
                actions.push(Action::Folder(project, FolderOp::Move { what: what.clone(), to: None }));
                ui.close_menu();
            }
        }
    });
}

/// Where the fold state of a table's reports in the tree is kept.
fn folded_reports_id() -> egui::Id {
    egui::Id::new("tree_folded_reports")
}

impl StatsApp {
    /// Project `id`'s folders.
    pub(super) fn folders(&self, id: u64) -> &[Folder] {
        self.project(id).map_or(&[][..], |p| p.folders.as_slice())
    }

    /// Whether `item` of project `id` is still open.
    fn item_exists(&self, id: u64, item: &TreeItem) -> bool {
        match item {
            TreeItem::Table(doc) => self.documents.iter().any(|d| d.id == *doc && d.project == id),
            TreeItem::Report { doc, report } => self
                .documents
                .iter()
                .any(|d| d.id == *doc && d.project == id && d.reports.iter().any(|r| r.id() == *report)),
            TreeItem::Dashboard(d) => self.project(id).is_some_and(|p| p.dashboards.iter().any(|x| x.id == *d)),
            TreeItem::Notebook(name) => self.project(id).is_some_and(|p| p.notebooks.iter().any(|n| &n.name == name)),
        }
    }

    /// Forget folder entries of project `id` for things that have closed.
    pub(super) fn prune_folders(&mut self, id: u64) {
        let Some(p) = self.projects.iter().position(|p| p.id == id) else { return };
        if self.projects[p].folders.is_empty() {
            return;
        }
        let mut folders = std::mem::take(&mut self.projects[p].folders);
        prune(&mut folders, &|item| self.item_exists(id, item));
        self.projects[p].folders = folders;
    }

    /// Project `id`'s folders as they are written to its file.
    pub(super) fn folders_for_file(&self, id: u64) -> Vec<ProjectFolder> {
        fn convert(app: &StatsApp, id: u64, f: &Folder) -> ProjectFolder {
            let items = f
                .items
                .iter()
                .filter_map(|item| match item {
                    TreeItem::Table(doc) => app
                        .documents
                        .iter()
                        .find(|d| d.id == *doc && d.project == id)
                        .map(|d| FolderItem::Table(d.table_id.clone())),
                    TreeItem::Report { doc, report } => {
                        let d = app.documents.iter().find(|d| d.id == *doc && d.project == id)?;
                        let index = d.reports.iter().position(|r| r.id() == *report)?;
                        Some(FolderItem::Report { table: d.table_id.clone(), index })
                    }
                    TreeItem::Dashboard(gui) => app
                        .project(id)?
                        .dashboards
                        .iter()
                        .find(|x| x.id == *gui)
                        .map(|x| FolderItem::Dashboard(x.dashboard.id.clone())),
                    TreeItem::Notebook(name) => app.item_exists(id, item).then(|| FolderItem::Notebook(name.clone())),
                })
                .collect();
            ProjectFolder {
                name: f.name.clone(),
                folders: f.folders.iter().map(|g| convert(app, id, g)).collect(),
                items,
            }
        }
        self.folders(id).iter().map(|f| convert(self, id, f)).collect()
    }

    /// Give project `id` the folders read from its file (after its tables,
    /// reports and dashboards are open).
    pub(super) fn apply_folders(&mut self, id: u64, saved: &[ProjectFolder]) {
        fn convert(app: &StatsApp, id: u64, f: &ProjectFolder) -> Folder {
            let mut folder = Folder::new(f.name.clone());
            folder.items = f
                .items
                .iter()
                .filter_map(|item| match item {
                    FolderItem::Table(t) => app
                        .documents
                        .iter()
                        .find(|d| d.project == id && &d.table_id == t)
                        .map(|d| TreeItem::Table(d.id)),
                    FolderItem::Report { table, index } => {
                        let d = app.documents.iter().find(|d| d.project == id && &d.table_id == table)?;
                        d.reports.get(*index).map(|r| TreeItem::Report { doc: d.id, report: r.id() })
                    }
                    FolderItem::Dashboard(dashboard) => app
                        .project(id)?
                        .dashboards
                        .iter()
                        .find(|x| &x.dashboard.id == dashboard)
                        .map(|x| TreeItem::Dashboard(x.id)),
                    FolderItem::Notebook(name) => Some(TreeItem::Notebook(name.clone())),
                })
                .collect();
            folder.folders = f.folders.iter().map(|g| convert(app, id, g)).collect();
            folder
        }
        let folders = saved.iter().map(|f| convert(self, id, f)).collect();
        if let Some(p) = self.project_mut(id) {
            p.folders = folders;
        }
    }

    /// Carry out `op` on project `project`'s folders.
    pub(super) fn folder_op(&mut self, ctx: &egui::Context, project: u64, op: FolderOp) {
        let Some(p) = self.projects.iter().position(|p| p.id == project) else { return };
        match op {
            FolderOp::New { parent, with } => {
                let mut taken = Vec::new();
                all_names(&self.projects[p].folders, &mut taken);
                let folder = Folder::new(unused_name("New folder", &taken));
                let (id, name) = (folder.id, folder.name.clone());
                let folders = &mut self.projects[p].folders;
                match parent.and_then(|f| find_mut(folders, f)) {
                    Some(parent) => parent.folders.push(folder),
                    None => folders.push(folder),
                }
                if let Some(what) = with {
                    apply_move(folders, &what, Some(id));
                }
                self.projects[p].collapsed = false;
                renaming::start(ctx, RenameTarget::Folder(project, id), Place::Sidebar, &name);
                self.modified_marks.remove(&project);
            }
            FolderOp::Move { what, to } => {
                if apply_move(&mut self.projects[p].folders, &what, to) {
                    self.modified_marks.remove(&project);
                }
            }
            FolderOp::Delete(id) => {
                if let Some(gone) = delete_folder(&mut self.projects[p].folders, id) {
                    self.info(format!("Removed the folder “{}”; what it held is still in the project.", gone.name));
                    self.modified_marks.remove(&project);
                }
            }
            FolderOp::Fold(id) => {
                if let Some(f) = find_mut(&mut self.projects[p].folders, id) {
                    f.collapsed = !f.collapsed;
                }
            }
            FolderOp::Show(id) => self.show_folder(ctx, project, id),
            FolderOp::OpenEach(id) => self.open_folder_in_windows(project, id),
        }
    }

    /// Rename folder `id` of project `project` (a blank name changes nothing).
    pub(super) fn rename_folder(&mut self, project: u64, id: u64, name: &str) {
        let Some(p) = self.projects.iter().position(|p| p.id == project) else { return };
        if name.is_empty() {
            return;
        }
        if let Some(f) = find_mut(&mut self.projects[p].folders, id) {
            if f.name != name {
                f.name = name.to_owned();
                self.modified_marks.remove(&project);
            }
        }
    }

    /// Bring the windows of everything in folder `id` to the front; when none
    /// of it has a window, show its first table (or dashboard, or report's
    /// table) in the main window.
    fn show_folder(&mut self, ctx: &egui::Context, project: u64, id: u64) {
        let Some(folder) = find(self.folders(project), id) else { return };
        let (name, items) = (folder.name.clone(), folder.all_items());
        let mut raised = 0;
        for item in &items {
            match item {
                TreeItem::Table(doc) => {
                    let n = self
                        .windows
                        .iter()
                        .filter(|w| w.document == *doc && !matches!(w.view, View::Dashboard(_)))
                        .count();
                    raised += n;
                    self.raise_windows_of(*doc);
                }
                TreeItem::Report { doc, report } => {
                    if self.in_window(*doc, View::Report(*report)) {
                        raised += 1;
                        self.raise_window(*doc, View::Report(*report));
                    }
                }
                TreeItem::Dashboard(d) => {
                    if self.in_window_dashboard(*d) {
                        raised += 1;
                        self.raise_dashboard_window(*d);
                    }
                }
                TreeItem::Notebook(_) => {}
            }
        }
        if raised > 0 {
            self.info(format!("Brought {} of “{name}” to the front.", count(raised, "window", "windows")));
            return;
        }
        let first_table = items.iter().find_map(|i| match i {
            TreeItem::Table(doc) => Some(*doc),
            _ => None,
        });
        let first_dashboard = items.iter().find_map(|i| match i {
            TreeItem::Dashboard(d) => Some(*d),
            _ => None,
        });
        let first_report = items.iter().find_map(|i| match i {
            TreeItem::Report { doc, report } => Some((*doc, *report)),
            _ => None,
        });
        if let Some(index) = first_table.and_then(|doc| self.documents.iter().position(|d| d.id == doc)) {
            self.set_active(index);
        } else if let Some(d) = first_dashboard {
            self.show_dashboard(d);
        } else if let Some((doc, report)) = first_report {
            self.show_report(ctx, doc, report);
        } else if items.is_empty() {
            self.info(format!("The folder “{name}” is empty. Drag tables, reports or dashboards onto it."));
        }
    }

    /// Give everything in folder `id` a window of its own: tables their data
    /// grid, reports and dashboards their own window, notebooks their window.
    fn open_folder_in_windows(&mut self, project: u64, id: u64) {
        let Some(folder) = find(self.folders(project), id) else { return };
        for item in folder.all_items() {
            match item {
                TreeItem::Table(doc) => self.pop_out(doc, View::Data),
                TreeItem::Report { doc, report } => self.pop_out(doc, View::Report(report)),
                TreeItem::Dashboard(d) => self.pop_out_dashboard(d),
                TreeItem::Notebook(name) => self.open_notebook(project, &name),
            }
        }
    }

    /// Show report `report` of document `doc`: bring its window (or the
    /// results window holding it) to the front, or show its table's results
    /// in the main window scrolled to it, open.
    pub(super) fn show_report(&mut self, ctx: &egui::Context, doc: u64, report: u64) {
        let Some(index) = self.documents.iter().position(|d| d.id == doc) else { return };
        let windowed = self.in_window(doc, View::Report(report)) || self.in_window(doc, View::Results);
        if !windowed {
            self.set_active(index);
        } else {
            self.set_target(index);
        }
        self.reveal(index, View::Report(report));
        if let Some(r) = self.documents[index].reports.iter_mut().find(|r| r.id() == report) {
            crate::report_view::jump_to_report(ctx, r);
        }
    }

    /// The rows of project `id`'s tree under its heading (`several`: the
    /// project's id when it holds more than one table): its folders, then
    /// what is in no folder.
    pub(super) fn project_tree(&self, ui: &mut egui::Ui, id: u64, several: Option<u64>, actions: &mut Vec<Action>) {
        let folders = self.folders(id);
        let place = placement(folders);
        for f in folders {
            self.folder_rows(ui, id, f, 1, &place, several, actions);
        }
        self.contents_rows(ui, id, None, 1, &place, several, actions);
    }

    /// Folder `f`'s row, `depth` levels in, and (unless folded) what is in it.
    #[allow(clippy::too_many_arguments)]
    fn folder_rows(
        &self,
        ui: &mut egui::Ui,
        project: u64,
        f: &Folder,
        depth: usize,
        place: &HashMap<TreeItem, u64>,
        several: Option<u64>,
        actions: &mut Vec<Action>,
    ) {
        let folders = self.folders(project);
        ui.horizontal(|ui| {
            ui.add_space(12.0 * depth as f32);
            let (arrow, fold) =
                if f.collapsed { ("▸", "Show what this folder holds") } else { ("▾", "Fold this folder away") };
            let button = ui.add(egui::Button::new(arrow).frame(false)).on_hover_text(fold);
            button
                .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Fold {}", f.name)));
            if button.clicked() {
                actions.push(Action::Folder(project, FolderOp::Fold(f.id)));
            }
            let size = 14.0 * theme::text_scale(ui);
            let icon = if f.collapsed { Icon::Folder } else { Icon::FolderOpen };
            icons::show(ui, icon, size, ui.visuals().text_color());
            let target = RenameTarget::Folder(project, f.id);
            if renaming::field(ui, target, Place::Sidebar, ui.available_width().min(200.0), actions) {
                return;
            }
            let n = f.all_items().len();
            let tip = format!(
                "Folder · {}\nClick to bring its windows to the front; double-click to rename; drag tables, reports \
                 and dashboards onto it; right-click for more",
                count(n, "item", "items")
            );
            let response = ui
                .add(egui::Label::new(RichText::new(&f.name)).truncate().sense(egui::Sense::click_and_drag()))
                .on_hover_text(tip);
            if response.double_clicked() {
                renaming::start(ui.ctx(), target, Place::Sidebar, &f.name);
            } else if response.clicked() {
                actions.push(Action::Folder(project, FolderOp::Show(f.id)));
            }
            drag_source(ui, &response, TreeDrag { project, what: Dragged::Folder(f.id), label: f.name.clone() });
            drop_target(ui, &response, project, Some(f.id), actions);
            response.context_menu(|ui| {
                renaming::menu_item(ui, target, Place::Sidebar, &f.name, "this folder");
                if ui.button("New folder inside").clicked() {
                    actions.push(Action::Folder(project, FolderOp::New { parent: Some(f.id), with: None }));
                    ui.close_menu();
                }
                ui.separator();
                if ui
                    .button("Show its windows")
                    .on_hover_text("Bring the windows of everything in it to the front")
                    .clicked()
                {
                    actions.push(Action::Folder(project, FolderOp::Show(f.id)));
                    ui.close_menu();
                }
                if ui
                    .button("Open each in a window")
                    .on_hover_text("Give every table, report, dashboard and notebook in it a window of its own")
                    .clicked()
                {
                    actions.push(Action::Folder(project, FolderOp::OpenEach(f.id)));
                    ui.close_menu();
                }
                ui.separator();
                move_menu(ui, project, folders, &Dragged::Folder(f.id), actions);
                if ui
                    .button("Delete folder")
                    .on_hover_text("Remove the folder only; what it holds moves up a level and stays in the project")
                    .clicked()
                {
                    actions.push(Action::Folder(project, FolderOp::Delete(f.id)));
                    ui.close_menu();
                }
            });
        });
        if f.collapsed {
            return;
        }
        for g in &f.folders {
            self.folder_rows(ui, project, g, depth + 1, place, several, actions);
        }
        self.contents_rows(ui, project, Some(f.id), depth + 1, place, several, actions);
    }

    /// The items of project `project` in folder `folder` (`None`: in no
    /// folder), `depth` levels in: tables (made tables under their source
    /// when it is here too) each with its reports that are in no folder, then
    /// reports filed here away from their table, dashboards and notebooks.
    #[allow(clippy::too_many_arguments)]
    fn contents_rows(
        &self,
        ui: &mut egui::Ui,
        project: u64,
        folder: Option<u64>,
        depth: usize,
        place: &HashMap<TreeItem, u64>,
        several: Option<u64>,
        actions: &mut Vec<Action>,
    ) {
        let here = |item: &TreeItem| place.get(item).copied() == folder;
        let tables: Vec<usize> =
            self.members(project).into_iter().filter(|&i| here(&TreeItem::Table(self.documents[i].id))).collect();
        for (i, d) in self.tree_order_of(&tables) {
            self.table_row(ui, i, depth + d, several, true, actions);
            self.reports_of(ui, i, depth + d + 1, place, actions);
        }
        if folder.is_some() {
            for i in self.members(project) {
                let doc = &self.documents[i];
                for r in &doc.reports {
                    if here(&TreeItem::Report { doc: doc.id, report: r.id() }) {
                        self.report_row(ui, i, r, depth, true, actions);
                    }
                }
            }
        }
        let Some(p) = self.project(project) else { return };
        for (k, d) in p.dashboards.iter().enumerate() {
            if here(&TreeItem::Dashboard(d.id)) {
                self.dashboard_row(ui, project, k, depth, actions);
            }
        }
        for n in &p.notebooks {
            if here(&TreeItem::Notebook(n.name.clone())) {
                self.notebook_row(ui, project, &n.name, depth, actions);
            }
        }
    }

    /// The reports of table `i` that are in no folder, `depth` levels in
    /// (unless the table's reports are folded away).
    pub(super) fn reports_of(
        &self,
        ui: &mut egui::Ui,
        i: usize,
        depth: usize,
        place: &HashMap<TreeItem, u64>,
        actions: &mut Vec<Action>,
    ) {
        let doc = &self.documents[i];
        if self.reports_folded(ui.ctx(), doc.id) {
            return;
        }
        for r in &doc.reports {
            if !place.contains_key(&TreeItem::Report { doc: doc.id, report: r.id() }) {
                self.report_row(ui, i, r, depth, false, actions);
            }
        }
    }

    /// Whether table `doc`'s reports are folded away in the tree.
    pub(super) fn reports_folded(&self, ctx: &egui::Context, doc: u64) -> bool {
        ctx.data(|d| d.get_temp::<std::collections::HashSet<u64>>(folded_reports_id()))
            .is_some_and(|s| s.contains(&doc))
    }

    /// The ▸/▾ before a table's name that folds its reports away (blank
    /// space of the same width when it has none, so names line up).
    pub(super) fn reports_fold_button(&self, ui: &mut egui::Ui, doc: &Document) {
        let width = 14.0 * theme::text_scale(ui);
        if doc.reports.is_empty() {
            ui.add_space(width);
            return;
        }
        let folded = self.reports_folded(ui.ctx(), doc.id);
        let (arrow, tip) =
            if folded { ("▸", "Show this table's reports") } else { ("▾", "Fold this table's reports away") };
        let response = ui.add_sized([width, 0.0], egui::Button::new(arrow).frame(false)).on_hover_text(tip);
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Reports of {}", doc.name()))
        });
        if response.clicked() {
            let id = doc.id;
            ui.ctx().data_mut(|d| {
                let set = d.get_temp_mut_or_default::<std::collections::HashSet<u64>>(folded_reports_id());
                if !set.remove(&id) {
                    set.insert(id);
                }
            });
        }
    }

    /// Report `r` of table `i` in the tree, `depth` levels in. `away`: it is
    /// filed in a folder apart from its table, and its tooltip says so.
    fn report_row(&self, ui: &mut egui::Ui, i: usize, r: &Report, depth: usize, away: bool, actions: &mut Vec<Action>) {
        let doc = &self.documents[i];
        let item = TreeItem::Report { doc: doc.id, report: r.id() };
        let title = r.title();
        ui.horizontal(|ui| {
            ui.add_space(12.0 * depth as f32);
            let size = 13.0 * theme::text_scale(ui);
            let (icon, kind) =
                if r.is_graph() { (Icon::ChartScatter, "Graph canvas") } else { (Icon::FileText, "Report") };
            icons::show(ui, icon, size, ui.visuals().weak_text_color());
            // Cut to the sidebar's width rather than widening it.
            let response = ui
                .add(egui::Label::new(title.as_str()).truncate().sense(egui::Sense::click_and_drag()))
                .on_hover_text(format!(
                    "{kind} of {}{}: {title}\nClick to show it; drag it onto a folder; right-click for more",
                    doc.name(),
                    if away { " (filed here apart from its table)" } else { "" }
                ));
            if response.clicked() {
                actions.push(Action::ShowReport { doc: doc.id, report: r.id() });
            }
            drag_source(
                ui,
                &response,
                TreeDrag { project: doc.project, what: Dragged::Item(item.clone()), label: title.clone() },
            );
            response.context_menu(|ui| {
                if ui.button("Show").clicked() {
                    actions.push(Action::ShowReport { doc: doc.id, report: r.id() });
                    ui.close_menu();
                }
                let view = View::Report(r.id());
                if self.in_window(doc.id, view) {
                    if ui.button("Back to main window").clicked() {
                        actions.push(Action::BringBack { doc: doc.id, view });
                        ui.close_menu();
                    }
                } else if ui
                    .button("Open in new window")
                    .on_hover_text("Show this report in a window of its own")
                    .clicked()
                {
                    actions.push(Action::PopOut { doc: doc.id, view });
                    ui.close_menu();
                }
                ui.separator();
                move_menu(ui, doc.project, self.folders(doc.project), &Dragged::Item(item.clone()), actions);
            });
            if self.in_window(doc.id, View::Report(r.id())) {
                theme::window_glyph(ui, ui.visuals().text_color())
                    .on_hover_text("In a window of its own. Click the report to bring the window to the front.")
                    .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, "In its own window"));
            }
        });
    }

    /// Notebook `name` of project `project` in the tree, `depth` levels in.
    fn notebook_row(&self, ui: &mut egui::Ui, project: u64, name: &str, depth: usize, actions: &mut Vec<Action>) {
        ui.horizontal(|ui| {
            ui.add_space(12.0 * depth as f32);
            let size = 13.0 * theme::text_scale(ui);
            icons::show(ui, Icon::BookOpen, size, ui.visuals().weak_text_color());
            let response = ui
                .add(egui::Label::new(name).truncate().sense(egui::Sense::click_and_drag()))
                .on_hover_text("Script notebook\nClick to open it; drag it onto a folder; right-click for more");
            if response.clicked() {
                actions.push(Action::OpenNotebook(project, name.to_owned()));
            }
            let item = TreeItem::Notebook(name.to_owned());
            drag_source(ui, &response, TreeDrag { project, what: Dragged::Item(item.clone()), label: name.to_owned() });
            response.context_menu(|ui| {
                if ui.button("Open").clicked() {
                    actions.push(Action::OpenNotebook(project, name.to_owned()));
                    ui.close_menu();
                }
                ui.separator();
                move_menu(ui, project, self.folders(project), &Dragged::Item(item.clone()), actions);
            });
        });
    }

    /// Make `response` (table `doc`'s row) draggable onto folders.
    pub(super) fn table_drag(&self, ui: &egui::Ui, response: &egui::Response, doc: &Document) {
        let drag = TreeDrag {
            project: doc.project,
            what: Dragged::Item(TreeItem::Table(doc.id)),
            label: doc.name().to_owned(),
        };
        drag_source(ui, response, drag);
    }

    /// Make `response` (dashboard `gui`'s row) draggable onto folders.
    pub(super) fn dashboard_drag(&self, ui: &egui::Ui, response: &egui::Response, project: u64, gui: u64, name: &str) {
        let drag = TreeDrag { project, what: Dragged::Item(TreeItem::Dashboard(gui)), label: name.to_owned() };
        drag_source(ui, response, drag);
    }

    /// The "Move to folder" submenu for `item` of project `project`.
    pub(super) fn move_to_folder_menu(
        &self,
        ui: &mut egui::Ui,
        project: u64,
        item: TreeItem,
        actions: &mut Vec<Action>,
    ) {
        move_menu(ui, project, self.folders(project), &Dragged::Item(item), actions);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> Vec<Folder> {
        let mut a = Folder::new("A");
        a.items = vec![TreeItem::Table(1), TreeItem::Report { doc: 1, report: 7 }];
        let mut b = Folder::new("B");
        b.items = vec![TreeItem::Dashboard(3)];
        a.folders.push(b);
        vec![a, Folder::new("C")]
    }

    #[test]
    fn items_move_between_folders_and_out() {
        let mut folders = tree();
        let (a, b, c) = (folders[0].id, folders[0].folders[0].id, folders[1].id);
        assert_eq!(placement(&folders)[&TreeItem::Table(1)], a);
        assert!(apply_move(&mut folders, &Dragged::Item(TreeItem::Table(1)), Some(c)));
        assert_eq!(placement(&folders)[&TreeItem::Table(1)], c);
        assert!(!folders[0].items.contains(&TreeItem::Table(1)));
        assert!(apply_move(&mut folders, &Dragged::Item(TreeItem::Dashboard(3)), None));
        assert!(!placement(&folders).contains_key(&TreeItem::Dashboard(3)));
        // Into a folder that does not exist: nothing changes.
        assert!(!apply_move(&mut folders, &Dragged::Item(TreeItem::Table(1)), Some(9999)));
        assert_eq!(placement(&folders)[&TreeItem::Table(1)], c);
        let _ = b;
    }

    #[test]
    fn folders_nest_but_never_into_themselves() {
        let mut folders = tree();
        let (a, b, c) = (folders[0].id, folders[0].folders[0].id, folders[1].id);
        // A into B (inside A) would make a loop.
        assert!(!apply_move(&mut folders, &Dragged::Folder(a), Some(b)));
        assert!(!apply_move(&mut folders, &Dragged::Folder(a), Some(a)));
        // C into B, then B (holding C) to the top.
        assert!(apply_move(&mut folders, &Dragged::Folder(c), Some(b)));
        assert_eq!(folders.len(), 1);
        assert_eq!(parent_of(&folders, c), Some(b));
        assert!(apply_move(&mut folders, &Dragged::Folder(b), None));
        assert_eq!(folders.iter().map(|f| f.id).collect::<Vec<_>>(), [a, b]);
        assert_eq!(find(&folders, c).map(|f| f.name.as_str()), Some("C"));
        // Already at the top: nothing to do.
        assert!(!apply_move(&mut folders, &Dragged::Folder(b), None));
    }

    #[test]
    fn deleting_a_folder_keeps_what_it_held() {
        let mut folders = tree();
        let (a, b) = (folders[0].id, folders[0].folders[0].id);
        delete_folder(&mut folders, b).unwrap();
        assert_eq!(placement(&folders)[&TreeItem::Dashboard(3)], a);
        delete_folder(&mut folders, a).unwrap();
        assert!(placement(&folders).is_empty());
        assert_eq!(folders.len(), 1, "only C is left");
        assert!(delete_folder(&mut folders, a).is_none());
    }

    #[test]
    fn closed_items_are_forgotten_and_names_stay_distinct() {
        let mut folders = tree();
        prune(&mut folders, &|item| !matches!(item, TreeItem::Report { .. }));
        assert_eq!(folders[0].items, [TreeItem::Table(1)]);
        let mut names = Vec::new();
        all_names(&folders, &mut names);
        assert_eq!(unused_name("A", &names), "A 2");
        assert_eq!(unused_name("New folder", &names), "New folder");
    }
}
