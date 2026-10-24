//! Dialog for setting up a gauge R&R study: crossed, nested (destructive) or
//! expanded (extra factors, fitted by REML).

use crate::dialog_kit::ComboExt as _;
use app_model::gauge_analysis::POOLING_LEVEL;
use app_model::{
    GaugeExtraFactor, GaugeFactorNesting, GaugeStudyDesign, GaugeStudyKind, GaugeStudyRequest, SavedReport,
};
use eframe::egui;
use stat_data::DataTable;

use crate::dialog_kit::{self as kit, Dialog, Outcome, Readiness, Remember};

const TITLE: &str = "Gauge R&R study";

/// One extra-factor row of the dialog (the column may still be unchosen).
#[derive(Clone)]
struct FactorRow {
    column: Option<usize>,
    random: bool,
    nesting: GaugeFactorNesting,
}

#[derive(Clone)]
pub struct GaugeDialog {
    edit: Option<u64>,
    part: Option<usize>,
    operator: Option<usize>,
    measurement: Option<usize>,
    use_tolerance: bool,
    tolerance: f64,
    pool_interaction: bool,
    study_multiplier: f64,
    kind: GaugeStudyKind,
    run_order: Option<usize>,
    parts_within_operators: bool,
    interactions: bool,
    factors: Vec<FactorRow>,
}

impl GaugeDialog {
    /// Pre-selects columns whose names suggest their role.
    pub fn new(table: &DataTable) -> Self {
        let guess = |words: &[&str]| {
            table.columns().iter().position(|c| {
                let name = c.name.to_lowercase();
                words.iter().any(|w| name.contains(w))
            })
        };
        let measurement = guess(&["measure", "value", "reading"]).filter(|&i| kit::is_numeric(table, i));
        Self {
            edit: None,
            part: guess(&["part", "item", "sample"]),
            operator: guess(&["operator", "appraiser", "inspector"]),
            measurement,
            use_tolerance: false,
            tolerance: 1.0,
            pool_interaction: false,
            study_multiplier: 6.0,
            kind: GaugeStudyKind::Crossed,
            run_order: None,
            parts_within_operators: false,
            interactions: true,
            factors: Vec::new(),
        }
    }

    /// The dialog filled from a saved gauge R&R study.
    pub fn from_saved(saved: &SavedReport, table: &DataTable) -> Option<Self> {
        let SavedReport::Gauge { request, .. } = saved else { return None };
        let design = &request.design;
        Some(Self {
            edit: None,
            part: kit::valid(table, request.part),
            operator: kit::valid(table, request.operator),
            measurement: kit::valid(table, request.measurement),
            use_tolerance: request.tolerance.is_some(),
            tolerance: request.tolerance.unwrap_or(1.0),
            pool_interaction: request.pool_interaction,
            study_multiplier: request.study_multiplier,
            kind: design.kind,
            run_order: design.run_order.and_then(|c| kit::valid(table, c)),
            parts_within_operators: design.parts_within_operators,
            interactions: design.interactions,
            factors: design
                .factors
                .iter()
                .map(|f| FactorRow { column: kit::valid(table, f.column), random: f.random, nesting: f.nesting })
                .collect(),
        })
    }

    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<GaugeStudyRequest> {
        let dialog = Dialog::new(TITLE).editing(self.edit);
        match dialog.show(ctx, |ui| self.contents(ui, table)) {
            Outcome::Reset => {
                *self = Self { edit: self.edit, part: None, operator: None, measurement: None, ..Self::new(table) };
                Outcome::Open
            }
            outcome => outcome,
        }
    }

    fn contents(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<GaugeStudyRequest> {
        let numeric = |i: usize| kit::is_numeric(table, i);
        egui::Grid::new("gauge_columns").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label("Study type:");
            egui::ComboBox::from_id_salt("gauge_kind")
                .selected_text(self.kind.label())
                .width(kit::combo_width(ui).max(18.0 * kit::em(ui)))
                .show_list(ui, |ui| {
                    for k in GaugeStudyKind::ALL {
                        ui.selectable_value(&mut self.kind, k, k.label());
                    }
                });
            ui.end_row();
            ui.label("Part:");
            kit::column_choice(ui, "gauge_part", table, &mut self.part, |_| true, None);
            ui.end_row();
            ui.label("Operator:");
            kit::column_choice(ui, "gauge_operator", table, &mut self.operator, |_| true, None);
            ui.end_row();
            ui.label("Measurement:");
            kit::column_choice(ui, "gauge_measurement", table, &mut self.measurement, numeric, None);
            ui.end_row();
            ui.label("Run order:");
            kit::column_choice(ui, "gauge_run_order", table, &mut self.run_order, numeric, Some("Row order"));
            ui.end_row();
        });
        let hint = match self.kind {
            GaugeStudyKind::Crossed => "One row per measurement; every part measured equally often by every operator.",
            GaugeStudyKind::Nested => {
                "One row per measurement; each operator measures their own parts, each equally often. Parts may be \
                 numbered 1, 2, 3… within each operator."
            }
            GaugeStudyKind::Expanded => {
                "One row per measurement; extra factors may be fixed or random, crossed or nested, and missing \
                 readings are allowed. Fitted by REML."
            }
        };
        kit::note(ui, hint);
        if self.kind == GaugeStudyKind::Expanded {
            self.expanded_options(ui, table);
        }
        kit::more_options(ui, |ui| {
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.use_tolerance, "Tolerance (USL − LSL):");
                ui.add_enabled(
                    self.use_tolerance,
                    egui::DragValue::new(&mut self.tolerance).speed(0.01).range(1e-12..=f64::MAX),
                );
            });
            ui.horizontal(|ui| {
                ui.label("Study variation = ");
                ui.add(egui::DragValue::new(&mut self.study_multiplier).speed(0.1).range(1.0..=10.0));
                ui.label("× σ");
            });
            if self.kind == GaugeStudyKind::Crossed {
                ui.checkbox(
                    &mut self.pool_interaction,
                    format!("Pool interaction into error when p > {POOLING_LEVEL}"),
                );
            }
        });
        let Some(part) = self.part else { return Err("Choose a part column".into()) };
        let Some(operator) = self.operator else { return Err("Choose an operator column".into()) };
        let Some(measurement) = self.measurement else { return Err("Choose a measurement column".into()) };
        if !kit::all_different(&[Some(part), Some(operator), Some(measurement)]) {
            return Err("Part, operator and measurement must be different columns".into());
        }
        let expanded = self.kind == GaugeStudyKind::Expanded;
        let mut factors = Vec::new();
        if expanded {
            let mut used = vec![part, operator, measurement];
            for f in &self.factors {
                let Some(column) = f.column else {
                    return Err("Choose a column for each extra factor, or remove it".into());
                };
                if used.contains(&column) {
                    return Err("Each extra factor needs a column of its own".into());
                }
                used.push(column);
                factors.push(GaugeExtraFactor { column, random: f.random, nesting: f.nesting });
            }
        }
        let kind = match self.kind {
            GaugeStudyKind::Crossed => "",
            GaugeStudyKind::Nested => "Nested ",
            GaugeStudyKind::Expanded => "Expanded ",
        };
        let summary = format!(
            "{kind}{} of {} by {} and {}",
            if kind.is_empty() { "Gauge R&R" } else { "gauge R&R" },
            kit::name(table, measurement),
            kit::name(table, part),
            kit::name(table, operator)
        );
        kit::ready(
            GaugeStudyRequest {
                part,
                operator,
                measurement,
                tolerance: self.use_tolerance.then_some(self.tolerance),
                pool_interaction: self.pool_interaction && self.kind == GaugeStudyKind::Crossed,
                study_multiplier: self.study_multiplier,
                design: GaugeStudyDesign {
                    kind: self.kind,
                    parts_within_operators: expanded && self.parts_within_operators,
                    factors,
                    interactions: self.interactions,
                    run_order: self.run_order,
                },
            },
            summary,
        )
    }

    /// Nesting of parts, interactions and the extra-factor rows.
    fn expanded_options(&mut self, ui: &mut egui::Ui, table: &DataTable) {
        let all: Vec<usize> = (0..table.n_cols()).collect();
        let name = |i: usize| kit::name(table, i);
        ui.checkbox(&mut self.parts_within_operators, "Parts nested within operators (destructive test)");
        ui.checkbox(&mut self.interactions, "Two-factor interactions");
        ui.label("Extra factors:");
        let mut remove = None;
        egui::Grid::new("gauge_factors").num_columns(4).spacing([8.0, 4.0]).show(ui, |ui| {
            for (k, f) in self.factors.iter_mut().enumerate() {
                let n = k + 1;
                egui::ComboBox::from_id_salt(("gauge_factor_column", k))
                    .selected_text(f.column.map_or("Choose a column…", name))
                    .width(150.0)
                    .show_list(ui, |ui| {
                        for &i in &all {
                            ui.selectable_value(&mut f.column, Some(i), name(i));
                        }
                    })
                    .response
                    .widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, format!("Factor {n} column"))
                    });
                egui::ComboBox::from_id_salt(("gauge_factor_role", k))
                    .selected_text(if f.random { "Random" } else { "Fixed" })
                    .width(80.0)
                    .show_list(ui, |ui| {
                        ui.selectable_value(&mut f.random, true, "Random");
                        ui.selectable_value(&mut f.random, false, "Fixed");
                    })
                    .response
                    .widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, format!("Factor {n} role"))
                    });
                egui::ComboBox::from_id_salt(("gauge_factor_nesting", k))
                    .selected_text(f.nesting.label())
                    .width(120.0)
                    .show_list(ui, |ui| {
                        for choice in GaugeFactorNesting::ALL {
                            ui.selectable_value(&mut f.nesting, choice, choice.label());
                        }
                    })
                    .response
                    .widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, format!("Factor {n} nesting"))
                    });
                if ui.button("Remove").on_hover_text("Remove this factor").clicked() {
                    remove = Some(k);
                }
                ui.end_row();
            }
        });
        if let Some(k) = remove {
            self.factors.remove(k);
        }
        if ui.button("Add factor").clicked() {
            self.factors.push(FactorRow { column: None, random: true, nesting: GaugeFactorNesting::Crossed });
        }
    }
}

impl Remember for GaugeDialog {
    fn memory_key(&self) -> String {
        TITLE.into()
    }

    fn set_edit(&mut self, report: Option<u64>) {
        self.edit = report;
    }
}
