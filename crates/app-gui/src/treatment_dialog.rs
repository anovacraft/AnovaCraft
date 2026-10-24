//! Dialog for treatment effects from observational data: the treatment and
//! its treated value, the outcome and its kind, covariates, subgroups, and
//! the method (weighting, doubly robust or matching) with its settings.

use app_model::{
    EffectMethod, Estimand, IntervalMethod, MatchingOptions, SavedReport, TreatmentEffectsRequest, TreatmentOutcome,
};
use eframe::egui::{self, RichText};
use stat_data::{DataTable, natural_order};

use crate::dialog_kit::{self as kit, ComboExt, Dialog, Outcome, Readiness, Remember};

/// The method chosen, without its settings.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Method {
    Weighting,
    DoublyRobust,
    Matching,
}

impl Method {
    const ALL: [(Self, &'static str, &'static str); 3] = [
        (
            Self::Weighting,
            "Inverse-probability weighting",
            "Weight each row by the inverse of the probability of the treatment it received",
        ),
        (
            Self::DoublyRobust,
            "Doubly robust (weighting with outcome models)",
            "Weighting augmented by outcome regressions in each group: consistent if either model is right",
        ),
        (
            Self::Matching,
            "Nearest-neighbour matching",
            "Pair each treated row with the control rows closest in the logit of the propensity score",
        ),
    ];
}

#[derive(Clone)]
pub struct TreatmentDialog {
    edit: Option<u64>,
    treatment: Option<usize>,
    treated_level: Option<String>,
    outcome: Option<usize>,
    kind: TreatmentOutcome,
    covariates: Vec<bool>,
    subgroups: Vec<bool>,
    method: Method,
    estimand: Estimand,
    matching: MatchingOptions,
    use_caliper: bool,
    caliper: f64,
    stabilized: bool,
    trim: bool,
    trim_at: f64,
    bootstrap: bool,
    replicates: usize,
    seed: u64,
    level: f64,
}

/// The distinct values of column `i` (natural order).
fn values(table: &DataTable, i: usize) -> Vec<String> {
    let Some(c) = table.column(i) else { return Vec::new() };
    let mut v: Vec<String> =
        (0..table.n_rows()).map(|r| c.level_text(r).trim().to_owned()).filter(|s| !s.is_empty()).collect();
    v.sort_by(|a, b| natural_order(a, b));
    v.dedup();
    v
}

/// The outcome kind that fits column `i`'s values: 0/1 binary, else
/// continuous.
fn guess_kind(table: &DataTable, i: usize) -> TreatmentOutcome {
    let binary = table
        .column(i)
        .and_then(|c| c.as_numeric())
        .is_some_and(|v| v.iter().filter(|x| x.is_finite()).all(|&x| x == 0.0 || x == 1.0));
    if binary { TreatmentOutcome::Binary } else { TreatmentOutcome::Continuous }
}

impl TreatmentDialog {
    pub fn new(table: &DataTable) -> Self {
        let d = TreatmentEffectsRequest::new(0, 0, TreatmentOutcome::Continuous, Vec::new());
        let m = MatchingOptions::default();
        Self {
            edit: None,
            treatment: None,
            treated_level: None,
            outcome: None,
            kind: d.outcome_kind,
            covariates: vec![false; table.n_cols()],
            subgroups: vec![false; table.n_cols()],
            method: Method::Weighting,
            estimand: d.estimand,
            matching: m,
            use_caliper: m.caliper.is_some(),
            caliper: m.caliper.unwrap_or(0.2),
            stabilized: d.stabilized,
            trim: false,
            trim_at: 0.05,
            bootstrap: false,
            replicates: 500,
            seed: 1,
            level: d.level,
        }
    }

    /// The dialog filled from a saved treatment-effect report.
    pub fn from_saved(saved: &SavedReport, table: &DataTable) -> Option<Self> {
        let SavedReport::TreatmentEffects { request: r } = saved else { return None };
        let mut d = Self::new(table);
        d.treatment = kit::valid(table, r.treatment);
        d.treated_level = r.treated_level.clone();
        d.outcome = kit::valid(table, r.outcome);
        d.kind = r.outcome_kind;
        d.covariates = kit::mask(table.n_cols(), &r.covariates);
        d.subgroups = kit::mask(table.n_cols(), &r.subgroups);
        d.method = match r.method {
            EffectMethod::Weighting => Method::Weighting,
            EffectMethod::DoublyRobust => Method::DoublyRobust,
            EffectMethod::Matching(m) => {
                d.matching = m;
                d.use_caliper = m.caliper.is_some();
                d.caliper = m.caliper.unwrap_or(0.2);
                Method::Matching
            }
        };
        d.estimand = r.estimand;
        d.stabilized = r.stabilized;
        d.trim = r.trim > 0.0;
        if r.trim > 0.0 {
            d.trim_at = r.trim;
        }
        if let IntervalMethod::Bootstrap { replicates, seed } = r.intervals {
            d.bootstrap = true;
            d.replicates = replicates;
            d.seed = seed;
        }
        d.level = r.level;
        Some(d)
    }

    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<TreatmentEffectsRequest> {
        let dialog = Dialog::new("Treatment effects").editing(self.edit);
        match dialog.show(ctx, |ui| self.contents(ui, table)) {
            Outcome::Reset => {
                *self = Self { edit: self.edit, ..Self::new(table) };
                Outcome::Open
            }
            outcome => outcome,
        }
    }

    fn contents(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<TreatmentEffectsRequest> {
        kit::note(
            ui,
            "Estimates the effect of a two-level treatment on an outcome from observational data, adjusting for \
             covariates measured before treatment through the propensity score (the probability of treatment \
             given the covariates). Check the balance and overlap sections: the estimate is only as good as the \
             covariates chosen.",
        );
        egui::Grid::new("treatment_roles").num_columns(2).spacing([kit::em(ui), kit::em(ui) * 0.4]).show(ui, |ui| {
            ui.label("Treatment:");
            if kit::column_choice(ui, "treatment", table, &mut self.treatment, |i| values(table, i).len() == 2, None) {
                self.treated_level = None;
            }
            ui.end_row();
            ui.label("Treated value:");
            match self.treatment {
                Some(t) => {
                    let levels = values(table, t);
                    let default = levels.last().cloned().unwrap_or_default();
                    let shown = self.treated_level.clone().unwrap_or_else(|| default.clone());
                    egui::ComboBox::from_id_salt("treated_level").selected_text(shown.clone()).show_list(ui, |ui| {
                        for l in &levels {
                            if ui.selectable_label(*l == shown, l).clicked() {
                                self.treated_level = Some(l.clone());
                            }
                        }
                    });
                }
                None => {
                    ui.label(RichText::new("choose a treatment with two values").weak());
                }
            }
            ui.end_row();
            ui.label("Outcome:");
            if kit::column_choice(ui, "outcome", table, &mut self.outcome, |i| kit::is_numeric(table, i), None) {
                if let Some(o) = self.outcome {
                    self.kind = guess_kind(table, o);
                }
            }
            ui.end_row();
            ui.label("Outcome kind:");
            ui.horizontal(|ui| {
                for k in TreatmentOutcome::ALL {
                    ui.radio_value(&mut self.kind, k, k.label());
                }
            });
            ui.end_row();
        });
        let roles: Vec<usize> = self.treatment.into_iter().chain(self.outcome).collect();
        ui.columns(2, |cols| {
            cols[0].strong("Covariates (measured before treatment):");
            kit::column_list(&mut cols[0], "covariates", table, &mut self.covariates, |i| !roles.contains(&i));
            cols[1].strong("Subgroups (optional):");
            kit::column_list(&mut cols[1], "subgroups", table, &mut self.subgroups, |i| !roles.contains(&i));
        });
        ui.horizontal(|ui| {
            ui.label("Method:");
            egui::ComboBox::from_id_salt("treatment_method")
                .selected_text(Method::ALL.iter().find(|m| m.0 == self.method).map_or("", |m| m.1))
                .show_list(ui, |ui| {
                    for (m, label, hover) in Method::ALL {
                        ui.selectable_value(&mut self.method, m, label).on_hover_text(hover);
                    }
                });
        });
        ui.horizontal(|ui| {
            ui.label("Effect:");
            for e in Estimand::ALL {
                ui.radio_value(&mut self.estimand, e, e.label());
            }
        });
        if self.method == Method::Matching {
            ui.horizontal(|ui| {
                ui.label("Matches per treated row:");
                ui.add(egui::DragValue::new(&mut self.matching.ratio).range(1..=10));
                ui.checkbox(&mut self.matching.replace, "With replacement")
                    .on_hover_text("A control row may be matched to several treated rows");
            });
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.use_caliper, "Caliper:")
                    .on_hover_text("Leave treated rows unmatched when no control is this close");
                ui.add_enabled(self.use_caliper, egui::DragValue::new(&mut self.caliper).speed(0.01).range(0.01..=5.0));
                ui.label("SD of the logit of the score");
            });
        }
        kit::more_options(ui, |ui| {
            ui.add_enabled_ui(self.method != Method::Matching, |ui| {
                ui.checkbox(&mut self.stabilized, "Stabilized weights").on_hover_text(
                    "Scale the saved weights by the share of rows in each group; the estimate is unchanged",
                );
            });
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.trim, "Trim scores outside")
                    .on_hover_text("Leave out rows whose propensity score is near 0 or 1 (poor overlap)");
                ui.add_enabled(self.trim, egui::DragValue::new(&mut self.trim_at).speed(0.005).range(0.005..=0.25));
                ui.label(format!("to {}", app_model::format::format_number(1.0 - self.trim_at, 3)));
            });
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.bootstrap, "Bootstrap intervals").on_hover_text(
                    "Repeat the whole analysis on resampled rows; otherwise sandwich (robust) intervals",
                );
                ui.add_enabled(self.bootstrap, egui::DragValue::new(&mut self.replicates).range(20..=10_000));
                ui.label("resamples, seed");
                ui.add_enabled(self.bootstrap, kit::seed_value(&mut self.seed));
            });
            ui.horizontal(|ui| {
                ui.label("Confidence level:");
                ui.add(egui::DragValue::new(&mut self.level).speed(0.005).range(0.5..=0.999).fixed_decimals(3));
            });
        });
        let Some(treatment) = self.treatment else { return Err("Choose a treatment".into()) };
        let Some(outcome) = self.outcome else { return Err("Choose an outcome".into()) };
        if treatment == outcome {
            return Err("The treatment and the outcome must be different columns".into());
        }
        let covariates: Vec<usize> =
            kit::chosen_columns(&self.covariates).into_iter().filter(|c| !roles.contains(c)).collect();
        if covariates.is_empty() {
            return Err("Choose at least one covariate".into());
        }
        let subgroups: Vec<usize> =
            kit::chosen_columns(&self.subgroups).into_iter().filter(|c| !roles.contains(c)).collect();
        if self.method == Method::Matching && self.estimand == Estimand::Ate && !self.matching.replace {
            return Err("Matching for the average effect over everyone (ATE) needs matching with replacement".into());
        }
        let method = match self.method {
            Method::Weighting => EffectMethod::Weighting,
            Method::DoublyRobust => EffectMethod::DoublyRobust,
            Method::Matching => EffectMethod::Matching(MatchingOptions {
                caliper: self.use_caliper.then_some(self.caliper),
                ..self.matching
            }),
        };
        let summary = format!(
            "{} of {} on {} adjusting for {}{}",
            self.estimand.short(),
            kit::name(table, treatment),
            kit::name(table, outcome),
            kit::count(covariates.len(), "covariate", "covariates"),
            if subgroups.is_empty() {
                String::new()
            } else {
                format!(", by {}", kit::count(subgroups.len(), "subgroup column", "subgroup columns"))
            }
        );
        kit::ready(
            TreatmentEffectsRequest {
                treatment,
                treated_level: self.treated_level.clone(),
                outcome,
                outcome_kind: self.kind,
                covariates,
                method,
                estimand: self.estimand,
                stabilized: self.stabilized,
                trim: if self.trim { self.trim_at } else { 0.0 },
                intervals: if self.bootstrap {
                    IntervalMethod::Bootstrap { replicates: self.replicates, seed: self.seed }
                } else {
                    IntervalMethod::Sandwich
                },
                level: self.level,
                subgroups,
            },
            summary,
        )
    }
}

impl Remember for TreatmentDialog {
    fn memory_key(&self) -> String {
        "Treatment effects".into()
    }

    fn set_edit(&mut self, report: Option<u64>) {
        self.edit = report;
    }

    fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        self.covariates = kit::mask(table.n_cols(), columns);
    }
}

/// Settings of a treatment-effect report edited in place from its options
/// menu: the effect, the interval method and the confidence level.
pub fn report_settings(ui: &mut egui::Ui, r: &mut TreatmentEffectsRequest) {
    ui.horizontal(|ui| {
        ui.label("Effect:");
        for e in Estimand::ALL {
            let allowed = !(e == Estimand::Ate && matches!(r.method, EffectMethod::Matching(m) if !m.replace));
            ui.add_enabled_ui(allowed, |ui| ui.radio_value(&mut r.estimand, e, e.short()))
                .response
                .on_disabled_hover_text("The ATE needs matching with replacement");
        }
    });
    ui.horizontal(|ui| {
        ui.label("Confidence level:");
        ui.add(egui::DragValue::new(&mut r.level).speed(0.005).range(0.5..=0.999).fixed_decimals(3));
    });
}
