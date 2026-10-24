//! The two manuals the app carries as PDFs (built from `docs/guide` and
//! `docs/manual` by `scripts/build-manual-pdfs.sh`): Help ▸ Quick guide and
//! Help ▸ Reference manual write the embedded PDF to the app's data folder
//! and open it in the system's PDF viewer, so they work offline and show the
//! books rather than their Markdown sources.

#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;
use std::path::PathBuf;

/// Where the manuals' Markdown sources can be read online (About window).
pub(super) const ONLINE_URL: &str = "https://github.com/anovacraft/AnovaCraft/tree/main/docs";

/// One of the two manuals.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Manual {
    /// Every menu item in plain language, for new users.
    QuickGuide,
    /// Every option and method in detail.
    Reference,
}

impl Manual {
    /// The manual's title, as on its cover and in the Help menu.
    pub(super) fn title(self) -> &'static str {
        match self {
            Self::QuickGuide => "AnovaCraft Quick Guide",
            Self::Reference => "AnovaCraft Reference Manual",
        }
    }

    /// The PDF built into the app (the web viewer carries none).
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn pdf(self) -> &'static [u8] {
        match self {
            Self::QuickGuide => include_bytes!("../../assets/manuals/quick-guide.pdf"),
            Self::Reference => include_bytes!("../../assets/manuals/reference-manual.pdf"),
        }
    }

    /// The file the PDF is written to: the title and a hash of the contents,
    /// so a newer build of the app writes a new file next to (and then
    /// removes) the old one rather than reusing it.
    #[cfg(not(target_arch = "wasm32"))]
    fn file_name(self) -> String {
        format!("{} {:016x}.pdf", self.title(), fnv1a(self.pdf()))
    }
}

/// The folder the manuals are written to: `manuals` beside the settings
/// file (a temporary folder in tests).
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn folder() -> Option<PathBuf> {
    if cfg!(test) {
        return Some(std::env::temp_dir().join(format!("anovacraft-test-manuals-{}", std::process::id())));
    }
    Some(app_model::settings::settings_path()?.parent()?.join("manuals"))
}

/// Write `manual` into `dir` (unless an identical copy is there already),
/// remove older versions of it, and return the file's path.
///
/// # Errors
/// The folder could not be created or the file could not be written.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn write(manual: Manual, dir: &Path) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let name = manual.file_name();
    let path = dir.join(&name);
    let bytes = manual.pdf();
    let current = std::fs::metadata(&path).is_ok_and(|m| m.len() == bytes.len() as u64);
    if !current {
        // Write beside it and rename, so a viewer never opens half a file.
        let partial = dir.join(format!("{name}.part{}", std::process::id()));
        std::fs::write(&partial, bytes)?;
        std::fs::rename(&partial, &path)?;
    }
    if let Ok(entries) = std::fs::read_dir(dir) {
        let prefix = format!("{} ", manual.title());
        for entry in entries.flatten() {
            let other = entry.file_name().to_string_lossy().into_owned();
            if other != name && other.starts_with(&prefix) && other.ends_with(".pdf") {
                // An older version; a viewer may still hold it open, which is fine.
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    Ok(path)
}

/// Write `manual` to the data folder and open it in the system's PDF viewer.
///
/// # Errors
/// A message for the status bar: no data folder, the file could not be
/// written, or no viewer could be started.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn open(manual: Manual) -> Result<PathBuf, String> {
    let dir = folder().ok_or("Could not find a folder for the manual (no home folder is set)")?;
    let path = write(manual, &dir).map_err(|e| format!("Could not save the {}: {e}", manual.title()))?;
    launch(&path).map_err(|e| format!("Could not open {} in a PDF viewer: {e}", path.display()))?;
    Ok(path)
}

/// The web viewer has no files to open.
#[cfg(target_arch = "wasm32")]
pub(super) fn open(manual: Manual) -> Result<PathBuf, String> {
    Err(format!("The {} opens in the desktop app; its pages are online at {ONLINE_URL}", manual.title()))
}

/// Open `path` with the system's default application.
#[cfg(all(not(test), not(target_arch = "wasm32")))]
pub(super) fn launch(path: &Path) -> std::io::Result<()> {
    use std::process::Command;
    let mut command = if cfg!(target_os = "macos") {
        Command::new("open")
    } else if cfg!(target_os = "windows") {
        // `start` is a shell built-in; its first quoted argument is a title.
        let mut c = Command::new("cmd");
        c.args(["/C", "start", ""]);
        c
    } else {
        Command::new("xdg-open")
    };
    command.arg(path).spawn().map(drop)
}

/// Tests record the file instead of starting a viewer.
#[cfg(test)]
pub(super) fn launch(path: &Path) -> std::io::Result<()> {
    LAUNCHED.with(|l| l.borrow_mut().push(path.to_path_buf()));
    Ok(())
}

#[cfg(test)]
thread_local! {
    /// The files tests would have opened, in order.
    pub(super) static LAUNCHED: std::cell::RefCell<Vec<PathBuf>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// 64-bit FNV-1a hash of `bytes`.
#[cfg(not(target_arch = "wasm32"))]
fn fnv1a(bytes: &[u8]) -> u64 {
    fnv1a_continue(0xcbf2_9ce4_8422_2325, bytes)
}

#[cfg(any(test, not(target_arch = "wasm32")))]
fn fnv1a_continue(mut h: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// The docs stamp of the manuals' sources under `root` (the repository),
/// as `scripts/manual-stamp.mjs` computes it: FNV-1a over each source file's
/// path, a zero byte, its length (u64, little-endian) and its contents (text
/// without carriage returns), in path order, skipping hidden files.
#[cfg(test)]
pub(super) fn docs_stamp(root: &Path) -> std::io::Result<String> {
    const SOURCES: [&str; 8] = [
        "docs/formulas.md",
        "docs/guide/book.toml",
        "docs/guide/pictures.css",
        "docs/guide/src",
        "docs/manual/book.toml",
        "docs/manual/pictures.css",
        "docs/manual/src",
        "docs/scripting.md",
    ];
    const TEXT: [&str; 8] = ["md", "toml", "css", "txt", "js", "hbs", "html", "svg"];
    fn collect(root: &Path, rel: &str, out: &mut Vec<String>) -> std::io::Result<()> {
        let full = root.join(rel);
        if full.is_dir() {
            for entry in std::fs::read_dir(&full)? {
                let name = entry?.file_name().to_string_lossy().into_owned();
                if !name.starts_with('.') {
                    collect(root, &format!("{rel}/{name}"), out)?;
                }
            }
        } else {
            out.push(rel.to_owned());
        }
        Ok(())
    }
    let mut paths = Vec::new();
    for source in SOURCES {
        collect(root, source, &mut paths)?;
    }
    paths.sort();
    let mut h = 0xcbf2_9ce4_8422_2325;
    for rel in paths {
        let mut data = std::fs::read(root.join(&rel))?;
        let ext = Path::new(&rel).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        if TEXT.contains(&ext.as_str()) {
            data.retain(|&b| b != b'\r');
        }
        h = fnv1a_continue(h, rel.as_bytes());
        h = fnv1a_continue(h, &[0]);
        h = fnv1a_continue(h, &(data.len() as u64).to_le_bytes());
        h = fnv1a_continue(h, &data);
    }
    Ok(format!("{h:016x}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_manuals_match_their_sources() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let stamp = docs_stamp(&root).unwrap();
        let built = include_str!("../../assets/manuals/docs-stamp.txt").trim();
        assert_eq!(
            built, stamp,
            "the manuals' Markdown changed since their PDFs were built: run ./scripts/build-manual-pdfs.sh \
             and commit crates/app-gui/assets/manuals"
        );
    }

    #[test]
    fn embedded_manuals_are_pdfs() {
        for manual in [Manual::QuickGuide, Manual::Reference] {
            let pdf = manual.pdf();
            assert!(pdf.starts_with(b"%PDF-"), "{manual:?}");
            assert!(pdf.len() > 100_000, "{manual:?}");
            assert!(pdf[pdf.len() - 1024..].windows(5).any(|w| w == b"%%EOF"), "{manual:?}");
        }
    }

    #[test]
    fn writing_keeps_one_current_copy() {
        let dir = std::env::temp_dir().join(format!("anovacraft-manual-write-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let old = dir.join("AnovaCraft Quick Guide 0000000000000000.pdf");
        std::fs::write(&old, b"old").unwrap();
        let other = dir.join("notes.txt");
        std::fs::write(&other, b"keep").unwrap();
        let path = write(Manual::QuickGuide, &dir).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), Manual::QuickGuide.pdf());
        assert!(!old.exists(), "the older version is removed");
        assert!(other.exists(), "other files stay");
        // Writing again finds the copy already there.
        assert_eq!(write(Manual::QuickGuide, &dir).unwrap(), path);
        let reference = write(Manual::Reference, &dir).unwrap();
        assert!(path.exists() && reference.exists(), "the two manuals live side by side");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
