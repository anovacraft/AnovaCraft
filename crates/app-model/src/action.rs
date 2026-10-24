//! The action model: everything a person can do to an open table, as typed
//! values that both the desktop application and scripts apply through the
//! same code, and that write themselves as the Rhai call doing the same
//! thing (for recording and "Copy as script").
//!
//! An [`Action`] works on a table's data, row states and report list (newest
//! report first, as the application lists them). Data changes are
//! [`Edit`]s; row-state actions give the whole new set (the rows selected,
//! excluded, hidden or coloured), so applying one twice is harmless; report
//! actions add, change or close one report by its position in the list.

use stat_core::Result;
use stat_data::{CellValue, DataTable, RowColor, RowMarker, RowState, RowStateManager, SelectionMode};

use crate::editing::{Applied, Edit};
use crate::project::{SavedReport, column_to_json, invalid, level_name, report_to_json};
use crate::script_text::{compact, number, pretty, string};

/// One thing done to a table.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// A change to the data (reports follow their columns).
    Edit(Edit),
    /// Select exactly these rows.
    Select(Vec<usize>),
    /// Exclude exactly these rows from analyses.
    SetExcluded(Vec<usize>),
    /// Hide exactly these rows in plots.
    SetHidden(Vec<usize>),
    /// Give exactly these rows these colours (other rows have none).
    SetColors(Vec<(usize, RowColor)>),
    /// Give exactly these rows these markers (other rows have none).
    SetMarkers(Vec<(usize, RowMarker)>),
    /// Colour every row (and give it a marker, for a categorical column)
    /// from its value in the named column, which plots then name in their
    /// key; see [`color_by_column`].
    ColorByColumn(String),
    /// Name (or with `None`, forget) the column plots key the row colours by.
    SetStyleColumn(Option<String>),
    /// Clear every row's colour and marker.
    ClearRowStyles,
    /// Keep exactly these saved row subsets, with this one active.
    SetSubsets(RowSubsets),
    /// Add a report at the top of the list.
    AddReport(SavedReport),
    /// Replace the request (and display options) of report `index`.
    UpdateReport(usize, SavedReport),
    /// Close report `index`.
    RemoveReport(usize),
}

/// Apply `edit` and keep `reports` pointing at the same columns: positions
/// move with inserted and removed columns, and renamed columns keep their
/// reports (graph canvases refer to columns by name, so only renames
/// reach them).
///
/// # Errors
/// As for [`Edit::apply`]; nothing changes on error.
pub fn apply_edit(
    edit: Edit,
    table: &mut DataTable,
    rows: &mut RowStateManager,
    reports: &mut [SavedReport],
) -> Result<Applied> {
    let before: Vec<String> = table.columns().iter().map(|c| c.name.clone()).collect();
    let applied = edit.apply(table, rows)?;
    for shift in &applied.shift {
        for r in reports.iter_mut() {
            if !matches!(r, SavedReport::Graph { .. }) {
                *r = r.clone().map_columns(|c| shift.map(c).unwrap_or(c));
            }
        }
    }
    if applied.shift.is_empty() && before.len() == table.n_cols() {
        for (old, col) in before.iter().zip(table.columns()) {
            if *old != col.name {
                for r in reports.iter_mut() {
                    r.rename_column(old, &col.name);
                }
            }
        }
    }
    Ok(applied)
}

/// A table's saved row subsets: each name with its rows (ascending), in the
/// order they were made, and the name of the active one (whose rows alone
/// analyses and plots use).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RowSubsets {
    /// Name and rows of each subset.
    pub subsets: Vec<(String, Vec<usize>)>,
    /// The active subset's name (`None`: every row).
    pub active: Option<String>,
}

impl RowSubsets {
    /// The subsets `rows` keeps.
    #[must_use]
    pub fn of(rows: &RowStateManager) -> Self {
        let names = rows.subset_names();
        Self {
            subsets: names.iter().enumerate().map(|(i, n)| (n.clone(), rows.subset_rows(i))).collect(),
            active: rows.active_subset().and_then(|i| names.get(i).cloned()),
        }
    }

    /// Replace the subsets of `rows` with these.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`](stat_core::StatError) for a row out
    /// of range, an empty or repeated name, more than
    /// [`stat_data::MAX_SUBSETS`] subsets, or an active name that is not
    /// one of them; nothing changes on error.
    pub fn apply_to(&self, rows: &mut RowStateManager) -> Result<()> {
        let n = rows.len();
        if self.subsets.len() > stat_data::MAX_SUBSETS {
            return Err(invalid(format!("a table keeps at most {} row subsets", stat_data::MAX_SUBSETS)));
        }
        for (i, (name, list)) in self.subsets.iter().enumerate() {
            if name.trim().is_empty() {
                return Err(invalid("a row subset needs a name"));
            }
            if self.subsets[..i].iter().any(|(other, _)| other == name) {
                return Err(invalid(format!("two row subsets are called {name:?}")));
            }
            check_rows(list, n)?;
        }
        let active = match &self.active {
            Some(name) => Some(
                self.subsets
                    .iter()
                    .position(|(n, _)| n == name)
                    .ok_or_else(|| invalid(format!("there is no row subset called {name:?}")))?,
            ),
            None => None,
        };
        rows.set_active_subset(None);
        while !rows.subset_names().is_empty() {
            rows.remove_subset(rows.subset_names().len() - 1);
        }
        for (name, list) in &self.subsets {
            rows.save_subset(name, list.iter().copied());
        }
        rows.set_active_subset(active);
        Ok(())
    }
}

/// Colour every row from its value in column `column` (by name): a colour
/// and marker per level of a categorical column (levels named by their
/// value labels), or the sequential ramp over a continuous column's range;
/// rows with a missing value lose their colour and marker. The column is
/// remembered, so plots name its levels in their key.
///
/// # Errors
/// [`StatError::InvalidArgument`](stat_core::StatError) for an unknown
/// column or one with no values; nothing changes on error.
pub fn color_by_column(table: &DataTable, rows: &mut RowStateManager, column: &str) -> Result<()> {
    if table.find(column).is_none() {
        return Err(invalid(format!("there is no column called {column:?}")));
    }
    let scheme = stat_viz::RowStyleScheme::of(table, column)
        .ok_or_else(|| invalid(format!("column {column:?} has no values to colour by")))?;
    let (colors, markers) = scheme.styles(rows.len());
    for (r, (c, m)) in colors.into_iter().zip(markers).enumerate() {
        rows.set_color([r], c);
        rows.set_marker([r], m);
    }
    rows.set_style_column(Some(column.to_owned()));
    Ok(())
}

/// A table's row colours and markers, and the column plots key them by.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RowStyles {
    /// `(row, colour)` for every coloured row, ascending.
    pub colors: Vec<(usize, RowColor)>,
    /// `(row, marker)` for every row with a marker, ascending.
    pub markers: Vec<(usize, RowMarker)>,
    /// The column the styles came from.
    pub column: Option<String>,
}

impl RowStyles {
    /// The styles `rows` holds.
    #[must_use]
    pub fn of(rows: &RowStateManager) -> Self {
        Self {
            colors: (0..rows.len()).filter_map(|r| rows.color(r).map(|c| (r, c))).collect(),
            markers: (0..rows.len()).filter_map(|r| rows.marker(r).map(|m| (r, m))).collect(),
            column: rows.style_column().map(str::to_owned),
        }
    }

    /// Give `rows` exactly these styles (rows past the end are skipped).
    pub fn apply_to(&self, rows: &mut RowStateManager) {
        let n = rows.len();
        rows.set_color(0..n, None);
        rows.set_marker(0..n, None);
        for &(r, c) in self.colors.iter().filter(|(r, _)| *r < n) {
            rows.set_color([r], Some(c));
        }
        for &(r, m) in self.markers.iter().filter(|(r, _)| *r < n) {
            rows.set_marker([r], Some(m));
        }
        rows.set_style_column(self.column.clone());
    }
}

fn check_rows(list: &[usize], n: usize) -> Result<()> {
    match list.iter().find(|&&r| r >= n) {
        Some(r) => Err(invalid(format!("row {r} is past the last row ({})", n.saturating_sub(1)))),
        None => Ok(()),
    }
}

fn rows_where(rows: &RowStateManager, test: impl Fn(usize) -> bool) -> Vec<usize> {
    (0..rows.len()).filter(|&r| test(r)).collect()
}

/// `"#rrggbb"` for a row colour.
#[must_use]
pub fn color_text(c: RowColor) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
}

fn row_list(list: &[usize]) -> String {
    format!("[{}]", list.iter().map(ToString::to_string).collect::<Vec<_>>().join(", "))
}

fn cell_text(value: &CellValue) -> String {
    match value {
        CellValue::Number(x) => number(*x),
        CellValue::Text(s) => string(s),
    }
}

/// The Rhai call that runs `report` on the table in variable `var`.
#[must_use]
pub fn analysis_call(var: &str, report: &SavedReport, table: &DataTable) -> String {
    format!("{var}.analyze({});", pretty(&report_to_json(report, table), 0))
}

impl Action {
    /// Apply the action to a table, its row states and its reports (newest
    /// first).
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] for a row or report number out of range,
    /// or as for [`apply_edit`]; nothing changes on error.
    pub fn apply(
        self,
        table: &mut DataTable,
        rows: &mut RowStateManager,
        reports: &mut Vec<SavedReport>,
    ) -> Result<()> {
        let n = rows.len();
        match self {
            Self::Edit(edit) => {
                apply_edit(edit, table, rows, reports)?;
            }
            Self::Select(list) => {
                check_rows(&list, n)?;
                rows.select(list, SelectionMode::Replace);
            }
            Self::SetExcluded(list) => {
                check_rows(&list, n)?;
                rows.set_excluded(0..n, false);
                rows.set_excluded(list, true);
            }
            Self::SetHidden(list) => {
                check_rows(&list, n)?;
                rows.set_hidden(0..n, false);
                rows.set_hidden(list, true);
            }
            Self::SetColors(list) => {
                check_rows(&list.iter().map(|(r, _)| *r).collect::<Vec<_>>(), n)?;
                rows.set_color(0..n, None);
                for (r, c) in list {
                    rows.set_color([r], Some(c));
                }
            }
            Self::SetMarkers(list) => {
                check_rows(&list.iter().map(|(r, _)| *r).collect::<Vec<_>>(), n)?;
                rows.set_marker(0..n, None);
                for (r, m) in list {
                    rows.set_marker([r], Some(m));
                }
            }
            Self::ColorByColumn(column) => color_by_column(table, rows, &column)?,
            Self::SetStyleColumn(column) => {
                rows.set_style_column(column);
            }
            Self::ClearRowStyles => {
                rows.clear_row_styles();
            }
            Self::SetSubsets(subsets) => subsets.apply_to(rows)?,
            Self::AddReport(saved) => reports.insert(0, saved),
            Self::UpdateReport(i, saved) => match reports.get_mut(i) {
                Some(r) => *r = saved,
                None => return Err(invalid(format!("there is no report {i} (the table has {})", reports.len()))),
            },
            Self::RemoveReport(i) => {
                if i >= reports.len() {
                    return Err(invalid(format!("there is no report {i} (the table has {})", reports.len())));
                }
                reports.remove(i);
            }
        }
        Ok(())
    }

    /// The Rhai statements doing this to the table in variable `var`, given
    /// the table as it is before the action (one statement per line).
    #[must_use]
    pub fn script(&self, var: &str, table: &DataTable, rows: &RowStateManager, reports: &[SavedReport]) -> String {
        match self {
            Self::Edit(edit) => edit_script(var, edit, table, rows, reports),
            Self::Select(list) if list.is_empty() => format!("{var}.clear_selection();"),
            Self::Select(list) => format!("{var}.select_rows({});", row_list(list)),
            Self::SetExcluded(list) => {
                let now = rows_where(rows, |r| rows.is_excluded_flag(r));
                set_script(var, list, &now, "exclude_rows", "include_all", "set_excluded")
            }
            Self::SetHidden(list) => {
                let now = rows_where(rows, |r| rows.is_hidden_flag(r));
                set_script(var, list, &now, "hide_rows", "unhide_all", "set_hidden")
            }
            Self::SetColors(list) => {
                let mut lines = Vec::new();
                let uncolored: Vec<usize> =
                    rows_where(rows, |r| rows.color(r).is_some() && !list.iter().any(|(x, _)| *x == r));
                if !uncolored.is_empty() {
                    lines.push(format!("{var}.uncolor_rows({});", row_list(&uncolored)));
                }
                // Group the changed rows by their new colour, in first-seen order.
                let mut groups: Vec<(RowColor, Vec<usize>)> = Vec::new();
                for &(r, c) in list {
                    if rows.color(r) == Some(c) {
                        continue;
                    }
                    match groups.iter_mut().find(|(g, _)| *g == c) {
                        Some((_, members)) => members.push(r),
                        None => groups.push((c, vec![r])),
                    }
                }
                for (c, members) in groups {
                    lines.push(format!("{var}.color_rows({}, {});", row_list(&members), string(&color_text(c))));
                }
                lines.join("\n")
            }
            Self::SetMarkers(list) => {
                let mut lines = Vec::new();
                let unmarked: Vec<usize> =
                    rows_where(rows, |r| rows.marker(r).is_some() && !list.iter().any(|(x, _)| *x == r));
                if !unmarked.is_empty() {
                    lines.push(format!("{var}.unmark_rows({});", row_list(&unmarked)));
                }
                let mut groups: Vec<(RowMarker, Vec<usize>)> = Vec::new();
                for &(r, m) in list {
                    if rows.marker(r) == Some(m) {
                        continue;
                    }
                    match groups.iter_mut().find(|(g, _)| *g == m) {
                        Some((_, members)) => members.push(r),
                        None => groups.push((m, vec![r])),
                    }
                }
                for (m, members) in groups {
                    lines.push(format!("{var}.mark_rows({}, {});", row_list(&members), string(m.key())));
                }
                lines.join("\n")
            }
            Self::ColorByColumn(column) => format!("{var}.color_by_column({});", string(column)),
            Self::SetStyleColumn(Some(column)) => format!("{var}.set_style_column({});", string(column)),
            Self::SetStyleColumn(None) => format!("{var}.set_style_column(());"),
            Self::ClearRowStyles => format!("{var}.clear_row_colors();"),
            Self::SetSubsets(wanted) => {
                let now = RowSubsets::of(rows);
                let mut lines = Vec::new();
                for (name, _) in &now.subsets {
                    if !wanted.subsets.iter().any(|(n, _)| n == name) {
                        lines.push(format!("{var}.remove_subset({});", string(name)));
                    }
                }
                for (name, list) in &wanted.subsets {
                    if !now.subsets.iter().any(|(n, l)| n == name && l == list) {
                        lines.push(format!("{var}.save_subset({}, {});", string(name), row_list(list)));
                    }
                }
                if wanted.active != now.active {
                    lines.push(match &wanted.active {
                        Some(name) => format!("{var}.use_subset({});", string(name)),
                        None => format!("{var}.use_all_rows();"),
                    });
                }
                lines.join("\n")
            }
            Self::AddReport(saved) => analysis_call(var, saved, table),
            Self::UpdateReport(i, saved) => {
                format!("{var}.update_report({i}, {});", pretty(&report_to_json(saved, table), 0))
            }
            Self::RemoveReport(i) => {
                let what = reports.get(*i).map(|r| format!("  // {}", crate::report_run::analysis_key(r, table)));
                format!("{var}.close_report({i});{}", what.unwrap_or_default())
            }
        }
    }
}

/// A whole-set row-state change written as the smallest call: adding rows,
/// clearing all, or giving the full set.
fn set_script(var: &str, wanted: &[usize], now: &[usize], add: &str, clear: &str, set: &str) -> String {
    if wanted.is_empty() {
        return format!("{var}.{clear}();");
    }
    if now.iter().all(|r| wanted.contains(r)) {
        let added: Vec<usize> = wanted.iter().copied().filter(|r| !now.contains(r)).collect();
        return format!("{var}.{add}({});", row_list(&added));
    }
    format!("{var}.{set}({});", row_list(wanted))
}

/// A column as a script map: a formula column by its formula (the values
/// are computed when it is inserted), others with their values.
fn column_spec(column: &stat_data::Column) -> stat_data::JsonValue {
    let json = column_to_json(column);
    match (column.formula.is_some(), json) {
        (true, stat_data::JsonValue::Object(members)) => {
            stat_data::JsonValue::Object(members.into_iter().filter(|(k, _)| k != "values").collect())
        }
        (_, json) => json,
    }
}

fn edit_script(var: &str, edit: &Edit, table: &DataTable, rows: &RowStateManager, reports: &[SavedReport]) -> String {
    let name = |j: usize| string(table.column(j).map_or("", |c| c.name.as_str()));
    match edit {
        Edit::SetCell { row, column, value } => {
            format!("{var}.set_cell({row}, {}, {});", name(*column), cell_text(value))
        }
        Edit::InsertRows { rows: new } => {
            let empty = table.empty_row();
            let blank = new.iter().all(|(_, v, s)| *v == empty && *s == RowState::default());
            let first = new.first().map_or(0, |(i, _, _)| *i);
            let consecutive = new.iter().enumerate().all(|(k, (i, _, _))| *i == first + k);
            if blank && consecutive {
                format!("{var}.insert_rows({first}, {});", new.len())
            } else {
                let items: Vec<String> = new
                    .iter()
                    .map(|(i, v, _)| format!("[{i}, [{}]]", v.iter().map(cell_text).collect::<Vec<_>>().join(", ")))
                    .collect();
                format!("{var}.insert_rows([{}]);", items.join(", "))
            }
        }
        Edit::RemoveRows { rows: which } => format!("{var}.remove_rows({});", row_list(which)),
        Edit::InsertColumn { index, column } => {
            format!("{var}.insert_column({index}, {});", compact(&column_spec(column)))
        }
        Edit::RemoveColumn { index } => format!("{var}.remove_column({});", name(*index)),
        Edit::RenameColumn { index, name: new } => {
            format!("{var}.rename_column({}, {});", name(*index), string(new))
        }
        Edit::RenameTable { name: new } => format!("{var}.rename_table({});", string(new)),
        Edit::ReplaceColumn { index, column } => {
            format!("{var}.replace_column({}, {});", name(*index), compact(&column_to_json(column)))
        }
        Edit::ReorderRows { order } => format!("{var}.reorder_rows({});", row_list(order)),
        Edit::SetFormula { index, formula: Some(f) } => {
            format!("{var}.set_formula({}, {});", name(*index), string(f))
        }
        Edit::SetFormula { index, formula: None } => format!("{var}.clear_formula({});", name(*index)),
        Edit::SetLevel { index, level } => {
            format!("{var}.set_level({}, {});", name(*index), string(level_name(*level)))
        }
        Edit::SetValueLabels { index, labels } if labels.is_empty() => {
            format!("{var}.clear_value_labels({});", name(*index))
        }
        Edit::SetValueLabels { index, labels } => {
            let pairs: Vec<String> = labels.iter().map(|(c, l)| format!("{}: {}", string(c), string(l))).collect();
            format!("{var}.set_value_labels({}, #{{{}}});", name(*index), pairs.join(", "))
        }
        Edit::SetHover { index, hover } => format!("{var}.set_hover({}, {hover});", name(*index)),
        Edit::Batch { label, edits } => {
            // Each part is written against the table as the parts before it
            // leave it.
            let (mut table, mut rows, mut reports) = (table.clone(), rows.clone(), reports.to_vec());
            let mut lines = vec![format!("// {label}")];
            for e in edits {
                lines.push(edit_script(var, e, &table, &rows, &reports));
                if apply_edit(e.clone(), &mut table, &mut rows, &mut reports).is_err() {
                    break;
                }
            }
            lines.join("\n")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Subsets apply as a whole set, refuse bad sets unchanged, and write
    /// themselves as the smallest script calls.
    #[test]
    fn row_subsets_apply_and_write_their_script() {
        let table = DataTable::new("t", vec![stat_data::Column::numeric("x", vec![1.0, 2.0, 3.0, 4.0])]).unwrap();
        let mut rows = RowStateManager::new(4);
        let mut reports = Vec::new();
        let wanted = RowSubsets {
            subsets: vec![("Low".into(), vec![0, 1]), ("High".into(), vec![2, 3])],
            active: Some("High".into()),
        };
        let script = Action::SetSubsets(wanted.clone()).script("t", &table, &rows, &reports);
        assert_eq!(
            script,
            "t.save_subset(\"Low\", [0, 1]);\nt.save_subset(\"High\", [2, 3]);\nt.use_subset(\"High\");"
        );
        Action::SetSubsets(wanted.clone()).apply(&mut table.clone(), &mut rows, &mut reports).unwrap();
        assert_eq!(RowSubsets::of(&rows), wanted);
        assert!(rows.is_excluded(0) && !rows.is_excluded(2));
        let fewer = RowSubsets { subsets: vec![("High".into(), vec![2, 3])], active: None };
        let script = Action::SetSubsets(fewer.clone()).script("t", &table, &rows, &reports);
        assert_eq!(script, "t.remove_subset(\"Low\");\nt.use_all_rows();");
        for bad in [
            RowSubsets { subsets: vec![("A".into(), vec![9])], active: None },
            RowSubsets { subsets: vec![("A".into(), vec![0]), ("A".into(), vec![1])], active: None },
            RowSubsets { subsets: vec![(" ".into(), vec![0])], active: None },
            RowSubsets { subsets: Vec::new(), active: Some("B".into()) },
        ] {
            assert!(Action::SetSubsets(bad).apply(&mut table.clone(), &mut rows, &mut reports).is_err());
            assert_eq!(RowSubsets::of(&rows), wanted, "nothing changes on error");
        }
    }
}
