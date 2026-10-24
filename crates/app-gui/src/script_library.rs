//! The sidebar's script library: the `.rhai` files in the scripts folder
//! (chosen once, kept in the settings) and the scripts kept in the active
//! project. Clicking a script opens it in the Script window; right-click
//! runs it, or keeps a folder script in the project.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use app_model::project::ProjectScript;
use eframe::egui::{self, RichText};

/// How often the folder is listed again while the section is open.
const REFRESH: Duration = Duration::from_secs(3);

/// What to do with a script file of the library.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryOp {
    /// Show it in the Script window.
    Open,
    /// Show it in the Script window and run it.
    Run,
    /// Copy it into the active document's project.
    KeepInProject,
}

/// What the section asks the application to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LibraryRequest {
    /// Ask for the scripts folder.
    ChooseFolder,
    /// Act on a script file.
    File(PathBuf, LibraryOp),
    /// Act on script `.0` of the active project (`ProjectScriptOp` by name:
    /// open, run or remove).
    Project(usize, ProjectRequest),
}

/// What to do with a script kept in the active project.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectRequest {
    Open,
    Run,
    Remove,
}

/// The listed folder, cached between frames.
#[derive(Default)]
pub struct ScriptLibrary {
    folder: Option<PathBuf>,
    files: Vec<PathBuf>,
    error: Option<String>,
    listed: Option<Instant>,
}

impl ScriptLibrary {
    /// List `folder` again on the next frame.
    pub fn refresh(&mut self) {
        self.listed = None;
    }

    fn list(&mut self, folder: Option<&Path>) {
        let stale = self.listed.is_none_or(|t| t.elapsed() > REFRESH);
        if self.folder.as_deref() == folder && !stale {
            return;
        }
        self.folder = folder.map(Path::to_path_buf);
        self.listed = Some(Instant::now());
        match folder.map(stat_script::script_files) {
            None => (self.files, self.error) = (Vec::new(), None),
            Some(Ok(files)) => (self.files, self.error) = (files, None),
            Some(Err(e)) => (self.files, self.error) = (Vec::new(), Some(e)),
        }
    }

    /// Draw the section: the folder's scripts, then the active project's
    /// (`project`: its name and scripts).
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        folder: Option<&Path>,
        project: Option<(&str, &[ProjectScript])>,
    ) -> Vec<LibraryRequest> {
        let mut requests = Vec::new();
        let response = egui::CollapsingHeader::new(RichText::new("Scripts").strong())
            .id_salt("script_library")
            .default_open(false)
            .show(ui, |ui| {
                self.list(folder);
                ui.horizontal(|ui| {
                    let shown = folder
                        .and_then(Path::file_name)
                        .map_or_else(|| "No scripts folder".to_owned(), |n| format!("Folder: {}", n.to_string_lossy()));
                    let tip = folder.map_or_else(
                        || "Choose a folder of .rhai scripts to list here".to_owned(),
                        |f| f.display().to_string(),
                    );
                    ui.label(RichText::new(shown).weak()).on_hover_text(tip);
                    if ui.small_button("Choose…").on_hover_text("Choose the scripts folder").clicked() {
                        requests.push(LibraryRequest::ChooseFolder);
                    }
                    if folder.is_some() && ui.small_button("Refresh").on_hover_text("List the folder again").clicked() {
                        self.refresh();
                    }
                });
                if let Some(e) = &self.error {
                    ui.label(RichText::new(format!("Error: {e}")).color(ui.visuals().error_fg_color).small());
                }
                if folder.is_some() && self.files.is_empty() && self.error.is_none() {
                    ui.label(RichText::new("No .rhai files in the folder").weak());
                }
                egui::ScrollArea::vertical().id_salt("script_library_scroll").max_height(140.0).show(ui, |ui| {
                    for path in &self.files {
                        let name = path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        let item = ui.selectable_label(false, name).on_hover_text(format!(
                            "{}\nClick to open in the Script window; right-click to run it or keep it in the project",
                            path.display()
                        ));
                        if item.clicked() {
                            requests.push(LibraryRequest::File(path.clone(), LibraryOp::Open));
                        }
                        item.context_menu(|ui| {
                            for (label, op) in [
                                ("Run", LibraryOp::Run),
                                ("Open in Script window", LibraryOp::Open),
                                ("Keep in project", LibraryOp::KeepInProject),
                            ] {
                                let enabled = op != LibraryOp::KeepInProject || project.is_some();
                                if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
                                    requests.push(LibraryRequest::File(path.clone(), op));
                                    ui.close_menu();
                                }
                            }
                        });
                    }
                    if let Some((table, scripts)) = project {
                        ui.label(RichText::new(format!("In project “{table}”")).weak().small());
                        if scripts.is_empty() {
                            ui.label(RichText::new("None kept — right-click a script to keep it").weak().small());
                        }
                        for (i, script) in scripts.iter().enumerate() {
                            let mark = if script.run_on_open { " (runs on open)" } else { "" };
                            let item = ui
                                .selectable_label(false, format!("{}{mark}", script.name))
                                .on_hover_text("Kept in the project. Click to open; right-click to run or remove");
                            if item.clicked() {
                                requests.push(LibraryRequest::Project(i, ProjectRequest::Open));
                            }
                            item.context_menu(|ui| {
                                for (label, op) in [
                                    ("Run", ProjectRequest::Run),
                                    ("Open in Script window", ProjectRequest::Open),
                                    ("Remove from project", ProjectRequest::Remove),
                                ] {
                                    if ui.button(label).clicked() {
                                        requests.push(LibraryRequest::Project(i, op));
                                        ui.close_menu();
                                    }
                                }
                            });
                        }
                    }
                });
            });
        response.header_response.on_hover_text("Scripts in your scripts folder and in the active project");
        requests
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_the_folder_and_notices_a_change_of_folder() {
        let dir = std::env::temp_dir().join(format!("anovacraft-library-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("summary.rhai"), "1").unwrap();
        let mut lib = ScriptLibrary::default();
        lib.list(Some(&dir));
        assert_eq!(lib.files, [dir.join("summary.rhai")]);
        std::fs::write(dir.join("another.rhai"), "1").unwrap();
        // Cached until refreshed.
        lib.list(Some(&dir));
        assert_eq!(lib.files.len(), 1);
        lib.refresh();
        lib.list(Some(&dir));
        assert_eq!(lib.files.len(), 2);
        lib.list(None);
        assert!(lib.files.is_empty() && lib.error.is_none());
        lib.list(Some(&dir.join("missing")));
        assert!(lib.error.is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
