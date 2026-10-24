//! Recovery saves while the program runs, and the offer to restore them
//! after a run that ended abnormally (see [`app_model::recovery`]).
//!
//! Every [`SAVE_INTERVAL`] seconds the app hands a copy of each open
//! project to a writer thread, which serializes it and writes it to this
//! run's recovery folder if it changed since it was opened or saved. Copying
//! the tables is the only work on the drawing thread.

use std::path::PathBuf;
use std::sync::mpsc;

use app_model::project::ProjectFile;
use app_model::recovery::{OrphanedSession, RecoveryInfo, RecoverySession};

use super::*;

/// Seconds between recovery saves.
pub(super) const SAVE_INTERVAL: f64 = 120.0;

/// Seconds between heartbeats of the writer thread.
const HEARTBEAT_SECS: u64 = 30;

/// Work for the writer thread.
enum Job {
    /// The project was just opened or saved as this.
    Baseline(u64, Box<ProjectFile>),
    /// Save a recovery copy if the project changed.
    Save(u64, RecoveryInfo, Box<ProjectFile>),
    /// The project closed.
    Forget(u64),
    /// The program is ending normally.
    Finish,
}

/// The app's end of the recovery writer thread.
pub(super) struct RecoveryWriter {
    jobs: mpsc::Sender<Job>,
    thread: Option<std::thread::JoinHandle<()>>,
    /// This run's session name (its folder under the recovery root).
    pub id: String,
    /// When (in egui time) projects were last handed over.
    pub last_save: f64,
}

impl RecoveryWriter {
    /// Start this run's session under `root` and its writer thread.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub fn start(root: &std::path::Path) -> std::io::Result<Self> {
        let id = format!("run-{}-{}", app_model::recovery::now_secs(), std::process::id());
        let mut session = RecoverySession::start(root, &id)?;
        #[cfg(not(target_arch = "wasm32"))]
        crash::set_session_dir(session.dir().to_path_buf());
        let (jobs, inbox) = mpsc::channel::<Job>();
        let thread = std::thread::Builder::new().name("recovery saves".to_owned()).spawn(move || {
            let timeout = std::time::Duration::from_secs(HEARTBEAT_SECS);
            loop {
                let job = match inbox.recv_timeout(timeout) {
                    Ok(job) => job,
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        let _ = session.heartbeat();
                        continue;
                    }
                    // The app is gone without saying so (a panic): keep the copies.
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                };
                // Of several saves of one project waiting, only the last matters.
                let mut batch = vec![job];
                batch.extend(inbox.try_iter());
                let later_save =
                    |i: usize, p: u64| batch[i + 1..].iter().any(|j| matches!(j, Job::Save(q, ..) if *q == p));
                let keep: Vec<bool> =
                    (0..batch.len()).map(|i| !matches!(&batch[i], Job::Save(p, ..) if later_save(i, *p))).collect();
                for (job, keep) in batch.into_iter().zip(keep) {
                    match job {
                        _ if !keep => {}
                        Job::Baseline(p, file) => session.set_baseline(p, &file),
                        Job::Save(p, info, file) => {
                            if let Err(e) = session.save(p, &info, &file) {
                                eprintln!("Could not write a recovery copy of “{}”: {e}", info.name);
                            }
                        }
                        Job::Forget(p) => session.forget(p),
                        Job::Finish => {
                            let _ = session.finish();
                            return;
                        }
                    }
                }
            }
        })?;
        Ok(Self { jobs, thread: Some(thread), id, last_save: 0.0 })
    }

    fn send(&self, job: Job) {
        let _ = self.jobs.send(job);
    }
}

impl Drop for RecoveryWriter {
    /// A normal exit removes this run's recovery folder; a panic keeps it
    /// for the next launch.
    fn drop(&mut self) {
        if crash::crashed() {
            return;
        }
        self.send(Job::Finish);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The offer, at launch, to restore what runs that ended abnormally saved.
#[derive(Debug, Clone)]
pub(super) struct RecoveryOffer {
    pub sessions: Vec<OrphanedSession>,
}

impl RecoveryOffer {
    /// The crash report of the most recent session that left one.
    fn crash_report(&self) -> Option<&PathBuf> {
        self.sessions.iter().rev().find_map(|s| s.crash_report.as_ref())
    }
}

/// When a recovery copy was written, as local date and time.
fn local_time(secs: u64) -> String {
    use chrono::TimeZone;
    i64::try_from(secs)
        .ok()
        .and_then(|s| chrono::Local.timestamp_opt(s, 0).single())
        .map_or_else(String::new, |t| t.format("%Y-%m-%d %H:%M").to_string())
}

impl StatsApp {
    /// Start recovery saves for this run and look for work left by runs
    /// that ended abnormally.
    #[cfg(not(any(test, target_arch = "wasm32")))]
    pub(super) fn start_recovery(&mut self) {
        let Some(root) = app_model::recovery::recovery_root() else { return };
        match RecoveryWriter::start(&root) {
            Ok(writer) => {
                let sessions =
                    app_model::recovery::orphaned_sessions(&root, &writer.id, app_model::recovery::now_secs());
                self.recovery = Some(writer);
                if !sessions.is_empty() {
                    self.recovery_offer = Some(RecoveryOffer { sessions });
                }
            }
            Err(e) => eprintln!("Recovery saves are off: could not write to {}: {e}", root.display()),
        }
    }

    /// Project `id` was opened or saved as it is now.
    pub(super) fn recovery_baseline(&self, id: u64) {
        if let Some(w) = &self.recovery {
            w.send(Job::Baseline(id, Box::new(self.project_file(id, false))));
        }
    }

    /// Project `id` closed.
    pub(super) fn recovery_forget(&self, id: u64) {
        if let Some(w) = &self.recovery {
            w.send(Job::Forget(id));
        }
    }

    /// Hand every open project to the writer when a save is due.
    pub(super) fn recovery_tick(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        let Some(w) = &self.recovery else { return };
        if now - w.last_save < SAVE_INTERVAL {
            return;
        }
        for p in &self.projects {
            let info = RecoveryInfo { name: self.project_name(p.id), original: p.path.clone() };
            w.send(Job::Save(p.id, info, Box::new(self.project_file(p.id, false))));
        }
        if let Some(w) = &mut self.recovery {
            w.last_save = now;
        }
        // Wake up for the next save even when nothing moves on screen.
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(SAVE_INTERVAL));
    }

    /// Open every project of the offer as an unsaved project (saving writes
    /// to the file it came from), then delete the recovery copies.
    fn restore_recovered(&mut self, offer: &RecoveryOffer) {
        let (mut restored, mut errors) = (0, Vec::new());
        for session in &offer.sessions {
            for p in &session.projects {
                let file = std::fs::read_to_string(&p.file)
                    .map_err(|e| e.to_string())
                    .and_then(|t| app_model::project::load_project_file(&t).map_err(|e| e.to_string()));
                match file {
                    Ok(file) => {
                        let title = if p.original.is_some() { None } else { Some(p.name.clone()) };
                        let new =
                            NewProject { path: p.original.clone(), title, unsaved: true, ..NewProject::default() };
                        self.open_project_file(file, new);
                        restored += 1;
                    }
                    Err(e) => errors.push(format!("“{}”: {e}", p.name)),
                }
            }
        }
        if errors.is_empty() {
            for s in &offer.sessions {
                let _ = s.discard();
            }
            self.info(format!("Restored {}. Save to keep the changes.", count(restored, "project", "projects")));
        } else {
            self.error(format!("Could not restore {}", errors.join("; ")));
        }
    }

    /// The restore offer, shown once at launch.
    pub(super) fn recovery_window(&mut self, ctx: &egui::Context) {
        let Some(offer) = self.recovery_offer.clone() else { return };
        let projects: Vec<&app_model::recovery::RecoveredProject> =
            offer.sessions.iter().flat_map(|s| &s.projects).collect();
        let mut choice = None;
        egui::Window::new("Restore unsaved work")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.set_max_width(460.0);
                let crash = offer.crash_report().is_some();
                ui.label(if crash {
                    "AnovaCraft stopped unexpectedly the last time it ran."
                } else {
                    "AnovaCraft did not close normally the last time it ran."
                });
                if projects.is_empty() {
                    ui.label("No unsaved work was lost.");
                } else {
                    ui.label("These projects had changes that were not saved; a copy was kept:");
                    ui.add_space(4.0);
                    for p in &projects {
                        let place =
                            p.original.as_ref().map_or_else(|| "never saved".to_owned(), |o| o.display().to_string());
                        ui.label(RichText::new(format!("•  {}", p.name)).strong())
                            .on_hover_text(p.file.display().to_string());
                        ui.label(
                            RichText::new(format!("    {place} · copy from {}", local_time(p.saved_at))).weak().small(),
                        );
                    }
                }
                if let Some(report) = offer.crash_report() {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("A crash report was written.").small());
                        if ui.small_button("Show crash report").on_hover_text(report.display().to_string()).clicked() {
                            choice = Some(Choice::ShowReport(report.clone()));
                        }
                    });
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if projects.is_empty() {
                        if ui.button("OK").clicked() {
                            choice = Some(Choice::Discard);
                        }
                        return;
                    }
                    if ui.button("Restore").on_hover_text("Open the copies as projects with unsaved changes").clicked()
                    {
                        choice = Some(Choice::Restore);
                    }
                    if ui.button("Discard").on_hover_text("Delete the copies").clicked() {
                        choice = Some(Choice::Discard);
                    }
                    if ui
                        .button("Decide later")
                        .on_hover_text("Keep the copies and ask again at the next launch")
                        .clicked()
                    {
                        choice = Some(Choice::Later);
                    }
                });
            });
        match choice {
            Some(Choice::Restore) => {
                self.recovery_offer = None;
                self.restore_recovered(&offer);
            }
            Some(Choice::Discard) => {
                self.recovery_offer = None;
                for s in &offer.sessions {
                    let _ = s.discard();
                }
            }
            Some(Choice::Later) => self.recovery_offer = None,
            Some(Choice::ShowReport(path)) => self.show_crash_report(Some(&path)),
            None => {}
        }
    }

    /// Open crash report `path` (or the crash-reports folder) in the
    /// system's viewer.
    pub(super) fn show_crash_report(&mut self, path: Option<&std::path::Path>) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let Some(target) = path.map(std::path::Path::to_path_buf).or_else(app_model::recovery::log_dir) else {
                self.error("No folder for crash reports is set on this computer");
                return;
            };
            if path.is_none() && !cfg!(test) {
                let _ = std::fs::create_dir_all(&target);
            }
            if let Err(e) = manuals::launch(&target) {
                self.error(format!("Could not open {}: {e}", target.display()));
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = path;
            self.error("Crash reports are kept by the desktop app");
        }
    }
}

/// What the restore offer's buttons ask for.
enum Choice {
    Restore,
    Discard,
    Later,
    ShowReport(PathBuf),
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The writer thread writes changed projects to the session's folder,
    /// and a normal exit (dropping the writer) removes the folder.
    #[test]
    fn writer_saves_changed_projects_and_cleans_up_on_exit() {
        let root = std::env::temp_dir().join(format!("anovacraft-recovery-writer-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let writer = RecoveryWriter::start(&root).unwrap();
        let dir = root.join(&writer.id);
        assert!(dir.join("heartbeat").exists());
        let table = DataTable::new("t", vec![stat_data::Column::numeric("y", vec![1.0, 2.0])]).unwrap();
        let mut file = ProjectFile::default();
        file.add_table(table, RowStateManager::new(2));
        writer.send(Job::Baseline(3, Box::new(file.clone())));
        writer.send(Job::Save(3, RecoveryInfo::default(), Box::new(file.clone())));
        file.tables[0].table.set_value(0, 0, stat_data::CellValue::Number(5.0)).unwrap();
        writer.send(Job::Save(4, RecoveryInfo::default(), Box::new(file.clone())));
        let copy = dir.join("project-4.acraft");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !copy.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(copy.exists(), "the never-saved project got a copy");
        assert!(!dir.join("project-3.acraft").exists(), "the unchanged one did not");
        drop(writer);
        assert!(!dir.exists(), "a normal exit removes the session");
        let _ = std::fs::remove_dir_all(&root);
    }
}
