//! The `Table` script type: a dataset with its row states and the reports
//! run on it, the same document a desktop tab holds.
//!
//! Every change goes through the application's own operations: data edits
//! are [`Edit`]s, row-state changes go through [`RowStateManager`], table
//! operations are [`TableOperation`]s read from the same JSON a project's
//! `"origin"` stores, and analyses are [`SavedReport`]s read from the same
//! JSON a project's `"reports"` store. A script therefore does exactly what
//! the corresponding menu command does.

use std::cell::RefCell;
use std::fmt;
use std::path::Path;
use std::rc::Rc;

use app_model::action::{Action, RowSubsets, color_text};
use app_model::editing::Edit;
use app_model::external::{DatabaseConnection, ExternalSource, Secrets, WebFormat, WebHeader, WebRequest};
use app_model::file_formats::{ExportFormat, FileKind, Opened, name_from_path};
use app_model::files::{export_table, open_path, open_path_with, read_sheet};
use app_model::html_export::{self, HtmlReport, HtmlSource};
use app_model::project::{PROJECT_EXTENSION, Project, ProjectScript, column_from_json, parse_color, save_project};
use app_model::report_export::{self, ExportOptions, ExportScope, PaperSize, ReportFormat};
use app_model::table_ops::{Source, TableOrigin, sort_edit};
use app_model::{SavedReport, report_from_json, run_report};
use rhai::{Array, Dynamic, Engine, FnPtr, INT, Map, NativeCallContext};
use stat_data::{DataTable, JsonValue, MeasurementLevel, RowColor, RowMarker, RowState, RowStateManager};

use crate::Host;
use crate::report::Report;
use crate::value::{ScriptResult, cell, cell_value, column_from_values, column_values, fail, script_err, to_json};

/// One open dataset: data, row states, reports and how it was made.
#[derive(Debug)]
pub(crate) struct Document {
    pub(crate) table: DataTable,
    pub(crate) rows: RowStateManager,
    /// Reports, newest first (as the application lists them).
    pub(crate) reports: Vec<SavedReport>,
    /// For each report, its position among the reports the table had when
    /// a script started on an open document (`None` for one run since).
    pub(crate) report_keys: Vec<Option<usize>>,
    pub(crate) origin: Option<TableOrigin>,
    /// The database or web source the table was read from.
    pub(crate) external: Option<app_model::external::ExternalSource>,
    /// Scripts kept with the project (carried through unchanged).
    pub(crate) scripts: Vec<ProjectScript>,
    /// The desktop document this table is (see [`crate::workspace`]); data
    /// edits are then logged in `edits` so the application can replay them.
    pub(crate) binding: Option<Binding>,
}

/// How a table bound to an open desktop document has changed.
#[derive(Debug, Default)]
pub(crate) struct Binding {
    /// The document's key in the application.
    pub(crate) key: u64,
    /// Data edits made by the script, in order.
    pub(crate) edits: Vec<Edit>,
    /// The table was replaced wholesale (not by edits).
    pub(crate) replaced: bool,
    /// The script set the selection.
    pub(crate) selection_set: bool,
    /// The script excluded or re-included rows.
    pub(crate) exclusion_set: bool,
    /// The script hid or showed rows.
    pub(crate) hidden_set: bool,
    /// The script coloured rows or removed colours.
    pub(crate) colors_set: bool,
    /// The script saved, removed or switched row subsets.
    pub(crate) subsets_set: bool,
    /// The script added, changed or closed reports.
    pub(crate) reports_changed: bool,
}

/// A table as scripts see it. Copies of a `Table` value refer to the same
/// document, like two references to one open file.
#[derive(Clone)]
pub struct Table(pub(crate) Rc<RefCell<Document>>);

impl fmt::Debug for Table {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.describe())
    }
}

impl Table {
    pub(crate) fn new(table: DataTable, rows: RowStateManager, reports: Vec<SavedReport>) -> Self {
        let report_keys = vec![None; reports.len()];
        Self(Rc::new(RefCell::new(Document {
            table,
            rows,
            reports,
            report_keys,
            origin: None,
            external: None,
            scripts: Vec::new(),
            binding: None,
        })))
    }

    /// The table, its row states and reports, as a project.
    #[must_use]
    pub fn project(&self) -> Project {
        let d = self.0.borrow();
        Project {
            table: d.table.clone(),
            rows: d.rows.clone(),
            reports: d.reports.clone(),
            origin: d.origin.clone(),
            external: d.external.clone(),
            scripts: d.scripts.clone(),
        }
    }

    fn describe(&self) -> String {
        let d = self.0.borrow();
        format!("Table {:?} ({} rows, {} columns)", d.table.name(), d.table.n_rows(), d.table.n_cols())
    }

    fn column_index(&self, name: &str) -> ScriptResult<usize> {
        let d = self.0.borrow();
        d.table.find(name).map_or_else(|| fail(format!("{:?} has no column {name:?}", d.table.name())), Ok)
    }

    /// Apply a data edit and keep the stored reports pointing at the same
    /// columns, as the desktop application does.
    pub(crate) fn apply(&self, edit: Edit) -> ScriptResult<()> {
        self.act(Action::Edit(edit))
    }

    /// Carry out an action through the shared action model (the code the
    /// application uses), noting what changed for an open document.
    pub(crate) fn act(&self, action: Action) -> ScriptResult<()> {
        let mut d = self.0.borrow_mut();
        let Document { table, rows, reports, report_keys, binding, .. } = &mut *d;
        let logged = match &action {
            Action::Edit(edit) if binding.is_some() => Some(edit.clone()),
            _ => None,
        };
        let keys = match &action {
            Action::AddReport(_) => Some((0, true)),
            Action::RemoveReport(i) => Some((*i, false)),
            _ => None,
        };
        let flag = match &action {
            Action::Edit(_) => None,
            Action::Select(_) => Some(0),
            Action::SetExcluded(_) => Some(1),
            Action::SetHidden(_) => Some(2),
            Action::SetColors(_)
            | Action::SetMarkers(_)
            | Action::ColorByColumn(_)
            | Action::SetStyleColumn(_)
            | Action::ClearRowStyles => Some(3),
            Action::SetSubsets(_) => Some(5),
            Action::AddReport(_) | Action::UpdateReport(..) | Action::RemoveReport(_) => Some(4),
        };
        action.apply(table, rows, reports).map_err(script_err)?;
        match keys {
            Some((_, true)) => report_keys.insert(0, None),
            Some((i, false)) => {
                report_keys.remove(i);
            }
            None => {}
        }
        if let Some(b) = binding.as_mut() {
            if let Some(edit) = logged {
                b.edits.push(edit);
            }
            match flag {
                Some(0) => b.selection_set = true,
                Some(1) => b.exclusion_set = true,
                Some(2) => b.hidden_set = true,
                Some(3) => b.colors_set = true,
                Some(4) => b.reports_changed = true,
                Some(5) => b.subsets_set = true,
                _ => {}
            }
        }
        Ok(())
    }

    fn rows_where(&self, ctx: &NativeCallContext, f: &FnPtr) -> ScriptResult<Vec<usize>> {
        let n = self.0.borrow().table.n_rows();
        let mut hits = Vec::new();
        for r in 0..n {
            // Borrow only while building the row, so the function may use the table.
            let row = self.row_map(r);
            let keep = f.call_within_context::<Dynamic>(ctx, (row,))?;
            // Stale row numbers would point past the end of a shrunken table.
            if self.0.borrow().table.n_rows() != n {
                return fail(
                    "the table's rows changed while rows were being tested; a row test must not add or remove rows",
                );
            }
            match keep.as_bool() {
                Ok(true) => hits.push(r),
                Ok(false) => {}
                Err(t) => return fail(format!("the row test must return true or false; it returned a {t}")),
            }
        }
        Ok(hits)
    }

    fn row_map(&self, r: usize) -> Map {
        let d = self.0.borrow();
        d.table.columns().iter().map(|c| (c.name.as_str().into(), cell(c, r))).collect()
    }

    /// Rows whose state passes `test`.
    fn rows_in(&self, test: impl Fn(&RowStateManager, usize) -> bool) -> Vec<usize> {
        let d = self.0.borrow();
        (0..d.rows.len()).filter(|&r| test(&d.rows, r)).collect()
    }

    /// The current row colours.
    fn row_colors(&self) -> Vec<(usize, RowColor)> {
        let d = self.0.borrow();
        (0..d.rows.len()).filter_map(|r| d.rows.color(r).map(|c| (r, c))).collect()
    }

    /// The current row markers.
    fn row_markers(&self) -> Vec<(usize, RowMarker)> {
        let d = self.0.borrow();
        (0..d.rows.len()).filter_map(|r| d.rows.marker(r).map(|m| (r, m))).collect()
    }

    /// A report position, checked.
    fn report_index(&self, i: INT) -> ScriptResult<usize> {
        let n = self.0.borrow().reports.len();
        match usize::try_from(i) {
            Ok(i) if i < n => Ok(i),
            _ => fail(format!("reports are numbered from 0 (newest first) to {}; got {i}", n as INT - 1)),
        }
    }

    fn row_list(&self, rows: &Array) -> ScriptResult<Vec<usize>> {
        let n = self.0.borrow().table.n_rows();
        rows.iter()
            .map(|v| match v.as_int().or_else(|_| v.as_float().map(|x| if x.fract() == 0.0 { x as INT } else { -1 })) {
                Ok(i) if i >= 0 && (i as usize) < n => Ok(i as usize),
                _ => fail(format!("row numbers run from 0 to {}; got {v}", n.saturating_sub(1))),
            })
            .collect()
    }

    /// Run a report request (JSON form) and store it with the table.
    pub(crate) fn analyze_json(&self, request: &JsonValue) -> ScriptResult<Report> {
        let saved = {
            let d = self.0.borrow();
            report_from_json(request, &d.table).map_err(|e| script_err(format!("analysis settings: {e}")))?
        };
        let report = {
            let d = self.0.borrow();
            let computed = run_report(&saved, &d.table, &d.rows).map_err(script_err)?;
            Report::new(computed, &d.table, &d.rows, Some(self.clone()))
        };
        self.act(Action::AddReport(saved))?;
        Ok(report)
    }

    /// Run a table operation (JSON form, defaults filled in) into a new table.
    fn derive(&self, request: &Map, others: &[Self]) -> ScriptResult<Self> {
        let op_name = request
            .get("operation")
            .and_then(|v| v.clone().into_string().ok())
            .ok_or_else(|| script_err("a table operation needs an \"operation\" name"))?
            .replace('_', "-");
        let mut req = request.clone();
        let docs: Vec<std::cell::Ref<'_, Document>> =
            std::iter::once(self).chain(others).map(|t| t.0.borrow()).collect();
        fill_operation_defaults(&op_name, &mut req, &docs[0].table);
        let mut sources: Array = docs.iter().map(|d| Dynamic::from(d.table.name().to_owned())).collect();
        if let Some(Ok(listed)) = req.get("sources").map(|v| v.clone().into_typed_array::<Dynamic>()) {
            sources = listed;
        }
        req.insert("operation".into(), op_name.clone().into());
        req.insert("sources".into(), Dynamic::from_array(sources));
        let name = req.remove("name").map(|n| n.to_string());
        let origin = TableOrigin::from_json(&to_json(&Dynamic::from_map(req))?)
            .map_err(|e| script_err(format!("table operation {op_name:?}: {e}")))?;
        let srcs: Vec<Source<'_>> = docs.iter().map(|d| Source { table: &d.table, rows: &d.rows }).collect();
        if origin.operation.needs_others() && srcs.len() < 2 {
            return fail(format!("table operation {op_name:?} needs other tables"));
        }
        let name = name.unwrap_or_else(|| format!("{}, {}", srcs[0].table.name(), origin.operation.past_tense()));
        let derived = origin.operation.run(srcs[0], &srcs[1..], &name).map_err(script_err)?;
        let doc = Document {
            table: derived.table,
            rows: derived.rows,
            reports: Vec::new(),
            report_keys: Vec::new(),
            origin: Some(derived.origin),
            external: None,
            scripts: Vec::new(),
            binding: None,
        };
        Ok(Self(Rc::new(RefCell::new(doc))))
    }

    /// Write the data (CSV, TSV, JSON, `.xlsx`) or the whole project
    /// (`.acraft`).
    fn save(&self, path: &Path) -> ScriptResult<()> {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        if ext == PROJECT_EXTENSION {
            let text = save_project(&self.project());
            return std::fs::write(path, text)
                .map_err(|e| script_err(format!("could not write {}: {e}", path.display())));
        }
        let ext = if ext == "tab" { "tsv".to_owned() } else { ext };
        let Some(format) = ExportFormat::ALL.into_iter().find(|f| f.extension() == ext) else {
            return fail(format!("cannot save a table as {ext:?}; use .csv, .tsv, .json, .xlsx or .acraft"));
        };
        export_table(&self.0.borrow().table, format, path).map_err(script_err)
    }

    /// Write the data as [`Table::save`] does, with columns that have value
    /// labels written as their labels.
    fn save_labelled(&self, path: &Path) -> ScriptResult<()> {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        let ext = if ext == "tab" { "tsv".to_owned() } else { ext };
        let Some(format) = ExportFormat::ALL.into_iter().find(|f| f.extension() == ext) else {
            return fail(format!("cannot write a table as {ext:?}; use .csv, .tsv, .json or .xlsx"));
        };
        let d = self.0.borrow();
        let labelled = app_model::files::with_value_labels(&d.table);
        export_table(labelled.as_ref().unwrap_or(&d.table), format, path).map_err(script_err)
    }

    /// Write every report of the table, in display order, to one document
    /// whose format the extension names: `.pdf`, `.docx` (Word), `.pptx`
    /// (PowerPoint), `.html` (or `.htm`), `.md` (plots as PNG files in a
    /// `<name>_files` folder beside it) or `.xlsx` (a sheet per report); see
    /// [`app_model::report_export`]. `paper` sizes PDF and Word pages. A
    /// report that cannot be computed appears with its error.
    fn save_reports(&self, path: &Path, paper: PaperSize) -> ScriptResult<()> {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        let Some(format) = ReportFormat::from_extension(&ext) else {
            return fail(format!("save_reports writes .pdf, .docx, .pptx, .html, .md or .xlsx; got {ext:?}"));
        };
        let d = self.0.borrow();
        let computed: Vec<(SavedReport, Result<app_model::ComputedReport, String>)> = d
            .reports
            .iter()
            .map(|saved| (saved.clone(), run_report(saved, &d.table, &d.rows).map_err(|e| e.to_string())))
            .collect();
        let pages: Vec<HtmlReport<'_>> = computed
            .iter()
            .map(|(saved, result)| match result {
                Ok(report) => HtmlReport::of(report),
                Err(message) => HtmlReport { title: "Report", saved, sections: Err(message), closed: &[] },
            })
            .collect();
        let date = html_export::export_date();
        let source = HtmlSource { table_name: d.table.name(), rows: &d.rows, date: &date, data: Some(&d.table) };
        report_export::export(ExportScope::All(&pages), source, format, ExportOptions { paper }, path)
            .map(|_| ())
            .map_err(script_err)
    }
}

/// Fill the optional settings of a table operation with the dialog's
/// defaults, and accept the shorter forms scripts use.
fn fill_operation_defaults(op: &str, req: &mut Map, table: &DataTable) {
    let mut default = |key: &str, value: Dynamic| {
        req.entry(key.into()).or_insert(value);
    };
    let all_columns = || Dynamic::from_array(table.columns().iter().map(|c| Dynamic::from(c.name.clone())).collect());
    match op {
        "remove-duplicates" => default("columns", Dynamic::from_array(Array::new())),
        "subset" => {
            default("rows", "all".into());
            default("columns", all_columns());
            default("sample", Dynamic::UNIT);
        }
        "combine-rows" => default("source_column", Dynamic::UNIT),
        "join" => {
            default("kind", "inner".into());
            default("relationship", "any".into());
        }
        "transpose" => default("label", Dynamic::UNIT),
        "multiple-imputation" => default("seed", "1".into()),
        "summarize" => {
            default("groups", Dynamic::from_array(Array::new()));
            default("statistics", Dynamic::from_array(vec!["n".into(), "mean".into(), "sd".into()]));
            default("quantile_type", INT::from(stat_core::descriptive::QuantileMethod::default().type_number()).into());
            default("include_excluded", false.into());
        }
        _ => {}
    }
    // Sort keys may be plain column names (ascending).
    if let Some(keys) = req.get_mut("keys").filter(|_| op == "sort") {
        if let Ok(list) = keys.clone().into_array() {
            *keys = Dynamic::from_array(list.into_iter().map(sort_key).collect());
        }
    }
    // Join keys may be plain names shared by both tables.
    if let Some(keys) = req.get_mut("keys").filter(|_| op == "join") {
        if let Ok(list) = keys.clone().into_array() {
            *keys = Dynamic::from_array(
                list.into_iter()
                    .map(|k| if k.is_string() { Dynamic::from_array(vec![k.clone(), k]) } else { k })
                    .collect(),
            );
        }
    }
    // Seeds are stored as text (JSON numbers hold only 53 bits exactly).
    if let Some(seed) = req.get_mut("seed").filter(|s| s.is_int()) {
        *seed = seed.to_string().into();
    }
    if let Some(sample) = req.get_mut("sample") {
        if let Some(mut m) = sample.clone().try_cast::<Map>() {
            if let Some(seed) = m.get_mut("seed").filter(|s| s.is_int()) {
                *seed = seed.to_string().into();
            }
            m.entry("strata".into()).or_insert_with(|| Dynamic::from_array(Array::new()));
            *sample = Dynamic::from_map(m);
        }
    }
}

/// A sort key: `"name"` (ascending) or `#{column: "name", descending: true}`.
fn sort_key(key: Dynamic) -> Dynamic {
    if key.is_string() {
        let mut m = Map::new();
        m.insert("column".into(), key);
        m.insert("descending".into(), false.into());
        Dynamic::from_map(m)
    } else {
        key
    }
}

fn parse_level(name: &str) -> ScriptResult<MeasurementLevel> {
    match name {
        "continuous" => Ok(MeasurementLevel::Continuous),
        "nominal" => Ok(MeasurementLevel::Nominal),
        "ordinal" => Ok(MeasurementLevel::Ordinal),
        _ => fail(format!("measurement levels are \"continuous\", \"nominal\" and \"ordinal\"; got {name:?}")),
    }
}

fn level_name(level: MeasurementLevel) -> &'static str {
    match level {
        MeasurementLevel::Continuous => "continuous",
        MeasurementLevel::Nominal => "nominal",
        MeasurementLevel::Ordinal => "ordinal",
    }
}

/// Open a data or project file as tables: one per non-empty sheet of a
/// workbook, exactly one for any other file.
pub(crate) fn open_tables(path: &Path) -> ScriptResult<Vec<Table>> {
    let opened = open_path(path).map_err(script_err)?;
    Ok(opened
        .into_iter()
        .map(|o| match o {
            Opened::Project(p) => {
                let Project { table, rows, reports, origin, external, scripts } = *p;
                let t = Table::new(table, rows, reports);
                {
                    let mut d = t.0.borrow_mut();
                    d.origin = origin;
                    d.external = external;
                    d.scripts = scripts;
                }
                t
            }
            Opened::Table(table, _) => {
                let rows = RowStateManager::new(table.n_rows());
                Table::new(table, rows, Vec::new())
            }
        })
        .collect())
}

/// Open a data or project file as a table: a workbook's first non-empty
/// sheet, or the sheet named `sheet`.
pub(crate) fn open_table(path: &Path, sheet: Option<&str>) -> ScriptResult<Table> {
    match sheet {
        None => Ok(open_tables(path)?.swap_remove(0)),
        Some(sheet) => {
            if FileKind::from_path(path) != Some(FileKind::Spreadsheet) {
                return fail(format!("{} is not a workbook, so it has no sheets", path.display()));
            }
            let (table, _) = read_sheet(path, sheet, &name_from_path(path)).map_err(script_err)?;
            let rows = RowStateManager::new(table.n_rows());
            Ok(Table::new(table, rows, Vec::new()))
        }
    }
}

/// Open a text file or workbook with Open Data settings given as a map (see
/// [`open_path_with`]); a workbook gives its first chosen sheet, or the
/// stacked table.
pub(crate) fn open_table_with(path: &Path, options: &Map) -> ScriptResult<Table> {
    let options = to_json(&Dynamic::from_map(options.clone()))?;
    let mut opened = open_path_with(path, &options).map_err(script_err)?;
    let Opened::Table(table, _) = opened.swap_remove(0) else {
        return fail(format!("{} is a project, which takes no open options", path.display()));
    };
    let rows = RowStateManager::new(table.n_rows());
    Ok(Table::new(table, rows, Vec::new()))
}

/// Read an external source into a table that remembers the source.
fn read_external(source: ExternalSource, secrets: &Secrets) -> ScriptResult<Table> {
    let (table, _) = source.read(secrets, &source.suggested_name()).map_err(script_err)?;
    let rows = RowStateManager::new(table.n_rows());
    let t = Table::new(table, rows, Vec::new());
    t.0.borrow_mut().external = Some(source);
    Ok(t)
}

/// `read_database` settings: the connection (see
/// [`DatabaseConnection::from_json`]), `password` and `query` (or `table`,
/// read whole).
fn database_settings(settings: Map) -> ScriptResult<(DatabaseConnection, String, String)> {
    let v = to_json(&Dynamic::from_map(settings))?;
    let connection = DatabaseConnection::from_json(&v).map_err(script_err)?;
    let text = |k: &str| v.get(k).and_then(JsonValue::as_str).unwrap_or_default().to_owned();
    let query = match (text("query"), text("table")) {
        (q, _) if !q.trim().is_empty() => q,
        (_, t) if !t.is_empty() => connection.kind.select_all(&t),
        _ => String::new(),
    };
    Ok((connection, text("password"), query))
}

/// `read_url` options: `headers` and `secret_headers` (maps of name to
/// value; secret ones are never saved in projects), `format` (`"auto"`,
/// `"json"`, `"delimited"` or `"html"`), `json_path` and `table` (which
/// table of a web page: a number from 1 or an id).
fn web_settings(url: &str, options: Map) -> ScriptResult<(WebRequest, Secrets)> {
    let v = to_json(&Dynamic::from_map(options))?;
    let mut request = WebRequest::new(url);
    let mut secrets = Secrets::default();
    for (key, secret) in [("headers", false), ("secret_headers", true)] {
        if let Some(members) = v.get(key).and_then(JsonValue::as_object) {
            for (name, value) in members {
                let value = value.as_str().map_or_else(|| value.to_compact(), str::to_owned);
                let secret = secret || app_model::external::is_secret_header(name);
                if secret {
                    secrets.headers.push((name.clone(), value.clone()));
                }
                request.headers.push(WebHeader { name: name.clone(), value, secret });
            }
        }
    }
    request.format = match v.get("format").and_then(JsonValue::as_str) {
        None | Some("auto") => WebFormat::Auto,
        Some("json") => WebFormat::Json,
        Some("delimited" | "csv") => WebFormat::Delimited,
        Some("html") => WebFormat::Html,
        Some(other) => {
            return fail(format!("unknown format {other:?}: use \"auto\", \"json\", \"delimited\" or \"html\""));
        }
    };
    request.json_path = v.get("json_path").and_then(JsonValue::as_str).unwrap_or_default().to_owned();
    request.html_table = match v.get("table") {
        Some(JsonValue::String(t)) => t.clone(),
        Some(JsonValue::Number(n)) if n.fract() == 0.0 && *n >= 1.0 => format!("{n:.0}"),
        _ => String::new(),
    };
    Ok((request, secrets))
}

/// Register the `Table` type and the table functions.
pub(crate) fn register(engine: &mut Engine, host: &Rc<Host>) {
    engine.register_type_with_name::<Table>("Table");
    engine.register_fn("to_string", |t: &mut Table| t.describe());
    engine.register_fn("to_debug", |t: &mut Table| t.describe());

    // Opening and creating tables.
    let h = Rc::clone(host);
    engine.register_fn("open_table", move |path: &str| open_table(&h.resolve(path), None));
    let h = Rc::clone(host);
    engine.register_fn("open_table", move |path: &str, sheet: &str| open_table(&h.resolve(path), Some(sheet)));
    let h = Rc::clone(host);
    engine.register_fn("open_table", move |path: &str, options: Map| open_table_with(&h.resolve(path), &options));
    let h = Rc::clone(host);
    engine.register_fn("open_workbook", move |path: &str| -> ScriptResult<Array> {
        Ok(open_tables(&h.resolve(path))?.into_iter().map(Dynamic::from).collect())
    });
    let h = Rc::clone(host);
    engine.register_fn("open_project", move |path: &str| -> ScriptResult<Table> {
        let path = h.resolve(path);
        if FileKind::from_path(&path) != Some(FileKind::Project) {
            return fail(format!("{} is not a project (.{PROJECT_EXTENSION}) file", path.display()));
        }
        open_table(&path, None)
    });
    engine.register_fn("read_database", |settings: Map| -> ScriptResult<Table> {
        let (connection, password, query) = database_settings(settings)?;
        let source = ExternalSource::Database { connection, query };
        let secrets = Secrets { password, ..Secrets::default() };
        read_external(source, &secrets)
    });
    engine.register_fn("database_tables", |settings: Map| -> ScriptResult<Array> {
        let (connection, password, _) = database_settings(settings)?;
        let tables = app_model::external::list_tables(&connection, &password).map_err(script_err)?;
        Ok(tables.into_iter().map(Dynamic::from).collect())
    });
    engine.register_fn("read_url", |url: &str| -> ScriptResult<Table> {
        read_external(ExternalSource::Web(WebRequest::new(url)), &Secrets::default())
    });
    engine.register_fn("read_url", |url: &str, options: Map| -> ScriptResult<Table> {
        let (request, secrets) = web_settings(url, options)?;
        read_external(ExternalSource::Web(request), &secrets)
    });
    engine.register_fn("new_table", |name: &str| {
        Table::new(DataTable::new(name, Vec::new()).expect("an empty table"), RowStateManager::new(0), Vec::new())
    });
    engine.register_fn("open_sample", |name: &str| -> ScriptResult<Table> {
        let wanted = name.to_lowercase();
        let catalog = app_model::sample::catalog();
        let info = catalog
            .iter()
            .find(|s| s.name.to_lowercase().starts_with(&wanted))
            .or_else(|| catalog.iter().find(|s| s.name.to_lowercase().contains(&wanted)))
            .ok_or_else(|| script_err(format!("no sample dataset is called {name:?}")))?;
        let s = (info.build)();
        let rows = RowStateManager::new(s.table.n_rows());
        Ok(Table::new(s.table, rows, s.reports))
    });

    engine.register_fn("split_plot_design", |settings: Map| -> ScriptResult<Table> {
        let json = to_json(&Dynamic::from_map(settings))?;
        let request = app_model::doe::split_plot_request_from_json(&json).map_err(script_err)?;
        let (project, _) = app_model::doe::design_project(&request).map_err(script_err)?;
        let t = Table::new(project.table, project.rows, project.reports);
        t.0.borrow_mut().scripts = project.scripts;
        Ok(t)
    });

    fn design_table(request: &app_model::doe::DesignRequest) -> ScriptResult<Table> {
        let (project, _) = app_model::doe::design_project(request).map_err(script_err)?;
        Ok(Table::new(project.table, project.rows, project.reports))
    }
    engine.register_fn("definitive_screening_design", |settings: Map| -> ScriptResult<Table> {
        let json = to_json(&Dynamic::from_map(settings))?;
        design_table(&app_model::doe::definitive_screening_request_from_json(&json).map_err(script_err)?)
    });
    engine.register_fn("compare_definitive_screening", |settings: Map| -> ScriptResult<Table> {
        let json = to_json(&Dynamic::from_map(settings))?;
        let request = app_model::doe::definitive_screening_request_from_json(&json).map_err(script_err)?;
        let app_model::doe::DesignKind::DefinitiveScreening { extra_runs } = request.kind else {
            return fail("not a definitive screening request");
        };
        let chosen = app_model::doe::DsdOptions { extra_runs, center_runs: request.center_points.unwrap_or(1) };
        let t = app_model::doe::dsd_comparison_table(request.factors.len(), chosen).map_err(script_err)?;
        let rows = RowStateManager::new(t.n_rows());
        Ok(Table::new(t, rows, Vec::new()))
    });
    engine.register_fn("screening_design", |settings: Map| -> ScriptResult<Table> {
        let json = to_json(&Dynamic::from_map(settings))?;
        design_table(&app_model::doe::categorical_screening_request_from_json(&json).map_err(script_err)?)
    });
    engine.register_fn("augment_whole_plots", |t: &mut Table, settings: Map| -> ScriptResult<Table> {
        let json = to_json(&Dynamic::from_map(settings))?;
        let out = {
            let d = t.0.borrow();
            let request =
                app_model::design_augment::whole_plots_request_from_json(&json, &d.table).map_err(script_err)?;
            app_model::design_augment::augment_table(&d.table, &request).map_err(script_err)?
        };
        let rows = RowStateManager::new(out.table.n_rows());
        Ok(Table::new(out.table, rows, Vec::new()))
    });
    engine.register_fn("simulate_responses", |t: &mut Table, settings: Map| -> ScriptResult<Table> {
        let json = to_json(&Dynamic::from_map(settings))?;
        let out = {
            let d = t.0.borrow();
            let request =
                app_model::design_simulation::simulate_request_from_json(&json, &d.table).map_err(script_err)?;
            app_model::design_simulation::simulate_design_responses(&d.table, &request).map_err(script_err)?
        };
        let n = t.0.borrow().table.n_cols();
        let edits: Vec<Edit> = out
            .columns
            .into_iter()
            .enumerate()
            .map(|(i, column)| Edit::InsertColumn { index: n + i, column })
            .collect();
        t.apply(Edit::Batch { label: "add simulated responses".to_owned(), edits })?;
        let rows = RowStateManager::new(out.coefficients.n_rows());
        Ok(Table::new(out.coefficients, rows, Vec::new()))
    });

    // Shape and values.
    engine.register_get("name", |t: &mut Table| t.0.borrow().table.name().to_owned());
    engine.register_fn("rows", |t: &mut Table| t.0.borrow().table.n_rows() as INT);
    engine.register_fn("columns", |t: &mut Table| -> Array {
        t.0.borrow().table.columns().iter().map(|c| c.name.clone().into()).collect()
    });
    engine.register_fn("has_column", |t: &mut Table, name: &str| t.0.borrow().table.find(name).is_some());
    engine.register_fn("column", |t: &mut Table, name: &str| -> ScriptResult<Array> {
        let j = t.column_index(name)?;
        Ok(column_values(&t.0.borrow().table.columns()[j]))
    });
    engine.register_fn("level", |t: &mut Table, name: &str| -> ScriptResult<String> {
        let j = t.column_index(name)?;
        Ok(level_name(t.0.borrow().table.columns()[j].level).to_owned())
    });
    engine.register_fn("row", |t: &mut Table, r: INT| -> ScriptResult<Map> {
        let r = t.row_list(&vec![r.into()])?[0];
        Ok(t.row_map(r))
    });
    engine.register_fn("cell", |t: &mut Table, r: INT, name: &str| -> ScriptResult<Dynamic> {
        let j = t.column_index(name)?;
        let r = t.row_list(&vec![r.into()])?[0];
        Ok(cell(&t.0.borrow().table.columns()[j], r))
    });

    // Data edits.
    engine.register_fn("set_cell", |t: &mut Table, r: INT, name: &str, value: Dynamic| -> ScriptResult<()> {
        let j = t.column_index(name)?;
        let r = t.row_list(&vec![r.into()])?[0];
        let value = cell_value(&t.0.borrow().table.columns()[j], &value)?;
        t.apply(Edit::SetCell { row: r, column: j, value })
    });
    fn add_column(t: &Table, name: &str, values: &Array, level: Option<&str>) -> ScriptResult<()> {
        let mut column = column_from_values(name, values);
        if let Some(level) = level {
            column.level = parse_level(level)?;
            if column.level == MeasurementLevel::Continuous && column.as_numeric().is_none() {
                return fail(format!("text column {name:?} cannot be continuous"));
            }
        }
        let empty = t.0.borrow().table.n_cols() == 0;
        if empty {
            // The first column sets the number of rows.
            let mut d = t.0.borrow_mut();
            let n = column.data.len();
            d.table = DataTable::new(d.table.name().to_owned(), vec![column]).map_err(script_err)?;
            d.rows = RowStateManager::new(n);
            if let Some(b) = &mut d.binding {
                b.replaced = true;
            }
            return Ok(());
        }
        let (n_rows, n_cols, taken) = {
            let d = t.0.borrow();
            (d.table.n_rows(), d.table.n_cols(), d.table.find(name).is_some())
        };
        if taken {
            return fail(format!("there is already a column {name:?}; use set_column to replace it"));
        }
        if values.len() != n_rows {
            return fail(format!("the table has {n_rows} rows; got {} values", values.len()));
        }
        t.apply(Edit::InsertColumn { index: n_cols, column })
    }
    engine.register_fn("add_column", |t: &mut Table, name: &str, values: Array| add_column(t, name, &values, None));
    engine.register_fn("add_column", |t: &mut Table, name: &str, values: Array, level: &str| {
        add_column(t, name, &values, Some(level))
    });
    engine.register_fn("set_column", |t: &mut Table, name: &str, values: Array| -> ScriptResult<()> {
        let j = t.column_index(name)?;
        let (n_rows, level) = {
            let d = t.0.borrow();
            (d.table.n_rows(), d.table.columns()[j].level)
        };
        if values.len() != n_rows {
            return fail(format!("the table has {n_rows} rows; got {} values", values.len()));
        }
        let mut column = column_from_values(name, &values);
        if column.as_numeric().is_some() || level != MeasurementLevel::Continuous {
            column.level = level;
        }
        t.apply(Edit::ReplaceColumn { index: j, column })
    });
    engine.register_fn("set_formula", |t: &mut Table, name: &str, formula: &str| -> ScriptResult<()> {
        let (found, n_rows, n_cols) = {
            let d = t.0.borrow();
            (d.table.find(name), d.table.n_rows(), d.table.n_cols())
        };
        match found {
            Some(j) => t.apply(Edit::SetFormula { index: j, formula: Some(formula.to_owned()) }),
            None => {
                if n_cols == 0 {
                    return fail("add a column of values first: the table has no rows");
                }
                let mut column = stat_data::Column::empty(name, stat_data::ColumnKind::Numeric, n_rows);
                column.formula = Some(formula.to_owned());
                t.apply(Edit::InsertColumn { index: n_cols, column })
            }
        }
    });
    engine.register_fn("formula", |t: &mut Table, name: &str| -> ScriptResult<Dynamic> {
        let j = t.column_index(name)?;
        Ok(t.0.borrow().table.columns()[j].formula.clone().map_or(Dynamic::UNIT, Dynamic::from))
    });
    engine.register_fn("clear_formula", |t: &mut Table, name: &str| -> ScriptResult<()> {
        let j = t.column_index(name)?;
        t.apply(Edit::SetFormula { index: j, formula: None })
    });
    engine.register_fn("remove_column", |t: &mut Table, name: &str| -> ScriptResult<()> {
        let j = t.column_index(name)?;
        let users =
            t.0.borrow()
                .reports
                .iter()
                .filter(|r| r.columns().contains(&j) || r.column_names().contains(&name))
                .count();
        if users > 0 {
            return fail(format!("column {name:?} is used by {users} report(s) on this table"));
        }
        t.apply(Edit::RemoveColumn { index: j })
    });
    engine.register_fn("rename_column", |t: &mut Table, old: &str, new: &str| -> ScriptResult<()> {
        let j = t.column_index(old)?;
        t.apply(Edit::RenameColumn { index: j, name: new.to_owned() })
    });
    engine.register_fn("rename_table", |t: &mut Table, new: &str| -> ScriptResult<()> {
        t.apply(Edit::RenameTable { name: new.to_owned() })
    });
    engine.register_fn("set_level", |t: &mut Table, name: &str, level: &str| -> ScriptResult<()> {
        let j = t.column_index(name)?;
        t.apply(Edit::SetLevel { index: j, level: parse_level(level)? })
    });
    engine.register_fn("set_value_labels", |t: &mut Table, name: &str, labels: Map| -> ScriptResult<()> {
        let j = t.column_index(name)?;
        let mut map = std::collections::BTreeMap::new();
        for (code, label) in labels {
            let label = label.into_string().map_err(|_| script_err("each value label must be text"))?;
            map.insert(code.to_string(), label);
        }
        t.apply(Edit::SetValueLabels { index: j, labels: map })
    });
    engine.register_fn("clear_value_labels", |t: &mut Table, name: &str| -> ScriptResult<()> {
        let j = t.column_index(name)?;
        t.apply(Edit::SetValueLabels { index: j, labels: std::collections::BTreeMap::new() })
    });
    engine.register_fn("value_labels", |t: &mut Table, name: &str| -> ScriptResult<Map> {
        let j = t.column_index(name)?;
        let d = t.0.borrow();
        Ok(d.table.columns()[j].value_labels.iter().map(|(c, l)| (c.as_str().into(), l.clone().into())).collect())
    });
    engine.register_fn("set_hover", |t: &mut Table, name: &str, hover: bool| -> ScriptResult<()> {
        let j = t.column_index(name)?;
        t.apply(Edit::SetHover { index: j, hover })
    });
    engine.register_fn("hover_columns", |t: &mut Table| -> Array {
        t.0.borrow().table.columns().iter().filter(|c| c.hover).map(|c| c.name.clone().into()).collect()
    });
    engine.register_fn("add_rows", |t: &mut Table, count: INT| -> ScriptResult<()> {
        let count = usize::try_from(count).map_err(|_| script_err("the number of rows must not be negative"))?;
        let edit = {
            let d = t.0.borrow();
            Edit::insert_empty_rows(&d.table, d.table.n_rows(), count)
        };
        t.apply(edit)
    });
    engine.register_fn("sort", |t: &mut Table, keys: Array| -> ScriptResult<()> {
        let keys: Vec<(String, bool)> = keys
            .into_iter()
            .map(|k| {
                let k = sort_key(k);
                let m = k.try_cast::<Map>().ok_or_else(|| script_err("sort keys are column names or maps"))?;
                let column =
                    m.get("column").map(ToString::to_string).ok_or_else(|| script_err("sort key without a column"))?;
                let descending = m.get("descending").and_then(|d| d.as_bool().ok()).unwrap_or(false);
                Ok((column, descending))
            })
            .collect::<ScriptResult<_>>()?;
        let edit = sort_edit(&t.0.borrow().table, &keys).map_err(script_err)?;
        t.apply(edit)
    });

    engine.register_fn("insert_rows", |t: &mut Table, at: INT, count: INT| -> ScriptResult<()> {
        let n = t.0.borrow().table.n_rows();
        let (at, count) = match (usize::try_from(at), usize::try_from(count)) {
            (Ok(a), Ok(c)) if a <= n => (a, c),
            _ => return fail(format!("rows go in at 0 to {n}, and the count must not be negative")),
        };
        let edit = Edit::insert_empty_rows(&t.0.borrow().table, at, count);
        t.apply(edit)
    });
    engine.register_fn("insert_rows", |t: &mut Table, items: Array| -> ScriptResult<()> {
        let mut rows = Vec::with_capacity(items.len());
        for item in items {
            let pair =
                item.into_typed_array::<Dynamic>().map_err(|_| script_err("each row is [position, [values…]]"))?;
            let [at, values] =
                <[Dynamic; 2]>::try_from(pair).map_err(|_| script_err("each row is [position, [values…]]"))?;
            let at = at
                .as_int()
                .ok()
                .and_then(|i| usize::try_from(i).ok())
                .ok_or_else(|| script_err("row positions are whole numbers from 0"))?;
            let values = values.into_typed_array::<Dynamic>().map_err(|_| script_err("row values are an array"))?;
            let d = t.0.borrow();
            if values.len() != d.table.n_cols() {
                return fail(format!("the table has {} columns; got {} values", d.table.n_cols(), values.len()));
            }
            let cells = d
                .table
                .columns()
                .iter()
                .zip(&values)
                .map(|(c, v)| cell_value(c, v))
                .collect::<ScriptResult<Vec<_>>>()?;
            rows.push((at, cells, RowState::default()));
        }
        if !rows.windows(2).all(|w| w[0].0 < w[1].0) {
            return fail("row positions must increase");
        }
        t.apply(Edit::InsertRows { rows })
    });
    engine.register_fn("remove_rows", |t: &mut Table, rows: Array| -> ScriptResult<()> {
        let mut rows = t.row_list(&rows)?;
        rows.sort_unstable();
        rows.dedup();
        t.apply(Edit::RemoveRows { rows })
    });
    engine.register_fn("reorder_rows", |t: &mut Table, order: Array| -> ScriptResult<()> {
        let order = t.row_list(&order)?;
        let n = t.0.borrow().table.n_rows();
        let mut seen = vec![false; n];
        if order.len() != n || order.iter().any(|&r| std::mem::replace(&mut seen[r], true)) {
            return fail(format!("the new order must list each of the {n} rows once"));
        }
        t.apply(Edit::ReorderRows { order })
    });
    fn column_arg(spec: Map) -> ScriptResult<stat_data::Column> {
        column_from_json(&to_json(&Dynamic::from_map(spec))?).map_err(|e| script_err(format!("column: {e}")))
    }
    engine.register_fn("insert_column", |t: &mut Table, index: INT, spec: Map| -> ScriptResult<()> {
        let mut column = column_arg(spec)?;
        let (n_rows, n_cols) = {
            let d = t.0.borrow();
            (d.table.n_rows(), d.table.n_cols())
        };
        if column.formula.is_some() && column.data.is_empty() {
            // A formula column given without values: they are computed.
            let level = column.level;
            column = stat_data::Column {
                level,
                formula: column.formula.take(),
                ..stat_data::Column::empty(column.name.clone(), column.kind(), n_rows)
            };
        }
        let index = match usize::try_from(index) {
            Ok(i) if i <= n_cols => i,
            _ => return fail(format!("columns go in at 0 to {n_cols}; got {index}")),
        };
        if n_cols == 0 {
            // The first column sets the number of rows.
            let mut d = t.0.borrow_mut();
            let n = column.data.len();
            d.table = DataTable::new(d.table.name().to_owned(), vec![column]).map_err(script_err)?;
            app_model::formula::refresh_all(&mut d.table);
            d.rows = RowStateManager::new(n);
            if let Some(b) = &mut d.binding {
                b.replaced = true;
            }
            return Ok(());
        }
        if column.data.len() != n_rows {
            return fail(format!("the table has {n_rows} rows; the column has {}", column.data.len()));
        }
        if t.0.borrow().table.find(&column.name).is_some() {
            return fail(format!("there is already a column {:?}", column.name));
        }
        t.apply(Edit::InsertColumn { index, column })
    });
    engine.register_fn("replace_column", |t: &mut Table, name: &str, spec: Map| -> ScriptResult<()> {
        let j = t.column_index(name)?;
        let column = column_arg(spec)?;
        let n_rows = t.0.borrow().table.n_rows();
        if column.data.len() != n_rows {
            return fail(format!("the table has {n_rows} rows; the column has {}", column.data.len()));
        }
        t.apply(Edit::ReplaceColumn { index: j, column })
    });

    // Row states (each through the shared action model).
    engine.register_fn("select", |ctx: NativeCallContext, t: &mut Table, f: FnPtr| -> ScriptResult<INT> {
        let hits = t.rows_where(&ctx, &f)?;
        let n = hits.len() as INT;
        t.act(Action::Select(hits))?;
        Ok(n)
    });
    engine.register_fn("select_rows", |t: &mut Table, rows: Array| -> ScriptResult<()> {
        let rows = t.row_list(&rows)?;
        t.act(Action::Select(rows))
    });
    engine.register_fn("clear_selection", |t: &mut Table| t.act(Action::Select(Vec::new())));
    engine.register_fn("selected_rows", |t: &mut Table| -> Array {
        t.0.borrow().rows.selected_rows().into_iter().map(|r| (r as INT).into()).collect()
    });
    fn add_to(t: &Table, more: Vec<usize>, now: impl Fn(&RowStateManager, usize) -> bool) -> Vec<usize> {
        let mut all = t.rows_in(now);
        all.extend(more);
        all.sort_unstable();
        all.dedup();
        all
    }
    engine.register_fn("exclude", |ctx: NativeCallContext, t: &mut Table, f: FnPtr| -> ScriptResult<INT> {
        let hits = t.rows_where(&ctx, &f)?;
        let n = hits.len() as INT;
        t.act(Action::SetExcluded(add_to(t, hits, |s, r| s.is_excluded_flag(r))))?;
        Ok(n)
    });
    engine.register_fn("exclude_rows", |t: &mut Table, rows: Array| -> ScriptResult<()> {
        let rows = t.row_list(&rows)?;
        t.act(Action::SetExcluded(add_to(t, rows, |s, r| s.is_excluded_flag(r))))
    });
    engine.register_fn("set_excluded", |t: &mut Table, rows: Array| -> ScriptResult<()> {
        let rows = t.row_list(&rows)?;
        t.act(Action::SetExcluded(rows))
    });
    engine.register_fn("include_all", |t: &mut Table| t.act(Action::SetExcluded(Vec::new())));
    engine.register_fn("excluded_rows", |t: &mut Table| -> Array {
        t.rows_in(|s, r| s.is_excluded(r)).into_iter().map(|r| (r as INT).into()).collect()
    });
    engine.register_fn("hide_rows", |t: &mut Table, rows: Array| -> ScriptResult<()> {
        let rows = t.row_list(&rows)?;
        t.act(Action::SetHidden(add_to(t, rows, |s, r| s.is_hidden_flag(r))))
    });
    engine.register_fn("set_hidden", |t: &mut Table, rows: Array| -> ScriptResult<()> {
        let rows = t.row_list(&rows)?;
        t.act(Action::SetHidden(rows))
    });
    engine.register_fn("unhide_all", |t: &mut Table| t.act(Action::SetHidden(Vec::new())));
    engine.register_fn("hidden_rows", |t: &mut Table| -> Array {
        t.rows_in(|s, r| s.is_hidden(r)).into_iter().map(|r| (r as INT).into()).collect()
    });
    // Saved row subsets: named sets of rows, one of which can be active.
    fn subsets(t: &Table) -> RowSubsets {
        RowSubsets::of(&t.0.borrow().rows)
    }
    engine.register_fn("save_subset", |t: &mut Table, name: &str, rows: Array| -> ScriptResult<()> {
        let rows = t.row_list(&rows)?;
        let mut s = subsets(t);
        match s.subsets.iter_mut().find(|(n, _)| n == name) {
            Some((_, list)) => *list = rows,
            None => s.subsets.push((name.to_owned(), rows)),
        }
        t.act(Action::SetSubsets(s))
    });
    engine.register_fn(
        "save_subset",
        |ctx: NativeCallContext, t: &mut Table, name: &str, f: FnPtr| -> ScriptResult<()> {
            let rows = t.rows_where(&ctx, &f)?;
            let mut s = subsets(t);
            match s.subsets.iter_mut().find(|(n, _)| n == name) {
                Some((_, list)) => *list = rows,
                None => s.subsets.push((name.to_owned(), rows)),
            }
            t.act(Action::SetSubsets(s))
        },
    );
    engine.register_fn("use_subset", |t: &mut Table, name: &str| -> ScriptResult<()> {
        let mut s = subsets(t);
        if !s.subsets.iter().any(|(n, _)| n == name) {
            return fail(format!("there is no row subset called {name:?}"));
        }
        s.active = Some(name.to_owned());
        t.act(Action::SetSubsets(s))
    });
    engine.register_fn("use_all_rows", |t: &mut Table| -> ScriptResult<()> {
        let mut s = subsets(t);
        s.active = None;
        t.act(Action::SetSubsets(s))
    });
    engine.register_fn("remove_subset", |t: &mut Table, name: &str| -> ScriptResult<()> {
        let mut s = subsets(t);
        let before = s.subsets.len();
        s.subsets.retain(|(n, _)| n != name);
        if s.subsets.len() == before {
            return fail(format!("there is no row subset called {name:?}"));
        }
        if s.active.as_deref() == Some(name) {
            s.active = None;
        }
        t.act(Action::SetSubsets(s))
    });
    engine.register_fn("subset_names", |t: &mut Table| -> Array {
        subsets(t).subsets.into_iter().map(|(n, _)| n.into()).collect()
    });
    engine.register_fn("subset_rows", |t: &mut Table, name: &str| -> ScriptResult<Array> {
        match subsets(t).subsets.into_iter().find(|(n, _)| n == name) {
            Some((_, rows)) => Ok(rows.into_iter().map(|r| (r as INT).into()).collect()),
            None => fail(format!("there is no row subset called {name:?}")),
        }
    });
    engine.register_fn("active_subset", |t: &mut Table| -> Dynamic {
        subsets(t).active.map_or(Dynamic::UNIT, Into::into)
    });
    engine.register_fn("color_rows", |t: &mut Table, rows: Array, color: &str| -> ScriptResult<()> {
        let rows = t.row_list(&rows)?;
        let Some(c) = parse_color(color) else {
            return fail(format!("colours are written \"#rrggbb\"; got {color:?}"));
        };
        let mut all: Vec<(usize, RowColor)> = t.row_colors().into_iter().filter(|(r, _)| !rows.contains(r)).collect();
        all.extend(rows.into_iter().map(|r| (r, c)));
        all.sort_unstable_by_key(|(r, _)| *r);
        all.dedup_by_key(|(r, _)| *r);
        t.act(Action::SetColors(all))
    });
    engine.register_fn("uncolor_rows", |t: &mut Table, rows: Array| -> ScriptResult<()> {
        let rows = t.row_list(&rows)?;
        let all = t.row_colors().into_iter().filter(|(r, _)| !rows.contains(r)).collect();
        t.act(Action::SetColors(all))
    });
    engine.register_fn("row_color", |t: &mut Table, r: INT| -> ScriptResult<Dynamic> {
        let r = t.row_list(&vec![r.into()])?[0];
        Ok(t.0.borrow().rows.color(r).map_or(Dynamic::UNIT, |c| color_text(c).into()))
    });
    engine.register_fn("mark_rows", |t: &mut Table, rows: Array, marker: &str| -> ScriptResult<()> {
        let rows = t.row_list(&rows)?;
        let Some(m) = RowMarker::from_key(marker) else {
            let names: Vec<&str> = RowMarker::ALL.iter().map(|m| m.key()).collect();
            return fail(format!("markers are {}; got {marker:?}", names.join(", ")));
        };
        let mut all: Vec<(usize, RowMarker)> = t.row_markers().into_iter().filter(|(r, _)| !rows.contains(r)).collect();
        all.extend(rows.into_iter().map(|r| (r, m)));
        all.sort_unstable_by_key(|(r, _)| *r);
        all.dedup_by_key(|(r, _)| *r);
        t.act(Action::SetMarkers(all))
    });
    engine.register_fn("unmark_rows", |t: &mut Table, rows: Array| -> ScriptResult<()> {
        let rows = t.row_list(&rows)?;
        let all = t.row_markers().into_iter().filter(|(r, _)| !rows.contains(r)).collect();
        t.act(Action::SetMarkers(all))
    });
    engine.register_fn("row_marker", |t: &mut Table, r: INT| -> ScriptResult<Dynamic> {
        let r = t.row_list(&vec![r.into()])?[0];
        Ok(t.0.borrow().rows.marker(r).map_or(Dynamic::UNIT, |m| m.key().into()))
    });
    engine.register_fn("color_by_column", |t: &mut Table, column: &str| -> ScriptResult<()> {
        t.act(Action::ColorByColumn(column.to_owned()))
    });
    engine.register_fn("clear_row_colors", |t: &mut Table| -> ScriptResult<()> { t.act(Action::ClearRowStyles) });
    engine.register_fn("set_style_column", |t: &mut Table, column: &str| -> ScriptResult<()> {
        t.column_index(column)?;
        t.act(Action::SetStyleColumn(Some(column.to_owned())))
    });
    engine.register_fn("set_style_column", |t: &mut Table, _none: ()| -> ScriptResult<()> {
        t.act(Action::SetStyleColumn(None))
    });
    engine.register_fn("style_column", |t: &mut Table| -> Dynamic {
        t.0.borrow().rows.style_column().map_or(Dynamic::UNIT, |c| c.to_owned().into())
    });

    // Table operations (new tables).
    engine.register_fn("derive", |t: &mut Table, request: Map| t.derive(&request, &[]));
    engine.register_fn("derive", |t: &mut Table, request: Map, others: Array| -> ScriptResult<Table> {
        let others = tables(others)?;
        t.derive(&request, &others)
    });
    for op in ["subset", "remove_duplicates", "stack", "unstack", "transpose", "summarize", "multiple_imputation"] {
        engine.register_fn(op, move |t: &mut Table, mut request: Map| {
            request.insert("operation".into(), op.into());
            t.derive(&request, &[])
        });
    }
    engine.register_fn("sorted", |t: &mut Table, keys: Array| {
        let mut request = Map::new();
        request.insert("operation".into(), "sort".into());
        request.insert("keys".into(), Dynamic::from_array(keys));
        t.derive(&request, &[])
    });
    engine.register_fn("join", |t: &mut Table, other: Table, mut request: Map| {
        request.insert("operation".into(), "join".into());
        t.derive(&request, &[other])
    });
    engine.register_fn("combine_rows", |t: &mut Table, others: Array| -> ScriptResult<Table> {
        let mut request = Map::new();
        request.insert("operation".into(), "combine-rows".into());
        t.derive(&request, &tables(others)?)
    });
    engine.register_fn("combine_rows", |t: &mut Table, others: Array, mut request: Map| -> ScriptResult<Table> {
        request.insert("operation".into(), "combine-rows".into());
        t.derive(&request, &tables(others)?)
    });

    // Analyses.
    engine.register_fn("analyze", |t: &mut Table, kind: &str, settings: Map| -> ScriptResult<Report> {
        let mut settings = settings;
        settings.insert("analysis".into(), kind.replace('_', "-").into());
        t.analyze_json(&to_json(&Dynamic::from_map(settings))?)
    });
    engine.register_fn("analyze", |t: &mut Table, settings: Map| -> ScriptResult<Report> {
        let Some(kind) = settings.get("analysis").map(ToString::to_string) else {
            return fail("analysis settings need an \"analysis\" name, e.g. \"linear-model\"");
        };
        let mut settings = settings;
        settings.insert("analysis".into(), kind.replace('_', "-").into());
        t.analyze_json(&to_json(&Dynamic::from_map(settings))?)
    });
    engine.register_fn("reports", |t: &mut Table| -> Array {
        let d = t.0.borrow();
        d.reports
            .iter()
            .map(|saved| match run_report(saved, &d.table, &d.rows) {
                Ok(computed) => Dynamic::from(Report::new(computed, &d.table, &d.rows, Some(t.clone()))),
                Err(e) => Dynamic::from(format!("could not be computed: {e}")),
            })
            .collect()
    });

    engine.register_fn("update_report", |t: &mut Table, i: INT, settings: Map| -> ScriptResult<Report> {
        let i = t.report_index(i)?;
        let request = to_json(&Dynamic::from_map(settings))?;
        let (saved, report) = {
            let d = t.0.borrow();
            let saved =
                report_from_json(&request, &d.table).map_err(|e| script_err(format!("analysis settings: {e}")))?;
            let computed = run_report(&saved, &d.table, &d.rows).map_err(script_err)?;
            (saved, Report::new(computed, &d.table, &d.rows, Some(t.clone())))
        };
        t.act(Action::UpdateReport(i, saved))?;
        Ok(report)
    });
    engine.register_fn("close_report", |t: &mut Table, i: INT| -> ScriptResult<()> {
        let i = t.report_index(i)?;
        t.act(Action::RemoveReport(i))
    });

    // Saving.
    let h = Rc::clone(host);
    engine.register_fn("save", move |t: &mut Table, path: &str| t.save(&h.resolve(path)));
    let h = Rc::clone(host);
    engine.register_fn("save_labelled", move |t: &mut Table, path: &str| t.save_labelled(&h.resolve(path)));
    let h = Rc::clone(host);
    engine.register_fn("save_reports", move |t: &mut Table, path: &str| {
        t.save_reports(&h.resolve(path), PaperSize::default())
    });
    let h = Rc::clone(host);
    engine.register_fn("save_reports", move |t: &mut Table, path: &str, options: rhai::Map| {
        t.save_reports(&h.resolve(path), crate::report::paper_option(&options)?)
    });
    let h = Rc::clone(host);
    engine.register_fn("save_project", move |t: &mut Table, path: &str| -> ScriptResult<()> {
        let path = h.resolve(path);
        if FileKind::from_path(&path) != Some(FileKind::Project) {
            return fail(format!("project files end in .{PROJECT_EXTENSION}"));
        }
        t.save(&path)
    });
}

fn tables(values: Array) -> ScriptResult<Vec<Table>> {
    values
        .into_iter()
        .map(|v| {
            let name = v.type_name();
            v.try_cast::<Table>().ok_or_else(|| script_err(format!("expected tables; got a {name}")))
        })
        .collect()
}
