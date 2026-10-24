//! Reports as Markdown text (GitHub-flavoured: pipe tables with numeric
//! columns right-aligned), with each plot saved as a 300 ppi PNG in a
//! folder beside the file and linked from the text.

use std::fmt::Write as _;
use std::path::PathBuf;

use stat_viz::export::VectorFormat;

use super::{Block, DocTable, ExportFile, ReportDocument};

/// `text` with the characters Markdown would read as formatting escaped
/// (and line breaks turned into spaces when `inline`).
fn escape(text: &str, inline: bool) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '|' | '#' => {
                out.push('\\');
                out.push(c);
            }
            '\n' if inline => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

fn table(t: &DocTable) -> String {
    let cell = |s: &str| {
        let s = escape(s, true);
        if s.trim().is_empty() { " ".to_owned() } else { s }
    };
    let mut out = String::from("|");
    for h in &t.table.columns {
        let _ = write!(out, " {} |", cell(h));
    }
    out.push_str("\n|");
    for &numeric in &t.numeric {
        out.push_str(if numeric { " ---: |" } else { " --- |" });
    }
    out.push('\n');
    for row in &t.table.rows {
        out.push('|');
        for j in 0..t.table.columns.len() {
            let _ = write!(out, " {} |", cell(row.get(j).map_or("", String::as_str)));
        }
        out.push('\n');
    }
    out
}

/// `doc` as Markdown, and its plots as PNG files under `folder` (named
/// `plot-01.png`, …), which the text links to by relative path.
#[must_use]
pub fn write(doc: &ReportDocument<'_>, folder: &str) -> (String, Vec<ExportFile>) {
    let mut out = format!("# {}\n\n_{}_\n", escape(&doc.title, true), escape(&doc.meta, true));
    let mut files = Vec::new();
    for b in &doc.blocks {
        out.push('\n');
        match b {
            Block::Heading(h) => {
                let _ = writeln!(out, "{} {}", "#".repeat(usize::from(h.level) + 1), escape(&h.text, true));
            }
            Block::Note(n) => {
                // A line break inside a paragraph: two trailing spaces.
                let lines: Vec<String> = n.lines().map(|l| escape(l, true)).collect();
                let _ = writeln!(out, "{}", lines.join("  \n"));
            }
            Block::Formula(f) => {
                let fence = if f.contains("```") { "~~~~" } else { "```" };
                let _ = writeln!(out, "{fence}text\n{}\n{fence}", f.trim_end());
            }
            Block::Table(t) if !t.table.columns.is_empty() => out.push_str(&table(t)),
            Block::Table(_) => {}
            Block::Figure(f) => match f.render(VectorFormat::Png) {
                Ok(png) => {
                    let name = format!("plot-{:02}.png", files.len() + 1);
                    let link = format!("{folder}/{name}");
                    let _ = writeln!(out, "![{}]({})", escape(&f.alt_text(), true), link.replace(' ', "%20"));
                    files.push((PathBuf::from(folder).join(name), png));
                }
                Err(e) => {
                    let _ = writeln!(out, "_{}_", escape(&e, true));
                }
            },
        }
    }
    (out, files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::html_export::HtmlReport;
    use crate::report::TextTable;
    use crate::report_export::ExportScope;
    use crate::report_export::tests::{plant_growth, source};

    #[test]
    fn tables_and_escaping() {
        let mut t = TextTable::new(["Term", "p-value"]);
        t.push(["a|b *c*", "< 0.0001"]);
        t.push(["", "0.25"]);
        let md = table(&DocTable::new(t));
        assert_eq!(md, "| Term | p-value |\n| --- | ---: |\n| a\\|b \\*c\\* | \\< 0.0001 |\n|   | 0.25 |\n");
    }

    #[test]
    fn documents_link_their_plots() {
        let (table, rows, computed) = plant_growth();
        let doc = ReportDocument::new(ExportScope::One(HtmlReport::of(&computed[0])), source(&table, &rows));
        let (text, files) = write(&doc, "plants files");
        assert!(text.starts_with(&format!("# {}\n", escape(&computed[0].title, true))));
        for s in &computed[0].sections {
            assert!(text.contains(&format!("## {}\n", escape(&s.title, true))), "{}", s.title);
        }
        assert!(!files.is_empty());
        for (path, bytes) in &files {
            assert!(bytes.starts_with(b"\x89PNG"));
            let name = path.file_name().unwrap().to_str().unwrap();
            assert!(text.contains(&format!("](plants%20files/{name})")));
        }
    }
}
