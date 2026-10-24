//! Exporting reports as documents: the "Export reports" dialog, opened from
//! a report's Export… button or ⋯ Options ▸ Export report…, and from
//! File ▸ Export reports… (⌘/Ctrl+Shift+E), in the window it was asked
//! from. It chooses what to export (every report of the table by default,
//! or just the report it was opened from), whether collapsed sections and
//! reports are left out (they are included by default), the format (PDF,
//! Word, PowerPoint, HTML, Markdown or Excel) and, for PDF and Word, the
//! paper size; the format and paper are remembered in the settings. The
//! files themselves are written by [`app_model::report_export`].

use std::collections::HashSet;
use std::path::PathBuf;

use eframe::egui;

use app_model::html_export::{self, HtmlReport, HtmlSource};
use app_model::report_export::{self, ExportOptions, ExportScope, PaperSize, ReportFormat};
use app_model::{ReportSection, SavedReport, SectionKind};
use stat_data::{DataTable, RowStateManager};

use crate::dialog_kit::{self, Dialog, Outcome};
use crate::file_io;

/// A report as an export sees it.
pub struct ReportForExport {
    /// The report heading.
    pub title: String,
    /// The request (graph canvases are drawn from it).
    pub saved: SavedReport,
    /// The sections, or why the analysis could not be computed.
    pub sections: Result<Vec<ReportSection>, String>,
    /// Indices of the sections collapsed on screen.
    pub closed: Vec<usize>,
    /// The whole report is collapsed on screen.
    pub collapsed: bool,
}

impl ReportForExport {
    /// The report as the page writer takes it.
    pub fn page(&self) -> HtmlReport<'_> {
        HtmlReport {
            title: &self.title,
            saved: &self.saved,
            sections: self.sections.as_deref().map_err(String::as_str),
            closed: &self.closed,
        }
    }
}

/// Indices of `sections` whose (kind, occurrence of that kind) key is in
/// `closed`, numbering occurrences as the report view does.
pub fn closed_indices(sections: &[ReportSection], closed: &HashSet<(SectionKind, usize)>) -> Vec<usize> {
    let mut seen: std::collections::HashMap<SectionKind, usize> = std::collections::HashMap::new();
    sections
        .iter()
        .enumerate()
        .filter_map(|(i, s)| {
            let n = seen.entry(s.kind).or_default();
            let key = (s.kind, *n);
            *n += 1;
            closed.contains(&key).then_some(i)
        })
        .collect()
}

/// The open export dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportDialog {
    /// The document whose reports are exported.
    pub doc: u64,
    /// The report it was opened from, if any.
    pub report: Option<u64>,
    /// Export every report of the table (rather than `report`).
    pub all: bool,
    /// Leave out the sections, and reports, collapsed on screen.
    pub skip_collapsed: bool,
    /// The chosen format.
    pub format: ReportFormat,
    /// The paper size of PDF and Word documents.
    pub paper: PaperSize,
}

/// What the dialog asks to export.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportChoice {
    /// The report to export, or `None` for every report of the table.
    pub report: Option<u64>,
    /// Leave out the sections, and reports, collapsed on screen.
    pub skip_collapsed: bool,
    /// The format.
    pub format: ReportFormat,
    /// Paper size of PDF and Word documents.
    pub paper: PaperSize,
}

impl ExportDialog {
    /// A dialog for document `doc`, opened from report `report` when given
    /// (which can then be exported alone), starting from every report with
    /// collapsed sections included, in the format and paper last used.
    pub fn new(doc: u64, report: Option<u64>, format: ReportFormat, paper: PaperSize) -> Self {
        Self { doc, report, all: true, skip_collapsed: false, format, paper }
    }

    /// Draw the dialog. `report_title` is the title of the report it was
    /// opened from (if it still exists); `n_reports` counts the table's
    /// reports.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        report_title: Option<&str>,
        table_name: &str,
        n_reports: usize,
    ) -> Outcome<ExportChoice> {
        Dialog::new("Export reports").without_rows().without_reset().action("Save as…").show(ctx, |ui| {
            if report_title.is_none() {
                self.all = true;
            }
            ui.label(egui::RichText::new("What to export").strong());
            let every = match n_reports {
                1 => format!("The report of {table_name}"),
                n => format!("All {n} reports of {table_name}"),
            };
            ui.radio_value(&mut self.all, true, every);
            if let Some(title) = report_title {
                ui.radio_value(&mut self.all, false, format!("Only this report: {title}"));
            }
            ui.checkbox(&mut self.skip_collapsed, "Leave out collapsed sections and reports").on_hover_text(
                "Export only what is open on screen. Otherwise every section is exported, collapsed or not.",
            );
            ui.add_space(6.0);
            ui.label(egui::RichText::new("Format").strong());
            for f in ReportFormat::ALL {
                ui.radio_value(&mut self.format, f, format!("{} (.{})", f.label(), f.extension()))
                    .on_hover_text(f.description());
                ui.indent(("export format", f.key()), |ui| {
                    ui.label(egui::RichText::new(f.description()).weak().small());
                });
            }
            if self.format.is_paged() {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Paper size").strong());
                    for p in PaperSize::ALL {
                        ui.radio_value(&mut self.paper, p, p.label());
                    }
                });
            }
            let report = if self.all { None } else { self.report };
            if self.all && n_reports == 0 {
                return Err(format!("{table_name} has no reports yet"));
            }
            let what = match report_title.filter(|_| !self.all) {
                Some(title) => format!("“{title}”"),
                None => "every report".to_owned(),
            };
            dialog_kit::ready(
                ExportChoice { report, skip_collapsed: self.skip_collapsed, format: self.format, paper: self.paper },
                format!("Save {what} as {}", self.format.label()),
            )
        })
    }
}

/// What an export of `reports` holds: every section of every report, or,
/// with `skip_collapsed`, only what is open on screen (collapsed reports
/// left out, unless `one` report is exported).
pub fn prepare(mut reports: Vec<ReportForExport>, one: bool, skip_collapsed: bool) -> Vec<ReportForExport> {
    if skip_collapsed {
        if !one {
            reports.retain(|r| !r.collapsed);
        }
    } else {
        for r in &mut reports {
            r.closed.clear();
        }
    }
    reports
}

/// Ask where to save `reports` (one report when `one` is set, else all of
/// them under the table's name) as `choice` and write the files. `None`
/// when the save dialog was cancelled; otherwise the main file's path or
/// the message to show.
pub fn export(
    reports: &[ReportForExport],
    one: bool,
    table: &DataTable,
    rows: &RowStateManager,
    format: ReportFormat,
    paper: PaperSize,
) -> Option<Result<PathBuf, String>> {
    let name = match (one, reports.first()) {
        (true, Some(r)) => r.title.clone(),
        _ => format!("{} reports", table.name()),
    };
    let path =
        file_io::pick_save_file(&format!("Export as {}", format.label()), format.label(), format.extension(), &name)?;
    let date = html_export::export_date();
    let source = HtmlSource { table_name: table.name(), rows, date: &date, data: Some(table) };
    let pages: Vec<HtmlReport<'_>> = reports.iter().map(ReportForExport::page).collect();
    let scope = match (one, pages.first()) {
        (true, Some(page)) => ExportScope::One(*page),
        _ => ExportScope::All(&pages),
    };
    Some(report_export::export(scope, source, format, ExportOptions { paper }, &path))
}
