//! The Value labels dialog: give stored values of a column the names
//! analyses, plots and keys show (1 → "Treatment"), as one undoable edit.

use std::collections::BTreeMap;

use app_model::editing::{ColumnShift, Edit};
use app_model::recode::{self, ValueCount};
use eframe::egui::{self, RichText};
use stat_data::DataTable;

use crate::dialog_kit::{self as kit, Dialog, Outcome, Readiness};

/// One row of the dialog: a stored value and its label.
struct Line {
    value: ValueCount,
    label: String,
}

/// State of an open Value labels dialog.
pub struct ValueLabelsDialog {
    /// The document it belongs to.
    pub document: u64,
    column: usize,
    revision: u64,
    lines: Vec<Line>,
    /// Labels of values no row holds any more (kept unless removed).
    unused: BTreeMap<String, String>,
}

impl ValueLabelsDialog {
    /// A dialog labelling the values of column `column` of `table`.
    pub fn new(document: u64, table: &DataTable, column: usize) -> Self {
        let mut dialog = Self { document, column, revision: u64::MAX, lines: Vec::new(), unused: BTreeMap::new() };
        dialog.load(table, column);
        dialog
    }

    /// Keep to the same column after columns were inserted or removed.
    /// Returns `false` when the column itself was removed.
    pub fn follow(&mut self, shift: ColumnShift) -> bool {
        match shift.map(self.column) {
            Some(column) => {
                self.column = column;
                true
            }
            None => false,
        }
    }

    /// Read the column's values and current labels afresh.
    fn load(&mut self, table: &DataTable, column: usize) {
        self.column = column;
        self.revision = table.revision();
        let Some(col) = table.column(column) else { return };
        let mut unused = col.value_labels.clone();
        self.lines = recode::distinct_values(table, column)
            .unwrap_or_default()
            .into_iter()
            .filter(|v| !v.text.is_empty())
            .map(|value| {
                let label = unused.remove(&value.text).unwrap_or_default();
                Line { value, label }
            })
            .collect();
        self.unused = unused;
    }

    /// The labels as typed (blank ones left out).
    fn labels(&self) -> BTreeMap<String, String> {
        let mut labels: BTreeMap<String, String> = self.unused.clone();
        for line in self.lines.iter().filter(|l| !l.label.trim().is_empty()) {
            labels.insert(line.value.text.clone(), line.label.trim().to_owned());
        }
        labels
    }

    /// Draw the dialog over `table`. Apply returns the edit to make, with a
    /// message for the status bar.
    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<(Edit, String)> {
        if self.column >= table.n_cols() {
            return Outcome::Cancel;
        }
        if table.revision() != self.revision {
            let typed: Vec<(String, String)> =
                self.lines.iter().map(|l| (l.value.text.clone(), l.label.clone())).collect();
            self.load(table, self.column);
            for line in &mut self.lines {
                if let Some((_, label)) = typed.iter().find(|(v, _)| *v == line.value.text) {
                    line.label.clone_from(label);
                }
            }
        }
        let dialog = Dialog::new("Value labels")
            .id_salt(("value_labels_dialog", self.document))
            .without_rows()
            .without_reset()
            .default_width(460.0)
            .action("Apply");
        match dialog.show(ctx, |ui| self.contents(ui, table)) {
            Outcome::Run(labels, place) => {
                let name = kit::name(table, self.column).to_owned();
                let message = if labels.is_empty() {
                    format!("Removed the value labels of “{name}”. Undo puts them back.")
                } else {
                    format!("“{name}” now has {}. Undo removes them.", kit::count(labels.len(), "label", "labels"))
                };
                Outcome::Run((Edit::SetValueLabels { index: self.column, labels }, message), place)
            }
            Outcome::Cancel => Outcome::Cancel,
            Outcome::Open | Outcome::Reset => Outcome::Open,
        }
    }

    fn contents(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<BTreeMap<String, String>> {
        ui.horizontal(|ui| {
            ui.label("Column");
            let mut choice = Some(self.column);
            kit::column_choice(ui, ("value_labels_column", self.document), table, &mut choice, |_| true, None);
            if let Some(choice) = choice.filter(|&c| c != self.column) {
                self.load(table, choice);
            }
        });
        kit::note(
            ui,
            "Type the name each stored value stands for. Analyses, plots and keys show the names; the data keep \
             the stored values. Leave a label blank to show the value itself.",
        );
        ui.horizontal(|ui| {
            if ui.small_button("Clear all labels").clicked() {
                for line in &mut self.lines {
                    line.label.clear();
                }
                self.unused.clear();
            }
        });
        ui.separator();
        egui::ScrollArea::vertical().max_height(kit::em(ui) * 21.0).auto_shrink([false, true]).show(ui, |ui| {
            egui::Grid::new(("value_label_lines", self.document)).num_columns(3).striped(true).show(ui, |ui| {
                ui.strong("Stored value");
                ui.strong("Rows");
                ui.strong("Label");
                ui.end_row();
                for (k, line) in self.lines.iter_mut().enumerate() {
                    ui.label(RichText::new(line.value.text.replace(' ', "·")).monospace());
                    ui.monospace(line.value.count.to_string());
                    let name = format!("Label for {}", line.value.text);
                    ui.add(egui::TextEdit::singleline(&mut line.label).desired_width(kit::em(ui) * 14.0).id_salt(k))
                        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, &name));
                    ui.end_row();
                }
            });
        });
        if !self.unused.is_empty() {
            kit::note(
                ui,
                &format!("{} for values no row holds now are kept.", kit::count(self.unused.len(), "label", "labels")),
            );
        }
        let labels = self.labels();
        let col = table.column(self.column);
        if col.is_some_and(|c| c.value_labels == labels) {
            return Err("Type or change a label".into());
        }
        // Two values sharing a label would merge in analyses.
        let mut seen: Vec<&str> = Vec::new();
        for label in labels.values() {
            if seen.contains(&label.as_str()) {
                return Err(format!("Two values are labelled “{label}”; give each its own label"));
            }
            seen.push(label);
        }
        let name = kit::name(table, self.column);
        let summary = if labels.is_empty() {
            format!("Remove the value labels of “{name}”")
        } else {
            format!("Label {} of “{name}”", kit::count(labels.len(), "value", "values"))
        };
        kit::ready(labels, summary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    /// The dialog lists each stored value once, with its current label.
    #[test]
    fn lines_hold_values_and_labels() {
        let mut arm = Column::numeric("Arm", vec![0.0, 1.0, 1.0, f64::NAN]);
        arm.value_labels = [("1".to_owned(), "Treatment".to_owned()), ("7".to_owned(), "Old".to_owned())].into();
        let table = DataTable::new("t", vec![arm]).unwrap();
        let dialog = ValueLabelsDialog::new(0, &table, 0);
        let lines: Vec<(&str, usize, &str)> =
            dialog.lines.iter().map(|l| (l.value.text.as_str(), l.value.count, l.label.as_str())).collect();
        assert_eq!(lines, [("0", 1, ""), ("1", 2, "Treatment")]);
        assert_eq!(dialog.unused.len(), 1, "a label of a value no row holds is kept");
        assert_eq!(dialog.labels().len(), 2);
    }
}
