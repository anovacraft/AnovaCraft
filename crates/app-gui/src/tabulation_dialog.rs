//! Dialogs for the multi-way table (Describe ▸ Multi-way table…) and the
//! stratified contingency analysis (Compare ▸ Stratified tables and exact
//! tests…).
//!
//! Both reports run through [`app_model::ComparisonReport`], so their
//! settings (statistics, totals, test options) are also edited in place
//! from the report's options menu ([`report_settings`]).

use app_model::{CellFrequency, CellSummary, MultiwayTableRequest, SavedReport, StratifiedRequest};
use eframe::egui::{self, RichText};
use stat_data::DataTable;

use crate::dialog_kit::{self as kit, ComboExt as _, Dialog, Outcome, Readiness, Remember};
use crate::report_view::ReportRequest;

/// Which report the dialog sets up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabulationTool {
    MultiwayTable,
    Stratified,
}

/// Dialog of the two tabulation reports.
#[derive(Clone)]
pub struct TabulationDialog {
    edit: Option<u64>,
    tool: TabulationTool,
    rows: Vec<usize>,
    columns: Vec<usize>,
    measures: Vec<usize>,
    frequency: Option<usize>,
    row: Option<usize>,
    column: Option<usize>,
    strata: Vec<usize>,
    multiway: MultiwayTableRequest,
    stratified: StratifiedRequest,
}

impl TabulationDialog {
    pub fn new(tool: TabulationTool) -> Self {
        Self {
            edit: None,
            tool,
            rows: Vec::new(),
            columns: Vec::new(),
            measures: Vec::new(),
            frequency: None,
            row: None,
            column: None,
            strata: Vec::new(),
            multiway: MultiwayTableRequest::new(Vec::new(), Vec::new()),
            stratified: StratifiedRequest::new(0, 0, Vec::new()),
        }
    }

    /// The dialog filled from a saved multi-way table or stratified analysis.
    pub fn from_saved(saved: &SavedReport, table: &DataTable) -> Option<Self> {
        let n = table.n_cols();
        let keep = |cols: &[usize]| cols.iter().copied().filter(|&c| c < n).collect::<Vec<_>>();
        let d = match saved {
            SavedReport::MultiwayTable { request } => Self {
                rows: keep(&request.rows),
                columns: keep(&request.columns),
                measures: keep(&request.measures),
                frequency: request.frequency.and_then(|c| kit::valid(table, c)),
                multiway: request.clone(),
                ..Self::new(TabulationTool::MultiwayTable)
            },
            SavedReport::StratifiedTables { request } => Self {
                row: kit::valid(table, request.row),
                column: kit::valid(table, request.column),
                strata: keep(&request.strata),
                frequency: request.frequency.and_then(|c| kit::valid(table, c)),
                stratified: request.clone(),
                ..Self::new(TabulationTool::Stratified)
            },
            _ => return None,
        };
        Some(d)
    }

    fn title(&self) -> &'static str {
        match self.tool {
            TabulationTool::MultiwayTable => "Multi-way table",
            TabulationTool::Stratified => "Stratified tables and exact tests",
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<Box<ReportRequest>> {
        let dialog = Dialog::new(self.title()).editing(self.edit);
        let outcome = dialog.show(ctx, |ui| {
            self.contents(ui, table)
                .map(|r| kit::Ready { output: Box::new(ReportRequest::Comparison(r.output)), summary: r.summary })
        });
        match outcome {
            Outcome::Reset => {
                *self = Self { edit: self.edit, ..Self::new(self.tool) };
                Outcome::Open
            }
            outcome => outcome,
        }
    }

    fn contents(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<SavedReport> {
        // Columns removed while the dialog is open drop out of every role.
        let n = table.n_cols();
        for list in [&mut self.rows, &mut self.columns, &mut self.measures, &mut self.strata] {
            list.retain(|&c| c < n);
        }
        for one in [&mut self.frequency, &mut self.row, &mut self.column] {
            *one = one.filter(|&c| c < n);
        }
        let numeric: Vec<usize> = (0..n).filter(|&i| kit::is_numeric(table, i)).collect();
        let all: Vec<usize> = (0..n).collect();
        match self.tool {
            TabulationTool::MultiwayTable => self.multiway_contents(ui, table, &all, &numeric),
            TabulationTool::Stratified => self.stratified_contents(ui, table, &all),
        }
    }

    fn multiway_contents(
        &mut self,
        ui: &mut egui::Ui,
        table: &DataTable,
        all: &[usize],
        numeric: &[usize],
    ) -> Readiness<SavedReport> {
        kit::note(
            ui,
            "Tick classification columns in nesting order (outermost first). Each cell shows the chosen counts \
             and percentages and the statistics of the continuous columns.",
        );
        let taken: Vec<usize> = self.frequency.into_iter().collect();
        ui.horizontal_top(|ui| {
            // What is summarized on the left, the classifications on the right.
            let others: Vec<usize> = self.rows.iter().chain(&self.columns).chain(&taken).copied().collect();
            pick_list(ui, table, "Continuous columns", numeric, &mut self.measures, &others, "mw_measures");
            let others: Vec<usize> = self.columns.iter().chain(&self.measures).chain(&taken).copied().collect();
            pick_list(ui, table, "Rows", all, &mut self.rows, &others, "mw_rows");
            let others: Vec<usize> = self.rows.iter().chain(&self.measures).chain(&taken).copied().collect();
            pick_list(ui, table, "Columns", all, &mut self.columns, &others, "mw_cols");
        });
        let name = |i: usize| kit::name(table, i).to_owned();
        let nesting = |cols: &[usize]| cols.iter().map(|&i| name(i)).collect::<Vec<_>>().join(" › ");
        if !self.rows.is_empty() {
            ui.label(RichText::new(format!("Rows: {}", nesting(&self.rows))).small());
        }
        if !self.columns.is_empty() {
            ui.label(RichText::new(format!("Columns: {}", nesting(&self.columns))).small());
        }
        multiway_settings(ui, &mut self.multiway);
        let used: Vec<usize> = self.rows.iter().chain(&self.columns).chain(&self.measures).copied().collect();
        kit::more_options(ui, |ui| {
            frequency_choice(ui, "mw_freq", table, &mut self.frequency, |i| {
                kit::is_numeric(table, i) && !used.contains(&i)
            });
        });
        if self.rows.is_empty() && self.columns.is_empty() {
            return Err("Choose at least one row or column classification".into());
        }
        let shows_measures = !self.measures.is_empty() && !self.multiway.statistics.is_empty();
        if self.multiway.frequencies.is_empty() && !shows_measures {
            return Err("Choose a count or percentage, or a continuous column and a statistic".into());
        }
        let mut summary = String::from("Multi-way table");
        if !self.rows.is_empty() {
            summary.push_str(&format!(" of {}", nesting(&self.rows)));
        }
        if !self.columns.is_empty() {
            summary.push_str(&format!(" by {}", nesting(&self.columns)));
        }
        kit::ready(
            SavedReport::MultiwayTable {
                request: MultiwayTableRequest {
                    rows: self.rows.clone(),
                    columns: self.columns.clone(),
                    measures: self.measures.clone(),
                    frequency: self.frequency,
                    ..self.multiway.clone()
                },
            },
            summary,
        )
    }

    fn stratified_contents(&mut self, ui: &mut egui::Ui, table: &DataTable, all: &[usize]) -> Readiness<SavedReport> {
        kit::note(
            ui,
            "One row per observation (or a frequency column of counts). For 2 × 2 tables the row variable is \
             the exposure or treatment and the column variable the outcome; the strata are the combinations of \
             the stratum columns. Leave the strata empty for one table with its exact test.",
        );
        let any = |_: usize| true;
        egui::Grid::new("strat_cols").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label("Rows:");
            kit::column_choice(ui, "strat_row", table, &mut self.row, any, None);
            ui.end_row();
            ui.label("Columns:");
            kit::column_choice(ui, "strat_col", table, &mut self.column, any, None);
            ui.end_row();
        });
        let picked: Vec<usize> = self.row.into_iter().chain(self.column).chain(self.frequency).collect();
        pick_list(ui, table, "Strata", all, &mut self.strata, &picked, "strat_strata");
        ui.checkbox(&mut self.stratified.exact, "Fisher's exact test of each table");
        let used: Vec<usize> = self.row.into_iter().chain(self.column).chain(self.strata.iter().copied()).collect();
        kit::more_options(ui, |ui| {
            frequency_choice(ui, "strat_freq", table, &mut self.frequency, |i| {
                kit::is_numeric(table, i) && !used.contains(&i)
            });
            stratified_options(ui, &mut self.stratified);
        });
        let Some(row) = self.row else { return Err("Choose the rows column".into()) };
        let Some(column) = self.column else { return Err("Choose the columns column".into()) };
        if row == column || self.strata.contains(&row) || self.strata.contains(&column) {
            return Err("Rows, columns and strata must be different columns".into());
        }
        let mut summary = format!("{} by {}", kit::name(table, row), kit::name(table, column));
        if !self.strata.is_empty() {
            let strata: Vec<&str> = self.strata.iter().map(|&c| kit::name(table, c)).collect();
            summary.push_str(&format!(", stratified by {}", strata.join(" × ")));
        }
        kit::ready(
            SavedReport::StratifiedTables {
                request: StratifiedRequest {
                    row,
                    column,
                    strata: self.strata.clone(),
                    frequency: self.frequency,
                    ..self.stratified.clone()
                },
            },
            summary,
        )
    }
}

impl Remember for TabulationDialog {
    fn memory_key(&self) -> String {
        self.title().into()
    }

    fn set_edit(&mut self, report: Option<u64>) {
        self.edit = report;
    }

    fn prefill(&mut self, columns: &[usize], _table: &DataTable) {
        match self.tool {
            TabulationTool::MultiwayTable => self.rows = columns.to_vec(),
            TabulationTool::Stratified => {
                self.row = columns.first().copied();
                self.column = columns.get(1).copied();
                self.strata = columns.iter().skip(2).copied().collect();
            }
        }
    }
}

/// The optional frequency (count) column.
fn frequency_choice(
    ui: &mut egui::Ui,
    id: &str,
    table: &DataTable,
    value: &mut Option<usize>,
    allow: impl Fn(usize) -> bool,
) {
    ui.horizontal(|ui| {
        ui.label("Frequency column:");
        kit::column_choice(ui, id, table, value, allow, Some("None (one per row)"));
    });
}

/// A list of check boxes; ticking appends to `chosen` (so the order of
/// ticking is the nesting order). Columns in `other` roles are disabled.
fn pick_list(
    ui: &mut egui::Ui,
    table: &DataTable,
    label: &str,
    choices: &[usize],
    chosen: &mut Vec<usize>,
    other: &[usize],
    id: &str,
) {
    ui.vertical(|ui| {
        ui.strong(label);
        egui::ScrollArea::vertical().id_salt(id).max_height(200.0).min_scrolled_width(160.0).show(ui, |ui| {
            for &i in choices {
                let name = table.column(i).map_or("", |c| c.name.as_str());
                let position = chosen.iter().position(|&c| c == i);
                let mut on = position.is_some();
                let text = match position {
                    Some(p) if chosen.len() > 1 => format!("{name} ({})", p + 1),
                    _ => name.to_owned(),
                };
                let enabled = !other.contains(&i);
                let response = ui.add_enabled(enabled, egui::Checkbox::new(&mut on, text));
                // Found by the column name, whatever its position number.
                response.widget_info(|| {
                    egui::WidgetInfo::selected(egui::WidgetType::Checkbox, enabled, on, format!("{label}: {name}"))
                });
                if response.changed() {
                    if on {
                        chosen.push(i);
                    } else {
                        chosen.retain(|&c| c != i);
                    }
                }
            }
        });
    });
}

/// Statistics and totals of a multi-way table.
fn multiway_settings(ui: &mut egui::Ui, r: &mut MultiwayTableRequest) {
    ui.horizontal_wrapped(|ui| {
        ui.label("In each cell:");
        for f in CellFrequency::ALL {
            let mut on = r.frequencies.contains(&f);
            if ui.checkbox(&mut on, f.label()).changed() {
                r.frequencies = CellFrequency::ALL
                    .into_iter()
                    .filter(|&x| if x == f { on } else { r.frequencies.contains(&x) })
                    .collect();
            }
        }
    });
    ui.horizontal_wrapped(|ui| {
        ui.label("Of each continuous column:");
        for s in CellSummary::ALL {
            let mut on = r.statistics.contains(&s);
            if ui.checkbox(&mut on, s.label()).changed() {
                r.statistics = CellSummary::ALL
                    .into_iter()
                    .filter(|&x| if x == s { on } else { r.statistics.contains(&x) })
                    .collect();
            }
        }
    });
    ui.horizontal(|ui| {
        ui.checkbox(&mut r.margins, "Totals (All row and column)");
        ui.checkbox(&mut r.subtotals, "Subtotals of outer levels");
    });
}

/// Test settings of a stratified analysis.
fn stratified_settings(ui: &mut egui::Ui, r: &mut StratifiedRequest) {
    ui.checkbox(&mut r.exact, "Fisher's exact test of each table");
    stratified_options(ui, r);
}

/// Confidence level, false-discovery-rate adjustment across strata and the
/// Monte Carlo fallback of a stratified analysis.
fn stratified_options(ui: &mut egui::Ui, r: &mut StratifiedRequest) {
    ui.horizontal(|ui| {
        ui.label("Adjust each stratum's p-values for the false discovery rate:").on_hover_text(
            "With many strata some tests come out significant by chance; the adjusted p-values (q-values) control \
             the expected share of false findings among those called significant",
        );
        let text = r.fdr.map_or("Off", app_model::FdrMethod::label);
        egui::ComboBox::from_id_salt("strat_fdr").selected_text(text).show_list(ui, |ui| {
            ui.selectable_value(&mut r.fdr, None, "Off");
            for m in app_model::FdrMethod::ALL {
                ui.selectable_value(&mut r.fdr, Some(m), m.label());
            }
        });
    });
    ui.horizontal(|ui| {
        ui.label("Confidence level of the common odds ratio:");
        ui.add(egui::DragValue::new(&mut r.confidence).speed(0.005).range(0.5..=0.999));
    });
    ui.add_enabled_ui(r.exact, |ui| {
        ui.horizontal(|ui| {
            ui.label("Tables too large for the exact computation: Monte Carlo with");
            ui.add(egui::DragValue::new(&mut r.monte_carlo_tables).speed(100.0).range(100..=1_000_000));
            ui.label("random tables, seed");
            ui.add(crate::dialog_kit::seed_value(&mut r.seed).speed(1.0));
        });
    });
}

/// In-report settings of the reports this module sets up; `false` for any
/// other report.
pub fn report_settings(ui: &mut egui::Ui, saved: &mut SavedReport) -> bool {
    match saved {
        SavedReport::MultiwayTable { request } => multiway_settings(ui, request),
        SavedReport::StratifiedTables { request } => stratified_settings(ui, request),
        _ => return false,
    }
    true
}
