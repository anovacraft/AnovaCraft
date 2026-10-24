//! Scripts against the application: every analysis run from a script gives
//! the same sections as the same request run directly (as the dialog and
//! project reloading run it), and the table, row-state, file and safety
//! behaviour of the script API.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use app_model::sample::catalog;
use app_model::{
    DistributionAnalysis, DistributionOptions, DistributionRequest, LinearModelAnalysis, LinearModelSpec,
    ReportOptions, report_to_json, run_report,
};
use rhai::Scope;
use stat_data::RowStateManager;
use stat_script::{Limits, Report, RunOptions, ScriptError, Table, new_engine, run_script};

/// Run `source` and return its printed lines.
fn run(source: &str) -> Result<Vec<String>, ScriptError> {
    run_with(source, &RunOptions::default())
}

fn run_with(source: &str, options: &RunOptions) -> Result<Vec<String>, ScriptError> {
    let lines = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&lines);
    run_script(source, options, move |s| sink.borrow_mut().push(s.to_owned()))?;
    Ok(lines.take())
}

/// A fresh empty directory for files a test writes.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("stat-script-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn every_sample_report_run_from_a_script_matches_the_direct_run() {
    let engine = new_engine(&RunOptions::default(), |_| {});
    let mut checked = 0;
    for info in catalog() {
        let sample = (info.build)();
        let rows = RowStateManager::new(sample.table.n_rows());
        for saved in &sample.reports {
            let direct = run_report(saved, &sample.table, &rows).unwrap();
            let mut scope = Scope::new();
            scope.push("name", info.name.to_owned());
            scope.push("request", report_to_json(saved, &sample.table).to_compact());
            let report: Report = engine
                .eval_with_scope(&mut scope, "open_sample(name).analyze(parse_json(request))")
                .unwrap_or_else(|e| panic!("{}: {e}", direct.title));
            let scripted = report.computed();
            assert_eq!(scripted.saved, *saved, "{}", direct.title);
            assert_eq!(scripted.title, direct.title);
            // Debug text compares NaN cells too.
            assert_eq!(format!("{:?}", scripted.sections), format!("{:?}", direct.sections), "{}", direct.title);
            checked += 1;
        }
    }
    assert!(checked >= 20, "only {checked} sample reports");
}

#[test]
fn script_requests_use_the_dialog_defaults() {
    let engine = new_engine(&RunOptions::default(), |_| {});
    let norris = app_model::sample::norris();
    let rows = RowStateManager::new(norris.n_rows());

    let r: Report = engine
        .eval(r#"open_sample("Norris").analyze("linear_model", #{ response: "Reading", predictors: ["Reference"] })"#)
        .unwrap();
    let spec = LinearModelSpec { response: 1, predictors: vec![0], effects: Vec::new(), splines: Vec::new() };
    let direct = LinearModelAnalysis::run(&norris, &rows, &spec).unwrap();
    assert_eq!(r.computed().title, direct.title());
    assert_eq!(format!("{:?}", r.computed().sections), format!("{:?}", direct.sections(&ReportOptions::default())));

    let r: Report = engine
        .eval(r#"open_sample("Norris").analyze("distribution", #{ column: "Reading", quantile_type: 8 })"#)
        .unwrap();
    let request = DistributionRequest { column: 1, quantile_method: Default::default(), censoring: None };
    let direct = DistributionAnalysis::run(&norris, &rows, &request).unwrap();
    assert_eq!(
        format!("{:?}", r.computed().sections),
        format!("{:?}", direct.sections(&DistributionOptions::default()))
    );
}

#[test]
fn sections_come_back_as_data() {
    let out = run(r#"
        let t = open_sample("Norris");
        let r = t.analyze("linear-model", #{ response: "Reading", predictors: ["Reference"],
                                             options: #{ show_residual_plot: true } });
        print(r.title);
        print(r.analysis);
        let est = r.section("parameter estimates");     // titles match without regard to case
        print(est.len());
        print(type_of(est[1].Estimate));
        let plot = r.section("Residuals vs Predicted");
        print(plot.points.len());
        print(r.request().response);
        print(r.has_section("No such section"));
    "#)
    .unwrap();
    assert_eq!(out[0], "Linear Model: Reading ~ Reference");
    assert_eq!(out[1], "linear-model");
    assert_eq!(out[2], "2");
    assert_eq!(out[3], "f64");
    assert_eq!(out[4], "36");
    assert_eq!(out[5], "Reading");
    assert_eq!(out[6], "false");
}

#[test]
fn contour_style_sets_the_maps_of_a_report() {
    let out = run(r#"
        let t = open_sample("Hillside");
        let req = #{ x: "Easting (m)", y: "Northing (m)", z: "Elevation (m)" };
        let plain = t.analyze("data-surface", req);
        req.contour_style = #{ fill: false, scale: "heat", levels: 4 };
        let styled = t.analyze("data-surface", req);
        let title = |r| r.sections().filter(|s| s.starts_with("Contour Plot"))[0];
        let a = plain.section(title.call(plain)).contours;
        let b = styled.section(title.call(styled)).contours;
        print(a.levels.len() > b.levels.len());
        print(b.lines.len() == b.levels.len());
        print(type_of(b.lines[0].lines[0][0][0]));
        print(styled.request().contour_style.scale);
        print(plain.request().contour_style == ());
    "#)
    .unwrap();
    assert_eq!(out, ["true", "true", "f64", "heat", "true"]);
    let err = run(r#"open_sample("Hillside").analyze("data-surface",
        #{ x: "Easting (m)", y: "Northing (m)", z: "Elevation (m)", contour_style: #{ scale: "rainbow" } })"#)
    .unwrap_err()
    .to_string();
    assert!(err.contains("scale"), "{err}");
}

#[test]
fn tables_are_built_edited_and_row_states_change_analyses() {
    let out = run(r#"
        let t = new_table("Scores");
        t.add_column("group", ["a", "a", "b", "b", "b", "c"]);
        t.add_column("score", [1, 2, 3, 4.5, (), 6]);
        print(t.level("group"));
        print(t.rows());
        print(t.column("score"));
        t.set_cell(4, "score", 5);
        print(t.cell(4, "score"));
        print(t.select(|row| row.group == "b"));
        print(t.selected_rows());
        print(t.exclude(|row| row.score > 5));
        let d = t.analyze("distribution", #{ column: "score", quantile_type: 8 });
        print(d.section("Summary Statistics")[0]);
        t.include_all();
        print(t.excluded_rows());
        t.sort([#{ column: "score", descending: true }]);
        print(t.column("score"));
        t.rename_column("score", "points");
        print(t.columns());
    "#)
    .unwrap();
    assert_eq!(out[0], "nominal");
    assert_eq!(out[1], "6");
    assert_eq!(out[2], "[1.0, 2.0, 3.0, 4.5, (), 6.0]");
    assert_eq!(out[3], "5.0");
    assert_eq!(out[4], "3");
    assert_eq!(out[5], "[2, 3, 4]");
    assert_eq!(out[6], "1");
    // Five rows remain after the exclusion.
    assert!(out[7].contains("5"), "{}", out[7]);
    assert_eq!(out[8], "[]");
    assert_eq!(out[9], "[6.0, 5.0, 4.5, 3.0, 2.0, 1.0]");
    assert_eq!(out[10], "[\"group\", \"points\"]");
}

/// Value labels name levels in reports, hover columns are listed, rows are
/// coloured and marked by a column, and a labelled table can be written
/// with its labels.
#[test]
fn value_labels_hover_and_row_styles() {
    let dir = scratch("value-labels");
    let path = dir.join("arms.csv");
    let out = run(&format!(
        r#"
        let t = new_table("Trial");
        t.add_column("arm", [0, 1, 1, 0]);
        t.add_column("y", [1.0, 2.5, 3.0, 1.5]);
        t.set_level("arm", "nominal");
        t.set_value_labels("arm", #{{"0": "Placebo", "1": "Active"}});
        print(t.value_labels("arm"));
        t.set_hover("y", true);
        print(t.hover_columns());
        t.color_by_column("arm");
        print(t.style_column());
        print(t.row_color(0) != t.row_color(1));
        print(t.row_marker(1));
        t.mark_rows([0], "diamond");
        print(t.row_marker(0));
        t.unmark_rows([0]);
        print(t.row_marker(0));
        t.save_labelled("{}");
        t.clear_row_colors();
        print(t.row_color(1));
        t.clear_value_labels("arm");
        print(t.value_labels("arm"));
    "#,
        path.display().to_string().replace('\\', "/")
    ))
    .unwrap();
    assert_eq!(out[0], "#{\"0\": \"Placebo\", \"1\": \"Active\"}");
    assert_eq!(out[1], "[\"y\"]");
    assert_eq!(out[2], "arm");
    assert_eq!(out[3], "true");
    assert_eq!(out[4], "circle", "Active, the first level by name");
    assert_eq!(out[5], "diamond");
    assert_eq!(out[6], "");
    assert_eq!(out[7], "");
    assert_eq!(out[8], "#{}");
    let written = std::fs::read_to_string(&path).unwrap();
    assert!(written.contains("Placebo") && written.contains("Active") && !written.contains("\n0,"), "{written}");
    assert!(run(r#"new_table("t").mark_rows([0], "star");"#).is_err(), "an unknown marker is refused");
}

#[test]
fn excluded_rows_give_the_same_result_as_the_direct_run() {
    let engine = new_engine(&RunOptions::default(), |_| {});
    let r: Report = engine
        .eval(
            r#"let t = open_sample("Norris");
               t.exclude_rows([0, 5, 9]);
               t.analyze("linear_model", #{ response: "Reading", predictors: ["Reference"] })"#,
        )
        .unwrap();
    let norris = app_model::sample::norris();
    let mut rows = RowStateManager::new(norris.n_rows());
    rows.set_excluded([0, 5, 9], true);
    let spec = LinearModelSpec { response: 1, predictors: vec![0], effects: Vec::new(), splines: Vec::new() };
    let direct = LinearModelAnalysis::run(&norris, &rows, &spec).unwrap();
    assert_eq!(format!("{:?}", r.computed().sections), format!("{:?}", direct.sections(&ReportOptions::default())));
}

#[test]
fn table_operations_make_new_tables() {
    let out = run(r#"
        let t = new_table("Wide");
        t.add_column("id", [1, 2, 3]);
        t.add_column("before", [10, 20, 30]);
        t.add_column("after", [11, 23, 29]);
        let long = t.stack(#{ columns: ["before", "after"], label: "when", value: "y" });
        print(long.name);
        print(long.rows());
        print(long.columns());
        let s = long.summarize(#{ groups: ["when"], columns: ["y"], statistics: ["n", "mean"] });
        print(s.rows());
        let sorted = t.sorted([#{ column: "after", descending: true }]);
        print(sorted.column("id"));
        print(t.column("id"));
        let other = new_table("Labels");
        other.add_column("id", [1, 3]);
        other.add_column("label", ["one", "three"]);
        let joined = t.join(other, #{ keys: ["id"], kind: "left" });
        print(joined.column("label"));
        print(t.combine_rows([t]).rows());
        print(t.subset(#{ columns: ["id"] }).columns());
    "#)
    .unwrap();
    assert_eq!(out[0], "Wide, stacked");
    assert_eq!(out[1], "6");
    assert!(out[2].contains("\"when\"") && out[2].contains("\"y\""), "{}", out[2]);
    assert_eq!(out[3], "2");
    assert_eq!(out[4], "[3.0, 2.0, 1.0]");
    assert_eq!(out[5], "[1.0, 2.0, 3.0]", "the source is unchanged");
    assert_eq!(out[6], "[\"one\", (), \"three\"]");
    assert_eq!(out[7], "6");
    assert_eq!(out[8], "[\"id\"]");
}

#[test]
fn multiple_imputation_makes_a_stacked_table_and_reports_pool() {
    let out = run(r#"
        let t = open_sample("Air quality");
        let mi = t.multiple_imputation(#{ columns: ["Temperature (°C)", "Humidity (%)", "Ozone (µg/m³)"],
                                          imputations: 3, iterations: 2, seed: 4 });
        print(mi.rows());
        print(mi.columns()[0]);
        let r = mi.analyze("linear_model", #{ response: "Ozone (µg/m³)",
                                             predictors: ["Temperature (°C)", "Humidity (%)"],
                                             pool_imputations: "Imputation" });
        print(r.has_section("Pooled Estimates"));
        print(r.request().pool_imputations);
        let again = t.multiple_imputation(#{ columns: ["Temperature (°C)", "Humidity (%)", "Ozone (µg/m³)"],
                                             imputations: 3, iterations: 2, seed: 4 });
        print(again.column("Ozone (µg/m³)") == mi.column("Ozone (µg/m³)"));
    "#)
    .unwrap();
    assert_eq!(out[0], (4 * 120).to_string());
    assert_eq!(out[1], "Imputation");
    assert_eq!(out[2], "true");
    assert_eq!(out[3], "Imputation");
    assert_eq!(out[4], "true", "the same seed makes the same imputations");
}

#[test]
fn files_are_written_and_projects_reopen_with_their_reports() {
    let dir = scratch("files");
    let options = RunOptions { base_dir: dir.clone(), ..RunOptions::default() };
    run_with(
        r#"
        let t = open_sample("Norris");
        t.exclude_rows([2]);
        let r = t.analyze("linear_model", #{ response: "Reading", predictors: ["Reference"] });
        r.save("model.md");
        r.save("model.html");
        r.save("model.pdf", #{ paper: "a4" });
        r.save("model.docx");
        r.save("model.pptx");
        r.save("model.xlsx");
        r.save_section("Parameter Estimates", "estimates.csv");
        r.save_section("Fitted Line", "points.tsv");
        r.save_section("Fitted Line", "fit.svg");
        r.save_section("Fitted Line", "fit.pdf");
        r.save_section("Fitted Line", "fit.png");
        t.save("norris.csv");
        let g = t.analyze(#{ analysis: "graph", encoding: #{ x: ["Reference"], y: ["Reading"] }, points: #{} });
        g.save("graph.svg");
        g.save("graph.png");
        t.save_reports("reports.html");
        t.save_reports("reports.pdf");
        t.save_reports("reports.docx", #{ paper: "a4" });
        t.save_reports("reports.md");
        t.save_project("norris.acraft");
        "#,
        &options,
    )
    .unwrap();
    let md = std::fs::read_to_string(dir.join("model.md")).unwrap();
    assert!(md.starts_with("# Linear Model: Reading ~ Reference\n"), "{md}");
    assert!(md.contains("## Parameter Estimates") && md.contains("| Term |"), "{md}");
    let csv = std::fs::read_to_string(dir.join("estimates.csv")).unwrap();
    assert!(csv.starts_with("Term,Estimate"), "{csv}");
    let html = std::fs::read_to_string(dir.join("model.html")).unwrap();
    assert!(html.starts_with("<!DOCTYPE html>") && html.contains("<h1>Linear Model: Reading ~ Reference</h1>"));
    assert!(html.contains("<h2>Parameter Estimates</h2>") && html.contains("<svg"));
    let all = std::fs::read_to_string(dir.join("reports.html")).unwrap();
    assert!(
        all.contains("<h2>Linear Model: Reading ~ Reference</h2>")
            && all.matches("<section class=\"report\">").count() == 3
    );
    // Documents: PDF, Word, PowerPoint and Excel, one report or all.
    assert!(std::fs::read(dir.join("model.pdf")).unwrap().starts_with(b"%PDF"));
    for name in ["model.docx", "model.pptx", "model.xlsx", "reports.docx"] {
        assert!(std::fs::read(dir.join(name)).unwrap().starts_with(b"PK"), "{name}");
    }
    assert!(std::fs::read(dir.join("reports.pdf")).unwrap().starts_with(b"%PDF"));
    let md = std::fs::read_to_string(dir.join("reports.md")).unwrap();
    assert!(md.contains("](reports_files/plot-01.png)") && dir.join("reports_files/plot-01.png").exists());
    assert_eq!(csv.lines().count(), 3);
    let tsv = std::fs::read_to_string(dir.join("points.tsv")).unwrap();
    assert_eq!(tsv.lines().count(), 37, "a heading and 36 points (excluded rows are plotted too)");
    assert!(std::fs::read_to_string(dir.join("norris.csv")).unwrap().starts_with("Reference,Reading"));
    assert!(std::fs::read_to_string(dir.join("graph.svg")).unwrap().contains("<svg"));
    // The fitted-line plot as a figure, its excluded row marked in the key.
    let fit = std::fs::read_to_string(dir.join("fit.svg")).unwrap();
    assert!(fit.contains("<svg") && fit.contains("Fitted Line") && fit.contains("Excluded rows"), "{fit}");
    assert!(std::fs::read(dir.join("fit.pdf")).unwrap().starts_with(b"%PDF"));
    assert!(std::fs::read(dir.join("fit.png")).unwrap().starts_with(b"\x89PNG"));
    assert!(std::fs::read(dir.join("graph.png")).unwrap().starts_with(b"\x89PNG"));

    // The project carries the data, exclusion and both reports.
    let out = run_with(
        r#"
        let t = open_project("norris.acraft");
        print(t.excluded_rows());
        let reports = t.reports();
        print(reports.len());
        print(reports[1].title);
        print(reports[0].analysis);  // newest first
        "#,
        &options,
    )
    .unwrap();
    assert_eq!(out, ["[2]", "3", "Linear Model: Reading ~ Reference", "graph"]);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn workbooks_are_read_and_written() {
    let dir = scratch("workbooks");
    // A workbook with an empty first sheet and two sheets of data.
    let mut book = rust_xlsxwriter::Workbook::new();
    book.add_worksheet().set_name("Notes").unwrap();
    let a = book.add_worksheet().set_name("Line A").unwrap();
    a.write_string(0, 0, "x").unwrap();
    a.write_string(0, 1, "y").unwrap();
    for i in 1..=4u32 {
        a.write_number(i, 0, f64::from(i)).unwrap();
        a.write_number(i, 1, 2.0 * f64::from(i) + 0.5).unwrap();
    }
    let b = book.add_worksheet().set_name("Line B").unwrap();
    b.write_string(0, 0, "operator").unwrap();
    b.write_string(1, 0, "Ana").unwrap();
    book.save(dir.join("runs.xlsx")).unwrap();

    let options = RunOptions { base_dir: dir.clone(), ..RunOptions::default() };
    let err = run_with(
        r#"
        let first = open_table("runs.xlsx");
        first.add_column("z", first.column("x").map(|v| v * 10.0));
        first.save("copy.xlsx");
        open_table("runs.xlsx", "Line C");
        "#,
        &options,
    )
    .unwrap_err();
    assert!(err.to_string().contains("has no sheet \"Line C\"") && err.to_string().contains("\"Notes\""), "{err}");
    assert!(dir.join("copy.xlsx").exists(), "the copy was saved before the error");
    let out = run_with(
        r#"
        let first = open_table("runs.xlsx");
        print(first.name);
        print(first.column("y"));
        print(open_table("runs.xlsx", "Line B").column("operator"));
        print(open_workbook("runs.xlsx").map(|t| t.name));
        print(open_table("copy.xlsx").column("z"));
        "#,
        &options,
    )
    .unwrap();
    assert_eq!(
        out,
        [
            "runs – Line A",
            "[2.5, 4.5, 6.5, 8.5]",
            "[\"Ana\"]",
            "[\"runs – Line A\", \"runs – Line B\"]",
            "[10.0, 20.0, 30.0, 40.0]",
        ]
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn tables_do_not_save_as_figures() {
    let err = run(r#"
        let r = open_sample("Norris").analyze("linear_model", #{ response: "Reading", predictors: ["Reference"] });
        r.save_section("Parameter Estimates", "estimates.svg");
        "#)
    .unwrap_err();
    assert!(err.to_string().contains("is a table, not a plot"), "{err}");
}

#[test]
fn reports_follow_column_edits_and_guard_their_columns() {
    let out = run(r#"
        let t = open_sample("Norris");
        t.add_column("Extra", t.column("Reading"));
        let r = t.analyze("linear_model", #{ response: "Extra", predictors: ["Reference"] });
        t.rename_column("Extra", "Copy");
        print(t.reports()[0].title);
        try { t.remove_column("Copy"); } catch (e) { print(e); }
    "#)
    .unwrap();
    assert_eq!(out[0], "Linear Model: Copy ~ Reference");
    assert!(out[1].contains("used by 1 report"), "{}", out[1]);
}

#[test]
fn errors_name_the_line_and_the_problem() {
    let e = run("let t = open_sample(\"Norris\");\n\nt.column(\"Nope\");").unwrap_err();
    assert_eq!(e.line, Some(3));
    assert!(e.message.contains("no column \"Nope\""), "{}", e.message);

    let e =
        run("let t = open_sample(\"Norris\");\nt.analyze(\"linear_model\", #{ response: \"Nope\", predictors: [] });")
            .unwrap_err();
    assert_eq!(e.line, Some(2));
    assert!(e.message.contains("missing column \"Nope\""), "{}", e.message);

    let e = run("let x = ;").unwrap_err();
    assert_eq!(e.line, Some(1));
    assert!(stat_script::check_syntax("let x = ;").is_err());
    assert!(stat_script::check_syntax("let x = 1;").is_ok());

    let e = run("fn f(t) { t.column(\"Nope\") }\nlet t = open_sample(\"Norris\");\nf(t);").unwrap_err();
    assert_eq!(e.line, Some(1), "the error is placed inside the function");
    assert!(e.message.contains("in function f"), "{}", e.message);

    let e = run(r#"open_sample("Norris").analyze("no_such_analysis", #{})"#).unwrap_err();
    assert!(e.message.contains("unknown analysis \"no-such-analysis\""), "{}", e.message);
}

#[test]
fn limits_cancellation_and_file_access_are_enforced() {
    let limits = Limits { max_operations: 10_000, ..Limits::default() };
    let e = run_with("let x = 0; loop { x += 1; }", &RunOptions { limits, ..RunOptions::default() }).unwrap_err();
    assert!(e.message.to_lowercase().contains("operations"), "{}", e.message);

    let cancel = Arc::new(AtomicBool::new(true));
    let e = run_with("let x = 0; loop { x += 1; }", &RunOptions { cancel: Some(cancel), ..RunOptions::default() })
        .unwrap_err();
    assert!(e.cancelled);

    let limits = Limits { max_call_levels: 8, ..Limits::default() };
    assert!(run_with("fn f(n) { f(n + 1) } f(0);", &RunOptions { limits, ..RunOptions::default() }).is_err());

    assert!(run(r#"import "anything" as m;"#).is_err(), "modules cannot be imported");
    assert!(run(r#"eval("1 + 1")"#).is_err(), "eval is disabled");
    assert!(run(r#"open_table("missing-file.csv")"#).unwrap_err().message.contains("could not read"));
}

#[test]
fn json_helpers_and_arguments() {
    let options = RunOptions { args: vec!["alpha".into(), "2".into()], ..RunOptions::default() };
    let out = run_with(
        r#"
        print(args);
        let m = parse_json(`{"a": [1, 2.5, null], "b": "x"}`);
        print(m.a[1]);
        print(to_json(#{ k: [1, ()] }).contains("null"));
        "#,
        &options,
    )
    .unwrap();
    assert_eq!(out, ["[\"alpha\", \"2\"]", "2.5", "true"]);
}

#[test]
fn table_values_are_shared_like_references() {
    let engine = new_engine(&RunOptions::default(), |_| {});
    let t: Table = engine.eval(r#"let a = new_table("T"); let b = a; b.add_column("x", [1, 2]); a"#).unwrap();
    assert_eq!(t.project().table.n_cols(), 1);
}

#[test]
fn the_headless_runner_runs_a_script_file() {
    let dir = scratch("runner");
    std::fs::write(
        dir.join("fit.rhai"),
        r#"let t = open_sample(args[0]);
           let r = t.analyze("linear_model", #{ response: "Reading", predictors: ["Reference"] });
           print(r.title);
           t.save("out.csv");"#,
    )
    .unwrap();
    let exe = env!("CARGO_BIN_EXE_anovacraft-run");
    let out = std::process::Command::new(exe).arg(dir.join("fit.rhai")).arg("Norris").output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "Linear Model: Reading ~ Reference\n");
    assert!(dir.join("out.csv").exists(), "relative paths start at the script's directory");

    std::fs::write(dir.join("bad.rhai"), "let x = 1;\nopen_sample(\"Norris\").column(\"Nope\");").unwrap();
    let out = std::process::Command::new(exe).arg(dir.join("bad.rhai")).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("line 2"), "{}", String::from_utf8_lossy(&out.stderr));

    let out = std::process::Command::new(exe).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let _ = std::fs::remove_dir_all(dir);
}

/// `--licences` prints the third-party notices built into the runner.
#[test]
fn the_headless_runner_prints_its_licences() {
    let exe = env!("CARGO_BIN_EXE_anovacraft-run");
    let out = std::process::Command::new(exe).arg("--licences").output().unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.starts_with("AnovaCraft: third-party notices"), "{}", &text[..200.min(text.len())]);
    assert!(text.contains("rhai ") && text.contains("VADER"));
}

/// `print` goes to standard output and `debug` to standard error, as the
/// runner's documentation says (both went to standard output).
#[test]
fn the_headless_runner_sends_debug_output_to_standard_error() {
    let dir = scratch("runner-debug");
    std::fs::write(dir.join("d.rhai"), "print(\"out\");\ndebug(\"dbg\");").unwrap();
    let exe = env!("CARGO_BIN_EXE_anovacraft-run");
    let out = std::process::Command::new(exe).arg(dir.join("d.rhai")).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "out\n");
    assert_eq!(String::from_utf8_lossy(&out.stderr), "[line 2] \"dbg\"\n");
    let _ = std::fs::remove_dir_all(dir);
}

/// `--results` with an extension the runner cannot write is a usage error
/// naming the accepted extensions (it silently wrote CSV), and a known
/// extension still writes that format.
#[test]
fn the_headless_runner_rejects_an_unknown_results_extension() {
    let dir = scratch("runner-results");
    let data = dir.join("data");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join("a.csv"), "x\n1\n2\n").unwrap();
    std::fs::write(dir.join("b.rhai"), "#{ rows: open_table(batch_file).rows() }").unwrap();
    let exe = env!("CARGO_BIN_EXE_anovacraft-run");
    let run = |results: &str| {
        std::process::Command::new(exe)
            .args(["--batch", &data.display().to_string(), "--results", &dir.join(results).display().to_string()])
            .arg(dir.join("b.rhai"))
            .output()
            .unwrap()
    };
    for name in ["out.txt", "out"] {
        let out = run(name);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(2), "{name}: {stderr}");
        assert!(stderr.contains(".csv, .tsv, .json or .xlsx"), "{name}: {stderr}");
        assert!(!dir.join(name).exists(), "{name} was written");
    }
    let out = run("OUT.TSV");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let tsv = std::fs::read_to_string(dir.join("OUT.TSV")).unwrap();
    assert!(tsv.lines().next().unwrap().contains('\t'), "{tsv}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn example_scripts_run() {
    let examples = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/scripts");
    let dir = scratch("examples");
    let options = RunOptions { args: vec![dir.display().to_string()], ..RunOptions::default() };
    let run_example = |name: &str| {
        let lines = Rc::new(RefCell::new(Vec::new()));
        let sink = Rc::clone(&lines);
        stat_script::run_file(&examples.join(name), &options, move |s| sink.borrow_mut().push(s.to_owned()))
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        lines.take()
    };

    let out = run_example("influential_rows.rhai");
    assert!(out.iter().any(|l| l == "Excluding rows [17, 44]"), "{out:?}");
    for f in ["all_rows.md", "outliers_excluded.md", "estimates.csv", "plant_growth_refit.acraft"] {
        assert!(dir.join(f).exists(), "{f}");
    }

    run_example("batch_summaries.rhai");
    let csv = std::fs::read_to_string(dir.join("summaries.csv")).unwrap();
    assert!(csv.starts_with("File,Column,Mean,Std Dev"), "{csv}");
    assert!(csv.contains("Speed (1000 km/s)"), "{csv}");

    let out = run_example("model_outputs.rhai");
    assert!(out.iter().any(|l| l == "Added [\"Predicted Reading\", \"Residual Reading\"]"), "{out:?}");
    for f in ["norris_fit.svg", "norris_residuals.pdf", "norris_with_residuals.xlsx", "norris_model.acraft"] {
        assert!(dir.join(f).exists(), "{f}");
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn building_large_arrays_takes_linear_time() {
    // With a Rhai size limit set, every push re-measures the whole array and
    // this takes tens of seconds; without one, well under a second.
    let start = std::time::Instant::now();
    let out = run("let x = []; for i in 0..100_000 { x.push(`v${i}`); } let t = new_table(\"T\"); t.add_column(\"x\", x); print(t.rows());")
        .unwrap();
    assert_eq!(out, ["100000"]);
    assert!(start.elapsed().as_secs_f64() < 5.0, "took {:?}", start.elapsed());
}

#[test]
fn size_limits_apply_when_set() {
    let limits = Limits { max_array_size: 100, max_string_size: 50, ..Limits::default() };
    let options = RunOptions { limits, ..RunOptions::default() };
    assert!(run_with("let x = []; for i in 0..101 { x.push(i); }", &options).is_err());
    assert!(run_with("let s = \"\"; for i in 0..51 { s += \"a\"; }", &options).is_err());
    assert!(run_with("let x = []; for i in 0..100 { x.push(i); }", &options).is_ok());
}

#[test]
fn the_benchmark_example_runs() {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/scripts/benchmark.rhai");
    let lines = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&lines);
    let options = RunOptions { args: vec!["2000".into()], ..RunOptions::default() };
    stat_script::run_file(&script, &options, move |s| sink.borrow_mut().push(s.to_owned())).unwrap();
    let out = lines.take();
    assert_eq!(out.len(), 10, "{out:?}");
    assert!(out.iter().all(|l| l.ends_with(" ms")), "{out:?}");
}

#[test]
fn reports_save_their_columns_to_the_table() {
    let out = run(r#"
        let t = open_sample("Norris");
        let r = t.analyze("linear_model", #{ response: "Reading", predictors: ["Reference"] });
        print(r.saveable_columns().map(|c| c.key));
        print(r.save_columns(["residual"]));
        print(r.save_columns());
        let res = t.column("Residual Reading");
        let fit = t.column("Predicted Reading");
        let y = t.column("Reading");
        let worst = 0.0;
        for i in 0..t.rows() {
            worst = max(worst, abs(y[i] - fit[i] - res[i]));
        }
        print(worst < 1e-9);
        // The prediction formula computes the same values, and follows edits.
        let f = t.column("Predicted Reading formula");
        print(type_of(t.formula("Predicted Reading formula")) == "string" && abs(f[3] - fit[3]) < 1e-9);
        t.set_cell(3, "Reference", t.column("Reference")[3] + 1);
        print(abs(t.column("Predicted Reading formula")[3] - f[3]) > 0.5);
        try { r.save_columns(["nope"]); } catch (e) { print(e); }
        // The request names its columns: after a rename it no longer fits.
        t.rename_column("Reference", "Ref");
        try { r.save_columns(["predicted"]); } catch (e) { print(e); }
    "#)
    .unwrap();
    assert_eq!(out[0], r#"["predicted", "prediction_formula", "residual"]"#);
    assert_eq!(out[1], r#"["Residual Reading"]"#);
    assert_eq!(out[2], r#"["Predicted Reading", "Predicted Reading formula", "Residual Reading 1"]"#);
    assert_eq!(out[3..6], ["true", "true", "true"]);
    assert!(
        out[6].contains(r#"offers the columns ["predicted", "prediction_formula", "residual"]; got "nope""#),
        "{}",
        out[6]
    );
    assert!(out[7].contains("no longer fits its table"), "{}", out[7]);
}

/// Formula columns made and changed from a script recompute with the data.
#[test]
fn formula_columns_from_scripts() {
    let printed = run(r#"
        let t = new_table("t");
        t.add_column("x", [1, 2, 3]);
        t.set_formula("y", "x * 10");
        print(t.column("y"));
        t.set_cell(0, "x", 5);
        print(t.column("y"));
        print(t.formula("y"));
        t.insert_column(0, #{ name: "tag", type: "text", level: "nominal", formula: "\"r\" + row()" });
        print(t.column("tag"));
        t.rename_column("x", "x value");
        print(t.formula("y"));
        t.clear_formula("y");
        print(type_of(t.formula("y")));
        t.set_cell(1, "y", 0);
        print(t.column("y"));
    "#)
    .unwrap();
    assert_eq!(
        printed,
        [
            "[10.0, 20.0, 30.0]",
            "[50.0, 20.0, 30.0]",
            "x * 10",
            "[\"r1\", \"r2\", \"r3\"]",
            "col(\"x value\") * 10",
            "()",
            "[50.0, 0.0, 30.0]",
        ]
    );
    let refused =
        run(r#"let t = new_table("t"); t.add_column("x", [1]); t.set_formula("y", "x"); t.set_cell(0, "y", 2);"#);
    assert!(refused.unwrap_err().to_string().contains("computed by a formula"));
    let bad = run(r#"let t = new_table("t"); t.add_column("x", [1]); t.set_formula("y", "x +");"#);
    assert!(bad.unwrap_err().to_string().contains("formula of \"y\""));
}

/// `split_plot_design` makes the design table with its plot column,
/// declaration and model script; the mixed model then runs on it.
#[test]
fn split_plot_design_from_a_script() {
    let lines = run(r#"
        let t = split_plot_design(#{
            factors: [#{ name: "Temp", low: 150, high: 190, changes: "hard" }, #{ name: "Time", low: 10, high: 30 }],
            model: "Temp + Time + Temp*Time", runs: 12, whole_plots: 4, response: "Yield"
        });
        print(t.columns());
        print(t.reports().len());
        let wp = t.column("Whole Plot");
        let temp = t.column("Temp");
        let time = t.column("Time");
        let y = [];
        for i in 0..t.rows() { y.push(50.0 + temp[i] / 10.0 + time[i] / 5.0 + wp[i] * 1.3 + (i % 3) * 0.7); }
        t.set_column("Yield", y);
        let r = t.analyze("mixed-model", #{ response: "Yield", predictors: ["Temp", "Time"],
                                            random: [#{ group: "Whole Plot" }], family: "normal" });
        print(r.has_section("Tests by Fixed Effect (F tests, Satterthwaite denominator degrees of freedom)"));
    "#)
    .unwrap();
    assert_eq!(lines[0], r#"["Run", "Std order", "Whole Plot", "Temp", "Time", "Yield"]"#);
    assert_eq!(lines[1], "1", "the declared design");
    assert_eq!(lines[2], "true");
    let err = run(r#"split_plot_design(#{ factors: [#{ changes: "soft" }], runs: 4 })"#).unwrap_err();
    assert!(err.to_string().contains("change level"), "{err}");
}

/// The screening, whole-plot and simulation design functions.
#[test]
fn screening_and_design_simulation_from_a_script() {
    let lines = run(r#"
        let d = definitive_screening_design(#{ factors: 6, extra_runs: 4, center_runs: 3 });
        print(d.rows());
        let c = compare_definitive_screening(#{ factors: 6, center_runs: 3 });
        print(c.column("Runs"));
        let s = screening_design(#{ factors: [#{ name: "Temp", low: 150, high: 190 }, #{ name: "Supplier", levels: 3 },
                                             #{ name: "Line", levels: 2 }], starts: 5 });
        print(s.rows());
        print(s.level("Supplier"));
        let k = s.simulate_responses(#{ factors: ["Temp", "Line"], model: "interactions", count: 2, seed: 4 });
        print(k.rows());
        print(s.has_column("Simulated Y 1") && s.has_column("Simulated Y 2"));
        let p = split_plot_design(#{
            factors: [#{ name: "Oven", low: 150, high: 190, changes: "hard" }, #{ name: "Time", low: 10, high: 30 }],
            runs: 8, whole_plots: 4, starts: 5
        });
        let a = p.augment_whole_plots(#{ factors: ["Oven", "Time"], whole_plot: "Whole Plot", whole_plots: 2, runs: 2,
                                         starts: 5 });
        print(a.rows());
        print(a.column("Whole Plot"));
    "#)
    .unwrap();
    assert_eq!(lines[0], "19");
    assert_eq!(lines[1], "[13.0, 15.0, 17.0, 19.0, 21.0, 23.0]");
    assert_eq!(lines[2], "12", "the default run count for a 5-term model (9 rounded up to a multiple of 6)");
    assert_eq!(lines[3], "nominal");
    assert_eq!(lines[4], "2");
    assert_eq!(lines[5], "true");
    assert_eq!(lines[6], "12");
    assert!(lines[7].ends_with("5.0, 5.0, 6.0, 6.0]") || lines[7].ends_with("6.0, 6.0, 5.0, 5.0]"), "{}", lines[7]);
}

/// The chart forms are chosen by the graph JSON's `form` object, and their
/// figures are written like any graph's.
#[test]
fn chart_forms_run_from_scripts() {
    let dir = scratch("chart-forms");
    let options = RunOptions { base_dir: dir.clone(), ..RunOptions::default() };
    let out = run_with(
        r#"
        let t = open_sample("Harbour weather");
        let rose = t.analyze(#{ analysis: "graph", encoding: #{ x: ["Wind direction (°)"], y: ["Wind speed (m/s)"] },
            form: #{ kind: "wind-rose", sectors: 8, percent: false } });
        print(rose.title);
        rose.save("rose.svg");
        let b = open_sample("Household budget");
        let fall = b.analyze(#{ analysis: "graph", encoding: #{ x: ["Item"], y: ["Amount"] },
            form: #{ kind: "waterfall" } });
        print(fall.title);
        fall.save("waterfall.pdf");
        "#,
        &options,
    )
    .unwrap();
    assert_eq!(out, ["Wind Rose: Wind speed (m/s) by Wind direction (°)", "Waterfall Chart: Amount by Item"]);
    let svg = std::fs::read_to_string(dir.join("rose.svg")).unwrap();
    assert!(svg.contains("<svg") && svg.contains(">NE<"), "compass labels");
    assert!(std::fs::read(dir.join("waterfall.pdf")).unwrap().starts_with(b"%PDF"));
    let _ = std::fs::remove_dir_all(dir);
}

/// The Bayesian analyses run from scripts under their project-file keys.
#[test]
fn bayesian_analyses_from_a_script() {
    let lines = run(r#"
        let s = open_sample("Eight schools (Rubin 1981)");
        let m = s.analyze("bayesian-model", #{ response: "Estimated effect", family: "normal",
                                              known_sd: "Standard error", random: [#{ group: "School" }],
                                              chains: 2, warmup: 300, draws: 300 });
        print(m.title);
        let t = open_sample("Stroop reaction times (simulated)");
        let bf = t.analyze("bayes_factor", #{ test: "two-sample", first: "Interference", second: "Age group" });
        print(bf.title);
        let e = t.analyze("bayesian-estimation", #{ parameter: "mean", response: "Interference" });
        print(e.title);
    "#)
    .unwrap();
    assert_eq!(lines[0], "Bayesian Hierarchical Normal Model: Estimated effect");
    assert_eq!(lines[1], "Bayes Factor t Test: Interference by Age group");
    assert_eq!(lines[2], "Bayesian Estimation: Mean of Interference");
}

#[test]
fn the_api_list_names_the_registered_functions() {
    let api = stat_script::api_functions();
    let names: Vec<&str> = api.iter().map(|f| f.name.as_str()).collect();
    for expected in ["open_sample", "current_table", "analyze", "rows", "section", "p_adjust"] {
        assert!(names.contains(&expected), "{expected} missing from {names:?}");
    }
    assert!(names.iter().all(|n| !n.contains('$') && !n.starts_with(|c: char| c.is_ascii_punctuation())));
    assert!(names.windows(2).all(|w| w[0] < w[1]), "sorted and unique");
    let open = api.iter().find(|f| f.name == "open_sample").unwrap();
    assert!(open.signatures[0].starts_with("open_sample("), "{:?}", open.signatures);
    assert!(!open.signatures[0].contains("stat_script::"), "{:?}", open.signatures);
}

#[test]
fn row_subsets_are_saved_switched_and_followed_by_analyses() {
    let out = run(r#"
        let t = open_sample("Norris");
        t.save_subset("Low", |row| row.Reference < 50);
        t.save_subset("First", [0, 1, 2]);
        print(t.subset_names());
        print(t.subset_rows("First"));
        print(t.active_subset() == ());
        t.use_subset("First");
        print(t.active_subset());
        print(t.excluded_rows().len());
        t.use_all_rows();
        print(t.excluded_rows().len());
        t.remove_subset("Low");
        print(t.subset_names());
        "#)
    .unwrap();
    assert_eq!(out[0], r#"["Low", "First"]"#);
    assert_eq!(out[1], "[0, 1, 2]");
    assert_eq!(out[2], "true");
    assert_eq!(out[3], "First");
    assert_eq!(out[4], "33", "every row outside the subset is left out of analyses");
    assert_eq!(out[5], "0");
    assert_eq!(out[6], r#"["First"]"#);
    assert!(run(r#"open_sample("Norris").use_subset("None")"#).is_err());
}

/// A row test that removes rows (through a second handle on the same table)
/// used to index past the end of the table and panic; it now stops with a
/// script error.
#[test]
fn a_row_test_that_removes_rows_is_an_error() {
    for verb in ["select", "exclude"] {
        let source =
            format!("let t = open_sample(\"Norris\");\nlet u = t;\nt.{verb}(|row| {{ u.remove_rows([0]); true }});");
        let e = run(&source).unwrap_err();
        assert!(e.message.contains("must not add or remove rows"), "{verb}: {}", e.message);
    }
    // A row test that only reads the table (through another handle) still works.
    let lines = run("let t = open_sample(\"Norris\");\nlet u = t;\nprint(t.select(|row| u.rows() > 0));").unwrap();
    assert_eq!(lines, vec!["36".to_owned()]);
}
