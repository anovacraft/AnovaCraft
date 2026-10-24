//! Recording what a person does in the application as a Rhai script that
//! replays it.
//!
//! The [`Recorder`] keeps, for every table it follows, the state a script
//! would have reached by running the lines written so far (the *replay
//! state*). The application tells it about data edits as they happen and
//! shows it each table's current row states and reports; it writes an
//! [`Action`] for every difference and applies the action to the replay
//! state, using the same code scripts use. So whatever the application did,
//! the script ends in the same place: same data, row states and reports.

use stat_data::{DataTable, RowColor, RowMarker, RowState, RowStateManager};

use crate::action::{Action, apply_edit};
use crate::editing::Edit;
use crate::project::{SavedReport, column_to_json, report_to_json};
use crate::script_text::{compact, pretty, string};
use crate::table_ops::TableOrigin;

/// How a table being recorded was opened.
#[derive(Debug, Clone, PartialEq)]
pub enum OpenedFrom {
    /// A data file (`sheet`: one sheet of a workbook, by name).
    File {
        /// Path as given to the application.
        path: String,
        /// Sheet name, for a workbook.
        sheet: Option<String>,
        /// The Open Data dialog's choices, as the map `open_table` takes
        /// (see [`crate::files::open_path_with`]); `None` when the detected
        /// settings were used.
        options: Option<stat_data::JsonValue>,
    },
    /// A project file.
    Project(String),
    /// A built-in sample, by its menu name.
    Sample(String),
    /// A table operation on recorded tables (`sources` by key, first the
    /// main one).
    Derived {
        /// The operation and its settings.
        origin: TableOrigin,
        /// Keys of the source tables.
        sources: Vec<u64>,
    },
    /// Data made in the application (a new table, a design, pasted data):
    /// written into the script value by value.
    Data,
}

/// A table the recorder follows: its script variable and replay state.
#[derive(Debug)]
struct Tracked {
    key: u64,
    var: String,
    table: DataTable,
    rows: RowStateManager,
    reports: Vec<SavedReport>,
    /// Report identities (the application's ids), aligned with `reports`.
    report_ids: Vec<u64>,
    closed: bool,
}

/// Writes a script while the person works. See the module documentation.
#[derive(Debug, Default)]
pub struct Recorder {
    lines: Vec<String>,
    tracked: Vec<Tracked>,
    /// Number of `let` variables written.
    variables: usize,
}

fn states(rows: &RowStateManager) -> Vec<RowState> {
    (0..rows.len()).map(|r| rows.state(r)).collect()
}

fn where_rows(rows: &RowStateManager, test: impl Fn(&RowState) -> bool) -> Vec<usize> {
    (0..rows.len()).filter(|&r| test(&rows.state(r))).collect()
}

fn colors(rows: &RowStateManager) -> Vec<(usize, RowColor)> {
    (0..rows.len()).filter_map(|r| rows.color(r).map(|c| (r, c))).collect()
}

fn markers(rows: &RowStateManager) -> Vec<(usize, RowMarker)> {
    (0..rows.len()).filter_map(|r| rows.marker(r).map(|m| (r, m))).collect()
}

/// The actions bringing the replay state's row colours and markers to
/// `rows`: one colour-by-column call or clear when that reproduces them
/// exactly, otherwise the whole sets (and the key column when it differs).
fn style_actions(table: &DataTable, replay: &RowStateManager, rows: &RowStateManager) -> Vec<Action> {
    let (want_colors, want_markers) = (colors(rows), markers(rows));
    if colors(replay) == want_colors && markers(replay) == want_markers && replay.style_column() == rows.style_column()
    {
        return Vec::new();
    }
    if !rows.has_row_styles() && rows.style_column().is_none() {
        return vec![Action::ClearRowStyles];
    }
    if let Some(column) = rows.style_column() {
        let mut trial = replay.clone();
        if crate::action::color_by_column(table, &mut trial, column).is_ok()
            && colors(&trial) == want_colors
            && markers(&trial) == want_markers
        {
            return vec![Action::ColorByColumn(column.to_owned())];
        }
    }
    let mut actions = Vec::new();
    if colors(replay) != want_colors {
        actions.push(Action::SetColors(want_colors));
    }
    if markers(replay) != want_markers {
        actions.push(Action::SetMarkers(want_markers));
    }
    if replay.style_column() != rows.style_column() {
        actions.push(Action::SetStyleColumn(rows.style_column().map(str::to_owned)));
    }
    actions
}

/// Reports as the same request when their saved forms match (by value, or
/// failing that by their project-file text, which treats NaN settings as
/// equal).
fn same_report(a: &SavedReport, b: &SavedReport, table: &DataTable) -> bool {
    a == b || report_to_json(a, table).to_compact() == report_to_json(b, table).to_compact()
}

impl Recorder {
    /// An empty recording.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn variable(&mut self) -> String {
        self.variables += 1;
        format!("t{}", self.variables)
    }

    fn find(&mut self, key: u64) -> Option<&mut Tracked> {
        self.tracked.iter_mut().find(|t| t.key == key && !t.closed)
    }

    /// Whether the table with `key` is being followed.
    #[must_use]
    pub fn follows(&self, key: u64) -> bool {
        self.tracked.iter().any(|t| t.key == key && !t.closed)
    }

    /// Follow a table that was already open when recording started.
    /// `position` is its tab position and `active` whether it was in front;
    /// `unique_name` whether no other open table has its name. The script
    /// finds it as the current table, by name, or by position.
    #[allow(clippy::too_many_arguments)]
    pub fn existing(
        &mut self,
        key: u64,
        position: usize,
        active: bool,
        unique_name: bool,
        table: &DataTable,
        rows: &RowStateManager,
        reports: &[(u64, SavedReport)],
    ) {
        let var = self.variable();
        let find = if active {
            "current_table()".to_owned()
        } else if unique_name {
            format!("table({})", string(table.name()))
        } else {
            format!("open_tables()[{position}]")
        };
        self.lines.push(format!("let {var} = {find};  // {}", table.name()));
        self.tracked.push(Tracked {
            key,
            var,
            table: table.clone(),
            rows: rows.clone(),
            reports: reports.iter().map(|(_, r)| r.clone()).collect(),
            report_ids: reports.iter().map(|(id, _)| *id).collect(),
            closed: false,
        });
    }

    /// Follow a table just opened or made. `table`, `rows` and `reports`
    /// are what the application opened; the recording then catches up with
    /// any difference from what the script's own call gives (for example a
    /// starting report the application adds to a data file).
    pub fn opened(
        &mut self,
        key: u64,
        from: OpenedFrom,
        table: &DataTable,
        rows: &RowStateManager,
        reports: &[(u64, SavedReport)],
    ) {
        let var = self.variable();
        let fresh = RowStateManager::new(table.n_rows());
        // What the script's call gives: (row states, reports with ids).
        let (open, replay_rows, replay_reports): (Vec<String>, RowStateManager, Vec<(u64, SavedReport)>) = match &from {
            OpenedFrom::File { path, options: Some(options), .. } => {
                (vec![format!("let {var} = open_table({}, {});", string(path), compact(options))], fresh, Vec::new())
            }
            OpenedFrom::File { path, sheet: None, options: None } => {
                (vec![format!("let {var} = open_table({});", string(path))], fresh, Vec::new())
            }
            OpenedFrom::File { path, sheet: Some(sheet), options: None } => {
                (vec![format!("let {var} = open_table({}, {});", string(path), string(sheet))], fresh, Vec::new())
            }
            OpenedFrom::Project(path) => {
                (vec![format!("let {var} = open_project({});", string(path))], rows.clone(), reports.to_vec())
            }
            OpenedFrom::Sample(name) => {
                (vec![format!("let {var} = open_sample({});", string(name))], fresh, reports.to_vec())
            }
            OpenedFrom::Derived { origin, sources } => {
                let vars: Vec<String> = sources
                    .iter()
                    .map(|k| self.tracked.iter().find(|t| t.key == *k).map_or("()".to_owned(), |t| t.var.clone()))
                    .collect();
                let mut request = origin.to_json();
                if let stat_data::JsonValue::Object(members) = &mut request {
                    members.retain(|(k, _)| k != "sources");
                    members.push(("name".to_owned(), stat_data::JsonValue::String(table.name().to_owned())));
                }
                let main = vars.first().cloned().unwrap_or_else(|| "()".to_owned());
                let call = if vars.len() > 1 {
                    format!("{main}.derive({}, [{}])", pretty(&request, 0), vars[1..].join(", "))
                } else {
                    format!("{main}.derive({})", pretty(&request, 0))
                };
                (vec![format!("let {var} = {call};")], rows.clone(), Vec::new())
            }
            OpenedFrom::Data => {
                let mut lines = vec![format!("let {var} = new_table({});", string(table.name()))];
                for (j, c) in table.columns().iter().enumerate() {
                    lines.push(format!("{var}.insert_column({j}, {});", compact(&column_to_json(c))));
                }
                (lines, fresh, Vec::new())
            }
        };
        self.lines.extend(open);
        self.lines.push(format!("{var}.show();"));
        self.tracked.push(Tracked {
            key,
            var,
            table: table.clone(),
            rows: replay_rows,
            reports: replay_reports.iter().map(|(_, r)| r.clone()).collect(),
            report_ids: replay_reports.iter().map(|(id, _)| *id).collect(),
            closed: false,
        });
        self.sync(key, table, rows, reports);
    }

    /// Data edits the application applied to table `key`, in order (as
    /// logged by [`crate::editing::EditHistory`]).
    pub fn edits(&mut self, key: u64, edits: Vec<Edit>) {
        let mut flat = Vec::new();
        for edit in edits {
            flatten(edit, &mut flat);
        }
        for edit in flat {
            // Inserted rows arrive in the script without row states; the
            // states are written afterwards like any other row-state change.
            let edit = match edit {
                Edit::InsertRows { rows } => {
                    Edit::InsertRows { rows: rows.into_iter().map(|(i, v, _)| (i, v, RowState::default())).collect() }
                }
                e => e,
            };
            self.act(key, Action::Edit(edit));
        }
    }

    /// Write `action` for table `key` and apply it to the replay state.
    /// A report added this way gets a placeholder id, which the caller
    /// replaces.
    fn act(&mut self, key: u64, action: Action) {
        let Some(t) = self.find(key) else { return };
        let line = action.script(&t.var, &t.table, &t.rows, &t.reports);
        let ids = match &action {
            Action::AddReport(_) => Some((0, true)),
            Action::RemoveReport(i) => Some((*i, false)),
            _ => None,
        };
        let applied = match &action {
            Action::Edit(edit) => apply_edit(edit.clone(), &mut t.table, &mut t.rows, &mut t.reports).map(|_| ()),
            _ => action.apply(&mut t.table, &mut t.rows, &mut t.reports),
        };
        match applied {
            Ok(()) => {
                match ids {
                    Some((_, true)) => t.report_ids.insert(0, u64::MAX),
                    Some((i, false)) => {
                        t.report_ids.remove(i);
                    }
                    None => {}
                }
                self.lines.push(line);
            }
            Err(e) => self.lines.push(format!("// Could not record a step: {e}")),
        }
    }

    /// Compare table `key` as the application now has it with the replay
    /// state, and write what differs: row states, then reports (by id).
    /// Call after every frame or command.
    pub fn sync(&mut self, key: u64, table: &DataTable, rows: &RowStateManager, reports: &[(u64, SavedReport)]) {
        let Some(t) = self.find(key) else { return };
        if t.table != *table {
            // Every change goes through the edit log, so this means an edit
            // reached the table some other way: carry the data across
            // column by column so the replay stays exact.
            let var = t.var.clone();
            match data_catch_up(&t.table, table) {
                Some(edits) => {
                    self.lines.push(format!("// {var}: data changed outside the recorded edits; copied across"));
                    for edit in edits {
                        self.act(key, Action::Edit(edit));
                    }
                }
                None => {
                    self.lines.push(format!("// {var}: the data changed in a way this recording could not follow"));
                    if let Some(t) = self.find(key) {
                        t.table = table.clone();
                        t.rows = rows.clone();
                    }
                }
            }
        }

        // Row states: whole sets, each only when it differs.
        let Some(t) = self.find(key) else { return };
        let mut actions = Vec::new();
        let styled = t.rows.style_column() != rows.style_column();
        if t.rows.len() == rows.len() && (styled || states(&t.rows) != states(rows)) {
            let pick = |rows: &RowStateManager, f: &dyn Fn(&RowState) -> bool| where_rows(rows, f);
            let selected = pick(rows, &|s| s.selected);
            if pick(&t.rows, &|s| s.selected) != selected {
                actions.push(Action::Select(selected));
            }
            let excluded = pick(rows, &|s| s.excluded);
            if pick(&t.rows, &|s| s.excluded) != excluded {
                actions.push(Action::SetExcluded(excluded));
            }
            let hidden = pick(rows, &|s| s.hidden);
            if pick(&t.rows, &|s| s.hidden) != hidden {
                actions.push(Action::SetHidden(hidden));
            }
            actions.extend(style_actions(&t.table, &t.rows, rows));
        }
        let wanted = crate::action::RowSubsets::of(rows);
        if t.rows.len() == rows.len() && crate::action::RowSubsets::of(&t.rows) != wanted {
            actions.push(Action::SetSubsets(wanted));
        }
        for action in actions {
            self.act(key, action);
        }

        // Reports: closed ones (from the bottom, so positions stay valid),
        // then new ones (each added at the top, the oldest first), then
        // changed ones.
        let ids: Vec<u64> = reports.iter().map(|(id, _)| *id).collect();
        let Some(t) = self.find(key) else { return };
        let gone: Vec<usize> = (0..t.report_ids.len()).rev().filter(|&i| !ids.contains(&t.report_ids[i])).collect();
        for i in gone {
            self.act(key, Action::RemoveReport(i));
        }
        for (id, saved) in reports.iter().rev() {
            if self.find(key).is_some_and(|t| !t.report_ids.contains(id)) {
                self.act(key, Action::AddReport(saved.clone()));
                if let Some(t) = self.find(key) {
                    t.report_ids[0] = *id;
                }
            }
        }
        let Some(t) = self.find(key) else { return };
        let changes: Vec<Action> = t
            .report_ids
            .iter()
            .enumerate()
            .filter_map(|(i, id)| {
                let (_, now) = reports.iter().find(|(r, _)| r == id)?;
                (!same_report(&t.reports[i], now, table)).then(|| Action::UpdateReport(i, now.clone()))
            })
            .collect();
        if t.report_ids != ids {
            let var = t.var.clone();
            self.lines.push(format!("// {var}: the application lists these reports in a different order"));
        }
        for action in changes {
            self.act(key, action);
        }
    }

    /// Table `key` was saved to `path` (a project when it ends in
    /// `.acraft`, else exported).
    pub fn saved(&mut self, key: u64, path: &str) {
        if let Some(t) = self.find(key) {
            let line = format!("{}.save({});", t.var, string(path));
            self.lines.push(line);
        }
    }

    /// Table `key` was closed: later lines no longer use it.
    pub fn closed(&mut self, key: u64) {
        if let Some(t) = self.find(key) {
            t.closed = true;
            let line = format!("// {} (\"{}\") closed", t.var, t.table.name());
            self.lines.push(line);
        }
    }

    /// Add a comment line.
    pub fn comment(&mut self, text: &str) {
        for line in text.lines() {
            self.lines.push(format!("// {line}"));
        }
    }

    /// The script recorded so far.
    #[must_use]
    pub fn script(&self) -> String {
        let mut out =
            String::from("// Recorded actions. Run in the Script window with the same tables open to repeat them.\n");
        for line in &self.lines {
            out.push_str(line);
            out.push('\n');
        }
        out
    }

    /// Number of lines recorded (comments included).
    #[must_use]
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// Whether nothing has been recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}

fn flatten(edit: Edit, out: &mut Vec<Edit>) {
    match edit {
        Edit::Batch { edits, .. } => edits.into_iter().for_each(|e| flatten(e, out)),
        e => out.push(e),
    }
}

/// Edits taking `from` to `to` when they have the same shape (each
/// differing column replaced, or renamed when only its name differs).
fn data_catch_up(from: &DataTable, to: &DataTable) -> Option<Vec<Edit>> {
    if from.n_rows() != to.n_rows() || from.n_cols() != to.n_cols() {
        return None;
    }
    let mut edits = Vec::new();
    for (j, (a, b)) in from.columns().iter().zip(to.columns()).enumerate() {
        if a.name != b.name {
            edits.push(Edit::RenameColumn { index: j, name: b.name.clone() });
        }
        if a.data != b.data || a.level != b.level || a.formula != b.formula {
            edits.push(Edit::ReplaceColumn { index: j, column: b.clone() });
        }
    }
    Some(edits)
}
