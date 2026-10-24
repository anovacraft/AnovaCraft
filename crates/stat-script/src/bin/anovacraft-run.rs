//! `anovacraft-run`: run an AnovaCraft script without the desktop application.
//!
//! ```text
//! anovacraft-run [--max-operations N] script.rhai [ARGUMENTS…]
//! anovacraft-run [--max-operations N] --batch FOLDER [--pattern GLOB] [--results FILE] script.rhai [ARGUMENTS…]
//! anovacraft-run --licences
//! ```
//!
//! The script's `print` output goes to standard output and `debug` output to
//! standard error. Arguments after the script path are available to the
//! script as the `args` array, and relative paths in the script are resolved
//! against the script's directory. The exit status is 0 on success, 1 when
//! the script fails (the error and its line go to standard error) and 2 for
//! a usage error. `--licences` prints the copyright notices and licence
//! texts of the fonts, data and libraries built into the program
//! (`THIRD-PARTY-NOTICES.txt`, embedded at build time).
//!
//! With `--batch`, the script runs once per data file in FOLDER (or per file
//! matching `--pattern`, e.g. `"*.csv"`), with the constants `batch_file`,
//! `batch_name` and `batch_index` set (see `stat_script::batch`). What each
//! run returns is collected into a results table, written to `--results`
//! (`.csv`, `.tsv`, `.json` or `.xlsx`, by extension, in any case; any
//! other extension is a usage error, reported before anything runs) or else
//! printed to standard output as CSV (the runs' own output then goes to
//! standard error). The exit status is 1 when any run failed.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::rc::Rc;

use app_model::file_formats::ExportFormat;
use stat_script::{BatchOptions, RunOptions, run_batch, run_file_split};

/// The third-party notices, embedded so they travel with the executable.
const THIRD_PARTY_NOTICES: &str = include_str!("../../../../THIRD-PARTY-NOTICES.txt");

const USAGE: &str = "usage: anovacraft-run [--max-operations N] [--batch FOLDER [--pattern GLOB] [--results FILE]] \
                     script.rhai [ARGUMENTS…]\n       anovacraft-run --licences";

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let mut options = RunOptions::default();
    let mut batch: Option<PathBuf> = None;
    let mut pattern: Option<String> = None;
    let mut results: Option<PathBuf> = None;
    let script = loop {
        match args.next().as_deref() {
            Some("-h" | "--help") => {
                println!(
                    "{USAGE}\n\nRuns an AnovaCraft Rhai script. Arguments after the script are in its `args` array.\n\
                     --batch FOLDER   run the script once per data file in FOLDER and collect what each run returns\n\
                     --pattern GLOB   only files whose names match GLOB (* and ?), e.g. \"*.csv\"\n\
                     --results FILE   write the results table to FILE (.csv, .tsv, .json or .xlsx); \
                     default: CSV to standard output\n\
                     --licences       print the licences of the open-source software built into this program"
                );
                return ExitCode::SUCCESS;
            }
            Some("--licences" | "--licenses") => {
                print!("{THIRD_PARTY_NOTICES}");
                return ExitCode::SUCCESS;
            }
            Some("--max-operations") => match args.next().and_then(|n| n.parse().ok()) {
                Some(n) => options.limits.max_operations = n,
                None => {
                    eprintln!("--max-operations needs a whole number (0 for no limit)\n{USAGE}");
                    return ExitCode::from(2);
                }
            },
            Some(flag @ ("--batch" | "--pattern" | "--results")) => {
                let Some(value) = args.next() else {
                    eprintln!("{flag} needs a value\n{USAGE}");
                    return ExitCode::from(2);
                };
                match flag {
                    "--batch" => batch = Some(PathBuf::from(value)),
                    "--pattern" => pattern = Some(value),
                    _ => results = Some(PathBuf::from(value)),
                }
            }
            Some(path) if !path.starts_with("--") => break PathBuf::from(path),
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            }
        }
    };
    options.args = args.collect();
    let Some(folder) = batch else {
        if pattern.is_some() || results.is_some() {
            eprintln!("--pattern and --results need --batch\n{USAGE}");
            return ExitCode::from(2);
        }
        let print: Rc<dyn Fn(&str)> = Rc::new(|line| println!("{line}"));
        let debug: Rc<dyn Fn(&str)> = Rc::new(|line| eprintln!("{line}"));
        return match run_file_split(&script, &options, print, debug) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("{}: {e}", script.display());
                ExitCode::FAILURE
            }
        };
    };
    let results = match results.map(|path| results_format(&path).map(|format| (path, format))).transpose() {
        Ok(results) => results,
        Err(message) => {
            eprintln!("{message}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    run_batch_command(&script, &folder, pattern, results.as_ref().map(|(p, f)| (p.as_path(), *f)), options)
}

/// The format `--results` writes, from the file's extension (any case).
///
/// # Errors
/// A message naming the accepted extensions when the path has none of them.
fn results_format(path: &Path) -> Result<ExportFormat, String> {
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    ExportFormat::ALL.into_iter().find(|f| f.extension() == ext).ok_or_else(|| {
        let names: Vec<String> = ExportFormat::ALL.iter().map(|f| format!(".{}", f.extension())).collect();
        let accepted = match names.split_last() {
            Some((last, rest)) if !rest.is_empty() => format!("{} or {last}", rest.join(", ")),
            _ => names.concat(),
        };
        format!("--results {}: the file must end in {accepted}", path.display())
    })
}

fn run_batch_command(
    script: &Path,
    folder: &Path,
    pattern: Option<String>,
    results: Option<(&Path, ExportFormat)>,
    mut options: RunOptions,
) -> ExitCode {
    let source = match std::fs::read_to_string(script) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("could not read {}: {e}", script.display());
            return ExitCode::FAILURE;
        }
    };
    options.base_dir = script.parent().map(Path::to_path_buf).unwrap_or_default();
    // Script output shares standard output only when the results go to a file.
    let output_to_stdout = results.is_some();
    let batch = BatchOptions { pattern, run: options };
    let run = match run_batch(&source, folder, &batch, move |file, line| {
        let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if output_to_stdout {
            println!("{name}: {line}");
        } else {
            eprintln!("{name}: {line}");
        }
    }) {
        Ok(run) => run,
        Err(e) => {
            eprintln!("{}: {e}", script.display());
            return ExitCode::FAILURE;
        }
    };
    for item in &run.items {
        if let Err(e) = &item.result {
            eprintln!("{}: {e}", item.file.display());
        }
    }
    let table = run.table("Batch results");
    match results {
        Some((path, format)) => {
            if let Err(e) = app_model::files::export_table(&table, format, path) {
                eprintln!("{e}");
                return ExitCode::FAILURE;
            }
        }
        None => print!("{}", ExportFormat::Csv.write_text(&table).unwrap_or_default()),
    }
    eprintln!("{} file(s) run, {} failed", run.items.len(), run.failures());
    if run.failures() > 0 { ExitCode::FAILURE } else { ExitCode::SUCCESS }
}
