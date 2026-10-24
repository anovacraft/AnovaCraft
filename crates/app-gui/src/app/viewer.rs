//! The read-only dashboard viewer: a published project's dashboards shown
//! full window, with no menus, sidebar or tabs. This is what the web build
//! runs (`web_main`); it is a mode of [`StatsApp`] so the viewer draws the
//! very same tiles, reports and plots as the desktop app.
//!
//! Filters, column pickers, brushing and display options work as in the
//! app, recomputed in place; tiles cannot be added, moved, resized, removed
//! or edited, nothing creates columns, reports or tables, and nothing is
//! saved.

use super::*;

/// The viewer's own state.
pub(super) struct Viewer {
    /// The published project's name, shown in the top bar.
    pub title: String,
}

impl StatsApp {
    /// A viewer of project `file` named `title`, showing the dashboard its
    /// layout shows (else its first).
    pub fn new_viewer(cc: &eframe::CreationContext<'_>, file: ProjectFile, title: String) -> Self {
        let mut app = Self::new(cc, Vec::new());
        // A read-only viewer has nothing to recover.
        (app.recovery, app.recovery_offer) = (None, None);
        app.load_viewer(file, title);
        app
    }

    /// Open `file` as the viewer's project.
    pub(super) fn load_viewer(&mut self, file: ProjectFile, title: String) {
        self.open_project_file(file, NewProject { title: Some(title.clone()), ..NewProject::default() });
        // A viewer has one window: views the layout put in windows of their own come back.
        let windowed = self.windows.iter().find_map(|w| match w.view {
            View::Dashboard(id) => Some(id),
            _ => None,
        });
        self.windows.clear();
        for project in &mut self.projects {
            for d in &mut project.dashboards {
                d.read_only = true;
            }
        }
        if self.shown_dashboard().is_none() {
            if let Some(first) = windowed.or_else(|| self.viewer_dashboards().first().map(|(id, _)| *id)) {
                self.show_dashboard(first);
            }
        }
        self.message = None;
        self.viewer = Some(Viewer { title });
    }

    /// Every dashboard of the viewer's project: id and name.
    pub(super) fn viewer_dashboards(&self) -> Vec<(u64, String)> {
        self.projects.iter().flat_map(|p| p.dashboards.iter().map(|d| (d.id, d.dashboard.name.clone()))).collect()
    }

    /// One frame of the viewer: the top bar (project name and, when there
    /// are several, a choice of dashboard) and the dashboard shown.
    pub(super) fn viewer_frame(&mut self, ctx: &egui::Context) {
        let Some(viewer) = &self.viewer else { return };
        let title = viewer.title.clone();
        self.publish_display_settings(ctx);
        ctx.data_mut(|d| d.insert_temp(report_view::read_only_id(), true));
        for id in self.on_screen_documents() {
            self.service_document(ctx, id);
        }
        let dashboards = self.viewer_dashboards();
        let shown = self.shown_dashboard();
        let message = self.message.clone();
        let mut choose = None;
        let mut dismiss = false;
        egui::TopBottomPanel::top("viewer_bar").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(&title).strong());
                if dashboards.len() > 1 {
                    ui.separator();
                    for (id, name) in &dashboards {
                        if ui.selectable_label(shown == Some(*id), name).clicked() {
                            choose = Some(*id);
                        }
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new("Published with AnovaCraft · changes are not saved").weak().small())
                        .on_hover_text(
                            "Filters, column pickers and selections recompute the tiles here in the browser; \
                             reloading the page starts again from the published project",
                        );
                });
            });
            if let Some((text, error)) = &message {
                ui.horizontal_wrapped(|ui| {
                    if *error {
                        ui.colored_label(theme::ERROR, text);
                    } else {
                        ui.label(RichText::new(text).weak());
                    }
                    if ui.small_button("×").on_hover_text("Dismiss").clicked() {
                        dismiss = true;
                    }
                });
            }
            ui.add_space(2.0);
        });
        if dismiss {
            self.message = None;
        }
        if let Some(id) = choose {
            self.show_dashboard(id);
        }
        egui::CentralPanel::default().show(ctx, |ui| match self.shown_dashboard() {
            Some(id) => {
                // Nothing the viewer draws asks for an action (no menus, windows or dialogs).
                let mut actions = Vec::new();
                ui.push_id(("viewer_dashboard", id), |ui| self.dashboard_view(ui, id, true, &mut actions));
            }
            None => {
                ui.centered_and_justified(|ui| {
                    ui.label(RichText::new("This project has no dashboard to show.").weak());
                });
            }
        });
    }
}

/// Publish the Greenhouse sample project's dashboard to `folder` (created
/// if missing), as "Publish dashboard as web app…" would: what
/// `scripts/build-web-viewer.sh --serve` shows. Fails when the web viewer
/// has not been built.
#[cfg(not(target_arch = "wasm32"))]
pub fn publish_sample(folder: &std::path::Path) -> Result<(), String> {
    let bundle = super::dashboards::viewer_bundle()
        .ok_or("the web viewer has not been built: run scripts/build-web-viewer.sh first")?;
    let file = app_model::sample::greenhouse_project();
    let Some(first) = file.dashboards.first().map(|d| d.id.clone()) else {
        return Err("the sample project has no dashboard".to_owned());
    };
    let title = format!("{} · Greenhouse trial", file.dashboards[0].name);
    std::fs::create_dir_all(folder).map_err(|e| format!("could not create {}: {e}", folder.display()))?;
    let file = app_model::web_publish::published_project(file, &first);
    super::dashboards::write_published_folder(folder, &title, &file, &bundle)
}
