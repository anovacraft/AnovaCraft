//! Scripts ▸ Run script over a folder: run one script once per data file in
//! a folder (see `stat_script::batch`) on a background thread, show its
//! output as it comes, and open what the runs returned as a new table.

#[cfg(target_arch = "wasm32")]
use crate::web_shims::rfd;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::thread::JoinHandle;
use std::time::Instant;

use eframe::egui::{self, RichText};
use stat_data::DataTable;
use stat_script::{BatchOptions, BatchRun, RunOptions, ScriptError, batch_files, run_batch};

use crate::file_io;

/// Output lines kept; older ones are dropped.
const MAX_LINES: usize = 2000;

/// Which script a batch runs.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ScriptChoice {
    /// The script in the Script window.
    ScriptWindow,
    /// A script file.
    File(PathBuf),
}

struct Running {
    cancel: Arc<AtomicBool>,
    output: Receiver<String>,
    handle: Option<JoinHandle<Result<BatchRun, ScriptError>>>,
    started: Instant,
}

/// The batch window's state.
pub struct BatchWindow {
    /// The window is shown.
    pub open: bool,
    script: ScriptChoice,
    folder: Option<PathBuf>,
    pattern: String,
    output: Vec<String>,
    running: Option<Running>,
}

impl Default for BatchWindow {
    fn default() -> Self {
        Self {
            open: false,
            script: ScriptChoice::ScriptWindow,
            folder: None,
            pattern: String::new(),
            output: Vec::new(),
            running: None,
        }
    }
}

/// The script in the Script window: text, name, folder for relative paths.
pub type WindowScript = (String, String, Option<PathBuf>);

impl BatchWindow {
    fn line(&mut self, text: impl Into<String>) {
        self.output.push(text.into());
        if self.output.len() > MAX_LINES {
            let extra = self.output.len() - MAX_LINES;
            self.output.drain(..extra);
        }
    }

    fn pattern(&self) -> Option<String> {
        let p = self.pattern.trim();
        (!p.is_empty()).then(|| p.to_owned())
    }

    fn start(&mut self, ctx: &egui::Context, window_script: &WindowScript) {
        let Some(folder) = self.folder.clone() else { return };
        let (source, name, base_dir) = match &self.script {
            ScriptChoice::ScriptWindow => window_script.clone(),
            ScriptChoice::File(path) => match std::fs::read_to_string(path) {
                Ok(text) => (
                    text,
                    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                    path.parent().map(PathBuf::from),
                ),
                Err(e) => {
                    self.line(format!("Error: could not read {}: {e}", path.display()));
                    return;
                }
            },
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let options = BatchOptions {
            pattern: self.pattern(),
            run: RunOptions {
                base_dir: base_dir.unwrap_or_default(),
                cancel: Some(Arc::clone(&cancel)),
                ..RunOptions::default()
            },
        };
        let (tx, rx) = channel();
        let repaint = ctx.clone();
        let handle = std::thread::Builder::new()
            .name("batch".into())
            .spawn(move || {
                let waker = repaint.clone();
                let run = run_batch(&source, &folder, &options, move |file, line| {
                    let file = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    let _ = tx.send(format!("{file}: {line}"));
                    waker.request_repaint();
                });
                repaint.request_repaint();
                run
            })
            .expect("a thread for the batch");
        self.output.clear();
        self.line(format!("Running {name} over the folder…"));
        self.running = Some(Running { cancel, output: rx, handle: Some(handle), started: Instant::now() });
    }

    /// Collect output; returns the results table once the batch has ended.
    fn poll(&mut self) -> Option<DataTable> {
        let running = self.running.as_mut()?;
        let finished = running.handle.as_ref().is_some_and(JoinHandle::is_finished);
        let lines: Vec<String> = running.output.try_iter().collect();
        for l in lines {
            self.line(l);
        }
        if !finished {
            return None;
        }
        let mut running = self.running.take()?;
        let late: Vec<String> = running.output.try_iter().collect();
        for l in late {
            self.line(l);
        }
        let seconds = running.started.elapsed().as_secs_f64();
        match running.handle.take()?.join() {
            Err(_) => {
                self.line("Error: the batch stopped unexpectedly (an internal error).");
                None
            }
            Ok(Err(e)) => {
                self.line(match e.line {
                    Some(l) => format!("Error on line {l}: {}", e.message),
                    None => format!("Error: {}", e.message),
                });
                None
            }
            Ok(Ok(run)) => {
                for item in &run.items {
                    if let Err(e) = &item.result {
                        let file = item.file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        self.line(format!("{file}: failed — {e}"));
                    }
                }
                let stopped =
                    if run.skipped > 0 { format!(", stopped before {} more", run.skipped) } else { String::new() };
                self.line(format!(
                    "Finished in {seconds:.2} s: {} file(s) run, {} failed{stopped}. The results open as a new table.",
                    run.items.len(),
                    run.failures()
                ));
                (!run.items.is_empty()).then(|| run.table("Batch results"))
            }
        }
    }

    /// Draw the window. Returns the results table when a batch finishes.
    pub fn show(&mut self, ctx: &egui::Context, window_script: &WindowScript) -> Option<DataTable> {
        let result = self.poll();
        if !self.open {
            return result;
        }
        let mut open = true;
        egui::Window::new("Run script over a folder")
            .id(egui::Id::new("batch_window"))
            .open(&mut open)
            .default_size([560.0, 420.0])
            .resizable(true)
            .show(ctx, |ui| self.contents(ui, window_script));
        if !open {
            self.open = false;
        }
        result
    }

    fn contents(&mut self, ui: &mut egui::Ui, window_script: &WindowScript) {
        let running = self.running.is_some();
        ui.add_enabled_ui(!running, |ui| {
            egui::Grid::new("batch_inputs").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                ui.label("Script");
                ui.vertical(|ui| {
                    let window_name = format!("The Script window's script ({})", window_script.1);
                    ui.radio_value(&mut self.script, ScriptChoice::ScriptWindow, window_name);
                    ui.horizontal(|ui| {
                        let file_label = match &self.script {
                            ScriptChoice::File(p) => format!("File: {}", p.display()),
                            ScriptChoice::ScriptWindow => "A script file".to_owned(),
                        };
                        let is_file = matches!(self.script, ScriptChoice::File(_));
                        let radio = ui.radio(is_file, file_label).clicked();
                        let choose = ui.button("Choose…").clicked();
                        if radio || choose {
                            let ext = stat_script::SCRIPT_EXTENSION;
                            if let Some(path) = file_io::pick_open_file("Script to run", "Rhai script", &[ext]) {
                                self.script = ScriptChoice::File(path);
                            }
                        }
                    });
                });
                ui.end_row();
                ui.label("Folder");
                ui.horizontal(|ui| {
                    let shown =
                        self.folder.as_ref().map_or_else(|| "None chosen".to_owned(), |f| f.display().to_string());
                    ui.label(RichText::new(shown).monospace());
                    if ui.button("Choose…").clicked() {
                        if let Some(folder) = rfd::FileDialog::new().set_title("Folder of data files").pick_folder() {
                            self.folder = Some(folder);
                        }
                    }
                });
                ui.end_row();
                ui.label("Files");
                ui.add(egui::TextEdit::singleline(&mut self.pattern).hint_text("Every data file, or e.g. *.csv"))
                    .on_hover_text("File names to include: * stands for any characters, ? for one");
                ui.end_row();
            });
        });
        if let Some(folder) = &self.folder {
            let summary = match batch_files(folder, self.pattern().as_deref()) {
                Ok(files) if files.is_empty() => "No files in the folder match.".to_owned(),
                Ok(files) => {
                    let names: Vec<String> = files
                        .iter()
                        .take(4)
                        .filter_map(|f| f.file_name().map(|n| n.to_string_lossy().into_owned()))
                        .collect();
                    let more = if files.len() > 4 { ", …" } else { "" };
                    format!("{} file(s): {}{more}", files.len(), names.join(", "))
                }
                Err(e) => e,
            };
            ui.label(RichText::new(summary).weak());
        }
        ui.label(
            RichText::new(
                "Each run sees batch_file, batch_name and batch_index; the value of its last line (a map gives \
                 one column per key) becomes its row of the results table.",
            )
            .weak()
            .small(),
        );
        ui.horizontal(|ui| {
            if running {
                ui.spinner();
                if ui.button("⏹ Stop").clicked() {
                    if let Some(r) = &self.running {
                        r.cancel.store(true, Ordering::Relaxed);
                    }
                }
            } else {
                let ready = self.folder.is_some();
                if ui
                    .add_enabled(ready, egui::Button::new("▶ Run"))
                    .on_disabled_hover_text("Choose a folder first")
                    .clicked()
                {
                    self.start(ui.ctx(), window_script);
                }
            }
        });
        ui.separator();
        egui::ScrollArea::vertical().stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
            for line in &self.output {
                let failed = line.starts_with("Error") || line.contains(": failed — ");
                let text = RichText::new(line).monospace();
                ui.label(if failed { text.color(ui.visuals().error_fg_color) } else { text });
            }
        });
    }
}

#[cfg(test)]
impl BatchWindow {
    /// Open the window on script file `script`, `folder` and `pattern`, as
    /// its controls would (for the manual's pictures).
    pub fn set_for_test(&mut self, script: PathBuf, folder: PathBuf, pattern: &str) {
        self.open = true;
        self.script = ScriptChoice::File(script);
        self.folder = Some(folder);
        self.pattern = pattern.to_owned();
    }

    /// Whether a batch is running.
    pub fn is_running(&self) -> bool {
        self.running.is_some()
    }
}
