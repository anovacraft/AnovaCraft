//! Stand-ins for the native file dialogs and system clipboard in the web
//! dashboard viewer (wasm32), where neither exists. The viewer is read-only
//! and never offers the commands that use them; if one is reached anyway it
//! behaves as if the person cancelled.

/// The file-dialog subset of `rfd` the app uses; every pick is cancelled.
pub mod rfd {
    use std::path::{Path, PathBuf};

    /// A file dialog that never opens.
    #[derive(Default)]
    pub struct FileDialog;

    impl FileDialog {
        pub fn new() -> Self {
            Self
        }
        pub fn set_title(self, _: &str) -> Self {
            self
        }
        pub fn add_filter(self, _: &str, _: &[&str]) -> Self {
            self
        }
        pub fn set_file_name(self, _: impl Into<String>) -> Self {
            self
        }
        pub fn set_directory(self, _: impl AsRef<Path>) -> Self {
            self
        }
        pub fn pick_file(self) -> Option<PathBuf> {
            None
        }
        pub fn pick_files(self) -> Option<Vec<PathBuf>> {
            None
        }
        pub fn pick_folder(self) -> Option<PathBuf> {
            None
        }
        pub fn save_file(self) -> Option<PathBuf> {
            None
        }
    }
}

/// The clipboard-reading subset of `arboard`; there is no clipboard to read.
pub mod arboard {
    /// Why the clipboard could not be read.
    #[derive(Debug)]
    pub struct Error;

    impl std::fmt::Display for Error {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("no clipboard in the web viewer")
        }
    }

    /// A clipboard that cannot be opened.
    pub struct Clipboard;

    impl Clipboard {
        pub fn new() -> Result<Self, Error> {
            Err(Error)
        }
        pub fn get_text(&mut self) -> Result<String, Error> {
            Err(Error)
        }
    }
}
