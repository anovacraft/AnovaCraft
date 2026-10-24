//! The Script window: edit, open, save and run Rhai scripts against the open
//! documents (see `stat_script` and `docs/scripting.md`).
//!
//! A run happens on a background thread, on copies of the open documents,
//! so the window stays responsive and Stop works between operations. When
//! it ends, the application merges what the script did (see
//! `StatsApp::apply_script_run`): data edits become one undoable step,
//! reports follow their columns, and tables the script showed open as new
//! documents.

use std::collections::VecDeque;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::thread::JoinHandle;
use std::time::Instant;

use eframe::egui::{self, Key, KeyboardShortcut, Modifiers, RichText};
use stat_script::{RunOptions, ScriptError, Workspace, WorkspaceRun};

use crate::code_editor::{CodeEditor, EditorSettings};
use crate::file_io;
use crate::script_edit::{self, FindOptions};
use crate::theme::ScriptPalette;

/// Run the whole script.
pub const RUN_SHORTCUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Enter);
/// Run the selected lines.
const RUN_SELECTION_SHORTCUT: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::Enter);

/// Open the find and replace bar.
const FIND_SHORTCUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::F);

/// Output lines kept; older ones are dropped.
const MAX_OUTPUT_LINES: usize = 5000;

/// A script shown when the window first opens.
const STARTER: &str = "\
// Scripts work on the open tables. See docs/scripting.md for the full list
// of functions; Run (Ctrl/⌘ + Enter) runs the script, Shift as well runs the
// selected lines only.
let t = current_table();
print(`${t.name}: ${t.rows()} rows, columns ${t.columns()}`);
";

/// One line in the output log.
#[derive(Debug, Clone, PartialEq)]
struct OutputLine {
    text: String,
    kind: LineKind,
    /// Script line it refers to (errors), for "go to line".
    line: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineKind {
    /// The script's `print` / `debug` output.
    Print,
    /// From the application: run started, finished, what changed.
    Note,
    Error,
}

/// A script running on a background thread.
struct Running {
    cancel: Arc<AtomicBool>,
    output: Receiver<String>,
    handle: Option<JoinHandle<WorkspaceRun>>,
    started: Instant,
}

/// What asked to replace or close the script while it had unsaved changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pending {
    New,
    Open,
    Quit,
}

/// What the window asks the application to do.
pub enum ScriptEvent {
    /// Run this source on the open documents.
    Run { source: String },
    /// The user cancelled quitting (from the unsaved-script question).
    CancelQuit,
    /// Keep this script in the current table's project.
    KeepInProject { name: String, source: String, run_on_open: bool },
}

/// Where a script shown in the window came from, when not from a file.
struct FromProject {
    /// The script's name in the project.
    name: String,
    /// The project's name, for the title.
    table: String,
    /// The project file's folder, for relative paths.
    base_dir: Option<PathBuf>,
}

/// The find and replace bar.
#[derive(Debug, Clone, Default)]
struct FindBar {
    needle: String,
    replacement: String,
    options: FindOptions,
    /// Index of the match selected last.
    current: Option<usize>,
    /// Result of the last Replace all.
    message: Option<String>,
}

/// The Script window's state (one script at a time).
pub struct ScriptWindow {
    /// The window is shown.
    pub open: bool,
    path: Option<PathBuf>,
    text: String,
    saved_text: String,
    output: VecDeque<OutputLine>,
    running: Option<Running>,
    /// 1-based line of the last error, highlighted in the editor.
    error_line: Option<usize>,
    /// Move the cursor to this 1-based line on the next frame.
    goto: Option<usize>,
    /// Selected characters in the editor, as of the last frame.
    selection: Option<Range<usize>>,
    confirm: Option<Pending>,
    /// The script was opened from a project rather than a file.
    from_project: Option<FromProject>,
    /// The editor: folding, bracket matching, completion.
    editor: CodeEditor,
    /// Find and replace, while open.
    find: Option<FindBar>,
    /// Column names of the active table, offered inside strings.
    columns: Vec<String>,
    /// Give the find box keyboard focus on the next frame.
    find_focus: Option<egui::Id>,
}

impl Default for ScriptWindow {
    fn default() -> Self {
        Self {
            open: false,
            path: None,
            text: STARTER.to_owned(),
            saved_text: STARTER.to_owned(),
            output: VecDeque::new(),
            running: None,
            error_line: None,
            goto: None,
            selection: None,
            confirm: None,
            from_project: None,
            editor: CodeEditor::new(editor_id()),
            find: None,
            columns: Vec::new(),
            find_focus: None,
        }
    }
}

fn editor_id() -> egui::Id {
    egui::Id::new("script_editor")
}

impl ScriptWindow {
    /// The active table's column names, for completion inside strings.
    pub fn set_columns(&mut self, columns: Vec<String>) {
        self.columns = columns;
    }

    /// The script has changes that are not in its file.
    pub fn is_modified(&self) -> bool {
        self.text != self.saved_text
    }

    /// A script is running.
    pub fn is_running(&self) -> bool {
        self.running.is_some()
    }

    /// The editor has keyboard focus (so ⌘S saves the script).
    pub fn editor_has_focus(&self, ctx: &egui::Context) -> bool {
        self.open && ctx.memory(|m| m.has_focus(editor_id()))
    }

    /// Ask about the unsaved script before quitting.
    pub fn ask_before_quit(&mut self) {
        self.open = true;
        self.confirm = Some(Pending::Quit);
    }

    /// Waiting for an answer about unsaved changes.
    pub fn is_asking(&self) -> bool {
        self.confirm.is_some()
    }

    /// Stop a running script (when quitting).
    pub fn stop(&self) {
        if let Some(r) = &self.running {
            r.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Open a script file in the window.
    pub fn open_file(&mut self, path: PathBuf) -> Result<(), String> {
        let text = std::fs::read_to_string(&path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
        self.text = text.clone();
        self.saved_text = text;
        self.path = Some(path);
        self.from_project = None;
        self.error_line = None;
        self.editor.reset();
        self.open = true;
        Ok(())
    }

    /// Show `text` as a new, unsaved script (a recording).
    pub fn open_text(&mut self, text: String) {
        self.text = text;
        self.saved_text = String::new();
        self.path = None;
        self.from_project = None;
        self.error_line = None;
        self.editor.reset();
        self.open = true;
    }

    /// Show a script kept in project `table` (stored at `project_path`) in
    /// the window. It is not linked to a file until saved as one.
    pub fn open_project_script(&mut self, name: &str, source: &str, table: &str, project_path: Option<&Path>) {
        self.text = source.to_owned();
        self.saved_text = source.to_owned();
        self.path = None;
        self.from_project = Some(FromProject {
            name: name.to_owned(),
            table: table.to_owned(),
            base_dir: project_path.and_then(Path::parent).map(PathBuf::from),
        });
        self.error_line = None;
        self.editor.reset();
        self.open = true;
    }

    /// The script was kept in project `table`: a script not saved to a file
    /// now belongs to that project (so it no longer counts as unsaved).
    pub fn kept_in_project(&mut self, name: &str, table: &str, project_path: Option<&Path>) {
        if self.path.is_none() {
            let text = self.text.clone();
            self.open_project_script(name, &text, table, project_path);
        }
    }

    /// The script's text, its name, and the folder its relative paths start
    /// from (for a batch run of the window's script).
    pub fn current_script(&self) -> (String, String, Option<PathBuf>) {
        let base_dir = match &self.from_project {
            Some(f) => f.base_dir.clone(),
            None => self.path.as_ref().and_then(|p| p.parent()).map(PathBuf::from),
        };
        (self.text.clone(), self.script_name(), base_dir)
    }

    /// The name to keep the script under in a project.
    fn script_name(&self) -> String {
        if let Some(f) = &self.from_project {
            return f.name.clone();
        }
        self.path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map_or_else(|| "Script".to_owned(), |s| s.to_string_lossy().into_owned())
    }

    fn title(&self) -> String {
        if let Some(f) = &self.from_project {
            let changed = if self.is_modified() { " (changed)" } else { "" };
            return format!("Script — {} in “{}”{changed}", f.name, f.table);
        }
        let name = self
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map_or_else(|| "Untitled script".to_owned(), |n| n.to_string_lossy().into_owned());
        format!("Script — {name}{}", if self.is_modified() { " (unsaved)" } else { "" })
    }

    /// Save to the script's file, asking for one if needed (or always with
    /// `ask`). Returns `true` if it was saved.
    pub fn save(&mut self, ask: bool) -> bool {
        let path = match (&self.path, ask) {
            (Some(p), false) => p.clone(),
            _ => {
                let name =
                    self.path.as_ref().and_then(|p| p.file_stem()).map_or("script".into(), |s| s.to_string_lossy());
                match file_io::pick_save_file("Save script", "Rhai script", stat_script::SCRIPT_EXTENSION, &name) {
                    Some(p) => p,
                    None => return false,
                }
            }
        };
        match std::fs::write(&path, &self.text) {
            Ok(()) => {
                self.saved_text = self.text.clone();
                self.path = Some(path);
                self.from_project = None;
                true
            }
            Err(e) => {
                self.note(format!("Could not save {}: {e}", path.display()), LineKind::Error, None);
                false
            }
        }
    }

    fn new_script(&mut self) {
        self.text.clear();
        self.saved_text.clear();
        self.path = None;
        self.from_project = None;
        self.error_line = None;
        self.editor.reset();
    }

    fn pick_and_open(&mut self) {
        let ext = stat_script::SCRIPT_EXTENSION;
        if let Some(path) = file_io::pick_open_file("Open script", "Rhai script", &[ext]) {
            if let Err(e) = self.open_file(path) {
                self.note(e, LineKind::Error, None);
            }
        }
    }

    fn note(&mut self, text: impl Into<String>, kind: LineKind, line: Option<usize>) {
        for part in text.into().split('\n') {
            self.output.push_back(OutputLine { text: part.to_owned(), kind, line });
        }
        while self.output.len() > MAX_OUTPUT_LINES {
            self.output.pop_front();
        }
    }

    /// Add a note from the application (what a run changed).
    pub fn add_note(&mut self, text: impl Into<String>) {
        self.note(text, LineKind::Note, None);
    }

    /// Start running `source` on `workspace` in the background. A script
    /// kept in a project passes its name and the project file, whose folder
    /// relative paths then start from; otherwise they start from the script
    /// file's folder.
    pub fn start(
        &mut self,
        ctx: &egui::Context,
        source: String,
        workspace: Workspace,
        project_script: Option<(&str, Option<&Path>)>,
    ) {
        let cancel = Arc::new(AtomicBool::new(false));
        let base_dir = match project_script {
            Some((_, project)) => project.and_then(Path::parent).map(PathBuf::from),
            None => match &self.from_project {
                Some(f) => f.base_dir.clone(),
                None => self.path.as_ref().and_then(|p| p.parent()).map(PathBuf::from),
            },
        };
        let what = match project_script {
            Some((name, _)) => name.to_owned(),
            None => self.from_project.as_ref().map_or_else(
                || {
                    self.path
                        .as_ref()
                        .and_then(|p| p.file_name())
                        .map_or("script".into(), |n| n.to_string_lossy().into_owned())
                },
                |f| f.name.clone(),
            ),
        };
        let options = RunOptions {
            base_dir: base_dir.unwrap_or_default(),
            cancel: Some(Arc::clone(&cancel)),
            ..RunOptions::default()
        };
        let (tx, rx) = channel();
        let repaint = ctx.clone();
        let handle = std::thread::Builder::new()
            .name("script".into())
            .spawn(move || {
                let waker = repaint.clone();
                let run = stat_script::run_in_workspace(&source, &options, workspace, move |line| {
                    let _ = tx.send(line.to_owned());
                    waker.request_repaint();
                });
                repaint.request_repaint();
                run
            })
            .expect("a thread for the script");
        self.error_line = None;
        self.note(format!("Running {what}…"), LineKind::Note, None);
        self.running = Some(Running { cancel, output: rx, handle: Some(handle), started: Instant::now() });
    }

    /// Collect output, and the result once the script has finished.
    pub fn poll(&mut self) -> Option<WorkspaceRun> {
        let running = self.running.as_mut()?;
        let lines: Vec<String> = running.output.try_iter().collect();
        let finished = running.handle.as_ref().is_some_and(JoinHandle::is_finished);
        for line in lines {
            self.note(line, LineKind::Print, None);
        }
        if !finished {
            return None;
        }
        let mut running = self.running.take()?;
        // Output sent just before the thread ended.
        let late: Vec<String> = running.output.try_iter().collect();
        for line in late {
            self.note(line, LineKind::Print, None);
        }
        let seconds = running.started.elapsed().as_secs_f64();
        let run = match running.handle.take()?.join() {
            Ok(run) => run,
            Err(_) => {
                self.note("The script stopped unexpectedly (an internal error).", LineKind::Error, None);
                return None;
            }
        };
        match &run.result {
            Ok(()) => self.note(format!("Finished in {seconds:.2} s."), LineKind::Note, None),
            Err(e) => self.report_error(e),
        }
        Some(run)
    }

    fn report_error(&mut self, e: &ScriptError) {
        if e.cancelled {
            self.note("Stopped.", LineKind::Note, None);
            return;
        }
        self.error_line = e.line;
        if e.line.is_some() {
            self.editor.unfold_all();
        }
        let text = match e.line {
            Some(line) => format!("Error on line {line}: {}", e.message),
            None => format!("Error: {}", e.message),
        };
        self.note(text, LineKind::Error, e.line);
    }

    /// Draw the window. Returns what the application should do.
    pub fn show(&mut self, ctx: &egui::Context) -> Option<ScriptEvent> {
        if !self.open {
            return None;
        }
        let mut event = None;
        let mut open = true;
        // The run keys belong to the Script window unless another text box
        // (a notebook cell) has the keyboard.
        let elsewhere = ctx.memory(|m| m.focused().is_some_and(|f| f != editor_id()));
        let run_keys = if elsewhere {
            (false, false)
        } else {
            ctx.input_mut(|i| {
                let selection = i.consume_shortcut(&RUN_SELECTION_SHORTCUT);
                (i.consume_shortcut(&RUN_SHORTCUT), selection)
            })
        };
        if self.editor_has_focus(ctx) && ctx.input_mut(|i| i.consume_shortcut(&FIND_SHORTCUT)) {
            self.open_find();
        }
        let title = self.title();
        egui::Window::new(title)
            .id(egui::Id::new("script_window"))
            .open(&mut open)
            .default_size([640.0, 560.0])
            .min_width(420.0)
            .resizable(true)
            .collapsible(true)
            .show(ctx, |ui| {
                event = self.contents(ui, run_keys);
            });
        if !open {
            event = self.closed();
        }
        event
    }

    /// The window was closed with its × button. The script stays and is
    /// there when reopened; an unanswered question about unsaved changes
    /// counts as Cancel (left pending, it would block quitting unseen).
    fn closed(&mut self) -> Option<ScriptEvent> {
        self.open = false;
        match self.confirm.take() {
            Some(Pending::Quit) => Some(ScriptEvent::CancelQuit),
            _ => None,
        }
    }

    fn contents(&mut self, ui: &mut egui::Ui, (run_key, run_selection_key): (bool, bool)) -> Option<ScriptEvent> {
        let mut event = None;
        if let Some(pending) = self.confirm {
            if let Some(e) = self.confirmation(ui, pending) {
                return Some(e);
            }
            if self.confirm.is_some() {
                return None;
            }
        }
        let running = self.is_running();
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(!running, egui::Button::new("New")).clicked() {
                if self.is_modified() { self.confirm = Some(Pending::New) } else { self.new_script() }
            }
            if ui.add_enabled(!running, egui::Button::new("Open…")).clicked() {
                if self.is_modified() { self.confirm = Some(Pending::Open) } else { self.pick_and_open() }
            }
            if ui.button("Save").on_hover_text("Ctrl/⌘ + S while editing").clicked() {
                self.save(false);
            }
            if ui.button("Save As…").clicked() {
                self.save(true);
            }
            crate::menus::menu_button(ui, "Keep in project", |ui| {
                let name = self.script_name();
                ui.label(RichText::new(format!("Keep as “{name}” in the current table's project")).weak());
                if ui.button("Keep").on_hover_text("Run it later from the table's right-click menu ▸ Scripts").clicked()
                {
                    event = Some(ScriptEvent::KeepInProject {
                        name: name.clone(),
                        source: self.text.clone(),
                        run_on_open: false,
                    });
                    ui.close_menu();
                }
                if ui
                    .button("Keep and run when the project opens")
                    .on_hover_text("Opening the project asks before running it")
                    .clicked()
                {
                    event = Some(ScriptEvent::KeepInProject { name, source: self.text.clone(), run_on_open: true });
                    ui.close_menu();
                }
            });
            ui.separator();
            let ctx = ui.ctx().clone();
            let run = egui::Button::new("▶ Run").shortcut_text(ctx.format_shortcut(&RUN_SHORTCUT));
            if (ui.add_enabled(!running, run).on_hover_text("Run the whole script on the open tables").clicked()
                || run_key)
                && !running
            {
                event = Some(ScriptEvent::Run { source: self.text.clone() });
            }
            let has_selection = self.selection.as_ref().is_some_and(|r| !r.is_empty());
            let run_sel =
                egui::Button::new("Run selection").shortcut_text(ctx.format_shortcut(&RUN_SELECTION_SHORTCUT));
            if (ui
                .add_enabled(!running && has_selection, run_sel)
                .on_hover_text("Run only the selected text")
                .on_disabled_hover_text("Select some lines in the editor first")
                .clicked()
                || (run_selection_key && has_selection))
                && !running
            {
                if let Some(range) = self.selection.clone() {
                    event = Some(ScriptEvent::Run { source: selection_source(&self.text, range) });
                }
            }
            if ui.add_enabled(running, egui::Button::new("■ Stop")).on_hover_text("Stop the running script").clicked()
            {
                self.stop();
            }
            if running {
                ui.spinner();
                ui.label(RichText::new("Running").weak());
            }
            ui.separator();
            let find = egui::Button::new("Find…").shortcut_text(ui.ctx().format_shortcut(&FIND_SHORTCUT));
            if ui.add(find).on_hover_text("Find and replace text in the script (or a recording)").clicked() {
                self.open_find();
            }
            crate::menus::menu_button(ui, "Fold", |ui| {
                if ui.button("Fold all blocks").on_hover_text("Show each outermost { … } block as ⋯").clicked() {
                    self.editor.fold_all(&self.text);
                    ui.close_menu();
                }
                if ui.add_enabled(self.editor.is_folded(), egui::Button::new("Unfold all")).clicked() {
                    self.editor.unfold_all();
                    ui.close_menu();
                }
            });
        });
        if self.find.is_some() {
            self.find_bar(ui);
        }
        if ctx_save_requested(ui.ctx(), self) {
            self.save(false);
        }
        ui.separator();
        let pal = ScriptPalette::of(ui);
        let log_height = (ui.available_height() * 0.3).clamp(80.0, 260.0);
        let editor_height = (ui.available_height() - log_height - 28.0).max(120.0);
        egui::ScrollArea::both()
            .id_salt("script_editor_scroll")
            .max_height(editor_height)
            .auto_shrink(false)
            .show(ui, |ui| self.editor(ui, &pal));
        ui.separator();
        ui.horizontal(|ui| {
            ui.label(RichText::new("Output").strong());
            if ui.small_button("Clear").clicked() {
                self.output.clear();
            }
        });
        let mut goto = None;
        egui::ScrollArea::vertical().id_salt("script_output").stick_to_bottom(true).auto_shrink(false).show(ui, |ui| {
            let mono = egui::TextStyle::Monospace.resolve(ui.style());
            for line in &self.output {
                let text = RichText::new(&line.text).font(mono.clone());
                let text = match line.kind {
                    LineKind::Print => text,
                    LineKind::Note => text.color(pal.note).italics(),
                    LineKind::Error => text.color(pal.error),
                };
                match line.line {
                    Some(l) => {
                        if ui.link(text).on_hover_text(format!("Go to line {l}")).clicked() {
                            goto = Some(l);
                        }
                    }
                    None => {
                        ui.label(text);
                    }
                }
            }
        });
        if goto.is_some() {
            self.goto = goto;
        }
        event
    }

    fn confirmation(&mut self, ui: &mut egui::Ui, pending: Pending) -> Option<ScriptEvent> {
        let when = match pending {
            Pending::New => "before starting a new script",
            Pending::Open => "before opening another script",
            Pending::Quit => "before quitting",
        };
        ui.label(RichText::new(format!("Save the changes to this script {when}?")).strong());
        ui.add_space(6.0);
        let mut proceed = false;
        let mut event = None;
        ui.horizontal(|ui| {
            if ui.button(if self.path.is_some() { "Save" } else { "Save…" }).clicked() && self.save(false) {
                proceed = true;
            }
            if ui.button("Don't Save").clicked() {
                // Forget the changes, so quitting can go ahead.
                self.saved_text = self.text.clone();
                proceed = true;
            }
            if ui.button("Cancel").clicked() {
                self.confirm = None;
                if pending == Pending::Quit {
                    event = Some(ScriptEvent::CancelQuit);
                }
            }
        });
        ui.separator();
        if proceed {
            self.confirm = None;
            match pending {
                Pending::New => self.new_script(),
                Pending::Open => self.pick_and_open(),
                Pending::Quit => {}
            }
        }
        event
    }

    fn editor(&mut self, ui: &mut egui::Ui, _pal: &ScriptPalette) {
        let found = self.find_matches();
        let settings = EditorSettings {
            rows: 16,
            error_line: self.error_line,
            columns: &self.columns,
            found: &found,
            gutter: true,
        };
        let output = self.editor.show(ui, &mut self.text, &settings);
        self.selection = output.selection;
        if output.changed {
            self.error_line = None;
        }
        if let Some(line) = self.goto.take() {
            self.editor.go_to_line(line);
        }
    }

    /// Show the find bar, starting from the selected text.
    fn open_find(&mut self) {
        let selected = self
            .selection
            .as_ref()
            .map(|r| self.text.chars().skip(r.start).take(r.len()).collect::<String>())
            .filter(|t| !t.contains('\n'));
        let bar = self.find.get_or_insert_with(FindBar::default);
        if let Some(t) = selected {
            bar.needle = t;
        }
        bar.current = None;
        bar.message = None;
        self.editor.unfold_all();
        let id = egui::Id::new("script_find_text");
        // Focus moves to the find box on the next frame.
        self.find_focus = Some(id);
    }

    /// Byte ranges of the find matches in the script.
    fn find_matches(&self) -> Vec<Range<usize>> {
        self.find.as_ref().map_or_else(Vec::new, |f| script_edit::find_all(&self.text, &f.needle, f.options))
    }

    /// Select match `i` in the editor.
    fn select_match(&mut self, matches: &[Range<usize>], i: usize) {
        let Some(m) = matches.get(i) else { return };
        let chars = script_edit::char_index(&self.text, m.start)..script_edit::char_index(&self.text, m.end);
        self.editor.select(chars);
        if let Some(f) = &mut self.find {
            f.current = Some(i);
        }
    }

    /// The find and replace bar: Find, Replace with, Match case, Whole
    /// names, Next, Previous, Replace, Replace all and a count of matches.
    fn find_bar(&mut self, ui: &mut egui::Ui) {
        let matches = self.find_matches();
        let mut close = false;
        let mut step: Option<isize> = None;
        let mut replace_one = false;
        let mut replace_every = false;
        let focus = self.find_focus.take();
        let Some(bar) = &mut self.find else { return };
        ui.horizontal_wrapped(|ui| {
            ui.label("Find");
            let find_box = ui.add(
                egui::TextEdit::singleline(&mut bar.needle)
                    .id(egui::Id::new("script_find_text"))
                    .desired_width(150.0)
                    .hint_text("text or a column name"),
            );
            if focus.is_some() {
                find_box.request_focus();
            }
            if find_box.changed() {
                bar.current = None;
                bar.message = None;
            }
            if find_box.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                step = Some(1);
            }
            ui.label("Replace with");
            ui.add(
                egui::TextEdit::singleline(&mut bar.replacement).id_salt("script_replace_text").desired_width(150.0),
            );
            ui.checkbox(&mut bar.options.match_case, "Match case");
            ui.checkbox(&mut bar.options.whole_names, "Whole names")
                .on_hover_text("Only where the text is a whole name, not part of a longer one");
        });
        ui.horizontal_wrapped(|ui| {
            let any = !matches.is_empty();
            if ui.add_enabled(any, egui::Button::new("Previous")).clicked() {
                step = Some(-1);
            }
            if ui.add_enabled(any, egui::Button::new("Next")).clicked() {
                step = Some(1);
            }
            if ui
                .add_enabled(any, egui::Button::new("Replace"))
                .on_hover_text("Replace the selected match and go to the next")
                .clicked()
            {
                replace_one = true;
            }
            if ui.add_enabled(any, egui::Button::new("Replace all")).clicked() {
                replace_every = true;
            }
            let count = match (matches.len(), bar.current) {
                (0, _) if bar.needle.is_empty() => String::new(),
                (0, _) => "No matches".to_owned(),
                (n, Some(i)) => format!("{} of {n}", i + 1),
                (1, None) => "1 match".to_owned(),
                (n, None) => format!("{n} matches"),
            };
            ui.label(RichText::new(count).weak());
            if let Some(m) = &bar.message {
                ui.label(RichText::new(m).weak());
            }
            if ui.small_button("✕").on_hover_text("Close find (Esc)").clicked() {
                close = true;
            }
        });
        if ui.input(|i| i.key_pressed(Key::Escape))
            && ui.memory(|m| m.focused() == Some(egui::Id::new("script_find_text")))
        {
            close = true;
        }
        let current = bar.current;
        let needle_len = bar.needle.len();
        let (replacement, options, needle) = (bar.replacement.clone(), bar.options, bar.needle.clone());
        if close {
            self.find = None;
            return;
        }
        if replace_every {
            let (text, n) = script_edit::replace_all(&self.text, &needle, &replacement, options);
            self.text = text;
            self.error_line = None;
            if let Some(bar) = &mut self.find {
                bar.current = None;
                bar.message = Some(format!("Replaced {n}"));
            }
            return;
        }
        if replace_one {
            if let Some(m) =
                current.and_then(|i| matches.get(i)).filter(|m| m.len() == needle_len || !options.match_case)
            {
                let at = m.start;
                self.text.replace_range(m.clone(), &replacement);
                self.error_line = None;
                let after = script_edit::find_all(&self.text, &needle, options);
                let next = after.iter().position(|r| r.start >= at + replacement.len()).unwrap_or(0);
                if after.is_empty() {
                    if let Some(bar) = &mut self.find {
                        bar.current = None;
                    }
                } else {
                    self.select_match(&after, next);
                }
                return;
            }
            step = Some(1);
        }
        if let Some(step) = step.filter(|_| !matches.is_empty()) {
            let n = matches.len() as isize;
            let next = match current {
                Some(i) => (i as isize + step).rem_euclid(n) as usize,
                None => {
                    // From the cursor onwards.
                    let from = self.selection.as_ref().map_or(0, |r| script_edit::byte_index(&self.text, r.start));
                    if step > 0 {
                        matches.iter().position(|m| m.start >= from).unwrap_or(0)
                    } else {
                        matches.iter().rposition(|m| m.start < from).unwrap_or(matches.len() - 1)
                    }
                }
            };
            self.select_match(&matches, next);
        }
    }
}

/// ⌘S while the script editor has focus saves the script, not the project.
fn ctx_save_requested(ctx: &egui::Context, window: &ScriptWindow) -> bool {
    window.editor_has_focus(ctx)
        && ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::S)))
}

/// The selected characters as a script whose line and column numbers match
/// the whole text: everything before the selection becomes spaces (line
/// breaks kept), so an error points at the right place in the editor.
fn selection_source(text: &str, chars: Range<usize>) -> String {
    let mut out = String::with_capacity(text.len());
    for (i, c) in text.chars().enumerate() {
        if i >= chars.end {
            break;
        }
        out.push(if i >= chars.start || c == '\n' { c } else { ' ' });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_selection_keeps_its_line_and_column_numbers() {
        let text = "let a = 1;\nlet b = é + 2;\nprint(b);";
        // From "é" to the end of line 2 (character indices).
        let start = text.chars().position(|c| c == 'é').unwrap();
        let end = start + "é + 2;".chars().count();
        let src = selection_source(text, start..end);
        assert_eq!(src, "          \n        é + 2;");
        assert_eq!(src.lines().nth(1).unwrap().chars().position(|c| c == 'é'), Some(8));
    }

    #[test]
    fn find_selects_matches_and_replace_all_counts_them() {
        let mut w = ScriptWindow { text: "x = \"Weight\"; y = weight;".into(), ..ScriptWindow::default() };
        w.open_find();
        w.find.as_mut().unwrap().needle = "Weight".into();
        assert_eq!(w.find_matches().len(), 2, "case-insensitive by default");
        w.find.as_mut().unwrap().options.match_case = true;
        assert_eq!(w.find_matches().len(), 1);
        assert_eq!(w.find_matches()[0], 5..11);
        let (text, n) = script_edit::replace_all(&w.text, "Weight", "Mass", w.find.as_ref().unwrap().options);
        assert_eq!((text.as_str(), n), ("x = \"Mass\"; y = weight;", 1));
    }

    #[test]
    fn the_output_log_is_bounded() {
        let mut w = ScriptWindow::default();
        for i in 0..MAX_OUTPUT_LINES + 10 {
            w.add_note(format!("{i}"));
        }
        assert_eq!(w.output.len(), MAX_OUTPUT_LINES);
        assert_eq!(w.output.front().unwrap().text, "10");
    }

    #[test]
    fn a_run_reports_output_errors_and_changes() {
        let ctx = egui::Context::default();
        let mut w = ScriptWindow::default();
        w.start(&ctx, "print(\"hi\");\nlet x = ;".into(), Workspace::default(), None);
        let run = loop {
            if let Some(run) = w.poll() {
                break run;
            }
            std::thread::yield_now();
        };
        assert!(run.result.is_err());
        assert_eq!(w.error_line, Some(2));
        let error = w.output.back().unwrap();
        assert_eq!((error.kind, error.line), (LineKind::Error, Some(2)));
        assert!(!w.is_running());
    }

    #[test]
    fn closing_the_window_cancels_an_unanswered_question() {
        let mut w = ScriptWindow { text: "print(1);".into(), open: true, ..ScriptWindow::default() };
        // New was chosen with unsaved changes, then the window was closed unanswered.
        w.confirm = Some(Pending::New);
        assert!(w.closed().is_none());
        assert!(!w.is_asking(), "a hidden question would stop quitting from going on");
        assert!(w.is_modified(), "the script itself is kept");
        w.ask_before_quit();
        assert!(matches!(w.closed(), Some(ScriptEvent::CancelQuit)));
        assert!(!w.is_asking());
    }

    #[test]
    fn a_kept_script_belongs_to_its_project() {
        let mut w = ScriptWindow { text: "print(1);".into(), ..ScriptWindow::default() };
        assert!(w.is_modified());
        assert_eq!(w.script_name(), "Script");
        w.kept_in_project("check", "Norris", Some(Path::new("/data/norris.acraft")));
        assert!(!w.is_modified(), "kept in the project, so not unsaved");
        assert_eq!(w.title(), "Script — check in “Norris”");
        assert_eq!(w.from_project.as_ref().unwrap().base_dir.as_deref(), Some(Path::new("/data")));
        // A script with a file stays linked to its file.
        w.path = Some(PathBuf::from("/s/tidy.rhai"));
        w.from_project = None;
        assert_eq!(w.script_name(), "tidy");
        w.kept_in_project("tidy", "Norris", None);
        assert!(w.from_project.is_none());
    }
}
