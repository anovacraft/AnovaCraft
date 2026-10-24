//! Script notebooks kept in projects (Scripts ▸ New notebook, Scripts ▸
//! Notebooks): the windows, running their cells on the open documents and
//! merging what the cells did.

use super::*;
use crate::notebook_window::{NotebookEvent, NotebookWindow};
use app_model::notebook::{CellKind, CellOutput, Notebook};

impl StatsApp {
    /// A script, or a notebook cell, is running (their changes merge through
    /// the same bookkeeping, so only one runs at a time).
    pub(super) fn any_script_running(&self) -> bool {
        self.script.is_running() || self.notebook_windows.iter().any(NotebookWindow::is_running)
    }

    /// Scripts ▸ New notebook: add a notebook to the active table's project
    /// and open it.
    pub(super) fn new_notebook(&mut self) {
        let Some(project) = self.documents.get(self.active).map(|d| d.project) else {
            self.error("Open a table first: notebooks are kept in a table's project.");
            return;
        };
        let Some(p) = self.project_mut(project) else { return };
        let name = app_model::table_ops::unique_name("Notebook", p.notebooks.iter().map(|n| n.name.as_str()));
        p.notebooks.push(Notebook::new(name.clone()));
        self.open_notebook(project, &name);
    }

    /// Show notebook `name` of project `project` in its window.
    pub(super) fn open_notebook(&mut self, project: u64, name: &str) {
        if let Some(w) = self.notebook_windows.iter_mut().find(|w| w.project == project && w.name == name) {
            w.open = true;
            return;
        }
        let serial = self.next_notebook_serial;
        self.next_notebook_serial += 1;
        self.notebook_windows.push(NotebookWindow::new(project, name, serial));
    }

    /// The notebook a window shows (`None` once it or its project is gone).
    fn window_notebook(&mut self, project: u64, name: &str) -> Option<&mut Notebook> {
        self.project_mut(project)?.notebooks.iter_mut().find(|n| n.name == name)
    }

    /// Collect output from running notebook cells and merge finished runs.
    pub(super) fn poll_notebooks(&mut self, ctx: &egui::Context) {
        for i in 0..self.notebook_windows.len() {
            let (project, name) = (self.notebook_windows[i].project, self.notebook_windows[i].name.clone());
            let mut windows = std::mem::take(&mut self.notebook_windows);
            let finished = self.window_notebook(project, &name).and_then(|n| windows[i].poll(n));
            self.notebook_windows = windows;
            let Some((cell, run)) = finished else { continue };
            let before = self.script_reports.clone();
            let notes = self.apply_script_run(run.run);
            let mut outputs: Vec<CellOutput> = Vec::new();
            for doc in &self.documents {
                let Some(old) = before.get(&doc.id) else { continue };
                for r in doc.reports.iter().filter(|r| !old.contains(&r.id())) {
                    outputs.push(CellOutput::Report { table: doc.table.name().to_owned(), title: r.title() });
                }
            }
            outputs.extend(notes.into_iter().map(CellOutput::Note));
            if let Some(c) = self.window_notebook(project, &name).and_then(|n| n.cells.get_mut(cell)) {
                c.outputs.extend(outputs);
            }
            if let Some(event) = self.notebook_windows[i].next_queued() {
                self.notebook_event(ctx, i, event);
            }
        }
    }

    /// Draw the notebook windows and carry out what they ask.
    pub(super) fn show_notebooks(&mut self, ctx: &egui::Context) {
        // Windows of deleted notebooks or closed projects go.
        let alive: Vec<bool> = self
            .notebook_windows
            .iter()
            .map(|w| self.project(w.project).is_some_and(|p| p.notebooks.iter().any(|n| n.name == w.name)))
            .collect();
        let mut k = 0;
        self.notebook_windows.retain(|w| {
            k += 1;
            alive[k - 1] || w.is_running()
        });
        let columns: Vec<String> = self
            .documents
            .get(self.active)
            .map(|d| d.table.columns().iter().map(|c| c.name.clone()).collect())
            .unwrap_or_default();
        for i in 0..self.notebook_windows.len() {
            let (project, name) = (self.notebook_windows[i].project, self.notebook_windows[i].name.clone());
            let project_name = self.project_name(project);
            let mut windows = std::mem::take(&mut self.notebook_windows);
            let event =
                self.window_notebook(project, &name).and_then(|n| windows[i].show(ctx, n, &project_name, &columns));
            self.notebook_windows = windows;
            if let Some(event) = event {
                self.notebook_event(ctx, i, event);
            }
        }
    }

    /// Carry out what notebook window `i` asked for.
    fn notebook_event(&mut self, ctx: &egui::Context, i: usize, event: NotebookEvent) {
        let (project, name) = (self.notebook_windows[i].project, self.notebook_windows[i].name.clone());
        match event {
            NotebookEvent::Run { cell } => {
                if self.any_script_running() {
                    self.notebook_windows[i].refused();
                    self.error("A script or another notebook cell is running; wait for it or stop it first.");
                    return;
                }
                let Some(source) = self
                    .window_notebook(project, &name)
                    .and_then(|n| n.cells.get(cell))
                    .filter(|c| c.kind == CellKind::Code)
                    .map(|c| c.source.clone())
                else {
                    return;
                };
                let workspace = self.script_workspace();
                let mut windows = std::mem::take(&mut self.notebook_windows);
                if let Some(n) = self.window_notebook(project, &name) {
                    windows[i].begin_cell(n, cell);
                }
                windows[i].start(ctx, cell, source, workspace);
                self.notebook_windows = windows;
            }
            NotebookEvent::ShowReport { table, title } => {
                let found = self.documents.iter().enumerate().find_map(|(d, doc)| {
                    (doc.table.name() == table)
                        .then(|| doc.reports.iter().position(|r| r.title() == title).map(|r| (d, r)))
                        .flatten()
                });
                match found {
                    Some((d, r)) => {
                        let doc = &mut self.documents[d];
                        let report = doc.reports.remove(r);
                        doc.reports.insert(0, report);
                        self.reveal(d, View::Results);
                    }
                    None => self.error(format!("“{title}” is no longer among the reports of “{table}”.")),
                }
            }
            NotebookEvent::ExportScript(script) => {
                if self.script.is_modified() || self.script.is_running() {
                    ctx.copy_text(script);
                    self.info("The script was copied to the clipboard (the Script window has unsaved changes).");
                } else {
                    self.script.open_text(script);
                }
            }
            NotebookEvent::Delete => {
                if let Some(p) = self.project_mut(project) {
                    p.notebooks.retain(|n| n.name != name);
                }
                self.info(format!("Deleted the notebook “{name}”. Save the project to keep the change."));
            }
            NotebookEvent::Rename(new) => {
                let Some(p) = self.project_mut(project) else { return };
                if p.notebooks.iter().any(|n| n.name == new) {
                    self.error(format!("This project already has a notebook called “{new}”."));
                    return;
                }
                if let Some(n) = p.notebooks.iter_mut().find(|n| n.name == name) {
                    n.name.clone_from(&new);
                }
                self.notebook_windows[i].name = new;
            }
        }
    }

    /// The Notebooks submenu items: the active project's notebooks.
    pub(super) fn notebook_menu_items(&mut self, ui: &mut egui::Ui) {
        let Some(project) = self.documents.get(self.active).map(|d| d.project) else { return };
        let names: Vec<String> =
            self.project(project).map(|p| p.notebooks.iter().map(|n| n.name.clone()).collect()).unwrap_or_default();
        if names.is_empty() {
            return;
        }
        crate::menus::menu_button(ui, "Notebooks", |ui| {
            for name in names {
                if ui.button(&name).on_hover_text("Open this notebook of the current table's project").clicked() {
                    self.open_notebook(project, &name);
                    ui.close_menu();
                }
            }
        });
    }
}
