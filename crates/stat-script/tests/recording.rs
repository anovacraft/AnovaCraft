//! Recorded scripts replay exactly: a session of edits, undo and redo,
//! row-state changes, report changes and table operations is recorded the
//! way the application records it, and running the script gives tables
//! equal to the session's in data, row states and reports.

use app_model::action::apply_edit;
use app_model::editing::{Edit, EditHistory};
use app_model::project::{Project, SavedReport, save_project};
use app_model::recording::{OpenedFrom, Recorder};
use app_model::sample::catalog;
use app_model::table_ops::{Source, TableOperation, TableOrigin, sort_edit};
use app_model::{LinearModelSpec, ReportOptions};
use stat_data::{CellValue, Column, ColumnKind, DataTable, MeasurementLevel, RowColor, RowStateManager, SelectionMode};
use stat_script::{DocumentChange, RunOptions, Workspace, run_in_workspace};

/// One open document as the application holds it.
struct Doc {
    key: u64,
    table: DataTable,
    rows: RowStateManager,
    /// `(id, report)`, newest first.
    reports: Vec<(u64, SavedReport)>,
    history: EditHistory,
}

impl Doc {
    fn new(key: u64, table: DataTable, rows: RowStateManager, reports: Vec<(u64, SavedReport)>) -> Self {
        let mut history = EditHistory::default();
        history.set_logging(true);
        Self { key, table, rows, reports, history }
    }

    /// Apply an edit as the application does (undoable, reports following
    /// their columns).
    fn edit(&mut self, edit: Edit) {
        // The history applies the edit; repeat the report-following step on
        // a scratch copy of the data so the reports move the same way.
        let (mut t, mut r) = (self.table.clone(), self.rows.clone());
        let mut saved: Vec<SavedReport> = self.reports.iter().map(|(_, s)| s.clone()).collect();
        apply_edit(edit.clone(), &mut t, &mut r, &mut saved).unwrap();
        self.history.apply(edit, &mut self.table, &mut self.rows).unwrap();
        for ((_, s), new) in self.reports.iter_mut().zip(saved) {
            *s = new;
        }
    }

    fn sync(&mut self, recorder: &mut Recorder) {
        recorder.edits(self.key, self.history.take_logged());
        recorder.sync(self.key, &self.table, &self.rows, &self.reports);
    }

    fn project(&self) -> Project {
        Project {
            table: self.table.clone(),
            rows: self.rows.clone(),
            reports: self.reports.iter().map(|(_, r)| r.clone()).collect(),
            origin: None,
            scripts: Vec::new(),
            external: None,
        }
    }
}

fn norris() -> (String, DataTable, Vec<SavedReport>) {
    let info = catalog().into_iter().find(|s| s.name.starts_with("Norris")).unwrap();
    let s = (info.build)();
    (info.name.to_owned(), s.table, s.reports)
}

/// The script's tables as projects (they come back as shown tables).
fn replay(script: &str) -> Vec<Project> {
    let run = run_in_workspace(script, &RunOptions::default(), Workspace::default(), |_| {});
    run.result.unwrap_or_else(|e| panic!("{e}\n{script}"));
    run.changes
        .into_iter()
        .map(|c| match c {
            DocumentChange::New { project } => *project,
            DocumentChange::Changed { .. } => panic!("no open documents were given"),
        })
        .collect()
}

/// Projects without the origin and scripts, which the comparison ignores.
fn plain(p: &Project) -> String {
    let mut p = p.clone();
    p.origin = None;
    p.scripts.clear();
    save_project(&p)
}

#[test]
fn a_recorded_session_replays_exactly() {
    let mut recorder = Recorder::new();
    let (sample_name, table, reports) = norris();
    let n = table.n_rows();
    let mut next_id = 100;
    let reports: Vec<(u64, SavedReport)> = reports
        .into_iter()
        .map(|r| {
            next_id += 1;
            (next_id, r)
        })
        .collect();
    let mut doc = Doc::new(1, table, RowStateManager::new(n), reports);
    recorder.opened(1, OpenedFrom::Sample(sample_name), &doc.table, &doc.rows, &doc.reports);

    // Data edits, including a paste-style batch and undo / redo.
    let reading = doc.table.find("Reading").unwrap();
    doc.edit(Edit::SetCell { row: 0, column: reading, value: CellValue::Number(0.125) });
    let twice: Vec<f64> = doc.table.columns()[reading].as_numeric().unwrap().iter().map(|x| 2.0 * x).collect();
    doc.edit(Edit::InsertColumn { index: 0, column: Column::numeric("Twice \"reading\"", twice) });
    doc.edit(Edit::RenameColumn { index: 0, name: "Doubled".into() });
    let group: Vec<String> = (0..n).map(|i| if i % 3 == 0 { "a" } else { "b" }.to_owned()).collect();
    doc.edit(Edit::InsertColumn { index: doc.table.n_cols(), column: Column::text("Group", group) });
    doc.edit(Edit::SetLevel { index: doc.table.n_cols() - 1, level: MeasurementLevel::Ordinal });
    // Formula columns: one kept, one turned into values.
    let mut score = Column::empty("Score", ColumnKind::Numeric, n);
    score.formula = Some("Reading * 2 + if_missing(lag(\"Reading\"), 0)".into());
    doc.edit(Edit::InsertColumn { index: doc.table.n_cols(), column: score });
    let mut label = Column::empty("Label", ColumnKind::Text, n);
    label.formula = Some("concat(Group, \"-\", row())".into());
    doc.edit(Edit::InsertColumn { index: doc.table.n_cols(), column: label });
    doc.sync(&mut recorder);
    doc.edit(Edit::RemoveRows { rows: vec![1, 4, 5] });
    doc.history.undo(&mut doc.table, &mut doc.rows).unwrap().unwrap();
    doc.history.redo(&mut doc.table, &mut doc.rows).unwrap().unwrap();
    doc.history.undo(&mut doc.table, &mut doc.rows).unwrap().unwrap();
    let order = sort_edit(&doc.table, &[("Reading".into(), true)]).unwrap();
    doc.edit(order);
    let (converted, _) = Edit::convert_column(&doc.table, 0, ColumnKind::Text).unwrap();
    doc.edit(converted);
    let at = doc.table.n_rows();
    doc.edit(Edit::Batch {
        label: "paste".into(),
        edits: vec![
            Edit::insert_empty_rows(&doc.table, at, 2),
            Edit::SetCell { row: at, column: reading, value: CellValue::Number(f64::NAN) },
            Edit::SetCell { row: at + 1, column: 0, value: CellValue::Text("x y".into()) },
        ],
    });
    let score = doc.table.find("Score").unwrap();
    doc.edit(Edit::SetFormula { index: score, formula: Some("Reading + col_mean(\"Reading\", \"Group\")".into()) });
    doc.edit(Edit::SetFormula { index: doc.table.find("Label").unwrap(), formula: None });
    doc.edit(Edit::RenameColumn { index: doc.table.find("Reading").unwrap(), name: "Reading (mm)".into() });
    assert_eq!(
        doc.table.columns()[score].formula.as_deref(),
        Some("col(\"Reading (mm)\") + col_mean(\"Reading (mm)\", \"Group\")")
    );
    doc.sync(&mut recorder);

    // Row states.
    doc.rows.select([2, 3, 7], SelectionMode::Replace);
    doc.rows.set_excluded([0, 9], true);
    doc.sync(&mut recorder);
    doc.rows.set_excluded([0], false);
    doc.rows.set_excluded([10, 11], true);
    doc.rows.set_hidden([5], true);
    doc.rows.set_color([1, 2], Some(RowColor { r: 200, g: 30, b: 60 }));
    doc.rows.set_color([3], Some(RowColor { r: 0, g: 90, b: 180 }));
    doc.sync(&mut recorder);
    doc.rows.set_color([2], None);
    doc.rows.clear_selection();
    doc.sync(&mut recorder);
    // Value labels, the hover choice, colours by column and a hand-set marker.
    let group = doc.table.find("Group").unwrap();
    let labels = [("a".to_owned(), "Group A".to_owned()), ("b".to_owned(), "Group B".to_owned())];
    doc.edit(Edit::SetValueLabels { index: group, labels: labels.into_iter().collect() });
    doc.edit(Edit::SetHover { index: group, hover: true });
    app_model::action::color_by_column(&doc.table, &mut doc.rows, "Group").unwrap();
    doc.sync(&mut recorder);
    doc.rows.set_marker([4], Some(stat_data::RowMarker::Cross));
    doc.sync(&mut recorder);

    // Reports: a new one, a changed option, one closed.
    let response = doc.table.find("Reading (mm)").unwrap();
    let predictor = doc.table.find("Reference").unwrap();
    let spec = LinearModelSpec {
        response,
        predictors: vec![predictor],
        effects: vec![vec![(0, 1)], vec![(0, 2)]],
        splines: Vec::new(),
    };
    next_id += 1;
    doc.reports.insert(0, (next_id, SavedReport::Linear { spec, options: ReportOptions::default() }));
    doc.sync(&mut recorder);
    if let (_, SavedReport::Linear { options, .. }) = &mut doc.reports[0] {
        options.show_residual_plot = !options.show_residual_plot;
        options.show_profiler = true;
    }
    doc.sync(&mut recorder);
    let last = doc.reports.len() - 1;
    doc.reports.remove(last);
    doc.sync(&mut recorder);

    // A table operation on it, then an edit on the new table.
    let origin = TableOrigin {
        operation: TableOperation::Sort { keys: vec![("Group".into(), false), ("Reference".into(), true)] },
        sources: vec![doc.table.name().to_owned()],
    };
    let derived =
        origin.operation.run(Source { table: &doc.table, rows: &doc.rows }, &[], "Norris sorted by group").unwrap();
    let mut sorted = Doc::new(2, derived.table, derived.rows, Vec::new());
    recorder.opened(
        2,
        OpenedFrom::Derived { origin: derived.origin, sources: vec![1] },
        &sorted.table,
        &sorted.rows,
        &[],
    );
    sorted.edit(Edit::RemoveColumn { index: 0 });
    sorted.rows.select([0], SelectionMode::Replace);
    sorted.sync(&mut recorder);

    // A table typed in (written into the script value by value).
    let made = DataTable::new(
        "Typed in",
        vec![
            Column::numeric("x", vec![1.0, 2.5, f64::NAN]),
            Column::text("label", vec!["a".into(), "".into(), "é".into()]),
        ],
    )
    .unwrap();
    let mut typed = Doc::new(3, made, RowStateManager::new(3), Vec::new());
    recorder.opened(3, OpenedFrom::Data, &typed.table, &typed.rows, &[]);
    typed.edit(Edit::SetCell { row: 1, column: 1, value: CellValue::Text("b".into()) });
    typed.sync(&mut recorder);

    let script = recorder.script();
    assert!(!script.contains("Could not record"), "{script}");
    assert!(!script.contains("outside the recorded edits"), "{script}");
    if let Ok(path) = std::env::var("ANOVACRAFT_RECORDING_OUT") {
        std::fs::write(path, &script).unwrap();
    }
    let tables = replay(&script);
    assert_eq!(tables.len(), 3, "{script}");
    for (doc, replayed) in [&doc, &sorted, &typed].into_iter().zip(&tables) {
        assert_eq!(plain(&doc.project()), plain(replayed), "{}\n{script}", doc.table.name());
    }
    // Readable calls, not data dumps, for ordinary steps.
    for call in [
        "t1.set_cell(0, \"Reading\", 0.125);",
        "t1.select_rows([2, 3, 7]);",
        "t1.close_report(",
        "t2.remove_column(",
        "formula: \"Reading * 2 + if_missing(lag(\\\"Reading\\\"), 0)\" }",
        "t1.set_formula(\"Score\", ",
        "t1.clear_formula(\"Label\");",
        "t1.set_value_labels(\"Group\", #{\"a\": \"Group A\", \"b\": \"Group B\"});",
        "t1.set_hover(\"Group\", true);",
        "t1.color_by_column(\"Group\");",
        "t1.mark_rows([4], \"cross\");",
    ] {
        assert!(script.contains(call), "{call}\n{script}");
    }
}

/// Recording follows tables that were already open, found by name or as
/// the current table, and the changes come back to those documents.
#[test]
fn open_tables_are_found_and_changed() {
    let (_, table, reports) = norris();
    let n = table.n_rows();
    let saved = reports.clone();
    let ids: Vec<(u64, SavedReport)> = reports.into_iter().enumerate().map(|(i, r)| (i as u64, r)).collect();
    let mut doc = Doc::new(7, table, RowStateManager::new(n), ids);
    let mut recorder = Recorder::new();
    recorder.existing(7, 0, true, true, &doc.table, &doc.rows, &doc.reports);
    doc.edit(Edit::SetCell { row: 3, column: 0, value: CellValue::Number(-1.0) });
    doc.rows.set_excluded([1], true);
    doc.reports.clear();
    doc.sync(&mut recorder);
    let script = recorder.script();
    assert!(script.contains("let t1 = current_table();"), "{script}");

    let project = Project {
        table: doc.table.clone(),
        rows: RowStateManager::new(n),
        reports: saved,
        origin: None,
        scripts: Vec::new(),
        external: None,
    };
    let mut original = project.clone();
    original.table = norris().1;
    let ws = Workspace { documents: vec![stat_script::OpenDocument { key: 7, project: original }], active: Some(0) };
    let run = run_in_workspace(&script, &RunOptions::default(), ws, |_| {});
    run.result.unwrap();
    let [DocumentChange::Changed { edits, excluded: Some(excluded), reports: Some(reports), .. }] = &run.changes[..]
    else {
        panic!("expected one changed document");
    };
    assert_eq!(edits.len(), 1);
    assert_eq!(excluded, &[1]);
    assert!(reports.is_empty(), "the report was closed");
}
