//! The formula editor: write or change the formula of a column, with a
//! browser of columns and functions that insert at the cursor, a live
//! preview of the first rows, the formula typeset as mathematics, and errors
//! shown at their place in the text.

use crate::dialog_kit::ComboExt as _;
use app_model::editing::Edit;
use app_model::formula::{self, Evaluation, FUNCTIONS, FormulaError};
use app_model::formula_typeset;
use eframe::egui::{self, RichText, TextStyle};
use stat_data::{ColumnKind, DataTable, TemporalDisplay, TemporalFormat};

use crate::dialog_kit::{self as kit, Dialog, Outcome, Readiness};
use crate::theme;

/// Rows shown in the preview.
const PREVIEW_ROWS: usize = 10;

/// How the computed values are stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Storage {
    /// Numbers as numbers, text as text.
    Automatic,
    /// A particular column type.
    Kind(ColumnKind),
}

impl Storage {
    fn label(self) -> &'static str {
        match self {
            Self::Automatic => "As computed",
            Self::Kind(ColumnKind::Numeric) => "Numbers",
            Self::Kind(ColumnKind::Text) => "Text",
            Self::Kind(ColumnKind::DateTime(d)) => d.format.label(),
        }
    }

    fn choices() -> Vec<Self> {
        let mut out = vec![Self::Automatic, Self::Kind(ColumnKind::Numeric), Self::Kind(ColumnKind::Text)];
        out.extend(TemporalFormat::ALL.into_iter().map(|f| Self::Kind(ColumnKind::DateTime(TemporalDisplay::new(f)))));
        out
    }

    fn same(self, other: Self) -> bool {
        match (self, other) {
            (Self::Kind(ColumnKind::DateTime(a)), Self::Kind(ColumnKind::DateTime(b))) => a.format == b.format,
            (a, b) => a == b,
        }
    }
}

/// The formula editor for one column (or a new one) of one document.
pub struct FormulaDialog {
    /// The document it belongs to.
    pub document: u64,
    /// The column being given a formula, or `None` for a new column.
    target: Option<usize>,
    /// Where a new column goes.
    at: usize,
    name: String,
    text: String,
    storage: Storage,
    filter: String,
    /// Character position of the text cursor, for inserting.
    cursor: usize,
    /// `(formula, table revision)` the preview was made for, and the preview.
    preview_key: Option<(String, u64)>,
    preview: Option<Result<Evaluation, FormulaError>>,
    /// Failures over the whole table, once checked on Apply; applying again
    /// accepts them.
    confirm: Option<String>,
    focus: bool,
}

impl FormulaDialog {
    /// The editor for column `target` of `table`, or for a new column
    /// inserted at `at`.
    pub fn new(document: u64, table: &DataTable, target: Option<usize>, at: usize) -> Self {
        let column = target.and_then(|j| table.column(j));
        let (name, text, storage) = match column {
            Some(c) => (c.name.clone(), c.formula.clone().unwrap_or_default(), Storage::Kind(c.kind())),
            None => (table.unused_column_name("Formula"), String::new(), Storage::Automatic),
        };
        let cursor = text.chars().count();
        Self {
            document,
            target,
            at,
            name,
            text,
            storage,
            filter: String::new(),
            cursor,
            preview_key: None,
            preview: None,
            confirm: None,
            focus: true,
        }
    }

    fn insert(&mut self, ctx: &egui::Context, id: egui::Id, snippet: &str) {
        let at = self.text.char_indices().nth(self.cursor).map_or(self.text.len(), |(i, _)| i);
        self.text.insert_str(at, snippet);
        self.cursor += snippet.chars().count();
        if let Some(mut state) = egui::widgets::text_edit::TextEditState::load(ctx, id) {
            let c = egui::text::CCursor::new(self.cursor);
            state.cursor.set_char_range(Some(egui::text_selection::CCursorRange::one(c)));
            state.store(ctx, id);
        }
        self.focus = true;
        self.confirm = None;
    }

    /// Keep the editor on its column while the table changes under it: a
    /// column inserted, deleted or moved before the target shifts its
    /// position, so the target is found again by name. `false` when the
    /// column is gone (deleted or renamed).
    fn follow_target(&mut self, table: &DataTable) -> bool {
        self.at = self.at.min(table.n_cols());
        let Some(j) = self.target else { return true };
        if table.column(j).is_some_and(|c| c.name == self.name) {
            return true;
        }
        match table.find(&self.name) {
            Some(k) => {
                self.target = Some(k);
                self.preview_key = None;
                true
            }
            None => false,
        }
    }

    /// Show the editor over `table`. Apply returns the edit to make, with a
    /// message for the status bar.
    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<(Edit, String)> {
        if !self.follow_target(table) {
            return Outcome::Cancel;
        }
        let key = (self.text.clone(), table.revision());
        if self.preview_key.as_ref() != Some(&key) {
            let rows = 0..PREVIEW_ROWS.min(table.n_rows());
            self.preview = Some(formula::evaluate(table, self.target, &self.text, rows));
            self.preview_key = Some(key);
        }
        let title = match self.target {
            Some(_) => format!("Formula for “{}”", self.name),
            None => "New formula column".to_owned(),
        };
        let verb = if self.confirm.is_some() { "Apply anyway" } else { "Apply" };
        let dialog = Dialog::new(title)
            .id_salt(("formula_dialog", self.document))
            .without_rows()
            .without_reset()
            .default_width(760.0)
            .action(verb);
        match dialog.show(ctx, |ui| self.contents(ui, table)) {
            Outcome::Run((), place) => match self.apply(table) {
                Some(edit) => Outcome::Run(edit, place),
                None => Outcome::Open,
            },
            Outcome::Cancel => Outcome::Cancel,
            Outcome::Open | Outcome::Reset => Outcome::Open,
        }
    }

    fn contents(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<()> {
        let editor_id = egui::Id::new(("formula_text", self.document));
        kit::note(
            ui,
            "Computed for every row and recomputed whenever the data changes. Columns with plain names are \
             variables; any column can be read with col(\"name\").",
        );
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label("Column");
            if self.target.is_none() {
                ui.add(egui::TextEdit::singleline(&mut self.name).desired_width(kit::em(ui) * 13.0));
            } else {
                ui.strong(&self.name);
            }
            ui.add_space(12.0);
            ui.label("Store as");
            egui::ComboBox::from_id_salt(("formula_storage", self.document))
                .selected_text(self.storage.label())
                .show_list(ui, |ui| {
                    for choice in Storage::choices() {
                        if ui.selectable_label(self.storage.same(choice), choice.label()).clicked() {
                            self.storage = choice;
                            self.confirm = None;
                        }
                    }
                });
        });
        ui.add_space(4.0);
        ui.columns(3, |cols| {
            self.column_browser(&mut cols[0], table, editor_id);
            self.function_browser(&mut cols[1], editor_id);
            self.help(&mut cols[2]);
        });
        ui.add_space(4.0);
        ui.label(RichText::new("Formula").strong());
        let output = egui::TextEdit::multiline(&mut self.text)
            .id(editor_id)
            .code_editor()
            .desired_rows(4)
            .desired_width(f32::INFINITY)
            .hint_text("for example  Weight / (Height / 100) ** 2")
            .show(ui);
        if output.response.changed() {
            self.confirm = None;
        }
        if let Some(range) = output.cursor_range {
            self.cursor = range.primary.ccursor.index;
        }
        if self.focus {
            output.response.request_focus();
            self.focus = false;
        }
        self.problems(ui);
        self.typeset_view(ui);
        ui.separator();
        self.preview_table(ui, table);
        let name = self.name.trim();
        if name.is_empty() {
            return Err("Name the column".into());
        }
        if self.text.trim().is_empty() {
            return Err("Write a formula".into());
        }
        if !matches!(&self.preview, Some(Ok(_))) {
            return Err("Correct the formula".into());
        }
        let summary = match self.target {
            Some(_) => format!("Set the formula of “{name}”"),
            None => format!("Add the formula column “{name}”"),
        };
        kit::ready((), summary)
    }

    /// Check the whole table, then build the edit (or ask to confirm
    /// failures first).
    fn apply(&mut self, table: &DataTable) -> Option<(Edit, String)> {
        let name = self.name.trim().to_owned();
        if self.target.is_none() && table.find(&name).is_some() {
            self.confirm = None;
            self.preview = Some(Err(FormulaError {
                message: format!("there is already a column named “{name}”"),
                position: None,
            }));
            self.preview_key = None;
            return None;
        }
        if let Err(e) = formula::check(table, self.target, &self.text) {
            self.preview = Some(Err(e));
            return None;
        }
        if self.confirm.is_none() {
            let all = formula::evaluate(table, self.target, &self.text, 0..table.n_rows()).ok()?;
            if let Some(summary) = all.failure_summary() {
                self.confirm = Some(summary);
                return None;
            }
        }
        let kind = match self.storage {
            Storage::Automatic => None,
            Storage::Kind(k) => Some(match (k, self.target.and_then(|j| table.column(j)).and_then(|c| c.temporal())) {
                // Keep the column's own date display when only the format matches.
                (ColumnKind::DateTime(d), Some(current)) if current.format == d.format => ColumnKind::DateTime(current),
                _ => k,
            }),
        };
        match Edit::formula_column(table, self.target, &name, self.at, self.text.trim(), kind) {
            Ok(edit) => {
                let message = match self.target {
                    Some(_) => format!("Formula of “{name}” set. Undo puts back the previous values."),
                    None => format!("Added formula column “{name}”. Undo removes it."),
                };
                Some((edit, message))
            }
            Err(e) => {
                self.preview = Some(Err(FormulaError { message: e.to_string(), position: None }));
                None
            }
        }
    }

    fn column_browser(&mut self, ui: &mut egui::Ui, table: &DataTable, editor_id: egui::Id) {
        ui.label(RichText::new("Columns").strong());
        ui.label(RichText::new("Click to insert").weak().small());
        egui::ScrollArea::vertical().id_salt("formula_columns").max_height(kit::em(ui) * 12.0).show(ui, |ui| {
            for (j, c) in table.columns().iter().enumerate() {
                if Some(j) == self.target {
                    continue;
                }
                let reference = formula::column_reference(&c.name);
                let mark = if c.formula.is_some() { "  ƒ" } else { "" };
                let level = c.level.label();
                let label = format!("{}{mark}", c.name);
                let hover =
                    format!("Inserts {reference}\n{level} column of {}", crate::table_view::kind_description(c.kind()));
                if ui.selectable_label(false, label).on_hover_text(hover).clicked() {
                    self.insert(ui.ctx(), editor_id, &reference);
                }
            }
        });
    }

    fn function_browser(&mut self, ui: &mut egui::Ui, editor_id: egui::Id) {
        ui.label(RichText::new("Functions").strong());
        ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text("Search").desired_width(f32::INFINITY));
        let filter = self.filter.to_lowercase();
        egui::ScrollArea::vertical().id_salt("formula_functions").max_height(kit::em(ui) * 11.0).show(ui, |ui| {
            let mut categories: Vec<&str> = Vec::new();
            for info in FUNCTIONS {
                if !categories.contains(&info.category) {
                    categories.push(info.category);
                }
            }
            for category in categories {
                let matching: Vec<_> = FUNCTIONS
                    .iter()
                    .filter(|f| f.category == category)
                    .filter(|f| {
                        filter.is_empty()
                            || f.template.to_lowercase().contains(&filter)
                            || f.help.to_lowercase().contains(&filter)
                    })
                    .collect();
                if matching.is_empty() {
                    continue;
                }
                egui::CollapsingHeader::new(category)
                    .id_salt(("formula_category", category))
                    .open((!filter.is_empty()).then_some(true))
                    .show(ui, |ui| {
                        for info in matching {
                            let text = RichText::new(info.template).monospace().small();
                            if ui.selectable_label(false, text).on_hover_text(info.help).clicked() {
                                self.insert(ui.ctx(), editor_id, info.template);
                            }
                        }
                    });
            }
        });
    }

    fn help(&self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Writing formulas").strong());
        for line in [
            "+ − * / ** for arithmetic; / always divides exactly.",
            "== != < <= > >= to compare; && || ! to combine.",
            "if … { … } else { … } to choose; let y = …; for steps.",
            "Text in quotes; + joins text.",
            "Dates are seconds since 1970-01-01.",
            "A missing number is NaN; a missing text is \"\".",
            "A row that fails gives a missing value.",
        ] {
            ui.label(RichText::new(line).small());
        }
    }

    /// The compile error at its place in the formula, or the failures found
    /// on Apply.
    fn problems(&self, ui: &mut egui::Ui) {
        match &self.preview {
            // An empty formula is the note under the settings ("Write a formula").
            Some(Err(_)) if self.text.trim().is_empty() => {}
            Some(Err(e)) => {
                ui.colored_label(theme::ERROR, format!("⚠ {e}"));
                if let Some((line, column)) = e.position
                    && let Some(text) = self.text.lines().nth(line.saturating_sub(1))
                {
                    let caret = format!("{}^", " ".repeat(column.saturating_sub(1)));
                    ui.label(RichText::new(format!("{text}\n{caret}")).monospace().color(theme::ERROR));
                }
            }
            Some(Ok(_)) => {
                if let Some(summary) = &self.confirm {
                    ui.colored_label(theme::ERROR, format!("⚠ {summary}. Apply anyway, or correct the formula."));
                }
            }
            None => {}
        }
    }

    /// The formula drawn as mathematics (fraction bars, powers, roots,
    /// sums, row subscripts) when it compiles and is a single expression.
    fn typeset_view(&self, ui: &mut egui::Ui) {
        if !matches!(&self.preview, Some(Ok(_))) {
            return;
        }
        let Some(markup) = formula_typeset::typeset(&self.text) else { return };
        let name = self.name.trim();
        let name = if name.is_empty() { "Result" } else { name };
        let math = stat_viz::MathText::parse(&format!("{} = {markup}", formula_typeset::escape(name)));
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Reads as").weak())
                .on_hover_text("The formula drawn as mathematics. Parentheses show how it is computed.");
            let font = egui::FontId::proportional(17.0 * theme::text_scale(ui));
            let galley = stat_viz::math_text::egui_text::galley(ui.ctx(), &math, &font, ui.visuals().strong_text_color());
            let spoken = format!("Typeset formula: {}", math.plain_text());
            egui::ScrollArea::horizontal().id_salt(("formula_typeset", self.document)).show(ui, |ui| {
                ui.add(egui::Label::new(egui::WidgetText::Galley(galley)).selectable(false))
                    .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &spoken));
            });
        });
    }

    fn preview_table(&self, ui: &mut egui::Ui, table: &DataTable) {
        ui.label(RichText::new("Preview").strong());
        let Some(Ok(eval)) = &self.preview else {
            ui.label(RichText::new("Write a formula to see its first values.").weak().small());
            return;
        };
        let used: Vec<usize> = formula::references(table, &self.text).into_iter().take(4).collect();
        let mono = |s: String| RichText::new(s).text_style(TextStyle::Monospace);
        egui::Grid::new(("formula_preview", self.document)).striped(true).min_col_width(60.0).show(ui, |ui| {
            ui.label(RichText::new("Row").weak());
            for &j in &used {
                ui.label(RichText::new(&table.columns()[j].name).weak());
            }
            ui.strong(if self.name.trim().is_empty() { "Result" } else { self.name.trim() });
            ui.end_row();
            for (k, value) in eval.values.iter().enumerate() {
                let failure = eval.failures.iter().find(|(r, _)| *r == k);
                ui.label(RichText::new((k + 1).to_string()).weak());
                for &j in &used {
                    ui.label(mono(table.columns()[j].cell_text(k)));
                }
                match failure {
                    Some((_, message)) => {
                        ui.colored_label(theme::ERROR, "⚠ error").on_hover_text(message);
                    }
                    None => {
                        let text = match (self.storage, value) {
                            (Storage::Kind(ColumnKind::DateTime(d)), formula::FormulaValue::Number(x)) => d.text(*x),
                            _ => value.text(),
                        };
                        ui.label(mono(text));
                    }
                }
                ui.end_row();
            }
        });
        if let Some((row, message)) = eval.failures.first() {
            ui.colored_label(theme::ERROR, format!("⚠ Row {}: {message}", row + 1));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    fn table(names: &[&str]) -> DataTable {
        DataTable::new("t", names.iter().map(|n| Column::numeric(*n, vec![1.0, 2.0])).collect()).unwrap()
    }

    #[test]
    fn the_editor_follows_its_column_when_columns_move() {
        let mut d = FormulaDialog::new(1, &table(&["a", "b", "c"]), Some(1), 0);
        // A column inserted before "b" while the editor is open.
        assert!(d.follow_target(&table(&["new", "a", "b", "c"])));
        assert_eq!(d.target, Some(2));
        // "b" deleted: the editor closes rather than writing to another column.
        assert!(!d.follow_target(&table(&["new", "a", "c"])));
    }

    #[test]
    fn a_new_column_position_stays_inside_the_table() {
        let mut d = FormulaDialog::new(1, &table(&["a", "b", "c"]), None, 3);
        assert!(d.follow_target(&table(&["a"])));
        assert_eq!(d.at, 1);
    }
}
