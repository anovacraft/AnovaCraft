//! Editable, virtualized data grid. Only the rows in view are laid out each
//! frame (`egui_extras::TableBody::rows`), so large tables stay responsive.
//!
//! Editing works like a spreadsheet: a click puts the cell cursor on a cell;
//! double-click, Enter or F2 — or simply typing — edits it; Enter / Tab
//! commit and move down / right (Shift reverses), Escape cancels. Arrow keys
//! move the cursor (with ⌘/Ctrl to the edge of the table; Home / End, Page
//! Up / Down too); Shift with any of them, Shift-click or dragging across
//! cells makes a block of cells; ⌘A / Ctrl+A selects every cell. Delete
//! clears the block, ⌘D / Ctrl+D fills its first row down. Copy (⌘C /
//! Ctrl+C) puts the selected columns, the block, the selected rows (two or
//! more) or the cursor cell on the clipboard as tab-separated text; Cut also
//! clears them; Paste (⌘V / Ctrl+V) stores clipboard text with its top-left
//! value at the block's corner or the cursor (see [`app_model::clipboard`]).
//!
//! Selecting: a click on a column heading selects the column, Shift-click
//! the run of columns from the last one clicked, ⌘/Ctrl-click adds or
//! removes one, and dragging across headings selects a run. Row numbers do
//! the same for rows, and their selection is the shared row selection that
//! every plot highlights ([`app_model::brushing::row_number_click`]). A click
//! in a data cell only moves the cursor.
//!
//! Right-click a column heading for rename, sorting, type, level, insert and
//! delete; right-click a row number to select, exclude / include, hide / show,
//! insert or delete rows.
//!
//! The grid never changes the data itself: it returns [`GridRequest`]s that
//! the application applies through the undo history.

use app_model::brushing::row_number_click;
use app_model::clipboard;
use app_model::editing::Edit;
use app_model::heading_chart::HeadingSummary;
use app_model::search::{self, Query};
use eframe::egui::{self, Key, RichText, Stroke, StrokeKind};
use egui_extras::{Column as GridColumn, TableBuilder};
use stat_data::{
    ColumnKind, DataTable, MeasurementLevel, RowState, RowStateManager, SelectionMode, TemporalDisplay, TemporalFormat,
};

use crate::theme;

pub mod rank;
pub mod recode;
pub mod value_labels;

const ROW_HEIGHT: f32 = 22.0;
/// Rows moved by Page Up / Page Down.
const PAGE_ROWS: isize = 20;
/// A cursor step large enough to reach the edge of any table (clamped there).
const TO_EDGE: isize = isize::MAX / 8;

/// A change the user asked for in the grid.
pub enum GridRequest {
    /// Store typed text (already validated for the column type) in a cell.
    SetCell { row: usize, column: usize, text: String },
    /// Insert `count` empty rows before row `at`.
    InsertRows { at: usize, count: usize },
    /// Delete these rows (ascending).
    RemoveRows(Vec<usize>),
    /// Insert `count` empty columns of `kind`, the first at position `at`,
    /// named from `name` (empty: "Column 1", "Column 2", …).
    InsertColumn { at: usize, kind: ColumnKind, count: usize, name: String },
    /// Delete a column.
    RemoveColumn(usize),
    /// Delete these columns, as one undoable step.
    RemoveColumns(Vec<usize>),
    /// Rename a column.
    RenameColumn(usize, String),
    /// Convert a column to another storage type (or another date display).
    ConvertColumn(usize, ColumnKind),
    /// Exchange month and day in every value of a date column.
    SwapDayMonth(usize),
    /// Change a column's measurement level.
    SetLevel(usize, MeasurementLevel),
    /// Open the formula editor for a column (`None`: a new column inserted at `at`).
    Formula { column: Option<usize>, at: usize },
    /// Remove a column's formula, keeping its values.
    ClearFormula(usize),
    /// Paste clipboard text with its top-left value at this cell.
    Paste { row: usize, column: usize, text: String },
    /// Clear these cells (every row × column given) as one undoable edit.
    Clear { rows: Vec<usize>, columns: Vec<usize> },
    /// Copy the first of these rows into the others, in each column given.
    FillDown { rows: Vec<usize>, columns: Vec<usize> },
    /// Sort the table's rows by one column.
    Sort { column: usize, descending: bool },
    /// Open the Recode dialog for a column.
    Recode(usize),
    /// Open the Value labels dialog for a column.
    ValueLabels(usize),
    /// Show (or stop showing) a column in plot tooltips.
    SetHover(usize, bool),
    /// Open the Colour and marker by column dialog (on this column, if given).
    ColorBy(Option<usize>),
    /// Apply a ready-made edit (Replace, Recode), reporting `message` after.
    Apply { edit: Edit, message: String },
    /// Undo the last edit.
    Undo,
    /// Redo the last undone edit.
    Redo,
}

/// Cell cursor and in-progress edits for one dataset's grid.
#[derive(Default)]
pub struct GridState {
    /// The cell cursor, `(row, column)`.
    cursor: Option<(usize, usize)>,
    /// The other corner of a block of cells extended from the cursor with Shift.
    anchor: Option<(usize, usize)>,
    /// Cell being edited, with its text.
    editing: Option<CellEditor>,
    /// Column being renamed, with the new name.
    renaming: Option<(usize, String)>,
    /// Bring this row into view on the next frame.
    scroll_to: Option<usize>,
    /// Bring this column into view on the next frame.
    scroll_to_column: Option<usize>,
    /// Columns selected by clicking their headings (ascending).
    columns: Vec<usize>,
    /// The heading clicked last, where a Shift-click run of columns starts.
    column_anchor: Option<usize>,
    /// The row number clicked last, where a Shift-click run of rows starts.
    row_anchor: Option<usize>,
    /// A drag across cells, row numbers or headings in progress.
    drag: Option<Drag>,
    /// The Find bar, while it is open.
    find: Option<FindBar>,
    /// Message about the last rejected input.
    pub error: Option<String>,
    /// Each column's first width by name, at the text scale `fitted_scale`
    /// (see [`fitted_width`]; measured once, not every frame).
    fitted: std::collections::HashMap<String, f32>,
    fitted_scale: f32,
    /// The cell text pane is shown (a multi-line editor for the cursor's cell).
    pub text_pane: bool,
    /// Columns with value labels show their stored values, not the labels.
    pub show_codes: bool,
    /// Small charts of each column's values under the headings.
    pub heading_charts: bool,
    /// Each column's heading chart by name, with the data and exclusion
    /// revisions it was computed at.
    charts: std::collections::HashMap<String, (u64, u64, std::rc::Rc<HeadingSummary>)>,
    /// The text pane's cell and its text while it is being edited.
    pane_text: Option<PaneText>,
}

/// What the text pane holds: the cell it shows and its edited text.
struct PaneText {
    row: usize,
    column: usize,
    text: String,
    /// The cell's text when the pane took it (to tell edits apart).
    original: String,
}

/// The Find bar above the grid (⌘/Ctrl+F).
#[derive(Default)]
struct FindBar {
    query: Query,
    /// Text that Replace puts in place of the found text.
    replacement: String,
    /// The Replace row is shown.
    replacing: bool,
    /// Give the text field focus on the next frame.
    focus: bool,
    /// Outcome of the last search, shown beside the buttons.
    status: Option<String>,
}

/// What a drag in the grid is selecting.
enum Drag {
    /// A block of cells from the anchor to the cell under the pointer.
    Cells,
    /// The run of rows from `from` to `to` (the row under the pointer);
    /// `add` keeps the rows selected before.
    Rows { from: usize, to: usize, add: bool },
    /// The run of columns from `from` to the heading under the pointer,
    /// joined to `base` (the columns selected before, with ⌘/Ctrl).
    Columns { from: usize, base: Vec<usize> },
}

struct CellEditor {
    row: usize,
    column: usize,
    text: String,
    /// Focus has been given to the text field (done on the first frame).
    focused: bool,
    /// The text is wider than the cell (or has line breaks), so it is edited
    /// in a larger box over the grid; once open it stays open for this edit.
    expanded: bool,
}

impl GridState {
    fn begin_edit(&mut self, table: &DataTable, row: usize, column: usize, replace_with: Option<String>) {
        if let Some(c) = table.column(column).filter(|c| c.formula.is_some()) {
            self.cursor = Some((row, column));
            self.error = Some(format!(
                "“{}” is computed by a formula. Right-click its name to edit the formula, or to convert it to values.",
                c.name
            ));
            return;
        }
        let text = replace_with.unwrap_or_else(|| table.column(column).map(|c| c.cell_text(row)).unwrap_or_default());
        self.cursor = Some((row, column));
        self.editing = Some(CellEditor { row, column, text, focused: false, expanded: false });
        self.error = None;
    }

    fn move_cursor(&mut self, table: &DataTable, d_row: isize, d_col: isize) {
        self.anchor = None;
        self.columns.clear();
        self.step_cursor(table, d_row, d_col);
    }

    /// Put the cursor on a cell (from a click); with `extend`, keep or start
    /// a block from where the cursor was.
    fn click_cell(&mut self, row: usize, column: usize, extend: bool) {
        self.anchor = if extend { self.anchor.or(self.cursor) } else { None };
        self.cursor = Some((row, column));
        self.columns.clear();
        self.error = None;
    }

    /// Forget the selected columns (after they were deleted).
    pub fn clear_columns(&mut self) {
        self.columns.clear();
        self.column_anchor = None;
    }

    /// Select every cell of the table as one block.
    pub fn select_all(&mut self, table: &DataTable) {
        if table.n_rows() == 0 || table.n_cols() == 0 {
            return;
        }
        self.columns.clear();
        self.cursor = Some((0, 0));
        self.anchor = Some((table.n_rows() - 1, table.n_cols() - 1));
        self.error = None;
    }

    /// Open the Find bar (or give its text field focus if already open).
    pub fn open_find(&mut self) {
        self.find.get_or_insert_with(FindBar::default).focus = true;
    }

    /// Move the cursor to the next cell containing the Find text.
    fn find_next(&mut self, table: &DataTable) {
        let Some(find) = &mut self.find else { return };
        match search::next_match(table, &find.query, self.cursor) {
            Some((r, c)) => {
                find.status = None;
                self.cursor = Some((r, c));
                self.anchor = None;
                self.columns.clear();
                self.scroll_to = Some(r);
                self.scroll_to_column = Some(c);
            }
            None => find.status = Some("Not found".into()),
        }
    }

    /// The cell cursor, `(row, column)`.
    pub fn cursor(&self) -> Option<(usize, usize)> {
        self.cursor
    }

    /// The column the user is working in: the first selected column, else
    /// the cursor's.
    pub fn current_column(&self) -> Option<usize> {
        self.columns.first().copied().or(self.cursor.map(|(_, c)| c))
    }

    /// Columns selected by clicking their headings (ascending), which
    /// dialogs opened now start with.
    pub fn selected_columns(&self) -> &[usize] {
        &self.columns
    }

    /// Select columns `columns` as if their headings were clicked (tests).
    #[cfg(test)]
    pub fn select_columns(&mut self, columns: &[usize]) {
        self.columns = columns.to_vec();
    }

    /// Put the cell cursor on a cell (tests).
    #[cfg(test)]
    pub fn place_cursor(&mut self, row: usize, column: usize) {
        self.click_cell(row, column, false);
    }

    /// A cell is open for editing (tests).
    #[cfg(test)]
    pub fn is_editing(&self) -> bool {
        self.editing.is_some()
    }

    /// A column heading is being renamed.
    pub fn is_renaming(&self) -> bool {
        self.renaming.is_some()
    }

    /// Select column `j` alone, put the cursor in it and bring it into view
    /// (a click on the variable's name in the sidebar).
    pub fn focus_column(&mut self, j: usize) {
        let row = self.cursor.map_or(0, |(r, _)| r);
        self.cursor = Some((row, j));
        self.anchor = None;
        self.editing = None;
        self.columns = vec![j];
        self.column_anchor = Some(j);
        self.scroll_to_column = Some(j);
    }

    /// Select the run of columns from the last heading clicked to `j`
    /// (`run`), add or remove `j` alone (`toggle`), or select only `j`.
    fn click_column(&mut self, j: usize, run: bool, toggle: bool) {
        match self.column_anchor {
            Some(a) if run => {
                let span = a.min(j)..=a.max(j);
                if toggle {
                    self.columns.extend(span);
                } else {
                    self.columns = span.collect();
                }
            }
            _ if toggle => {
                if let Some(i) = self.columns.iter().position(|&c| c == j) {
                    self.columns.remove(i);
                } else {
                    self.columns.push(j);
                }
                self.column_anchor = Some(j);
            }
            _ => {
                self.columns = vec![j];
                self.column_anchor = Some(j);
            }
        }
        self.columns.sort_unstable();
        self.columns.dedup();
        // The cursor goes to the column (keeping its row), with no block.
        self.anchor = None;
        self.cursor = Some((self.cursor.map_or(0, |c| c.0), j));
        self.error = None;
    }

    /// The selected cells as rows and columns: every row of the selected
    /// columns, else the block (or the cursor cell). `None` without a cursor.
    #[must_use]
    pub fn selection(&self, table: &DataTable) -> Option<(Vec<usize>, Vec<usize>)> {
        if !self.columns.is_empty() {
            return Some(((0..table.n_rows()).collect(), self.columns.clone()));
        }
        let (r0, c0, r1, c1) = self.block()?;
        Some(((r0..=r1).collect(), (c0..=c1).collect()))
    }

    /// Move the cursor, keeping (or starting) a block from where it was.
    fn extend_block(&mut self, table: &DataTable, d_row: isize, d_col: isize) {
        self.anchor = self.anchor.or(self.cursor);
        self.step_cursor(table, d_row, d_col);
    }

    fn step_cursor(&mut self, table: &DataTable, d_row: isize, d_col: isize) {
        if table.n_rows() == 0 || table.n_cols() == 0 {
            return;
        }
        let (r, c) = self.cursor.unwrap_or((0, 0));
        let r = r.saturating_add_signed(d_row).min(table.n_rows() - 1);
        let c = c.saturating_add_signed(d_col).min(table.n_cols() - 1);
        self.cursor = Some((r, c));
        self.scroll_to = Some(r);
    }

    /// Drop references to cells that no longer exist (after rows or columns
    /// were removed, e.g. by undo).
    fn clamp(&mut self, table: &DataTable) {
        let valid = |r: usize, c: usize| r < table.n_rows() && c < table.n_cols();
        if self.cursor.is_some_and(|(r, c)| !valid(r, c)) {
            self.cursor = None;
        }
        if self.cursor.is_none() || self.anchor.is_some_and(|(r, c)| !valid(r, c)) {
            self.anchor = None;
        }
        if self.editing.as_ref().is_some_and(|e| !valid(e.row, e.column)) {
            self.editing = None;
        }
        if self.renaming.as_ref().is_some_and(|(c, _)| *c >= table.n_cols()) {
            self.renaming = None;
        }
        self.columns.retain(|&c| c < table.n_cols());
        if self.column_anchor.is_some_and(|c| c >= table.n_cols()) {
            self.column_anchor = None;
        }
        if self.row_anchor.is_some_and(|r| r >= table.n_rows()) {
            self.row_anchor = None;
        }
    }
}

impl GridState {
    /// The block of cells from the cursor to the Shift anchor, as
    /// `(first row, first column, last row, last column)`; just the cursor
    /// cell without an anchor.
    fn block(&self) -> Option<(usize, usize, usize, usize)> {
        let (r, c) = self.cursor?;
        let (ar, ac) = self.anchor.unwrap_or((r, c));
        Some((r.min(ar), c.min(ac), r.max(ar), c.max(ac)))
    }

    /// The row of the cell cursor, if there is one.
    pub fn cursor_row(&self) -> Option<usize> {
        self.cursor.map(|(r, _)| r)
    }

    /// What Copy puts on the clipboard, and a description of it: the
    /// selected columns (every row, with a line of column names); else the
    /// block of cells when it has more than one; else the selected rows when
    /// there are two or more (every column, with a line of column names);
    /// else the cursor cell. `None` with nothing to copy.
    pub fn copy_selection(&self, table: &DataTable, rows: &RowStateManager) -> Option<(String, String)> {
        if !self.columns.is_empty() {
            let all: Vec<usize> = (0..table.n_rows()).collect();
            let text = clipboard::copy_text(table, &all, &self.columns, true);
            let n = self.columns.len();
            return Some((text, format!("Copied {n} column{} with names", if n == 1 { "" } else { "s" })));
        }
        let block = self.block();
        let selected = rows.selected_rows();
        match block {
            Some((r0, c0, r1, c1)) if (r0, c0) != (r1, c1) || selected.len() < 2 => {
                let text =
                    clipboard::copy_text(table, &(r0..=r1).collect::<Vec<_>>(), &(c0..=c1).collect::<Vec<_>>(), false);
                let (h, w) = (r1 - r0 + 1, c1 - c0 + 1);
                let what = if h * w == 1 { "Copied 1 cell".to_owned() } else { format!("Copied {h} × {w} cells") };
                Some((text, what))
            }
            _ if !selected.is_empty() => Some(Self::copy_rows(table, &selected)),
            _ => None,
        }
    }

    /// What Cut takes: the selected columns (with a line of column names) or
    /// the block (or cursor cell) as clipboard text, and its rows and columns
    /// to clear. Unlike Copy it never takes whole selected rows, which are
    /// removed with Delete selected rows instead.
    #[must_use]
    pub fn cut_selection(&self, table: &DataTable) -> Option<(String, Vec<usize>, Vec<usize>)> {
        let (rows, columns) = self.selection(table)?;
        let text = clipboard::copy_text(table, &rows, &columns, !self.columns.is_empty());
        Some((text, rows, columns))
    }

    /// The given rows, every column, with a line of column names.
    pub fn copy_rows(table: &DataTable, selected: &[usize]) -> (String, String) {
        let text = clipboard::copy_text(table, selected, &(0..table.n_cols()).collect::<Vec<_>>(), true);
        let n = selected.len();
        (text, format!("Copied {n} row{} with column names", if n == 1 { "" } else { "s" }))
    }

    /// Where a paste goes: the top of the first selected column, the block's
    /// top-left cell, or the first cell of an empty table. `None` without a cursor in a table that has cells.
    #[must_use]
    pub fn paste_target(&self, table: &DataTable) -> Option<(usize, usize)> {
        if let Some(&c) = self.columns.first() {
            return Some((0, c));
        }
        match self.block() {
            Some((r, c, _, _)) => Some((r, c)),
            None if table.n_rows() == 0 => Some((0, 0)),
            None => None,
        }
    }
}

/// Draw the toolbar and grid; return the edits the user asked for.
pub fn show(
    ui: &mut egui::Ui,
    table: &DataTable,
    rows: &mut RowStateManager,
    state: &mut GridState,
    undo: Option<&str>,
    redo: Option<&str>,
) -> Vec<GridRequest> {
    let mut requests = Vec::new();
    // The pane's width before anything is laid out; the grid never grows past it.
    let pane_width = ui.available_width();
    state.clamp(table);
    if !ui.input(|i| i.pointer.primary_down()) {
        state.drag = None;
    }
    toolbar(ui, table, rows, state, undo, redo, &mut requests);
    find_bar(ui, table, rows, state, &mut requests);
    // An entry error shows beside the cell being edited instead (see `cell_editor`).
    if let Some(e) = state.error.as_ref().filter(|_| state.editing.is_none()) {
        ui.colored_label(theme::ERROR, format!("⚠ {e}"));
    }
    if state.text_pane {
        text_pane(ui, table, state, &mut requests);
    } else if let Some(pending) = state.pane_text.take() {
        commit_pane(table, state, pending, &mut requests);
    }
    keyboard(ui, table, rows, state, &mut requests);

    let modifiers = ui.input(|i| i.modifiers);
    let pointer = ui.input(|i| i.pointer.interact_pos());
    // A row-number click or drag: rows and how to apply them, after drawing.
    let mut row_pick: Option<(Vec<usize>, SelectionMode)> = None;
    // A click on a cell or a column heading ends a selection of rows.
    let mut clear_rows = false;
    let mut ops: Vec<RowOp> = Vec::new();
    let selected_rows = rows.selected_rows();
    let scroll_to = state.scroll_to.take();
    let scroll_to_column = state.scroll_to_column.take();
    let block = state.block().filter(|(r0, c0, r1, c1)| (r0, c0) != (r1, c1));
    let block_fill = ui.visuals().selection.bg_fill.gamma_multiply(0.35);
    let mut body_rect: Option<egui::Rect> = None;

    // First widths fitted to each column's heading and values.
    let scale = theme::text_scale(ui);
    if state.fitted_scale != scale {
        state.fitted.clear();
        state.fitted_scale = scale;
    }
    let widths: Vec<f32> = table
        .columns()
        .iter()
        .map(|col| match state.fitted.get(&col.name) {
            Some(&w) => w,
            None => {
                let w = fitted_width(ui, col, table.n_rows());
                state.fitted.insert(col.name.clone(), w);
                w
            }
        })
        .collect();

    // Horizontal scrolling for wide tables; the grid scrolls vertically itself.
    egui::ScrollArea::horizontal().id_salt("data_grid_hscroll").max_width(pane_width).auto_shrink([false, false]).show(
        ui,
        |ui| {
            // Plain labels, so a click anywhere on a row selects the row rather than text.
            ui.style_mut().interaction.selectable_labels = false;
            // Rows and the row-number column grow with the text-size setting.
            let scale = theme::text_scale(ui);
            let row_height = ROW_HEIGHT * scale;
            let mut builder = TableBuilder::new(ui)
                .striped(true)
                .resizable(true)
                // Dragging selects cells, rows or columns; the scroll bar and
                // wheel still scroll.
                .sense(egui::Sense::click_and_drag())
                .drag_to_scroll(false)
                .auto_shrink([false, false])
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .column(GridColumn::exact(56.0 * scale));
            for &width in &widths {
                builder = builder.column(GridColumn::initial(width).at_least(48.0).clip(true));
            }
            if let Some(r) = scroll_to {
                builder = builder.scroll_to_row(r, None);
            }
            let chart_height = if state.heading_charts { HEADING_CHART_HEIGHT * scale } else { 0.0 };
            builder
                .header(row_height + 4.0 + chart_height, |mut header| {
                    header.col(|ui| {
                        ui.strong("#").on_hover_text(
                            "Row numbers: click to select a row, Shift-click for a run of rows, ⌘/Ctrl-click to add \
                             or remove one, or drag down them. × marks an excluded row, ◌ a row hidden in plots, ⊘ a row \
                             outside the active row subset",
                        );
                    });
                    for (j, col) in table.columns().iter().enumerate() {
                        let selected = state.columns.contains(&j);
                        let mut chart = None;
                        let (_, cell) = header.col(|ui| {
                            if selected {
                                ui.painter().rect_filled(ui.max_rect(), 0.0, ui.visuals().selection.bg_fill);
                            }
                            if scroll_to_column == Some(j) {
                                ui.scroll_to_rect(ui.max_rect(), Some(egui::Align::Center));
                            }
                            if chart_height > 0.0 {
                                ui.vertical(|ui| {
                                    ui.horizontal(|ui| column_header(ui, j, col, state, &mut requests));
                                    let summary = state.heading_summary(table, rows, j);
                                    chart = Some((heading_chart(ui, col, &summary, chart_height), summary));
                                });
                            } else {
                                column_header(ui, j, col, state, &mut requests);
                            }
                        });
                        // A click on a bar of the heading chart selects its rows.
                        let on_bar = chart.as_ref().and_then(|((rect, bars), summary)| {
                            let p = cell.interact_pointer_pos().filter(|p| rect.contains(*p))?;
                            let bar = bars.iter().position(|b| b.contains(p))?;
                            Some(app_model::heading_chart::bar_rows(col, rows, summary, bar))
                        });
                        match on_bar {
                            Some(picked) if cell.clicked() => {
                                row_pick = Some((
                                    picked,
                                    app_model::brushing::selection_mode(
                                        modifiers.shift,
                                        modifiers.command,
                                        modifiers.alt,
                                    ),
                                ));
                                state.scroll_to = None;
                            }
                            _ => clear_rows |= heading_pointer(&cell, j, col, state, &mut requests, modifiers),
                        }
                    }
                })
                .body(|mut body| {
                    body_rect = Some(body.ui_mut().clip_rect());
                    body.rows(row_height, table.n_rows(), |mut row| {
                        let r = row.index();
                        let row_state = rows.state(r);
                        let outside = rows.is_outside_subset(r);
                        row.set_selected(row_state.selected);
                        let (_, number) = row.col(|ui| {
                            // Row number, then the row's state marks (no separate column for them).
                            ui.label(RichText::new((r + 1).to_string()).weak());
                            let marks = state_marks(row_state, outside);
                            if !marks.is_empty() {
                                ui.label(marks);
                            }
                        });
                        if let Some(pick) = row_number_pointer(&number, r, state, modifiers) {
                            row_pick = Some(pick);
                        }
                        number.context_menu(|ui| row_menu(ui, r, rows, &selected_rows, &mut requests, &mut ops));
                        for (j, col) in table.columns().iter().enumerate() {
                            let is_cursor = state.cursor == Some((r, j));
                            let is_editing = state.editing.as_ref().is_some_and(|e| e.row == r && e.column == j);
                            let in_block = state.columns.contains(&j)
                                || block
                                    .is_some_and(|(r0, c0, r1, c1)| (r0..=r1).contains(&r) && (c0..=c1).contains(&j));
                            let (_, cell) = row.col(|ui| {
                                if in_block {
                                    ui.painter().rect_filled(ui.max_rect(), 0.0, block_fill);
                                }
                                if is_editing {
                                    cell_editor(ui, table, state, &mut requests);
                                } else {
                                    cell_text(ui, col, r, row_state.excluded || outside, !state.show_codes);
                                }
                                if is_cursor {
                                    ui.painter().rect_stroke(
                                        ui.max_rect().shrink(1.0),
                                        2.0_f32,
                                        Stroke::new(2.0_f32, theme::DIVIDER_ACTIVE),
                                        StrokeKind::Inside,
                                    );
                                }
                            });
                            if !is_editing {
                                clear_rows |= cell_pointer(&cell, table, r, j, state, modifiers.shift);
                            }
                        }
                    });
                });
        },
    );

    // Dragging past the top or bottom of the grid keeps extending the
    // selection a row at a time, scrolling with it.
    if let (Some(rect), Some(p)) = (body_rect, pointer) {
        let step: isize = if p.y > rect.bottom() {
            1
        } else if p.y < rect.top() {
            -1
        } else {
            0
        };
        if step != 0 && table.n_rows() > 0 {
            if matches!(state.drag, Some(Drag::Cells)) {
                state.step_cursor(table, step, 0);
                ui.ctx().request_repaint();
            } else if let Some(Drag::Rows { from, to, add }) = &mut state.drag {
                *to = to.saturating_add_signed(step).min(table.n_rows() - 1);
                let (from, to, add) = (*from, *to, *add);
                state.scroll_to = Some(to);
                let mode = if add { SelectionMode::Extend } else { SelectionMode::Replace };
                row_pick = Some(((from.min(to)..=from.max(to)).collect(), mode));
                ui.ctx().request_repaint();
            }
        }
    }

    if let Some((picked, mode)) = row_pick {
        rows.select(picked, mode);
    } else if clear_rows {
        rows.clear_selection();
    }
    for op in ops {
        op.apply(rows);
    }
    requests
}

/// A cell's value as the grid shows it: numbers and dates right-aligned so
/// digits line up, a missing number or date as a faint dot, excluded rows
/// faint and italic. With `labels`, a value with a value label shows the
/// label (the stored value on hover).
fn cell_text(ui: &mut egui::Ui, col: &stat_data::Column, r: usize, excluded: bool, labels: bool) {
    let text = col.cell_text(r);
    let numeric = col.kind() != ColumnKind::Text;
    let style = |t: RichText| if excluded { t.weak().italics() } else { t };
    if let Some(label) = col.label_of(&text).filter(|_| labels) {
        ui.label(style(RichText::new(label))).on_hover_text(format!("Stored value {text}"));
    } else if !numeric {
        ui.label(style(RichText::new(text)));
    } else if text.is_empty() {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new("·").weak()).on_hover_text("Missing value");
        });
    } else {
        let short = match &col.data {
            stat_data::ColumnData::Numeric(v) => short_number(v[r]),
            _ => None,
        };
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| match short {
            Some(short) => {
                ui.label(style(RichText::new(short).monospace())).on_hover_text(text);
            }
            None => {
                ui.label(style(RichText::new(text).monospace()));
            }
        });
    }
}

/// Significant digits the grid shows of a long number (computed means and the
/// like); the full value is in the cell's tooltip, its editor and copies.
const GRID_DIGITS: usize = 8;

/// `x` rounded to [`GRID_DIGITS`] significant digits, when its full form has
/// more than two digits beyond that. Whole numbers (identifiers, counts,
/// codes) are always shown in full.
fn short_number(x: f64) -> Option<String> {
    if x.fract() == 0.0 {
        return None;
    }
    let full = stat_data::format_number(x);
    let mantissa = full.split(['e', 'E']).next().unwrap_or(&full);
    let digits = mantissa.chars().filter(char::is_ascii_digit).collect::<String>();
    let significant = digits.trim_start_matches('0').len();
    if significant <= GRID_DIGITS + 2 {
        return None;
    }
    let rounded: f64 = format!("{x:.*e}", GRID_DIGITS - 1).parse().ok()?;
    Some(stat_data::format_number(rounded))
}

/// Clicks and drags on a data cell: a click moves the cursor (Shift-click
/// extends a block from it), double-click edits, and a drag selects a block.
/// Returns whether the cell was clicked or a drag started on it (which
/// clears the row selection).
fn cell_pointer(
    cell: &egui::Response,
    table: &DataTable,
    r: usize,
    j: usize,
    state: &mut GridState,
    shift: bool,
) -> bool {
    if cell.double_clicked() {
        state.begin_edit(table, r, j, None);
    } else if cell.clicked() {
        state.click_cell(r, j, shift);
        return true;
    } else if cell.drag_started_by(egui::PointerButton::Primary) {
        state.click_cell(r, j, shift);
        // The block runs from the cell the drag started on.
        state.anchor = state.anchor.or(Some((r, j)));
        state.drag = Some(Drag::Cells);
        return true;
    } else if matches!(state.drag, Some(Drag::Cells)) && cell.contains_pointer() && state.cursor != Some((r, j)) {
        state.cursor = Some((r, j));
    }
    false
}

/// Clicks and drags on a row number: the rows to select and how. The
/// selection is the shared row selection, so plots highlight it too.
fn row_number_pointer(
    number: &egui::Response,
    r: usize,
    state: &mut GridState,
    modifiers: egui::Modifiers,
) -> Option<(Vec<usize>, SelectionMode)> {
    if number.clicked() || number.drag_started_by(egui::PointerButton::Primary) {
        // Selecting rows ends a selection of columns.
        state.columns.clear();
    }
    if number.clicked() {
        let pick = row_number_click(state.row_anchor, r, modifiers.shift, modifiers.command, modifiers.alt);
        if !modifiers.shift || state.row_anchor.is_none() {
            state.row_anchor = Some(r);
        }
        return Some(pick);
    }
    if number.drag_started_by(egui::PointerButton::Primary) {
        let from = if modifiers.shift { state.row_anchor.unwrap_or(r) } else { r };
        state.row_anchor = Some(from);
        let add = modifiers.command;
        state.drag = Some(Drag::Rows { from, to: r, add });
        let mode = if add { SelectionMode::Extend } else { SelectionMode::Replace };
        return Some(((from.min(r)..=from.max(r)).collect(), mode));
    }
    if let Some(Drag::Rows { from, to, add }) = &mut state.drag {
        if number.contains_pointer() && *to != r {
            *to = r;
            let mode = if *add { SelectionMode::Extend } else { SelectionMode::Replace };
            return Some((((*from).min(r)..=(*from).max(r)).collect(), mode));
        }
    }
    None
}

/// Clicks, drags and the right-click menu of a column heading. Returns
/// whether the heading was clicked or a drag started on it (which clears
/// the row selection).
fn heading_pointer(
    cell: &egui::Response,
    j: usize,
    col: &stat_data::Column,
    state: &mut GridState,
    requests: &mut Vec<GridRequest>,
    modifiers: egui::Modifiers,
) -> bool {
    if state.renaming.as_ref().is_some_and(|(c, _)| *c == j) {
        return false;
    }
    let pressed = cell.clicked() || cell.drag_started_by(egui::PointerButton::Primary);
    let formula = col.formula.as_deref().map(|f| format!("\nComputed by the formula  {f}")).unwrap_or_default();
    let cell = cell.clone().on_hover_text(format!(
        "{} — {} {}{formula}\nClick to select the column (Shift-click for a run, ⌘/Ctrl-click to add) · \
         double-click to rename · right-click for column options",
        col.name,
        col.level.label(),
        kind_description(col.kind())
    ));
    if cell.double_clicked() {
        state.renaming = Some((j, col.name.clone()));
    } else if cell.clicked() {
        state.click_column(j, modifiers.shift, modifiers.command);
    } else if cell.drag_started_by(egui::PointerButton::Primary) {
        let base = if modifiers.command { state.columns.clone() } else { Vec::new() };
        let from = if modifiers.shift { state.column_anchor.unwrap_or(j) } else { j };
        state.column_anchor = Some(from);
        state.drag = Some(Drag::Columns { from, base });
        state.click_column(j, true, modifiers.command);
    } else if let Some(Drag::Columns { from, base }) = &state.drag {
        if cell.contains_pointer() {
            let mut columns = base.clone();
            columns.extend((*from).min(j)..=(*from).max(j));
            columns.sort_unstable();
            columns.dedup();
            if columns != state.columns {
                state.columns = columns;
                state.cursor = Some((state.cursor.map_or(0, |c| c.0), j));
                state.anchor = None;
            }
        }
    }
    cell.context_menu(|ui| column_menu(ui, j, col, state, requests));
    pressed
}

/// The Find bar: text, Match case, Next (or Enter), Select matching rows,
/// and × (or Escape) to close.
fn find_bar(
    ui: &mut egui::Ui,
    table: &DataTable,
    rows: &mut RowStateManager,
    state: &mut GridState,
    requests: &mut Vec<GridRequest>,
) {
    let Some(find) = &mut state.find else { return };
    let mut next = false;
    let mut close = false;
    let mut select = false;
    let (mut replace_one, mut replace_every) = (false, false);
    ui.horizontal(|ui| {
        ui.label("Find");
        let field =
            ui.add(egui::TextEdit::singleline(&mut find.query.text).hint_text("Text in any cell").desired_width(180.0));
        if std::mem::take(&mut find.focus) {
            field.request_focus();
        }
        if field.changed() {
            find.status = None;
        }
        if field.lost_focus() {
            if ui.input(|i| i.key_pressed(Key::Enter)) {
                next = true;
                field.request_focus();
            } else if ui.input(|i| i.key_pressed(Key::Escape)) {
                close = true;
            }
        }
        if ui.checkbox(&mut find.query.match_case, "Match case").changed() {
            find.status = None;
        }
        let ready = !find.query.text.is_empty();
        if ui
            .add_enabled(ready, egui::Button::new("Next"))
            .on_hover_text("Go to the next matching cell (Enter)")
            .clicked()
        {
            next = true;
        }
        let in_columns = !state.columns.is_empty();
        let hint = if in_columns {
            "Select every row with a match in the selected columns"
        } else {
            "Select every row with a match in any column"
        };
        if ui.add_enabled(ready, egui::Button::new("Select matching rows")).on_hover_text(hint).clicked() {
            select = true;
        }
        if let Some(status) = &find.status {
            ui.label(RichText::new(status).weak());
        }
        ui.toggle_value(&mut find.replacing, "Replace…").on_hover_text("Show the Replace row");
        if ui.small_button("×").on_hover_text("Close the Find bar (Escape)").clicked() {
            close = true;
        }
    });
    if find.replacing {
        ui.horizontal(|ui| {
            ui.label("Replace with");
            ui.add(egui::TextEdit::singleline(&mut find.replacement).desired_width(180.0));
            let ready = !find.query.text.is_empty();
            replace_one = ui
                .add_enabled(ready, egui::Button::new("Replace"))
                .on_hover_text("Replace in the cell at the cursor if it matches, then go to the next match")
                .clicked();
            let scope = if state.columns.is_empty() { "every column" } else { "the selected columns" };
            replace_every = ui
                .add_enabled(ready, egui::Button::new("Replace all"))
                .on_hover_text(format!("Replace in every matching cell of {scope}, as one undoable step"))
                .clicked();
        });
    }
    let report = |r: &search::Replacement| {
        let skipped = match r.skipped {
            0 => String::new(),
            n => format!("; {n} left alone (not a valid value for the column, or a formula column)"),
        };
        match r.changed {
            0 => format!("Nothing replaced{skipped}"),
            1 => format!("Replaced 1 cell{skipped}"),
            n => format!("Replaced {n} cells{skipped}"),
        }
    };
    if replace_one {
        let cell = state.cursor.filter(|&(r, c)| {
            table.column(c).is_some_and(|col| search::replace_text(&col.cell_text(r), &find.query, "").is_some())
        });
        if let Some(cell) = cell {
            let result = search::replace_in_cells(table, &find.query, &find.replacement, [cell]);
            let text = report(&result);
            find.status = Some(text.clone());
            if let Some(edit) = result.edit {
                requests.push(GridRequest::Apply { edit, message: text });
            }
        }
        next = true;
    }
    if replace_every {
        let result = search::replace_all(table, &find.query, &find.replacement, &state.columns);
        let text = report(&result);
        find.status = Some(text.clone());
        if let Some(edit) = result.edit {
            requests.push(GridRequest::Apply { edit, message: format!("{text}. Undo puts them back.") });
        }
    }
    if select {
        let matched = search::matching_rows(table, &find.query, &state.columns);
        find.status = Some(match matched.len() {
            0 => "No rows match".to_string(),
            1 => "1 row selected".to_string(),
            n => format!("{n} rows selected"),
        });
        rows.select(matched, SelectionMode::Replace);
    }
    if next {
        state.find_next(table);
    }
    if close {
        state.find = None;
    }
}

fn toolbar(
    ui: &mut egui::Ui,
    table: &DataTable,
    rows: &RowStateManager,
    state: &mut GridState,
    undo: Option<&str>,
    redo: Option<&str>,
    requests: &mut Vec<GridRequest>,
) {
    // Wrapped: a row wider than the pane would widen the whole pane's layout
    // area and push the grid (and its scroll bar) under the side-by-side divider.
    ui.horizontal_wrapped(|ui| {
        let undo_button = ui.add_enabled(undo.is_some(), egui::Button::new("Undo"));
        if undo_button.on_hover_text(undo.map_or("Nothing to undo".to_owned(), |l| format!("Undo {l}"))).clicked() {
            requests.push(GridRequest::Undo);
        }
        let redo_button = ui.add_enabled(redo.is_some(), egui::Button::new("Redo"));
        if redo_button.on_hover_text(redo.map_or("Nothing to redo".to_owned(), |l| format!("Redo {l}"))).clicked() {
            requests.push(GridRequest::Redo);
        }
        ui.separator();
        // Above and below the selected rows, else the cell cursor's row.
        let selected = rows.selected_rows();
        let reference = match (selected.first(), selected.last()) {
            (Some(&first), Some(&last)) => Some((first, last)),
            _ => state.cursor.map(|(r, _)| (r, r)).filter(|&(r, _)| r < table.n_rows()),
        };
        let mut add_rows = None;
        let row_menu = crate::menus::menu_button(ui, "+ Row", |ui| {
            let suggested = (selected.len() > 1).then_some(selected.len());
            let count = how_many(ui, ROWS_KEY, MAX_ROWS, suggested);
            let what = plural(count, "row");
            let place = match reference {
                Some((first, last)) if !selected.is_empty() && first != last => {
                    format!("Above or below the selected rows ({}–{})", first + 1, last + 1)
                }
                Some((first, _)) if !selected.is_empty() => format!("Above or below the selected row ({})", first + 1),
                Some((row, _)) => format!("Above or below row {} (the cell cursor)", row + 1),
                None => "Select rows or click a cell to insert above or below".to_owned(),
            };
            ui.label(RichText::new(place).weak().small());
            let (above, below) = reference.map_or((None, None), |(first, last)| (Some(first), Some(last + 1)));
            for (label, at) in [
                (format!("Insert {what} above"), above),
                (format!("Insert {what} below"), below),
                (format!("Add {what} at the end"), Some(table.n_rows())),
            ] {
                if ui.add_enabled(at.is_some(), egui::Button::new(label)).clicked() {
                    add_rows = at.map(|at| (at, count));
                    ui.close_menu();
                }
            }
        });
        let row_menu = row_menu.response.on_hover_text("Add empty rows above, below or at the end");
        if let Some((at, count)) = add_rows {
            requests.push(GridRequest::InsertRows { at, count });
            state.cursor = Some((at, state.cursor.map_or(0, |c| c.1)));
            state.anchor = None;
            state.scroll_to = Some(at);
        }
        // Left and right of the selected columns, else the cell cursor's column.
        let columns = match (state.columns.iter().min(), state.columns.iter().max()) {
            (Some(&first), Some(&last)) => Some((first, last)),
            _ => state.cursor.map(|(_, c)| (c, c)).filter(|&(c, _)| c < table.n_cols()),
        };
        let heading = |j: usize| table.column(j).map_or("", |c| c.name.as_str());
        let column_menu = crate::menus::menu_button(ui, "+ Column", |ui| {
            let n_selected = state.columns.len();
            let count = how_many(ui, COLUMNS_KEY, MAX_COLUMNS, (n_selected > 1).then_some(n_selected));
            let kind = column_kind_choice(ui);
            let name = column_name_field(ui);
            let what = plural(count, "column");
            let place = match columns {
                Some((first, last)) if n_selected > 1 => format!(
                    "Left or right of the selected columns ({} to {})",
                    heading(first),
                    heading(last)
                ),
                Some((first, _)) if n_selected == 1 => {
                    format!("Left or right of the selected column ({})", heading(first))
                }
                Some((column, _)) => format!("Left or right of {} (the cell cursor)", heading(column)),
                None => "Select columns or click a cell to insert left or right".to_owned(),
            };
            ui.label(RichText::new(place).weak().small());
            let (left, right) = columns.map_or((None, None), |(first, last)| (Some(first), Some(last + 1)));
            for (label, at) in [
                (format!("Insert {what} to the left"), left),
                (format!("Insert {what} to the right"), right),
                (format!("Add {what} at the end"), Some(table.n_cols())),
            ] {
                if ui.add_enabled(at.is_some(), egui::Button::new(label)).clicked() {
                    if let Some(at) = at {
                        requests.push(GridRequest::InsertColumn { at, kind, count, name: name.clone() });
                    }
                    forget_column_name(ui.ctx());
                    ui.close_menu();
                }
            }
            ui.separator();
            // A formula column is one column, named in its own dialog.
            for (label, at) in [
                ("Insert formula column to the left…", left),
                ("Insert formula column to the right…", right),
                ("Add formula column at the end…", Some(table.n_cols())),
            ] {
                let button = ui.add_enabled(at.is_some(), egui::Button::new(label));
                if button.on_hover_text("A column computed from the others").clicked() {
                    if let Some(at) = at {
                        requests.push(GridRequest::Formula { column: None, at });
                    }
                    ui.close_menu();
                }
            }
        });
        let column_menu = column_menu.response.on_hover_text("Add empty columns left, right or at the end");
        // Opening either menu ends the cell edit in progress, keeping what was typed.
        if row_menu.clicked() || column_menu.clicked() {
            if let Some(e) = &state.editing {
                let id = egui::Id::new(("cell_editor", e.row, e.column));
                if finish_edit(table, state, requests) {
                    ui.memory_mut(|m| m.surrender_focus(id));
                }
            }
        }
        let selected = rows.counts().selected;
        let delete = ui.add_enabled(selected > 0, egui::Button::new("Delete selected rows"));
        if delete.on_disabled_hover_text("Select rows first (click or drag down the row numbers)").clicked() {
            requests.push(GridRequest::RemoveRows(rows.selected_rows()));
        }
        let delete = ui.add_enabled(!state.columns.is_empty(), egui::Button::new("Delete selected columns"));
        if delete.on_disabled_hover_text("Select columns first (click or drag across the column headings)").clicked() {
            requests.push(GridRequest::RemoveColumns(state.columns.clone()));
        }
        ui.separator();
        ui.toggle_value(&mut state.text_pane, "Text pane")
            .on_hover_text("Show the cursor's cell in a larger editor, for long text such as comments or notes");
        ui.toggle_value(&mut state.heading_charts, "Column charts")
            .on_hover_text("A small chart of each column's values under its heading; click a bar to select its rows");
        if table.columns().iter().any(|c| !c.value_labels.is_empty()) {
            ui.toggle_value(&mut state.show_codes, "Stored values").on_hover_text(
                "Show the stored values of columns with value labels instead of the labels (analyses always use \
                 the labels)",
            );
        }
        ui.label(RichText::new("?").weak().strong()).on_hover_text(
            "Click a cell, then type to edit (or double-click, Enter or F2); Enter / Tab to commit.\n\
             Drag across cells, or Shift with a click or the arrow keys, for a block; ⌘A / Ctrl+A selects all.\n\
             Delete clears the block; ⌘D / Ctrl+D fills its first row down.\n\
             Click a column heading to select the column; Shift-click for a run, ⌘/Ctrl-click to add.\n\
             Click the row numbers to select rows (highlighted in every plot); Shift-click for a run.\n\
             Enter on the last row adds a row; ⌘⇧I / Ctrl+Shift+I inserts rows above the block (or columns left of selected headings).\n\
             Right-click a heading or a row number for more.",
        );
    });
}

/// Keys handled while the grid has the cell cursor and nothing else has focus.
fn keyboard(
    ui: &egui::Ui,
    table: &DataTable,
    rows: &RowStateManager,
    state: &mut GridState,
    requests: &mut Vec<GridRequest>,
) {
    if state.editing.is_some() || state.renaming.is_some() {
        return;
    }
    if ui.ctx().memory(|m| m.focused().is_some()) {
        return;
    }
    clipboard_keys(ui, table, rows, state, requests);
    if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, Key::A)) {
        state.select_all(table);
        return;
    }
    if ui.input_mut(|i| i.consume_shortcut(&INSERT_SHORTCUT)) {
        insert_at_selection(table, state, requests);
        return;
    }
    let Some((r, c)) = state.cursor else { return };
    let (moves, extend, edit, clear, fill, typed) = ui.input_mut(|i| {
        // Movement keys, with Shift taken from each key press (it may be
        // released before the frame that handles it).
        let mut moves = (0isize, 0isize);
        let mut extend = false;
        for e in &i.events {
            if let egui::Event::Key { key, pressed: true, modifiers, .. } = e {
                let far = modifiers.command;
                let step = match key {
                    Key::ArrowDown if far => (TO_EDGE, 0),
                    Key::ArrowUp if far => (-TO_EDGE, 0),
                    Key::ArrowRight if far => (0, TO_EDGE),
                    Key::ArrowLeft if far => (0, -TO_EDGE),
                    Key::ArrowDown => (1, 0),
                    Key::ArrowUp => (-1, 0),
                    Key::ArrowRight => (0, 1),
                    Key::ArrowLeft => (0, -1),
                    Key::Home if far => (-TO_EDGE, -TO_EDGE),
                    Key::End if far => (TO_EDGE, TO_EDGE),
                    Key::Home => (0, -TO_EDGE),
                    Key::End => (0, TO_EDGE),
                    Key::PageDown => (PAGE_ROWS, 0),
                    Key::PageUp => (-PAGE_ROWS, 0),
                    _ => continue,
                };
                moves = (moves.0.saturating_add(step.0), moves.1.saturating_add(step.1));
                extend |= modifiers.shift;
            }
        }
        let m = i.modifiers;
        if i.consume_key(m, Key::Tab) {
            moves.1 += if m.shift { -1 } else { 1 };
        }
        let edit = i.key_pressed(Key::Enter) || i.key_pressed(Key::F2);
        let clear = i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace);
        let fill = i.consume_key(egui::Modifiers::COMMAND, Key::D);
        let typed: String = if i.modifiers.command || i.modifiers.ctrl {
            String::new()
        } else {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Text(t) => Some(t.as_str()),
                    _ => None,
                })
                .collect()
        };
        (moves, extend, edit, clear, fill, typed)
    });
    if !typed.is_empty() {
        state.begin_edit(table, r, c, Some(typed));
    } else if edit {
        state.begin_edit(table, r, c, None);
    } else if clear {
        if let Some((rows, columns)) = state.selection(table) {
            requests.push(GridRequest::Clear { rows, columns });
        }
    } else if fill {
        match state.selection(table) {
            Some((rows, columns)) if rows.len() > 1 => requests.push(GridRequest::FillDown { rows, columns }),
            _ => state.error = Some("Select two or more rows of cells to fill down".to_owned()),
        }
    } else if extend {
        state.columns.clear();
        state.extend_block(table, moves.0, moves.1);
    } else if moves != (0, 0) {
        state.move_cursor(table, moves.0, moves.1);
    }
}

/// The insert shortcut: as many columns as are selected by their headings,
/// to their left; otherwise as many rows as the cell block spans (one for
/// just the cursor), above it.
fn insert_at_selection(table: &DataTable, state: &mut GridState, requests: &mut Vec<GridRequest>) {
    if let (Some(&at), n) = (state.columns.first(), state.columns.len()) {
        requests.push(GridRequest::InsertColumn { at, kind: ColumnKind::Numeric, count: n, name: String::new() });
        return;
    }
    let Some((r0, _, r1, _)) = state.block() else {
        state.error = Some("Click a cell, or select column headings, to insert beside".to_owned());
        return;
    };
    if r0 < table.n_rows() {
        requests.push(GridRequest::InsertRows { at: r0, count: r1 - r0 + 1 });
    }
}

/// Copy, cut and paste from the keyboard (the platform turns ⌘C / Ctrl+C,
/// ⌘X / Ctrl+X and ⌘V / Ctrl+V into these events).
fn clipboard_keys(
    ui: &egui::Ui,
    table: &DataTable,
    rows: &RowStateManager,
    state: &mut GridState,
    requests: &mut Vec<GridRequest>,
) {
    let (copy, cut, paste) = ui.input(|i| {
        let copy = i.events.iter().any(|e| matches!(e, egui::Event::Copy));
        let cut = i.events.iter().any(|e| matches!(e, egui::Event::Cut));
        let paste = i.events.iter().find_map(|e| match e {
            egui::Event::Paste(text) => Some(text.clone()),
            _ => None,
        });
        (copy, cut, paste)
    });
    if copy {
        if let Some((text, _)) = state.copy_selection(table, rows) {
            ui.ctx().copy_text(text);
        }
    } else if cut {
        if let Some((text, rows, columns)) = state.cut_selection(table) {
            ui.ctx().copy_text(text);
            requests.push(GridRequest::Clear { rows, columns });
        }
    }
    if let Some(text) = paste {
        match state.paste_target(table) {
            Some((row, column)) => requests.push(GridRequest::Paste { row, column, text }),
            None => state.error = Some("Click the cell where the pasted values should start".to_owned()),
        }
    }
}

/// Lines the text pane shows.
const PANE_ROWS: usize = 5;

/// Store the text pane's edit, if it changed and the column takes it.
fn commit_pane(table: &DataTable, state: &mut GridState, pane: PaneText, requests: &mut Vec<GridRequest>) -> bool {
    if pane.text == pane.original {
        return true;
    }
    let Some(col) = table.column(pane.column).filter(|_| pane.row < table.n_rows()) else { return true };
    if let Err(e) = col.parse_value(&pane.text) {
        state.error = Some(format!("{e}. Correct the text in the pane, or revert it."));
        state.pane_text = Some(pane);
        return false;
    }
    requests.push(GridRequest::SetCell { row: pane.row, column: pane.column, text: pane.text });
    true
}

/// The text pane above the grid: the cursor cell's whole value in a
/// multi-line editor. Edits are stored (one undoable cell edit) with Save,
/// ⌘/Ctrl+Enter, or when the cursor moves to another cell.
fn text_pane(ui: &mut egui::Ui, table: &DataTable, state: &mut GridState, requests: &mut Vec<GridRequest>) {
    let cursor = state.cursor.filter(|&(r, c)| r < table.n_rows() && c < table.n_cols());
    // Moving to another cell stores the edit of the one left.
    if let Some(pane) = state.pane_text.take() {
        if Some((pane.row, pane.column)) == cursor {
            state.pane_text = Some(pane);
        } else if !commit_pane(table, state, pane, requests) {
            // Kept with its error; the pane goes on showing that cell.
        }
    }
    let pane_id = egui::Id::new("cell_text_pane");
    let (row, column) = match (&state.pane_text, cursor) {
        (Some(p), _) => (p.row, p.column),
        (None, Some(rc)) => rc,
        (None, None) => {
            ui.label(RichText::new("Click a cell to see its whole text here.").weak());
            return;
        }
    };
    let Some(col) = table.column(column) else { return };
    // The cell's value as stored, unless the pane holds an edit or the
    // value changed under it (an undo, a paste).
    let current = col.cell_text(row);
    let editing = ui.memory(|m| m.has_focus(pane_id));
    if state.pane_text.as_ref().is_none_or(|p| p.text == p.original && p.original != current && !editing) {
        state.pane_text = Some(PaneText { row, column, text: current.clone(), original: current });
    }
    let computed = col.formula.is_some();
    let Some(pane) = state.pane_text.as_mut() else { return };
    let changed = pane.text != pane.original;
    let mut save = false;
    let mut revert = false;
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("Row {} · {}", row + 1, col.name)).strong());
        let chars = pane.text.chars().count();
        ui.label(RichText::new(format!("{chars} characters")).weak().small());
        if computed {
            ui.label(RichText::new("computed by a formula").weak().small());
        }
        if ui.add_enabled(changed, egui::Button::new("Save")).on_hover_text("Store the text (⌘/Ctrl+Enter)").clicked()
        {
            save = true;
        }
        if ui.add_enabled(changed, egui::Button::new("Revert")).clicked() {
            revert = true;
        }
    });
    let font = egui::TextStyle::Body.resolve(ui.style());
    let height = ui.fonts(|f| f.row_height(&font)) * PANE_ROWS as f32 + 8.0;
    egui::ScrollArea::vertical().id_salt("cell_text_pane_scroll").max_height(height).show(ui, |ui| {
        let edit = egui::TextEdit::multiline(&mut pane.text)
            .id(pane_id)
            .desired_rows(PANE_ROWS)
            .desired_width(f32::INFINITY)
            .interactive(!computed);
        let response = ui.add(edit);
        if response.has_focus() && ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter)) {
            save = true;
        }
    });
    if revert {
        pane.text = pane.original.clone();
        state.error = None;
    } else if save {
        if let Some(pane) = state.pane_text.take() {
            let original = pane.text.clone();
            let (row, column) = (pane.row, pane.column);
            if commit_pane(table, state, pane, requests) {
                state.error = None;
                state.pane_text = Some(PaneText { row, column, text: original.clone(), original });
            }
        }
    }
    ui.separator();
}

/// End the cell edit in progress (if any), storing what was typed. An entry
/// the column cannot take keeps the editor open with the reason; returns
/// whether the edit ended.
fn finish_edit(table: &DataTable, state: &mut GridState, requests: &mut Vec<GridRequest>) -> bool {
    let Some(editor) = state.editing.as_mut() else { return true };
    let (row, column, text) = (editor.row, editor.column, editor.text.clone());
    let Some(col) = table.column(column) else {
        state.editing = None;
        return true;
    };
    if let Err(e) = col.parse_value(&text) {
        // Keep the editor open so the entry can be corrected (or Escape to cancel).
        state.error =
            Some(format!("{e}. Correct it, press Escape to cancel, or change the column's data type to text."));
        editor.focused = false;
        return false;
    }
    if text != col.cell_text(row) {
        requests.push(GridRequest::SetCell { row, column, text });
    }
    state.editing = None;
    state.error = None;
    true
}

/// Lines the expanded cell editor shows before it scrolls.
const EXPANDED_LINES: f32 = 8.0;

/// The text field for the cell being edited, and what happens when editing ends.
///
/// Text that fits the cell is edited in place. Text wider than the cell (or
/// with line breaks) is edited in a box laid over the grid at the cell: at
/// least the cell's width, as wide as the text up to half the window, then
/// wrapped and growing downward (scrolling past [`EXPANDED_LINES`]). There
/// Enter still commits and Shift+Enter starts a new line in a text column.
fn cell_editor(ui: &mut egui::Ui, table: &DataTable, state: &mut GridState, requests: &mut Vec<GridRequest>) {
    let Some(editor) = state.editing.as_mut() else { return };
    let id = egui::Id::new(("cell_editor", editor.row, editor.column));
    let text_column = table.column(editor.column).is_some_and(|c| c.kind() == ColumnKind::Text);
    let cell = ui.max_rect();
    let font = egui::TextStyle::Body.resolve(ui.style());
    let text_width = ui.fonts(|f| f.layout_no_wrap(editor.text.clone(), font, egui::Color32::PLACEHOLDER).size().x);
    // The in-cell field's margins (2 points each side) plus the cursor.
    editor.expanded |= editor.text.contains('\n') || text_width + 6.0 > cell.width();
    let focused_now = editor.focused && ui.memory(|m| m.has_focus(id));
    // Tab is taken before the text field sees it, so it commits and moves
    // within the grid instead of moving keyboard focus to another widget.
    let tab = focused_now
        && ui.input_mut(|i| {
            let m = i.modifiers;
            i.consume_key(m, Key::Tab)
        });
    // In the expanded box Enter commits (the multi-line field would otherwise
    // take it); Shift+Enter is left to the field in a text column.
    let box_enter = editor.expanded
        && focused_now
        && ui.input_mut(|i| {
            let mut found = false;
            i.events.retain(|e| match e {
                egui::Event::Key { key: Key::Enter, pressed: true, modifiers, .. }
                    if !(text_column && modifiers.shift) =>
                {
                    found = true;
                    false
                }
                _ => true,
            });
            found
        });
    let response = if editor.expanded {
        let screen = ui.ctx().screen_rect();
        let widest = (screen.width() / 2.0).max(cell.width());
        let width = (text_width + 12.0).clamp(cell.width(), widest);
        let line = ui.text_style_height(&egui::TextStyle::Body);
        let multiline = egui::TextEdit::multiline(&mut editor.text)
            .id(id)
            .desired_width(width)
            .desired_rows(1)
            .margin(egui::Margin::symmetric(2, 1))
            .return_key(text_column.then(|| egui::KeyboardShortcut::new(egui::Modifiers::SHIFT, Key::Enter)));
        egui::Area::new(id.with("expanded"))
            .order(egui::Order::Foreground)
            .fixed_pos(cell.left_top())
            .constrain_to(screen)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).inner_margin(egui::Margin::same(2)).show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(line * EXPANDED_LINES + 4.0)
                        .auto_shrink([true, true])
                        .show(ui, |ui| ui.add(multiline))
                        .inner
                })
            })
            .inner
            .inner
    } else {
        ui.add(
            egui::TextEdit::singleline(&mut editor.text)
                .id(id)
                .desired_width(f32::INFINITY)
                .margin(egui::Margin::symmetric(2, 0)),
        )
    };
    if let Some(e) = &state.error {
        // The entry was rejected: outline the cell and explain just below it.
        ui.painter().rect_stroke(response.rect, 2.0_f32, egui::Stroke::new(1.5_f32, theme::ERROR), StrokeKind::Outside);
        egui::Area::new(id.with("error"))
            .order(egui::Order::Tooltip)
            .fixed_pos(response.rect.left_bottom() + egui::vec2(0.0, 4.0))
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_max_width(320.0);
                    ui.colored_label(theme::ERROR, format!("⚠ {e}"));
                });
            });
    }
    if !editor.focused {
        response.request_focus();
        // Put the text cursor at the end, so typing continues after what is there.
        if let Some(mut text_state) = egui::widgets::text_edit::TextEditState::load(ui.ctx(), id) {
            let end = egui::text::CCursor::new(editor.text.chars().count());
            text_state.cursor.set_char_range(Some(egui::text_selection::CCursorRange::one(end)));
            text_state.store(ui.ctx(), id);
        }
        editor.focused = true;
        return;
    }
    if !tab && !box_enter && !response.lost_focus() {
        return;
    }
    let (escape, enter, shift) =
        ui.input(|i| (i.key_pressed(Key::Escape), box_enter || i.key_pressed(Key::Enter), i.modifiers.shift));
    if escape {
        state.editing = None;
        state.error = None;
        return;
    }
    let (row, column) = (editor.row, editor.column);
    if !finish_edit(table, state, requests) {
        return;
    }
    if tab {
        state.move_cursor(table, 0, if shift { -1 } else { 1 });
    } else if enter && !shift && row + 1 == table.n_rows() {
        // Enter on the last row adds a row, so data can be typed in a column
        // without stopping.
        requests.push(GridRequest::InsertRows { at: table.n_rows(), count: 1 });
        state.cursor = Some((table.n_rows(), column));
        state.anchor = None;
        state.columns.clear();
        state.scroll_to = Some(table.n_rows());
    } else if enter {
        state.move_cursor(table, if shift { -1 } else { 1 }, 0);
    }
    // Leave nothing focused, so arrow keys and typing go to the grid again.
    ui.memory_mut(|m| m.surrender_focus(id));
}

/// A column heading: its measurement level, name, type and formula mark, or
/// the rename field while it is being renamed.
fn column_header(
    ui: &mut egui::Ui,
    j: usize,
    col: &stat_data::Column,
    state: &mut GridState,
    requests: &mut Vec<GridRequest>,
) {
    if let Some((index, name)) = state.renaming.as_mut().filter(|(c, _)| *c == j) {
        let response =
            ui.add(egui::TextEdit::singleline(name).id_salt(("rename", *index)).desired_width(f32::INFINITY));
        if !response.has_focus() && !response.lost_focus() {
            response.request_focus();
        }
        if response.lost_focus() {
            let escape = ui.input(|i| i.key_pressed(Key::Escape));
            let new_name = name.trim().to_owned();
            if !escape && new_name != col.name {
                requests.push(GridRequest::RenameColumn(j, new_name));
            }
            state.renaming = None;
        }
        return;
    }
    let about = column_about(col);
    // The level icon is a drag handle: drop the column on a graph shelf or
    // a dialog's column list (dragging the rest of the heading selects
    // columns).
    let payload = crate::report_view::ColumnDrag { name: col.name.clone(), from: None };
    ui.dnd_drag_source(ui.id().with(("heading_drag", col.name.as_str())), payload, |ui| level_badge(ui, col.level))
        .response
        .on_hover_cursor(egui::CursorIcon::Grab)
        .on_hover_text(format!("{about}\nDrag the icon onto a graph shelf or a dialog's column list"));
    // A name too long for the column loses its middle, not its end, so
    // names that differ only at the end (as Summarize by group's "… mean"
    // and "… std dev" columns do) stay distinguishable.
    let reserve = 4.0 + if col.formula.is_some() { 12.0 } else { 0.0 };
    let font = egui::TextStyle::Body.resolve(ui.style());
    let shown = elide_middle(ui, &col.name, &font, (ui.available_width() - reserve).max(30.0));
    ui.label(RichText::new(&shown).strong()).on_hover_text(&about);
    if col.formula.is_some() {
        ui.label(RichText::new("ƒ").weak().italics());
    }
}

/// Height of a heading chart, before text scaling.
const HEADING_CHART_HEIGHT: f32 = 30.0;

impl GridState {
    /// The heading chart of column `j`, computed again only when the data
    /// or the excluded rows changed.
    fn heading_summary(&mut self, table: &DataTable, rows: &RowStateManager, j: usize) -> std::rc::Rc<HeadingSummary> {
        let col = &table.columns()[j];
        let stamp = (table.revision(), rows.exclusion_revision());
        if let Some((rev, excl, summary)) = self.charts.get(&col.name)
            && (*rev, *excl) == stamp
        {
            return std::rc::Rc::clone(summary);
        }
        let summary = std::rc::Rc::new(app_model::heading_chart::summarize(col, rows));
        self.charts.insert(col.name.clone(), (stamp.0, stamp.1, std::rc::Rc::clone(&summary)));
        summary
    }
}

/// Draw a heading chart in a strip `height` tall; returns the strip and
/// each bar's rectangle (the strip itself for a line), for clicks.
fn heading_chart(
    ui: &mut egui::Ui,
    col: &stat_data::Column,
    summary: &HeadingSummary,
    height: f32,
) -> (egui::Rect, Vec<egui::Rect>) {
    use app_model::heading_chart::HeadingChart;
    let width = ui.available_width().max(24.0);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let pal = theme::plot_palette(ui);
    let area = rect.shrink2(egui::vec2(2.0, 3.0));
    let painter = ui.painter_at(rect);
    let fill = pal.point.gamma_multiply(0.75);
    let mut bars = Vec::new();
    let missing = if summary.missing > 0 { format!("\n{} missing", summary.missing) } else { String::new() };
    let mut tips: Vec<String> = Vec::new();
    let number = |v: f64| col.value_text(v);
    match &summary.chart {
        HeadingChart::Histogram { min, max, counts } => {
            let top = counts.iter().copied().max().unwrap_or(1).max(1) as f32;
            let w = area.width() / counts.len() as f32;
            for (k, &c) in counts.iter().enumerate() {
                let x0 = area.left() + w * k as f32;
                let h = area.height() * c as f32 / top;
                let bar = egui::Rect::from_min_max(egui::pos2(x0, area.top()), egui::pos2(x0 + w, area.bottom()));
                if c > 0 {
                    let drawn = egui::Rect::from_min_max(
                        egui::pos2(x0 + 0.5, area.bottom() - h.max(1.0)),
                        egui::pos2(x0 + w - 0.5, area.bottom()),
                    );
                    painter.rect_filled(drawn, 0.0, fill);
                }
                let step = (max - min) / counts.len() as f64;
                let lo = min + step * k as f64;
                tips.push(format!("{} to {}: {}", number(lo), number(lo + step), crate::dialog_kit::count(c, "row", "rows")));
                bars.push(bar);
            }
        }
        HeadingChart::Levels { levels, .. } => {
            let top = levels.iter().map(|l| l.1).max().unwrap_or(1).max(1) as f32;
            let w = area.width() / levels.len() as f32;
            for (k, (name, c)) in levels.iter().enumerate() {
                let x0 = area.left() + w * k as f32;
                let h = area.height() * *c as f32 / top;
                let bar = egui::Rect::from_min_max(egui::pos2(x0, area.top()), egui::pos2(x0 + w, area.bottom()));
                let drawn = egui::Rect::from_min_max(
                    egui::pos2(x0 + 1.0, area.bottom() - h.max(1.0)),
                    egui::pos2(x0 + w - 1.0, area.bottom()),
                );
                painter.rect_filled(drawn, 0.0, fill);
                tips.push(format!("{name}: {}", crate::dialog_kit::count(*c, "row", "rows")));
                bars.push(bar);
            }
        }
        HeadingChart::Line { points, min, max } => {
            let n = points.len().max(2) as f32 - 1.0;
            let span = if max > min { max - min } else { 1.0 };
            let line: Vec<egui::Pos2> = points
                .iter()
                .enumerate()
                .map(|(i, (_, v))| {
                    let x = area.left() + area.width() * i as f32 / n;
                    let y = area.bottom() - area.height() * ((v - min) / span) as f32;
                    egui::pos2(x, y)
                })
                .collect();
            painter.add(egui::Shape::line(line, egui::Stroke::new(1.25_f32, pal.point)));
            tips.push(format!("{} to {} in row order", number(*min), number(*max)));
            bars.push(area);
        }
        HeadingChart::Empty => {
            tips.push("No values".to_owned());
        }
    }
    painter.line_segment(
        [egui::pos2(area.left(), area.bottom()), egui::pos2(area.right(), area.bottom())],
        egui::Stroke::new(1.0_f32, pal.axis_spine),
    );
    let hovered = response.hover_pos().and_then(|p| bars.iter().position(|b| b.contains(p)));
    let tip = match hovered {
        Some(k) => format!("{}{missing}\nClick to select these rows", tips[k]),
        None => format!("{}{missing}", tips.join("\n")),
    };
    let name = format!("Chart of {}", col.name);
    response
        .on_hover_text(tip)
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, &name));
    (rect, bars)
}

/// A column's tooltip: its name, measurement level and data type ("GNP —
/// continuous, numbers"), and whether a formula computes it.
fn column_about(col: &stat_data::Column) -> String {
    let formula = if col.formula.is_some() { "; computed by a formula" } else { "" };
    format!("{} — {}, {}{formula}", col.name, col.level.label(), kind_description(col.kind()))
}

/// The width a column first gets: room for its heading (level icon and whole
/// name) and the longest of its first values, between about 64 and 180
/// points at the standard text size. Dragging a column's edge still resizes it.
fn fitted_width(ui: &egui::Ui, col: &stat_data::Column, n_rows: usize) -> f32 {
    let scale = theme::text_scale(ui);
    let measure = |text: &str, style: egui::TextStyle| {
        let font = style.resolve(ui.style());
        ui.fonts(|f| f.layout_no_wrap(text.to_owned(), font, egui::Color32::WHITE).size().x)
    };
    let heading = LEVEL_ICON_SIZE * scale
        + measure(&col.name, egui::TextStyle::Body)
        + if col.formula.is_some() { 14.0 * scale } else { 0.0 }
        + 22.0 * scale;
    let style = if col.kind() == ColumnKind::Text { egui::TextStyle::Body } else { egui::TextStyle::Monospace };
    let shown = |r: usize| match &col.data {
        stat_data::ColumnData::Numeric(v) => short_number(v[r]).unwrap_or_else(|| col.cell_text(r)),
        _ => col.cell_text(r),
    };
    let values =
        (0..n_rows.min(FIT_ROWS)).map(|r| measure(&shown(r), style.clone()) + 16.0 * scale).fold(0.0_f32, f32::max);
    heading.max(values).clamp(64.0 * scale, 180.0 * scale)
}

/// Rows whose values set a column's first width (the rest scroll into view later).
const FIT_ROWS: usize = 200;

/// `text`, shortened in the middle with "…" to fit `max_width` in `font`.
fn elide_middle(ui: &egui::Ui, text: &str, font: &egui::FontId, max_width: f32) -> String {
    let width = |t: &str| ui.fonts(|f| f.layout_no_wrap(t.to_owned(), font.clone(), egui::Color32::WHITE).size().x);
    if width(text) <= max_width {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    // Keep as many characters as fit, two parts head to tail at about 3:2.
    let candidate = |keep: usize| {
        let tail = keep * 2 / 5;
        let head = keep - tail;
        let mut s: String = chars[..head].iter().collect();
        s.push('…');
        s.extend(&chars[chars.len() - tail..]);
        s
    };
    let (mut lo, mut hi) = (0, chars.len().saturating_sub(1));
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if width(&candidate(mid)) <= max_width { lo = mid } else { hi = mid - 1 }
    }
    candidate(lo)
}

/// Side of the measurement-level icon at the standard text size.
const LEVEL_ICON_SIZE: f32 = 16.0;

/// The measurement level as a small coloured badge holding the level's icon,
/// drawn from the Lucide icon set (ISC licence, `assets/icons/LICENSE-Lucide.txt`):
/// `ruler` for continuous, `tag` for nominal and `list-ordered` for ordinal
/// columns. Shape and colour both carry the level, and the level's name is in
/// the tooltip of whatever shows it.
pub fn level_badge(ui: &mut egui::Ui, level: MeasurementLevel) -> egui::Response {
    let side = LEVEL_ICON_SIZE * theme::text_scale(ui);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        painter.rect_filled(rect, side * 0.2, theme::level_color(level));
        // Lucide draws on a 24-unit grid with 2-unit strokes; the icon fills
        // the badge less a small margin.
        let inner = rect.shrink(side * 0.14);
        let k = inner.width() / 24.0;
        let at = |x: f32, y: f32| inner.min + egui::vec2(x * k, y * k);
        let stroke = Stroke::new((2.0 * k).max(1.0), egui::Color32::WHITE);
        let line = |points: &[(f32, f32)]| {
            painter.add(egui::Shape::line(points.iter().map(|&(x, y)| at(x, y)).collect(), stroke));
        };
        match level {
            MeasurementLevel::Continuous => {
                // ruler: a bar on the diagonal with four ticks.
                painter.add(egui::Shape::closed_line(
                    [(22.0, 17.0), (17.0, 22.0), (2.0, 7.0), (7.0, 2.0)].iter().map(|&(x, y)| at(x, y)).collect(),
                    stroke,
                ));
                for (x, y) in [(8.5, 6.5), (11.5, 9.5), (14.5, 12.5), (17.5, 15.5)] {
                    line(&[(x, y), (x + 2.0, y - 2.0)]);
                }
            }
            MeasurementLevel::Nominal => {
                // tag: a label with a hole.
                painter.add(egui::Shape::closed_line(
                    [
                        (12.6, 2.6),
                        (4.0, 2.0),
                        (2.0, 4.0),
                        (2.0, 11.2),
                        (11.3, 21.3),
                        (14.7, 21.3),
                        (21.3, 14.7),
                        (21.3, 11.3),
                    ]
                    .iter()
                    .map(|&(x, y)| at(x, y))
                    .collect(),
                    stroke,
                ));
                painter.circle_filled(at(7.5, 7.5), 1.6 * k, egui::Color32::WHITE);
            }
            MeasurementLevel::Ordinal => {
                // list-ordered: three lines with the numbers 1 and 2.
                for y in [6.0, 12.0, 18.0] {
                    line(&[(10.0, y), (21.0, y)]);
                }
                line(&[(4.0, 6.0), (5.0, 6.0), (5.0, 10.0)]);
                line(&[(4.0, 10.0), (6.0, 10.0)]);
                line(&[(4.0, 14.0), (5.4, 14.2), (6.0, 15.0), (5.5, 16.2), (4.3, 17.2), (4.0, 18.0), (6.0, 18.0)]);
            }
        }
    }
    response
}

/// The right-click menu of column heading `j` (also shown for the variable
/// in the sidebar).
pub fn column_menu(
    ui: &mut egui::Ui,
    j: usize,
    col: &stat_data::Column,
    state: &mut GridState,
    requests: &mut Vec<GridRequest>,
) {
    let kind = col.kind();
    if ui.button("Rename…").clicked() {
        state.renaming = Some((j, col.name.clone()));
        ui.close_menu();
    }
    if ui.button("Sort rows ascending").on_hover_text("By this column, missing values last; undoable").clicked() {
        requests.push(GridRequest::Sort { column: j, descending: false });
        ui.close_menu();
    }
    if ui.button("Sort rows descending").on_hover_text("By this column, missing values last; undoable").clicked() {
        requests.push(GridRequest::Sort { column: j, descending: true });
        ui.close_menu();
    }
    ui.separator();
    ui.menu_button("Data type", |ui| {
        for (k, label) in [(ColumnKind::Numeric, "Numbers"), (ColumnKind::Text, "Text")] {
            if ui.radio(kind == k, label).clicked() && kind != k {
                requests.push(GridRequest::ConvertColumn(j, k));
                ui.close_menu();
            }
        }
        ui.separator();
        // Switching between date formats keeps the offset and fraction digits.
        let current = col.temporal();
        for format in TemporalFormat::ALL {
            let selected = current.is_some_and(|d| d.format == format);
            let display = current.map_or_else(|| TemporalDisplay::new(format), |d| TemporalDisplay { format, ..d });
            let radio = ui.radio(selected, format.label()).on_hover_text(format_hint(format));
            if radio.clicked() && !selected {
                requests.push(GridRequest::ConvertColumn(j, ColumnKind::DateTime(display)));
                ui.close_menu();
            }
        }
    });
    if let Some(display) = col.temporal() {
        date_display_menu(ui, j, display, requests);
    }
    ui.menu_button("Measurement level", |ui| {
        for level in [MeasurementLevel::Continuous, MeasurementLevel::Nominal, MeasurementLevel::Ordinal] {
            let allowed = kind.is_numeric() || level != MeasurementLevel::Continuous;
            let radio = ui.add_enabled(allowed, egui::RadioButton::new(col.level == level, level.label()));
            if radio.on_disabled_hover_text("Text columns cannot be continuous").clicked() && col.level != level {
                requests.push(GridRequest::SetLevel(j, level));
                ui.close_menu();
            }
        }
    });
    ui.separator();
    let edit_label = if col.formula.is_some() { "Edit formula…" } else { "Compute with a formula…" };
    let edit = ui.button(edit_label);
    let edit = if col.formula.is_some() {
        edit
    } else {
        edit.on_hover_text("The column's values are replaced by the formula's (Undo puts them back)")
    };
    if edit.clicked() {
        requests.push(GridRequest::Formula { column: Some(j), at: j });
        ui.close_menu();
    }
    if col.formula.is_some()
        && ui
            .button("Convert formula to values")
            .on_hover_text("Keep the current values as entered data, and stop recomputing them")
            .clicked()
    {
        requests.push(GridRequest::ClearFormula(j));
        ui.close_menu();
    }
    if ui
        .button("Recode values…")
        .on_hover_text("Give each distinct value a new one: merge categories, fix spelling or spacing")
        .clicked()
    {
        requests.push(GridRequest::Recode(j));
        ui.close_menu();
    }
    let labels = if col.value_labels.is_empty() { "Value labels…".to_owned() } else {
        format!("Value labels ({})…", col.value_labels.len())
    };
    if ui
        .button(labels)
        .on_hover_text("Name what stored values stand for (1 = Treatment); analyses, plots and keys show the names")
        .clicked()
    {
        requests.push(GridRequest::ValueLabels(j));
        ui.close_menu();
    }
    let mut hover = col.hover;
    if ui
        .checkbox(&mut hover, "Show on hover in plots")
        .on_hover_text("Plots list this column's value in the tooltip of each point")
        .changed()
    {
        requests.push(GridRequest::SetHover(j, hover));
        ui.close_menu();
    }
    if ui
        .button("Colour and marker rows by this column…")
        .on_hover_text("Every row takes a colour and marker from its value here; plots draw them")
        .clicked()
    {
        requests.push(GridRequest::ColorBy(Some(j)));
        ui.close_menu();
    }
    if ui.button("Insert formula column to the left…").clicked() {
        requests.push(GridRequest::Formula { column: None, at: j });
        ui.close_menu();
    }
    if ui.button("Insert formula column to the right…").clicked() {
        requests.push(GridRequest::Formula { column: None, at: j + 1 });
        ui.close_menu();
    }
    ui.separator();
    // With this heading among several selected ones, insert as many as are
    // selected, beside the whole selection.
    let block = state.columns.contains(&j) && state.columns.len() > 1;
    let (first, last) = if block { (state.columns[0], state.columns[state.columns.len() - 1]) } else { (j, j) };
    let count = how_many(ui, COLUMNS_KEY, MAX_COLUMNS, block.then_some(state.columns.len()));
    let kind = column_kind_choice(ui);
    let name = column_name_field(ui);
    let what = plural(count, "column");
    let shortcut = format!("Also {}, with the headings selected", ui.ctx().format_shortcut(&INSERT_SHORTCUT));
    let mut at = None;
    if ui.button(format!("Insert {what} to the left")).on_hover_text(shortcut).clicked() {
        at = Some(first);
    }
    if ui.button(format!("Insert {what} to the right")).clicked() {
        at = Some(last + 1);
    }
    if let Some(at) = at {
        requests.push(GridRequest::InsertColumn { at, kind, count, name });
        forget_column_name(ui.ctx());
        ui.close_menu();
    }
    ui.separator();
    if block {
        if ui.button(format!("Delete {} selected columns", state.columns.len())).clicked() {
            requests.push(GridRequest::RemoveColumns(state.columns.clone()));
            ui.close_menu();
        }
    } else if ui.button("Delete column").clicked() {
        requests.push(GridRequest::RemoveColumn(j));
        ui.close_menu();
    }
}

/// A change to row states chosen from a row's context menu, applied after
/// the grid is drawn.
enum RowOp {
    /// Make these rows the whole selection.
    SelectOnly(Vec<usize>),
    /// Deselect these rows.
    Deselect(Vec<usize>),
    /// Exclude (`true`) or include (`false`) these rows in analyses.
    Exclude(Vec<usize>, bool),
    /// Hide (`true`) or show (`false`) these rows in plots.
    Hide(Vec<usize>, bool),
}

impl RowOp {
    fn apply(self, rows: &mut RowStateManager) {
        match self {
            Self::SelectOnly(r) => {
                rows.select(r, SelectionMode::Replace);
            }
            Self::Deselect(r) => {
                rows.select(r, SelectionMode::Subtract);
            }
            Self::Exclude(r, on) => {
                rows.set_excluded(r, on);
            }
            Self::Hide(r, on) => {
                rows.set_hidden(r, on);
            }
        }
    }
}

/// Session memory of the "How many" field of the row insert actions.
pub const ROWS_KEY: &str = "insert_rows_count";
/// Session memory of the "How many" field of the column insert actions.
pub const COLUMNS_KEY: &str = "insert_columns_count";
/// Most rows one insert adds.
pub const MAX_ROWS: usize = 100_000;
/// Most columns one insert adds.
pub const MAX_COLUMNS: usize = 1_000;
/// Insert rows above the cell cursor (as many as the block has), or columns
/// to the left of the selected column headings.
pub const INSERT_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::COMMAND.plus(egui::Modifiers::SHIFT), Key::I);
/// The kinds of empty column the add menus offer, in their words.
pub fn column_kinds() -> [(&'static str, ColumnKind); 3] {
    [
        ("Numbers", ColumnKind::Numeric),
        ("Text", ColumnKind::Text),
        ("Dates", ColumnKind::date_time(TemporalFormat::Date)),
    ]
}

/// A "How many" field for a menu's add or insert actions, starting at one and
/// remembered for the session under `key` (so the next menu offers the same
/// number). `suggested` (the size of the selection the action applies to)
/// replaces the number whenever it changes, so selecting four rows offers
/// four. Returns the number to add.
pub fn how_many(ui: &mut egui::Ui, key: &str, max: usize, suggested: Option<usize>) -> usize {
    let id = egui::Id::new(key);
    let mut count = ui.ctx().data_mut(|d| {
        let last = d.get_temp::<Option<usize>>(id.with("suggested")).flatten();
        if suggested.is_some() && suggested != last {
            d.insert_temp(id, suggested.unwrap_or(1));
        }
        d.insert_temp(id.with("suggested"), suggested);
        *d.get_temp_mut_or(id, 1_usize)
    });
    ui.horizontal(|ui| {
        let label = ui.label("How many");
        ui.add(egui::DragValue::new(&mut count).range(1..=max)).labelled_by(label.id);
    });
    let count = count.clamp(1, max);
    ui.ctx().data_mut(|d| d.insert_temp(id, count));
    count
}

/// Where the column name typed in an add menu is kept until it is used.
const NAME_KEY: &str = "insert_columns_name";

/// A "Name" field for new columns: empty gives "Column 1", "Column 2", …; a
/// name is used as is for one column and numbered for several ("Rep 1",
/// "Rep 2", …). Returns the text typed.
pub fn column_name_field(ui: &mut egui::Ui) -> String {
    let id = egui::Id::new(NAME_KEY);
    let mut name = ui.ctx().data_mut(|d| d.get_temp::<String>(id)).unwrap_or_default();
    ui.horizontal(|ui| {
        let label = ui.label("Name");
        ui.add(egui::TextEdit::singleline(&mut name).hint_text("Column 1, 2, …").desired_width(120.0))
            .labelled_by(label.id)
            .on_hover_text("Several columns are numbered from the name: Rep gives Rep 1, Rep 2, …");
    });
    ui.ctx().data_mut(|d| d.insert_temp(id, name.clone()));
    name
}

/// Clear the add menus' "Name" field once columns were added with it.
pub fn forget_column_name(ctx: &egui::Context) {
    ctx.data_mut(|d| d.remove::<String>(egui::Id::new(NAME_KEY)));
}

/// The kind of column the column heading menu inserts, remembered for the session.
fn column_kind_choice(ui: &mut egui::Ui) -> ColumnKind {
    let id = egui::Id::new("insert_columns_kind");
    let mut chosen = ui.ctx().data_mut(|d| *d.get_temp_mut_or(id, 0_usize));
    ui.horizontal(|ui| {
        ui.label("Type");
        for (i, (label, _)) in column_kinds().iter().enumerate() {
            ui.selectable_value(&mut chosen, i, *label);
        }
    });
    ui.ctx().data_mut(|d| d.insert_temp(id, chosen));
    column_kinds()[chosen.min(2)].1
}

/// "row" or "3 rows": the object of an insert action's label.
pub fn plural(count: usize, what: &str) -> String {
    if count == 1 { what.to_owned() } else { format!("{count} {what}s") }
}

/// Right-click menu of a row number. When the clicked row is selected, the
/// row-state and delete actions apply to every selected row; otherwise to the
/// clicked row only.
fn row_menu(
    ui: &mut egui::Ui,
    r: usize,
    rows: &RowStateManager,
    selected: &[usize],
    requests: &mut Vec<GridRequest>,
    ops: &mut Vec<RowOp>,
) {
    let target: Vec<usize> = if selected.contains(&r) { selected.to_vec() } else { vec![r] };
    let what =
        if target.len() == 1 { format!("row {}", target[0] + 1) } else { format!("{} selected rows", target.len()) };
    ui.label(RichText::new(format!("Applies to {what}")).weak().small());
    ui.separator();

    let is_selected = selected.contains(&r);
    if (!is_selected || selected.len() > 1) && ui.button(format!("Select only row {}", r + 1)).clicked() {
        ops.push(RowOp::SelectOnly(vec![r]));
        ui.close_menu();
    }
    if is_selected && ui.button(format!("Deselect {what}")).clicked() {
        ops.push(RowOp::Deselect(target.clone()));
        ui.close_menu();
    }
    ui.separator();

    let any_included = target.iter().any(|&t| !rows.is_excluded_flag(t));
    let any_excluded = target.iter().any(|&t| rows.is_excluded_flag(t));
    if ui.add_enabled(any_included, egui::Button::new(format!("Exclude {what} from analyses"))).clicked() {
        ops.push(RowOp::Exclude(target.clone(), true));
        ui.close_menu();
    }
    if ui.add_enabled(any_excluded, egui::Button::new(format!("Include {what} in analyses"))).clicked() {
        ops.push(RowOp::Exclude(target.clone(), false));
        ui.close_menu();
    }
    let any_shown = target.iter().any(|&t| !rows.is_hidden_flag(t));
    let any_hidden = target.iter().any(|&t| rows.is_hidden_flag(t));
    if ui.add_enabled(any_shown, egui::Button::new(format!("Hide {what} in plots"))).clicked() {
        ops.push(RowOp::Hide(target.clone(), true));
        ui.close_menu();
    }
    if ui.add_enabled(any_hidden, egui::Button::new(format!("Show {what} in plots"))).clicked() {
        ops.push(RowOp::Hide(target.clone(), false));
        ui.close_menu();
    }
    ui.separator();

    // On one of several selected rows: as many as are selected, above the
    // first or below the last.
    let block = target.len() > 1;
    let (first, last) = (target[0], target[target.len() - 1]);
    let count = how_many(ui, ROWS_KEY, MAX_ROWS, block.then_some(target.len()));
    let rows_label = plural(count, "row");
    let shortcut = format!("Also {}, from the cell cursor or block", ui.ctx().format_shortcut(&INSERT_SHORTCUT));
    if ui.button(format!("Insert {rows_label} above")).on_hover_text(shortcut).clicked() {
        requests.push(GridRequest::InsertRows { at: first, count });
        ui.close_menu();
    }
    if ui.button(format!("Insert {rows_label} below")).clicked() {
        requests.push(GridRequest::InsertRows { at: last + 1, count });
        ui.close_menu();
    }
    ui.separator();
    if ui.button(format!("Delete {what}")).clicked() {
        requests.push(GridRequest::RemoveRows(target));
        ui.close_menu();
    }
}

fn state_marks(state: RowState, outside_subset: bool) -> String {
    let mut s = String::new();
    if outside_subset {
        s.push('⊘');
    }
    if state.excluded {
        s.push('×');
    }
    if state.hidden {
        s.push('◌');
    }
    s
}

/// What a column of `kind` holds, for tooltips.
pub fn kind_description(kind: ColumnKind) -> &'static str {
    match kind {
        ColumnKind::Numeric => "numbers",
        ColumnKind::Text => "text",
        ColumnKind::DateTime(d) => match d.format {
            TemporalFormat::Date => "dates",
            TemporalFormat::DateTime => "dates and times",
            TemporalFormat::TimeOfDay => "times of day",
            TemporalFormat::Duration => "durations",
        },
    }
}

fn format_hint(format: TemporalFormat) -> &'static str {
    match format {
        TemporalFormat::Date => "Shown as 2026-10-13. Text such as 10/13/2026 or 13 Oct 2026 is read as a date.",
        TemporalFormat::DateTime => "Shown as 2026-10-13 15:57:58.",
        TemporalFormat::TimeOfDay => "Shown as 15:57:58 (seconds after midnight).",
        TemporalFormat::Duration => "Shown as 1:30:00, or 2 03:15:00 for two days and more.",
    }
}

/// Offsets from UTC offered in the menu, in minutes: whole hours from −12 to
/// +14 and the common half- and three-quarter-hour offsets.
const OFFSETS: [i16; 31] = [
    -720, -660, -600, -540, -480, -420, -360, -300, -240, -210, -180, -120, -60, 0, 60, 120, 180, 210, 240, 270, 300,
    330, 345, 360, 420, 480, 540, 570, 600, 660, 720,
];

fn offset_label(minutes: i16) -> String {
    if minutes == 0 {
        return "UTC".to_owned();
    }
    let sign = if minutes < 0 { '−' } else { '+' };
    let a = minutes.unsigned_abs();
    format!("UTC {sign}{:02}:{:02}", a / 60, a % 60)
}

/// Display options of a date column: fraction digits, the offset from UTC, and
/// swapping day and month.
fn date_display_menu(ui: &mut egui::Ui, j: usize, display: TemporalDisplay, requests: &mut Vec<GridRequest>) {
    ui.menu_button("Date display", |ui| {
        let mut set = |ui: &mut egui::Ui, d: TemporalDisplay| {
            requests.push(GridRequest::ConvertColumn(j, ColumnKind::DateTime(d)));
            ui.close_menu();
        };
        if display.format != TemporalFormat::Date {
            ui.menu_button("Decimal places of seconds", |ui| {
                for digits in [0, 1, 2, 3, 6] {
                    if ui.radio(display.fraction_digits == digits, digits.to_string()).clicked() {
                        set(ui, TemporalDisplay { fraction_digits: digits, ..display });
                    }
                }
            });
        }
        if matches!(display.format, TemporalFormat::Date | TemporalFormat::DateTime) {
            ui.menu_button(format!("Show at {}", offset_label(display.offset_minutes)), |ui| {
                egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                    for minutes in OFFSETS {
                        if ui.radio(display.offset_minutes == minutes, offset_label(minutes)).clicked() {
                            set(ui, TemporalDisplay { offset_minutes: minutes, ..display });
                        }
                    }
                });
            })
            .response
            .on_hover_text("Values are stored in UTC; the offset changes only how they are shown and read.");
            ui.separator();
            if ui
                .button("Swap day and month")
                .on_hover_text("For dates written day first (03/04/2026 = 3 April) that were read month first.")
                .clicked()
            {
                requests.push(GridRequest::SwapDayMonth(j));
                ui.close_menu();
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::short_number;

    #[test]
    fn long_numbers_are_shortened_for_the_grid() {
        assert_eq!(short_number(75.433_333_333_333_34).as_deref(), Some("75.433333"));
        assert_eq!(short_number(2.0 / 3.0).as_deref(), Some("0.66666667"));
        assert_eq!(short_number(1.0e-7 / 3.0).as_deref(), Some("3.3333333e-8"));
        // Entered values keep every digit.
        assert_eq!(short_number(12.5), None);
        assert_eq!(short_number(1_234.567_891), None);
        assert_eq!(short_number(f64::NAN), None);
        // Whole numbers such as identifiers are never rounded.
        assert_eq!(short_number(400_638_133_393.0), None);
        assert_eq!(short_number(-1_697_040_123_456.0), None);
    }
}
