//! Dialogs for resampling: Compare ▸ Permutation test… and "Bootstrap this
//! table…", which a report's table offers from its right-click menu and the
//! report's ⋯ Options menu.
//!
//! A table menu cannot reach the application, so it leaves a
//! [`BootstrapStart`] in egui's temporary memory ([`request_bootstrap`]) and
//! the application picks it up the next frame ([`take_bootstrap_request`])
//! and opens the [`BootstrapDialog`].

use app_model::permutation_analysis::{CorrelationStatistic, GroupStatistic, MAX_RESAMPLES};
use app_model::resampling::{JACKKNIFE_LIMIT, MAX_REPLICATES, numeric_cells};
use app_model::{Alternative, BootstrapRequest, PermutationDesign, PermutationRequest, SavedReport, TextTable};
use eframe::egui::{self, RichText};
use stat_data::DataTable;

use crate::dialog_kit::{self as kit, Dialog, Outcome, Readiness, Remember};
use crate::report_view::ReportRequest;

/// Title of the permutation-test dialog.
pub const PERMUTATION_TITLE: &str = "Permutation test";
/// Title of the bootstrap dialog.
pub const BOOTSTRAP_TITLE: &str = "Bootstrap a table";

/// A table chosen for bootstrapping: the report that made it, the section's
/// title and the table as shown.
#[derive(Clone)]
pub struct BootstrapStart {
    pub saved: SavedReport,
    pub report_title: String,
    pub section: String,
    pub table: TextTable,
}

fn start_id() -> egui::Id {
    egui::Id::new("bootstrap_table_request")
}

/// Ask the application to open the bootstrap dialog for `start`.
pub fn request_bootstrap(ctx: &egui::Context, start: BootstrapStart) {
    ctx.data_mut(|d| d.insert_temp(start_id(), start));
}

/// The bootstrap the user asked for since the last frame, if any.
pub fn take_bootstrap_request(ctx: &egui::Context) -> Option<BootstrapStart> {
    ctx.data_mut(|d| {
        let start = d.get_temp::<BootstrapStart>(start_id());
        d.remove::<BootstrapStart>(start_id());
        start
    })
}

/// The arrangement being tested.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Design {
    TwoGroups,
    Paired,
    Correlation,
}

/// Compare ▸ Permutation test….
#[derive(Clone)]
pub struct PermutationDialog {
    edit: Option<u64>,
    design: Design,
    first: Option<usize>,
    second: Option<usize>,
    group_statistic: GroupStatistic,
    correlation: CorrelationStatistic,
    alternative: Alternative,
    resamples: usize,
    seed: u64,
    exact_limit: u64,
}

impl Default for PermutationDialog {
    fn default() -> Self {
        let d = PermutationRequest::new(PermutationDesign::Paired { first: 0, second: 0 });
        Self {
            edit: None,
            design: Design::TwoGroups,
            first: None,
            second: None,
            group_statistic: GroupStatistic::MeanDifference,
            correlation: CorrelationStatistic::Pearson,
            alternative: d.alternative,
            resamples: d.resamples,
            seed: d.seed,
            exact_limit: d.exact_limit,
        }
    }
}

impl PermutationDialog {
    /// The dialog filled from a saved permutation test.
    pub fn from_saved(saved: &SavedReport, table: &DataTable) -> Option<Self> {
        let SavedReport::PermutationTest { request } = saved else { return None };
        let valid = |c: usize| kit::valid(table, c);
        let base = Self {
            alternative: request.alternative,
            resamples: request.resamples,
            seed: request.seed,
            exact_limit: request.exact_limit,
            ..Self::default()
        };
        Some(match request.design {
            PermutationDesign::TwoGroups { response, group, statistic } => Self {
                design: Design::TwoGroups,
                first: valid(response),
                second: valid(group),
                group_statistic: statistic,
                ..base
            },
            PermutationDesign::Paired { first, second } => {
                Self { design: Design::Paired, first: valid(first), second: valid(second), ..base }
            }
            PermutationDesign::Correlation { x, y, statistic } => {
                Self { design: Design::Correlation, first: valid(x), second: valid(y), correlation: statistic, ..base }
            }
        })
    }

    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<Box<ReportRequest>> {
        let dialog = Dialog::new(PERMUTATION_TITLE).editing(self.edit);
        let outcome = dialog.show(ctx, |ui| {
            self.contents(ui, table).map(|r| kit::Ready {
                output: Box::new(ReportRequest::Resampling(Box::new(SavedReport::PermutationTest {
                    request: r.output,
                }))),
                summary: r.summary,
            })
        });
        match outcome {
            Outcome::Reset => {
                *self = Self { edit: self.edit, ..Self::default() };
                Outcome::Open
            }
            outcome => outcome,
        }
    }

    fn contents(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<PermutationRequest> {
        kit::note(
            ui,
            "Compares the statistic with its values over rearrangements of the data that the null hypothesis \
             makes equally likely: every arrangement when there are few, random ones otherwise.",
        );
        ui.horizontal(|ui| {
            for (d, label) in [
                (Design::TwoGroups, "Two groups"),
                (Design::Paired, "Paired columns"),
                (Design::Correlation, "Correlation"),
            ] {
                if ui.radio_value(&mut self.design, d, label).changed() {
                    self.first = None;
                    self.second = None;
                }
            }
        });
        let numeric = |i: usize| kit::is_numeric(table, i);
        let any_column = self.design == Design::TwoGroups;
        let (first_label, second_label) = match self.design {
            Design::TwoGroups => ("Response:", "Group (two levels):"),
            Design::Paired => ("First:", "Second (difference first − second):"),
            Design::Correlation => ("X:", "Y:"),
        };
        egui::Grid::new("permutation_columns").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label(first_label);
            kit::column_choice(ui, "perm_first", table, &mut self.first, numeric, None);
            ui.end_row();
            ui.label(second_label);
            kit::column_choice(ui, "perm_second", table, &mut self.second, |i| any_column || numeric(i), None);
            ui.end_row();
        });
        match self.design {
            Design::TwoGroups => {
                ui.horizontal(|ui| {
                    ui.label("Statistic:");
                    for s in [GroupStatistic::MeanDifference, GroupStatistic::MedianDifference] {
                        ui.radio_value(&mut self.group_statistic, s, s.label());
                    }
                });
            }
            Design::Paired => {
                ui.label("Statistic: mean difference (signs flipped at random under the null)");
            }
            Design::Correlation => {
                ui.horizontal(|ui| {
                    ui.label("Statistic:");
                    for s in [CorrelationStatistic::Pearson, CorrelationStatistic::Spearman] {
                        ui.radio_value(&mut self.correlation, s, s.label());
                    }
                });
            }
        }
        ui.horizontal(|ui| {
            ui.label("Alternative:");
            for (a, label) in [
                (Alternative::TwoSided, "Two-sided"),
                (Alternative::Less, "Statistic lower"),
                (Alternative::Greater, "Statistic higher"),
            ] {
                ui.radio_value(&mut self.alternative, a, label);
            }
        });
        kit::more_options(ui, |ui| {
            egui::Grid::new("permutation_settings").num_columns(2).show(ui, |ui| {
                ui.label("Exact when arrangements are at most:");
                ui.add(egui::DragValue::new(&mut self.exact_limit).speed(1000.0).range(1..=10_000_000));
                ui.end_row();
                ui.label("Otherwise, random arrangements:");
                ui.add(egui::DragValue::new(&mut self.resamples).speed(100.0).range(99..=MAX_RESAMPLES));
                ui.end_row();
                ui.label("Seed:");
                ui.add(crate::dialog_kit::seed_value(&mut self.seed).speed(1.0));
                ui.end_row();
            });
        });
        let Some(first) = self.first else {
            return Err(format!("Choose the {}", first_label.trim_end_matches(':').to_lowercase()));
        };
        let Some(second) = self.second else {
            return Err(match self.design {
                Design::TwoGroups => "Choose the group column".into(),
                Design::Paired => "Choose the second column".into(),
                Design::Correlation => "Choose the Y column".into(),
            });
        };
        if first == second {
            return Err("A column is chosen twice".into());
        }
        let design = match self.design {
            Design::TwoGroups => {
                PermutationDesign::TwoGroups { response: first, group: second, statistic: self.group_statistic }
            }
            Design::Paired => PermutationDesign::Paired { first, second },
            Design::Correlation => PermutationDesign::Correlation { x: first, y: second, statistic: self.correlation },
        };
        let summary = match self.design {
            Design::TwoGroups => {
                format!(
                    "Permutation test of {} between the groups of {}",
                    kit::name(table, first),
                    kit::name(table, second)
                )
            }
            Design::Paired => {
                format!("Permutation test of {} − {}", kit::name(table, first), kit::name(table, second))
            }
            Design::Correlation => format!(
                "Permutation test of the correlation of {} and {}",
                kit::name(table, first),
                kit::name(table, second)
            ),
        };
        kit::ready(
            PermutationRequest {
                design,
                alternative: self.alternative,
                resamples: self.resamples,
                seed: self.seed,
                exact_limit: self.exact_limit,
            },
            summary,
        )
    }
}

impl Remember for PermutationDialog {
    fn memory_key(&self) -> String {
        PERMUTATION_TITLE.into()
    }

    fn set_edit(&mut self, report: Option<u64>) {
        self.edit = report;
    }
}

/// "Bootstrap this table…": which cells, how many replicates, seed, level
/// and strata.
#[derive(Clone)]
pub struct BootstrapDialog {
    start: BootstrapStart,
    /// Columns of the table with numeric cells, and whether each is chosen.
    columns: Vec<(String, bool)>,
    /// Row labels with numeric cells, and whether each is chosen.
    rows: Vec<(String, bool)>,
    replicates: usize,
    seed: u64,
    level: f64,
    stratify: Option<usize>,
}

impl BootstrapDialog {
    pub fn new(start: BootstrapStart) -> Self {
        let cells = numeric_cells(&start.table);
        let mut columns: Vec<(String, bool)> = Vec::new();
        let mut rows: Vec<(String, bool)> = Vec::new();
        for (row, column) in cells {
            if !columns.iter().any(|c| c.0 == column) {
                columns.push((column, false));
            }
            if !rows.iter().any(|r| r.0 == row) {
                rows.push((row, true));
            }
        }
        // Keep the table's column order.
        columns.sort_by_key(|c| start.table.columns.iter().position(|h| *h == c.0));
        if let Some(c) = columns.first_mut() {
            c.1 = true;
        }
        let d = BootstrapRequest::new(start.saved.clone(), "", Vec::new());
        Self { start, columns, rows, replicates: d.replicates, seed: d.seed, level: d.level, stratify: None }
    }

    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<Box<ReportRequest>> {
        let dialog = Dialog::new(BOOTSTRAP_TITLE);
        let outcome = dialog.show(ctx, |ui| {
            self.contents(ui, table).map(|r| kit::Ready {
                output: Box::new(ReportRequest::Resampling(Box::new(SavedReport::Bootstrap { request: r.output }))),
                summary: r.summary,
            })
        });
        match outcome {
            Outcome::Reset => {
                *self = Self::new(self.start.clone());
                Outcome::Open
            }
            outcome => outcome,
        }
    }

    fn contents(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<BootstrapRequest> {
        ui.label(RichText::new(format!("{} · {}", self.start.report_title, self.start.section)).strong());
        kit::note(
            ui,
            &format!(
                "Re-runs the report on resamples of its rows (drawn with replacement) and collects the chosen \
                 cells: bias, standard error, percentile and BCa intervals. BCa needs the jackknife, computed for \
                 up to {JACKKNIFE_LIMIT} rows."
            ),
        );
        if self.columns.is_empty() {
            return Err("This table has no numeric cells to bootstrap".into());
        }
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.strong("Columns");
                egui::ScrollArea::vertical().id_salt("boot_columns").max_height(200.0).show(ui, |ui| {
                    for (name, on) in &mut self.columns {
                        ui.checkbox(on, name.as_str());
                    }
                });
            });
            ui.vertical(|ui| {
                ui.strong("Rows");
                egui::ScrollArea::vertical().id_salt("boot_rows").max_height(200.0).show(ui, |ui| {
                    for (name, on) in &mut self.rows {
                        ui.checkbox(on, name.as_str());
                    }
                });
            });
        });
        egui::Grid::new("bootstrap_settings").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label("Replicates:");
            ui.add(egui::DragValue::new(&mut self.replicates).speed(50.0).range(100..=MAX_REPLICATES));
            ui.end_row();
            ui.label("Confidence level:");
            ui.add(egui::DragValue::new(&mut self.level).speed(0.005).range(0.5..=0.999));
            ui.end_row();
        });
        kit::more_options(ui, |ui| {
            egui::Grid::new("bootstrap_more").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                ui.label("Seed:");
                ui.add(crate::dialog_kit::seed_value(&mut self.seed).speed(1.0));
                ui.end_row();
                ui.label("Resample within levels of:");
                kit::column_choice(ui, "boot_strata", table, &mut self.stratify, |_| true, Some("(no strata)"));
                ui.end_row();
            });
        });
        let columns: Vec<String> = self.columns.iter().filter(|c| c.1).map(|c| c.0.clone()).collect();
        let rows: Vec<String> = self.rows.iter().filter(|r| r.1).map(|r| r.0.clone()).collect();
        if columns.is_empty() || rows.is_empty() {
            return Err("Choose at least one column and one row".into());
        }
        let all_rows = rows.len() == self.rows.len();
        let summary = format!(
            "Bootstrap of {} in {}, {} replicates",
            kit::count(columns.len() * rows.len(), "cell", "cells"),
            self.start.section,
            self.replicates
        );
        let request = BootstrapRequest {
            rows: if all_rows { Vec::new() } else { rows },
            replicates: self.replicates,
            seed: self.seed,
            level: self.level,
            stratify: self.stratify,
            ..BootstrapRequest::new(self.start.saved.clone(), self.start.section.clone(), columns)
        };
        kit::ready(request, summary)
    }
}

impl Remember for BootstrapDialog {
    fn memory_key(&self) -> String {
        // Settings belong to one report's table, not to every bootstrap.
        format!("{BOOTSTRAP_TITLE}\u{1f}{}\u{1f}{}", self.start.report_title, self.start.section)
    }

    fn set_edit(&mut self, _report: Option<u64>) {}
}
