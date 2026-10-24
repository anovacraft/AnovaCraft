//! Reusing plots and reports elsewhere: pasting a copied plot style,
//! "Duplicate on another table…" and project templates (File ▸ Save
//! project as template…, File ▸ New project from template…).
//!
//! The work is done by `app_model::style_paste`,
//! `app_model::report_transfer` and `app_model::project_template`; this
//! module holds the dialogs, which ask for the target table (or data files)
//! and for the column standing in for each column whose name differs.

use std::path::PathBuf;

use app_model::project_template::{ProjectTemplate, TEMPLATE_EXTENSION, merge_projects, template_text};
use app_model::report_transfer::{ColumnMapping, ColumnNeed, duplicate_report, report_needs};

use super::*;
use crate::dialog_kit::{ComboExt, Dialog, Outcome, ready};
use crate::plot_style_menu::{PasteTarget, StyleRequest};

/// The Duplicate on another table dialog.
pub(super) struct DuplicateDialog {
    /// The document whose report is copied (by id).
    source: u64,
    /// The report's title.
    title: String,
    /// The report as it was when the dialog opened.
    saved: app_model::SavedReport,
    /// The columns it reads.
    needs: Vec<ColumnNeed>,
    /// The table chosen (document id) and the mapping onto it.
    target: Option<(u64, ColumnMapping)>,
}

/// Where a template table's data comes from.
pub(super) enum TemplateSource {
    /// An open table (document id).
    Open(u64),
    /// A data file read for the template.
    File(PathBuf, DataTable),
}

/// The New project from template dialog.
pub(super) struct TemplateDialog {
    /// The template's file name, which names the new project.
    name: String,
    template: ProjectTemplate,
    /// Each data file of a folder becomes a table (one-table templates).
    folder: Option<(PathBuf, Vec<(PathBuf, DataTable)>)>,
    /// Use the folder rather than one table.
    use_folder: bool,
    /// Each template table's data and mapping.
    sources: Vec<Option<(TemplateSource, ColumnMapping)>>,
    /// The folder's mapping (onto its first file).
    folder_mapping: Option<ColumnMapping>,
    /// Problems reading files, shown in the dialog.
    message: Option<String>,
}

/// "Name (numeric, continuous)".
fn need_label(need: &ColumnNeed) -> String {
    format!("{} ({}, {})", need.name, need.kind.label(), app_model::project::level_name(need.level))
}

/// One row per need: its name and a list of `table`'s columns to choose
/// from. Returns `true` when a choice changed.
fn mapping_grid(ui: &mut egui::Ui, salt: impl std::hash::Hash, mapping: &mut ColumnMapping, table: &DataTable) -> bool {
    let mut changed = false;
    egui::Grid::new(("column_mapping", salt)).num_columns(2).striped(true).show(ui, |ui| {
        ui.label(egui::RichText::new("Column").strong());
        ui.label(egui::RichText::new(format!("In {}", table.name())).strong());
        ui.end_row();
        for (i, (need, chosen)) in mapping.needs.iter().zip(mapping.chosen.iter_mut()).enumerate() {
            ui.label(need_label(need));
            let shown = chosen.clone().unwrap_or_else(|| "Choose…".to_owned());
            egui::ComboBox::from_id_salt(("column_mapping_choice", i))
                .selected_text(shown)
                .width(crate::dialog_kit::combo_width(ui))
                .show_list(ui, |ui| {
                    for c in table.columns() {
                        if ui.selectable_label(chosen.as_deref() == Some(c.name.as_str()), &c.name).clicked() {
                            *chosen = Some(c.name.clone());
                            changed = true;
                        }
                    }
                });
            ui.end_row();
        }
    });
    for w in mapping.kind_warnings(table) {
        crate::dialog_kit::note(ui, &format!("⚠ {w}"));
    }
    changed
}

/// The first table in the data file at `path`.
fn read_data_file(path: &std::path::Path) -> Result<DataTable, String> {
    file_io::open_path(path)?
        .into_iter()
        .map(|item| match item {
            Opened::Table(t, _) => t,
            Opened::Project(p) => p.table,
        })
        .next()
        .ok_or_else(|| format!("{} holds no table", path.display()))
}

impl StatsApp {
    /// Carry out a plot style request of report `report` of document `index`.
    pub(super) fn carry_out_style(&mut self, ctx: &egui::Context, index: usize, report: u64, request: StyleRequest) {
        let Some(doc) = self.documents.get_mut(index) else { return };
        match request {
            StyleRequest::Copy(key) => {
                let copied = doc.reports.iter().find(|r| r.id() == report).and_then(|r| r.copy_plot_style(key.as_deref()));
                if let Some(c) = copied {
                    let from = c.from.clone();
                    crate::plot_style_menu::set_copied(ctx, c);
                    self.info(format!("Copied the style of “{from}”. Right-click another plot ▸ Paste plot style."));
                }
            }
            StyleRequest::Paste(target) => {
                let Some(copied) = crate::plot_style_menu::copied(ctx) else { return };
                let parts = crate::plot_style_menu::parts(ctx);
                let project = doc.project;
                let n: usize = match target {
                    PasteTarget::Plot(key) => doc
                        .reports
                        .iter_mut()
                        .filter(|r| r.id() == report)
                        .map(|r| r.paste_plot_style(&copied, parts, key.as_deref().or(Some(""))))
                        .sum(),
                    PasteTarget::Report => {
                        doc.reports.iter_mut().filter(|r| r.id() == report).map(|r| r.paste_plot_style(&copied, parts, None)).sum()
                    }
                    PasteTarget::Project => self
                        .documents
                        .iter_mut()
                        .filter(|d| d.project == project)
                        .flat_map(|d| d.reports.iter_mut())
                        .map(|r| r.paste_plot_style(&copied, parts, None))
                        .sum(),
                };
                self.info(format!("Pasted the style of “{}” onto {}.", copied.from, dialog_kit_count(n)));
            }
        }
    }

    /// Open Duplicate on another table for report `report` of document `index`.
    pub(super) fn open_duplicate_dialog(&mut self, index: usize, report: u64) {
        let Some(doc) = self.documents.get(index) else { return };
        let Some(r) = doc.reports.iter().find(|r| r.id() == report) else { return };
        let saved = r.saved();
        let needs = report_needs(&saved, &doc.table);
        self.duplicate = Some(DuplicateDialog { source: doc.id, title: r.title(), saved, needs, target: None });
    }

    /// The Duplicate on another table dialog, when open.
    pub(super) fn duplicate_dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.duplicate else { return };
        let documents = &self.documents;
        let projects: Vec<(u64, String)> = self.projects.iter().map(|p| (p.id, p.title.clone().unwrap_or_default())).collect();
        let source = dialog.source;
        let outcome = Dialog::new("Duplicate on another table").creates().without_reset().action("Duplicate").show(ctx, |ui| {
            ui.label(format!("Report: {}", dialog.title));
            ui.horizontal(|ui| {
                ui.label("Table:");
                let current = dialog.target.as_ref().and_then(|(id, _)| documents.iter().find(|d| d.id == *id));
                egui::ComboBox::from_id_salt("duplicate_target")
                    .selected_text(current.map_or("Choose a table…", |d| d.name()))
                    .width(crate::dialog_kit::combo_width(ui))
                    .show_list(ui, |ui| {
                        for d in documents.iter().filter(|d| d.id != source) {
                            let project = projects.iter().find(|(id, _)| *id == d.project).map(|(_, t)| t.as_str());
                            let label = match project {
                                Some(p) if !p.is_empty() && p != d.name() => format!("{} · {p}", d.name()),
                                _ => d.name().to_owned(),
                            };
                            if ui.selectable_label(current.is_some_and(|c| c.id == d.id), label).clicked() {
                                dialog.target = Some((d.id, ColumnMapping::new(dialog.needs.clone(), &d.table)));
                            }
                        }
                    });
            });
            let Some((target, mapping)) = &mut dialog.target else {
                return Err("Choose the table to put the copy on".to_owned());
            };
            let Some(table) = documents.iter().find(|d| d.id == *target).map(|d| &d.table) else {
                return Err("That table is closed".to_owned());
            };
            if mapping.is_exact() {
                crate::dialog_kit::note(ui, "Every column it reads has a column of the same name there.");
            } else {
                crate::dialog_kit::note(ui, "Choose the column that stands in for each column whose name differs:");
            }
            mapping_grid(ui, "duplicate", mapping, table);
            let pairs = mapping.pairs(table).map_err(|e| e.to_string())?;
            ready((*target, pairs), format!("A copy of “{}” on {}", dialog.title, table.name()))
        });
        match outcome {
            Outcome::Open | Outcome::Reset => {}
            Outcome::Cancel => self.duplicate = None,
            Outcome::Run((target, pairs), _) => {
                let Some(dialog) = self.duplicate.take() else { return };
                let source = self.documents.iter().find(|d| d.id == dialog.source).map(|d| (*d.table).clone());
                let Some(index) = self.documents.iter().position(|d| d.id == target) else { return };
                let Some(source) = source else { return };
                match duplicate_report(&dialog.saved, &source, &self.documents[index].table, &pairs) {
                    Ok(saved) => {
                        let id = self.next_report_id;
                        self.next_report_id += 1;
                        let doc = &mut self.documents[index];
                        doc.reports.insert(0, Report::new(id, saved, &doc.table, &doc.rows));
                        let name = doc.name().to_owned();
                        self.set_active(index);
                        self.reveal(index, View::Results);
                        self.info(format!("Copied “{}” onto {name}.", dialog.title));
                    }
                    Err(e) => self.error(format!("Could not copy “{}” onto that table: {e}", dialog.title)),
                }
            }
        }
    }

    /// File ▸ Save project as template…: the project of document `index`
    /// without its data.
    pub(super) fn save_template(&mut self, index: usize) {
        let Some(project) = self.documents.get(index).map(|d| d.project) else { return };
        let file = self.project_file(project, false);
        let (text, notes) = match template_text(&file) {
            Ok(t) => t,
            Err(e) => return self.error(format!("Could not make a template: {e}")),
        };
        let name = self.project_name(project);
        let Some(path) = file_io::pick_save_file("Save project as template", "Project template", TEMPLATE_EXTENSION, &name)
        else {
            return;
        };
        match std::fs::write(&path, text) {
            Ok(()) => {
                let mut message = format!(
                    "Saved the template {}: its reports, graphs, dashboards and plot styles, without data.",
                    path.display()
                );
                for n in notes {
                    message.push_str(&format!(" {n}."));
                }
                self.info(message);
            }
            Err(e) => self.error(format!("Could not write {}: {e}", path.display())),
        }
    }

    /// File ▸ New project from template…: ask for the template file.
    pub(super) fn new_from_template(&mut self) {
        let Some(path) = file_io::pick_open_file("New project from template", "Project template", &[TEMPLATE_EXTENSION])
        else {
            return;
        };
        match std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|t| ProjectTemplate::parse(&t).map_err(|e| e.to_string())) {
            Ok(template) => self.open_template(app_model::file_formats::name_from_path(&path), template),
            Err(e) => self.error(format!("Could not read {}: {e}", path.display())),
        }
    }

    /// Open the New project from template dialog for `template`, named `name`.
    pub(super) fn open_template(&mut self, name: String, template: ProjectTemplate) {
        let n = template.tables.len();
        let mut dialog = TemplateDialog {
            name,
            template,
            folder: None,
            use_folder: false,
            sources: (0..n).map(|_| None).collect(),
            folder_mapping: None,
            message: None,
        };
        // A one-table template starts on the table shown, if it fits.
        if n == 1
            && let Some(doc) = self.documents.get(self.active)
        {
            let mapping = ColumnMapping::new(dialog.template.tables[0].needs.clone(), &doc.table);
            if mapping.is_exact() {
                dialog.sources[0] = Some((TemplateSource::Open(doc.id), mapping));
            }
        }
        self.template = Some(dialog);
    }

    /// The New project from template dialog, when open.
    pub(super) fn template_dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.template else { return };
        let documents = &self.documents;
        let table_of = |source: &TemplateSource| -> Option<DataTable> {
            match source {
                TemplateSource::Open(id) => documents.iter().find(|d| d.id == *id).map(|d| (*d.table).clone()),
                TemplateSource::File(_, t) => Some(t.clone()),
            }
        };
        let mut pick: Option<(Option<usize>, PathBuf)> = None;
        let outcome = Dialog::new("New project from template").creates().without_reset().action("Create").show(ctx, |ui| {
            ui.label(format!("Template: {}", dialog.name));
            let one = dialog.template.tables.len() == 1;
            if one {
                ui.horizontal(|ui| {
                    ui.radio_value(&mut dialog.use_folder, false, "One table");
                    ui.radio_value(&mut dialog.use_folder, true, "Each data file in a folder");
                });
            }
            if let Some(m) = &dialog.message {
                ui.colored_label(theme::ERROR, m);
            }
            if dialog.use_folder {
                if ui.button("Choose a folder…").clicked()
                    && let Some(folder) = file_io::pick_folder("Folder of data files")
                {
                    pick = Some((None, folder));
                }
                let Some((folder, files)) = &dialog.folder else { return Err("Choose a folder of data files".to_owned()) };
                ui.label(format!("{} data file(s) in {}", files.len(), folder.display()));
                let Some((_, first)) = files.first() else { return Err("The folder holds no data file".to_owned()) };
                let mapping = dialog.folder_mapping.get_or_insert_with(|| ColumnMapping::new(dialog.template.tables[0].needs.clone(), first));
                crate::dialog_kit::note(ui, "Columns are matched by name in each file; choices made here apply to every file:");
                mapping_grid(ui, "folder", mapping, first);
                mapping.pairs(first).map_err(|e| e.to_string())?;
                return ready((), format!("A project of {} table(s) from {}", files.len(), dialog.name));
            }
            let mut missing = None;
            for (i, (t, source)) in dialog.template.tables.iter().zip(dialog.sources.iter_mut()).enumerate() {
                ui.separator();
                ui.label(egui::RichText::new(format!("Table “{}”: {} column(s) read", t.name, t.needs.len())).strong());
                ui.horizontal(|ui| {
                    ui.label("Data:");
                    let shown = match source.as_ref().map(|(s, _)| s) {
                        Some(TemplateSource::Open(id)) => documents.iter().find(|d| d.id == *id).map_or("—", |d| d.name()).to_owned(),
                        Some(TemplateSource::File(p, _)) => p.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default(),
                        None => "Choose an open table…".to_owned(),
                    };
                    egui::ComboBox::from_id_salt(("template_source", i))
                        .selected_text(shown)
                        .width(crate::dialog_kit::combo_width(ui))
                        .show_list(ui, |ui| {
                            for d in documents {
                                if ui.selectable_label(false, d.name()).clicked() {
                                    *source = Some((TemplateSource::Open(d.id), ColumnMapping::new(t.needs.clone(), &d.table)));
                                }
                            }
                        });
                    if ui.button("Data file…").clicked()
                        && let Some(path) = file_io::pick_open_file("Data for the template", "Data", &app_model::file_formats::OPEN_EXTENSIONS)
                    {
                        pick = Some((Some(i), path));
                    }
                });
                match source {
                    Some((s, mapping)) => {
                        if let Some(table) = table_of(s) {
                            mapping_grid(ui, ("template", i), mapping, &table);
                            if let Err(e) = mapping.pairs(&table) {
                                missing.get_or_insert(e.to_string());
                            }
                        }
                    }
                    None => {
                        missing.get_or_insert(format!("Choose the data for “{}”", t.name));
                    }
                }
            }
            match missing {
                Some(m) => Err(m),
                None => ready((), format!("A project from {}", dialog.name)),
            }
        });
        // Files chosen this frame are read now.
        if let Some((slot, path)) = pick {
            let dialog = self.template.as_mut().expect("open");
            match slot {
                Some(i) => match read_data_file(&path) {
                    Ok(t) => {
                        let mapping = ColumnMapping::new(dialog.template.tables[i].needs.clone(), &t);
                        dialog.sources[i] = Some((TemplateSource::File(path, t), mapping));
                        dialog.message = None;
                    }
                    Err(e) => dialog.message = Some(e),
                },
                None => {
                    let files = stat_script::batch::batch_files(&path, None).unwrap_or_default();
                    let mut read = Vec::new();
                    let mut problems = Vec::new();
                    for f in files {
                        match read_data_file(&f) {
                            Ok(t) => read.push((f, t)),
                            Err(e) => problems.push(e),
                        }
                    }
                    dialog.message = (!problems.is_empty()).then(|| problems.join("; "));
                    dialog.folder = Some((path, read));
                    dialog.folder_mapping = None;
                }
            }
        }
        match outcome {
            Outcome::Open | Outcome::Reset => {}
            Outcome::Cancel => self.template = None,
            Outcome::Run((), _) => {
                let Some(dialog) = self.template.take() else { return };
                self.build_from_template(dialog);
            }
        }
    }

    /// Build and open the project the template dialog describes.
    fn build_from_template(&mut self, dialog: TemplateDialog) {
        let TemplateDialog { name, template, folder, use_folder, sources, folder_mapping, .. } = dialog;
        let mut notes = Vec::new();
        let file = if use_folder {
            let (Some((_, files)), Some(mapping)) = (folder, folder_mapping) else { return };
            let mut built = Vec::new();
            for (path, table) in files {
                // The choices made on the first file, else matching by name.
                let mut m = mapping.clone();
                if m.pairs(&table).is_err() {
                    m = ColumnMapping::new(mapping.needs.clone(), &table);
                }
                match template.apply(std::slice::from_ref(&table), std::slice::from_ref(&m)) {
                    Ok((f, n)) => {
                        notes.extend(n);
                        built.push(f);
                    }
                    Err(e) => notes.push(format!("{} was left out: {e}", path.display())),
                }
            }
            if built.is_empty() {
                return self.error(format!("No data file fitted the template. {}", notes.join(" ")));
            }
            merge_projects(built)
        } else {
            let mut tables = Vec::new();
            let mut mappings = Vec::new();
            for (source, mapping) in sources.into_iter().flatten() {
                let table = match source {
                    TemplateSource::Open(id) => self.documents.iter().find(|d| d.id == id).map(|d| (*d.table).clone()),
                    TemplateSource::File(_, t) => Some(t),
                };
                let Some(table) = table else { return self.error("A table chosen for the template was closed.") };
                tables.push(table);
                mappings.push(mapping);
            }
            match template.apply(&tables, &mappings) {
                Ok((f, n)) => {
                    notes.extend(n);
                    f
                }
                Err(e) => return self.error(format!("Could not build the project: {e}")),
            }
        };
        self.open_project_file(file, NewProject { title: Some(name.clone()), unsaved: true, ..NewProject::default() });
        let mut message = format!("Built a new project from the template {name}.");
        for n in notes {
            message.push_str(&format!(" {n}."));
        }
        self.info(message);
    }
}

/// "1 plot" or "n plots".
fn dialog_kit_count(n: usize) -> String {
    crate::dialog_kit::count(n, "plot", "plots")
}
