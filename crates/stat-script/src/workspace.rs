//! Scripts run against the desktop application's open documents.
//!
//! The application hands a script copies of its open documents
//! ([`OpenDocument`]); the script reaches them with `open_tables()` and
//! `current_table()`. When the script finishes, [`run_in_workspace`] reports
//! what it did as [`DocumentChange`]s: the data edits made to each open
//! document (in order, so the application can replay them as one undoable
//! step and keep its reports pointing at the same columns), the selection
//! and exclusions, the new reports, and the tables the script asked to show
//! with `t.show()`.
//!
//! Scripts run on copies, so the application stays responsive and a script
//! never sees a half-finished change; the application decides how to merge
//! the result (for example, if a document was edited while the script ran,
//! the script's version opens as a new document instead).

use std::cell::RefCell;
use std::rc::Rc;

use app_model::SavedReport;
use app_model::action::{RowStyles, RowSubsets};
use app_model::editing::Edit;
use app_model::project::Project;
use rhai::{Array, Dynamic, Engine, Scope};
use stat_data::RowStateManager;

use crate::table::{Binding, Table};
use crate::value::{ScriptResult, fail};
use crate::{RunOptions, ScriptError, new_engine_with};

/// One document open in the application, handed to a script.
#[derive(Debug, Clone)]
pub struct OpenDocument {
    /// The application's identifier for the document, returned in
    /// [`DocumentChange::Changed`].
    pub key: u64,
    /// The data, row states, reports and origin.
    pub project: Project,
}

/// The documents a script may use, and which one is current.
#[derive(Debug, Clone, Default)]
pub struct Workspace {
    /// Open documents, in tab order.
    pub documents: Vec<OpenDocument>,
    /// Index into `documents` of the active document (`current_table()`).
    pub active: Option<usize>,
}

/// What a script did to one document.
#[derive(Debug, Clone)]
pub enum DocumentChange {
    /// An open document was changed.
    Changed {
        /// [`OpenDocument::key`] of the document.
        key: u64,
        /// Data edits in the order the script made them; each edit's indices
        /// refer to the table as the edits before it left it (as in
        /// [`Edit::Batch`]).
        edits: Vec<Edit>,
        /// Selected rows at the end, if the script set the selection.
        selected: Option<Vec<usize>>,
        /// Excluded rows at the end, if the script excluded or re-included
        /// rows.
        excluded: Option<Vec<usize>>,
        /// Hidden rows at the end, if the script hid or showed rows.
        hidden: Option<Vec<usize>>,
        /// Row colours and markers at the end, if the script changed them.
        colors: Option<Box<RowStyles>>,
        /// Saved row subsets at the end, if the script changed them.
        subsets: Option<RowSubsets>,
        /// The report list at the end (newest first), if the script added,
        /// changed or closed reports: each with its position among the
        /// document's reports when the script started (`None` for a new one).
        reports: Option<Vec<(Option<usize>, SavedReport)>>,
        /// The document as the script left it: what to open as a new
        /// document if the edits cannot be replayed.
        project: Box<Project>,
    },
    /// A table the script asked to show (`t.show()`), or an open document
    /// the script replaced wholesale: open it as a new document.
    New {
        /// The table with its row states, reports and origin.
        project: Box<Project>,
    },
}

/// The outcome of [`run_in_workspace`].
#[derive(Debug, Clone)]
pub struct WorkspaceRun {
    /// `Ok` if the script ran to the end.
    pub result: Result<(), ScriptError>,
    /// What the script did, including the changes made before a failure.
    pub changes: Vec<DocumentChange>,
}

/// Tables of one run: those bound to open documents, and those shown.
#[derive(Debug, Default)]
pub(crate) struct Session {
    pub(crate) open: Vec<Table>,
    pub(crate) active: Option<usize>,
    pub(crate) shown: RefCell<Vec<Table>>,
}

/// Run a script with the application's open documents available.
///
/// Changes made before an error are still returned, so they can be kept (and
/// undone) like the effect of any other command.
pub fn run_in_workspace(
    source: &str,
    options: &RunOptions,
    workspace: Workspace,
    output: impl Fn(&str) + 'static,
) -> WorkspaceRun {
    let open: Vec<Table> = workspace.documents.into_iter().map(bind).collect();
    let session = Rc::new(Session { open, active: workspace.active, shown: RefCell::default() });
    let engine = new_engine_with(options, output, Rc::clone(&session));
    let result = engine.compile(source).map_err(|e| ScriptError::from_eval(&e.into())).and_then(|ast| {
        let mut scope = Scope::new();
        let args: Array = options.args.iter().map(|a| a.clone().into()).collect();
        scope.push_constant("args", args);
        engine.run_ast_with_scope(&mut scope, &ast).map_err(|e| ScriptError::from_eval(&e))
    });
    drop(engine);
    WorkspaceRun { result, changes: changes(&session) }
}

fn bind(doc: OpenDocument) -> Table {
    let Project { table, rows, reports, origin, external, scripts } = doc.project;
    let binding = Binding {
        key: doc.key,
        edits: Vec::new(),
        replaced: false,
        selection_set: false,
        exclusion_set: false,
        hidden_set: false,
        colors_set: false,
        subsets_set: false,
        reports_changed: false,
    };
    let keys = (0..reports.len()).map(Some).collect();
    let t = Table::new(table, rows, reports);
    {
        let mut d = t.0.borrow_mut();
        d.origin = origin;
        d.external = external;
        d.scripts = scripts;
        d.binding = Some(binding);
        d.report_keys = keys;
    }
    t
}

fn excluded(rows: &RowStateManager) -> Vec<usize> {
    (0..rows.len()).filter(|&r| rows.is_excluded_flag(r)).collect()
}

fn changes(session: &Session) -> Vec<DocumentChange> {
    let mut out = Vec::new();
    for t in &session.open {
        let project = Box::new(t.project());
        let mut d = t.0.borrow_mut();
        let Some(b) = d.binding.take() else { continue };
        if b.replaced {
            out.push(DocumentChange::New { project });
            continue;
        }
        let selected = b.selection_set.then(|| d.rows.selected_rows());
        let excluded = b.exclusion_set.then(|| excluded(&d.rows));
        let hidden = b.hidden_set.then(|| (0..d.rows.len()).filter(|&r| d.rows.is_hidden_flag(r)).collect());
        let colors = b.colors_set.then(|| Box::new(RowStyles::of(&d.rows)));
        let subsets = b.subsets_set.then(|| RowSubsets::of(&d.rows));
        let reports = b.reports_changed.then(|| d.report_keys.iter().copied().zip(d.reports.iter().cloned()).collect());
        let unchanged = b.edits.is_empty()
            && selected.is_none()
            && excluded.is_none()
            && hidden.is_none()
            && colors.is_none()
            && subsets.is_none()
            && reports.is_none();
        if !unchanged {
            out.push(DocumentChange::Changed {
                key: b.key,
                edits: b.edits,
                selected,
                excluded,
                hidden,
                colors,
                subsets,
                reports,
                project,
            });
        }
    }
    for t in session.shown.borrow().iter() {
        out.push(DocumentChange::New { project: Box::new(t.project()) });
    }
    out
}

/// Register `open_tables`, `current_table`, `table` and `show`.
pub(crate) fn register(engine: &mut Engine, session: &Rc<Session>) {
    let s = Rc::clone(session);
    engine.register_fn("open_tables", move || -> Array { s.open.iter().cloned().map(Dynamic::from).collect() });
    let s = Rc::clone(session);
    engine.register_fn("current_table", move || -> ScriptResult<Table> {
        match s.active.and_then(|i| s.open.get(i)) {
            Some(t) => Ok(t.clone()),
            None => fail("no table is open (current_table() is the active table when a script runs in the app)"),
        }
    });
    let s = Rc::clone(session);
    engine.register_fn("table", move |name: &str| -> ScriptResult<Table> {
        let mut found = s.open.iter().filter(|t| t.0.borrow().table.name() == name);
        match (found.next(), found.next()) {
            (Some(t), None) => Ok(t.clone()),
            (Some(_), Some(_)) => fail(format!("more than one open table is called {name:?}; use open_tables()[i]")),
            (None, _) => fail(format!("no open table is called {name:?}")),
        }
    });
    let s = Rc::clone(session);
    engine.register_fn("show", move |t: &mut Table| {
        let known = s.open.iter().chain(s.shown.borrow().iter()).any(|o| Rc::ptr_eq(&o.0, &t.0));
        if !known {
            s.shown.borrow_mut().push(t.clone());
        }
    });
}

/// Runs the cells of a script notebook one after another, keeping what
/// earlier cells defined (variables, `fn` functions, tables they opened)
/// for later ones, as if the cells were one script run in steps.
///
/// Each run gets fresh copies of the open documents, as [`run_in_workspace`]
/// does; a variable that held an open table from an earlier cell (`let t =
/// current_table();`) is pointed at the fresh copy of the same document, so
/// it sees edits made since and its changes are reported again.
///
/// The session holds script values, which cannot cross threads: create and
/// use it on one thread (the application keeps one worker thread per open
/// notebook).
pub struct NotebookSession {
    scope: Scope<'static>,
    functions: rhai::AST,
    /// Tables bound to open documents, by document key.
    tables: Vec<(u64, Table)>,
}

impl std::fmt::Debug for NotebookSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "NotebookSession({} variables)", self.scope.len())
    }
}

/// The outcome of running one notebook cell.
#[derive(Debug, Clone)]
pub struct CellRun {
    /// Whether it ran to the end, and what it changed in the open documents.
    pub run: WorkspaceRun,
    /// The value of the cell's last line, as text or tables (nothing for a
    /// cell ending in `;`).
    pub value: Vec<app_model::notebook::CellOutput>,
}

/// Rows of a table value shown in a cell's output.
const VALUE_ROWS: usize = 50;

impl NotebookSession {
    /// A session with nothing defined yet; `args` are the script's `args`.
    #[must_use]
    pub fn new(args: &[String]) -> Self {
        let mut scope = Scope::new();
        let args: Array = args.iter().map(|a| a.clone().into()).collect();
        scope.push_constant("args", args);
        Self { scope, functions: rhai::AST::empty(), tables: Vec::new() }
    }

    /// Names of the variables defined so far, in order.
    #[must_use]
    pub fn variables(&self) -> Vec<String> {
        self.scope.iter().map(|(name, _, _)| name.to_owned()).filter(|n| n != "args").collect()
    }

    /// Run one cell's `source` with the application's open documents.
    pub fn run_cell(
        &mut self,
        source: &str,
        options: &RunOptions,
        workspace: Workspace,
        output: impl Fn(&str) + 'static,
    ) -> CellRun {
        let mut open = Vec::new();
        let mut tables = Vec::new();
        for doc in workspace.documents {
            let key = doc.key;
            let fresh = bind(doc);
            let table = match self.tables.iter().find(|(k, _)| *k == key) {
                Some((_, old)) => match Rc::try_unwrap(fresh.0) {
                    Ok(cell) => {
                        *old.0.borrow_mut() = cell.into_inner();
                        old.clone()
                    }
                    Err(rc) => Table(rc),
                },
                None => fresh,
            };
            tables.push((key, table.clone()));
            open.push(table);
        }
        self.tables = tables;
        let session = Rc::new(Session { open, active: workspace.active, shown: RefCell::default() });
        let engine = new_engine_with(options, output, Rc::clone(&session));
        let value = engine.compile(source).map_err(|e| ScriptError::from_eval(&e.into())).and_then(|ast| {
            let combined = self.functions.merge(&ast);
            self.functions = self.functions.merge(&ast.clone_functions_only());
            engine.eval_ast_with_scope::<Dynamic>(&mut self.scope, &combined).map_err(|e| ScriptError::from_eval(&e))
        });
        drop(engine);
        let (result, value) = match value {
            Ok(v) => (Ok(()), value_outputs(&v)),
            Err(e) => (Err(e), Vec::new()),
        };
        CellRun { run: WorkspaceRun { result, changes: changes(&session) }, value }
    }
}

/// A cell's value as outputs: a table's first rows, a report's tables, rows
/// of maps (a report section) as a table, a map as names and values, and
/// anything else as text.
fn value_outputs(v: &Dynamic) -> Vec<app_model::notebook::CellOutput> {
    use app_model::notebook::CellOutput;
    use app_model::report::{SectionContent, TextTable};
    if v.is_unit() {
        return Vec::new();
    }
    if let Some(t) = v.clone().try_cast::<Table>() {
        let d = t.0.borrow();
        let columns: Vec<String> = d.table.columns().iter().map(|c| c.name.clone()).collect();
        let mut table = TextTable::new(columns);
        let n = d.table.n_rows();
        for r in 0..n.min(VALUE_ROWS) {
            table.push(d.table.columns().iter().map(|c| c.cell_text(r)));
        }
        let shown = if n > VALUE_ROWS { format!(", first {VALUE_ROWS} shown") } else { String::new() };
        let title = format!("{} ({n} rows{shown})", d.table.name());
        return vec![CellOutput::Table { title, table }];
    }
    if let Some(r) = v.clone().try_cast::<crate::Report>() {
        let computed = r.computed();
        let mut out = vec![CellOutput::Value(computed.title.clone())];
        for s in &computed.sections {
            match &s.content {
                SectionContent::Table(t) => {
                    out.push(CellOutput::Table { title: s.title.clone(), table: t.clone() });
                }
                SectionContent::Note(text) | SectionContent::Formula(text) => {
                    out.push(CellOutput::Value(format!("{}: {text}", s.title)));
                }
                _ => {}
            }
        }
        return out;
    }
    if let Some(items) = v.read_lock::<Array>() {
        let maps: Vec<rhai::Map> = items.iter().filter_map(|i| i.read_lock::<rhai::Map>().map(|m| m.clone())).collect();
        if !items.is_empty() && maps.len() == items.len() {
            let mut columns: Vec<String> = Vec::new();
            for m in &maps {
                for k in m.keys() {
                    if !columns.iter().any(|c| c == k.as_str()) {
                        columns.push(k.to_string());
                    }
                }
            }
            let mut table = TextTable::new(columns.clone());
            for m in maps.iter().take(VALUE_ROWS) {
                table.push(columns.iter().map(|c| m.get(c.as_str()).map_or_else(String::new, crate::value::as_text)));
            }
            let title = if maps.len() > VALUE_ROWS {
                format!("{} rows, first {VALUE_ROWS} shown", maps.len())
            } else {
                format!("{} rows", maps.len())
            };
            return vec![CellOutput::Table { title, table }];
        }
    }
    if let Some(m) = v.read_lock::<rhai::Map>() {
        let mut table = TextTable::new(["Name", "Value"]);
        for (k, val) in m.iter() {
            table.push([k.to_string(), crate::value::as_text(val)]);
        }
        return vec![CellOutput::Table { title: String::new(), table }];
    }
    let text = crate::value::as_text(v);
    vec![CellOutput::Value(text)]
}
