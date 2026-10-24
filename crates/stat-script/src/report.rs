//! The `Report` script type: a computed analysis whose sections are
//! available as data (tables as arrays of row maps, plots as their points)
//! and can be written to files.

use std::fmt::Write as _;
use std::path::Path;
use std::rc::Rc;

use app_model::editing::Edit;
use app_model::html_export::{self, HtmlReport, HtmlSource};
use app_model::report_export::{self, ExportOptions, ExportScope, PaperSize, ReportFormat};
use app_model::report_figure::section_figure;
use app_model::{
    ComputedReport, ReportColumn, ReportSection, SavedReport, SectionContent, TextTable, analysis_key, graph_figure,
    report_columns, report_from_json, report_to_json,
};
use rhai::{Array, Dynamic, Engine, FLOAT, INT, Map};
use stat_data::{DataTable, RowStateManager};
use stat_viz::export::VectorFormat;

use crate::Host;
use crate::table::Table;
use crate::value::{ScriptResult, fail, from_json, report_cell, script_err};

/// A computed report as scripts see it.
#[derive(Debug, Clone)]
pub struct Report(Rc<Inner>);

#[derive(Debug)]
struct Inner {
    computed: ComputedReport,
    /// The analysis identifier, e.g. `"linear-model"`.
    key: String,
    /// The request as JSON (columns by name).
    request: stat_data::JsonValue,
    /// The data a graph canvas is drawn from (graphs only).
    graph_data: Option<(DataTable, RowStateManager)>,
    /// The table the report was run on, for `save_columns`.
    owner: Option<Table>,
    /// Row states when the report ran, so figures mark excluded rows.
    rows: RowStateManager,
    /// Name of the table the report was run on (HTML page headings).
    table_name: String,
}

impl Report {
    pub(crate) fn new(
        computed: ComputedReport,
        table: &DataTable,
        rows: &RowStateManager,
        owner: Option<Table>,
    ) -> Self {
        let key = analysis_key(&computed.saved, table);
        let request = report_to_json(&computed.saved, table);
        let graph_data = matches!(computed.saved, SavedReport::Graph { .. }).then(|| (table.clone(), rows.clone()));
        let table_name = table.name().to_owned();
        Self(Rc::new(Inner { computed, key, request, graph_data, owner, rows: rows.clone(), table_name }))
    }

    /// The columns the report offers, computed on its table as it is now
    /// (the request is matched to the table's columns by name).
    fn offered_columns(&self) -> ScriptResult<(Table, Vec<ReportColumn>)> {
        let owner = self.0.owner.clone().ok_or_else(|| script_err("this report is not attached to a table"))?;
        let columns = {
            let d = owner.0.borrow();
            let saved = report_from_json(&self.0.request, &d.table)
                .map_err(|e| script_err(format!("the report no longer fits its table: {e}")))?;
            report_columns(&saved, &d.table, &d.rows).map_err(script_err)?
        };
        Ok((owner, columns))
    }

    /// Add the report's columns `keys` (all it offers when `None`) to its
    /// table; returns the new columns' names.
    fn save_columns(&self, keys: Option<&[String]>) -> ScriptResult<Array> {
        let (owner, offered) = self.offered_columns()?;
        if offered.is_empty() {
            return fail(format!("{:?} has no columns to save", self.0.computed.title));
        }
        let chosen: Vec<ReportColumn> = match keys {
            None => offered,
            Some(keys) => keys
                .iter()
                .map(|k| {
                    offered.iter().find(|c| c.key == *k).cloned().ok_or_else(|| {
                        let list: Vec<&str> = offered.iter().map(|c| c.key.as_str()).collect();
                        script_err(format!("{:?} offers the columns {list:?}; got {k:?}", self.0.computed.title))
                    })
                })
                .collect::<ScriptResult<_>>()?,
        };
        let mut names = Array::new();
        for c in chosen {
            let (index, name) = {
                let d = owner.0.borrow();
                let name =
                    if d.table.find(&c.name).is_some() { d.table.unused_column_name(&c.name) } else { c.name.clone() };
                (d.table.n_cols(), name)
            };
            let column = c.into_column(name.clone());
            owner.apply(Edit::InsertColumn { index, column })?;
            names.push(name.into());
        }
        Ok(names)
    }

    /// The computed title and sections.
    #[must_use]
    pub fn computed(&self) -> &ComputedReport {
        &self.0.computed
    }

    fn find(&self, title: &str) -> ScriptResult<&ReportSection> {
        let sections = &self.0.computed.sections;
        sections
            .iter()
            .find(|s| s.title == title)
            .or_else(|| sections.iter().find(|s| s.title.eq_ignore_ascii_case(title)))
            // Titles may carry details after the name, e.g. "Influence Diagnostics (2 of 60 rows flagged)".
            .or_else(|| {
                sections.iter().find(|s| {
                    let lower = s.title.to_lowercase();
                    lower
                        .strip_prefix(&title.to_lowercase())
                        .is_some_and(|rest| rest.starts_with(" (") || rest.starts_with(':'))
                })
            })
            .ok_or_else(|| {
                let list: Vec<String> = sections.iter().map(|s| format!("{:?}", s.title)).collect();
                script_err(format!(
                    "{:?} has no section {title:?}; its sections are {}",
                    self.0.computed.title,
                    list.join(", ")
                ))
            })
    }

    /// One cell of a table section: the row whose first cell is `row`, in
    /// `column` (default: the second column, as in a statistic–value table).
    fn value(&self, title: &str, row: &str, column: Option<&str>) -> ScriptResult<Dynamic> {
        let table = match &self.find(title)?.content {
            SectionContent::Table(t) => t.clone(),
            SectionContent::EffectPareto(p) => p.table(),
            _ => return fail(format!("section {title:?} is not a table")),
        };
        let j = match column {
            None if table.columns.len() >= 2 => 1,
            None => return fail(format!("section {title:?} has a single column; name the column")),
            Some(c) => table.columns.iter().position(|h| h == c).ok_or_else(|| {
                script_err(format!("section {title:?} has no column {c:?}; its columns are {:?}", table.columns))
            })?,
        };
        let cells = table
            .rows
            .iter()
            .find(|r| r.first().is_some_and(|f| f == row))
            .ok_or_else(|| script_err(format!("section {title:?} has no row {row:?}")))?;
        Ok(report_cell(&cells[j]))
    }

    /// The whole report as Markdown text: title, then each section (tables
    /// in full, plots summarised).
    #[must_use]
    pub fn text(&self) -> String {
        let mut out = format!("# {}\n", self.0.computed.title);
        for s in &self.0.computed.sections {
            let _ = write!(out, "\n## {}\n\n", s.title);
            match &s.content {
                SectionContent::Table(t) => out.push_str(&markdown_table(t)),
                SectionContent::EffectPareto(p) => out.push_str(&markdown_table(&p.table())),
                SectionContent::Note(n) => {
                    out.push_str(n);
                    out.push('\n');
                }
                SectionContent::Formula(f) => {
                    let _ = writeln!(out, "```\n{f}\n```");
                }
                other => {
                    let _ = writeln!(out, "_{}_", plot_summary(other));
                }
            }
        }
        out
    }

    /// The whole report as a self-contained HTML page
    /// ([`app_model::html_export::report_html`]), every section included.
    #[must_use]
    pub fn html(&self) -> String {
        let date = html_export::export_date();
        html_export::report_html(HtmlReport::of(&self.0.computed), self.html_source(&date))
    }

    /// The table name, row states and (for graphs) data of this report, for
    /// an HTML page dated `date`.
    pub(crate) fn html_source<'a>(&'a self, date: &'a str) -> HtmlSource<'a> {
        HtmlSource {
            table_name: &self.0.table_name,
            rows: &self.0.rows,
            date,
            data: self.0.graph_data.as_ref().map(|(t, _)| t),
        }
    }

    /// Write the report: `.md` or `.txt` for the whole report as text;
    /// `.html` (or `.htm`) for a self-contained web page with tables and
    /// plots; `.pdf`, `.docx` (Word), `.pptx` (PowerPoint) or `.xlsx` (Excel)
    /// for a document with tables and plots ([`app_model::report_export`]),
    /// `paper` sizing PDF and Word pages; `.png`, `.svg`, `.pdf` or `.eps`
    /// for a graph canvas drawn as a figure.
    fn save(&self, path: &Path, paper: PaperSize) -> ScriptResult<()> {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        let is_graph = matches!(self.0.computed.saved, SavedReport::Graph { .. });
        let document = match ext.as_str() {
            "docx" => Some(ReportFormat::Word),
            "pptx" => Some(ReportFormat::PowerPoint),
            "xlsx" => Some(ReportFormat::Excel),
            "pdf" if !is_graph => Some(ReportFormat::Pdf),
            _ => None,
        };
        if let Some(format) = document {
            let date = html_export::export_date();
            let scope = ExportScope::One(HtmlReport::of(&self.0.computed));
            return report_export::export(scope, self.html_source(&date), format, ExportOptions { paper }, path)
                .map(|_| ())
                .map_err(script_err);
        }
        let bytes = match ext.as_str() {
            "md" | "txt" => self.text().into_bytes(),
            "html" | "htm" => self.html().into_bytes(),
            "png" | "svg" | "pdf" | "eps" => {
                let format = figure_format(&ext).expect("a figure extension");
                let (SavedReport::Graph { spec }, Some((table, rows))) = (&self.0.computed.saved, &self.0.graph_data)
                else {
                    return fail(
                        "only graph canvases save whole as figures; save one plot with save_section(title, path)",
                    );
                };
                graph_figure(spec, table, rows, format).map_err(script_err)?
            }
            _ => {
                return fail(format!(
                    "cannot save a report as {ext:?}; use .pdf, .docx, .pptx, .xlsx, .html, .md, .txt, \
                     or .png/.svg/.pdf/.eps for graphs"
                ));
            }
        };
        write_file(path, &bytes)
    }

    /// Write one section: its data as CSV (`.csv`) or tab-separated text
    /// (`.tsv`, `.tab`, `.txt`) — a table as shown, or a plot's points with
    /// their 1-based row numbers, as the data view numbers rows — or a plot
    /// as a `.png` picture or an `.svg`, `.pdf` or `.eps` figure.
    fn save_section(&self, title: &str, path: &Path) -> ScriptResult<()> {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        let section = self.find(title)?;
        let format = match ext.as_str() {
            "csv" | "tsv" | "tab" | "txt" => {
                let sep = if ext == "csv" { ',' } else { '\t' };
                let table = section_table(&section.content);
                return write_file(path, delimited(&table, sep).as_bytes());
            }
            _ => match figure_format(&ext) {
                Some(format) => format,
                None => {
                    return fail(format!(
                        "sections save as .csv, .tsv, or (plots) .png, .svg, .pdf or .eps; got {ext:?}"
                    ));
                }
            },
        };
        let Some(bytes) = section_figure(section, Some(&self.0.rows), format) else {
            return fail(format!("section {title:?} is a table, not a plot; save it as .csv or .tsv"));
        };
        write_file(path, &bytes)
    }
}

/// The paper size in a script's options map (`#{paper: "a4"}`; Letter when
/// absent).
pub(crate) fn paper_option(options: &Map) -> ScriptResult<PaperSize> {
    match options.get("paper") {
        None => Ok(PaperSize::default()),
        Some(v) => {
            let name = v.clone().into_string().map_err(|_| script_err("paper is a text: \"letter\" or \"a4\""))?;
            PaperSize::from_key(&name)
                .ok_or_else(|| script_err(format!("unknown paper {name:?}: use \"letter\" or \"a4\"")))
        }
    }
}

/// The figure format a file extension names (`png`, `svg`, `pdf`, `eps`).
fn figure_format(ext: &str) -> Option<VectorFormat> {
    VectorFormat::ALL.into_iter().find(|f| f.extension() == ext)
}

pub(crate) fn write_file(path: &Path, bytes: &[u8]) -> ScriptResult<()> {
    std::fs::write(path, bytes).map_err(|e| script_err(format!("could not write {}: {e}", path.display())))
}

/// A section's content as a table of text cells (plots: one row per point).
fn section_table(content: &SectionContent) -> TextTable {
    let num = |x: f64| stat_data::format_number(x);
    match content {
        SectionContent::Table(t) => t.clone(),
        SectionContent::EffectPareto(p) => p.table(),
        SectionContent::WordCloud(w) => w.table(),
        SectionContent::Note(n) => {
            let mut t = TextTable::new(["Note"]);
            t.push([n.clone()]);
            t
        }
        SectionContent::Formula(f) => {
            let mut t = TextTable::new(["Formula"]);
            t.push([f.clone()]);
            t
        }
        SectionContent::Tree(d) => {
            let mut t = TextTable::new(["Node", "Parent", "Rule", "Summary", "Leaf"]);
            for (i, n) in d.nodes.iter().enumerate() {
                let parent =
                    d.nodes.iter().position(|p| p.children.contains(&i)).map_or(String::new(), |p| (p + 1).to_string());
                t.push([
                    (i + 1).to_string(),
                    parent,
                    n.rule.clone(),
                    n.summary.clone(),
                    n.leaf.map_or(String::new(), |l| l.to_string()),
                ]);
            }
            t
        }
        SectionContent::Plot(p) => {
            let mut t = TextTable::new(["Row", p.x_label.as_str(), p.y_label.as_str()]);
            for q in &p.points {
                t.push([(q.row + 1).to_string(), num(q.x), num(q.y)]);
            }
            t
        }
        SectionContent::BoxPlot(b) => {
            let mut t = TextTable::new(["Row", b.x_label.as_str()]);
            for q in &b.points {
                t.push([(q.row + 1).to_string(), num(q.x)]);
            }
            t
        }
        SectionContent::Bars(b) => {
            let mut t = TextTable::new(["Center", "Width", "Count"]);
            for bar in &b.bars {
                t.push([num(bar.center), num(bar.width), bar.count().to_string()]);
            }
            t
        }
        SectionContent::ControlChart(c) => {
            let labelled = c.points.iter().any(|p| p.label.is_some());
            let mut header = vec!["Sample", c.y_label.as_str(), "Rules"];
            if labelled {
                header.insert(1, "Label");
            }
            let noted = c.points.iter().any(|p| p.note.is_some());
            if noted {
                header.push("Note");
            }
            let mut t = TextTable::new(header);
            for p in &c.points {
                let mut rules: Vec<String> = p.rules.iter().map(ToString::to_string).collect();
                if p.signal {
                    rules.push("signal".to_owned());
                }
                let mut row = vec![p.sample.to_string(), num(p.value), rules.join(" ")];
                if labelled {
                    row.insert(1, p.label.clone().unwrap_or_default());
                }
                if noted {
                    row.push(p.note.clone().unwrap_or_default());
                }
                t.push(row);
            }
            t
        }
        SectionContent::Pareto(p) => p.table(),
        SectionContent::CauseEffect(d) => d.table(),
        SectionContent::HeatMap(h) => h.table(),
        SectionContent::IntensityMap(m) => m.table(),
        SectionContent::Raster(r) => app_model::image_plot::raster_table(r),
        SectionContent::Survival(s) => {
            let mut t = TextTable::new(["Row", s.x_label.as_str(), s.y_label.as_str(), "Censored"]);
            for (q, &c) in s.points.iter().zip(&s.censored) {
                t.push([(q.row + 1).to_string(), num(q.x), num(q.y), if c { "yes" } else { "no" }.to_owned()]);
            }
            t
        }
    }
}

fn plot_summary(content: &SectionContent) -> String {
    match content {
        SectionContent::Plot(p) => format!("Plot of {} against {} ({} points).", p.y_label, p.x_label, p.points.len()),
        SectionContent::BoxPlot(b) => format!("Box plot of {} ({} values).", b.x_label, b.points.len()),
        SectionContent::Bars(b) => format!("Bar chart of {} ({} bars).", b.x_label, b.bars.len()),
        SectionContent::ControlChart(c) => format!("Control chart of {} ({} points).", c.y_label, c.points.len()),
        SectionContent::Survival(s) => format!("Survival plot ({} curves).", s.curves.len()),
        SectionContent::Tree(d) => format!(
            "Tree diagram ({} nodes, {} leaves).",
            d.nodes.len(),
            d.nodes.iter().filter(|n| n.leaf.is_some()).count()
        ),
        SectionContent::Pareto(p) => format!(
            "Pareto chart of {} ({} bars in {} panels).",
            p.category,
            p.panels.iter().map(|q| q.bars.len()).sum::<usize>(),
            p.panels.len()
        ),
        SectionContent::CauseEffect(d) => {
            format!("Cause-and-effect diagram of {} ({} main branches).", d.effect, d.branches.len())
        }
        SectionContent::HeatMap(h) => {
            format!("Heat map of {} ({} × {} cells).", h.measure, h.rows.len(), h.columns.len())
        }
        SectionContent::IntensityMap(m) => format!(
            "Map of {} over {} and {} ({} × {} cells).",
            m.value_label,
            m.x_label,
            m.y_label,
            m.x.len(),
            m.y.len()
        ),
        SectionContent::Raster(r) => format!(
            "Image plot of {} over {} and {} ({} × {} cells).",
            r.value_title, r.x_title, r.y_title, r.cols, r.rows
        ),
        _ => String::new(),
    }
}

fn markdown_table(t: &TextTable) -> String {
    let esc = |s: &str| s.replace('|', "\\|");
    let mut out = format!(
        "| {} |\n|{}\n",
        t.columns.iter().map(|c| esc(c)).collect::<Vec<_>>().join(" | "),
        "---|".repeat(t.columns.len())
    );
    for r in &t.rows {
        let _ = writeln!(out, "| {} |", r.iter().map(|c| esc(c)).collect::<Vec<_>>().join(" | "));
    }
    out
}

fn delimited(t: &TextTable, sep: char) -> String {
    let field = |s: &str| {
        if sep == '\t' {
            s.replace(['\t', '\n'], " ")
        } else if s.contains([',', '"', '\n']) {
            format!("\"{}\"", s.replace('"', "\"\""))
        } else {
            s.to_owned()
        }
    };
    let line = |cells: &[String]| cells.iter().map(|c| field(c)).collect::<Vec<_>>().join(&sep.to_string());
    let mut out = line(&t.columns);
    out.push('\n');
    for r in &t.rows {
        out.push_str(&line(r));
        out.push('\n');
    }
    out
}

fn points(p: &[app_model::RowPoint]) -> Array {
    p.iter()
        .map(|q| {
            let mut m = Map::new();
            m.insert("row".into(), (q.row as INT).into());
            m.insert("x".into(), (q.x as FLOAT).into());
            m.insert("y".into(), (q.y as FLOAT).into());
            Dynamic::from_map(m)
        })
        .collect()
}

fn polyline(p: &[[f64; 2]]) -> Dynamic {
    Dynamic::from_array(p.iter().map(|&[x, y]| Dynamic::from_array(vec![x.into(), y.into()])).collect())
}

fn map<const N: usize>(members: [(&str, Dynamic); N]) -> Dynamic {
    Dynamic::from_map(members.into_iter().map(|(k, v)| (k.into(), v)).collect::<Map>())
}

/// A section's content as a script value.
fn section_value(content: &SectionContent) -> Dynamic {
    let text = |s: &str| Dynamic::from(s.to_owned());
    match content {
        SectionContent::Table(_) | SectionContent::EffectPareto(_) => {
            let t = section_table(content);
            Dynamic::from_array(
                t.rows
                    .iter()
                    .map(|r| {
                        Dynamic::from_map(
                            t.columns.iter().zip(r).map(|(c, v)| (c.as_str().into(), report_cell(v))).collect::<Map>(),
                        )
                    })
                    .collect(),
            )
        }
        SectionContent::Note(n) | SectionContent::Formula(n) => text(n),
        SectionContent::Plot(p) => map([
            ("kind", text("plot")),
            ("x_label", text(&p.x_label)),
            ("y_label", text(&p.y_label)),
            ("points", Dynamic::from_array(points(&p.points))),
            ("line", p.line.as_deref().map_or(Dynamic::UNIT, polyline)),
            (
                "band",
                p.band.as_ref().map_or(Dynamic::UNIT, |(lo, hi)| Dynamic::from_array(vec![polyline(lo), polyline(hi)])),
            ),
            ("contours", p.contour.as_ref().map_or(Dynamic::UNIT, contour_map)),
        ]),
        SectionContent::BoxPlot(b) => map([
            ("kind", text("box-plot")),
            ("x_label", text(&b.x_label)),
            ("q1", b.q1.into()),
            ("median", b.median.into()),
            ("q3", b.q3.into()),
            ("whisker_low", b.whisker_low.into()),
            ("whisker_high", b.whisker_high.into()),
            ("mean", b.mean.into()),
            ("points", Dynamic::from_array(points(&b.points))),
            ("outliers", Dynamic::from_array(b.outliers.iter().map(|&i| (i as INT).into()).collect())),
        ]),
        SectionContent::Bars(b) => map([
            ("kind", text("bars")),
            ("x_label", text(&b.x_label)),
            ("y_label", text(&b.y_label)),
            (
                "bars",
                Dynamic::from_array(
                    b.bars
                        .iter()
                        .map(|bar| {
                            map([
                                ("center", bar.center.into()),
                                ("width", bar.width.into()),
                                ("count", (bar.count() as INT).into()),
                                ("rows", Dynamic::from_array(bar.rows.iter().map(|&r| (r as INT).into()).collect())),
                            ])
                        })
                        .collect(),
                ),
            ),
        ]),
        SectionContent::ControlChart(c) => map([
            ("kind", text("control-chart")),
            ("x_label", text(&c.x_label)),
            ("y_label", text(&c.y_label)),
            ("center", c.center.into()),
            ("lower", c.lower.into()),
            ("upper", c.upper.into()),
            (
                "points",
                Dynamic::from_array(
                    c.points
                        .iter()
                        .map(|p| {
                            map([
                                ("sample", (p.sample as INT).into()),
                                ("value", p.value.into()),
                                ("rows", Dynamic::from_array(p.rows.iter().map(|&r| (r as INT).into()).collect())),
                                ("rules", Dynamic::from_array(p.rules.iter().map(|&r| INT::from(r).into()).collect())),
                                ("label", p.label.as_deref().map_or(Dynamic::UNIT, text)),
                                ("note", p.note.as_deref().map_or(Dynamic::UNIT, text)),
                                ("signal", p.signal.into()),
                            ])
                        })
                        .collect(),
                ),
            ),
            (
                "limits",
                Dynamic::from_array(
                    c.limit_steps()
                        .iter()
                        .map(|s| {
                            map([
                                ("from", s.from.into()),
                                ("to", s.to.into()),
                                ("center", s.center.into()),
                                ("lower", s.lower.into()),
                                ("upper", s.upper.into()),
                            ])
                        })
                        .collect(),
                ),
            ),
            (
                "phases",
                Dynamic::from_array(
                    c.phases.iter().map(|m| map([("at", m.at.into()), ("label", text(&m.label))])).collect(),
                ),
            ),
        ]),
        SectionContent::Survival(s) => map([
            ("kind", text("survival")),
            ("x_label", text(&s.x_label)),
            ("y_label", text(&s.y_label)),
            (
                "curves",
                Dynamic::from_array(
                    s.curves.iter().map(|c| map([("label", text(&c.label)), ("steps", polyline(&c.steps))])).collect(),
                ),
            ),
            ("points", Dynamic::from_array(points(&s.points))),
            ("censored", Dynamic::from_array(s.censored.iter().map(|&c| c.into()).collect())),
        ]),
        SectionContent::Pareto(p) => map([
            ("kind", text("pareto")),
            ("category", text(&p.category)),
            ("y_label", text(&p.y_label)),
            (
                "panels",
                Dynamic::from_array(
                    p.panels
                        .iter()
                        .map(|q| {
                            map([
                                ("label", text(&q.label)),
                                ("total", q.total.into()),
                                (
                                    "bars",
                                    Dynamic::from_array(
                                        q.bars
                                            .iter()
                                            .map(|b| {
                                                map([
                                                    ("label", text(&b.label)),
                                                    ("value", b.value.into()),
                                                    ("share", b.share.into()),
                                                    ("cumulative", b.cumulative.into()),
                                                    ("other", (!b.pooled.is_empty()).into()),
                                                    (
                                                        "rows",
                                                        Dynamic::from_array(
                                                            b.rows.iter().map(|&r| (r as INT).into()).collect(),
                                                        ),
                                                    ),
                                                ])
                                            })
                                            .collect(),
                                    ),
                                ),
                            ])
                        })
                        .collect(),
                ),
            ),
        ]),
        SectionContent::WordCloud(w) => map([
            ("kind", text("word-cloud")),
            (
                "words",
                Dynamic::from_array(
                    w.words
                        .iter()
                        .map(|x| {
                            map([
                                ("text", text(&x.text)),
                                ("count", (x.count as INT).into()),
                                ("size", Dynamic::from(x.size)),
                                ("rows", Dynamic::from_array(x.rows.iter().map(|&r| (r as INT).into()).collect())),
                            ])
                        })
                        .collect(),
                ),
            ),
        ]),
        SectionContent::HeatMap(h) => map([
            ("kind", text("heat-map")),
            ("measure", text(&h.measure)),
            ("rows", Dynamic::from_array(h.rows.iter().map(|r| text(r)).collect())),
            ("columns", Dynamic::from_array(h.columns.iter().map(|c| text(c)).collect())),
            (
                "values",
                Dynamic::from_array(
                    h.values
                        .iter()
                        .map(|row| Dynamic::from_array(row.iter().map(|&v| Dynamic::from(v)).collect()))
                        .collect(),
                ),
            ),
        ]),
        SectionContent::IntensityMap(m) => {
            let nums = |v: &[f64]| Dynamic::from_array(v.iter().map(|&x| Dynamic::from(x)).collect());
            let y: Vec<f64> = m.y.iter().map(|&y| m.y_value(y)).collect();
            map([
                ("kind", text("intensity-map")),
                ("x_label", text(&m.x_label)),
                ("y_label", text(&m.y_label)),
                ("value_label", text(&m.value_label)),
                ("x", nums(&m.x)),
                ("y", nums(&y)),
                ("values", Dynamic::from_array(m.values.iter().map(|c| nums(c)).collect())),
            ])
        }
        SectionContent::Raster(r) => {
            let nums = |v: &[f64]| Dynamic::from_array(v.iter().map(|&x| Dynamic::from(x)).collect());
            let rows: Vec<Dynamic> = r.values.chunks(r.cols.max(1)).map(nums).collect();
            map([
                ("kind", text("image")),
                ("x_label", text(&r.x_title)),
                ("y_label", text(&r.y_title)),
                ("value_label", text(&r.value_title)),
                ("x_range", nums(&[r.x_range.0, r.x_range.1])),
                ("y_range", nums(&[r.y_range.0, r.y_range.1])),
                ("y_down", Dynamic::from(r.y_down)),
                ("values", Dynamic::from_array(rows)),
            ])
        }
        SectionContent::CauseEffect(d) => {
            fn node(n: &app_model::CauseNode) -> Dynamic {
                map([
                    ("label", Dynamic::from(n.label.clone())),
                    ("rows", Dynamic::from_array(n.rows.iter().map(|&r| (r as INT).into()).collect())),
                    ("children", Dynamic::from_array(n.children.iter().map(node).collect())),
                ])
            }
            map([
                ("kind", text("cause-effect")),
                ("effect", text(&d.effect)),
                ("branches", Dynamic::from_array(d.branches.iter().map(node).collect())),
            ])
        }
        SectionContent::Tree(d) => map([
            ("kind", text("tree")),
            ("classes", Dynamic::from_array(d.classes.iter().map(|c| text(c)).collect())),
            (
                "nodes",
                Dynamic::from_array(
                    d.nodes
                        .iter()
                        .map(|n| {
                            map([
                                ("rule", text(&n.rule)),
                                ("summary", text(&n.summary)),
                                ("rows", Dynamic::from_array(n.rows.iter().map(|&r| (r as INT).into()).collect())),
                                (
                                    "children",
                                    Dynamic::from_array(n.children.iter().map(|&c| (c as INT).into()).collect()),
                                ),
                                ("shares", Dynamic::from_array(n.shares.iter().map(|&p| p.into()).collect())),
                                ("leaf", n.leaf.map_or(Dynamic::UNIT, |l| (l as INT).into())),
                            ])
                        })
                        .collect(),
                ),
            ),
        ]),
    }
}

/// Register the `Report` type and its functions.
pub(crate) fn register(engine: &mut Engine, host: &Rc<Host>) {
    engine.register_type_with_name::<Report>("Report");
    engine.register_get("title", |r: &mut Report| r.0.computed.title.clone());
    engine.register_get("analysis", |r: &mut Report| r.0.key.clone());
    engine.register_fn("to_string", |r: &mut Report| r.text());
    engine.register_fn("to_debug", |r: &mut Report| format!("Report {:?}", r.0.computed.title));
    engine.register_fn("text", |r: &mut Report| r.text());
    engine.register_fn("sections", |r: &mut Report| -> Array {
        r.0.computed.sections.iter().map(|s| s.title.clone().into()).collect()
    });
    engine.register_fn("has_section", |r: &mut Report, title: &str| r.find(title).is_ok());
    engine.register_fn("section", |r: &mut Report, title: &str| -> ScriptResult<Dynamic> {
        Ok(section_value(&r.find(title)?.content))
    });
    engine.register_fn("request", |r: &mut Report| from_json(&r.0.request));
    engine.register_fn("value", |r: &mut Report, title: &str, row: &str| r.value(title, row, None));
    engine
        .register_fn("value", |r: &mut Report, title: &str, row: &str, column: &str| r.value(title, row, Some(column)));
    engine.register_fn("saveable_columns", |r: &mut Report| -> ScriptResult<Array> {
        let (_, columns) = r.offered_columns()?;
        Ok(columns
            .into_iter()
            .map(|c| {
                let mut m = Map::new();
                m.insert("key".into(), c.key.into());
                m.insert("name".into(), c.name.into());
                Dynamic::from_map(m)
            })
            .collect())
    });
    engine.register_fn("save_columns", |r: &mut Report| r.save_columns(None));
    engine.register_fn("save_columns", |r: &mut Report, keys: Array| -> ScriptResult<Array> {
        let keys: Vec<String> = keys.iter().map(ToString::to_string).collect();
        r.save_columns(Some(&keys))
    });
    let h = Rc::clone(host);
    engine.register_fn("save", move |r: &mut Report, path: &str| r.save(&h.resolve(path), PaperSize::default()));
    let h = Rc::clone(host);
    engine.register_fn("save", move |r: &mut Report, path: &str, options: Map| {
        r.save(&h.resolve(path), paper_option(&options)?)
    });
    let h = Rc::clone(host);
    engine.register_fn("save_section", move |r: &mut Report, title: &str, path: &str| {
        r.save_section(title, &h.resolve(path))
    });
}

/// A contour map as script data: its levels (in the report's style) and
/// each level's lines, `#{ levels: [..], lines: [#{ level, lines: [[[x, y], ..], ..] }, ..] }`.
fn contour_map(m: &app_model::report::ContourMap) -> Dynamic {
    let d = m.drawing(false);
    let lines = d
        .lines
        .iter()
        .map(|(level, ls)| {
            map([("level", (*level).into()), ("lines", Dynamic::from_array(ls.iter().map(|l| polyline(l)).collect()))])
        })
        .collect();
    map([
        ("levels", Dynamic::from_array(d.levels.iter().map(|&l| l.into()).collect())),
        ("lines", Dynamic::from_array(lines)),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delimited_text_quotes_only_where_needed() {
        let mut t = TextTable::new(["a", "b,c"]);
        t.push(["1", "say \"hi\""]);
        assert_eq!(delimited(&t, ','), "a,\"b,c\"\n1,\"say \"\"hi\"\"\"\n");
        assert_eq!(delimited(&t, '\t'), "a\tb,c\n1\tsay \"hi\"\n");
    }

    #[test]
    fn markdown_tables_escape_bars() {
        let mut t = TextTable::new(["x|y"]);
        t.push(["1"]);
        assert_eq!(markdown_table(&t), "| x\\|y |\n|---|\n| 1 |\n");
    }
}
