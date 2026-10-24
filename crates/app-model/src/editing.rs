//! Data editing with undo and redo.
//!
//! Every change to a dataset's values or structure is an [`Edit`]. Applying an
//! edit returns its inverse — the edit that puts things back — so the
//! [`EditHistory`] keeps a stack of inverses: undoing applies the top inverse
//! and pushes *its* inverse onto the redo stack, and vice versa.
//!
//! Row edits keep the [`RowStateManager`] aligned with the table: removed rows
//! take their selection, exclusion, hiding and color with them, and restoring
//! them brings those back.
//!
//! Inserting or removing a column moves the columns after it. Analyses refer to
//! columns by position, so an applied edit reports the move as a
//! [`ColumnShift`], and [`ColumnShift::map`] translates an old position to the
//! new one (a [`Edit::Batch`] reports one shift per column it inserts or
//! removes, in order). Callers should not remove a column an analysis still
//! uses ([`Edit::removed_columns`] tells which columns an edit would remove).

use std::collections::BTreeMap;

use stat_core::{Result, StatError};
use stat_data::{CellValue, Column, ColumnData, ColumnKind, DataTable, MeasurementLevel, RowState, RowStateManager};

use crate::formula::{self, Changed, RowChanges};

/// Most edits kept for undo.
pub const HISTORY_LIMIT: usize = 500;

/// Most memory, in bytes, the undo history of one table may hold (roughly:
/// see [`Edit::approx_bytes`]). The oldest edits are forgotten beyond it,
/// but the most recent one is always kept, however large.
pub const HISTORY_BYTES: usize = 512 * 1024 * 1024;

/// One reversible change to a dataset.
#[derive(Debug, Clone, PartialEq)]
pub enum Edit {
    /// Store a value in one cell.
    SetCell {
        /// Row index.
        row: usize,
        /// Column index.
        column: usize,
        /// New value (its type must match the column).
        value: CellValue,
    },
    /// Insert rows: in strictly ascending order of index, each row ends up at
    /// its index, with the given values (one per column) and row state.
    InsertRows {
        /// `(final index, values, state)` per row.
        rows: Vec<(usize, Vec<CellValue>, RowState)>,
    },
    /// Remove rows (strictly ascending indices).
    RemoveRows {
        /// Rows to remove.
        rows: Vec<usize>,
    },
    /// Insert a column at `index`.
    InsertColumn {
        /// Position of the new column.
        index: usize,
        /// The column.
        column: Column,
    },
    /// Remove column `index`.
    RemoveColumn {
        /// Column index.
        index: usize,
    },
    /// Rename column `index`.
    RenameColumn {
        /// Column index.
        index: usize,
        /// New name.
        name: String,
    },
    /// Replace column `index` wholesale (used for type conversion).
    ReplaceColumn {
        /// Column index.
        index: usize,
        /// Replacement.
        column: Column,
    },
    /// Put the rows in a new order (sorting): row `i` becomes the row that
    /// was at `order[i]`; row states move with their rows.
    ReorderRows {
        /// A permutation of the row indices.
        order: Vec<usize>,
    },
    /// Give column `index` a formula (its values are then computed), or
    /// remove its formula and keep the values it has (`None`).
    SetFormula {
        /// Column index.
        index: usize,
        /// The formula, or `None` to keep the values as entered data.
        formula: Option<String>,
    },
    /// Change a column's measurement level.
    SetLevel {
        /// Column index.
        index: usize,
        /// New level.
        level: MeasurementLevel,
    },
    /// Give the table another name (not blank; surrounding spaces dropped).
    RenameTable {
        /// New name.
        name: String,
    },
    /// Replace the value labels of column `index` (stored value's cell text
    /// → label; empty to remove them).
    SetValueLabels {
        /// Column index.
        index: usize,
        /// The labels.
        labels: BTreeMap<String, String>,
    },
    /// Show (or stop showing) column `index` in plot tooltips.
    SetHover {
        /// Column index.
        index: usize,
        /// Whether plots list the column on hover.
        hover: bool,
    },
    /// Several edits applied in order as one step (a paste), undone together.
    /// Each edit's indices refer to the table as the edits before it left it.
    Batch {
        /// Description for menus ("Undo …").
        label: String,
        /// The edits, in order.
        edits: Vec<Edit>,
    },
}

/// How column positions moved as a result of an edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnShift {
    /// A column was inserted at this index; later columns moved right.
    Inserted(usize),
    /// The column at this index was removed; later columns moved left.
    Removed(usize),
}

impl ColumnShift {
    /// New position of the column that was at `index`; `None` if that column
    /// was the one removed.
    #[must_use]
    pub fn map(self, index: usize) -> Option<usize> {
        match self {
            Self::Inserted(at) => Some(if index >= at { index + 1 } else { index }),
            Self::Removed(at) => match index.cmp(&at) {
                std::cmp::Ordering::Less => Some(index),
                std::cmp::Ordering::Equal => None,
                std::cmp::Ordering::Greater => Some(index - 1),
            },
        }
    }
}

/// The result of applying an edit.
#[derive(Debug, Clone, PartialEq)]
pub struct Applied {
    /// The edit that undoes this one.
    pub inverse: Edit,
    /// Column movements, in the order they happened (empty if none).
    pub shift: Vec<ColumnShift>,
}

impl Edit {
    /// Set a cell from typed text, interpreted for the column's type. In a
    /// column with value labels, typing a label stores the value it names.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] for an unknown column or text that is not
    /// a number in a numeric column.
    pub fn set_cell_text(table: &DataTable, row: usize, column: usize, text: &str) -> Result<Self> {
        let col = table.column(column).ok_or_else(|| invalid(format!("no column {column}")))?;
        // A value label typed in place of its value stores the value.
        let text = if col.value_labels.contains_key(text) { text } else { col.code_of(text) };
        Ok(Self::SetCell { row, column, value: col.parse_value(text)? })
    }

    /// Remove `columns` (any order; duplicates ignored): one
    /// [`Edit::RemoveColumn`] for one column, otherwise an [`Edit::Batch`]
    /// removing them from the right, so they undo as one step.
    #[must_use]
    pub fn remove_columns(columns: &[usize]) -> Self {
        let mut columns = columns.to_vec();
        columns.sort_unstable();
        columns.dedup();
        if let [index] = columns[..] {
            return Self::RemoveColumn { index };
        }
        let label = format!("delete {} columns", columns.len());
        Self::Batch { label, edits: columns.into_iter().rev().map(|index| Self::RemoveColumn { index }).collect() }
    }

    /// `count` empty rows inserted before row `at` (`at = n_rows` appends).
    #[must_use]
    pub fn insert_empty_rows(table: &DataTable, at: usize, count: usize) -> Self {
        let empty = table.empty_row();
        Self::InsertRows { rows: (at..at + count).map(|i| (i, empty.clone(), RowState::default())).collect() }
    }

    /// An empty column of `kind` with an unused name, inserted at `index`.
    #[must_use]
    pub fn new_column(table: &DataTable, index: usize, kind: ColumnKind) -> Self {
        Self::InsertColumn { index, column: Column::empty(table.unused_column_name("Column"), kind, table.n_rows()) }
    }

    /// `count` empty columns of `kind`, inserted side by side starting at
    /// `index`. With `stem` empty they are named like [`Self::new_column`]
    /// ("Column 1", "Column 2", …); otherwise one column takes `stem` itself
    /// and several are numbered from it ("Rep 1", "Rep 2", …). Names already
    /// in use are skipped. Several columns are one [`Edit::Batch`], so they
    /// undo as one step; one column is a plain [`Edit::InsertColumn`].
    #[must_use]
    pub fn new_columns(table: &DataTable, index: usize, kind: ColumnKind, count: usize, stem: &str) -> Self {
        let stem = stem.trim();
        let n = table.n_rows();
        let free = |c: &String| table.find(c).is_none();
        if count <= 1 {
            let name = match stem {
                "" => table.unused_column_name("Column"),
                s if table.find(s).is_none() => s.to_owned(),
                s => (2..).map(|i| format!("{s} {i}")).find(free).unwrap_or_default(),
            };
            return Self::InsertColumn { index, column: Column::empty(name, kind, n) };
        }
        let base = if stem.is_empty() { "Column" } else { stem };
        let mut names = (1..).map(|i| format!("{base} {i}")).filter(free);
        let edits = (0..count)
            .map(|k| Self::InsertColumn {
                index: index + k,
                column: Column::empty(names.next().unwrap_or_default(), kind, n),
            })
            .collect();
        Self::Batch { label: format!("add {count} columns"), edits }
    }

    /// The edit giving column `target` formula `formula`, or (`target` =
    /// `None`) inserting a new formula column `name` at `at`. `kind` is how
    /// the values are stored; `None` stores numbers as numbers and text as
    /// text (an existing column keeps its type unless the values are text).
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] for an unknown column.
    pub fn formula_column(
        table: &DataTable,
        target: Option<usize>,
        name: &str,
        at: usize,
        formula: &str,
        kind: Option<ColumnKind>,
    ) -> Result<Self> {
        let n = table.n_rows();
        let formula = Some(formula.to_owned());
        let Some(j) = target else {
            let column = Column { formula, ..Column::empty(name, kind.unwrap_or(ColumnKind::Numeric), n) };
            return Ok(Self::InsertColumn { index: at, column });
        };
        let col = table.column(j).ok_or_else(|| invalid(format!("no column {j}")))?;
        match kind {
            Some(kind) if kind != col.kind() => {
                let level = if kind == ColumnKind::Text && col.level == MeasurementLevel::Continuous {
                    MeasurementLevel::Nominal
                } else {
                    col.level
                };
                let column = Column { level, formula, ..Column::empty(col.name.clone(), kind, n) };
                Ok(Self::ReplaceColumn { index: j, column })
            }
            _ => Ok(Self::SetFormula { index: j, formula }),
        }
    }

    /// Convert column `index` to `kind`; also returns how many values could not
    /// be converted and will become missing.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] for an unknown column.
    pub fn convert_column(table: &DataTable, index: usize, kind: ColumnKind) -> Result<(Self, usize)> {
        let col = table.column(index).ok_or_else(|| invalid(format!("no column {index}")))?;
        let (column, lost) = col.converted(kind);
        Ok((Self::ReplaceColumn { index, column }, lost))
    }

    /// Exchange month and day in every value of date-time column `index` (to
    /// correct slash dates read month first that were written day first);
    /// also returns how many values have a day above 12 and will become
    /// missing.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] for an unknown column or one that does
    /// not hold dates.
    pub fn swap_day_month(table: &DataTable, index: usize) -> Result<(Self, usize)> {
        let col = table.column(index).ok_or_else(|| invalid(format!("no column {index}")))?;
        let (column, lost) =
            col.day_month_swapped().ok_or_else(|| invalid(format!("column {:?} does not hold dates", col.name)))?;
        Ok((Self::ReplaceColumn { index, column }, lost))
    }

    /// Clear the cells at every `(row, column)` of `rows` × `columns` (Delete
    /// in the grid, and Cut) as one undoable step. Formula columns, cells
    /// already missing and indices out of range are skipped; `None` when
    /// nothing would change.
    #[must_use]
    pub fn clear_cells(table: &DataTable, rows: &[usize], columns: &[usize]) -> Option<Self> {
        let mut edits = Vec::new();
        for &j in columns {
            let Some(col) = table.column(j).filter(|c| c.formula.is_none()) else { continue };
            for &r in rows.iter().filter(|&&r| r < table.n_rows()) {
                if !col.cell_text(r).is_empty() {
                    edits.push(Self::SetCell { row: r, column: j, value: col.missing() });
                }
            }
        }
        let n = edits.len();
        (n > 0).then(|| Self::Batch { label: format!("clear {n} cell{}", if n == 1 { "" } else { "s" }), edits })
    }

    /// Copy the first of `rows` into the others, in each of `columns` (Fill
    /// down in the grid), as one undoable step. `rows` are taken in the order
    /// given; formula columns, cells that already hold the value and indices
    /// out of range are skipped; `None` when nothing would change.
    #[must_use]
    pub fn fill_down(table: &DataTable, rows: &[usize], columns: &[usize]) -> Option<Self> {
        let (&first, rest) = rows.split_first()?;
        if first >= table.n_rows() {
            return None;
        }
        let mut edits = Vec::new();
        for &j in columns {
            let Some(col) = table.column(j).filter(|c| c.formula.is_none()) else { continue };
            let text = col.cell_text(first);
            for &r in rest.iter().filter(|&&r| r < table.n_rows()) {
                if col.cell_text(r) != text {
                    edits.push(Self::SetCell { row: r, column: j, value: col.value(first) });
                }
            }
        }
        let n = edits.len();
        (n > 0).then(|| Self::Batch { label: format!("fill down {n} cell{}", if n == 1 { "" } else { "s" }), edits })
    }

    /// Roughly how much memory the edit holds, in bytes: the values it
    /// carries (a whole column for a column replaced or removed, every
    /// value of the rows for rows removed) plus a little for the rest.
    #[must_use]
    pub fn approx_bytes(&self) -> usize {
        let base = std::mem::size_of::<Self>();
        let cell = |v: &CellValue| match v {
            CellValue::Number(_) => 8,
            CellValue::Text(t) => 24 + t.len(),
        };
        let column_bytes = |c: &Column| {
            let data = match &c.data {
                ColumnData::Numeric(v) | ColumnData::DateTime { values: v, .. } => 8 * v.len(),
                ColumnData::Text(v) => v.iter().map(|t| 24 + t.len()).sum(),
            };
            let labels: usize = c.value_labels.iter().map(|(k, l)| 48 + k.len() + l.len()).sum();
            data + labels + c.name.len() + c.formula.as_ref().map_or(0, String::len)
        };
        base + match self {
            Self::SetCell { value, .. } => cell(value),
            Self::InsertRows { rows } => {
                rows.iter().map(|(_, values, _)| 64 + values.iter().map(cell).sum::<usize>()).sum()
            }
            Self::RemoveRows { rows } => 8 * rows.len(),
            Self::InsertColumn { column, .. } | Self::ReplaceColumn { column, .. } => column_bytes(column),
            Self::ReorderRows { order } => 8 * order.len(),
            Self::RenameColumn { name, .. } | Self::RenameTable { name } => name.len(),
            Self::SetFormula { formula, .. } => formula.as_ref().map_or(0, String::len),
            Self::SetValueLabels { labels, .. } => labels.iter().map(|(k, l)| 48 + k.len() + l.len()).sum(),
            Self::Batch { label, edits } => label.len() + edits.iter().map(Self::approx_bytes).sum::<usize>(),
            Self::RemoveColumn { .. } | Self::SetLevel { .. } | Self::SetHover { .. } => 0,
        }
    }

    /// A short description for menus ("Undo …").
    #[must_use]
    pub fn label(&self) -> String {
        let plural = |n: usize, what: &str| if n == 1 { format!("1 {what}") } else { format!("{n} {what}s") };
        match self {
            Self::SetCell { .. } => "cell edit".to_owned(),
            Self::InsertRows { rows } => format!("add {}", plural(rows.len(), "row")),
            Self::RemoveRows { rows } => format!("delete {}", plural(rows.len(), "row")),
            Self::InsertColumn { column, .. } => format!("add column {:?}", column.name),
            Self::RemoveColumn { .. } => "delete column".to_owned(),
            Self::RenameColumn { name, .. } => format!("rename column to {name:?}"),
            Self::RenameTable { name } => format!("rename table to {name:?}"),
            Self::ReplaceColumn { column, .. } => format!("change values of {:?}", column.name),
            Self::SetLevel { level, .. } => format!("set level to {}", level.label()),
            Self::SetValueLabels { labels, .. } if labels.is_empty() => "remove value labels".to_owned(),
            Self::SetValueLabels { .. } => "set value labels".to_owned(),
            Self::SetHover { hover: true, .. } => "show column on hover".to_owned(),
            Self::SetHover { hover: false, .. } => "stop showing column on hover".to_owned(),
            Self::SetFormula { formula: Some(_), .. } => "set formula".to_owned(),
            Self::SetFormula { formula: None, .. } => "convert formula to values".to_owned(),
            Self::ReorderRows { .. } => "sort rows".to_owned(),
            Self::Batch { label, .. } => label.clone(),
        }
    }

    /// The existing columns this edit would remove, as indices into the
    /// table before the edit (which has `n_cols` columns), ascending.
    #[must_use]
    pub fn removed_columns(&self, n_cols: usize) -> Vec<usize> {
        // Current position → original index (`None` for a column the edit added).
        let mut original: Vec<Option<usize>> = (0..n_cols).map(Some).collect();
        let mut removed = Vec::new();
        self.track_columns(&mut original, &mut removed);
        removed.sort_unstable();
        removed
    }

    fn track_columns(&self, original: &mut Vec<Option<usize>>, removed: &mut Vec<usize>) {
        match self {
            Self::RemoveColumn { index } if *index < original.len() => removed.extend(original.remove(*index)),
            Self::InsertColumn { index, .. } if *index <= original.len() => original.insert(*index, None),
            Self::Batch { edits, .. } => edits.iter().for_each(|e| e.track_columns(original, removed)),
            _ => {}
        }
    }

    /// Apply the edit to `table` and `rows` (which must describe the same rows),
    /// then recompute the formula columns it affects.
    ///
    /// Formula columns are guarded: their cells cannot be set, a column a
    /// formula uses cannot be removed, a formula must compile and not make the
    /// formulas form a cycle, and renaming a column rewrites the formulas
    /// that use it.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] (or a dimension error) if the edit does not
    /// fit the table; nothing is changed in that case.
    pub fn apply(self, table: &mut DataTable, rows: &mut RowStateManager) -> Result<Applied> {
        let label = self.label();
        let before = formula_types(table);
        let n_rows = table.n_rows();
        let (mut applied, changed) = self.apply_data(table, rows)?;
        formula::refresh(table, &changed);
        // A formula column stays text once a result made it text (see
        // `formula::fill`), so undoing the edit alone would not bring its
        // type back: undo also restores the type and level it had.
        let after = formula_types(table);
        let restore: Vec<Edit> = before
            .into_iter()
            .filter(|b| after.iter().any(|a| a.name == b.name && (a.kind != b.kind || a.level != b.level)))
            .map(|b| b.restoring_edit(n_rows))
            .collect();
        if !restore.is_empty() {
            let edits = std::iter::once(applied.inverse).chain(restore).collect();
            applied.inverse = Self::Batch { label, edits };
        }
        Ok(applied)
    }

    /// Apply the edit without recomputing formulas; also says what changed.
    fn apply_data(self, table: &mut DataTable, rows: &mut RowStateManager) -> Result<(Applied, Changed)> {
        debug_assert_eq!(table.n_rows(), rows.len(), "table and row states disagree");
        let name_of = |table: &DataTable, j: usize| table.column(j).map(|c| c.name.clone()).unwrap_or_default();
        let mut changed = Changed::Nothing;
        let (inverse, shift) = match self {
            Self::SetCell { row, column, value } => {
                if let Some(c) = table.column(column).filter(|c| c.formula.is_some()) {
                    return Err(invalid(format!(
                        "column {:?} is computed by a formula; change the formula, or convert it to values first",
                        c.name
                    )));
                }
                let old = table.set_value(row, column, value)?;
                changed = Changed::Partial(RowChanges::cell(name_of(table, column), row));
                (Self::SetCell { row, column, value: old }, Vec::new())
            }
            Self::InsertRows { rows: new } => {
                let positions: Vec<usize> = new.iter().map(|(i, _, _)| *i).collect();
                let states: Vec<(usize, RowState)> = new.iter().map(|(i, _, s)| (*i, *s)).collect();
                table.insert_rows(new.into_iter().map(|(i, v, _)| (i, v)).collect())?;
                rows.insert_rows(&states);
                changed = Changed::Partial(RowChanges::inserted(positions.clone()));
                (Self::RemoveRows { rows: positions }, Vec::new())
            }
            Self::RemoveRows { rows: which } => {
                let values = table.remove_rows(&which)?;
                let states = rows.remove_rows(&which);
                let restored = which.into_iter().zip(values).zip(states).map(|((i, v), s)| (i, v, s)).collect();
                changed = Changed::Partial(RowChanges::moved());
                (Self::InsertRows { rows: restored }, Vec::new())
            }
            Self::InsertColumn { index, column } => {
                let name = column.name.clone();
                let formula = column.formula.clone();
                table.insert_column(index, column)?;
                if let Some(f) = formula
                    && let Err(e) = formula::check(table, Some(index), &f)
                {
                    let _ = table.remove_column(index);
                    return Err(invalid(format!("formula of {name:?}: {e}")));
                }
                changed = Changed::Columns(vec![name]);
                (Self::RemoveColumn { index }, vec![ColumnShift::Inserted(index)])
            }
            Self::RemoveColumn { index } => {
                let users = if index < table.n_cols() { formula::users(table, index) } else { Vec::new() };
                if !users.is_empty() {
                    return Err(invalid(format!(
                        "column {:?} is used by the formula of {}; change that formula or convert it to values first",
                        name_of(table, index),
                        users.iter().map(|u| format!("{u:?}")).collect::<Vec<_>>().join(", ")
                    )));
                }
                let column = table.remove_column(index)?;
                (Self::InsertColumn { index, column }, vec![ColumnShift::Removed(index)])
            }
            Self::RenameColumn { index, name } => {
                let old = table.rename_column(index, &name)?;
                // Formulas follow the column to its new name.
                for j in 0..table.n_cols() {
                    if let Some(f) = table.columns()[j].formula.as_deref() {
                        let rewritten = formula::renamed(f, &old, &name);
                        if rewritten != f {
                            table.set_formula(j, Some(rewritten))?;
                        }
                    }
                }
                changed = Changed::Columns(vec![name]);
                (Self::RenameColumn { index, name: old }, Vec::new())
            }
            Self::ReplaceColumn { index, column } => {
                let name = column.name.clone();
                let formula = column.formula.clone();
                let old = table.replace_column(index, column)?;
                if let Some(f) = formula
                    && let Err(e) = formula::check(table, Some(index), &f)
                {
                    let _ = table.replace_column(index, old);
                    return Err(invalid(format!("formula of {name:?}: {e}")));
                }
                changed = Changed::Columns(vec![name]);
                (Self::ReplaceColumn { index, column: old }, Vec::new())
            }
            Self::SetFormula { index, formula: Some(f) } => {
                let old = table.column(index).ok_or_else(|| invalid(format!("no column {index}")))?.clone();
                formula::check(table, Some(index), &f)
                    .map_err(|e| invalid(format!("formula of {:?}: {e}", old.name)))?;
                table.set_formula(index, Some(f))?;
                changed = Changed::Columns(vec![old.name.clone()]);
                (Self::ReplaceColumn { index, column: old }, Vec::new())
            }
            Self::SetFormula { index, formula: None } => {
                let old = table.set_formula(index, None)?;
                (Self::SetFormula { index, formula: old }, Vec::new())
            }
            Self::ReorderRows { order } => {
                table.reorder_rows(&order)?;
                rows.reorder(&order);
                changed = Changed::Partial(RowChanges::moved());
                let mut inverse = vec![0; order.len()];
                for (i, &r) in order.iter().enumerate() {
                    inverse[r] = i;
                }
                (Self::ReorderRows { order: inverse }, Vec::new())
            }
            Self::RenameTable { name } => {
                let name = name.trim();
                if name.is_empty() {
                    return Err(invalid("a table needs a name".to_owned()));
                }
                let old = table.set_name(name);
                (Self::RenameTable { name: old }, Vec::new())
            }
            Self::SetLevel { index, level } => {
                let old = table.column(index).ok_or_else(|| invalid(format!("no column {index}")))?.level;
                table.set_level(index, level)?;
                (Self::SetLevel { index, level: old }, Vec::new())
            }
            Self::SetValueLabels { index, labels } => {
                let old = table.set_value_labels(index, labels)?;
                // Reports name the column's levels by label, so they follow.
                changed = Changed::Columns(vec![name_of(table, index)]);
                (Self::SetValueLabels { index, labels: old }, Vec::new())
            }
            Self::SetHover { index, hover } => {
                let old = table.set_hover(index, hover)?;
                (Self::SetHover { index, hover: old }, Vec::new())
            }
            Self::Batch { label, edits } => {
                let mut inverses = Vec::with_capacity(edits.len());
                let mut shifts = Vec::new();
                for edit in edits {
                    match edit.apply_data(table, rows) {
                        Ok((applied, more)) => {
                            inverses.push(applied.inverse);
                            shifts.extend(applied.shift);
                            changed = std::mem::take(&mut changed).and(more);
                        }
                        Err(e) => {
                            // Put back what was done, so a failed batch changes nothing.
                            for inverse in inverses.into_iter().rev() {
                                let _ = inverse.apply_data(table, rows);
                            }
                            return Err(e);
                        }
                    }
                }
                inverses.reverse();
                (Self::Batch { label, edits: inverses }, shifts)
            }
        };
        Ok((Applied { inverse, shift }, changed))
    }
}

/// A formula column's position, type and level, recorded before an edit so
/// that its undo can restore them.
struct FormulaType {
    index: usize,
    name: String,
    kind: ColumnKind,
    level: MeasurementLevel,
    display: Option<stat_data::TemporalDisplay>,
    formula: String,
}

impl FormulaType {
    /// An edit putting back an (empty) column of the recorded type, level and
    /// formula at the recorded position; the refresh that follows computes
    /// its values.
    fn restoring_edit(self, n_rows: usize) -> Edit {
        let data = match (self.display, self.kind) {
            (Some(display), _) => ColumnData::DateTime { values: vec![f64::NAN; n_rows], display },
            (None, ColumnKind::Text) => ColumnData::Text(vec![String::new(); n_rows]),
            (None, _) => ColumnData::Numeric(vec![f64::NAN; n_rows]),
        };
        let column = Column { level: self.level, formula: Some(self.formula), ..Column::empty(self.name, ColumnKind::Numeric, 0) };
        let column = Column { data, ..column };
        Edit::ReplaceColumn { index: self.index, column }
    }
}

/// The type and level of each formula column of `table`.
fn formula_types(table: &DataTable) -> Vec<FormulaType> {
    table
        .columns()
        .iter()
        .enumerate()
        .filter_map(|(index, c)| {
            Some(FormulaType {
                index,
                name: c.name.clone(),
                kind: c.kind(),
                level: c.level,
                display: c.temporal(),
                formula: c.formula.clone()?,
            })
        })
        .collect()
}

/// One step on the undo or redo stack.
#[derive(Debug, Clone)]
struct Step {
    label: String,
    /// The edit that takes the step.
    edit: Edit,
    /// Its size ([`Edit::approx_bytes`]).
    bytes: usize,
    /// The table's revision and the row states' saved revision the step
    /// was recorded at, and the ones it left: taking the step on exactly
    /// what it left gives the same contents back, so the revisions go back
    /// too (and an edit undone counts as no change to save).
    revisions: [(u64, u64); 2],
}

/// The table's revision and the row states' saved revision.
fn revisions(table: &DataTable, rows: &RowStateManager) -> (u64, u64) {
    (table.revision(), rows.saved_revision())
}

/// Undo and redo stacks for one dataset, bounded by [`HISTORY_LIMIT`] edits
/// and [`HISTORY_BYTES`] of memory.
#[derive(Debug, Clone, Default)]
pub struct EditHistory {
    /// Inverses of applied edits, most recent last.
    undo: Vec<Step>,
    /// Edits to re-apply, most recently undone last.
    redo: Vec<Step>,
    /// While recording: every edit applied (including the ones undo and
    /// redo apply), in order.
    log: Option<Vec<Edit>>,
}

impl EditHistory {
    /// Apply `edit`, record it for undo, and clear the redo stack.
    ///
    /// # Errors
    /// As for [`Edit::apply`]; nothing is recorded on error.
    pub fn apply(&mut self, edit: Edit, table: &mut DataTable, rows: &mut RowStateManager) -> Result<Vec<ColumnShift>> {
        let label = edit.label();
        let logged = self.log.is_some().then(|| edit.clone());
        let before = revisions(table, rows);
        let applied = edit.apply(table, rows)?;
        if let (Some(log), Some(edit)) = (&mut self.log, logged) {
            log.push(edit);
        }
        let bytes = applied.inverse.approx_bytes();
        let after = revisions(table, rows);
        self.undo.push(Step { label, edit: applied.inverse, bytes, revisions: [before, after] });
        self.redo.clear();
        self.trim(HISTORY_LIMIT, HISTORY_BYTES);
        Ok(applied.shift)
    }

    /// Forget the oldest edits until at most `limit` are kept and they hold
    /// at most `bytes` (keeping the most recent one whatever its size).
    fn trim(&mut self, limit: usize, bytes: usize) {
        let mut total: usize = self.undo.iter().map(|s| s.bytes).sum();
        let excess = self.undo.len().saturating_sub(limit);
        let mut drop = 0;
        while drop < self.undo.len() - 1 && (drop < excess || total > bytes) {
            total -= self.undo[drop].bytes;
            drop += 1;
        }
        self.undo.drain(..drop);
    }

    /// Roughly how much memory the undo and redo stacks hold, in bytes.
    #[must_use]
    pub fn approx_bytes(&self) -> usize {
        self.undo.iter().chain(&self.redo).map(|s| s.bytes).sum()
    }

    /// The edit an undo would apply, and its label.
    #[must_use]
    pub fn next_undo(&self) -> Option<(&str, &Edit)> {
        self.undo.last().map(|s| (s.label.as_str(), &s.edit))
    }

    /// The edit a redo would apply, and its label.
    #[must_use]
    pub fn next_redo(&self) -> Option<(&str, &Edit)> {
        self.redo.last().map(|s| (s.label.as_str(), &s.edit))
    }

    /// Undo the most recent edit. `None` if there is nothing to undo.
    ///
    /// # Errors
    /// As for [`Edit::apply`] (only if the data was changed outside the
    /// history); the entry is kept in that case.
    pub fn undo(&mut self, table: &mut DataTable, rows: &mut RowStateManager) -> Option<Result<Vec<ColumnShift>>> {
        let edit = self.log.is_some().then(|| self.undo.last().map(|s| s.edit.clone())).flatten();
        let result = Self::step(&mut self.undo, &mut self.redo, table, rows);
        self.log_step(edit, result.as_ref());
        result
    }

    /// Re-apply the most recently undone edit. `None` if there is nothing to redo.
    ///
    /// # Errors
    /// As for [`EditHistory::undo`].
    pub fn redo(&mut self, table: &mut DataTable, rows: &mut RowStateManager) -> Option<Result<Vec<ColumnShift>>> {
        let edit = self.log.is_some().then(|| self.redo.last().map(|s| s.edit.clone())).flatten();
        let result = Self::step(&mut self.redo, &mut self.undo, table, rows);
        self.log_step(edit, result.as_ref());
        result
    }

    fn log_step(&mut self, edit: Option<Edit>, result: Option<&Result<Vec<ColumnShift>>>) {
        if let (Some(log), Some(edit), Some(Ok(_))) = (&mut self.log, edit, result) {
            log.push(edit);
        }
    }

    /// Start (`true`) or stop keeping a log of every edit applied, for
    /// recording; stopping discards the log.
    pub fn set_logging(&mut self, on: bool) {
        self.log = on.then(Vec::new);
    }

    /// The edits applied since the last call (empty when not logging).
    pub fn take_logged(&mut self) -> Vec<Edit> {
        self.log.as_mut().map(std::mem::take).unwrap_or_default()
    }

    fn step(
        from: &mut Vec<Step>,
        to: &mut Vec<Step>,
        table: &mut DataTable,
        rows: &mut RowStateManager,
    ) -> Option<Result<Vec<ColumnShift>>> {
        let step = from.pop()?;
        let before = revisions(table, rows);
        match step.edit.clone().apply(table, rows) {
            Ok(applied) => {
                let [recorded, left] = step.revisions;
                // Nothing changed the data since the step was recorded, so
                // taking it gives back the contents the recorded revisions
                // stand for.
                if before.0 == left.0 {
                    table.restore_revision(recorded.0);
                }
                if before.1 == left.1 {
                    rows.restore_saved_revision(recorded.1);
                }
                let after = revisions(table, rows);
                let bytes = applied.inverse.approx_bytes();
                to.push(Step { label: step.label, edit: applied.inverse, bytes, revisions: [before, after] });
                Some(Ok(applied.shift))
            }
            Err(e) => {
                from.push(step);
                Some(Err(e))
            }
        }
    }

    /// Forget all history.
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::{SelectionMode, TemporalFormat};

    /// While logging, the history lists every edit it applies, undo and
    /// redo included, as the edit actually applied.
    #[test]
    fn logging_lists_every_applied_edit() {
        let mut t = table();
        let mut rows = RowStateManager::new(4);
        let mut h = EditHistory::default();
        let set = Edit::SetCell { row: 0, column: 0, value: CellValue::Number(9.0) };
        h.apply(set.clone(), &mut t, &mut rows).unwrap();
        assert!(h.take_logged().is_empty(), "not logging yet");
        h.set_logging(true);
        let rename = Edit::RenameColumn { index: 1, name: "group".into() };
        h.apply(rename.clone(), &mut t, &mut rows).unwrap();
        h.undo(&mut t, &mut rows).unwrap().unwrap();
        h.redo(&mut t, &mut rows).unwrap().unwrap();
        assert_eq!(h.take_logged(), [rename.clone(), Edit::RenameColumn { index: 1, name: "g".into() }, rename]);
        assert!(h.take_logged().is_empty(), "taken once");
        h.set_logging(false);
        h.undo(&mut t, &mut rows).unwrap().unwrap();
        assert!(h.take_logged().is_empty());
    }

    /// Renaming the table trims the name, refuses a blank one, and undoes
    /// and redoes like any other edit.
    #[test]
    fn renaming_the_table_is_undoable() {
        let mut t = table();
        let mut rows = RowStateManager::new(4);
        let mut h = EditHistory::default();
        let old = t.name().to_owned();
        assert!(h.apply(Edit::RenameTable { name: "  ".into() }, &mut t, &mut rows).is_err());
        h.apply(Edit::RenameTable { name: " Batch 2 ".into() }, &mut t, &mut rows).unwrap();
        assert_eq!(t.name(), "Batch 2");
        h.undo(&mut t, &mut rows).unwrap().unwrap();
        assert_eq!(t.name(), old);
        h.redo(&mut t, &mut rows).unwrap().unwrap();
        assert_eq!(t.name(), "Batch 2");
    }

    /// F-21: a cell edit that makes a formula return text turns the formula
    /// column into text; undoing the edit turns it back into a continuous
    /// number column, and redo makes it text again.
    #[test]
    fn undo_restores_a_formula_columns_type() {
        let mut t = table();
        let mut rows = RowStateManager::new(4);
        let mut h = EditHistory::default();
        let mut c = Column::empty("size", ColumnKind::Numeric, 4);
        c.formula = Some("if x > 5 { \"big\" } else { x * 2 }".into());
        h.apply(Edit::InsertColumn { index: 2, column: c }, &mut t, &mut rows).unwrap();
        let kind = |t: &DataTable| (t.column(2).unwrap().kind(), t.column(2).unwrap().level);
        assert_eq!(kind(&t), (ColumnKind::Numeric, MeasurementLevel::Continuous));
        h.apply(Edit::SetCell { row: 0, column: 0, value: CellValue::Number(9.0) }, &mut t, &mut rows).unwrap();
        assert_eq!(kind(&t), (ColumnKind::Text, MeasurementLevel::Nominal));
        h.undo(&mut t, &mut rows).unwrap().unwrap();
        assert_eq!(kind(&t), (ColumnKind::Numeric, MeasurementLevel::Continuous));
        assert_eq!(t.column(2).unwrap().as_numeric().unwrap(), &[2.0, 4.0, 6.0, 8.0]);
        assert_eq!(t.column(0).unwrap().as_numeric().unwrap()[0], 1.0);
        h.redo(&mut t, &mut rows).unwrap().unwrap();
        assert_eq!(kind(&t), (ColumnKind::Text, MeasurementLevel::Nominal));
        assert_eq!(t.column(2).unwrap().cell_text(0), "big");
        h.undo(&mut t, &mut rows).unwrap().unwrap();
        assert_eq!(kind(&t), (ColumnKind::Numeric, MeasurementLevel::Continuous));
    }

    fn table() -> DataTable {
        DataTable::new(
            "t",
            vec![
                Column::numeric("x", vec![1.0, 2.0, 3.0, 4.0]),
                Column::text("g", vec!["a".into(), "b".into(), "c".into(), "d".into()]),
            ],
        )
        .unwrap()
    }

    #[test]
    fn clear_cells_skips_missing_and_formula_cells_and_undoes() {
        let mut t = table();
        let mut rows = RowStateManager::new(4);
        let mut h = EditHistory::default();
        let edit = Edit::clear_cells(&t, &[0, 1, 9], &[0, 1, 7]).expect("something to clear");
        assert_eq!(edit.label(), "clear 4 cells");
        h.apply(edit, &mut t, &mut rows).unwrap();
        assert_eq!(t.column(0).unwrap().cell_text(0), "");
        assert_eq!(t.column(1).unwrap().cell_text(1), "");
        assert_eq!(t.column(0).unwrap().cell_text(2), "3");
        assert!(Edit::clear_cells(&t, &[0, 1], &[0, 1]).is_none(), "already empty");
        h.undo(&mut t, &mut rows).unwrap().unwrap();
        assert_eq!(t, table());

        let computed = Column { formula: Some("row()".into()), ..Column::numeric("r", vec![1.0, 2.0, 3.0, 4.0]) };
        let f = DataTable::new("f", vec![computed]).unwrap();
        assert!(Edit::clear_cells(&f, &[0], &[0]).is_none(), "formula cells are computed");
    }

    #[test]
    fn fill_down_copies_the_first_row() {
        let mut t = table();
        let mut rows = RowStateManager::new(4);
        let mut h = EditHistory::default();
        let edit = Edit::fill_down(&t, &[1, 2, 3], &[0, 1]).expect("cells to fill");
        assert_eq!(edit.label(), "fill down 4 cells");
        h.apply(edit, &mut t, &mut rows).unwrap();
        let texts = |j: usize| (0..4).map(|r| t.column(j).unwrap().cell_text(r)).collect::<Vec<_>>();
        assert_eq!(texts(0), ["1", "2", "2", "2"]);
        assert_eq!(texts(1), ["a", "b", "b", "b"]);
        assert!(Edit::fill_down(&t, &[1, 2, 3], &[0, 1]).is_none(), "nothing left to change");
        assert!(Edit::fill_down(&t, &[2], &[0]).is_none(), "one row has nothing to fill");
    }

    #[test]
    fn every_edit_undoes_and_redoes() {
        let original = table();
        let mut t = original.clone();
        let mut rows = RowStateManager::new(4);
        rows.select([1, 3], SelectionMode::Replace);
        rows.set_excluded([2], true);
        let original_states: Vec<RowState> = (0..4).map(|r| rows.state(r)).collect();
        let mut h = EditHistory::default();

        // Each edit is built from the table as it is when the edit is made.
        type MakeEdit = Box<dyn Fn(&DataTable) -> Edit>;
        let edits: Vec<MakeEdit> = vec![
            Box::new(|t| Edit::set_cell_text(t, 0, 0, "10").unwrap()),
            Box::new(|t| Edit::set_cell_text(t, 1, 1, "").unwrap()),
            Box::new(|t| Edit::insert_empty_rows(t, 1, 2)),
            Box::new(|_| Edit::RemoveRows { rows: vec![0, 3, 5] }),
            Box::new(|t| Edit::new_column(t, 1, ColumnKind::Numeric)),
            Box::new(|_| Edit::RenameColumn { index: 0, name: "renamed".into() }),
            Box::new(|_| Edit::SetLevel { index: 2, level: MeasurementLevel::Ordinal }),
            Box::new(|t| Edit::new_column(t, 0, ColumnKind::date_time(TemporalFormat::Date))),
            Box::new(|t| Edit::set_cell_text(t, 1, 0, "03/04/2026").unwrap()),
            Box::new(|t| Edit::swap_day_month(t, 0).unwrap().0),
            Box::new(|t| Edit::convert_column(t, 0, ColumnKind::date_time(TemporalFormat::DateTime)).unwrap().0),
            Box::new(|t| Edit::ReorderRows { order: (0..t.n_rows()).rev().collect() }),
            Box::new(|t| Edit::ReorderRows { order: (0..t.n_rows()).rev().collect() }),
        ];
        let mut snapshots = vec![t.clone()];
        for make in edits {
            h.apply(make(&t), &mut t, &mut rows).unwrap();
            assert_eq!(t.n_rows(), rows.len());
            snapshots.push(t.clone());
        }
        assert_eq!(t.column(0).unwrap().cell_text(1), "2026-04-03 00:00:00");
        assert!(Edit::swap_day_month(&t, 1).is_err(), "not a date column");
        let (converted, lost) = Edit::convert_column(&t, 0, ColumnKind::Text).unwrap();
        assert_eq!(lost, 0);
        h.apply(converted, &mut t, &mut rows).unwrap();
        snapshots.push(t.clone());
        let after = t.clone();

        // Undo everything, checking each intermediate state.
        for expected in snapshots.iter().rev().skip(1) {
            h.undo(&mut t, &mut rows).unwrap().unwrap();
            assert_eq!(&t, expected);
        }
        assert!(h.undo(&mut t, &mut rows).is_none());
        assert_eq!(t, original);
        let states: Vec<RowState> = (0..4).map(|r| rows.state(r)).collect();
        assert_eq!(states, original_states, "row states restored with their rows");

        // Redo everything.
        while h.redo(&mut t, &mut rows).is_some() {}
        assert_eq!(t, after);
        // A new edit clears the redo stack.
        h.undo(&mut t, &mut rows);
        h.apply(Edit::set_cell_text(&t, 0, 0, "2026-01-01").unwrap(), &mut t, &mut rows).unwrap();
        assert!(h.next_redo().is_none());
    }

    #[test]
    fn rejected_edits_change_nothing() {
        let mut t = table();
        let mut rows = RowStateManager::new(4);
        let mut h = EditHistory::default();
        assert!(Edit::set_cell_text(&t, 0, 0, "abc").is_err());
        assert!(h.apply(Edit::RenameColumn { index: 0, name: "g".into() }, &mut t, &mut rows).is_err());
        assert!(h.apply(Edit::RemoveRows { rows: vec![9] }, &mut t, &mut rows).is_err());
        assert!(h.apply(Edit::SetLevel { index: 1, level: MeasurementLevel::Continuous }, &mut t, &mut rows).is_err());
        assert!(h.next_undo().is_none());
        assert_eq!(t, table());
        assert_eq!(rows.len(), 4);
    }

    #[test]
    fn row_removal_carries_row_states() {
        let mut t = table();
        let mut rows = RowStateManager::new(4);
        rows.set_excluded([1], true);
        rows.select([3], SelectionMode::Replace);
        let mut h = EditHistory::default();
        h.apply(Edit::RemoveRows { rows: vec![1] }, &mut t, &mut rows).unwrap();
        assert_eq!(t.column(1).unwrap().data, ColumnData::Text(vec!["a".into(), "c".into(), "d".into()]));
        assert_eq!(rows.counts().excluded, 0);
        assert_eq!(rows.selected_rows(), vec![2]);
        h.undo(&mut t, &mut rows);
        assert!(rows.is_excluded(1));
        assert_eq!(rows.selected_rows(), vec![3]);
    }

    #[test]
    fn column_shifts() {
        let mut t = table();
        let mut rows = RowStateManager::new(4);
        let mut h = EditHistory::default();
        let shift = h.apply(Edit::new_column(&t, 0, ColumnKind::Text), &mut t, &mut rows).unwrap();
        assert_eq!(shift, [ColumnShift::Inserted(0)]);
        assert_eq!(shift[0].map(0), Some(1));
        assert_eq!(t.find("x"), Some(1));
        let undo = h.next_undo().unwrap().1.removed_columns(t.n_cols());
        assert_eq!(undo, [0], "undoing an insert removes the column");
        let shift = h.undo(&mut t, &mut rows).unwrap().unwrap();
        assert_eq!(shift, [ColumnShift::Removed(0)]);
        assert_eq!(ColumnShift::Removed(1).map(0), Some(0));
        assert_eq!(ColumnShift::Removed(1).map(1), None);
        assert_eq!(ColumnShift::Removed(1).map(2), Some(1));
        assert_eq!(h.next_redo().unwrap().0, "add column \"Column 1\"");
    }

    #[test]
    fn several_new_columns_are_one_step() {
        let mut t = table();
        let mut rows = RowStateManager::new(4);
        let mut h = EditHistory::default();
        h.apply(Edit::new_column(&t, 2, ColumnKind::Numeric), &mut t, &mut rows).unwrap();
        let shift = h.apply(Edit::new_columns(&t, 1, ColumnKind::Text, 3, ""), &mut t, &mut rows).unwrap();
        assert_eq!(shift, [ColumnShift::Inserted(1), ColumnShift::Inserted(2), ColumnShift::Inserted(3)]);
        let names: Vec<&str> = t.columns().iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["x", "Column 2", "Column 3", "Column 4", "g", "Column 1"]);
        assert!((1..4).all(|j| t.column(j).unwrap().kind() == ColumnKind::Text));
        assert_eq!(h.next_undo().unwrap().0, "add 3 columns");
        h.undo(&mut t, &mut rows).unwrap().unwrap();
        assert_eq!(t.n_cols(), 3, "one undo removes all three");
        let one = Edit::new_columns(&t, 0, ColumnKind::Numeric, 1, "");
        assert!(matches!(one, Edit::InsertColumn { index: 0, .. }));
    }

    #[test]
    fn several_columns_are_deleted_as_one_step() {
        let mut t = table();
        let mut rows = RowStateManager::new(4);
        let mut h = EditHistory::default();
        h.apply(Edit::new_columns(&t, 1, ColumnKind::Numeric, 2, "k"), &mut t, &mut rows).unwrap();
        let removed = Edit::remove_columns(&[3, 0, 3]).removed_columns(t.n_cols());
        assert_eq!(removed, [0, 3]);
        h.apply(Edit::remove_columns(&[3, 0, 3]), &mut t, &mut rows).unwrap();
        let names: Vec<&str> = t.columns().iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["k 1", "k 2"]);
        assert_eq!(h.next_undo().unwrap().0, "delete 2 columns");
        h.undo(&mut t, &mut rows).unwrap().unwrap();
        let names: Vec<&str> = t.columns().iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["x", "k 1", "k 2", "g"]);
        assert!(matches!(Edit::remove_columns(&[2]), Edit::RemoveColumn { index: 2 }));
    }

    #[test]
    fn new_columns_take_names_from_a_stem() {
        let mut t = table();
        let mut rows = RowStateManager::new(4);
        let mut h = EditHistory::default();
        let name = |e: &Edit| match e {
            Edit::InsertColumn { column, .. } => column.name.clone(),
            _ => String::new(),
        };
        assert_eq!(name(&Edit::new_columns(&t, 2, ColumnKind::Numeric, 1, " Weight ")), "Weight");
        assert_eq!(name(&Edit::new_columns(&t, 2, ColumnKind::Numeric, 1, "x")), "x 2", "x is taken");
        h.apply(Edit::new_columns(&t, 2, ColumnKind::Numeric, 1, "Rep 2"), &mut t, &mut rows).unwrap();
        h.apply(Edit::new_columns(&t, 3, ColumnKind::Numeric, 3, "Rep"), &mut t, &mut rows).unwrap();
        let names: Vec<&str> = t.columns().iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["x", "g", "Rep 2", "Rep 1", "Rep 3", "Rep 4"], "Rep 2 is skipped");
    }

    #[test]
    fn sorting_moves_row_states_and_undoes() {
        let mut t = table();
        let mut rows = RowStateManager::new(4);
        rows.set_excluded([0], true);
        rows.select([3], SelectionMode::Replace);
        let mut h = EditHistory::default();
        h.apply(Edit::ReorderRows { order: vec![3, 0, 2, 1] }, &mut t, &mut rows).unwrap();
        assert_eq!(t.column(0).unwrap().as_numeric().unwrap(), &[4.0, 1.0, 3.0, 2.0]);
        assert!(rows.is_excluded(1) && rows.is_selected(0));
        assert_eq!(h.next_undo().unwrap().0, "sort rows");
        h.undo(&mut t, &mut rows).unwrap().unwrap();
        assert_eq!(t, table());
        assert!(rows.is_excluded(0) && rows.is_selected(3));
        assert!(h.apply(Edit::ReorderRows { order: vec![0, 0, 1, 2] }, &mut t, &mut rows).is_err());
        assert_eq!(t, table());
    }

    /// After cell edits, pastes that add rows, deletions and sorts (and
    /// their undo), formulas recomputed only where needed hold exactly what
    /// recomputing every row gives, row-wise or not.
    #[test]
    fn partial_recomputation_matches_a_full_one() {
        let mut t = table();
        let mut rows = RowStateManager::new(4);
        let mut h = EditHistory::default();
        for (k, f) in ["x * 2 + 1", "x - col_mean(\"x\")", "lag(\"x\")", "row() + random_uniform()", "upper(g)"]
            .into_iter()
            .enumerate()
        {
            let mut c = Column::empty(format!("f{k}"), ColumnKind::Numeric, 4);
            c.formula = Some(f.into());
            h.apply(Edit::InsertColumn { index: t.n_cols(), column: c }, &mut t, &mut rows).unwrap();
        }
        let check = |t: &DataTable, what: &str| {
            let mut full = t.clone();
            formula::refresh_all(&mut full);
            assert_eq!(*t, full, "after {what}");
        };
        h.apply(Edit::set_cell_text(&t, 1, 0, "7").unwrap(), &mut t, &mut rows).unwrap();
        check(&t, "a cell edit");
        h.apply(Edit::set_cell_text(&t, 2, 1, "z").unwrap(), &mut t, &mut rows).unwrap();
        check(&t, "a text cell edit");
        let mut paste = vec![Edit::insert_empty_rows(&t, 1, 2)];
        paste.push(Edit::SetCell { row: 1, column: 0, value: CellValue::Number(9.0) });
        paste.push(Edit::SetCell { row: 4, column: 0, value: CellValue::Number(-3.0) });
        h.apply(Edit::Batch { label: "paste".into(), edits: paste }, &mut t, &mut rows).unwrap();
        check(&t, "a paste adding rows");
        h.apply(Edit::RemoveRows { rows: vec![0, 3] }, &mut t, &mut rows).unwrap();
        check(&t, "deleting rows");
        h.apply(Edit::ReorderRows { order: vec![3, 2, 1, 0] }, &mut t, &mut rows).unwrap();
        check(&t, "sorting");
        while h.undo(&mut t, &mut rows).is_some() {
            check(&t, "undo");
        }
    }

    /// Undo gives the table and row states back their revisions, so a
    /// change undone counts as no change; a change made outside the history
    /// in between stops that.
    #[test]
    fn undo_restores_revisions() {
        let mut t = table();
        let mut rows = RowStateManager::new(4);
        let mut h = EditHistory::default();
        let start = (t.revision(), rows.saved_revision());
        h.apply(Edit::insert_empty_rows(&t, 1, 1), &mut t, &mut rows).unwrap();
        let edited = (t.revision(), rows.saved_revision());
        assert_ne!(edited, start);
        h.undo(&mut t, &mut rows).unwrap().unwrap();
        assert_eq!((t.revision(), rows.saved_revision()), start);
        h.redo(&mut t, &mut rows).unwrap().unwrap();
        assert_eq!((t.revision(), rows.saved_revision()), edited);
        t.set_value(0, 0, CellValue::Number(5.0)).unwrap();
        h.undo(&mut t, &mut rows).unwrap().unwrap();
        assert_ne!(t.revision(), start.0, "the data is not what it was");
    }

    /// The history forgets its oldest edits beyond the byte budget, but
    /// always keeps the newest.
    #[test]
    fn history_is_bounded_by_size() {
        let mut t = table();
        let mut rows = RowStateManager::new(4);
        let mut h = EditHistory::default();
        let big = |v: f64| Edit::ReplaceColumn { index: 0, column: Column::numeric("x", vec![v; 4]) };
        assert!(big(0.0).approx_bytes() >= 32, "four numbers");
        for v in 0..10 {
            h.apply(big(f64::from(v)), &mut t, &mut rows).unwrap();
        }
        let one = h.undo[0].bytes;
        h.trim(HISTORY_LIMIT, 3 * one);
        assert_eq!(h.undo.len(), 3, "the oldest went");
        assert!(h.approx_bytes() <= 3 * one);
        h.trim(HISTORY_LIMIT, 1);
        assert_eq!(h.undo.len(), 1, "the newest stays");
        h.undo(&mut t, &mut rows).unwrap().unwrap();
        assert_eq!(t.column(0).unwrap().as_numeric().unwrap(), &[8.0; 4]);
        let mut h = EditHistory::default();
        for v in 0..5 {
            h.apply(big(f64::from(v)), &mut t, &mut rows).unwrap();
        }
        h.trim(2, usize::MAX);
        assert_eq!(h.undo.len(), 2, "the count limit still holds");
    }

    /// Formula columns recompute after every edit (and undo), cannot be
    /// typed into, keep the columns they use, and follow renames.
    #[test]
    fn formula_columns_follow_edits() {
        let original = table();
        let mut t = original.clone();
        let mut rows = RowStateManager::new(4);
        let mut h = EditHistory::default();
        let numbers = |t: &DataTable, j: usize| t.column(j).unwrap().as_numeric().unwrap().to_vec();
        let mut c = Column::empty("double", ColumnKind::Numeric, 4);
        c.formula = Some("x * 2".into());
        h.apply(Edit::InsertColumn { index: 2, column: c }, &mut t, &mut rows).unwrap();
        assert_eq!(numbers(&t, 2), [2.0, 4.0, 6.0, 8.0]);
        h.apply(Edit::set_cell_text(&t, 0, 0, "10").unwrap(), &mut t, &mut rows).unwrap();
        assert_eq!(numbers(&t, 2)[0], 20.0);
        h.undo(&mut t, &mut rows).unwrap().unwrap();
        assert_eq!(numbers(&t, 2)[0], 2.0, "undo recomputes too");
        h.redo(&mut t, &mut rows).unwrap().unwrap();

        let typed = h.apply(Edit::SetCell { row: 0, column: 2, value: CellValue::Number(1.0) }, &mut t, &mut rows);
        assert!(typed.unwrap_err().to_string().contains("computed by a formula"));
        let removed = h.apply(Edit::RemoveColumn { index: 0 }, &mut t, &mut rows);
        assert!(removed.unwrap_err().to_string().contains("used by the formula of \"double\""));
        let cycle = h.apply(Edit::SetFormula { index: 0, formula: Some("double + 1".into()) }, &mut t, &mut rows);
        assert!(cycle.unwrap_err().to_string().contains("circle"));
        let bad = h.apply(Edit::SetFormula { index: 2, formula: Some("x +".into()) }, &mut t, &mut rows);
        assert!(bad.is_err());

        h.apply(Edit::RenameColumn { index: 0, name: "first value".into() }, &mut t, &mut rows).unwrap();
        assert_eq!(t.column(2).unwrap().formula.as_deref(), Some("col(\"first value\") * 2"));
        h.apply(Edit::ReorderRows { order: vec![3, 2, 1, 0] }, &mut t, &mut rows).unwrap();
        assert_eq!(numbers(&t, 2), [8.0, 6.0, 4.0, 20.0]);
        h.apply(Edit::RemoveRows { rows: vec![0] }, &mut t, &mut rows).unwrap();
        h.apply(Edit::SetFormula { index: 2, formula: Some("row()".into()) }, &mut t, &mut rows).unwrap();
        assert_eq!(numbers(&t, 2), [1.0, 2.0, 3.0]);
        h.apply(Edit::SetFormula { index: 2, formula: None }, &mut t, &mut rows).unwrap();
        h.apply(Edit::SetCell { row: 0, column: 2, value: CellValue::Number(9.0) }, &mut t, &mut rows).unwrap();
        h.apply(Edit::RemoveColumn { index: 0 }, &mut t, &mut rows).unwrap();

        // Choosing a type: dates stay dates; a new column takes the values' type.
        let edit = Edit::formula_column(&t, Some(1), "", 0, "\"n\" + row()", None).unwrap();
        assert!(matches!(edit, Edit::SetFormula { index: 1, .. }));
        let date = ColumnKind::date_time(TemporalFormat::Date);
        let edit = Edit::formula_column(&t, Some(1), "", 0, "row() * 86400", Some(date)).unwrap();
        h.apply(edit, &mut t, &mut rows).unwrap();
        assert_eq!(t.column(1).unwrap().cell_text(1), "1970-01-03");
        let edit = Edit::formula_column(&t, None, "tag", 2, "\"n\" + row()", None).unwrap();
        h.apply(edit, &mut t, &mut rows).unwrap();
        assert_eq!(t.column(2).unwrap().cell_text(2), "n3");
        assert_eq!(t.column(2).unwrap().level, MeasurementLevel::Nominal);

        while h.undo(&mut t, &mut rows).is_some() {}
        assert_eq!(t, original);
        while h.redo(&mut t, &mut rows).is_some() {}
        assert_eq!(t.column(2).unwrap().formula.as_deref(), Some("\"n\" + row()"));
    }

    #[test]
    fn labels() {
        let t = table();
        assert_eq!(Edit::insert_empty_rows(&t, 4, 1).label(), "add 1 row");
        assert_eq!(Edit::RemoveRows { rows: vec![0, 1] }.label(), "delete 2 rows");
    }
}
