//! The Open Data dialog: shown before a text file, workbook, web-page table
//! or pasted text becomes a table, so the user can say where the table sits
//! (which row holds the column names, where the values start and end, which
//! columns to read) while a preview shades the cells each choice takes in.
//! The block logic is `app_model::open_data`; this module only draws it.

use std::path::PathBuf;

use app_model::external::{ExternalSource, Secrets};
use app_model::file_formats::{FileKind, name_from_path, text_sheet};
use app_model::files::{RawFile, sheet_table_name};
use app_model::open_data::{
    Block, CellRole, OpenSettings, RawSheet, detect_settings, read_block, stack_sheets, table_from_sheet,
};
use eframe::egui::{self, RichText};
use stat_data::{DataTable, JsonValue};

use crate::dialog_kit::{self as kit, ComboExt, Dialog, Outcome};

/// Rows the preview shows until "Show every row" is ticked.
const PREVIEW_ROWS: usize = 200;
/// Columns the preview shows at most.
const PREVIEW_COLUMNS: usize = 200;

/// Where the cells came from, and so what opening them makes.
pub enum Origin {
    /// A file on disk.
    File(PathBuf),
    /// The system clipboard (Paste as new table).
    Clipboard,
    /// A web address: the table remembers its source for Refresh from source.
    Web { source: Box<ExternalSource>, secrets: Secrets },
}

/// One table the dialog opened.
pub struct OpenedTable {
    pub table: DataTable,
    pub notes: Vec<String>,
    /// The settings it was read with.
    pub settings: OpenSettings,
    /// The map a script's `open_table` takes to read it the same way, when
    /// the settings differ from the detected ones (or several sheets were
    /// stacked).
    pub options: Option<JsonValue>,
}

/// What the dialog produced this frame.
pub enum OpenDataOutcome {
    Open,
    Cancel,
    /// Open these tables (several for a workbook's sheets).
    Done {
        origin: Origin,
        tables: Vec<OpenedTable>,
    },
}

/// The column separators offered for text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Separator {
    Detect,
    Given(char),
}

const SEPARATORS: [(char, &str); 5] =
    [(',', "Comma"), ('\t', "Tab"), (';', "Semicolon"), ('|', "Vertical bar"), (' ', "Space")];

fn separator_name(c: char) -> &'static str {
    SEPARATORS.iter().find(|(d, _)| *d == c).map_or("Other", |(_, n)| n)
}

/// Text that can be split again when the separator changes.
struct TextInput {
    kind: FileKind,
    text: String,
    separator: Separator,
    /// The separator the text is split at now.
    used: char,
}

/// What a click in the preview sets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ClickSets {
    Corner,
    NamesRow,
    FirstValuesRow,
    LastValuesRow,
    FirstColumn,
    LastColumn,
}

impl ClickSets {
    const ALL: [Self; 6] =
        [Self::Corner, Self::NamesRow, Self::FirstValuesRow, Self::LastValuesRow, Self::FirstColumn, Self::LastColumn];

    fn label(self) -> &'static str {
        match self {
            Self::Corner => "Top-left cell of the table",
            Self::NamesRow => "Row of column names",
            Self::FirstValuesRow => "First row of values",
            Self::LastValuesRow => "Last row of values",
            Self::FirstColumn => "First column",
            Self::LastColumn => "Last column",
        }
    }

    /// Apply a click on 1-based `row`, `col`.
    fn apply(self, block: &mut Block, row: usize, col: usize) {
        match self {
            Self::Corner => {
                block.first_column = col;
                if block.last_column.is_some_and(|l| l < col) {
                    block.last_column = None;
                }
                block.set_header_row(Some(row));
                block.data_start = block.header_end().map_or(row, |e| e + 1);
                if block.data_end.is_some_and(|e| e < block.data_start) {
                    block.data_end = None;
                }
            }
            Self::NamesRow => block.set_header_row(Some(row)),
            Self::FirstValuesRow => block.set_data_start(row),
            Self::LastValuesRow => block.data_end = Some(row.max(block.data_start)),
            Self::FirstColumn => {
                block.first_column = col;
                if block.last_column.is_some_and(|l| l < col) {
                    block.last_column = None;
                }
            }
            Self::LastColumn => block.last_column = Some(col.max(block.first_column)),
        }
    }
}

struct SheetChoice {
    sheet: RawSheet,
    open: bool,
    settings: OpenSettings,
    detected: OpenSettings,
}

pub struct OpenDataDialog {
    origin: Option<Origin>,
    /// The table name (the file's name).
    name: String,
    text: Option<TextInput>,
    sheets: Vec<SheetChoice>,
    workbook: bool,
    current: usize,
    same_for_all: bool,
    stack: bool,
    sheet_column: String,
    show_all: bool,
    click_sets: ClickSets,
    /// The extra missing markers as typed (comma-separated).
    missing: String,
    /// How the cells were read (decoding, which table of a page).
    notes: Vec<String>,
    error: Option<String>,
    /// What the current choices give: (settings and sheet they were made
    /// for, the summary or what is wrong).
    summary: Option<(SummaryKey, Result<String, String>)>,
    /// The open project the tables can join instead of starting a project
    /// of their own: its id, its name, and whether they join it.
    join: Option<(u64, String, bool)>,
}

/// What a readiness summary was made for: the sheet shown, its settings,
/// and the stacking and same-settings choices.
type SummaryKey = (usize, OpenSettings, bool, bool);

impl OpenDataDialog {
    fn new(origin: Origin, name: String) -> Self {
        Self {
            origin: Some(origin),
            name,
            text: None,
            sheets: Vec::new(),
            workbook: false,
            current: 0,
            same_for_all: true,
            stack: false,
            sheet_column: "Sheet".to_owned(),
            show_all: false,
            click_sets: ClickSets::Corner,
            missing: String::new(),
            notes: Vec::new(),
            error: None,
            summary: None,
            join: None,
        }
    }

    /// Offer to add the tables to open project `id` (named `name`),
    /// ticked when `on`. Tables from the web always start a project of
    /// their own (they keep their source to refresh from).
    pub fn offer_project(&mut self, id: u64, name: String, on: bool) {
        if !matches!(self.origin, Some(Origin::Web { .. })) {
            self.join = Some((id, name, on));
        }
    }

    /// The project the tables are to join, if the box is ticked.
    pub fn joining_project(&self) -> Option<u64> {
        self.join.as_ref().filter(|j| j.2).map(|j| j.0)
    }

    /// The dialog for a data file read as cells.
    pub fn from_file(path: PathBuf, raw: RawFile) -> Result<Self, String> {
        let mut d = Self::new(Origin::File(path.clone()), name_from_path(&path));
        match raw {
            RawFile::Text { kind, text, notes } => {
                d.notes = notes;
                d.text = Some(TextInput { kind, text, separator: Separator::Detect, used: ',' });
                d.split()?;
            }
            RawFile::Workbook(sheets) => {
                d.workbook = true;
                d.sheets = sheets
                    .into_iter()
                    .map(|sheet| {
                        let detected = detect_settings(&sheet, None);
                        SheetChoice { open: !sheet.is_blank(), settings: detected.clone(), detected, sheet }
                    })
                    .collect();
                if d.sheets.iter().all(|s| !s.open) {
                    return Err(format!("{} has no data", path.display()));
                }
                d.current = d.sheets.iter().position(|s| s.open).unwrap_or(0);
                // One setting for all sheets when they all look alike.
                d.same_for_all = d.sheets.iter().filter(|s| s.open).all(|s| s.detected == d.sheets[d.current].detected);
            }
        }
        Ok(d)
    }

    /// The dialog for pasted text.
    pub fn from_clipboard(text: String) -> Result<Self, String> {
        if text.trim().is_empty() {
            return Err("the clipboard holds no text".to_owned());
        }
        let mut d = Self::new(Origin::Clipboard, "Pasted".to_owned());
        d.text = Some(TextInput { kind: FileKind::DelimitedText, text, separator: Separator::Detect, used: '\t' });
        d.split()?;
        Ok(d)
    }

    /// The dialog for a web-page table or delimited text fetched from the web.
    pub fn from_web(
        source: ExternalSource,
        secrets: Secrets,
        name: String,
        sheet: RawSheet,
        delimiter: Option<char>,
        settings: OpenSettings,
        notes: Vec<String>,
    ) -> Self {
        let detected = match delimiter {
            Some(d) => detect_settings(&sheet, Some(d)),
            None => settings.clone(),
        };
        let mut d = Self::new(Origin::Web { source: Box::new(source), secrets }, name);
        d.missing = settings.values.missing.join(", ");
        d.notes = notes;
        d.sheets = vec![SheetChoice { sheet, open: true, settings, detected }];
        d
    }

    /// Split the text at the chosen separator and detect the block again.
    fn split(&mut self) -> Result<(), String> {
        let Some(text) = &mut self.text else { return Ok(()) };
        let given = match text.separator {
            Separator::Detect => None,
            Separator::Given(c) => Some(c),
        };
        // Pasted text is tab-separated when it has a tab.
        let given =
            given.or((matches!(self.origin, Some(Origin::Clipboard)) && text.text.contains('\t')).then_some('\t'));
        let (sheet, used) = text_sheet(text.kind, &self.name, &text.text, given).map_err(|e| e.to_string())?;
        text.used = used;
        let mut detected = detect_settings(&sheet, Some(used));
        detected.delimiter = given;
        let mut settings = detected.clone();
        settings.values.missing = self.missing_list();
        self.sheets = vec![SheetChoice { sheet, open: true, settings, detected }];
        self.current = 0;
        self.summary = None;
        Ok(())
    }

    fn missing_list(&self) -> Vec<String> {
        self.missing.split(',').map(str::trim).filter(|m| !m.is_empty()).map(str::to_owned).collect()
    }

    /// The block of the sheet shown in the preview.
    #[cfg(test)]
    pub fn current_block(&self) -> Block {
        self.sheets[self.current].settings.block
    }

    /// Whether every table to open starts in its sheet's top-left cell with
    /// nothing else to decide, so it can open without the dialog.
    pub fn is_plain(&self) -> bool {
        self.sheets
            .iter()
            .filter(|s| s.open)
            .all(|s| s.settings.block.is_plain(&s.sheet) && !s.settings.values.decimal_comma)
    }

    /// The settings the user is changing: the current sheet's (and so every
    /// sheet's, when one setting serves them all).
    fn edit_settings(&mut self, f: impl Fn(&mut OpenSettings)) {
        let same = self.same_for_all;
        let current = self.current;
        for (i, s) in self.sheets.iter_mut().enumerate() {
            if i == current || (same && s.open) {
                f(&mut s.settings);
            }
        }
    }

    /// Read every chosen table.
    pub fn build(&self) -> Result<Vec<OpenedTable>, String> {
        let chosen: Vec<&SheetChoice> = self.sheets.iter().filter(|s| s.open && !s.sheet.is_blank()).collect();
        if chosen.is_empty() {
            return Err("choose at least one sheet".to_owned());
        }
        let options = |s: &SheetChoice, sheet_key: bool| -> Option<JsonValue> {
            let mut wanted = s.settings.clone();
            // Scripts detect the separator the same way; name it only when chosen.
            wanted.delimiter = s.settings.delimiter;
            let changed = wanted != s.detected;
            if !changed {
                return None;
            }
            let JsonValue::Object(mut members) = wanted.to_json() else { return None };
            if sheet_key {
                members.insert(0, ("sheet".to_owned(), JsonValue::String(s.sheet.name.clone())));
            }
            Some(JsonValue::Object(members))
        };
        if self.workbook && self.stack && chosen.len() > 1 {
            let pairs: Vec<(&RawSheet, &OpenSettings)> = chosen.iter().map(|s| (&s.sheet, &s.settings)).collect();
            let (table, notes) = stack_sheets(&self.name, &pairs, &self.sheet_column).map_err(|e| e.to_string())?;
            let JsonValue::Object(mut members) = chosen[0].settings.to_json() else { unreachable!("a map") };
            let names = chosen.iter().map(|s| JsonValue::String(s.sheet.name.clone())).collect();
            members.splice(
                0..0,
                [
                    ("sheets".to_owned(), JsonValue::Array(names)),
                    ("stack".to_owned(), JsonValue::Bool(true)),
                    ("sheet_column".to_owned(), JsonValue::String(self.sheet_column.clone())),
                ],
            );
            return Ok(vec![OpenedTable {
                table,
                notes,
                settings: chosen[0].settings.clone(),
                options: Some(JsonValue::Object(members)),
            }]);
        }
        let several = self.workbook && self.sheets.len() > 1;
        chosen
            .iter()
            .map(|s| {
                let name = if several { sheet_table_name(&self.name, &s.sheet.name) } else { self.name.clone() };
                let (table, notes) = table_from_sheet(&name, &s.sheet, &s.settings).map_err(|e| {
                    if self.workbook { format!("sheet {:?}: {e}", s.sheet.name) } else { e.to_string() }
                })?;
                Ok(OpenedTable { table, notes, settings: s.settings.clone(), options: options(s, self.workbook) })
            })
            .collect()
    }

    /// Open the tables with the settings as they are (no dialog shown).
    pub fn finish(mut self) -> Result<OpenDataOutcome, String> {
        let mut tables = self.build()?;
        if let Some(first) = tables.first_mut() {
            first.notes.splice(0..0, std::mem::take(&mut self.notes));
        }
        Ok(OpenDataOutcome::Done { origin: self.origin.take().unwrap_or(Origin::Clipboard), tables })
    }

    fn title(&self) -> String {
        match &self.origin {
            Some(Origin::File(path)) => {
                format!(
                    "Open Data: {}",
                    path.file_name().map_or_else(String::new, |f| f.to_string_lossy().into_owned())
                )
            }
            Some(Origin::Clipboard) => "Open Data: pasted text".to_owned(),
            Some(Origin::Web { source, .. }) => format!("Open Data: {}", source.describe()),
            None => "Open Data".to_owned(),
        }
    }

    pub fn show(&mut self, ctx: &egui::Context) -> OpenDataOutcome {
        let title = self.title();
        let em = ctx.style().text_styles.get(&egui::TextStyle::Body).map_or(14.0, |f| f.size);
        let dialog = Dialog::new(title).id_salt("open-data").creates().action("Open").default_width(em * 62.0);
        let outcome = dialog.show(ctx, |ui| self.contents(ui));
        match outcome {
            Outcome::Open => OpenDataOutcome::Open,
            Outcome::Cancel => OpenDataOutcome::Cancel,
            Outcome::Reset => {
                // Back to what was detected.
                self.missing.clear();
                if let Some(t) = &mut self.text {
                    t.separator = Separator::Detect;
                }
                if self.text.is_some() {
                    if let Err(e) = self.split() {
                        self.error = Some(e);
                    }
                } else {
                    for s in &mut self.sheets {
                        s.settings = s.detected.clone();
                    }
                }
                self.stack = false;
                self.summary = None;
                OpenDataOutcome::Open
            }
            Outcome::Run((), _) => match self.build() {
                Ok(mut tables) => {
                    if let Some(first) = tables.first_mut() {
                        first.notes.splice(0..0, std::mem::take(&mut self.notes));
                    }
                    OpenDataOutcome::Done { origin: self.origin.take().unwrap_or(Origin::Clipboard), tables }
                }
                Err(e) => {
                    self.error = Some(e);
                    OpenDataOutcome::Open
                }
            },
        }
    }

    fn contents(&mut self, ui: &mut egui::Ui) -> kit::Readiness<()> {
        let em = kit::em(ui);
        if let Some((_, name, on)) = &mut self.join {
            ui.checkbox(on, format!("Add to the open project “{name}”")).on_hover_text(
                "Ticked: the tables join that project and are saved with it. Cleared: they start a project of \
                 their own.",
            );
            ui.add_space(4.0);
        }
        if self.workbook {
            self.sheet_list(ui);
            ui.add_space(4.0);
        }
        if self.text.is_some() {
            self.text_options(ui);
            ui.add_space(4.0);
        }
        let Some(current) = self.sheets.get(self.current) else {
            return Err("There is nothing to open".into());
        };
        let (sheet_name, n_rows) = (current.sheet.name.clone(), current.sheet.n_rows());
        ui.horizontal(|ui| {
            if self.workbook {
                ui.label(RichText::new(format!("Sheet “{sheet_name}”")).strong());
                ui.add_space(12.0);
            }
            ui.label("A click in the preview sets");
            egui::ComboBox::from_id_salt("open-data-click")
                .selected_text(self.click_sets.label())
                .width(em * 15.0)
                .show_list(ui, |ui| {
                    for mode in ClickSets::ALL {
                        ui.selectable_value(&mut self.click_sets, mode, mode.label());
                    }
                });
        });
        let current = &self.sheets[self.current];
        if let Some((row, col)) = preview(ui, &current.sheet, &current.settings.block, self.show_all, em) {
            let mode = self.click_sets;
            self.edit_settings(|s| mode.apply(&mut s.block, row, col));
        }
        let shown = n_rows.min(if self.show_all { usize::MAX } else { PREVIEW_ROWS });
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(format!(
                    "Rows 1–{shown} of {n_rows} · bold, underlined and shaded: column names (▸ beside the row number) \
                     · shaded: values · faint: not read"
                ))
                .weak()
                .small(),
            );
            if n_rows > PREVIEW_ROWS {
                ui.checkbox(&mut self.show_all, "Show every row");
            }
        });
        ui.add_space(4.0);
        self.block_controls(ui);
        if let Some(e) = &self.error {
            ui.add(
                egui::Label::new(RichText::new(format!("Could not open: {e}")).color(ui.visuals().error_fg_color))
                    .wrap(),
            );
        }
        for note in &self.notes {
            kit::note(ui, note);
        }
        self.readiness()
    }

    /// Workbooks: which sheets to open, one setting or one per sheet, and stacking.
    fn sheet_list(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Sheets").strong());
        let mut select = None;
        ui.horizontal_wrapped(|ui| {
            for (i, s) in self.sheets.iter_mut().enumerate() {
                let blank = s.sheet.is_blank();
                ui.add_enabled_ui(!blank, |ui| {
                    ui.checkbox(&mut s.open, "").widget_info(|| {
                        egui::WidgetInfo::selected(
                            egui::WidgetType::Checkbox,
                            !blank,
                            s.open,
                            format!("Open sheet {}", s.sheet.name),
                        )
                    });
                });
                let label = if blank {
                    format!("{} (empty)", s.sheet.name)
                } else {
                    format!("{} ({} × {})", s.sheet.name, s.sheet.n_rows(), s.sheet.n_cols())
                };
                if ui
                    .selectable_label(i == self.current, label)
                    .on_hover_text("Show this sheet in the preview")
                    .clicked()
                {
                    select = Some(i);
                }
                ui.add_space(8.0);
            }
        });
        if let Some(i) = select {
            self.current = i;
        }
        let several = self.sheets.iter().filter(|s| !s.sheet.is_blank()).count() > 1;
        if several {
            let before = self.same_for_all;
            ui.checkbox(&mut self.same_for_all, "Use the same settings for every chosen sheet")
                .on_hover_text("Off: each sheet keeps its own header row, first row of values and columns");
            if self.same_for_all && !before {
                let settings = self.sheets[self.current].settings.clone();
                for s in self.sheets.iter_mut().filter(|s| s.open) {
                    s.settings = settings.clone();
                }
            }
            ui.horizontal(|ui| {
                ui.checkbox(
                    &mut self.stack,
                    "Stack the chosen sheets into one table, with their names in a column called",
                )
                .on_hover_text("Columns are matched by name; a sheet without a column leaves it blank");
                ui.add_enabled(
                    self.stack,
                    egui::TextEdit::singleline(&mut self.sheet_column).desired_width(kit::em(ui) * 8.0),
                )
                .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, self.stack, "Sheet name column"));
            });
        }
    }

    /// Text: separator, decimal mark and extra missing markers.
    fn text_options(&mut self, ui: &mut egui::Ui) {
        let Some(text) = &self.text else { return };
        let (separator, used) = (text.separator, text.used);
        let mut new_separator = separator;
        let detected_decimal = self.sheets.first().is_some_and(|s| s.detected.values.decimal_comma);
        ui.horizontal_wrapped(|ui| {
            ui.label("Columns separated by");
            let shown = match separator {
                Separator::Detect => format!("Detect ({})", separator_name(used).to_lowercase()),
                Separator::Given(c) => separator_name(c).to_owned(),
            };
            egui::ComboBox::from_id_salt("open-data-separator").selected_text(shown).show_list(ui, |ui| {
                ui.selectable_value(&mut new_separator, Separator::Detect, "Detect");
                for (c, name) in SEPARATORS {
                    ui.selectable_value(&mut new_separator, Separator::Given(c), name);
                }
            });
            ui.add_space(12.0);
            ui.label("Decimal mark");
            let mut comma = self.sheets.get(self.current).is_some_and(|s| s.settings.values.decimal_comma);
            let before = comma;
            let tag = |is_comma: bool| if is_comma == detected_decimal { " (detected)" } else { "" };
            ui.radio_value(&mut comma, false, format!("Point 1.5{}", tag(false)));
            ui.radio_value(&mut comma, true, format!("Comma 1,5{}", tag(true)));
            if comma != before {
                self.edit_settings(|s| s.values.decimal_comma = comma);
            }
        });
        if new_separator != separator {
            if let Some(t) = &mut self.text {
                t.separator = new_separator;
            }
            if let Err(e) = self.split() {
                self.error = Some(e);
            }
        }
        ui.horizontal(|ui| {
            let label = ui.label("Also read as missing:");
            let edit = ui.add(
                egui::TextEdit::singleline(&mut self.missing)
                    .hint_text("for example -, n.a., 999")
                    .desired_width(kit::em(ui) * 16.0),
            );
            edit.clone()
                .labelled_by(label.id)
                .on_hover_text("Texts separated by commas; NA, N/A, null and blanks always are");
            if edit.changed() {
                let list = self.missing_list();
                self.edit_settings(|s| s.values.missing = list.clone());
            }
        });
    }

    /// Where the table sits, as numbers.
    fn block_controls(&mut self, ui: &mut egui::Ui) {
        let Some(current) = self.sheets.get(self.current) else { return };
        let (n_rows, n_cols) = (current.sheet.n_rows().max(1), current.sheet.n_cols().max(1));
        let mut b = current.settings.block;
        let before = b;
        egui::Grid::new("open-data-block").num_columns(4).spacing([10.0, 4.0]).show(ui, |ui| {
            let mut names = b.header_row.is_some();
            ui.checkbox(&mut names, "Column names in row");
            let mut row = b.header_row.unwrap_or(b.data_start.saturating_sub(1).max(1));
            let field = ui.add_enabled(names, egui::DragValue::new(&mut row).range(1..=n_rows));
            field.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::DragValue, names, "Column names in row"));
            if names != b.header_row.is_some() || (names && Some(row) != b.header_row) {
                b.set_header_row(names.then_some(row));
            }
            let label = ui.label("Rows of names");
            let mut count = b.header_rows;
            ui.add_enabled(names, egui::DragValue::new(&mut count).range(1..=20)).labelled_by(label.id);
            if count != b.header_rows {
                b.header_rows = count;
                b.set_header_row(b.header_row);
            }
            ui.end_row();

            let label = ui.label("Values from row");
            let mut start = b.data_start;
            ui.add(egui::DragValue::new(&mut start).range(1..=n_rows)).labelled_by(label.id);
            if start != b.data_start {
                b.set_data_start(start);
            }
            let mut stop = b.data_end.is_some();
            ui.checkbox(&mut stop, "to row");
            let mut end = b.data_end.unwrap_or(n_rows);
            ui.add_enabled(stop, egui::DragValue::new(&mut end).range(b.data_start..=n_rows))
                .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::DragValue, stop, "Last row of values"));
            b.data_end = stop.then_some(end.max(b.data_start));
            ui.end_row();

            let label = ui.label("Columns from");
            let mut first = b.first_column;
            ui.add(egui::DragValue::new(&mut first).range(1..=n_cols)).labelled_by(label.id);
            if first != b.first_column {
                b.first_column = first;
                if b.last_column.is_some_and(|l| l < first) {
                    b.last_column = None;
                }
            }
            let mut stop = b.last_column.is_some();
            ui.checkbox(&mut stop, "to column");
            let mut last = b.last_column.unwrap_or(n_cols);
            ui.add_enabled(stop, egui::DragValue::new(&mut last).range(b.first_column..=n_cols))
                .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::DragValue, stop, "Last column"));
            b.last_column = stop.then_some(last.max(b.first_column));
            ui.end_row();
        });
        if b != before {
            self.edit_settings(|s| s.block = b);
        }
    }

    /// Ready when the current choices give a table; the summary names it.
    fn readiness(&mut self) -> kit::Readiness<()> {
        let chosen = self.sheets.iter().filter(|s| s.open && !s.sheet.is_blank()).count();
        if chosen == 0 {
            return Err("Choose at least one sheet to open".into());
        }
        let Some(current) = self.sheets.get(self.current) else { return Err("There is nothing to open".into()) };
        let key = (self.current, current.settings.clone(), current.open, self.stack);
        if self.summary.as_ref().is_none_or(|(k, _)| *k != key) {
            let result = read_block(&current.sheet, &current.settings)
                .map(|(names, rows, _)| {
                    let names = stat_data::unique_names(names);
                    let shown: Vec<&str> = names.iter().take(6).map(String::as_str).collect();
                    let more = if names.len() > 6 { format!(", … ({} in all)", names.len()) } else { String::new() };
                    format!(
                        "{} × {}: {}{more}",
                        kit::count(rows.len(), "row", "rows"),
                        kit::count(names.len(), "column", "columns"),
                        shown.join(", ")
                    )
                })
                .map_err(|e| e.to_string());
            self.summary = Some((key, result));
        }
        let Some((_, result)) = &self.summary else { unreachable!("just set") };
        match result {
            Ok(text) => {
                let what = if self.workbook && chosen > 1 {
                    if self.stack {
                        format!("Stack {chosen} sheets into “{}”", self.name)
                    } else {
                        format!("Open {chosen} sheets as tables")
                    }
                } else {
                    format!("Open “{}”", self.name)
                };
                let text = if current.open { format!("{what} · this sheet: {text}") } else { what };
                kit::ready((), text)
            }
            Err(e) => {
                if current.open {
                    Err(format!("This sheet: {e}"))
                } else {
                    kit::ready((), format!("Open {chosen} sheets"))
                }
            }
        }
    }
}

/// Draw the cells with row and column numbers, shading the header and the
/// values. Returns the 1-based cell clicked, if any.
fn preview(ui: &mut egui::Ui, sheet: &RawSheet, block: &Block, show_all: bool, em: f32) -> Option<(usize, usize)> {
    let visuals = ui.visuals().clone();
    let row_height = ui.text_style_height(&egui::TextStyle::Body) + 6.0;
    let n_rows = if show_all { sheet.n_rows() } else { sheet.n_rows().min(PREVIEW_ROWS) };
    let n_cols = sheet.n_cols().clamp(1, PREVIEW_COLUMNS);
    let gutter = em * 3.5;
    let width = em * 7.0;
    let font = egui::TextStyle::Body.resolve(ui.style());
    let mono = egui::TextStyle::Monospace.resolve(ui.style());
    let header_fill = visuals.selection.bg_fill.gamma_multiply(0.55);
    let data_fill = visuals.selection.bg_fill.gamma_multiply(0.18);
    let line = visuals.widgets.noninteractive.bg_stroke;
    let strong = visuals.strong_text_color();
    let normal = visuals.text_color();
    let faint = visuals.weak_text_color().gamma_multiply(0.7);
    let mut clicked = None;
    egui::Frame::group(ui.style()).inner_margin(2.0).show(ui, |ui| {
        let height = (row_height * (n_rows as f32 + 1.0)).clamp(row_height * 4.0, em * 22.0);
        egui::ScrollArea::both().id_salt("open-data-preview").max_height(height).auto_shrink([false, true]).show_rows(
            ui,
            row_height,
            n_rows + 1,
            |ui, range| {
                ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                for r in range {
                    ui.horizontal(|ui| {
                        // The gutter: the row number (column numbers on the first line).
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(gutter, row_height), egui::Sense::hover());
                        let painter = ui.painter_at(rect);
                        if r == 0 {
                            painter.rect_filled(rect, 0.0, visuals.faint_bg_color);
                        } else {
                            // Names rows are marked ▸ beside their number, rows not read are faint.
                            let (tag, color) = match block.role(r, block.first_column) {
                                CellRole::Header => (format!("▸ {r}"), strong),
                                CellRole::Data => (r.to_string(), normal),
                                CellRole::Outside => (r.to_string(), faint),
                            };
                            painter.rect_filled(rect, 0.0, visuals.faint_bg_color);
                            painter.text(
                                rect.right_center() - egui::vec2(4.0, 0.0),
                                egui::Align2::RIGHT_CENTER,
                                tag,
                                mono.clone(),
                                color,
                            );
                        }
                        for c in 0..n_cols {
                            let sense = if r == 0 { egui::Sense::hover() } else { egui::Sense::click() };
                            let (rect, response) = ui.allocate_exact_size(egui::vec2(width, row_height), sense);
                            let painter = ui.painter_at(rect);
                            painter.rect_stroke(rect, 0.0, line, egui::StrokeKind::Inside);
                            if r == 0 {
                                painter.rect_filled(rect.shrink(0.5), 0.0, visuals.faint_bg_color);
                                painter.text(
                                    rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    (c + 1).to_string(),
                                    mono.clone(),
                                    normal,
                                );
                                continue;
                            }
                            let (row, col) = (r, c + 1);
                            let role = block.role(row, col);
                            let text = sheet.cell(row - 1, c);
                            let (fill, color, font_id) = match role {
                                CellRole::Header => (Some(header_fill), strong, font.clone()),
                                CellRole::Data => (Some(data_fill), normal, font.clone()),
                                CellRole::Outside => (None, faint, font.clone()),
                            };
                            if let Some(fill) = fill {
                                painter.rect_filled(rect.shrink(0.5), 0.0, fill);
                            }
                            let galley = ui.fonts(|f| {
                                let mut job = egui::text::LayoutJob::single_section(
                                    text.to_owned(),
                                    egui::TextFormat { font_id, color, ..Default::default() },
                                );
                                job.wrap = egui::text::TextWrapping::truncate_at_width(width - 6.0);
                                if role == CellRole::Header {
                                    job.sections[0].format.underline = egui::Stroke::new(1.0_f32, color);
                                }
                                f.layout_job(job)
                            });
                            painter.galley(rect.left_center() + egui::vec2(3.0, -galley.size().y / 2.0), galley, color);
                            let label = format!("Row {row}, column {col}");
                            let response = response.on_hover_text(if text.is_empty() {
                                label.clone()
                            } else {
                                format!("{label}: {text}")
                            });
                            response.widget_info(|| {
                                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label.clone())
                            });
                            if response.clicked() {
                                clicked = Some((row, col));
                            }
                        }
                    });
                }
            },
        );
    });
    clicked
}
