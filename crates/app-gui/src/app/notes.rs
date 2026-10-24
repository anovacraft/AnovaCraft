//! Project notes: free text kept with each project and saved in its file.
//!
//! The sidebar shows the notes of the project the menus act on (the active
//! document's), below the row counts, so working in another project's table
//! switches to that project's notes. The notes can move into a window of
//! their own, which follows the active project in the same way; closing it
//! (or "Back to sidebar") puts them back in the sidebar.

use super::*;

/// Id of the notes window's viewport.
fn notes_viewport() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("project_notes")
}

/// Hint shown in an empty notes box.
const NOTES_HINT: &str = "Notes about this project: what the data are, decisions, things to check. Saved with the \
                          project.";

/// The notes box: a multi-line editor of `notes` at least `rows` lines high,
/// filling the width.
fn notes_editor(ui: &mut egui::Ui, notes: &mut String, rows: usize, salt: impl std::hash::Hash) -> egui::Response {
    ui.add(
        egui::TextEdit::multiline(notes)
            .id_salt(salt)
            .hint_text(NOTES_HINT)
            .desired_rows(rows)
            .desired_width(f32::INFINITY),
    )
    .on_hover_text("Saved with the project; the first line shows beside it in the start page's recent projects")
}

impl StatsApp {
    /// The project whose notes the sidebar and the notes window show: the
    /// active document's.
    fn notes_project(&self) -> Option<u64> {
        self.documents.get(self.active).map(|d| d.project)
    }

    /// The sidebar's Notes section: a heading with a button that moves the
    /// notes into a window of their own, then the notes (or, while they are
    /// in their window, a line saying so and a button to bring them back).
    pub(super) fn notes_section(&mut self, ui: &mut egui::Ui) {
        let Some(project) = self.notes_project() else { return };
        let name = self.project_name(project);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Notes").strong()).on_hover_text(format!("Notes kept with the project “{name}”"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if !self.notes_window && notes_window_button(ui).clicked() {
                    self.notes_window = true;
                }
            });
        });
        if self.notes_window {
            ui.label(RichText::new("In a window of their own").weak().small());
            ui.horizontal(|ui| {
                if ui.small_button("Show window").clicked() {
                    ui.ctx().send_viewport_cmd_to(notes_viewport(), egui::ViewportCommand::Focus);
                }
                if ui.small_button("Back to sidebar").clicked() {
                    self.notes_window = false;
                }
            });
            return;
        }
        let rows = ((ui.available_height() - 8.0) / ui.text_style_height(&egui::TextStyle::Body)).floor();
        if let Some(p) = self.project_mut(project) {
            notes_editor(ui, &mut p.notes, (rows as usize).clamp(3, 30), ("project_notes", project));
        }
    }

    /// The notes window, while the notes are in a window of their own: the
    /// active project's name and its notes filling the window.
    pub(super) fn show_notes_window(&mut self, ctx: &egui::Context) {
        if !self.notes_window {
            return;
        }
        let project = self.notes_project();
        let name = project.map(|p| self.project_name(p));
        let title = name.as_ref().map_or_else(|| "Notes".to_owned(), |n| format!("Notes — {n}"));
        let builder = egui::ViewportBuilder::default()
            .with_title(&title)
            .with_inner_size([420.0, 360.0])
            .with_min_inner_size([240.0, 160.0]);
        let (mut open, mut back) = (true, false);
        ctx.show_viewport_immediate(notes_viewport(), builder, |ctx, class| {
            let mut contents = |ui: &mut egui::Ui| {
                ui.horizontal(|ui| {
                    match &name {
                        Some(name) => ui.label(RichText::new(name).strong()),
                        None => ui.label(RichText::new("No project is open").weak()),
                    };
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .button("Back to sidebar")
                            .on_hover_text("Close this window and show the notes in the sidebar again")
                            .clicked()
                        {
                            back = true;
                        }
                    });
                });
                ui.label(RichText::new("Shows the notes of the project you are working in.").weak().small());
                let Some(project) = project else { return };
                let rows = ((ui.available_height() - 8.0) / ui.text_style_height(&egui::TextStyle::Body)).floor();
                if let Some(p) = self.project_mut(project) {
                    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                        notes_editor(ui, &mut p.notes, (rows as usize).max(3), ("project_notes_window", project));
                    });
                }
            };
            if class == egui::ViewportClass::Embedded {
                egui::Window::new(&title)
                    .id(egui::Id::new("project_notes_window"))
                    .open(&mut open)
                    .default_size([420.0, 360.0])
                    .collapsible(false)
                    .resizable(true)
                    .show(ctx, |ui| contents(ui));
            } else {
                egui::CentralPanel::default().show(ctx, |ui| contents(ui));
                if ctx.input(|i| i.viewport().close_requested()) {
                    open = false;
                }
            }
        });
        if !open || back {
            self.notes_window = false;
        }
    }
}

/// The small button beside the Notes heading that moves the notes into a
/// window of their own: the window glyph, named for screen readers and tests.
fn notes_window_button(ui: &mut egui::Ui) -> egui::Response {
    let response = ui.small_button("\u{2003}").on_hover_text("Show the notes in a window of their own");
    let size = 12.0 * theme::text_scale(ui);
    let glyph = egui::Rect::from_center_size(response.rect.center(), egui::vec2(size, size));
    let color = ui.style().interact(&response).text_color();
    theme::paint_window_glyph(ui.painter(), glyph, color);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Notes in their own window"));
    response
}
