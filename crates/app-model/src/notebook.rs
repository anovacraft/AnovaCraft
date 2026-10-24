//! Script notebooks: a document of text cells and Rhai code cells with the
//! outputs of their last run (printed lines, errors, the cell's value shown
//! as text or as tables, and the reports it added), kept in a project.
//!
//! The cells are run by `stat_script::NotebookSession`, which keeps the
//! variables and functions of earlier cells for later ones, as a single
//! script would. Running is the application's business; this module is the
//! document and its saved form.
//!
//! # Saved form
//! A project's `"notebooks"` member (optional; see [`crate::project`]):
//!
//! ```text
//! "notebooks": [
//!   { "name": "…",
//!     "cells": [
//!       { "kind": "text", "source": "…" },
//!       { "kind": "code", "source": "…", "run": 3,          (run: optional)
//!         "outputs": [                                      (optional)
//!           { "kind": "printed", "text": "…" },
//!           { "kind": "error", "text": "…", "line": 4 },    (line: optional)
//!           { "kind": "value", "text": "…" },
//!           { "kind": "table", "title": "…", "columns": ["…"], "rows": [["…"], …] },
//!           { "kind": "report", "table": "…", "title": "…" },
//!           { "kind": "note", "text": "…" } ] },
//!       … ] },
//!   … ]
//! ```

use stat_core::Result;
use stat_data::JsonValue;

use crate::project::{array, invalid, member, obj, s};
use crate::report::TextTable;

/// What a cell holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellKind {
    /// Notes in plain text (lines starting with `#` show as headings).
    Text,
    /// Rhai code.
    Code,
}

/// One output of a code cell's last run.
#[derive(Debug, Clone, PartialEq)]
pub enum CellOutput {
    /// A line the cell printed with `print` or `debug`.
    Printed(String),
    /// Why the cell failed, with the cell's 1-based line when known.
    Error {
        /// What went wrong.
        message: String,
        /// The cell's 1-based line, when known.
        line: Option<usize>,
    },
    /// The value of the cell's last line, as text.
    Value(String),
    /// A table: the cell's value (a table, rows of a report section, a
    /// report's tables).
    Table {
        /// Heading (may be empty).
        title: String,
        /// The cells as text.
        table: TextTable,
    },
    /// A report the cell added to an open table.
    Report {
        /// Name of the table the report belongs to.
        table: String,
        /// The report's title.
        title: String,
    },
    /// What the run changed (from the application).
    Note(String),
}

/// One cell.
#[derive(Debug, Clone, PartialEq)]
pub struct NotebookCell {
    /// Text or code.
    pub kind: CellKind,
    /// The cell's text.
    pub source: String,
    /// Outputs of the last run (code cells).
    pub outputs: Vec<CellOutput>,
    /// The run number of the last run, counting up through the notebook's
    /// runs since it was opened (`None`: not run).
    pub run: Option<u32>,
}

impl NotebookCell {
    /// An empty cell.
    #[must_use]
    pub fn new(kind: CellKind, source: impl Into<String>) -> Self {
        Self { kind, source: source.into(), outputs: Vec::new(), run: None }
    }
}

/// A notebook: named cells.
#[derive(Debug, Clone, PartialEq)]
pub struct Notebook {
    /// Name, unique within the project.
    pub name: String,
    /// The cells, in order.
    pub cells: Vec<NotebookCell>,
}

impl Notebook {
    /// A new notebook with a heading and one empty code cell.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            cells: vec![
                NotebookCell::new(CellKind::Text, format!("# {name}\nNotes about this analysis.")),
                NotebookCell::new(CellKind::Code, "let t = current_table();\nt"),
            ],
            name,
        }
    }

    /// The notebook as one Rhai script: text cells become `//` comments and
    /// code cells follow each other, so it runs from the Script window or
    /// `anovacraft-run` as the cells would run from first to last.
    #[must_use]
    pub fn to_script(&self) -> String {
        let mut parts = Vec::new();
        for cell in &self.cells {
            match cell.kind {
                CellKind::Text => {
                    let text: Vec<String> = cell
                        .source
                        .lines()
                        .map(|l| if l.is_empty() { "//".to_owned() } else { format!("// {l}") })
                        .collect();
                    if !text.is_empty() {
                        parts.push(text.join("\n"));
                    }
                }
                CellKind::Code => {
                    let code = cell.source.trim_end();
                    if !code.is_empty() {
                        // A last line without `;` is the cell's value; end it so the next cell parses.
                        let ends = code.ends_with(';') || code.ends_with('}');
                        parts.push(if ends { code.to_owned() } else { format!("{code};") });
                    }
                }
            }
        }
        let mut out = parts.join("\n\n");
        out.push('\n');
        out
    }

    /// Clear every cell's outputs and run number.
    pub fn clear_outputs(&mut self) {
        for c in &mut self.cells {
            c.outputs.clear();
            c.run = None;
        }
    }

    /// The JSON form (see the module documentation).
    #[must_use]
    pub fn to_json(&self) -> JsonValue {
        let cells = self
            .cells
            .iter()
            .map(|c| {
                let mut m = vec![
                    ("kind".to_owned(), s(if c.kind == CellKind::Text { "text" } else { "code" })),
                    ("source".to_owned(), s(&c.source)),
                ];
                if let Some(run) = c.run {
                    m.push(("run".to_owned(), JsonValue::Number(f64::from(run))));
                }
                if !c.outputs.is_empty() {
                    m.push(("outputs".to_owned(), JsonValue::Array(c.outputs.iter().map(output_json).collect())));
                }
                JsonValue::Object(m)
            })
            .collect();
        obj([("name", s(&self.name)), ("cells", JsonValue::Array(cells))])
    }

    /// Read the JSON form.
    ///
    /// # Errors
    /// A member is missing or of the wrong kind.
    pub fn from_json(v: &JsonValue) -> Result<Self> {
        let name = text(v, "name")?;
        let mut cells = Vec::new();
        for c in array(v, "cells")? {
            let kind = match text(c, "kind")?.as_str() {
                "text" => CellKind::Text,
                "code" => CellKind::Code,
                other => return Err(invalid(format!("notebooks: unknown cell kind {other:?}"))),
            };
            let run = match c.get("run") {
                None | Some(JsonValue::Null) => None,
                Some(r) => Some(
                    r.as_usize()
                        .and_then(|r| u32::try_from(r).ok())
                        .ok_or_else(|| invalid("notebooks: \"run\" must be a whole number"))?,
                ),
            };
            let outputs = match c.get("outputs") {
                None | Some(JsonValue::Null) => Vec::new(),
                Some(_) => array(c, "outputs")?.iter().map(output_from_json).collect::<Result<_>>()?,
            };
            cells.push(NotebookCell { kind, source: text(c, "source")?, outputs, run });
        }
        Ok(Self { name, cells })
    }
}

fn text(v: &JsonValue, key: &str) -> Result<String> {
    member(v, key)?.as_str().map(str::to_owned).ok_or_else(|| invalid(format!("notebooks: {key:?} must be text")))
}

fn output_json(o: &CellOutput) -> JsonValue {
    match o {
        CellOutput::Printed(t) => obj([("kind", s("printed")), ("text", s(t))]),
        CellOutput::Value(t) => obj([("kind", s("value")), ("text", s(t))]),
        CellOutput::Note(t) => obj([("kind", s("note")), ("text", s(t))]),
        CellOutput::Error { message, line } => match line {
            Some(l) => obj([("kind", s("error")), ("text", s(message)), ("line", JsonValue::Number(*l as f64))]),
            None => obj([("kind", s("error")), ("text", s(message))]),
        },
        CellOutput::Report { table, title } => obj([("kind", s("report")), ("table", s(table)), ("title", s(title))]),
        CellOutput::Table { title, table } => obj([
            ("kind", s("table")),
            ("title", s(title)),
            ("columns", JsonValue::Array(table.columns.iter().map(|c| s(c)).collect())),
            (
                "rows",
                JsonValue::Array(
                    table.rows.iter().map(|r| JsonValue::Array(r.iter().map(|c| s(c)).collect())).collect(),
                ),
            ),
        ]),
    }
}

fn output_from_json(v: &JsonValue) -> Result<CellOutput> {
    Ok(match text(v, "kind")?.as_str() {
        "printed" => CellOutput::Printed(text(v, "text")?),
        "value" => CellOutput::Value(text(v, "text")?),
        "note" => CellOutput::Note(text(v, "text")?),
        "error" => CellOutput::Error {
            message: text(v, "text")?,
            line: match v.get("line") {
                None | Some(JsonValue::Null) => None,
                Some(l) => Some(l.as_usize().ok_or_else(|| invalid("notebooks: \"line\" must be a whole number"))?),
            },
        },
        "report" => CellOutput::Report { table: text(v, "table")?, title: text(v, "title")? },
        "table" => {
            let strings = |a: &JsonValue| -> Result<Vec<String>> {
                match a {
                    JsonValue::Array(items) => items
                        .iter()
                        .map(|i| {
                            i.as_str().map(str::to_owned).ok_or_else(|| invalid("notebooks: table cells must be text"))
                        })
                        .collect(),
                    _ => Err(invalid("notebooks: a table's columns and rows must be lists")),
                }
            };
            let columns = strings(member(v, "columns")?)?;
            let rows: Vec<Vec<String>> = array(v, "rows")?.iter().map(strings).collect::<Result<_>>()?;
            if rows.iter().any(|r| r.len() != columns.len()) {
                return Err(invalid("notebooks: every table row needs one cell per column"));
            }
            CellOutput::Table { title: text(v, "title")?, table: TextTable { columns, rows } }
        }
        other => return Err(invalid(format!("notebooks: unknown output kind {other:?}"))),
    })
}

/// The `"notebooks"` member of a project document (empty when absent).
///
/// # Errors
/// A malformed notebook, or two notebooks with one name.
pub fn read_notebooks(doc: &JsonValue) -> Result<Vec<Notebook>> {
    let list: Vec<Notebook> = match doc.get("notebooks") {
        None | Some(JsonValue::Null) => return Ok(Vec::new()),
        Some(_) => array(doc, "notebooks")?.iter().map(Notebook::from_json).collect::<Result<_>>()?,
    };
    for (i, n) in list.iter().enumerate() {
        if list[..i].iter().any(|m| m.name == n.name) {
            return Err(invalid(format!("notebooks: two notebooks are called {:?}", n.name)));
        }
    }
    Ok(list)
}

/// The JSON form of a project's notebooks.
#[must_use]
pub fn notebooks_json(notebooks: &[Notebook]) -> JsonValue {
    JsonValue::Array(notebooks.iter().map(Notebook::to_json).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Notebook {
        let mut n = Notebook::new("Checks");
        let mut table = TextTable::new(["Term", "Estimate"]);
        table.push(["Intercept", "1.5"]);
        n.cells[1].outputs = vec![
            CellOutput::Printed("hello".into()),
            CellOutput::Value("Table \"Norris\" (36 rows, 2 columns)".into()),
            CellOutput::Table { title: "Parameter Estimates".into(), table },
            CellOutput::Report { table: "Norris".into(), title: "Linear Model: Reading".into() },
            CellOutput::Note("1 report added".into()),
        ];
        n.cells[1].run = Some(2);
        n.cells.push(NotebookCell {
            kind: CellKind::Code,
            source: "let x = ;".into(),
            outputs: vec![CellOutput::Error { message: "bad".into(), line: Some(1) }],
            run: Some(3),
        });
        n
    }

    #[test]
    fn notebooks_round_trip_through_json() {
        let n = sample();
        let doc = obj([("notebooks", notebooks_json(std::slice::from_ref(&n)))]);
        let text = doc.to_pretty();
        let back = read_notebooks(&stat_data::parse_json(&text).unwrap()).unwrap();
        assert_eq!(back, [n]);
        assert!(read_notebooks(&obj([("x", s("y"))])).unwrap().is_empty());
    }

    #[test]
    fn malformed_notebooks_are_refused() {
        let n = sample().to_json();
        let twice = obj([("notebooks", JsonValue::Array(vec![n.clone(), n]))]);
        assert!(read_notebooks(&twice).unwrap_err().to_string().contains("two notebooks"));
        let bad =
            stat_data::parse_json(r#"{"notebooks": [{"name": "a", "cells": [{"kind": "slide", "source": ""}]}]}"#)
                .unwrap();
        assert!(read_notebooks(&bad).is_err());
        let ragged = stat_data::parse_json(
            r#"{"notebooks": [{"name": "a", "cells": [{"kind": "code", "source": "",
                "outputs": [{"kind": "table", "title": "", "columns": ["a", "b"], "rows": [["1"]]}]}]}]}"#,
        )
        .unwrap();
        assert!(read_notebooks(&ragged).is_err());
    }

    #[test]
    fn a_notebook_becomes_one_script() {
        let mut n = Notebook::new("Checks");
        n.cells[0].source = "# Checks\n\nFirst look.".into();
        n.cells.push(NotebookCell::new(CellKind::Code, "print(t.rows());\n"));
        n.cells.push(NotebookCell::new(CellKind::Code, "  "));
        assert_eq!(
            n.to_script(),
            "// # Checks\n//\n// First look.\n\nlet t = current_table();\nt;\n\nprint(t.rows());\n"
        );
        n.clear_outputs();
        assert!(n.cells.iter().all(|c| c.outputs.is_empty() && c.run.is_none()));
    }
}
