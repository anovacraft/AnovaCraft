//! Script notebooks (Scripts ▸ New notebook): a window of text cells and
//! Rhai code cells kept in a project, each code cell run on its own or all
//! in order, with its printed lines, errors, value (as text or tables) and
//! the reports it added shown under it.
//!
//! Cells run on a worker thread that holds one [`NotebookSession`], so later
//! cells see the variables and functions of earlier ones; Restart starts a
//! fresh session. Each run gets copies of the open documents and its changes
//! are merged by the application exactly as a Script window run's are (see
//! `StatsApp::apply_script_run`).

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};

use app_model::notebook::{CellKind, CellOutput, Notebook, NotebookCell};
use eframe::egui::{self, Key, KeyboardShortcut, Modifiers, RichText};
use stat_script::{CellRun, NotebookSession, RunOptions, Workspace};

use crate::code_editor::{CodeEditor, EditorSettings};
use crate::theme::ScriptPalette;

/// Run the cell being edited.
const RUN_CELL: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Enter);
/// Run the cell being edited and move to the next one.
const RUN_AND_NEXT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::SHIFT, Key::Enter);

/// A job for the notebook's worker thread.
enum Job {
    Run { source: String, workspace: Workspace, options: RunOptions },
}

/// What the worker thread sends back.
enum Reply {
    Printed(String),
    Done(Box<CellRun>),
}

/// The worker thread holding the notebook's session.
struct Kernel {
    jobs: Sender<Job>,
    replies: Receiver<Reply>,
}

impl Kernel {
    fn start(ctx: &egui::Context) -> Self {
        let (jobs, job_rx) = channel::<Job>();
        let (reply_tx, replies) = channel::<Reply>();
        let repaint = ctx.clone();
        std::thread::Builder::new()
            .name("notebook".into())
            .spawn(move || {
                let mut session = NotebookSession::new(&[]);
                for job in job_rx {
                    let Job::Run { source, workspace, options } = job;
                    let tx = reply_tx.clone();
                    let waker = repaint.clone();
                    let run = session.run_cell(&source, &options, workspace, move |line| {
                        let _ = tx.send(Reply::Printed(line.to_owned()));
                        waker.request_repaint();
                    });
                    if reply_tx.send(Reply::Done(Box::new(run))).is_err() {
                        break;
                    }
                    repaint.request_repaint();
                }
            })
            .expect("a thread for the notebook");
        Self { jobs, replies }
    }
}

/// What the window asks the application to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotebookEvent {
    /// Run code cell `cell` (start it with [`NotebookWindow::start`]).
    Run { cell: usize },
    /// Show report `title` of the table called `table`.
    ShowReport { table: String, title: String },
    /// Show this script in the Script window.
    ExportScript(String),
    /// Delete the notebook from its project.
    Delete,
    /// Rename the notebook.
    Rename(String),
}

/// A cell being run.
struct Running {
    cell: usize,
    cancel: Arc<AtomicBool>,
}

/// One open notebook's window. The notebook itself lives in its project
/// (`OpenProject::notebooks`) and is passed in each frame.
pub struct NotebookWindow {
    /// The project the notebook belongs to.
    pub project: u64,
    /// The notebook's name in that project.
    pub name: String,
    /// The window is shown.
    pub open: bool,
    /// One editor per cell, in cell order.
    editors: Vec<CodeEditor>,
    /// Text cells being edited (shown as text otherwise).
    editing: Vec<bool>,
    kernel: Option<Kernel>,
    running: Option<Running>,
    /// Code cells waiting to run (Run all).
    queue: VecDeque<usize>,
    /// Number of the next run.
    next_run: u32,
    /// Make this cell's editor take the keyboard on the next frame.
    focus: Option<usize>,
    /// The rename box, while open.
    renaming: Option<String>,
    /// Unique number of this window, for widget ids.
    serial: u64,
    /// Editors made so far (each cell editor gets its own id).
    editors_made: u64,
}

impl NotebookWindow {
    /// A window for notebook `name` of project `project`.
    #[must_use]
    pub fn new(project: u64, name: &str, serial: u64) -> Self {
        Self {
            project,
            name: name.to_owned(),
            open: true,
            editors: Vec::new(),
            editing: Vec::new(),
            kernel: None,
            running: None,
            queue: VecDeque::new(),
            next_run: 1,
            focus: None,
            renaming: None,
            serial,
            editors_made: 0,
        }
    }

    /// A cell is running or waiting to.
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.running.is_some()
    }

    /// Stop the running cell and the cells waiting after it.
    pub fn stop(&mut self) {
        self.queue.clear();
        if let Some(r) = &self.running {
            r.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Run code cell `cell`'s `source` with `workspace` (the open documents).
    pub fn start(&mut self, ctx: &egui::Context, cell: usize, source: String, workspace: Workspace) {
        let kernel = self.kernel.get_or_insert_with(|| Kernel::start(ctx));
        let cancel = Arc::new(AtomicBool::new(false));
        let options = RunOptions { cancel: Some(Arc::clone(&cancel)), ..RunOptions::default() };
        if kernel.jobs.send(Job::Run { source, workspace, options }).is_err() {
            // The worker ended (it should not): start a fresh one next time.
            self.kernel = None;
            return;
        }
        self.running = Some(Running { cell, cancel });
    }

    /// The cell could not start (another script runs); drop the queue.
    pub fn refused(&mut self) {
        self.queue.clear();
    }

    /// Collect printed lines into `notebook`, and the result once the
    /// running cell has finished: `(cell, run)`.
    pub fn poll(&mut self, notebook: &mut Notebook) -> Option<(usize, CellRun)> {
        let cell = self.running.as_ref()?.cell;
        let kernel = self.kernel.as_ref()?;
        let mut done = None;
        for reply in kernel.replies.try_iter() {
            match reply {
                Reply::Printed(line) => {
                    if let Some(c) = notebook.cells.get_mut(cell) {
                        c.outputs.push(CellOutput::Printed(line));
                    }
                }
                Reply::Done(run) => done = Some(*run),
            }
        }
        let run = done?;
        self.running = None;
        if let Some(c) = notebook.cells.get_mut(cell) {
            match &run.run.result {
                Ok(()) => c.outputs.extend(run.value.iter().cloned()),
                Err(e) if e.cancelled => c.outputs.push(CellOutput::Note("Stopped.".to_owned())),
                Err(e) => c.outputs.push(CellOutput::Error { message: e.message.clone(), line: e.line }),
            }
        }
        if run.run.result.is_err() {
            // Run all stops at the first failing cell.
            self.queue.clear();
        }
        Some((cell, run))
    }

    /// The next cell waiting to run (Run all), as a Run event.
    pub fn next_queued(&mut self) -> Option<NotebookEvent> {
        if self.running.is_some() {
            return None;
        }
        self.queue.pop_front().map(|cell| NotebookEvent::Run { cell })
    }

    /// Mark cell `cell` as starting: clear its outputs and give it a run
    /// number.
    pub fn begin_cell(&mut self, notebook: &mut Notebook, cell: usize) {
        if let Some(c) = notebook.cells.get_mut(cell) {
            c.outputs.clear();
            c.run = Some(self.next_run);
            self.next_run += 1;
        }
    }

    /// A fresh id for a cell editor.
    fn editor_id(&mut self) -> egui::Id {
        self.editors_made += 1;
        egui::Id::new(("notebook_cell", self.serial, self.editors_made))
    }

    /// Keep one editor per cell.
    fn sync_editors(&mut self, n: usize) {
        while self.editors.len() < n {
            let id = self.editor_id();
            self.editors.push(CodeEditor::new(id));
            self.editing.push(false);
        }
        self.editors.truncate(n);
        self.editing.truncate(n);
    }

    /// Draw the window for `notebook` (in project `project_name`), with the
    /// active table's `columns` for completion. Returns what the
    /// application should do.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        notebook: &mut Notebook,
        project_name: &str,
        columns: &[String],
    ) -> Option<NotebookEvent> {
        if !self.open {
            return None;
        }
        self.sync_editors(notebook.cells.len());
        let mut open = true;
        let mut event = None;
        egui::Window::new(format!("Notebook — {} in “{project_name}”", notebook.name))
            .id(egui::Id::new(("notebook_window", self.serial)))
            .open(&mut open)
            .default_size([700.0, 640.0])
            .min_width(460.0)
            .resizable(true)
            .collapsible(true)
            .show(ctx, |ui| {
                event = self.contents(ui, notebook, columns);
            });
        if !open {
            self.open = false;
        }
        event
    }

    fn contents(&mut self, ui: &mut egui::Ui, notebook: &mut Notebook, columns: &[String]) -> Option<NotebookEvent> {
        let mut event = None;
        let running = self.is_running();
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(!running, egui::Button::new("▶ Run all"))
                .on_hover_text("Run every code cell from the top, stopping at an error")
                .clicked()
            {
                self.queue = notebook
                    .cells
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| c.kind == CellKind::Code)
                    .map(|(i, _)| i)
                    .collect();
                event = self.next_queued();
            }
            if ui.add_enabled(running, egui::Button::new("■ Stop")).on_hover_text("Stop the running cell").clicked() {
                self.stop();
            }
            if ui
                .add_enabled(!running, egui::Button::new("Restart"))
                .on_hover_text("Forget the variables and functions earlier cells defined")
                .clicked()
            {
                self.kernel = None;
                self.next_run = 1;
            }
            if running {
                ui.spinner();
                ui.label(RichText::new("Running").weak());
            }
            ui.separator();
            if ui.button("+ Text").on_hover_text("Add a text cell at the end").clicked() {
                notebook.cells.push(NotebookCell::new(CellKind::Text, ""));
                self.sync_editors(notebook.cells.len());
                let last = notebook.cells.len() - 1;
                self.editing[last] = true;
                self.focus = Some(last);
            }
            if ui.button("+ Code").on_hover_text("Add a code cell at the end").clicked() {
                notebook.cells.push(NotebookCell::new(CellKind::Code, ""));
                self.sync_editors(notebook.cells.len());
                self.focus = Some(notebook.cells.len() - 1);
            }
            crate::menus::menu_button(ui, "Notebook", |ui| {
                if ui.button("Clear outputs").clicked() {
                    notebook.clear_outputs();
                    ui.close_menu();
                }
                if ui
                    .button("Copy as one script")
                    .on_hover_text("Show the code cells, with the text cells as comments, in the Script window")
                    .clicked()
                {
                    event = Some(NotebookEvent::ExportScript(notebook.to_script()));
                    ui.close_menu();
                }
                if ui.button("Rename…").clicked() {
                    self.renaming = Some(notebook.name.clone());
                    ui.close_menu();
                }
                if ui.add_enabled(!running, egui::Button::new("Delete notebook")).clicked() {
                    event = Some(NotebookEvent::Delete);
                    ui.close_menu();
                }
            });
        });
        if let Some(name) = &mut self.renaming {
            let mut done = None;
            ui.horizontal(|ui| {
                ui.label("Name");
                let r = ui.add(egui::TextEdit::singleline(name).id_salt(("notebook_rename", self.serial)));
                if ui.button("Rename").clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter))) {
                    done = Some(true);
                }
                if ui.button("Cancel").clicked() {
                    done = Some(false);
                }
            });
            match done {
                Some(true) => {
                    let name = name.trim().to_owned();
                    self.renaming = None;
                    if !name.is_empty() && name != notebook.name {
                        event = Some(NotebookEvent::Rename(name));
                    }
                }
                Some(false) => self.renaming = None,
                None => {}
            }
        }
        ui.separator();
        let mut action: Option<CellAction> = None;
        egui::ScrollArea::vertical().id_salt(("notebook_cells", self.serial)).auto_shrink(false).show(ui, |ui| {
            for i in 0..notebook.cells.len() {
                if let Some(a) = self.cell(ui, notebook, i, columns, running) {
                    action = Some(a);
                }
                ui.add_space(6.0);
            }
            if notebook.cells.is_empty() {
                ui.label(RichText::new("No cells: add a text or code cell above.").weak());
            }
        });
        match action {
            Some(CellAction::Run(i, advance)) => {
                event = Some(NotebookEvent::Run { cell: i });
                if advance {
                    if i + 1 >= notebook.cells.len() {
                        notebook.cells.push(NotebookCell::new(CellKind::Code, ""));
                    }
                    self.focus = Some(i + 1);
                }
            }
            Some(CellAction::Up(i)) if i > 0 => {
                notebook.cells.swap(i, i - 1);
                self.editors.swap(i, i - 1);
                self.editing.swap(i, i - 1);
            }
            Some(CellAction::Down(i)) if i + 1 < notebook.cells.len() => {
                notebook.cells.swap(i, i + 1);
                self.editors.swap(i, i + 1);
                self.editing.swap(i, i + 1);
            }
            Some(CellAction::Delete(i)) => {
                notebook.cells.remove(i);
                self.editors.remove(i);
                self.editing.remove(i);
            }
            Some(CellAction::InsertBelow(i, kind)) => {
                notebook.cells.insert(i + 1, NotebookCell::new(kind, ""));
                let id = self.editor_id();
                self.editors.insert(i + 1, CodeEditor::new(id));
                self.editing.insert(i + 1, kind == CellKind::Text);
                self.focus = Some(i + 1);
            }
            Some(CellAction::ShowReport(table, title)) => event = Some(NotebookEvent::ShowReport { table, title }),
            Some(CellAction::GoToLine(i, line)) => self.editors[i].go_to_line(line),
            _ => {}
        }
        event
    }

    /// One cell: a header (kind, run number, buttons), the editor or text,
    /// and a code cell's outputs.
    fn cell(
        &mut self,
        ui: &mut egui::Ui,
        notebook: &mut Notebook,
        i: usize,
        columns: &[String],
        running: bool,
    ) -> Option<CellAction> {
        let mut action = None;
        let kind = notebook.cells[i].kind;
        let pal = ScriptPalette::of(ui);
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let cell = &notebook.cells[i];
                let label = match (kind, cell.run) {
                    (CellKind::Text, _) => "Text".to_owned(),
                    (CellKind::Code, Some(n)) => format!("Code [{n}]"),
                    (CellKind::Code, None) => "Code [ ]".to_owned(),
                };
                let is_running = self.running.as_ref().is_some_and(|r| r.cell == i);
                ui.label(RichText::new(label).small().strong());
                if is_running {
                    ui.spinner();
                } else if self.queue.contains(&i) {
                    ui.label(RichText::new("waiting").small().weak());
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("✕").on_hover_text("Delete this cell").clicked() && !is_running {
                        action = Some(CellAction::Delete(i));
                    }
                    if ui.small_button("↓").on_hover_text("Move down").clicked() {
                        action = Some(CellAction::Down(i));
                    }
                    if ui.small_button("↑").on_hover_text("Move up").clicked() {
                        action = Some(CellAction::Up(i));
                    }
                    crate::menus::menu_button(ui, "+", |ui| {
                        if ui.button("Code cell below").clicked() {
                            action = Some(CellAction::InsertBelow(i, CellKind::Code));
                            ui.close_menu();
                        }
                        if ui.button("Text cell below").clicked() {
                            action = Some(CellAction::InsertBelow(i, CellKind::Text));
                            ui.close_menu();
                        }
                    });
                    match kind {
                        CellKind::Code => {
                            let run = ui
                                .add_enabled(!running, egui::Button::new("▶ Run").small())
                                .on_hover_text("Run this cell (Ctrl/⌘ + Enter; Shift + Enter runs and moves on)");
                            if run.clicked() {
                                action = Some(CellAction::Run(i, false));
                            }
                        }
                        CellKind::Text => {
                            let editing = self.editing[i];
                            if ui.small_button(if editing { "Done" } else { "Edit" }).clicked() {
                                self.editing[i] = !editing;
                                if !editing {
                                    self.focus = Some(i);
                                }
                            }
                        }
                    }
                });
            });
            match kind {
                CellKind::Code => {
                    let rows = notebook.cells[i].source.lines().count().clamp(2, 30);
                    let settings = EditorSettings {
                        rows,
                        error_line: notebook.cells[i].outputs.iter().find_map(|o| match o {
                            CellOutput::Error { line, .. } => *line,
                            _ => None,
                        }),
                        columns,
                        found: &[],
                        gutter: true,
                    };
                    let id = self.editors[i].id();
                    if self.focus == Some(i) {
                        ui.ctx().memory_mut(|m| m.request_focus(id));
                        self.focus = None;
                    }
                    let completing = self.editors[i].completing();
                    let keys = if ui.ctx().memory(|m| m.has_focus(id)) && !completing {
                        ui.ctx().input_mut(|inp| (inp.consume_shortcut(&RUN_CELL), inp.consume_shortcut(&RUN_AND_NEXT)))
                    } else {
                        (false, false)
                    };
                    let out = self.editors[i].show(ui, &mut notebook.cells[i].source, &settings);
                    if out.changed {
                        notebook.cells[i].run = None;
                    }
                    if !running && (keys.0 || keys.1) {
                        action = Some(CellAction::Run(i, keys.1));
                    }
                    if let Some(a) = outputs(ui, &notebook.cells[i].outputs, &pal, i) {
                        action = Some(a);
                    }
                }
                CellKind::Text => {
                    if self.editing[i] {
                        let r = ui.add(
                            egui::TextEdit::multiline(&mut notebook.cells[i].source)
                                .id(self.editors[i].id())
                                .desired_width(f32::INFINITY)
                                .desired_rows(3)
                                .hint_text("Notes; a line starting with # is a heading"),
                        );
                        if self.focus == Some(i) {
                            r.request_focus();
                            self.focus = None;
                        }
                    } else {
                        let r = text_cell(ui, &notebook.cells[i].source);
                        if r.double_clicked() {
                            self.editing[i] = true;
                            self.focus = Some(i);
                        }
                    }
                }
            }
        });
        action
    }
}

/// What a cell asked for.
enum CellAction {
    /// Run the cell; `true` also moves to the next cell.
    Run(usize, bool),
    Up(usize),
    Down(usize),
    Delete(usize),
    InsertBelow(usize, CellKind),
    ShowReport(String, String),
    GoToLine(usize, usize),
}

/// A text cell as text: lines starting with `#` as headings. Double-click
/// to edit.
fn text_cell(ui: &mut egui::Ui, source: &str) -> egui::Response {
    let r = ui
        .vertical(|ui| {
            if source.trim().is_empty() {
                ui.label(RichText::new("Empty text cell (double-click or Edit to write)").weak().italics());
            }
            for line in source.lines() {
                let hashes = line.chars().take_while(|c| *c == '#').count();
                if hashes > 0 && line[hashes..].starts_with(' ') {
                    let size = match hashes {
                        1 => 20.0,
                        2 => 17.0,
                        _ => 15.0,
                    };
                    ui.label(RichText::new(line[hashes..].trim()).strong().size(size));
                } else if line.is_empty() {
                    ui.add_space(4.0);
                } else {
                    ui.label(line);
                }
            }
        })
        .response;
    ui.interact(r.rect, r.id.with("text_cell"), egui::Sense::click()).on_hover_text("Double-click to edit")
}

/// A code cell's outputs. Returns a report or line link clicked.
fn outputs(ui: &mut egui::Ui, outputs: &[CellOutput], pal: &ScriptPalette, cell: usize) -> Option<CellAction> {
    let mut action = None;
    if outputs.is_empty() {
        return None;
    }
    ui.add_space(2.0);
    let mono = egui::TextStyle::Monospace.resolve(ui.style());
    for (k, o) in outputs.iter().enumerate() {
        match o {
            CellOutput::Printed(t) => {
                ui.label(RichText::new(t).font(mono.clone()));
            }
            CellOutput::Value(t) => {
                ui.label(RichText::new(format!("= {t}")).font(mono.clone()));
            }
            CellOutput::Note(t) => {
                ui.label(RichText::new(t).color(pal.note).italics());
            }
            CellOutput::Error { message, line } => {
                let text = match line {
                    Some(l) => format!("Error on line {l}: {message}"),
                    None => format!("Error: {message}"),
                };
                let text = RichText::new(format!("⚠ {text}")).color(pal.error);
                match line {
                    Some(l) => {
                        if ui.link(text).on_hover_text(format!("Go to line {l}")).clicked() {
                            action = Some(CellAction::GoToLine(cell, *l));
                        }
                    }
                    None => {
                        ui.label(text);
                    }
                }
            }
            CellOutput::Report { table, title } => {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("Report added to “{table}”:")).color(pal.note));
                    if ui.link(title).on_hover_text("Show the report in its table's results").clicked() {
                        action = Some(CellAction::ShowReport(table.clone(), title.clone()));
                    }
                });
            }
            CellOutput::Table { title, table } => {
                if !title.is_empty() {
                    ui.label(RichText::new(title).strong());
                }
                egui::ScrollArea::horizontal().id_salt(("notebook_table", cell, k)).show(ui, |ui| {
                    egui::Grid::new(("notebook_grid", cell, k)).striped(true).show(ui, |ui| {
                        for c in &table.columns {
                            ui.label(RichText::new(c).strong());
                        }
                        ui.end_row();
                        for row in &table.rows {
                            for v in row {
                                ui.label(RichText::new(v).font(mono.clone()));
                            }
                            ui.end_row();
                        }
                    });
                });
            }
        }
    }
    action
}
