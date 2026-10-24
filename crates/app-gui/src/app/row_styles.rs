//! Row colours and markers from a column (Data ▸ Row states ▸ Colour and
//! marker by column…): every row takes a colour, and for a categorical
//! column a marker, from its value, and every plot without a grouping of
//! its own draws them (`stat_viz::row_styles`).

use super::*;

/// The Colour and marker by column dialog.
pub(super) struct ColorByDialog {
    /// Document (by id) whose rows are coloured.
    pub(super) document: u64,
    column: Option<usize>,
}

impl ColorByDialog {
    pub(super) fn new(document: u64, table: &DataTable, column: Option<usize>) -> Self {
        let column = column.or_else(|| (0..table.n_cols()).find(|&j| !crate::dialog_kit::is_continuous(table, j)));
        Self { document, column }
    }
}

/// The levels (or ramp ends) the dialog previews: name, colour, marker.
fn preview(ui: &mut egui::Ui, scheme: &stat_viz::RowStyleScheme) {
    let dark = ui.visuals().dark_mode;
    let swatch = |ui: &mut egui::Ui, color: egui::Color32, shape: egui_plot::MarkerShape| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 14.0), egui::Sense::hover());
        crate::report_view::paint_marker(ui.painter(), rect.center(), 5.0, shape, color, true);
    };
    match scheme {
        stat_viz::RowStyleScheme::Levels { levels, codes, .. } => {
            const SHOWN: usize = 24;
            egui::Grid::new("color_by_preview").num_columns(3).striped(true).show(ui, |ui| {
                for (k, (name, color, marker)) in levels.iter().take(SHOWN).enumerate() {
                    ui.horizontal(|ui| swatch(ui, theme::row_color(*color, dark), theme::row_marker(*marker)));
                    ui.label(name);
                    let n = codes.iter().filter(|c| **c == Some(k)).count();
                    ui.label(RichText::new(count(n, "row", "rows")).weak());
                    ui.end_row();
                }
            });
            if levels.len() > SHOWN {
                ui.label(RichText::new(format!("…and {} more levels", levels.len() - SHOWN)).weak());
            }
        }
        stat_viz::RowStyleScheme::Ramp { min, max, .. } => {
            ui.horizontal(|ui| {
                ui.label(app_model::format::format_number(*min, 4));
                let (rect, _) = ui.allocate_exact_size(egui::vec2(160.0, 12.0), egui::Sense::hover());
                let steps = 32;
                for i in 0..steps {
                    let t = i as f64 / f64::from(steps - 1);
                    let c = theme::row_color(stat_viz::row_styles::row_color(stat_viz::palette::ramp(t)), dark);
                    let x0 = rect.left() + rect.width() * i as f32 / steps as f32;
                    let r = egui::Rect::from_min_max(
                        egui::pos2(x0, rect.top()),
                        egui::pos2(x0 + rect.width() / steps as f32 + 0.5, rect.bottom()),
                    );
                    ui.painter().rect_filled(r, 0.0, c);
                }
                ui.label(app_model::format::format_number(*max, 4));
            });
            ui.label(
                RichText::new("A continuous column gives a ramp of colours from its smallest to largest value").weak(),
            );
        }
    }
}

impl StatsApp {
    /// Open the dialog for the active document (on `column`, if given).
    pub(super) fn open_color_by(&mut self, column: Option<usize>) {
        let Some(doc) = self.documents.get(self.active) else { return };
        let mut dialog = ColorByDialog::new(doc.id, &doc.table, column);
        if column.is_none()
            && let Some(j) = doc.rows.style_column().and_then(|c| doc.table.find(c))
        {
            dialog.column = Some(j);
        }
        self.color_by_dialog = Some(dialog);
    }

    /// The dialog, while it is open.
    pub(super) fn color_by_window(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.color_by_dialog else { return };
        let Some(doc) = self.documents.iter_mut().find(|d| d.id == dialog.document) else {
            self.color_by_dialog = None;
            return;
        };
        let table = &doc.table;
        let outcome = crate::dialog_kit::Dialog::new("Colour and marker by column")
            .id_salt(("color_by_dialog", dialog.document))
            .without_rows()
            .without_reset()
            .default_width(380.0)
            .action("Apply")
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Column");
                    crate::dialog_kit::column_choice(ui, "color_by_column", table, &mut dialog.column, |_| true, None);
                });
                crate::dialog_kit::note(
                    ui,
                    "Each row takes a colour (and, for categories, a marker shape) from its value in this column. \
                     Plots without a grouping of their own draw these and name the levels in their key.",
                );
                let Some(j) = dialog.column.filter(|&j| j < table.n_cols()) else {
                    return Err("Choose a column".to_owned());
                };
                let name = table.columns()[j].name.clone();
                let Some(scheme) = stat_viz::RowStyleScheme::of(table, &name) else {
                    return Err(format!("“{name}” has no values to colour by"));
                };
                ui.separator();
                egui::ScrollArea::vertical().max_height(crate::dialog_kit::em(ui) * 18.0).show(ui, |ui| {
                    preview(ui, &scheme);
                });
                crate::dialog_kit::ready(name.clone(), format!("Colour every row by “{name}”"))
            });
        match outcome {
            Outcome::Open | Outcome::Reset => {}
            Outcome::Cancel => self.color_by_dialog = None,
            Outcome::Run(name, _) => match app_model::action::color_by_column(&doc.table, &mut doc.rows, &name) {
                Ok(()) => {
                    self.color_by_dialog = None;
                    self.info(format!("Rows coloured by “{name}”. Data ▸ Row states ▸ Clear row colours undoes it."));
                }
                Err(e) => self.error(e.to_string()),
            },
        }
    }
}
