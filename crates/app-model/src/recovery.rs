//! Crash reports and recovery saves for the desktop application.
//!
//! **Crash reports.** When the program panics, the panic hook writes the
//! message, where it happened and a backtrace to a text file in the log
//! folder ([`log_dir`]):
//!
//! | System | Folder |
//! |---|---|
//! | macOS | `~/Library/Logs/AnovaCraft` |
//! | Windows | `%LOCALAPPDATA%\AnovaCraft\Logs` |
//! | Linux and others | `$XDG_STATE_HOME/anovacraft/logs` (default `~/.local/state/…`) |
//!
//! Only the newest [`MAX_REPORTS`] reports are kept.
//!
//! **Recovery saves.** Each run of the application is a *session* with a
//! folder of its own under [`recovery_root`] (the `Recovery` folder next to
//! the settings file). While it runs, projects with changes since they were
//! opened or saved are written there as ordinary project files every couple
//! of minutes, and the session's heartbeat file is touched every half
//! minute. Saving a project removes its recovery copy, and a normal exit
//! removes the whole folder. A folder whose heartbeat has stopped therefore
//! belongs to a run that ended abnormally (a crash, a forced quit, a power
//! cut), and the next launch offers to restore its projects
//! ([`orphaned_sessions`]).
//!
//! Recovery copies leave out row selections and the window layout, which
//! are not counted as unsaved changes either.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::io;
use std::path::{Path, PathBuf};

use stat_data::{JsonValue, parse_json};

use crate::project::{ProjectFile, ProjectLayout, save_project_file};

/// Crash reports kept in the log folder; older ones are deleted.
pub const MAX_REPORTS: usize = 20;

/// Seconds without a heartbeat after which a session is taken to have ended
/// abnormally.
pub const STALE_AFTER_SECS: u64 = 120;

/// File in a session folder holding the last heartbeat (seconds since 1970).
const HEARTBEAT: &str = "heartbeat";

/// File in a session folder naming the crash report written when it ended.
const CRASH_NOTE: &str = "crash-report";

/// The folder crash reports are written to on this computer.
#[must_use]
pub fn log_dir() -> Option<PathBuf> {
    log_dir_for(std::env::consts::OS, |k| std::env::var_os(k).map(PathBuf::from))
}

/// The crash-report folder for operating system `os` (as in
/// `std::env::consts::OS`) with environment lookup `env`.
pub fn log_dir_for(os: &str, env: impl Fn(&str) -> Option<PathBuf>) -> Option<PathBuf> {
    let nonempty = |k: &str| env(k).filter(|p| !p.as_os_str().is_empty());
    Some(match os {
        "macos" => nonempty("HOME")?.join("Library").join("Logs").join("AnovaCraft"),
        "windows" => nonempty("LOCALAPPDATA").or_else(|| nonempty("APPDATA"))?.join("AnovaCraft").join("Logs"),
        _ => nonempty("XDG_STATE_HOME")
            .filter(|p| p.is_absolute())
            .or_else(|| nonempty("HOME").map(|h| h.join(".local").join("state")))?
            .join("anovacraft")
            .join("logs"),
    })
}

/// The folder holding recovery sessions on this computer.
#[must_use]
pub fn recovery_root() -> Option<PathBuf> {
    recovery_root_for(std::env::consts::OS, |k| std::env::var_os(k).map(PathBuf::from))
}

/// The recovery folder for operating system `os` with environment lookup
/// `env`: `Recovery` beside the settings file.
pub fn recovery_root_for(os: &str, env: impl Fn(&str) -> Option<PathBuf>) -> Option<PathBuf> {
    crate::settings::settings_path_for(os, env).and_then(|p| p.parent().map(|d| d.join("Recovery")))
}

/// Seconds since 1970 now.
#[must_use]
pub fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// What a crash report says.
#[derive(Debug, Clone, Default)]
pub struct CrashReport {
    /// The panic message.
    pub message: String,
    /// Source file, line and column of the panic, when known.
    pub location: Option<String>,
    /// Name of the thread that panicked, when it has one.
    pub thread: Option<String>,
    /// The backtrace, as the standard library prints it.
    pub backtrace: String,
    /// The program version.
    pub version: String,
    /// Local date and time, for reading.
    pub when: String,
    /// The panic was caught (an analysis failed and showed an error in its
    /// report) rather than ending the program.
    pub caught: bool,
}

impl CrashReport {
    /// The report as text.
    #[must_use]
    pub fn text(&self) -> String {
        let mut out = String::new();
        out.push_str(if self.caught {
            "AnovaCraft: an analysis stopped with an internal error (the program kept running)\n"
        } else {
            "AnovaCraft stopped unexpectedly\n"
        });
        out.push_str(&format!("Time: {}\n", self.when));
        out.push_str(&format!("Version: {}\n", self.version));
        out.push_str(&format!("System: {} {}\n", std::env::consts::OS, std::env::consts::ARCH));
        if let Some(t) = &self.thread {
            out.push_str(&format!("Thread: {t}\n"));
        }
        if let Some(l) = &self.location {
            out.push_str(&format!("Location: {l}\n"));
        }
        out.push_str(&format!("\nMessage:\n{}\n\nBacktrace:\n{}\n", self.message, self.backtrace));
        out
    }
}

/// Write `report` to a new file in `dir` (created if needed), named from
/// `stamp` (a file-name-safe time such as `2026-10-21-041502`), and delete
/// all but the newest [`MAX_REPORTS`] reports there.
///
/// # Errors
/// The folder could not be created or the file not written.
pub fn write_crash_report(dir: &Path, stamp: &str, report: &CrashReport) -> io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let kind = if report.caught { "analysis-error" } else { "crash" };
    let mut path = dir.join(format!("{kind}-{stamp}.txt"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{kind}-{stamp}-{n}.txt"));
        n += 1;
    }
    std::fs::write(&path, report.text())?;
    prune_reports(dir);
    Ok(path)
}

/// Delete the oldest reports in `dir` beyond [`MAX_REPORTS`].
fn prune_reports(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut reports: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            (name.starts_with("crash-") || name.starts_with("analysis-error-")) && name.ends_with(".txt")
        })
        .map(|p| (std::fs::metadata(&p).and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH), p))
        .collect();
    if reports.len() <= MAX_REPORTS {
        return;
    }
    reports.sort();
    for (_, p) in &reports[..reports.len() - MAX_REPORTS] {
        let _ = std::fs::remove_file(p);
    }
}

/// How a recovered project is described when it is offered back.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RecoveryInfo {
    /// The project's name as the sidebar shows it.
    pub name: String,
    /// The file it was opened from or last saved to.
    pub original: Option<PathBuf>,
}

/// The project as recovery copies hold it: no row selections or layout.
fn recovery_form(file: &ProjectFile) -> String {
    let mut file = file.clone();
    file.layout = ProjectLayout::default();
    for t in &mut file.tables {
        t.rows.clear_selection();
    }
    save_project_file(&file)
}

fn fingerprint(text: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

/// This run's recovery folder and what it holds. Used from one thread (the
/// app's recovery writer), so writing never holds up drawing.
#[derive(Debug)]
pub struct RecoverySession {
    dir: PathBuf,
    /// Fingerprint of each project as last opened or saved.
    baselines: HashMap<u64, u64>,
    /// Fingerprint of each project's recovery copy.
    written: HashMap<u64, u64>,
}

impl RecoverySession {
    /// Start session `id` (unique per run) under `root`, creating its folder
    /// and first heartbeat.
    ///
    /// # Errors
    /// The folder could not be created or written.
    pub fn start(root: &Path, id: &str) -> io::Result<Self> {
        let dir = root.join(id);
        std::fs::create_dir_all(&dir)?;
        let session = Self { dir, baselines: HashMap::new(), written: HashMap::new() };
        session.heartbeat()?;
        Ok(session)
    }

    /// The session's folder.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Record that the session is still running.
    ///
    /// # Errors
    /// The heartbeat file could not be written.
    pub fn heartbeat(&self) -> io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        std::fs::write(self.dir.join(HEARTBEAT), now_secs().to_string())
    }

    fn project_path(&self, project: u64) -> PathBuf {
        self.dir.join(format!("project-{project}.acraft"))
    }

    fn info_path(&self, project: u64) -> PathBuf {
        self.dir.join(format!("project-{project}.json"))
    }

    /// Project `project` was just opened or saved as `file`: until it
    /// differs from this, it needs no recovery copy (any old one is removed).
    pub fn set_baseline(&mut self, project: u64, file: &ProjectFile) {
        self.baselines.insert(project, fingerprint(&recovery_form(file)));
        self.remove_copy(project);
    }

    /// Write a recovery copy of `file` (project `project`) if it differs
    /// both from its baseline and from the copy already written; a project
    /// back at its baseline loses its copy. Returns whether a copy was
    /// written. A project with no baseline (never saved) always counts as
    /// changed.
    ///
    /// # Errors
    /// The copy could not be written.
    pub fn save(&mut self, project: u64, info: &RecoveryInfo, file: &ProjectFile) -> io::Result<bool> {
        let text = recovery_form(file);
        let print = fingerprint(&text);
        if self.baselines.get(&project) == Some(&print) {
            self.remove_copy(project);
            return Ok(false);
        }
        if self.written.get(&project) == Some(&print) {
            return Ok(false);
        }
        std::fs::create_dir_all(&self.dir)?;
        // Write beside, then rename, so a crash while writing leaves the
        // previous copy whole.
        let path = self.project_path(project);
        let partial = path.with_extension("partial");
        std::fs::write(&partial, text)?;
        std::fs::rename(&partial, &path)?;
        let mut members = vec![
            ("name".to_owned(), JsonValue::String(info.name.clone())),
            ("saved_at".to_owned(), JsonValue::number(now_secs() as f64)),
        ];
        if let Some(original) = &info.original {
            members.push(("original".to_owned(), JsonValue::String(original.to_string_lossy().into_owned())));
        }
        std::fs::write(self.info_path(project), JsonValue::Object(members).to_pretty())?;
        self.written.insert(project, print);
        Ok(true)
    }

    /// Project `project` was closed: drop its recovery copy.
    pub fn forget(&mut self, project: u64) {
        self.baselines.remove(&project);
        self.remove_copy(project);
    }

    fn remove_copy(&mut self, project: u64) {
        self.written.remove(&project);
        let _ = std::fs::remove_file(self.project_path(project));
        let _ = std::fs::remove_file(self.info_path(project));
    }

    /// The program is ending normally: remove the session's folder.
    ///
    /// # Errors
    /// The folder could not be removed.
    pub fn finish(self) -> io::Result<()> {
        match std::fs::remove_dir_all(&self.dir) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }
}

/// Note in session folder `dir` that it ended with the crash report at
/// `report` (called from the panic hook).
///
/// # Errors
/// The note could not be written.
pub fn note_crash(dir: &Path, report: &Path) -> io::Result<()> {
    std::fs::write(dir.join(CRASH_NOTE), report.to_string_lossy().as_bytes())
}

/// A project saved for recovery by a run that ended abnormally.
#[derive(Debug, Clone, PartialEq)]
pub struct RecoveredProject {
    /// The recovery copy (an ordinary project file).
    pub file: PathBuf,
    /// The project's name when it was saved.
    pub name: String,
    /// The file it had been opened from or saved to.
    pub original: Option<PathBuf>,
    /// When the copy was written (seconds since 1970).
    pub saved_at: u64,
}

/// A run that ended abnormally: its recovered projects and crash report.
#[derive(Debug, Clone, PartialEq)]
pub struct OrphanedSession {
    /// The session's folder.
    pub dir: PathBuf,
    /// Projects it had saved for recovery, oldest first.
    pub projects: Vec<RecoveredProject>,
    /// The crash report it left, when it ended in a panic.
    pub crash_report: Option<PathBuf>,
    /// Its last heartbeat (seconds since 1970).
    pub last_seen: u64,
}

impl OrphanedSession {
    /// Delete the session's folder and its recovery copies.
    ///
    /// # Errors
    /// The folder could not be removed.
    pub fn discard(&self) -> io::Result<()> {
        match std::fs::remove_dir_all(&self.dir) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }
}

/// Sessions under `root` other than `own` whose heartbeat stopped more than
/// [`STALE_AFTER_SECS`] before `now`, oldest first. Ended sessions that left
/// nothing to restore and no crash report are removed on the way.
#[must_use]
pub fn orphaned_sessions(root: &Path, own: &str, now: u64) -> Vec<OrphanedSession> {
    let Ok(entries) = std::fs::read_dir(root) else { return Vec::new() };
    let mut found = Vec::new();
    for dir in entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()) {
        if dir.file_name().is_some_and(|n| n == own) {
            continue;
        }
        let last_seen =
            std::fs::read_to_string(dir.join(HEARTBEAT)).ok().and_then(|t| t.trim().parse::<u64>().ok()).unwrap_or(0);
        if now.saturating_sub(last_seen) <= STALE_AFTER_SECS {
            continue; // still running (another window of the program)
        }
        let crash_report = std::fs::read_to_string(dir.join(CRASH_NOTE)).ok().map(|t| PathBuf::from(t.trim()));
        let mut projects: Vec<RecoveredProject> = std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "acraft"))
            .map(|file| {
                let info = std::fs::read_to_string(file.with_extension("json")).ok().and_then(|t| parse_json(&t).ok());
                let get = |k: &str| info.as_ref().and_then(|v| v.get(k));
                let name = get("name").and_then(JsonValue::as_str).map_or_else(
                    || file.file_stem().map_or_else(String::new, |s| s.to_string_lossy().into_owned()),
                    str::to_owned,
                );
                let original = get("original").and_then(JsonValue::as_str).map(PathBuf::from);
                let saved_at = get("saved_at").and_then(JsonValue::as_f64).map_or(last_seen, |s| s as u64);
                RecoveredProject { file, name, original, saved_at }
            })
            .collect();
        if projects.is_empty() && crash_report.is_none() {
            let _ = std::fs::remove_dir_all(&dir);
            continue;
        }
        projects.sort_by(|a, b| (a.saved_at, &a.name).cmp(&(b.saved_at, &b.name)));
        found.push(OrphanedSession { dir, projects, crash_report, last_seen });
    }
    found.sort_by_key(|s| s.last_seen);
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::load_project_file;
    use stat_data::{Column, DataTable, RowStateManager};

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("anovacraft-recovery-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn project(values: Vec<f64>) -> ProjectFile {
        let n = values.len();
        let table = DataTable::new("Runs", vec![Column::numeric("y", values)]).unwrap();
        let mut file = ProjectFile::default();
        file.add_table(table, RowStateManager::new(n));
        file
    }

    #[test]
    fn per_platform_log_folders() {
        let env = |vars: &'static [(&'static str, &'static str)]| {
            move |k: &str| vars.iter().find(|v| v.0 == k).map(|v| PathBuf::from(v.1))
        };
        assert_eq!(
            log_dir_for("macos", env(&[("HOME", "/Users/a")])),
            Some(PathBuf::from("/Users/a/Library/Logs/AnovaCraft"))
        );
        assert_eq!(
            log_dir_for("windows", env(&[("LOCALAPPDATA", "C:/Users/a/AppData/Local")])),
            Some(PathBuf::from("C:/Users/a/AppData/Local/AnovaCraft/Logs"))
        );
        assert_eq!(
            log_dir_for("linux", env(&[("HOME", "/home/a")])),
            Some(PathBuf::from("/home/a/.local/state/anovacraft/logs"))
        );
        // XDG_STATE_HOME counts only when absolute, which "/s" is not on Windows.
        const STATE: &str = if cfg!(windows) { "C:/s" } else { "/s" };
        assert_eq!(
            log_dir_for("linux", env(&[("HOME", "/home/a"), ("XDG_STATE_HOME", STATE)])),
            Some(PathBuf::from(STATE).join("anovacraft").join("logs"))
        );
        assert_eq!(log_dir_for("linux", env(&[])), None);
        assert_eq!(
            recovery_root_for("macos", env(&[("HOME", "/Users/a")])),
            Some(PathBuf::from("/Users/a/Library/Application Support/Statistical Engine/Recovery"))
        );
    }

    #[test]
    fn crash_reports_carry_the_message_and_are_pruned() {
        let dir = temp_dir("logs");
        let report = CrashReport {
            message: "index out of bounds".to_owned(),
            location: Some("src/lib.rs:10:5".to_owned()),
            thread: Some("main".to_owned()),
            backtrace: "0: main".to_owned(),
            version: "0.1.0".to_owned(),
            when: "2026-10-21 04:15:02".to_owned(),
            caught: false,
        };
        let path = write_crash_report(&dir, "2026-10-21-041502", &report).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("AnovaCraft stopped unexpectedly"));
        assert!(text.contains("index out of bounds") && text.contains("src/lib.rs:10:5") && text.contains("0: main"));
        // The same second twice gets a second file.
        let again = write_crash_report(&dir, "2026-10-21-041502", &report).unwrap();
        assert_ne!(path, again);
        let caught = CrashReport { caught: true, ..report.clone() };
        let caught_path = write_crash_report(&dir, "x", &caught).unwrap();
        assert!(caught_path.file_name().unwrap().to_string_lossy().starts_with("analysis-error-"));
        for i in 0..30 {
            write_crash_report(&dir, &format!("t{i:02}"), &report).unwrap();
        }
        let count = std::fs::read_dir(&dir).unwrap().count();
        assert_eq!(count, MAX_REPORTS);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn copies_follow_changes_from_the_baseline() {
        let root = temp_dir("session");
        let mut session = RecoverySession::start(&root, "run-1").unwrap();
        let info = RecoveryInfo { name: "Runs".to_owned(), original: Some(PathBuf::from("/data/runs.acraft")) };
        let saved = project(vec![1.0, 2.0, 3.0]);
        session.set_baseline(7, &saved);
        // Unchanged: nothing to write.
        assert!(!session.save(7, &info, &saved).unwrap());
        assert!(!session.project_path(7).exists());
        // A selection alone is not a change.
        let mut selected = saved.clone();
        selected.tables[0].rows.select([0], stat_data::SelectionMode::Replace);
        assert!(!session.save(7, &info, &selected).unwrap());
        // An edit is written once, and read back as the edited project.
        let edited = project(vec![1.0, 2.0, 30.0]);
        assert!(session.save(7, &info, &edited).unwrap());
        assert!(!session.save(7, &info, &edited).unwrap());
        let back = load_project_file(&std::fs::read_to_string(session.project_path(7)).unwrap()).unwrap();
        assert_eq!(back.tables[0].table.column(0).unwrap().as_numeric().unwrap(), &[1.0, 2.0, 30.0]);
        // Undoing the edit removes the copy; saving does too.
        assert!(!session.save(7, &info, &saved).unwrap());
        assert!(!session.project_path(7).exists());
        assert!(session.save(7, &info, &edited).unwrap());
        session.set_baseline(7, &edited);
        assert!(!session.project_path(7).exists());
        // A never-saved project always has a copy.
        assert!(session.save(8, &RecoveryInfo::default(), &saved).unwrap());
        session.forget(8);
        assert!(!session.project_path(8).exists());
        let dir = session.dir().to_path_buf();
        session.finish().unwrap();
        assert!(!dir.exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn sessions_whose_heartbeat_stopped_are_offered_back() {
        let root = temp_dir("orphans");
        let mut crashed = RecoverySession::start(&root, "crashed").unwrap();
        let info = RecoveryInfo { name: "Greenhouse".to_owned(), original: Some(PathBuf::from("/g.acraft")) };
        crashed.save(1, &info, &project(vec![4.0, 5.0])).unwrap();
        note_crash(crashed.dir(), Path::new("/logs/crash-1.txt")).unwrap();
        let empty = RecoverySession::start(&root, "empty").unwrap();
        let alive = RecoverySession::start(&root, "alive").unwrap();
        let mine = RecoverySession::start(&root, "mine").unwrap();
        let now = now_secs();
        // Make the first two look old; `alive` keeps its fresh heartbeat.
        for s in [&crashed, &empty] {
            std::fs::write(s.dir().join(HEARTBEAT), (now - 600).to_string()).unwrap();
        }
        std::fs::write(mine.dir().join(HEARTBEAT), "0").unwrap();
        let found = orphaned_sessions(&root, "mine", now);
        assert_eq!(found.len(), 1, "{found:#?}");
        let s = &found[0];
        assert_eq!(s.crash_report, Some(PathBuf::from("/logs/crash-1.txt")));
        assert_eq!(s.projects.len(), 1);
        assert_eq!(s.projects[0].name, "Greenhouse");
        assert_eq!(s.projects[0].original, Some(PathBuf::from("/g.acraft")));
        assert!(load_project_file(&std::fs::read_to_string(&s.projects[0].file).unwrap()).is_ok());
        // The ended session with nothing in it was cleared away.
        assert!(!empty.dir().exists());
        assert!(alive.dir().exists() && mine.dir().exists());
        s.discard().unwrap();
        assert!(orphaned_sessions(&root, "mine", now).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }
}
