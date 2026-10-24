//! Dialogs for the dose–response (probit) analysis, method comparison
//! (Deming and Passing–Bablok regression) and shelf-life estimation.

use crate::dialog_kit::ComboExt as _;
use crate::dialog_kit::{self as kit, Dialog, Outcome, Readiness, Remember};
use crate::report_view::ReportRequest;
use app_model::{
    DoseResponseRequest, DoseScale, Link, MethodComparisonRequest, SavedReport, ShelfLifeRequest, StabilityTimeUnit,
};
use eframe::egui;
use stat_data::DataTable;

#[derive(Clone)]
pub struct DoseDialog {
    dose: Option<usize>,
    response: Option<usize>,
    trials: Option<usize>,
    link: Link,
    scale: DoseScale,
    /// Response percentages, comma separated.
    percents: String,
    level: f64,
    heterogeneity_alpha: f64,
}

#[derive(Clone)]
pub struct MethodDialog {
    reference: Option<usize>,
    test: Option<usize>,
    deming: bool,
    passing_bablok: bool,
    ratio: f64,
    level: f64,
}

#[derive(Clone)]
pub struct ShelfDialog {
    time: Option<usize>,
    response: Option<usize>,
    batch: Option<usize>,
    use_lower: bool,
    lower: f64,
    use_upper: bool,
    upper: f64,
    pooling_alpha: f64,
    level: f64,
    /// Unit of the time column; `None` when not stated.
    time_unit: Option<StabilityTimeUnit>,
    /// Kept from a saved report being edited (not shown in the dialog).
    horizon: Option<f64>,
    /// Batches as a random effect instead of the fixed-batch ICH Q1E analysis.
    random_batches: bool,
    /// With random batches: bound for a future batch (else the population mean).
    future_batch: bool,
}

/// Which analysis the dialog sets up.
#[derive(Clone)]
pub enum RegressionKind {
    DoseResponse(DoseDialog),
    MethodComparison(MethodDialog),
    ShelfLife(ShelfDialog),
}

#[derive(Clone)]
pub struct RegressionDialog {
    edit: Option<u64>,
    kind: RegressionKind,
}

fn numeric(table: &DataTable) -> impl Fn(usize) -> bool + '_ {
    move |i| kit::is_numeric(table, i)
}

impl RegressionDialog {
    pub fn dose_response() -> Self {
        let d = DoseResponseRequest::probit(0, 0, None);
        Self {
            edit: None,
            kind: RegressionKind::DoseResponse(DoseDialog {
                dose: None,
                response: None,
                trials: None,
                link: d.link,
                scale: d.scale,
                percents: "10, 50, 90".to_owned(),
                level: d.confidence_level,
                heterogeneity_alpha: d.heterogeneity_alpha,
            }),
        }
    }

    pub fn method_comparison() -> Self {
        Self {
            edit: None,
            kind: RegressionKind::MethodComparison(MethodDialog {
                reference: None,
                test: None,
                deming: true,
                passing_bablok: true,
                ratio: 1.0,
                level: 0.95,
            }),
        }
    }

    pub fn shelf_life() -> Self {
        Self {
            edit: None,
            kind: RegressionKind::ShelfLife(ShelfDialog {
                time: None,
                response: None,
                batch: None,
                use_lower: true,
                lower: 95.0,
                use_upper: false,
                upper: 105.0,
                pooling_alpha: 0.25,
                level: 0.95,
                time_unit: Some(StabilityTimeUnit::Months),
                horizon: None,
                random_batches: false,
                future_batch: true,
            }),
        }
    }

    /// The dialog filled from a saved dose–response, method comparison or
    /// shelf-life report.
    pub fn from_saved(saved: &SavedReport, table: &DataTable) -> Option<Self> {
        let kind = match saved {
            SavedReport::DoseResponse { request: r } => {
                let percents: Vec<String> =
                    r.probabilities.iter().map(|p| ((p * 100.0 * 1e6).round() / 1e6).to_string()).collect();
                RegressionKind::DoseResponse(DoseDialog {
                    dose: kit::valid(table, r.dose),
                    response: kit::valid(table, r.response),
                    trials: r.trials.and_then(|t| kit::valid(table, t)),
                    link: r.link,
                    scale: r.scale,
                    percents: percents.join(", "),
                    level: r.confidence_level,
                    heterogeneity_alpha: r.heterogeneity_alpha,
                })
            }
            SavedReport::MethodComparison { request: r } => RegressionKind::MethodComparison(MethodDialog {
                reference: kit::valid(table, r.reference),
                test: kit::valid(table, r.test),
                deming: r.deming,
                passing_bablok: r.passing_bablok,
                ratio: r.variance_ratio,
                level: r.confidence_level,
            }),
            SavedReport::ShelfLife { request: r } => {
                let RegressionKind::ShelfLife(fresh) = Self::shelf_life().kind else { unreachable!() };
                RegressionKind::ShelfLife(ShelfDialog {
                    time: kit::valid(table, r.time),
                    response: kit::valid(table, r.response),
                    batch: r.batch.and_then(|b| kit::valid(table, b)),
                    use_lower: r.lower.is_some(),
                    lower: r.lower.unwrap_or(fresh.lower),
                    use_upper: r.upper.is_some(),
                    upper: r.upper.unwrap_or(fresh.upper),
                    pooling_alpha: r.pooling_alpha,
                    level: r.confidence_level,
                    time_unit: r.time_unit,
                    horizon: r.horizon,
                    random_batches: r.random_batches,
                    future_batch: r.future_batch,
                })
            }
            _ => return None,
        };
        Some(Self { edit: None, kind })
    }

    fn title(&self) -> &'static str {
        match self.kind {
            RegressionKind::DoseResponse(_) => "Dose–response (probit) analysis",
            RegressionKind::MethodComparison(_) => "Method comparison",
            RegressionKind::ShelfLife(_) => "Shelf life",
        }
    }

    /// The same kind of dialog with nothing chosen.
    fn fresh(&self) -> Self {
        match self.kind {
            RegressionKind::DoseResponse(_) => Self::dose_response(),
            RegressionKind::MethodComparison(_) => Self::method_comparison(),
            RegressionKind::ShelfLife(_) => Self::shelf_life(),
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<Box<ReportRequest>> {
        let dialog = Dialog::new(self.title()).editing(self.edit);
        let outcome = dialog.show(ctx, |ui| match &mut self.kind {
            RegressionKind::DoseResponse(d) => d.contents(ui, table),
            RegressionKind::MethodComparison(d) => d.contents(ui, table),
            RegressionKind::ShelfLife(d) => d.contents(ui, table),
        });
        match outcome {
            Outcome::Reset => {
                *self = Self { edit: self.edit, ..self.fresh() };
                Outcome::Open
            }
            outcome => outcome.map(Box::new),
        }
    }
}

impl Remember for RegressionDialog {
    fn memory_key(&self) -> String {
        self.title().into()
    }

    fn set_edit(&mut self, report: Option<u64>) {
        self.edit = report;
    }

    fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        let mut numeric = columns.iter().copied().filter(|&c| kit::is_numeric(table, c));
        let (first, second) = (numeric.next(), numeric.next());
        match &mut self.kind {
            RegressionKind::DoseResponse(d) => (d.dose, d.response) = (first, second),
            RegressionKind::MethodComparison(d) => (d.reference, d.test) = (first, second),
            RegressionKind::ShelfLife(d) => (d.time, d.response) = (first, second),
        }
    }
}

impl DoseDialog {
    fn probabilities(&self) -> Option<Vec<f64>> {
        let v: Option<Vec<f64>> = self
            .percents
            .split([',', ' ', ';'])
            .filter(|s| !s.trim().is_empty())
            .map(|s| s.trim().trim_end_matches('%').parse::<f64>().ok().filter(|p| *p > 0.0 && *p < 100.0))
            .map(|p| p.map(|p| p / 100.0))
            .collect();
        v.filter(|v| !v.is_empty())
    }

    fn contents(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<ReportRequest> {
        egui::Grid::new("dose_grid").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
            ui.label("Dose:");
            kit::column_choice(ui, "dose_dose", table, &mut self.dose, numeric(table), None);
            ui.end_row();
            ui.label("Number responding:");
            kit::column_choice(ui, "dose_response", table, &mut self.response, numeric(table), None);
            ui.end_row();
            ui.label("Number of subjects:");
            kit::column_choice(
                ui,
                "dose_trials",
                table,
                &mut self.trials,
                numeric(table),
                Some("One subject per row (0/1)"),
            );
            ui.end_row();
            ui.label("Link:");
            ui.horizontal(|ui| {
                for link in DoseResponseRequest::LINKS {
                    ui.radio_value(&mut self.link, link, link.label());
                }
            });
            ui.end_row();
            ui.label("Dose scale:");
            ui.horizontal(|ui| {
                for scale in DoseScale::ALL {
                    ui.radio_value(&mut self.scale, scale, scale.label());
                }
            });
            ui.end_row();
            ui.label("Effective doses for (%):");
            ui.text_edit_singleline(&mut self.percents);
            ui.end_row();
        });
        if self.scale != DoseScale::Linear {
            kit::note(ui, "Rows with a zero dose (controls) are left out on a log scale.");
        }
        kit::more_options(ui, |ui| {
            egui::Grid::new("dose_more").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                ui.label("Fiducial level:");
                ui.add(egui::DragValue::new(&mut self.level).range(0.5..=0.999).speed(0.005).max_decimals(3));
                ui.end_row();
                ui.label("Heterogeneity test level:");
                ui.add(
                    egui::DragValue::new(&mut self.heterogeneity_alpha).range(0.0..=0.5).speed(0.005).max_decimals(3),
                )
                .on_hover_text(
                    "When the χ² test of fit is significant at this level, variances are multiplied by the \
                     heterogeneity factor and t replaces z",
                );
                ui.end_row();
            });
        });
        let Some(dose) = self.dose else { return Err("Choose the dose column".into()) };
        let Some(response) = self.response else { return Err("Choose the number responding".into()) };
        if !kit::all_different(&[self.dose, self.response, self.trials]) {
            return Err("Dose, response and trials must be different columns".into());
        }
        let Some(probabilities) = self.probabilities() else {
            return Err("Give response percentages between 0 and 100".into());
        };
        let summary = format!(
            "Dose–response of {} on {}, {} link",
            kit::name(table, response),
            kit::name(table, dose),
            self.link.label()
        );
        kit::ready(
            ReportRequest::DoseResponse(DoseResponseRequest {
                link: self.link,
                scale: self.scale,
                probabilities,
                confidence_level: self.level,
                heterogeneity_alpha: self.heterogeneity_alpha,
                ..DoseResponseRequest::probit(dose, response, self.trials)
            }),
            summary,
        )
    }
}

impl MethodDialog {
    fn contents(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<ReportRequest> {
        egui::Grid::new("method_grid").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
            ui.label("Comparison method (X):");
            kit::column_choice(ui, "method_reference", table, &mut self.reference, numeric(table), None);
            ui.end_row();
            ui.label("Test method (Y):");
            kit::column_choice(ui, "method_test", table, &mut self.test, numeric(table), None);
            ui.end_row();
            ui.label("Fits:");
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.deming, "Deming");
                ui.checkbox(&mut self.passing_bablok, "Passing–Bablok");
            });
            ui.end_row();
        });
        kit::more_options(ui, |ui| {
            egui::Grid::new("method_more").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                ui.label("Error-variance ratio Y : X:");
                ui.add_enabled(
                    self.deming,
                    egui::DragValue::new(&mut self.ratio).range(1e-6..=1e6).speed(0.01).max_decimals(4),
                )
                .on_hover_text("Var(error of Y) / Var(error of X); 1 gives orthogonal regression");
                ui.end_row();
                ui.label("Confidence level:");
                ui.add(egui::DragValue::new(&mut self.level).range(0.5..=0.999).speed(0.005).max_decimals(3));
                ui.end_row();
            });
        });
        let Some(reference) = self.reference else { return Err("Choose the comparison method (X)".into()) };
        let Some(test) = self.test else { return Err("Choose the test method (Y)".into()) };
        if reference == test {
            return Err("The two methods must be different columns".into());
        }
        let fits = match (self.deming, self.passing_bablok) {
            (true, true) => "Deming and Passing–Bablok fits",
            (true, false) => "Deming fit",
            (false, true) => "Passing–Bablok fit",
            (false, false) => return Err("Choose at least one fit".into()),
        };
        let summary = format!("{fits} of {} on {}", kit::name(table, test), kit::name(table, reference));
        kit::ready(
            ReportRequest::MethodComparison(MethodComparisonRequest {
                deming: self.deming,
                passing_bablok: self.passing_bablok,
                variance_ratio: self.ratio,
                confidence_level: self.level,
                ..MethodComparisonRequest::new(reference, test)
            }),
            summary,
        )
    }
}

impl ShelfDialog {
    fn contents(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<ReportRequest> {
        egui::Grid::new("shelf_grid").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
            ui.label("Time:");
            kit::column_choice(ui, "shelf_time", table, &mut self.time, numeric(table), None);
            ui.end_row();
            ui.label("Response:");
            kit::column_choice(ui, "shelf_response", table, &mut self.response, numeric(table), None);
            ui.end_row();
            ui.label("Batch:");
            kit::column_choice(ui, "shelf_batch", table, &mut self.batch, |_| true, Some("None (one batch)"));
            ui.end_row();
            if self.batch.is_some() {
                ui.label("Batches are:");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.random_batches, false, "Fixed (ICH Q1E pooling)");
                    ui.radio_value(&mut self.random_batches, true, "A random sample of batches");
                });
                ui.end_row();
                if self.random_batches {
                    ui.label("Bound for:");
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut self.future_batch, true, "A future batch");
                        ui.radio_value(&mut self.future_batch, false, "The mean of all batches");
                    });
                    ui.end_row();
                }
            }
            ui.label("Time unit:");
            let unit_name = |u: Option<StabilityTimeUnit>| match u {
                Some(StabilityTimeUnit::Months) => "Months",
                Some(StabilityTimeUnit::Weeks) => "Weeks",
                Some(StabilityTimeUnit::Days) => "Days",
                Some(StabilityTimeUnit::Years) => "Years",
                None => "Not stated",
            };
            egui::ComboBox::from_id_salt("shelf_unit")
                .selected_text(unit_name(self.time_unit))
                .width(kit::combo_width(ui))
                .show_list(ui, |ui| {
                    for u in StabilityTimeUnit::ALL.into_iter().map(Some).chain([None]) {
                        ui.selectable_value(&mut self.time_unit, u, unit_name(u));
                    }
                })
                .response
                .on_hover_text("ICH Q1E allows a shelf life of at most twice the data and 12 months beyond them");
            ui.end_row();
            ui.checkbox(&mut self.use_lower, "Lower limit:");
            ui.add_enabled(self.use_lower, egui::DragValue::new(&mut self.lower).speed(0.1));
            ui.end_row();
            ui.checkbox(&mut self.use_upper, "Upper limit:");
            ui.add_enabled(self.use_upper, egui::DragValue::new(&mut self.upper).speed(0.1));
            ui.end_row();
        });
        kit::note(ui, "One limit: one-sided bound. Both limits: two-sided bounds compared with both.");
        kit::more_options(ui, |ui| {
            egui::Grid::new("shelf_more").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                ui.label("Poolability test level:");
                ui.add(egui::DragValue::new(&mut self.pooling_alpha).range(0.01..=0.5).speed(0.005).max_decimals(3))
                    .on_hover_text("ICH Q1E uses 0.25");
                ui.end_row();
                ui.label("Confidence level:");
                ui.add(egui::DragValue::new(&mut self.level).range(0.5..=0.999).speed(0.005).max_decimals(3));
                ui.end_row();
            });
        });
        let Some(time) = self.time else { return Err("Choose the time column".into()) };
        let Some(response) = self.response else { return Err("Choose the response".into()) };
        if !kit::all_different(&[self.time, self.response, self.batch]) {
            return Err("Time, response and batch must be different columns".into());
        }
        if !self.use_lower && !self.use_upper {
            return Err("Set a lower or an upper limit".into());
        }
        if self.use_lower && self.use_upper && self.lower >= self.upper {
            return Err("Set the lower limit below the upper limit".into());
        }
        let random = self.random_batches && self.batch.is_some();
        if random {
            kit::note(
                ui,
                "Batch-to-batch variation is fitted as random intercepts and slopes (needs 3 or more batches).",
            );
        }
        let batches = match self.batch {
            Some(b) if random => format!("{} as random", kit::name(table, b)),
            Some(b) => format!("by {}", kit::name(table, b)),
            None => "one batch".to_owned(),
        };
        let summary =
            format!("Shelf life of {} over {}, {batches}", kit::name(table, response), kit::name(table, time));
        kit::ready(
            ReportRequest::ShelfLife(ShelfLifeRequest {
                lower: self.use_lower.then_some(self.lower),
                upper: self.use_upper.then_some(self.upper),
                pooling_alpha: self.pooling_alpha,
                confidence_level: self.level,
                time_unit: self.time_unit,
                horizon: self.horizon,
                random_batches: random,
                future_batch: self.future_batch,
                ..ShelfLifeRequest::lower(time, response, self.batch, self.lower)
            }),
            summary,
        )
    }
}
