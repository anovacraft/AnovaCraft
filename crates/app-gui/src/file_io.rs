//! The native file dialogs, and project writing. Reading and writing data
//! files lives in [`app_model::files`].

use std::path::{Path, PathBuf};

use app_model::file_formats::{ExportFormat, OPEN_EXTENSIONS};
use app_model::image_table::IMAGE_EXTENSIONS;
use app_model::project::{PROJECT_EXTENSION, ProjectFile, save_project_file};

pub use app_model::files::{export_table, open_path};

#[cfg(target_arch = "wasm32")]
use crate::web_shims::rfd;

/// Write a one-table project file (tests write fixtures with it).
#[cfg(test)]
pub fn write_project(project: &app_model::project::Project, path: &Path) -> Result<(), String> {
    std::fs::write(path, app_model::project::save_project(project))
        .map_err(|e| format!("could not write {}: {e}", path.display()))
}

/// Write a project file of any number of tables (a project of one table
/// with no saved layout is written in the one-table form).
pub fn write_project_file(file: &ProjectFile, path: &Path) -> Result<(), String> {
    std::fs::write(path, save_project_file(file)).map_err(|e| format!("could not write {}: {e}", path.display()))
}

/// Ask for files to open.
pub fn pick_files_to_open() -> Option<Vec<PathBuf>> {
    let all: Vec<&str> = OPEN_EXTENSIONS.iter().chain(&IMAGE_EXTENSIONS).copied().collect();
    rfd::FileDialog::new()
        .set_title("Open data or project")
        .add_filter("All supported files", &all)
        .add_filter("Projects", &[PROJECT_EXTENSION])
        .add_filter("Delimited text", &["csv", "tsv", "tab", "txt", "dat"])
        .add_filter("JSON", &["json"])
        .add_filter("Spreadsheets", &["xlsx", "xlsm", "xlsb", "xls", "ods"])
        .add_filter("Images", &IMAGE_EXTENSIONS)
        .pick_files()
}

/// Kinds of data file the start page's Import chips ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    /// Delimited text.
    Text,
    /// Spreadsheet workbooks.
    Workbook,
}

/// Ask for files of one kind to open.
pub fn pick_files_of_kind(kind: FileKind) -> Option<Vec<PathBuf>> {
    let (title, label, extensions): (&str, &str, &[&str]) = match kind {
        FileKind::Text => ("Import text", "Delimited text", &["csv", "tsv", "tab", "txt", "dat"]),
        FileKind::Workbook => ("Import a workbook", "Spreadsheets", &["xlsx", "xlsm", "xlsb", "xls", "ods"]),
    };
    rfd::FileDialog::new().set_title(title).add_filter(label, extensions).pick_files()
}

/// Ask where to save a project, suggesting `name`.
pub fn pick_project_destination(name: &str) -> Option<PathBuf> {
    let path = rfd::FileDialog::new()
        .set_title("Save project")
        .add_filter("Project", &[PROJECT_EXTENSION])
        .set_file_name(format!("{}.{PROJECT_EXTENSION}", safe_file_stem(name)))
        .save_file()?;
    Some(with_extension(path, PROJECT_EXTENSION))
}

/// Ask where to export data in `format`, suggesting `name`.
pub fn pick_export_destination(name: &str, format: ExportFormat) -> Option<PathBuf> {
    let path = rfd::FileDialog::new()
        .set_title("Export data")
        .add_filter(format.label(), &[format.extension()])
        .set_file_name(format!("{}.{}", safe_file_stem(name), format.extension()))
        .save_file()?;
    Some(with_extension(path, format.extension()))
}

/// Ask where to save a file of one kind (`label`, `extension`), suggesting
/// `name`.
pub fn pick_save_file(title: &str, label: &str, extension: &str, name: &str) -> Option<PathBuf> {
    #[cfg(test)]
    if let Some(answer) = test_save_answer::take() {
        return answer.map(|p| with_extension(p, extension));
    }
    let path = rfd::FileDialog::new()
        .set_title(title)
        .add_filter(label, &[extension])
        .set_file_name(format!("{}.{extension}", safe_file_stem(name)))
        .save_file()?;
    Some(with_extension(path, extension))
}

/// Ask for a folder (`title` names what it is for).
pub fn pick_folder(title: &str) -> Option<PathBuf> {
    #[cfg(test)]
    if let Some(answer) = test_save_answer::take() {
        return answer;
    }
    rfd::FileDialog::new().set_title(title).pick_folder()
}

/// Ask for one file to open of the given kind.
pub fn pick_open_file(title: &str, label: &str, extensions: &[&str]) -> Option<PathBuf> {
    #[cfg(test)]
    if let Some(answer) = test_save_answer::take() {
        return answer;
    }
    rfd::FileDialog::new().set_title(title).add_filter(label, extensions).pick_file()
}

/// Add `extension` unless the path already has it (case-insensitively).
fn with_extension(path: PathBuf, extension: &str) -> PathBuf {
    let has = path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case(extension));
    if has {
        path
    } else {
        let mut s = path.into_os_string();
        s.push(".");
        s.push(extension);
        PathBuf::from(s)
    }
}

/// A file-name-safe version of a table name.
fn safe_file_stem(name: &str) -> String {
    let s: String = name.chars().map(|c| if "/\\:*?\"<>|".contains(c) { '_' } else { c }).collect();
    if s.trim().is_empty() { "Untitled".to_owned() } else { s.trim().to_owned() }
}

/// The local path of a `file://` URL (`file:///Users/a/My%20File.acraft`,
/// optionally with a host such as `localhost`), with percent-escapes decoded;
/// `None` for other URLs or malformed escapes.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))] // used by the macOS Finder bridge
pub fn path_from_file_url(url: &str) -> Option<PathBuf> {
    let rest = url.trim_end_matches('\0').strip_prefix("file://")?;
    let path = &rest[rest.find('/')?..];
    let bytes = path.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    let decoded = String::from_utf8(out).ok()?;
    let trimmed = decoded.trim_end_matches('/');
    Some(PathBuf::from(if trimmed.is_empty() { "/" } else { trimmed }))
}

/// Tests answer [`pick_save_file`] instead of a native dialog: the next call
/// returns the path set here (or `None`, as if cancelled) and records the
/// dialog's title.
#[cfg(test)]
pub mod test_save_answer {
    use std::cell::RefCell;
    use std::path::PathBuf;

    thread_local! {
        static ANSWER: RefCell<Option<Option<PathBuf>>> = const { RefCell::new(None) };
    }

    /// Answer the next save, folder or open dialog with `path` (`None`: cancel).
    pub fn set(path: Option<PathBuf>) {
        ANSWER.with(|a| *a.borrow_mut() = Some(path));
    }

    /// The pending answer, if a test set one; `true` while unused.
    pub fn pending() -> bool {
        ANSWER.with(|a| a.borrow().is_some())
    }

    pub(super) fn take() -> Option<Option<PathBuf>> {
        ANSWER.with(|a| a.borrow_mut().take())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_urls_become_paths() {
        let p = |u: &str| path_from_file_url(u);
        assert_eq!(p("file:///Users/a/My%20File.acraft"), Some(PathBuf::from("/Users/a/My File.acraft")));
        assert_eq!(p("file://localhost/tmp/x.csv"), Some(PathBuf::from("/tmp/x.csv")));
        assert_eq!(p("file:///tmp/%C3%A9t%C3%A9.acraft\0"), Some(PathBuf::from("/tmp/été.acraft")));
        assert_eq!(p("file:///tmp/dir/"), Some(PathBuf::from("/tmp/dir")));
        assert_eq!(p("https://example.com/x"), None);
        assert_eq!(p("file:///bad%2"), None);
        assert_eq!(p("file:///bad%zz"), None);
    }
}
