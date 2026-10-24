//! Batch runs: one script run once per data file in a folder, with what each
//! run returned collected into a results table.
//!
//! Each run gets its own engine and these constants besides `args`:
//!
//! | Constant | Value |
//! |---|---|
//! | `batch_file` | the file's full path (pass it to `open_table` or `open_project`) |
//! | `batch_name` | the file's name |
//! | `batch_index` | its position in the batch, from 0 |
//!
//! The value of the script's last statement is its result for that file: a
//! map gives one results column per key (numbers stay numbers), any other
//! value except `()` a `Result` column. A run that fails is recorded with
//! its error and the batch carries on.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::Ordering;

use app_model::file_formats::FileKind;
use rhai::Dynamic;
use stat_data::{Column, DataTable};

use crate::{RunOptions, ScriptError, eval_script};

/// How to run a batch.
#[derive(Debug, Clone, Default)]
pub struct BatchOptions {
    /// File-name pattern (`*` any characters, `?` one, case-insensitive),
    /// e.g. `*.csv`. `None`: every file the application can open.
    pub pattern: Option<String>,
    /// Options for each run (base folder, `args`, limits, cancel flag).
    pub run: RunOptions,
}

/// One value a run returned.
#[derive(Debug, Clone, PartialEq)]
pub enum BatchValue {
    /// A number.
    Number(f64),
    /// Anything else, as text.
    Text(String),
}

/// One file's run.
#[derive(Debug, Clone, PartialEq)]
pub struct BatchItem {
    /// The file.
    pub file: PathBuf,
    /// The named values the run returned, or why it failed.
    pub result: Result<Vec<(String, BatchValue)>, ScriptError>,
}

/// A finished (or stopped) batch.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BatchRun {
    /// One item per file run, in order.
    pub items: Vec<BatchItem>,
    /// Files left out because the batch was stopped.
    pub skipped: usize,
}

impl BatchRun {
    /// Number of failed runs.
    #[must_use]
    pub fn failures(&self) -> usize {
        self.items.iter().filter(|i| i.result.is_err()).count()
    }

    /// The results as a table: `File`, `Status` (`ok` or `error`), `Error`
    /// (when any run failed), then one column per returned name in order of
    /// first appearance (numeric when every value given is a number).
    #[must_use]
    pub fn table(&self, name: &str) -> DataTable {
        let file_names: Vec<String> = self
            .items
            .iter()
            .map(|i| {
                i.file.file_name().map_or_else(|| i.file.display().to_string(), |n| n.to_string_lossy().into_owned())
            })
            .collect();
        let mut columns = vec![
            Column::text("File", file_names),
            Column::text(
                "Status",
                self.items.iter().map(|i| if i.result.is_ok() { "ok" } else { "error" }.to_owned()).collect(),
            ),
        ];
        if self.failures() > 0 {
            columns.push(Column::text(
                "Error",
                self.items
                    .iter()
                    .map(|i| i.result.as_ref().err().map(ToString::to_string).unwrap_or_default())
                    .collect(),
            ));
        }
        let mut keys: Vec<&str> = Vec::new();
        for item in &self.items {
            for (k, _) in item.result.as_deref().unwrap_or_default() {
                if !keys.contains(&k.as_str()) {
                    keys.push(k);
                }
            }
        }
        for key in keys {
            let values: Vec<Option<&BatchValue>> = self
                .items
                .iter()
                .map(|i| i.result.as_deref().ok().and_then(|r| r.iter().find(|(k, _)| k == key).map(|(_, v)| v)))
                .collect();
            let numeric = values.iter().all(|v| !matches!(v, Some(BatchValue::Text(_))));
            // A blank result name still needs a column heading.
            let mut name = if key.trim().is_empty() { "Result".to_owned() } else { key.to_owned() };
            while columns.iter().any(|c| c.name == name) {
                name.push('_');
            }
            columns.push(if numeric {
                Column::numeric(
                    name,
                    values.iter().map(|v| if let Some(BatchValue::Number(x)) = v { *x } else { f64::NAN }).collect(),
                )
            } else {
                Column::text(
                    name,
                    values
                        .iter()
                        .map(|v| match v {
                            Some(BatchValue::Number(x)) => stat_data::format_number(*x),
                            Some(BatchValue::Text(t)) => t.clone(),
                            None => String::new(),
                        })
                        .collect(),
                )
            });
        }
        DataTable::new(name, columns).expect("results columns have equal length and distinct names")
    }
}

/// `true` when `name` matches `pattern` (`*`, `?`; ASCII case-insensitive).
#[must_use]
pub fn matches_pattern(pattern: &str, name: &str) -> bool {
    // Iterative matching with backtracking to the last `*` only: O(len(p) ·
    // len(n)) at worst (trying every split at every `*` is exponential).
    let p: Vec<char> = pattern.chars().collect();
    let n: Vec<char> = name.chars().collect();
    let (mut i, mut j) = (0, 0);
    // Position after the last `*` seen, and where in `n` it started matching.
    let mut star: Option<(usize, usize)> = None;
    while j < n.len() {
        match p.get(i) {
            Some('*') => {
                i += 1;
                star = Some((i, j));
            }
            Some(&c) if c == '?' || c.eq_ignore_ascii_case(&n[j]) => {
                i += 1;
                j += 1;
            }
            _ => match star {
                // Let the last `*` absorb one more character and retry.
                Some((si, sj)) => {
                    i = si;
                    j = sj + 1;
                    star = Some((si, sj + 1));
                }
                None => return false,
            },
        }
    }
    p[i..].iter().all(|&c| c == '*')
}

/// The files of `folder` a batch runs over, sorted by name: those matching
/// `pattern`, or (without one) every file the application can open. The
/// paths are absolute, so a script's own base folder does not change them.
///
/// # Errors
/// The folder cannot be read.
pub fn batch_files(folder: &Path, pattern: Option<&str>) -> Result<Vec<PathBuf>, String> {
    let folder = std::path::absolute(folder).unwrap_or_else(|_| folder.to_path_buf());
    let entries =
        std::fs::read_dir(&folder).map_err(|e| format!("could not read the folder {}: {e}", folder.display()))?;
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .filter(|p| {
            let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            match pattern {
                Some(pat) if !pat.trim().is_empty() => matches_pattern(pat.trim(), &name),
                _ => !name.starts_with('.') && FileKind::from_path(p).is_some(),
            }
        })
        .collect();
    files.sort();
    Ok(files)
}

fn batch_value(v: &Dynamic) -> BatchValue {
    if let Ok(i) = v.as_int() {
        BatchValue::Number(i as f64)
    } else if let Ok(x) = v.as_float() {
        BatchValue::Number(x)
    } else if v.is_string() {
        BatchValue::Text(v.clone().into_string().unwrap_or_default())
    } else {
        BatchValue::Text(v.to_string())
    }
}

fn collect(value: &Dynamic) -> Vec<(String, BatchValue)> {
    if value.is_unit() {
        Vec::new()
    } else if let Some(map) = value.read_lock::<rhai::Map>() {
        map.iter().map(|(k, v)| (k.to_string(), batch_value(v))).collect()
    } else {
        vec![("Result".to_owned(), batch_value(value))]
    }
}

/// Run `source` once for every file of `folder` (see [`batch_files`]).
/// `output` receives each run's `print`/`debug` lines with the file they
/// came from. A syntax error stops the batch before any run; a failed run
/// is recorded and the batch goes on; the cancel flag stops it between
/// files (and inside a run).
///
/// # Errors
/// A syntax error, or a folder that cannot be read.
pub fn run_batch(
    source: &str,
    folder: &Path,
    options: &BatchOptions,
    output: impl Fn(&Path, &str) + 'static,
) -> Result<BatchRun, ScriptError> {
    crate::check_syntax(source)?;
    let files = batch_files(folder, options.pattern.as_deref()).map_err(|message| ScriptError {
        message,
        line: None,
        column: None,
        cancelled: false,
    })?;
    let output = Rc::new(output);
    let mut run = BatchRun::default();
    for (index, file) in files.iter().enumerate() {
        if options.run.cancel.as_ref().is_some_and(|c| c.load(Ordering::Relaxed)) {
            run.skipped = files.len() - index;
            break;
        }
        let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let constants = [
            ("batch_file", Dynamic::from(file.to_string_lossy().into_owned())),
            ("batch_name", Dynamic::from(name)),
            ("batch_index", Dynamic::from(index as i64)),
        ];
        let sink = Rc::clone(&output);
        let path = file.clone();
        let result =
            eval_script(source, &options.run, move |line| sink(&path, line), &constants).map(|value| collect(&value));
        run.items.push(BatchItem { file: file.clone(), result });
    }
    Ok(run)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("anovacraft-batch-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn patterns() {
        assert!(matches_pattern("*.csv", "a.CSV"));
        assert!(matches_pattern("run-??.txt", "run-01.txt"));
        assert!(!matches_pattern("run-??.txt", "run-1.txt"));
        assert!(matches_pattern("*", ""));
        assert!(!matches_pattern("*.csv", "a.tsv"));
        assert!(matches_pattern("a*b*c", "aXXbYbZc"));
        assert!(!matches_pattern("a*b*c", "aXXbYbZ"));
        assert!(matches_pattern("*?", "x"));
        assert!(!matches_pattern("*?", ""));
        assert!(matches_pattern("**", ""));
        assert!(!matches_pattern("", "a"));
    }

    /// Many `*`s against a long name that does not match took exponential
    /// time (13 s for this 60-character name in a debug build; a 255-character
    /// file name never finished), freezing the batch window as the pattern
    /// was typed.
    #[test]
    fn patterns_with_many_stars_match_quickly() {
        let start = std::time::Instant::now();
        let name = "a".repeat(255);
        assert!(!matches_pattern("*a*a*a*a*a*a*a*a*a*a*b", &name));
        assert!(matches_pattern("*a*a*a*a*a*a*a*a*a*a*", &name));
        assert!(start.elapsed() < std::time::Duration::from_secs(1), "{:?}", start.elapsed());
    }

    /// A result with a blank name gets the heading "Result" instead of an
    /// empty column name.
    #[test]
    fn blank_result_names_get_a_heading() {
        let dir = folder("blank-key");
        std::fs::write(dir.join("a.csv"), "x\n1\n2\n").unwrap();
        let run = run_batch(r#"#{ "": 1, " ": 2, File: 3 }"#, &dir, &BatchOptions::default(), |_, _| {}).unwrap();
        let table = run.table("Results");
        let names: Vec<&str> = table.columns().iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["File", "Status", "Result", "Result_", "File_"]);
        assert!(names.iter().all(|n| !n.trim().is_empty()));
    }

    #[test]
    fn runs_every_file_and_collects_results() {
        let dir = folder("collect");
        std::fs::write(dir.join("b.csv"), "x,y\n1,2\n3,5\n").unwrap();
        std::fs::write(dir.join("a.csv"), "x,y\n1,1\n2,2\n4,4\n").unwrap();
        std::fs::write(dir.join("broken.csv"), "y\n1\n").unwrap();
        std::fs::write(dir.join("notes.md"), "not data").unwrap();
        let source = r#"
            let t = open_table(batch_file);
            print(`${batch_index}: ${batch_name}`);
            let s = 0.0;
            for v in t.column("x") { s += v; }
            #{ rows: t.rows(), total: s, first: batch_index == 0 }
        "#;
        let lines = Rc::new(RefCell::new(Vec::new()));
        let sink = Rc::clone(&lines);
        let run = run_batch(source, &dir, &BatchOptions::default(), move |p, l| {
            sink.borrow_mut().push(format!("{}|{l}", p.file_name().unwrap().to_string_lossy()));
        })
        .unwrap();
        // notes.md is not a data file; the others run in name order.
        let names: Vec<String> =
            run.items.iter().map(|i| i.file.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["a.csv", "b.csv", "broken.csv"]);
        assert_eq!(lines.borrow()[0], "a.csv|0: a.csv");
        assert_eq!(run.failures(), 1, "{:?}", run.items[2]);
        let t = run.table("Batch results");
        assert_eq!(t.column(t.find("rows").unwrap()).unwrap().as_numeric().unwrap()[..2], [3.0, 2.0]);
        assert_eq!(t.column(t.find("total").unwrap()).unwrap().as_numeric().unwrap()[..2], [7.0, 4.0]);
        assert_eq!(t.column(t.find("Status").unwrap()).unwrap().cell_text(2), "error");
        assert!(!t.column(t.find("Error").unwrap()).unwrap().cell_text(2).is_empty());
        // Booleans are text.
        assert_eq!(t.column(t.find("first").unwrap()).unwrap().cell_text(0), "true");
        // A pattern narrows the files; a plain value becomes a Result column.
        let options = BatchOptions { pattern: Some("b.*".into()), ..BatchOptions::default() };
        let run = run_batch("open_table(batch_file).rows()", &dir, &options, |_, _| {}).unwrap();
        assert_eq!(run.items.len(), 1);
        assert_eq!(run.items[0].result.as_ref().unwrap()[0], ("Result".to_owned(), BatchValue::Number(2.0)));
        // A syntax error stops before any run.
        assert!(run_batch("let = ;", &dir, &BatchOptions::default(), |_, _| {}).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn script_library_lists_rhai_files() {
        let dir = folder("library");
        for n in ["b.rhai", "A.RHAI", ".hidden.rhai", "c.csv"] {
            std::fs::write(dir.join(n), "1").unwrap();
        }
        std::fs::create_dir_all(dir.join("sub.rhai")).unwrap();
        let names: Vec<String> = crate::script_files(&dir)
            .unwrap()
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["A.RHAI", "b.rhai"]);
        assert!(crate::script_files(&dir.join("missing")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cancel_skips_the_rest() {
        let dir = folder("cancel");
        for n in ["a.csv", "b.csv"] {
            std::fs::write(dir.join(n), "x\n1\n").unwrap();
        }
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let options = BatchOptions {
            run: RunOptions { cancel: Some(cancel), ..RunOptions::default() },
            ..BatchOptions::default()
        };
        let run = run_batch("1", &dir, &options, |_, _| {}).unwrap();
        assert!(run.items.is_empty());
        assert_eq!(run.skipped, 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
