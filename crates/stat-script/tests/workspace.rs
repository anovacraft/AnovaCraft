//! Scripts run against the desktop application's open documents: the
//! changes they report replay onto the original documents exactly.

use std::cell::RefCell;
use std::rc::Rc;

use app_model::editing::Edit;
use app_model::project::{Project, save_project};
use app_model::sample::catalog;
use stat_data::{RowStateManager, SelectionMode};
use stat_script::{DocumentChange, OpenDocument, RunOptions, Workspace, WorkspaceRun, run_in_workspace, run_script};

fn sample(name: &str) -> Project {
    let info = catalog().into_iter().find(|s| s.name.starts_with(name)).unwrap();
    let s = (info.build)();
    let rows = RowStateManager::new(s.table.n_rows());
    Project { table: s.table, rows, reports: s.reports, origin: None, scripts: Vec::new(), external: None }
}

fn workspace(projects: Vec<Project>, active: usize) -> Workspace {
    let documents =
        projects.into_iter().enumerate().map(|(i, project)| OpenDocument { key: 10 + i as u64, project }).collect();
    Workspace { documents, active: Some(active) }
}

fn run(source: &str, ws: Workspace) -> (WorkspaceRun, Vec<String>) {
    let lines = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&lines);
    let run = run_in_workspace(source, &RunOptions::default(), ws, move |s| sink.borrow_mut().push(s.to_owned()));
    (run, lines.take())
}

/// Replay a change on the original project, as the application does.
fn replay(original: &Project, edits: &[Edit], selected: Option<&Vec<usize>>, excluded: Option<&Vec<usize>>) -> Project {
    let mut p = original.clone();
    let batch = Edit::Batch { label: "script".into(), edits: edits.to_vec() };
    let applied = batch.apply(&mut p.table, &mut p.rows).unwrap();
    for shift in &applied.shift {
        p.reports = p.reports.into_iter().map(|r| r.map_columns(|c| shift.map(c).unwrap_or(c))).collect();
    }
    if let Some(selected) = selected {
        p.rows.select(selected.iter().copied(), SelectionMode::Replace);
    }
    if let Some(excluded) = excluded {
        let n = p.rows.len();
        p.rows.set_excluded(0..n, false);
        p.rows.set_excluded(excluded.iter().copied(), true);
    }
    p
}

#[test]
fn work_on_open_documents_replays_onto_the_originals() {
    let norris = sample("Norris");
    let source = r#"
        let t = current_table();
        print(t.name);
        print(open_tables().len());
        t.add_column("Twice", t.column("Reference").map(|x| 2.0 * x));
        t.set_cell(0, "Reading", 1.5);
        t.rename_column("Reading", "Observed");
        t.sort(["Reference"]);
        t.exclude(|r| r.Reference > 90.0);
        t.select(|r| r.Reference < 10.0);
        t.analyze("linear_model", #{ response: "Observed", predictors: ["Twice"] });
    "#;
    let (run, lines) = run(source, workspace(vec![norris.clone(), sample("Longley")], 0));
    run.result.unwrap();
    assert_eq!(lines, [norris.table.name().to_owned(), "2".to_owned()]);
    assert_eq!(run.changes.len(), 1, "only the changed document is reported");
    let DocumentChange::Changed { key, edits, selected, excluded, reports, project, hidden, colors, .. } =
        &run.changes[0]
    else {
        panic!("expected a change to an open document");
    };
    assert_eq!(*key, 10);
    assert_eq!(edits.len(), 4);
    assert!(selected.as_ref().is_some_and(|s| !s.is_empty()) && excluded.as_ref().is_some_and(|e| !e.is_empty()));
    assert!(hidden.is_none() && colors.is_none());
    // The new report is first; the others keep their places.
    let reports = reports.as_ref().unwrap();
    assert_eq!(reports.len(), norris.reports.len() + 1);
    assert_eq!(reports[0].0, None);
    assert!(reports[1..].iter().enumerate().all(|(i, (k, _))| *k == Some(i)));
    // Replaying the edits gives the script's table, and the original reports
    // follow their columns exactly as the script's copies did.
    let mut replayed = replay(&norris, edits, selected.as_ref(), excluded.as_ref());
    replayed.reports.insert(0, reports[0].1.clone());
    assert_eq!(save_project(&replayed), save_project(project));
}

#[test]
fn shown_tables_open_as_new_documents_and_others_do_not() {
    let source = r#"
        let t = current_table();
        let s = t.summarize(#{ groups: [], columns: ["Reading"] });
        s.show();
        s.show();
        t.show();
        open_sample("Longley");
    "#;
    let (run, _) = run(source, workspace(vec![sample("Norris")], 0));
    run.result.unwrap();
    assert_eq!(run.changes.len(), 1);
    let DocumentChange::New { project } = &run.changes[0] else { panic!("expected a new document") };
    assert!(project.origin.is_some(), "a derived table keeps its origin");
}

#[test]
fn changes_before_an_error_are_kept() {
    let source = "let t = current_table();\nt.set_cell(0, \"Reading\", 3.0);\nt.cell(0, \"Nope\");\n";
    let (failed, _) = run(source, workspace(vec![sample("Norris")], 0));
    let err = failed.result.unwrap_err();
    assert_eq!(err.line, Some(3));
    assert!(matches!(
        &failed.changes[..],
        [DocumentChange::Changed { edits, selected: None, excluded: None, .. }] if edits.len() == 1
    ));
    // Only the row states a script set are reported.
    let (selecting, _) = run("current_table().select_rows([2, 3]);", workspace(vec![sample("Norris")], 0));
    assert!(matches!(
        &selecting.changes[..],
        [DocumentChange::Changed { edits, selected: Some(s), excluded: None, .. }] if edits.is_empty() && *s == [2, 3]
    ));
}

#[test]
fn without_open_documents_current_table_explains_itself() {
    let (run, _) = run("current_table()", Workspace::default());
    assert!(run.result.unwrap_err().message.contains("no table is open"));
    // Headless runs have no open tables, and show() does nothing.
    run_script(
        "if open_tables().len() != 0 { throw \"open\" } new_table(\"x\").show();",
        &RunOptions::default(),
        |_| {},
    )
    .unwrap_or_else(|e| panic!("{e}"));
}

#[test]
fn workspaces_and_results_cross_threads() {
    fn send<T: Send>() {}
    send::<Workspace>();
    send::<WorkspaceRun>();
}

#[test]
fn the_flag_outliers_example_works_on_the_current_table() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/scripts/flag_outliers.rhai");
    let source = std::fs::read_to_string(path).unwrap();
    let (run, lines) = run(&source, workspace(vec![sample("Michelson")], 0));
    run.result.unwrap_or_else(|e| panic!("{e}"));
    assert!(lines[0].starts_with("Speed"), "{lines:?}");
    let [
        DocumentChange::Changed { edits, selected: Some(selected), reports: Some(reports), .. },
        DocumentChange::New { project },
    ] = &run.changes[..]
    else {
        panic!("expected the current table changed and a summary shown: {:?}", run.changes.len());
    };
    assert_eq!(edits.len(), 1, "one z-score column");
    assert!(!selected.is_empty() && selected.len() < 20, "{selected:?}");
    assert_eq!(reports.iter().filter(|(k, _)| k.is_none()).count(), 1);
    assert_eq!(project.table.n_rows(), 1);
    assert!(project.table.name().ends_with("summary"));
}

#[test]
fn notebook_cells_share_variables_functions_and_tables() {
    use app_model::notebook::CellOutput;
    use stat_script::NotebookSession;

    let norris = sample("Norris");
    let mut session = NotebookSession::new(&[]);
    let cell = |session: &mut NotebookSession, source: &str, project: &Project| {
        let lines = Rc::new(RefCell::new(Vec::new()));
        let sink = Rc::clone(&lines);
        let run = session.run_cell(source, &RunOptions::default(), workspace(vec![project.clone()], 0), move |s| {
            sink.borrow_mut().push(s.to_owned());
        });
        (run, lines.take())
    };
    // Cell 1 defines a variable, a function and holds the open table.
    let (first, _) = cell(&mut session, "let t = current_table();\nfn twice(x) { 2 * x }\nlet n = t.rows();", &norris);
    assert!(first.run.result.is_ok(), "{:?}", first.run.result);
    assert!(first.value.is_empty(), "a cell ending in `;` has no value");
    assert_eq!(session.variables(), ["t", "n"]);
    // Cell 2 uses them; its value is shown.
    let (second, printed) = cell(&mut session, "print(twice(n));\nn + 1", &norris);
    assert!(second.run.result.is_ok(), "{:?}", second.run.result);
    assert_eq!(printed, [format!("{}", 2 * norris.table.n_rows())]);
    assert_eq!(second.value, [CellOutput::Value(format!("{}", norris.table.n_rows() + 1))]);
    // The application changed the document in between: `t` sees the new copy,
    // and edits through it are reported against the open document.
    let mut edited = norris.clone();
    edited.table = stat_data::DataTable::new("Norris (edited)", norris.table.columns().to_vec()).unwrap();
    let (third, printed) = cell(&mut session, "print(t.name);\nt.set_cell(0, \"Reading\", 1.5);\nt", &edited);
    assert!(third.run.result.is_ok(), "{:?}", third.run.result);
    assert_eq!(printed, ["Norris (edited)"]);
    assert!(matches!(&third.run.changes[..], [DocumentChange::Changed { key: 10, edits, .. }] if edits.len() == 1));
    match &third.value[..] {
        [CellOutput::Table { title, table }] => {
            assert!(title.starts_with("Norris (edited) ("), "{title}");
            assert_eq!(table.rows[0][table.columns.iter().position(|c| c == "Reading").unwrap()], "1.5");
        }
        other => panic!("{other:?}"),
    }
    // A failing cell reports its line, and later cells still run.
    let (bad, _) = cell(&mut session, "let a = 1;\nlet b = nope();", &norris);
    assert_eq!(bad.run.result.unwrap_err().line, Some(2));
    let (after, _) = cell(&mut session, "[#{ a: 1, b: \"x\" }, #{ a: 2 }]", &norris);
    match &after.value[..] {
        [CellOutput::Table { table, .. }] => {
            assert_eq!(table.columns, ["a", "b"]);
            assert_eq!(table.rows, [vec!["1".to_owned(), "x".to_owned()], vec!["2".to_owned(), String::new()]]);
        }
        other => panic!("{other:?}"),
    }
    // A report's tables are its value's tables.
    let (report, _) = cell(
        &mut session,
        "t.analyze(\"linear_model\", #{ response: \"Reading\", predictors: [\"Reference\"] })",
        &norris,
    );
    assert!(report.run.result.is_ok(), "{:?}", report.run.result);
    assert!(
        report.value.iter().any(|o| matches!(o, CellOutput::Table { title, .. } if title == "Parameter Estimates"))
    );
}
