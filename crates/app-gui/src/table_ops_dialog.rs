//! Dialogs for the Data menu's table operations (Rows, Reshape, Merge and
//! Summarize by group). One dialog type serves all nine operations; each shows
//! the options for its operation and a preview of the result (its size, any
//! notes, and the first rows) before anything changes.
//!
//! Sorting and removing duplicates change the current table as one undoable
//! edit unless "Put the result in a new table" is ticked; every other
//! operation makes a new table. The new table joins the current table's
//! project (listed under it, and updatable when it changes) unless "Keep in
//! this project" is cleared, which opens it as a separate, unsaved document.
//! It gets a short name saying what it holds ("Summary by Fertilizer").

use crate::dialog_kit::ComboExt as _;
use app_model::editing::Edit;
use app_model::DistanceMetric;
use app_model::table_ops::{self, DerivedDocument, SampleSpec, Source, SubsetRows, TableOperation, unique_name};
use eframe::egui::{self, RichText};
use stat_core::descriptive::QuantileMethod;
use stat_data::DataTable;
use stat_data::table_ops::{JoinKind, Relationship, SampleSize, Statistic};
use stat_viz::FilterCondition;

use crate::design_dialog::sentence_fragment;
use crate::dialog_kit::{self as kit, Dialog, Outcome, Readiness, Remember};

/// Rows shown in the preview.
const PREVIEW_ROWS: usize = 20;

/// Which operation a dialog is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableOpKind {
    Sort,
    RemoveDuplicates,
    Subset,
    CombineRows,
    Join,
    Stack,
    Unstack,
    Transpose,
    Summarize,
    DistanceMatrix,
}

impl TableOpKind {
    /// Window title.
    fn title(self) -> &'static str {
        match self {
            Self::Sort => "Sort rows",
            Self::RemoveDuplicates => "Remove duplicate rows",
            Self::Subset => "Subset and sample",
            Self::CombineRows => "Combine rows",
            Self::Join => "Join tables",
            Self::Stack => "Stack columns",
            Self::Unstack => "Unstack by label",
            Self::Transpose => "Transpose",
            Self::Summarize => "Summarize by group",
            Self::DistanceMatrix => "Distance matrix",
        }
    }

    /// One line under the title saying what the operation does.
    fn blurb(self) -> &'static str {
        match self {
            Self::Sort => "Order rows by one or more columns. Missing values go last; ties keep their order.",
            Self::RemoveDuplicates => "Keep the first of each set of rows that agree on the chosen columns.",
            Self::Subset => "Copy chosen rows and columns, optionally as a random sample, to a new table.",
            Self::CombineRows => "Append other open tables' rows, matching columns by name.",
            Self::Join => "Match rows of another open table by key columns and put them side by side.",
            Self::Stack => "Wide to long: chosen columns become a label column and a value column.",
            Self::Unstack => "Long to wide: one new column per level of a label column.",
            Self::Transpose => "Rows become columns and columns become rows.",
            Self::Summarize => "Statistics of columns for each combination of grouping columns.",
            Self::DistanceMatrix => {
                "The distance between every pair of rows, one column per row, for principal coordinates, \
                 scaling or clustering."
            }
        }
    }
}

/// What the dialog asks the application to do when its action is chosen.
pub enum TableOpAction {
    /// Apply this edit to the active document, then show the message.
    Edit(Edit, String),
    /// Nothing to change; show the message.
    Info(String),
    /// Open the result as a new table: in the current table's project
    /// (`true`) or as a separate document.
    Derived(Box<DerivedDocument>, bool),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RowsChoice {
    All,
    Selected,
    Included,
    Matching,
}

const STATISTICS: [(Statistic, &str); 8] = [
    (Statistic::Count, "N (values)"),
    (Statistic::Missing, "Missing"),
    (Statistic::Mean, "Mean"),
    (Statistic::StdDev, "Standard deviation"),
    (Statistic::Min, "Minimum"),
    (Statistic::Max, "Maximum"),
    (Statistic::Median, "Median"),
    (Statistic::Sum, "Sum"),
];

const QUANTILE_METHODS: [(QuantileMethod, &str); 9] = [
    (QuantileMethod::MedianUnbiased, "Type 8 — median-unbiased (recommended)"),
    (QuantileMethod::NormalUnbiased, "Type 9 — unbiased for normal data"),
    (QuantileMethod::Linear, "Type 7 — linear, 1 + p(n − 1)"),
    (QuantileMethod::Weibull, "Type 6 — p(n + 1)"),
    (QuantileMethod::Midpoint, "Type 5 — midpoint knots"),
    (QuantileMethod::LinearCdf, "Type 4 — linear empirical CDF"),
    (QuantileMethod::NearestEven, "Type 3 — nearest even order statistic"),
    (QuantileMethod::AveragedInverseCdf, "Type 2 — inverse CDF, averaged"),
    (QuantileMethod::InverseCdf, "Type 1 — inverse empirical CDF"),
];

/// A dialog for one table operation on the active document.
#[derive(Clone)]
pub struct TableOpDialog {
    kind: TableOpKind,
    /// Main column choice (columns to keep, stack, transpose, summarize,
    /// compare for duplicates, or unstack's value columns).
    columns: Vec<bool>,
    /// Second column choice (unstack's ID columns, summarize's groups).
    columns2: Vec<bool>,
    /// Sort keys: column and descending.
    keys: Vec<(Option<usize>, bool)>,
    new_table: bool,
    /// Put a new table in the current table's project.
    keep_in_project: bool,
    rows: RowsChoice,
    conditions: Vec<FilterCondition>,
    sample: bool,
    by_count: bool,
    count: usize,
    percent: f64,
    strata: Vec<bool>,
    seed: u64,
    /// Other open documents chosen to combine with (by document position).
    others: Vec<bool>,
    source_column: bool,
    source_name: String,
    /// Document to join with, and key pairs (this table, other table).
    other: Option<usize>,
    join_keys: Vec<(Option<usize>, Option<usize>)>,
    join_kind: JoinKind,
    relationship: Relationship,
    label_name: String,
    value_name: String,
    /// Unstack's label column or transpose's column of names.
    label: Option<usize>,
    statistics: [bool; 8],
    quantiles: String,
    method: QuantileMethod,
    include_excluded: bool,
    /// Distance matrix: the measure and whether to standardize first.
    metric: DistanceMetric,
    standardize: bool,
    /// Key of the request the preview was computed for, and the preview.
    preview_key: String,
    preview: Option<Result<DerivedDocument, String>>,
}

impl TableOpDialog {
    /// A dialog for `kind` on `table`, with sensible starting choices.
    pub fn new(kind: TableOpKind, table: &DataTable, n_documents: usize, active: usize) -> Self {
        let n = table.n_cols();
        let all = vec![true; n];
        let none = vec![false; n];
        let columns = match kind {
            TableOpKind::Subset | TableOpKind::RemoveDuplicates => all,
            _ => none.clone(),
        };
        Self {
            kind,
            columns,
            columns2: none.clone(),
            keys: vec![(None, false)],
            new_table: false,
            keep_in_project: true,
            rows: RowsChoice::All,
            conditions: Vec::new(),
            sample: false,
            by_count: false,
            count: (table.n_rows() / 10).max(1),
            percent: 10.0,
            strata: none,
            seed: 1,
            others: (0..n_documents).map(|_| false).collect(),
            source_column: true,
            source_name: "Source table".to_owned(),
            other: (0..n_documents).find(|&i| i != active),
            join_keys: vec![(None, None)],
            join_kind: JoinKind::Left,
            relationship: Relationship::Any,
            label_name: "Label".to_owned(),
            value_name: "Value".to_owned(),
            label: None,
            statistics: [true, true, true, true, true, true, false, false],
            quantiles: String::new(),
            method: QuantileMethod::default(),
            include_excluded: false,
            metric: DistanceMetric::Euclidean,
            standardize: false,
            preview_key: String::new(),
            preview: None,
        }
    }

    /// The operation as chosen so far, with the documents it also reads, or
    /// what is still missing.
    fn request(
        &self,
        table: &DataTable,
        docs: &[Source<'_>],
        active: usize,
    ) -> Result<(TableOperation, Vec<usize>), String> {
        let name = |i: usize| table.column(i).map(|c| c.name.clone()).unwrap_or_default();
        let chosen = |v: &[bool]| (0..v.len()).filter(|&i| v[i]).map(name).collect::<Vec<_>>();
        let need =
            |v: &[String], what: &str| if v.is_empty() { Err(format!("Choose at least one {what}")) } else { Ok(()) };
        let op = match self.kind {
            TableOpKind::Sort => {
                let keys: Vec<(String, bool)> =
                    self.keys.iter().filter_map(|(c, d)| c.map(|c| (name(c), *d))).collect();
                if keys.is_empty() {
                    return Err("Choose a column to sort by".into());
                }
                TableOperation::Sort { keys }
            }
            TableOpKind::RemoveDuplicates => TableOperation::RemoveDuplicates { columns: chosen(&self.columns) },
            TableOpKind::Subset => {
                let columns = chosen(&self.columns);
                need(&columns, "column to keep")?;
                let rows = match self.rows {
                    RowsChoice::All => SubsetRows::All,
                    RowsChoice::Selected => SubsetRows::Selected,
                    RowsChoice::Included => SubsetRows::Included,
                    RowsChoice::Matching => SubsetRows::Matching(self.conditions.clone()),
                };
                let sample = self.sample.then(|| SampleSpec {
                    size: if self.by_count {
                        SampleSize::Count(self.count)
                    } else {
                        SampleSize::Fraction(self.percent / 100.0)
                    },
                    strata: chosen(&self.strata),
                    seed: self.seed,
                });
                TableOperation::Subset { rows, columns, sample }
            }
            TableOpKind::CombineRows => {
                let others: Vec<usize> = (0..self.others.len()).filter(|&i| self.others[i] && i != active).collect();
                if others.is_empty() {
                    return Err("Choose at least one other open table".into());
                }
                let source_column =
                    self.source_column.then(|| self.source_name.trim().to_owned()).filter(|s| !s.is_empty());
                return Ok((TableOperation::CombineRows { source_column }, others));
            }
            TableOpKind::Join => {
                let other =
                    self.other.filter(|&o| o != active && o < docs.len()).ok_or("Choose a table to join with")?;
                let right = docs[other].table;
                let keys: Vec<(String, String)> = self
                    .join_keys
                    .iter()
                    .filter_map(|(a, b)| Some((name(a.as_ref().copied()?), right.column((*b)?)?.name.clone())))
                    .collect();
                if keys.is_empty() {
                    return Err("Choose a pair of key columns".into());
                }
                return Ok((
                    TableOperation::Join { keys, kind: self.join_kind, relationship: self.relationship },
                    vec![other],
                ));
            }
            TableOpKind::Stack => {
                let columns = chosen(&self.columns);
                need(&columns, "column to stack")?;
                TableOperation::Stack { columns, label: self.label_name.clone(), value: self.value_name.clone() }
            }
            TableOpKind::Unstack => {
                let label = self.label.map(name).ok_or("Choose the label column")?;
                let values = chosen(&self.columns);
                need(&values, "value column")?;
                TableOperation::Unstack { label, values, ids: chosen(&self.columns2) }
            }
            TableOpKind::Transpose => {
                let columns = chosen(&self.columns);
                need(&columns, "column to transpose")?;
                TableOperation::Transpose { columns, label: self.label.map(name) }
            }
            TableOpKind::Summarize => {
                let columns = chosen(&self.columns);
                need(&columns, "column to summarize")?;
                let mut statistics: Vec<Statistic> =
                    STATISTICS.iter().zip(self.statistics).filter(|(_, on)| *on).map(|((s, _), _)| *s).collect();
                for part in self.quantiles.split([',', ' ', ';']).filter(|p| !p.trim().is_empty()) {
                    let pct: f64 = part
                        .trim()
                        .trim_end_matches('%')
                        .parse()
                        .map_err(|_| format!("“{part}” is not a percentage"))?;
                    if !(0.0..=100.0).contains(&pct) {
                        return Err(format!("Quantiles are percentages from 0 to 100 (got {pct})"));
                    }
                    statistics.push(Statistic::Quantile(pct / 100.0));
                }
                if statistics.is_empty() {
                    return Err("Choose at least one statistic".into());
                }
                TableOperation::Summarize {
                    groups: chosen(&self.columns2),
                    columns,
                    statistics,
                    quantile_method: self.method,
                    include_excluded: self.include_excluded,
                }
            }
            TableOpKind::DistanceMatrix => {
                let columns = chosen(&self.columns);
                need(&columns, "column to compare")?;
                let standardize =
                    self.standardize && matches!(self.metric, DistanceMetric::Euclidean | DistanceMetric::Manhattan);
                TableOperation::DistanceMatrix { columns, metric: self.metric, standardize, label: self.label.map(name) }
            }
        };
        Ok((op, Vec::new()))
    }

    /// Whether the operation changes the current table rather than making a
    /// new one (sorting and removing duplicates, unless asked for a new table).
    fn in_place(&self) -> bool {
        matches!(self.kind, TableOpKind::Sort | TableOpKind::RemoveDuplicates) && !self.new_table
    }

    /// Show the dialog. `docs` are all open documents; `active` is the one the
    /// operation starts from.
    pub fn show(&mut self, ctx: &egui::Context, docs: &[Source<'_>], active: usize) -> Outcome<TableOpAction> {
        let Some(primary) = docs.get(active).copied() else { return Outcome::Cancel };
        let table = primary.table;
        for v in [&mut self.columns, &mut self.columns2, &mut self.strata] {
            v.resize(table.n_cols(), false);
        }
        self.others.resize(docs.len(), false);
        let in_place = self.in_place();
        let mut dialog = Dialog::new(self.kind.title()).creates().default_width(560.0);
        match self.kind {
            TableOpKind::Sort if in_place => dialog = dialog.action("Sort"),
            TableOpKind::RemoveDuplicates if in_place => dialog = dialog.action("Remove duplicates"),
            _ => {}
        }
        match dialog.show(ctx, |ui| self.contents(ui, docs, active)) {
            Outcome::Reset => {
                *self = Self::new(self.kind, table, docs.len(), active);
                Outcome::Open
            }
            Outcome::Run((op, others), place) => match self.finish(table, in_place, op, &others, docs, active) {
                Some(action) => Outcome::Run(action, place),
                None => Outcome::Open,
            },
            Outcome::Open => Outcome::Open,
            Outcome::Cancel => Outcome::Cancel,
        }
    }

    /// The options and preview; ready with the operation (and the other
    /// documents it reads), summarised as the table it will make or change.
    fn contents(
        &mut self,
        ui: &mut egui::Ui,
        docs: &[Source<'_>],
        active: usize,
    ) -> Readiness<(TableOperation, Vec<usize>)> {
        let table = docs[active].table;
        kit::note(ui, self.kind.blurb());
        ui.add_space(4.0);
        self.options(ui, docs, active);
        if !self.in_place() {
            kit::keep_in_project(ui, &mut self.keep_in_project);
        }
        ui.separator();
        let request = self.request(table, docs, active);
        self.preview_section(ui, docs, active, &request);
        let (op, others) = request?;
        let result = match &self.preview {
            Some(Ok(doc)) => &doc.table,
            Some(Err(e)) => return Err(sentence_fragment(e)),
            None => return Err("Choose the operation's settings".into()),
        };
        let summary = match &op {
            TableOperation::Sort { keys } if self.in_place() => {
                let by: Vec<String> = keys
                    .iter()
                    .map(|(c, descending)| if *descending { format!("{c} (descending)") } else { c.clone() })
                    .collect();
                format!("Sort “{}” by {}", table.name(), by.join(", "))
            }
            TableOperation::RemoveDuplicates { .. } if self.in_place() => {
                match table.n_rows().saturating_sub(result.n_rows()) {
                    0 => format!("No duplicate rows in “{}”", table.name()),
                    n => format!("Remove {} from “{}”", kit::count(n, "duplicate row", "duplicate rows"), table.name()),
                }
            }
            _ => {
                let project = kit::project_name(ui.ctx());
                let place =
                    if self.keep_in_project && !project.is_empty() { format!(" in {project}") } else { String::new() };
                format!(
                    "New table “{}”{place}: {} × {}",
                    result_name(&op, &others, docs),
                    kit::count(result.n_rows(), "row", "rows"),
                    kit::count(result.n_cols(), "column", "columns")
                )
            }
        };
        kit::ready((op, others), summary)
    }

    fn finish(
        &mut self,
        table: &DataTable,
        in_place: bool,
        op: TableOperation,
        others: &[usize],
        docs: &[Source<'_>],
        active: usize,
    ) -> Option<TableOpAction> {
        if in_place {
            return match &op {
                TableOperation::Sort { keys } => Some(match table_ops::sort_edit(table, keys) {
                    Ok(edit) => {
                        let by = keys.iter().map(|k| format!("“{}”", k.0)).collect::<Vec<_>>().join(", ");
                        TableOpAction::Edit(edit, format!("Sorted by {by}. Undo restores the original order."))
                    }
                    Err(e) => TableOpAction::Info(e.to_string()),
                }),
                TableOperation::RemoveDuplicates { columns } => {
                    Some(match table_ops::remove_duplicates_edit(table, columns) {
                        Ok(Some(edit)) => {
                            let n = match &edit {
                                Edit::RemoveRows { rows } => rows.len(),
                                _ => 0,
                            };
                            TableOpAction::Edit(edit, format!("Removed {n} duplicate row(s). Undo restores them."))
                        }
                        Ok(None) => TableOpAction::Info("No duplicate rows were found; nothing changed.".into()),
                        Err(e) => TableOpAction::Info(e.to_string()),
                    })
                }
                _ => None,
            };
        }
        let sources: Vec<Source<'_>> = others.iter().filter_map(|&i| docs.get(i).copied()).collect();
        let name = result_name(&op, others, docs);
        match op.run(docs[active], &sources, &name) {
            Ok(doc) => Some(TableOpAction::Derived(Box::new(doc), self.keep_in_project)),
            Err(e) => {
                self.preview = Some(Err(e.to_string()));
                None
            }
        }
    }

    fn preview_section(
        &mut self,
        ui: &mut egui::Ui,
        docs: &[Source<'_>],
        active: usize,
        request: &Result<(TableOperation, Vec<usize>), String>,
    ) {
        // Recompute only when the request changes (or the data does).
        let revisions: Vec<(u64, u64)> = docs.iter().map(|d| (d.table.revision(), d.rows.revision())).collect();
        let key = format!("{request:?}{revisions:?}");
        if key != self.preview_key {
            self.preview_key = key;
            self.preview = Some(match request {
                Err(missing) => Err(missing.clone()),
                Ok((op, others)) => {
                    let sources: Vec<Source<'_>> = others.iter().filter_map(|&i| docs.get(i).copied()).collect();
                    op.run(docs[active], &sources, "preview").map_err(|e| e.to_string())
                }
            });
        }
        ui.label(RichText::new("Preview").strong());
        // What is missing, or why the operation fails, is the note under
        // the settings; its size is the summary.
        match &self.preview {
            None | Some(Err(_)) => {
                ui.label(RichText::new("Nothing to preview yet").weak().small());
            }
            Some(Ok(doc)) => {
                let t = &doc.table;
                let shown = t.n_rows().min(PREVIEW_ROWS);
                if shown < t.n_rows() {
                    ui.label(RichText::new(format!("First {shown} rows")).weak().small());
                }
                for note in &doc.notes {
                    kit::note(ui, note);
                }
                egui::ScrollArea::both().id_salt("table_op_preview").max_height(kit::em(ui) * 15.0).show(ui, |ui| {
                    egui::Grid::new("table_op_preview_grid").striped(true).spacing([14.0, 2.0]).show(ui, |ui| {
                        for c in t.columns() {
                            ui.label(RichText::new(&c.name).strong());
                        }
                        ui.end_row();
                        for r in 0..shown {
                            for c in t.columns() {
                                let text = c.cell_text(r);
                                if let (Some(values), None) = (c.as_numeric(), c.temporal()) {
                                    // Rounded for reading; the table keeps the full value.
                                    let v = values[r];
                                    let text =
                                        if v.is_nan() { String::new() } else { app_model::format::format_number(v, 7) };
                                    ui.label(RichText::new(text).monospace());
                                } else if c.as_numeric().is_some() {
                                    ui.label(RichText::new(text).monospace());
                                } else {
                                    ui.label(text);
                                }
                            }
                            ui.end_row();
                        }
                    });
                });
            }
        }
    }
    fn options(&mut self, ui: &mut egui::Ui, docs: &[Source<'_>], active: usize) {
        let primary = docs[active];
        let table = primary.table;
        match self.kind {
            TableOpKind::Sort => {
                ui.label("Sort by (most important first):");
                let mut remove = None;
                for (k, (column, descending)) in self.keys.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        column_combo(ui, ("sort_key", k), table, column, None);
                        ui.radio_value(descending, false, "Ascending");
                        ui.radio_value(descending, true, "Descending");
                        if k > 0 && ui.small_button("Remove").clicked() {
                            remove = Some(k);
                        }
                    });
                }
                if let Some(k) = remove {
                    self.keys.remove(k);
                }
                if ui.button("Add a sort key").clicked() {
                    self.keys.push((None, false));
                }
                ui.checkbox(
                    &mut self.new_table,
                    "Put the result in a new table (otherwise this table is sorted; Undo restores it)",
                );
            }
            TableOpKind::RemoveDuplicates => {
                ui.label("Rows are duplicates when they agree on these columns (none ticked: all columns):");
                column_checks(ui, "dup_columns", table, &mut self.columns);
                ui.checkbox(
                    &mut self.new_table,
                    "Put the result in a new table (otherwise rows are deleted here; Undo restores them)",
                );
            }
            TableOpKind::Subset => {
                let counts = primary.rows.counts();
                ui.label("Rows:");
                ui.horizontal_wrapped(|ui| {
                    ui.radio_value(&mut self.rows, RowsChoice::All, format!("All ({})", counts.total));
                    ui.radio_value(&mut self.rows, RowsChoice::Selected, format!("Selected ({})", counts.selected));
                    ui.radio_value(
                        &mut self.rows,
                        RowsChoice::Included,
                        format!("Not excluded ({})", counts.total - counts.excluded),
                    );
                    ui.radio_value(&mut self.rows, RowsChoice::Matching, "Meeting conditions");
                });
                if self.rows == RowsChoice::Matching {
                    conditions_editor(ui, table, &mut self.conditions);
                }
                ui.label("Columns to keep:");
                column_checks(ui, "subset_columns", table, &mut self.columns);
                ui.checkbox(&mut self.sample, "Take a random sample of these rows");
                if self.sample {
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut self.by_count, false, "Percent");
                        ui.add_enabled(
                            !self.by_count,
                            egui::DragValue::new(&mut self.percent).range(0.0..=100.0).suffix(" %"),
                        );
                        ui.radio_value(&mut self.by_count, true, "Rows");
                        ui.add_enabled(self.by_count, egui::DragValue::new(&mut self.count).range(0..=table.n_rows()));
                        ui.label("Seed");
                        ui.add(crate::dialog_kit::seed_value(&mut self.seed));
                    });
                    ui.label(RichText::new("Stratify by (samples each group in proportion to its size):").small());
                    column_checks(ui, "subset_strata", table, &mut self.strata);
                }
            }
            TableOpKind::CombineRows => {
                ui.label(format!("Append the rows of these open tables after “{}”:", table.name()));
                let mut any = false;
                for (i, doc) in docs.iter().enumerate() {
                    if i != active {
                        any = true;
                        ui.checkbox(&mut self.others[i], doc.table.name());
                    }
                }
                if !any {
                    ui.label(RichText::new("Open another table first (File ▸ Open…).").weak());
                }
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.source_column, "Record each row's table in a column named");
                    ui.add_enabled(
                        self.source_column,
                        egui::TextEdit::singleline(&mut self.source_name).desired_width(kit::em(ui) * 9.0),
                    );
                });
            }
            TableOpKind::Join => {
                ui.horizontal(|ui| {
                    ui.label(format!("Join “{}” with", table.name()));
                    let current = self.other.and_then(|o| docs.get(o)).map_or("Choose a table…", |d| d.table.name());
                    egui::ComboBox::from_id_salt("join_other").selected_text(current).show_list(ui, |ui| {
                        for (i, doc) in docs.iter().enumerate() {
                            if i != active && ui.selectable_label(self.other == Some(i), doc.table.name()).clicked() {
                                self.other = Some(i);
                                self.join_keys = vec![(None, None)];
                            }
                        }
                    });
                });
                let Some(right) = self.other.and_then(|o| docs.get(o)).map(|d| d.table) else {
                    ui.label(RichText::new("Open another table first (File ▸ Open…).").weak());
                    return;
                };
                ui.label("Rows match when every pair of key columns holds equal values (missing values never match):");
                let mut remove = None;
                for (k, (a, b)) in self.join_keys.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        column_combo(ui, ("join_left", k), table, a, None);
                        ui.label("=");
                        column_combo(ui, ("join_right", k), right, b, None);
                        if b.is_none() {
                            // Default to the column of the same name.
                            *b = a.and_then(|a| table.column(a)).and_then(|c| right.find(&c.name));
                        }
                        if k > 0 && ui.small_button("Remove").clicked() {
                            remove = Some(k);
                        }
                    });
                }
                if let Some(k) = remove {
                    self.join_keys.remove(k);
                }
                if ui.button("Add a key pair").clicked() {
                    self.join_keys.push((None, None));
                }
                ui.label("Keep:");
                ui.horizontal_wrapped(|ui| {
                    ui.radio_value(&mut self.join_kind, JoinKind::Inner, "Matched rows only");
                    ui.radio_value(&mut self.join_kind, JoinKind::Left, format!("Every row of “{}”", table.name()));
                    ui.radio_value(&mut self.join_kind, JoinKind::Right, format!("Every row of “{}”", right.name()));
                    ui.radio_value(&mut self.join_kind, JoinKind::Full, "Every row of both");
                });
                kit::more_options(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Check that keys are");
                        let label = |r: Relationship| match r {
                            Relationship::Any => "anything (no check)",
                            Relationship::ManyToOne => "unique in the other table",
                            Relationship::OneToOne => "unique in both tables",
                        };
                        egui::ComboBox::from_id_salt("join_relationship")
                            .selected_text(label(self.relationship))
                            .show_list(ui, |ui| {
                                for r in [Relationship::Any, Relationship::ManyToOne, Relationship::OneToOne] {
                                    ui.selectable_value(&mut self.relationship, r, label(r));
                                }
                            });
                    });
                });
            }
            TableOpKind::Stack => {
                ui.label("Columns to stack:");
                column_checks(ui, "stack_columns", table, &mut self.columns);
                egui::Grid::new("stack_names").num_columns(2).show(ui, |ui| {
                    ui.label("Label column name");
                    ui.text_edit_singleline(&mut self.label_name);
                    ui.end_row();
                    ui.label("Value column name");
                    ui.text_edit_singleline(&mut self.value_name);
                    ui.end_row();
                });
            }
            TableOpKind::Unstack => {
                ui.horizontal(|ui| {
                    ui.label("Label column (its levels become columns):");
                    column_combo(ui, "unstack_label", table, &mut self.label, None);
                });
                ui.columns(2, |cols| {
                    cols[0].label("Value columns:");
                    column_checks(&mut cols[0], "unstack_values", table, &mut self.columns);
                    cols[1].label("ID columns (one result row each):");
                    column_checks(&mut cols[1], "unstack_ids", table, &mut self.columns2);
                });
            }
            TableOpKind::DistanceMatrix => {
                ui.horizontal(|ui| {
                    ui.label("Distance:");
                    egui::ComboBox::from_id_salt("distance_metric")
                        .selected_text(self.metric.label())
                        .width(kit::combo_width(ui))
                        .show_list(ui, |ui| {
                            for m in DistanceMetric::ALL {
                                ui.selectable_value(&mut self.metric, m, m.label())
                                    .on_hover_text(crate::pcoa_dialog::metric_hint(m));
                            }
                        });
                });
                ui.label(if crate::pcoa_dialog::takes_categories(self.metric) {
                    "Columns to compare (any type):"
                } else {
                    "Numeric columns to compare:"
                });
                column_checks(ui, "distance_columns", table, &mut self.columns);
                if matches!(self.metric, DistanceMetric::Euclidean | DistanceMetric::Manhattan) {
                    ui.checkbox(&mut self.standardize, "Standardize columns first (recommended when units differ)");
                }
                ui.horizontal(|ui| {
                    ui.label("Name the rows and columns by");
                    column_combo(ui, "distance_label", table, &mut self.label, Some("row number"));
                });
                kit::note(ui, "Excluded rows are left out. Gower allows missing values; other distances skip rows with any.");
            }
            TableOpKind::Transpose => {
                ui.label("Columns to transpose:");
                column_checks(ui, "transpose_columns", table, &mut self.columns);
                ui.horizontal(|ui| {
                    ui.label("Name the new columns by");
                    column_combo(ui, "transpose_label", table, &mut self.label, Some("row number"));
                });
            }
            TableOpKind::Summarize => {
                ui.columns(2, |cols| {
                    cols[0].label("Summarize:");
                    column_checks(&mut cols[0], "summary_columns", table, &mut self.columns);
                    cols[1].label("Group by:");
                    column_checks(&mut cols[1], "summary_groups", table, &mut self.columns2);
                });
                ui.label("Statistics:");
                ui.horizontal_wrapped(|ui| {
                    for (on, (_, label)) in self.statistics.iter_mut().zip(STATISTICS) {
                        ui.checkbox(on, label);
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Quantiles (%):");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.quantiles)
                            .hint_text("e.g. 25, 75")
                            .desired_width(kit::em(ui) * 8.0),
                    );
                });
                ui.checkbox(&mut self.include_excluded, "Include excluded rows");
                kit::more_options(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Quantile definition:");
                        let current = QUANTILE_METHODS.iter().find(|m| m.0 == self.method).map_or("", |m| m.1);
                        egui::ComboBox::from_id_salt("summary_method")
                            .selected_text(current)
                            .width(kit::em(ui) * 17.0)
                            .show_list(ui, |ui| {
                                for (m, label) in QUANTILE_METHODS {
                                    ui.selectable_value(&mut self.method, m, label);
                                }
                            });
                    });
                });
            }
        }
    }
}

impl Remember for TableOpDialog {
    fn memory_key(&self) -> String {
        self.kind.title().into()
    }

    fn set_edit(&mut self, _report: Option<u64>) {}

    /// The grid's selected columns become the sort keys, or the columns to
    /// compare, keep, stack, transpose, summarize or unstack.
    fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        match self.kind {
            TableOpKind::Sort => self.keys = columns.iter().map(|&c| (Some(c), false)).collect(),
            TableOpKind::RemoveDuplicates
            | TableOpKind::Subset
            | TableOpKind::Stack
            | TableOpKind::Unstack
            | TableOpKind::Transpose
            | TableOpKind::Summarize
            | TableOpKind::DistanceMatrix => self.columns = kit::mask(table.n_cols(), columns),
            TableOpKind::CombineRows | TableOpKind::Join => {}
        }
    }
}

/// The new table's name: short (it is listed under its source), and
/// different from every open table's.
fn result_name(op: &TableOperation, others: &[usize], docs: &[Source<'_>]) -> String {
    let others: Vec<&str> = others.iter().filter_map(|&i| docs.get(i)).map(|d| d.table.name()).collect();
    unique_name(&op.default_name(&others), docs.iter().map(|d| d.table.name()))
}

/// Check boxes for the columns of `table` (the dialog kit's column list).
fn column_checks(ui: &mut egui::Ui, id: &str, table: &DataTable, checks: &mut Vec<bool>) {
    kit::column_list(ui, id, table, checks, |_| true);
}

/// A drop-down choosing one column of `table`; `none` names an empty choice.
fn column_combo(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash,
    table: &DataTable,
    slot: &mut Option<usize>,
    none: Option<&str>,
) {
    kit::column_choice(ui, id, table, slot, |_| true, none);
}
/// Edit a list of row conditions: levels of categorical columns or a range of
/// continuous ones. Rows must meet every condition.
fn conditions_editor(ui: &mut egui::Ui, table: &DataTable, conditions: &mut Vec<FilterCondition>) {
    let mut remove = None;
    for (k, c) in conditions.iter_mut().enumerate() {
        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(c.column()).strong());
                if ui.small_button("Remove").clicked() {
                    remove = Some(k);
                }
            });
            match c {
                FilterCondition::Range { min, max, .. } => {
                    ui.horizontal(|ui| {
                        ui.label("From");
                        ui.add(egui::DragValue::new(min).speed(0.1));
                        ui.label("to");
                        ui.add(egui::DragValue::new(max).speed(0.1));
                    });
                }
                FilterCondition::Levels { column, levels } => {
                    let all = stat_viz::filter::column_levels(table, column);
                    ui.horizontal_wrapped(|ui| {
                        for level in all {
                            let mut on = levels.contains(&level);
                            if ui.checkbox(&mut on, &level).changed() {
                                if on {
                                    levels.push(level);
                                } else {
                                    levels.retain(|l| *l != level);
                                }
                            }
                        }
                    });
                }
            }
        });
    }
    if let Some(k) = remove {
        conditions.remove(k);
    }
    let mut add: Option<usize> = None;
    ui.horizontal(|ui| {
        ui.label("Add a condition on");
        column_combo(ui, "condition_add", table, &mut add, Some("…"));
    });
    if let Some(i) = add
        && let Some(c) = table.column(i).and_then(|c| FilterCondition::for_column(table, &c.name))
    {
        conditions.push(c);
    }
}
