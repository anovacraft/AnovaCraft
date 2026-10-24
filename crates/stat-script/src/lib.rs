//! # stat-script
//!
//! Scripting for AnovaCraft in [Rhai](https://rhai.rs), an embedded
//! language implemented in pure Rust (MIT/Apache-2.0). Scripts open and
//! build tables, change row states, run table operations and analyses, read
//! report sections as data and write files. The same engine will run formula
//! columns and recorded actions (ROADMAP §1.2, §11.3).
//!
//! Scripts call the application's own operations: an analysis run from a
//! script is a [`app_model::SavedReport`] request run by
//! [`app_model::run_report`], exactly as a dialog's request is, so its
//! sections are identical. The script API is documented in
//! `docs/scripting.md`.
//!
//! [`run_script`] and [`run_file`] run a script on its own (the
//! `anovacraft-run` runner); [`run_in_workspace`] runs one on copies of the
//! desktop application's open tables and reports what it changed, for the
//! application to merge.
//!
//! ```
//! use std::{cell::RefCell, rc::Rc};
//!
//! let source = r#"
//!     let t = open_sample("Norris");
//!     let r = t.analyze("linear_model", #{ response: "Reading", predictors: ["Reference"] });
//!     for row in r.section("Parameter Estimates") { print(row.Term); }
//! "#;
//! let lines = Rc::new(RefCell::new(Vec::new()));
//! let sink = Rc::clone(&lines);
//! stat_script::run_script(source, &Default::default(), move |s| sink.borrow_mut().push(s.to_owned())).unwrap();
//! assert_eq!(*lines.borrow(), ["Intercept", "Reference"]);
//! ```
//!
//! ## Safety
//! Each run has limits on operations and call depth, and optionally on
//! string, array and map sizes ([`Limits`]); a cancel flag ([`RunOptions::cancel`]) stops a script
//! between operations. Scripts reach files only through the registered
//! functions (opening and saving tables, projects and exports): modules
//! cannot be imported and `eval` is disabled.

pub mod batch;
mod report;
mod signal;
mod signal_processing;
mod stats;
pub mod syntax;
mod table;
mod value;
mod workspace;

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use rhai::module_resolvers::DummyModuleResolver;
use rhai::{Dynamic, Engine, EvalAltResult, Position, Scope};

pub use batch::{BatchItem, BatchOptions, BatchRun, BatchValue, batch_files, run_batch};
pub use report::Report;
pub use table::Table;
pub use workspace::{
    CellRun, DocumentChange, NotebookSession, OpenDocument, Workspace, WorkspaceRun, run_in_workspace,
};

/// File extension of script files (without the dot).
pub const SCRIPT_EXTENSION: &str = "rhai";

/// The script files (`.rhai`, any case; hidden files left out) directly in
/// `folder`, sorted by name case-insensitively: the sidebar's script library.
///
/// # Errors
/// The folder cannot be read.
pub fn script_files(folder: &Path) -> Result<Vec<PathBuf>, String> {
    let entries =
        std::fs::read_dir(folder).map_err(|e| format!("could not read the folder {}: {e}", folder.display()))?;
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.'))
                && p.extension().is_some_and(|e| e.eq_ignore_ascii_case(SCRIPT_EXTENSION))
        })
        .collect();
    files.sort_by_key(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()));
    Ok(files)
}

/// Resource limits for one script run.
///
/// The size limits are off by default. Rhai enforces any of them by
/// measuring every array and map (with the strings inside) after each change
/// to one, so with a size limit set, building an array element by element
/// takes time proportional to the square of its length (about 25 s for
/// 100 000 elements instead of milliseconds). The operation limit bounds
/// runaway scripts instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Maximum number of operations (roughly, evaluated expressions); 0 for
    /// no limit. Time spent inside an analysis counts as one operation.
    pub max_operations: u64,
    /// Maximum depth of nested function calls.
    pub max_call_levels: usize,
    /// Maximum length of a string, in bytes; 0 for no limit.
    pub max_string_size: usize,
    /// Maximum number of elements in an array; 0 for no limit.
    pub max_array_size: usize,
    /// Maximum number of members in an object map; 0 for no limit.
    pub max_map_size: usize,
    /// Maximum nesting depth of expressions.
    pub max_expr_depth: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_operations: 1_000_000_000,
            max_call_levels: 64,
            max_string_size: 0,
            max_array_size: 0,
            max_map_size: 0,
            max_expr_depth: 128,
        }
    }
}

/// How to run a script.
#[derive(Debug, Clone, Default)]
pub struct RunOptions {
    /// Directory that relative file paths in the script are resolved
    /// against (usually the script's own directory). Empty: the current
    /// directory.
    pub base_dir: PathBuf,
    /// Values of the script's `args` array (command-line arguments).
    pub args: Vec<String>,
    /// Resource limits.
    pub limits: Limits,
    /// Set to `true` from another thread to stop the script.
    pub cancel: Option<Arc<AtomicBool>>,
}

/// Why a script failed, with the line and column where it did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptError {
    /// What went wrong.
    pub message: String,
    /// 1-based line, when known.
    pub line: Option<usize>,
    /// 1-based column, when known.
    pub column: Option<usize>,
    /// The script was stopped by its cancel flag.
    pub cancelled: bool,
}

impl std::fmt::Display for ScriptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (self.line, self.column) {
            (Some(l), Some(c)) => write!(f, "line {l}, column {c}: {}", self.message),
            (Some(l), None) => write!(f, "line {l}: {}", self.message),
            _ => f.write_str(&self.message),
        }
    }
}

impl std::error::Error for ScriptError {}

impl ScriptError {
    fn from_eval(e: &EvalAltResult) -> Self {
        let pos = innermost_position(e);
        Self {
            message: describe(e),
            line: pos.line(),
            column: pos.position(),
            cancelled: matches!(e, EvalAltResult::ErrorTerminated(..)),
        }
    }
}

fn innermost_position(e: &EvalAltResult) -> Position {
    match e {
        EvalAltResult::ErrorInFunctionCall(_, _, inner, pos) => {
            let p = innermost_position(inner);
            if p.is_none() { *pos } else { p }
        }
        other => other.position(),
    }
}

fn describe(e: &EvalAltResult) -> String {
    match e {
        EvalAltResult::ErrorInFunctionCall(name, _, inner, _) if !name.is_empty() => {
            format!("{} (in function {name})", describe(inner))
        }
        EvalAltResult::ErrorInFunctionCall(_, _, inner, _) => describe(inner),
        EvalAltResult::ErrorRuntime(v, _) => v.clone().into_string().unwrap_or_else(|_| v.to_string()),
        EvalAltResult::ErrorTerminated(..) => "the script was stopped".to_owned(),
        other => {
            let text = other.to_string();
            // Position is reported separately.
            match text.rfind(" (line ") {
                Some(i) if text.ends_with(')') => text[..i].to_owned(),
                _ => text,
            }
        }
    }
}

/// What registered functions share: where relative paths start.
#[derive(Debug)]
pub(crate) struct Host {
    base_dir: PathBuf,
}

impl Host {
    /// `path` resolved against the base directory.
    pub(crate) fn resolve(&self, path: &str) -> PathBuf {
        let p = Path::new(path);
        if p.is_absolute() || self.base_dir.as_os_str().is_empty() { p.to_path_buf() } else { self.base_dir.join(p) }
    }
}

/// A Rhai engine with the AnovaCraft API registered, the limits applied and
/// `print`/`debug` output sent to `output`. No tables are open:
/// `open_tables()` is empty and `t.show()` does nothing (see
/// [`run_in_workspace`] for scripts run in the desktop application).
#[must_use]
pub fn new_engine(options: &RunOptions, output: impl Fn(&str) + 'static) -> Engine {
    new_engine_with(options, output, Rc::default())
}

fn new_engine_with(options: &RunOptions, output: impl Fn(&str) + 'static, session: Rc<workspace::Session>) -> Engine {
    let output: Rc<dyn Fn(&str)> = Rc::new(output);
    new_engine_split(options, Rc::clone(&output), output, session)
}

/// An engine sending `print` output to `print` and `debug` output (prefixed
/// with its line) to `debug`.
fn new_engine_split(
    options: &RunOptions,
    print: Rc<dyn Fn(&str)>,
    debug: Rc<dyn Fn(&str)>,
    session: Rc<workspace::Session>,
) -> Engine {
    let mut engine = Engine::new();
    let l = options.limits;
    engine.set_max_operations(l.max_operations);
    engine.set_max_call_levels(l.max_call_levels);
    engine.set_max_string_size(l.max_string_size);
    engine.set_max_array_size(l.max_array_size);
    engine.set_max_map_size(l.max_map_size);
    engine.set_max_expr_depths(l.max_expr_depth, l.max_expr_depth);
    // No file access except through the registered functions.
    engine.set_module_resolver(DummyModuleResolver::new());
    engine.disable_symbol("eval");
    if let Some(cancel) = options.cancel.clone() {
        engine.on_progress(move |_| cancel.load(Ordering::Relaxed).then_some(Dynamic::UNIT));
    }
    engine.on_print(move |s| print(s));
    engine.on_debug(move |s, _, pos| match pos.line() {
        Some(line) => debug(&format!("[line {line}] {s}")),
        None => debug(s),
    });

    let host = Rc::new(Host { base_dir: options.base_dir.clone() });
    table::register(&mut engine, &host);
    report::register(&mut engine, &host);
    workspace::register(&mut engine, &session);
    stats::register(&mut engine);
    signal::register(&mut engine);
    signal_processing::register(&mut engine);
    engine.register_fn("parse_json", |text: &str| -> value::ScriptResult<Dynamic> {
        stat_data::parse_json(text).map(|j| value::from_json(&j)).map_err(value::script_err)
    });
    engine.register_fn("to_json", |v: Dynamic| -> value::ScriptResult<String> {
        value::to_json(&v).map(|j| j.to_pretty())
    });
    engine
}

/// A function scripts can call, for completion in an editor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiFunction {
    /// The name, as typed (`analyze`, `open_sample`, …).
    pub name: String,
    /// The signatures registered under the name, such as
    /// `open_sample(name: &str) -> Table`, shortest first.
    pub signatures: Vec<String>,
}

/// Every function the AnovaCraft API registers (the application's own, not
/// Rhai's built-in ones), sorted by name. Methods appear under their name
/// (`t.rows()` as `rows`); property getters and setters, and operators, are
/// left out.
#[must_use]
pub fn api_functions() -> Vec<ApiFunction> {
    let engine = new_engine(&RunOptions::default(), |_| {});
    let mut out: Vec<ApiFunction> = Vec::new();
    for signature in engine.gen_fn_signatures(false) {
        let name = signature.split('(').next().unwrap_or_default().trim();
        let name = name.rsplit(' ').next().unwrap_or(name);
        let identifier = name.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !identifier {
            continue;
        }
        // Readable type names: Rhai writes the Rust paths of registered types.
        let signature = signature
            .replace("stat_script::table::", "")
            .replace("stat_script::report::", "")
            .replace("rhai::", "")
            .replace("ImmutableString", "String")
            .replace("&mut ", "");
        match out.iter_mut().find(|f| f.name == name) {
            Some(f) => {
                if !f.signatures.contains(&signature) {
                    f.signatures.push(signature);
                }
            }
            None => out.push(ApiFunction { name: name.to_owned(), signatures: vec![signature] }),
        }
    }
    for f in &mut out {
        f.signatures.sort_by_key(|s| (s.len(), s.clone()));
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Check a script's syntax without running it.
///
/// # Errors
/// The first syntax error, with its position.
pub fn check_syntax(source: &str) -> Result<(), ScriptError> {
    let engine = new_engine(&RunOptions::default(), |_| {});
    engine.compile(source).map(|_| ()).map_err(|e| ScriptError::from_eval(&e.into()))
}

/// Run a script, sending its `print` and `debug` output to `output`.
///
/// # Errors
/// A [`ScriptError`] for a syntax error, a failed operation (with the
/// application's message), an exceeded limit, or cancellation.
pub fn run_script(source: &str, options: &RunOptions, output: impl Fn(&str) + 'static) -> Result<(), ScriptError> {
    eval_script(source, options, output, &[]).map(|_| ())
}

/// Run a script with extra named constants in scope besides `args`, and
/// return the value of its last statement.
pub(crate) fn eval_script(
    source: &str,
    options: &RunOptions,
    output: impl Fn(&str) + 'static,
    constants: &[(&str, Dynamic)],
) -> Result<Dynamic, ScriptError> {
    let engine = new_engine(options, output);
    run_compiled(&engine, source, options, constants)
}

/// Compile and run `source` on `engine` with `args` and `constants` in scope.
fn run_compiled(
    engine: &Engine,
    source: &str,
    options: &RunOptions,
    constants: &[(&str, Dynamic)],
) -> Result<Dynamic, ScriptError> {
    let ast = engine.compile(source).map_err(|e| ScriptError::from_eval(&e.into()))?;
    let mut scope = Scope::new();
    let args: rhai::Array = options.args.iter().map(|a| a.clone().into()).collect();
    scope.push_constant("args", args);
    for (name, value) in constants {
        scope.push_constant_dynamic(*name, value.clone());
    }
    engine.eval_ast_with_scope::<Dynamic>(&mut scope, &ast).map_err(|e| ScriptError::from_eval(&e))
}

/// Run a script file: relative paths in it are resolved against the file's
/// directory unless `options.base_dir` is set.
///
/// # Errors
/// As [`run_script`], or when the file cannot be read.
pub fn run_file(path: &Path, options: &RunOptions, output: impl Fn(&str) + 'static) -> Result<(), ScriptError> {
    let output: Rc<dyn Fn(&str)> = Rc::new(output);
    run_file_split(path, options, Rc::clone(&output), output)
}

/// Run a script file as [`run_file`] does, sending `print` output to `print`
/// and `debug` output to `debug` (the `anovacraft-run` runner writes them to
/// standard output and standard error).
///
/// # Errors
/// As [`run_file`].
pub fn run_file_split(
    path: &Path,
    options: &RunOptions,
    print: Rc<dyn Fn(&str)>,
    debug: Rc<dyn Fn(&str)>,
) -> Result<(), ScriptError> {
    let source = std::fs::read_to_string(path).map_err(|e| ScriptError {
        message: format!("could not read {}: {e}", path.display()),
        line: None,
        column: None,
        cancelled: false,
    })?;
    let mut options = options.clone();
    if options.base_dir.as_os_str().is_empty() {
        options.base_dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    }
    let engine = new_engine_split(&options, print, debug, Rc::default());
    run_compiled(&engine, &source, &options, &[]).map(|_| ())
}
