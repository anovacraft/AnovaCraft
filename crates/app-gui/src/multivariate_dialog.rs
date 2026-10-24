//! Dialog for a multivariate control chart (Hotelling T², generalized
//! variance |S| or MEWMA).

use app_model::{MewmaLimit, MultivariateChartKind, MultivariateChartRequest, SavedReport};
use eframe::egui;
use stat_data::DataTable;

use crate::dialog_kit::{self as kit, Dialog, Outcome, Readiness, Remember};

const TITLE: &str = "Multivariate control chart";

#[derive(Clone)]
pub struct MultivariateDialog {
    edit: Option<u64>,
    kind: MultivariateChartKind,
    /// Measurements, indexed by column.
    columns: Vec<bool>,
    /// Subgroups from an ID column (`true`) or consecutive runs of a size.
    subgroup_from_column: bool,
    subgroup: Option<usize>,
    subgroup_size: usize,
    label: Option<usize>,
    use_reference: bool,
    reference: usize,
    alpha: f64,
    lambda: f64,
    /// MEWMA limit from an in-control run length (`true`) or a given value.
    limit_from_arl: bool,
    arl: f64,
    limit: f64,
}

impl Default for MultivariateDialog {
    fn default() -> Self {
        Self {
            edit: None,
            kind: MultivariateChartKind::HotellingT2,
            columns: Vec::new(),
            subgroup_from_column: false,
            subgroup: None,
            subgroup_size: 1,
            label: None,
            use_reference: false,
            reference: 20,
            alpha: MultivariateChartRequest::DEFAULT_ALPHA,
            lambda: MultivariateChartRequest::DEFAULT_LAMBDA,
            limit_from_arl: true,
            arl: 200.0,
            limit: 10.0,
        }
    }
}

impl MultivariateDialog {
    /// The dialog filled from a saved multivariate chart.
    pub fn from_saved(saved: &SavedReport, table: &DataTable) -> Option<Self> {
        let SavedReport::MultivariateChart { request } = saved else { return None };
        let d = Self::default();
        let subgroup = request.subgroup.and_then(|c| kit::valid(table, c));
        let (limit_from_arl, arl, limit) = match request.mewma_limit {
            MewmaLimit::InControlArl(arl) => (true, arl, d.limit),
            MewmaLimit::Value(h) => (false, d.arl, h),
        };
        Some(Self {
            kind: request.kind,
            columns: kit::mask(table.n_cols(), &request.columns),
            subgroup_from_column: request.subgroup.is_some(),
            subgroup,
            subgroup_size: if request.subgroup.is_some() { d.subgroup_size } else { request.subgroup_size },
            label: request.label.and_then(|c| kit::valid(table, c)),
            use_reference: request.reference.is_some(),
            reference: request.reference.unwrap_or(d.reference),
            alpha: request.alpha,
            lambda: request.lambda,
            limit_from_arl,
            arl,
            limit,
            ..d
        })
    }

    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<MultivariateChartRequest> {
        let dialog = Dialog::new(TITLE).editing(self.edit);
        match dialog.show(ctx, |ui| self.contents(ui, table)) {
            Outcome::Reset => {
                *self = Self { edit: self.edit, ..Self::default() };
                Outcome::Open
            }
            outcome => outcome,
        }
    }

    fn contents(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<MultivariateChartRequest> {
        ui.label("Chart type:");
        for kind in MultivariateChartKind::ALL {
            ui.radio_value(&mut self.kind, kind, kind.label());
        }
        if self.kind == MultivariateChartKind::GeneralizedVariance && self.subgroup_size < 2 {
            self.subgroup_size = 5;
        }
        ui.add_space(6.0);
        ui.label("Measurements (two or more):");
        kit::column_list(ui, "mv_columns", table, &mut self.columns, |i| kit::is_numeric(table, i));
        let subgrouped = self.kind != MultivariateChartKind::Mewma;
        ui.add_enabled_ui(subgrouped, |ui| {
            ui.add_space(6.0);
            ui.label("Subgroups:");
            ui.horizontal(|ui| {
                ui.radio_value(&mut self.subgroup_from_column, true, "Rows sharing a value of");
                if kit::column_choice(ui, "mv_subgroup", table, &mut self.subgroup, |_| true, None) {
                    self.subgroup_from_column = true;
                }
            });
            ui.horizontal(|ui| {
                ui.radio_value(&mut self.subgroup_from_column, false, "Consecutive runs of");
                let min = if self.kind == MultivariateChartKind::GeneralizedVariance { 2 } else { 1 };
                ui.add(egui::DragValue::new(&mut self.subgroup_size).range(min..=1000));
                ui.label("rows (1: individual observations)");
            });
        });
        if !subgrouped {
            kit::note(ui, "A MEWMA chart smooths individual observations.");
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label("Point labels:");
            kit::column_choice(ui, "mv_label", table, &mut self.label, |_| true, Some("Sequence number"));
        });
        kit::more_options(ui, |ui| {
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.use_reference, "Estimate from the first");
                ui.add_enabled(self.use_reference, egui::DragValue::new(&mut self.reference).range(3..=100_000));
                ui.label("points; monitor the rest");
            });
            match self.kind {
                MultivariateChartKind::HotellingT2 => {
                    ui.horizontal(|ui| {
                        ui.label("False-alarm probability α per point:");
                        ui.add(egui::DragValue::new(&mut self.alpha).range(1e-6..=0.2).speed(0.0005).max_decimals(5));
                    });
                }
                MultivariateChartKind::GeneralizedVariance => {
                    kit::note(ui, "Limits sit three standard errors from E|S|.");
                }
                MultivariateChartKind::Mewma => {
                    ui.horizontal(|ui| {
                        ui.label("Smoothing weight λ:");
                        ui.add(egui::DragValue::new(&mut self.lambda).range(0.01..=1.0).speed(0.01));
                    });
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut self.limit_from_arl, true, "Limit for an in-control run length of");
                        ui.add_enabled(
                            self.limit_from_arl,
                            egui::DragValue::new(&mut self.arl)
                                .range(10.0..=stat_quality::multivariate::MEWMA_MAX_ARL)
                                .speed(1.0),
                        )
                        .on_hover_text("10 to 2000: the limit is found by simulation, which longer run lengths would make too slow");
                    });
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut self.limit_from_arl, false, "Upper limit H:");
                        ui.add_enabled(
                            !self.limit_from_arl,
                            egui::DragValue::new(&mut self.limit).range(0.01..=1e6).speed(0.1),
                        );
                    });
                }
            }
        });
        let columns: Vec<usize> = kit::chosen_columns(&self.columns)
            .into_iter()
            .filter(|&c| c < table.n_cols() && kit::is_numeric(table, c))
            .collect();
        if columns.len() < 2 {
            return Err("Choose at least two measurements".into());
        }
        let by_column = subgrouped && self.subgroup_from_column;
        if by_column && self.subgroup.is_none() {
            return Err("Choose the subgroup column".into());
        }
        let subgroup_size = if by_column || !subgrouped { 1 } else { self.subgroup_size };
        let mut summary =
            format!("{} chart of {}", self.kind.label(), kit::count(columns.len(), "measurement", "measurements"));
        match (by_column, self.subgroup) {
            (true, Some(s)) => summary.push_str(&format!(" in subgroups by {}", kit::name(table, s))),
            _ if subgroup_size > 1 => summary.push_str(&format!(" in subgroups of {subgroup_size}")),
            _ => {}
        }
        let request = MultivariateChartRequest {
            subgroup: if by_column { self.subgroup } else { None },
            subgroup_size,
            label: self.label,
            reference: self.use_reference.then_some(self.reference),
            alpha: self.alpha,
            lambda: self.lambda,
            mewma_limit: if self.limit_from_arl {
                MewmaLimit::InControlArl(self.arl)
            } else {
                MewmaLimit::Value(self.limit)
            },
            ..MultivariateChartRequest::new(columns, self.kind)
        };
        kit::ready(request, summary)
    }
}

impl Remember for MultivariateDialog {
    fn memory_key(&self) -> String {
        TITLE.into()
    }

    fn set_edit(&mut self, report: Option<u64>) {
        self.edit = report;
    }

    fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        let numeric: Vec<usize> = columns.iter().copied().filter(|&c| kit::is_numeric(table, c)).collect();
        self.columns = kit::mask(table.n_cols(), &numeric);
    }
}
