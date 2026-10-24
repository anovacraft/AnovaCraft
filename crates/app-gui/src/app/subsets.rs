//! Saved row subsets (Data ▸ Row subsets, and the sidebar's Rows section):
//! named sets of rows a table keeps, one of which can be active so that
//! every report, plot and summary uses only its rows.

use super::*;

/// The naming window for a new or renamed subset.
pub(super) struct SubsetPrompt {
    /// Document (by id) whose rows are named.
    document: u64,
    /// The subset being renamed (`None`: save the selection as a new one).
    rename: Option<usize>,
    pub(super) name: String,
    /// Why the last name was refused.
    problem: Option<String>,
}

/// The Row subsets submenu: save the selection, switch the active subset,
/// select, rename or delete it.
pub(super) fn subset_menu_items(ui: &mut egui::Ui, doc: &mut Document, prompt: &mut Option<SubsetPrompt>) {
    {
        let rows = &mut doc.rows;
        let any_selected = rows.counts().selected > 0;
        if ui
            .add_enabled(any_selected, egui::Button::new("Save selection as subset…"))
            .on_hover_text("Name the selected rows so you can switch to them later")
            .clicked()
        {
            let name = app_model::table_ops::unique_name("Subset", rows.subset_names().iter().map(String::as_str));
            *prompt = Some(SubsetPrompt { document: doc.id, rename: None, name, problem: None });
            ui.close_menu();
            return;
        }
        ui.separator();
        let active = rows.active_subset();
        if ui.radio(active.is_none(), "All rows").on_hover_text("Analyses use every row (less exclusions)").clicked() {
            rows.set_active_subset(None);
            ui.close_menu();
        }
        for (i, name) in rows.subset_names().to_vec().into_iter().enumerate() {
            let n = rows.subset_rows(i).len();
            if ui.radio(active == Some(i), format!("{name} ({})", count(n, "row", "rows"))).clicked() {
                rows.set_active_subset(Some(i));
                ui.close_menu();
            }
        }
        ui.separator();
        let Some(i) = active else {
            ui.label(RichText::new("Choose a subset to select, rename or delete it").weak());
            return;
        };
        if ui.button("Select the subset's rows").clicked() {
            rows.select(rows.subset_rows(i), stat_data::SelectionMode::Replace);
            ui.close_menu();
        }
        if ui
            .add_enabled(any_selected, egui::Button::new("Replace with the selection"))
            .on_hover_text("The subset keeps its name and holds the selected rows instead")
            .clicked()
        {
            let name = rows.subset_names()[i].clone();
            rows.save_subset(&name, rows.selected_rows());
            ui.close_menu();
        }
        if ui.button("Rename subset…").clicked() {
            let name = rows.subset_names()[i].clone();
            *prompt = Some(SubsetPrompt { document: doc.id, rename: Some(i), name, problem: None });
            ui.close_menu();
        }
        if ui.button("Delete subset").on_hover_text("Forget the subset; analyses use all rows again").clicked() {
            rows.remove_subset(i);
            ui.close_menu();
        }
    }
}

impl StatsApp {
    /// The sidebar's subset line: which subset is active, with a menu to
    /// switch.
    pub(super) fn subset_sidebar(&mut self, ui: &mut egui::Ui) {
        let Some(doc) = self.documents.get_mut(self.active) else { return };
        let rows = &doc.rows;
        let label = match rows.active_subset() {
            Some(i) => format!("Subset: {}", rows.subset_names()[i]),
            None if rows.subset_names().is_empty() => "Subset: all rows".to_owned(),
            None => format!("Subset: all rows ({} saved)", rows.subset_names().len()),
        };
        let outside = rows.counts().outside_subset;
        ui.horizontal(|ui| {
            ui.menu_button(label, |ui| subset_menu_items(ui, doc, &mut self.subset_prompt)).response.on_hover_text(
                if outside > 0 {
                    format!(
                        "{} outside the active subset are left out of analyses and plots",
                        count(outside, "row", "rows")
                    )
                } else {
                    "Save selected rows as a named subset, or switch between saved subsets".to_owned()
                },
            );
        });
    }

    /// The naming window, while one is open.
    pub(super) fn subset_prompt_window(&mut self, ctx: &egui::Context) {
        let Some(prompt) = &mut self.subset_prompt else { return };
        let title = if prompt.rename.is_some() { "Rename subset" } else { "Save selection as subset" };
        let mut done = None;
        egui::Window::new(title).collapsible(false).resizable(false).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Name:");
                let edit = ui.add(egui::TextEdit::singleline(&mut prompt.name).desired_width(kit_em(ui) * 14.0));
                if edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    done = Some(true);
                }
                if !edit.has_focus() && ui.memory(|m| m.focused().is_none()) {
                    edit.request_focus();
                }
            });
            if let Some(problem) = &prompt.problem {
                ui.colored_label(ui.visuals().error_fg_color, problem);
            }
            ui.horizontal(|ui| {
                if ui.button(if prompt.rename.is_some() { "Rename" } else { "Save" }).clicked() {
                    done = Some(true);
                }
                if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
                    done = Some(false);
                }
            });
        });
        match done {
            Some(false) => self.subset_prompt = None,
            Some(true) => {
                let Some(prompt) = self.subset_prompt.as_mut() else { return };
                let name = prompt.name.trim().to_owned();
                let Some(doc) = self.documents.iter_mut().find(|d| d.id == prompt.document) else {
                    self.subset_prompt = None;
                    return;
                };
                let rows = &mut doc.rows;
                let taken = rows.subset_names().iter().enumerate().any(|(i, n)| *n == name && Some(i) != prompt.rename);
                prompt.problem = if name.is_empty() {
                    Some("Give the subset a name".to_owned())
                } else if taken {
                    Some(format!("A subset is already called “{name}”"))
                } else {
                    let ok = match prompt.rename {
                        Some(i) => rows.rename_subset(i, &name),
                        None => rows.save_subset(&name, rows.selected_rows()).is_some(),
                    };
                    (!ok).then(|| format!("A table can keep at most {} subsets", stat_data::MAX_SUBSETS))
                };
                if prompt.problem.is_none() {
                    self.subset_prompt = None;
                }
            }
            None => {}
        }
    }
}

fn kit_em(ui: &egui::Ui) -> f32 {
    crate::dialog_kit::em(ui)
}
