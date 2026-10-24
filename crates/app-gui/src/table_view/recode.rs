//! The Recode dialog: give each distinct value of a column a new value, in
//! place or into a new column (`app_model::recode`).

use std::collections::HashMap;

use app_model::editing::{ColumnShift, Edit};
use app_model::recode::{self, RecodeTarget, ValueCount, helpers};
use eframe::egui::{self, RichText};
use stat_data::DataTable;

use crate::dialog_kit::{self as kit, Dialog, Outcome, Readiness};
use crate::theme;

/// One row of the dialog: a current value and its new value.
struct Line {
    old: ValueCount,
    new: String,
}

/// State of an open Recode dialog.
pub struct RecodeDialog {
    /// The document it belongs to.
    pub document: u64,
    column: usize,
    /// The table revision the value list was read at.
    revision: u64,
    lines: Vec<Line>,
    in_place: bool,
    new_name: String,
    error: Option<String>,
}

impl RecodeDialog {
    /// A dialog recoding column `column` of `table` (in document `document`).
    pub fn new(document: u64, table: &DataTable, column: usize) -> Self {
        let mut dialog = Self {
            document,
            column,
            revision: u64::MAX,
            lines: Vec::new(),
            in_place: false,
            new_name: String::new(),
            error: None,
        };
        dialog.load(table, column);
        dialog
    }

    /// Keep to the same column after columns were inserted or removed
    /// (otherwise the values of whatever column moved into its place would
    /// be recoded). Returns `false` when the column itself was removed.
    pub fn follow(&mut self, shift: ColumnShift) -> bool {
        match shift.map(self.column) {
            Some(column) => {
                self.column = column;
                true
            }
            None => false,
        }
    }

    /// Read the values of `column` afresh (a new column, or the data changed).
    fn load(&mut self, table: &DataTable, column: usize) {
        self.column = column;
        self.revision = table.revision();
        self.lines = recode::distinct_values(table, column)
            .unwrap_or_default()
            .into_iter()
            .map(|old| Line { new: old.text.clone(), old })
            .collect();
        if let Some(col) = table.column(column) {
            self.new_name = table.unused_column_name(&format!("{} recoded", col.name));
        }
        self.error = None;
    }

    /// The old → new pairs that differ.
    fn mapping(&self) -> HashMap<String, String> {
        self.lines.iter().filter(|l| l.new != l.old.text).map(|l| (l.old.text.clone(), l.new.clone())).collect()
    }

    /// Draw the dialog over `table`. Recode returns the edit to make, with a
    /// message for the status bar.
    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<(Edit, String)> {
        if self.column >= table.n_cols() {
            return Outcome::Cancel;
        }
        if table.revision() != self.revision {
            // Keep the new values typed so far for values that are still there.
            let typed: HashMap<String, String> =
                self.lines.iter().map(|l| (l.old.text.clone(), l.new.clone())).collect();
            self.load(table, self.column);
            for line in &mut self.lines {
                if let Some(new) = typed.get(&line.old.text) {
                    line.new.clone_from(new);
                }
            }
        }
        let dialog = Dialog::new("Recode column")
            .id_salt(("recode_dialog", self.document))
            .without_rows()
            .without_reset()
            .default_width(520.0)
            .action("Recode");
        match dialog.show(ctx, |ui| self.contents(ui, table)) {
            Outcome::Run((mapping, target), place) => {
                match recode::recode_edit(table, self.column, &mapping, &target) {
                    Ok((edit, changed)) => {
                        let name = table.column(self.column).map_or("", |c| c.name.as_str());
                        let message = match &target {
                            RecodeTarget::InPlace => {
                                format!("Recoded {changed} cell(s) of “{name}”. Undo puts them back.")
                            }
                            RecodeTarget::NewColumn(new) => {
                                format!("Added “{}”, “{name}” recoded ({changed} cell(s) changed)", new.trim())
                            }
                        };
                        Outcome::Run((edit, message), place)
                    }
                    Err(e) => {
                        self.error = Some(e.to_string());
                        Outcome::Open
                    }
                }
            }
            Outcome::Cancel => Outcome::Cancel,
            Outcome::Open | Outcome::Reset => Outcome::Open,
        }
    }

    fn contents(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<(HashMap<String, String>, RecodeTarget)> {
        ui.horizontal(|ui| {
            ui.label("Column");
            let mut choice = Some(self.column);
            kit::column_choice(ui, ("recode_column", self.document), table, &mut choice, |_| true, None);
            if let Some(choice) = choice.filter(|&c| c != self.column) {
                self.load(table, choice);
            }
        });
        kit::note(
            ui,
            "Type a new value beside each current one. Values given the same new value are merged; a blank new \
             value makes those cells missing.",
        );
        ui.horizontal_wrapped(|ui| {
            ui.label("All new values:");
            let mut apply = |f: &dyn Fn(&str) -> String| {
                for line in &mut self.lines {
                    line.new = f(&line.new);
                }
            };
            if ui.small_button("Trim spaces").clicked() {
                apply(&helpers::trim_spaces);
            }
            if ui.small_button("lower case").clicked() {
                apply(&|t: &str| t.to_lowercase());
            }
            if ui.small_button("UPPER CASE").clicked() {
                apply(&|t: &str| t.to_uppercase());
            }
            if ui.small_button("Title Case").clicked() {
                apply(&helpers::title_case);
            }
            if ui.small_button("Reset").on_hover_text("Put back the current values").clicked() {
                for line in &mut self.lines {
                    line.new.clone_from(&line.old.text);
                }
            }
        });
        ui.separator();
        egui::ScrollArea::vertical().max_height(kit::em(ui) * 21.0).auto_shrink([false, true]).show(ui, |ui| {
            egui::Grid::new(("recode_values", self.document)).num_columns(3).striped(true).show(ui, |ui| {
                ui.strong("Current value");
                ui.strong("Rows");
                ui.strong("New value");
                ui.end_row();
                for line in &mut self.lines {
                    if line.old.text.is_empty() {
                        ui.label(RichText::new("(missing)").weak().italics());
                    } else {
                        // Show leading and trailing spaces, which are easy to miss.
                        ui.label(RichText::new(line.old.text.replace(' ', "·")).monospace())
                            .on_hover_text("· marks a space");
                    }
                    ui.monospace(line.old.count.to_string());
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut line.new).desired_width(kit::em(ui) * 12.0));
                        if line.new != line.old.text {
                            ui.label("✎").on_hover_text("Changed");
                        }
                    });
                    ui.end_row();
                }
            });
        });
        ui.separator();
        ui.radio_value(&mut self.in_place, false, "Into a new column");
        ui.add_enabled_ui(!self.in_place, |ui| {
            ui.horizontal(|ui| {
                ui.add_space(20.0);
                ui.label("Name");
                ui.text_edit_singleline(&mut self.new_name);
            });
        });
        ui.radio_value(&mut self.in_place, true, "Replace the values in this column (undoable)");
        if let Some(e) = &self.error {
            ui.colored_label(theme::ERROR, format!("⚠ {e}"));
        }
        let mapping = self.mapping();
        let rows: usize = self.lines.iter().filter(|l| l.new != l.old.text).map(|l| l.old.count).sum();
        let name = kit::name(table, self.column);
        let target = if self.in_place {
            if mapping.is_empty() {
                return Err("Type a new value for at least one value".into());
            }
            RecodeTarget::InPlace
        } else {
            let new = self.new_name.trim();
            if new.is_empty() {
                return Err("Name the new column".into());
            }
            if table.find(new).is_some() {
                return Err(format!("There is already a column named “{new}”"));
            }
            RecodeTarget::NewColumn(self.new_name.clone())
        };
        let changed = format!(
            "{} changed, affecting {}",
            kit::count(mapping.len(), "value", "values"),
            kit::count(rows, "row", "rows")
        );
        let summary = match &target {
            RecodeTarget::InPlace => format!("Recode “{name}” in place: {changed}"),
            RecodeTarget::NewColumn(new) => format!("New column “{}” from “{name}”: {changed}", new.trim()),
        };
        kit::ready((mapping, target), summary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    #[test]
    fn the_dialog_stays_on_its_column_when_columns_move() {
        let table = DataTable::new("t", vec![Column::text("a", vec!["x".into()]), Column::text("b", vec!["y".into()])])
            .unwrap();
        let mut dialog = RecodeDialog::new(0, &table, 1);
        assert!(dialog.follow(ColumnShift::Inserted(0)));
        assert_eq!(dialog.column, 2);
        assert!(dialog.follow(ColumnShift::Inserted(3)));
        assert_eq!(dialog.column, 2);
        assert!(dialog.follow(ColumnShift::Removed(0)));
        assert_eq!(dialog.column, 1);
        assert!(!dialog.follow(ColumnShift::Removed(1)), "its column was removed");
    }
}
