//! The New rank column dialog (Data ▸ New rank column…): a formula column
//! `rank_ties("column", "rule")` ranking a numeric column, optionally within
//! groups, with a chosen rule for ties ([`stat_core::ranking`]). Being a
//! formula, the ranks follow later edits.

use app_model::editing::Edit;
use eframe::egui;
use stat_core::ranking::TieRule;
use stat_data::DataTable;

use crate::dialog_kit::{self as kit, Dialog, Outcome, Readiness};

/// State of an open New rank column dialog.
pub struct RankDialog {
    /// The document it belongs to.
    pub document: u64,
    column: Option<usize>,
    by: Option<usize>,
    rule: TieRule,
    name: String,
    /// The name last suggested, replaced when the column changes unless typed over.
    suggested: String,
}

/// The formula ranking `column` (within groups of `by`) under `rule`.
#[must_use]
pub fn rank_formula(column: &str, rule: TieRule, by: Option<&str>) -> String {
    let quote = |s: &str| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""));
    match by {
        Some(by) => format!("rank_ties({}, {}, {})", quote(column), quote(rule.key()), quote(by)),
        None => format!("rank_ties({}, {})", quote(column), quote(rule.key())),
    }
}

impl RankDialog {
    /// A dialog for `table` (in document `document`), ranking `column` if given.
    pub fn new(document: u64, table: &DataTable, column: Option<usize>) -> Self {
        let column = column.filter(|&c| kit::is_numeric(table, c));
        let mut d =
            Self { document, column, by: None, rule: TieRule::Average, name: String::new(), suggested: String::new() };
        d.suggest(table);
        d
    }

    fn suggest(&mut self, table: &DataTable) {
        let base =
            self.column.and_then(|c| table.column(c)).map_or("Rank".to_owned(), |c| format!("Rank of {}", c.name));
        if self.name.is_empty() || self.name == self.suggested {
            self.name = table.unused_column_name(&base);
        }
        self.suggested.clone_from(&self.name);
    }

    /// Show the dialog; Run gives the edit adding the column and a status message.
    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<(Edit, String)> {
        let dialog = Dialog::new("New rank column")
            .id_salt(("rank_dialog", self.document))
            .without_rows()
            .without_reset()
            .action("Add column");
        match dialog.show(ctx, |ui| self.contents(ui, table)) {
            Outcome::Run((formula, name), place) => {
                match Edit::formula_column(table, None, &name, table.n_cols(), &formula, None) {
                    Ok(edit) => {
                        Outcome::Run((edit, format!("Added rank column “{name}” ({formula}). Undo removes it.")), place)
                    }
                    Err(_) => Outcome::Open,
                }
            }
            Outcome::Open => Outcome::Open,
            Outcome::Reset => Outcome::Open,
            Outcome::Cancel => Outcome::Cancel,
        }
    }

    fn contents(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<(String, String)> {
        egui::Grid::new("rank_roles").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
            ui.label("Column to rank:");
            if kit::column_choice(ui, "rank_column", table, &mut self.column, |i| kit::is_numeric(table, i), None) {
                self.suggest(table);
            }
            ui.end_row();
            ui.label("Rank within groups of:").on_hover_text("Ranks restart at 1 in each group of this column");
            kit::column_choice(ui, "rank_by", table, &mut self.by, |i| Some(i) != self.column, Some("None"));
            ui.end_row();
            ui.label("New column name:");
            ui.add(egui::TextEdit::singleline(&mut self.name).desired_width(kit::em(ui) * 14.0));
            ui.end_row();
        });
        ui.add_space(6.0);
        ui.label("Tied values:");
        for rule in TieRule::ALL {
            ui.radio_value(&mut self.rule, rule, rule.label());
        }
        kit::note(ui, "Ranks run from 1 at the smallest value; missing values get no rank.");
        let Some(c) = self.column else { return Err("Choose a numeric column to rank".into()) };
        let name = self.name.trim().to_owned();
        if name.is_empty() {
            return Err("Name the new column".into());
        }
        if table.find(&name).is_some() {
            return Err(format!("There is already a column named “{name}”"));
        }
        let by = self.by.map(|b| kit::name(table, b));
        let formula = rank_formula(kit::name(table, c), self.rule, by);
        let summary = format!("Adds the formula column “{name}” = {formula}");
        kit::ready((formula, name), summary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formulas_quote_their_names() {
        assert_eq!(rank_formula("x", TieRule::Dense, None), "rank_ties(\"x\", \"dense\")");
        assert_eq!(rank_formula("a \"b\"", TieRule::Minimum, Some("g")), "rank_ties(\"a \\\"b\\\"\", \"min\", \"g\")");
    }
}
