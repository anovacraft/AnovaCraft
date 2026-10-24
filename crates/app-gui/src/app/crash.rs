//! Panics: the crash-report hook, and running an analysis so that a panic
//! inside it becomes that report's error instead of closing the program.
//!
//! The reports and recovery folders are described in
//! [`app_model::recovery`].

use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind};
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

thread_local! {
    /// How many [`guarded`] calls are running on this thread.
    static GUARDED: Cell<u32> = const { Cell::new(0) };
}

/// Set when a panic outside [`guarded`] is ending the program.
static CRASHED: AtomicBool = AtomicBool::new(false);

/// This run's recovery folder, where the hook notes the crash report.
#[cfg(not(target_arch = "wasm32"))]
static SESSION_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Whether the program is ending in a panic (so the recovery copies must be
/// kept for the next launch).
pub fn crashed() -> bool {
    CRASHED.load(Ordering::Relaxed) || std::thread::panicking()
}

/// Tell the hook which folder holds this run's recovery copies.
#[cfg(not(target_arch = "wasm32"))]
pub fn set_session_dir(dir: PathBuf) {
    let _ = SESSION_DIR.set(dir);
}

/// The text of a panic payload.
pub fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown error".to_owned())
}

/// Run `f`, turning a panic into `Err` with a message for the report.
pub fn guarded<T>(f: impl FnOnce() -> T) -> Result<T, String> {
    GUARDED.with(|g| g.set(g.get() + 1));
    let result = catch_unwind(AssertUnwindSafe(f));
    GUARDED.with(|g| g.set(g.get().saturating_sub(1)));
    result.map_err(|payload| {
        format!(
            "The analysis stopped with an internal error ({}). This is a bug; the details were written to the \
             crash reports folder (Help ▸ Show crash reports).",
            panic_text(payload.as_ref())
        )
    })
}

/// Install the hook that writes a crash report for every panic (marked as
/// caught for panics inside [`guarded`]) before the standard message.
#[cfg(not(target_arch = "wasm32"))]
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let caught = GUARDED.with(|g| g.get() > 0);
        if !caught {
            CRASHED.store(true, Ordering::Relaxed);
        }
        let now = chrono::Local::now();
        let report = app_model::recovery::CrashReport {
            message: panic_text(info.payload()),
            location: info.location().map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column())),
            thread: std::thread::current().name().map(str::to_owned),
            backtrace: std::backtrace::Backtrace::force_capture().to_string(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            when: now.format("%Y-%m-%d %H:%M:%S %:z").to_string(),
            caught,
        };
        if let Some(dir) = app_model::recovery::log_dir() {
            let stamp = now.format("%Y-%m-%d-%H%M%S").to_string();
            match app_model::recovery::write_crash_report(&dir, &stamp, &report) {
                Ok(path) => {
                    if let (false, Some(session)) = (caught, SESSION_DIR.get()) {
                        let _ = app_model::recovery::note_crash(session, &path);
                    }
                    eprintln!("Crash report written to {}", path.display());
                }
                Err(e) => eprintln!("Could not write a crash report to {}: {e}", dir.display()),
            }
        }
        previous(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guarded_turns_a_panic_into_an_error() {
        assert_eq!(guarded(|| 4), Ok(4));
        let e = guarded(|| -> usize {
            let v: Vec<usize> = Vec::new();
            v[3]
        })
        .unwrap_err();
        assert!(e.contains("internal error") && e.contains("index out of bounds"), "{e}");
        let e = guarded(|| panic!("{}", String::from("formatted"))).unwrap_err();
        assert!(e.contains("formatted"));
        // The count is back to zero, so later panics count as crashes.
        assert_eq!(GUARDED.with(Cell::get), 0);
    }
}
