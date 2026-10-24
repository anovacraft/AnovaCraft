//! Dialogs for Reliability ▸ Proportional hazards… and Reliability ▸
//! Parametric survival regression…: failure times with censoring,
//! predictors and their effects (interactions and powers, kept
//! hierarchical), and the model's options.

use crate::dialog_kit::ComboExt as _;
use app_model::hierarchy::{hierarchical_closure, set_effect};
use app_model::{
    AftDistribution, HazardProfile, LifeDataLayout, ProportionalHazardsRequest, SavedReport, StatusCoding,
    SurvivalRegressionRequest, TieMethod, TimeTransform,
};
use eframe::egui::{self, RichText};
use stat_data::DataTable;

use crate::dialog_kit::{self as kit, Readiness};

/// Numbers separated by commas, semicolons or spaces; `None` if any is not a number.
fn parse_numbers(text: &str) -> Option<Vec<f64>> {
    text.split([',', ';', ' '])
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.trim().parse::<f64>().ok().filter(|v| v.is_finite()))
        .collect()
}

/// Numbers as text, separated by commas ("1, 10, 50").
fn numbers_text(values: impl IntoIterator<Item = f64>) -> String {
    values.into_iter().map(|v| app_model::format::format_number(v, 10)).collect::<Vec<_>>().join(", ")
}

/// Predictor columns and the model's effects, with the hierarchy kept.
#[derive(Clone, Default)]
struct EffectsDraft {
    /// Indexed by column.
    predictors: Vec<bool>,
    /// Effects as `(column, power)` products.
    effects: Vec<Vec<(usize, u8)>>,
    hierarchical: bool,
}

impl EffectsDraft {
    fn new() -> Self {
        Self { hierarchical: true, ..Self::default() }
    }

    /// The draft of a saved request's `predictors` and `effects` (positions).
    fn from_request(predictors: &[usize], effects: &[Vec<(usize, u8)>], table: &DataTable) -> Self {
        let valid = |c: usize| kit::valid(table, c);
        let kept: Vec<usize> = predictors.iter().copied().filter_map(valid).collect();
        let effects: Vec<Vec<(usize, u8)>> = if effects.is_empty() {
            kept.iter().map(|&c| vec![(c, 1)]).collect()
        } else {
            effects
                .iter()
                .filter_map(|e| {
                    e.iter().map(|&(k, p)| predictors.get(k).copied().and_then(valid).map(|c| (c, p))).collect()
                })
                .collect()
        };
        let mut closed = hierarchical_closure(&effects);
        let mut mine = effects.clone();
        closed.sort();
        mine.sort();
        Self { predictors: kit::mask(table.n_cols(), &kept), effects, hierarchical: closed == mine }
    }

    /// The predictor list and effect picker; returns the chosen columns.
    fn ui(&mut self, ui: &mut egui::Ui, salt: &str, table: &DataTable, allow: impl Fn(usize) -> bool) -> Vec<usize> {
        let n = table.n_cols();
        let name = |i: usize| kit::name(table, i).to_owned();
        let continuous = |i: usize| kit::is_continuous(table, i);
        ui.label(RichText::new("Predictors").strong());
        kit::column_list(ui, (salt, "predictors"), table, &mut self.predictors, &allow);
        let chosen: Vec<usize> =
            kit::chosen_columns(&self.predictors).into_iter().filter(|&i| i < n && allow(i)).collect();
        self.effects.retain(|e| e.iter().all(|(c, _)| chosen.contains(c)));
        for &c in &chosen {
            if !self.effects.iter().any(|e| e.iter().any(|(x, _)| *x == c)) {
                self.effects.push(vec![(c, 1)]);
            }
        }
        if chosen.is_empty() {
            return chosen;
        }
        let candidates = crate::model_dialog::candidate_effects(&chosen, &continuous);
        ui.horizontal(|ui| {
            ui.label("Model effects:");
            if ui.small_button("Main effects").clicked() {
                self.effects = chosen.iter().map(|&c| vec![(c, 1)]).collect();
            }
            if ui.small_button("+ Two-factor interactions").clicked() {
                for e in candidates.iter().filter(|e| e.len() == 2) {
                    set_effect(&mut self.effects, e, true, self.hierarchical);
                }
            }
        });
        let label = |e: &[(usize, u8)]| {
            e.iter()
                .map(|&(c, p)| {
                    if p == 1 {
                        name(c)
                    } else {
                        format!("{}{}", name(c), stat_core::text::superscript(&p.to_string()))
                    }
                })
                .collect::<Vec<_>>()
                .join(" × ")
        };
        egui::ScrollArea::vertical().id_salt((salt, "effects")).max_height(kit::em(ui) * 6.0).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                for e in &candidates {
                    let mut on = self.effects.contains(e);
                    if ui.checkbox(&mut on, label(e)).changed() {
                        set_effect(&mut self.effects, e, on, self.hierarchical);
                    }
                }
            });
        });
        if ui
            .checkbox(&mut self.hierarchical, "Keep the model hierarchical")
            .on_hover_text(
                "Adding an interaction or power adds the effects it is built from; removing an effect removes the \
                 interactions and powers built on it",
            )
            .changed()
            && self.hierarchical
        {
            self.effects = hierarchical_closure(&self.effects);
        }
        chosen
    }

    /// Effects as positions in `chosen`, sorted by order; empty when they
    /// are exactly the main effects.
    fn request_effects(&self, chosen: &[usize]) -> Vec<Vec<(usize, u8)>> {
        let position = |c: usize| chosen.iter().position(|&x| x == c).unwrap_or(0);
        let mut effects: Vec<Vec<(usize, u8)>> =
            self.effects.iter().map(|e| e.iter().map(|&(c, p)| (position(c), p)).collect()).collect();
        effects.sort_by_key(|e| (e.iter().map(|f| u32::from(f.1)).sum::<u32>(), e.clone()));
        let mains = effects.len() == chosen.len() && effects.iter().all(|e| matches!(e.as_slice(), [(_, 1)]));
        if mains { Vec::new() } else { effects }
    }
}

/// The "curves for" drop-down: one of the chosen predictors, or none.
fn curve_choice(ui: &mut egui::Ui, salt: &str, table: &DataTable, slot: &mut Option<usize>, chosen: &[usize]) {
    if slot.is_some_and(|c| !chosen.contains(&c)) {
        *slot = None;
    }
    kit::column_choice(ui, (salt, "curve_by"), table, slot, |i| chosen.contains(&i), Some("None: the average unit"));
}

fn confidence(ui: &mut egui::Ui, value: &mut f64) {
    ui.label("Confidence level:");
    ui.add(egui::DragValue::new(value).range(0.5..=0.999).speed(0.005).max_decimals(3));
    ui.end_row();
}

/// Proportional hazards (Cox) regression.
#[derive(Clone)]
pub struct ProportionalHazardsDialog {
    time: Option<usize>,
    censor: Option<usize>,
    coding: StatusCoding,
    strata: Option<usize>,
    model: EffectsDraft,
    ties: TieMethod,
    transform: TimeTransform,
    curve_by: Option<usize>,
    times: String,
    profile: HazardProfile,
    profile_at_median: bool,
    profile_time: f64,
    confidence: f64,
}

impl Default for ProportionalHazardsDialog {
    fn default() -> Self {
        Self {
            time: None,
            censor: None,
            coding: StatusCoding::NonzeroIsCensored,
            strata: None,
            model: EffectsDraft::new(),
            ties: TieMethod::Efron,
            transform: TimeTransform::KaplanMeier,
            curve_by: None,
            times: String::new(),
            profile: HazardProfile::HazardRatio,
            profile_at_median: true,
            profile_time: 1.0,
            confidence: ProportionalHazardsRequest::DEFAULT_CONFIDENCE,
        }
    }
}

impl ProportionalHazardsDialog {
    /// The dialog filled from a saved request.
    pub fn from_saved(r: &ProportionalHazardsRequest, table: &DataTable) -> Self {
        let valid = |c: usize| kit::valid(table, c);
        Self {
            time: valid(r.time),
            censor: r.censor.and_then(valid),
            coding: r.coding,
            strata: r.strata.and_then(valid),
            model: EffectsDraft::from_request(&r.predictors, &r.effects, table),
            ties: r.ties,
            transform: r.transform,
            curve_by: r.curve_by.and_then(valid),
            times: numbers_text(r.times.iter().copied()),
            profile: r.profile,
            profile_at_median: r.profile_time.is_none(),
            profile_time: r.profile_time.unwrap_or(1.0),
            confidence: r.confidence,
        }
    }

    /// The first numeric selected column as time, the rest as predictors.
    pub fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        self.time = columns.iter().copied().find(|&c| kit::is_numeric(table, c));
        let rest: Vec<usize> = columns.iter().copied().filter(|&c| Some(c) != self.time).collect();
        self.model.predictors = kit::mask(table.n_cols(), &rest);
    }

    /// Draw the settings; ready when they are complete and valid.
    pub fn ui(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<SavedReport> {
        ui.set_min_width(kit::em(ui) * 36.0);
        kit::note(
            ui,
            "Cox regression: how predictors multiply the hazard of failure, with no assumption about its shape \
             over time. One row per unit, with its failure or censoring time.",
        );
        let numeric = |i: usize| kit::is_numeric(table, i);
        egui::Grid::new("ph_columns").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label("Time:");
            kit::column_choice(ui, "ph_time", table, &mut self.time, numeric, None);
            ui.end_row();
            ui.label("Censor (optional):");
            kit::column_choice(
                ui,
                "ph_censor",
                table,
                &mut self.censor,
                numeric,
                Some("None: every time is a failure"),
            );
            ui.end_row();
            ui.label("");
            ui.vertical(|ui| {
                ui.radio_value(&mut self.coding, StatusCoding::NonzeroIsCensored, "Non-zero = censored");
                ui.radio_value(&mut self.coding, StatusCoding::NonzeroIsEvent, "Non-zero = failure (event)");
            });
            ui.end_row();
            ui.label("Strata (optional):").on_hover_text(
                "A separate baseline hazard for each level; the predictors' effects are shared across strata",
            );
            kit::column_choice(ui, "ph_strata", table, &mut self.strata, |_| true, Some("None"));
            ui.end_row();
        });
        ui.separator();
        let (time, censor, strata) = (self.time, self.censor, self.strata);
        let chosen = self.model.ui(ui, "ph", table, |i| Some(i) != time && Some(i) != censor && Some(i) != strata);
        ui.horizontal(|ui| {
            ui.label("Survival curves for:");
            curve_choice(ui, "ph", table, &mut self.curve_by, &chosen);
        });
        kit::more_options(ui, |ui| {
            egui::Grid::new("ph_settings").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                ui.label("Tied failure times:");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.ties, TieMethod::Efron, TieMethod::Efron.label());
                    ui.radio_value(&mut self.ties, TieMethod::Breslow, TieMethod::Breslow.label());
                });
                ui.end_row();
                ui.label("Proportional-hazards test time scale:");
                egui::ComboBox::from_id_salt("ph_transform").selected_text(self.transform.label()).show_list(
                    ui,
                    |ui| {
                        for t in TimeTransform::ALL {
                            ui.selectable_value(&mut self.transform, t, t.label());
                        }
                    },
                );
                ui.end_row();
                ui.label("Survival at times:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.times)
                        .desired_width(kit::combo_width(ui))
                        .hint_text("e.g. 12, 24"),
                );
                ui.end_row();
                ui.label("Response Explorer shows:");
                ui.vertical(|ui| {
                    ui.radio_value(&mut self.profile, HazardProfile::HazardRatio, HazardProfile::HazardRatio.label());
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut self.profile, HazardProfile::Survival, "Survival probability at");
                        let on = self.profile == HazardProfile::Survival;
                        ui.add_enabled_ui(on, |ui| {
                            ui.radio_value(&mut self.profile_at_median, true, "the median time");
                            ui.radio_value(&mut self.profile_at_median, false, "time");
                            ui.add_enabled(
                                !self.profile_at_median,
                                egui::DragValue::new(&mut self.profile_time).range(1e-9..=1e12).speed(0.5),
                            );
                        });
                    });
                });
                ui.end_row();
                confidence(ui, &mut self.confidence);
            });
        });
        let time = self.time.ok_or("Choose a time column")?;
        if chosen.is_empty() {
            return Err("Choose at least one predictor".into());
        }
        let Some(times) = parse_numbers(&self.times).filter(|v| v.iter().all(|t| *t > 0.0)) else {
            return Err("Survival times must be positive numbers".into());
        };
        let request = ProportionalHazardsRequest {
            coding: self.coding,
            effects: self.model.request_effects(&chosen),
            strata: self.strata,
            ties: self.ties,
            confidence: self.confidence,
            transform: self.transform,
            curve_by: self.curve_by,
            times,
            profile: self.profile,
            profile_time: (self.profile == HazardProfile::Survival && !self.profile_at_median)
                .then_some(self.profile_time),
            ..ProportionalHazardsRequest::new(time, self.censor, chosen.clone())
        };
        let cols = request.columns();
        if (1..cols.len()).any(|i| cols[..i].contains(&cols[i])) {
            return Err("A column is chosen twice".into());
        }
        let mut summary = format!(
            "Proportional hazards of {} on {}",
            kit::name(table, time),
            kit::count(self.model.effects.len(), "effect", "effects")
        );
        if let Some(s) = self.strata {
            summary.push_str(&format!(", stratified by {}", kit::name(table, s)));
        }
        kit::ready(SavedReport::ProportionalHazards { request, profilers: Default::default() }, summary)
    }
}

/// Which data layout the parametric dialog collects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Layout {
    Times,
    Intervals,
}

/// Parametric (accelerated failure time) survival regression.
#[derive(Clone)]
pub struct SurvivalRegressionDialog {
    layout: Layout,
    time: Option<usize>,
    censor: Option<usize>,
    coding: StatusCoding,
    start: Option<usize>,
    end: Option<usize>,
    frequency: Option<usize>,
    model: EffectsDraft,
    distribution: AftDistribution,
    curve_by: Option<usize>,
    percentiles: String,
    times: String,
    profile_percent: f64,
    confidence: f64,
}

impl Default for SurvivalRegressionDialog {
    fn default() -> Self {
        Self {
            layout: Layout::Times,
            time: None,
            censor: None,
            coding: StatusCoding::NonzeroIsCensored,
            start: None,
            end: None,
            frequency: None,
            model: EffectsDraft::new(),
            distribution: AftDistribution::Weibull,
            curve_by: None,
            percentiles: numbers_text(SurvivalRegressionRequest::DEFAULT_PROBABILITIES.iter().map(|p| p * 100.0)),
            times: String::new(),
            profile_percent: 50.0,
            confidence: SurvivalRegressionRequest::DEFAULT_CONFIDENCE,
        }
    }
}

impl SurvivalRegressionDialog {
    /// The dialog filled from a saved request.
    pub fn from_saved(r: &SurvivalRegressionRequest, table: &DataTable) -> Self {
        let valid = |c: usize| kit::valid(table, c);
        let mut d = Self {
            frequency: r.frequency.and_then(valid),
            model: EffectsDraft::from_request(&r.predictors, &r.effects, table),
            distribution: r.distribution,
            curve_by: r.curve_by.and_then(valid),
            percentiles: numbers_text(r.probabilities.iter().map(|p| p * 100.0)),
            times: numbers_text(r.times.iter().copied()),
            profile_percent: r.profile_probability * 100.0,
            confidence: r.confidence,
            ..Self::default()
        };
        match r.layout {
            LifeDataLayout::Times { time, censor, coding } => {
                d.layout = Layout::Times;
                d.time = valid(time);
                d.censor = censor.and_then(valid);
                d.coding = coding;
            }
            LifeDataLayout::Intervals { start, end } => {
                d.layout = Layout::Intervals;
                d.start = valid(start);
                d.end = valid(end);
            }
        }
        d
    }

    /// The first numeric selected column as time, the rest as predictors.
    pub fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        self.time = columns.iter().copied().find(|&c| kit::is_numeric(table, c));
        let rest: Vec<usize> = columns.iter().copied().filter(|&c| Some(c) != self.time).collect();
        self.model.predictors = kit::mask(table.n_cols(), &rest);
    }

    /// Draw the settings; ready when they are complete and valid.
    pub fn ui(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<SavedReport> {
        ui.set_min_width(kit::em(ui) * 36.0);
        kit::note(
            ui,
            "Accelerated failure time model: the log of the failure time is linear in the predictors, with a \
             Weibull, lognormal, log-logistic or exponential distribution. Right, left and interval censoring.",
        );
        let numeric = |i: usize| kit::is_numeric(table, i);
        ui.horizontal(|ui| {
            ui.label("Data:");
            ui.radio_value(&mut self.layout, Layout::Times, "Time and censor columns");
            ui.radio_value(&mut self.layout, Layout::Intervals, "Start and end of the failure interval");
        });
        egui::Grid::new("aft_columns").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            match self.layout {
                Layout::Times => {
                    ui.label("Time:");
                    kit::column_choice(ui, "aft_time", table, &mut self.time, numeric, None);
                    ui.end_row();
                    ui.label("Censor (optional):");
                    kit::column_choice(
                        ui,
                        "aft_censor",
                        table,
                        &mut self.censor,
                        numeric,
                        Some("None: every time is a failure"),
                    );
                    ui.end_row();
                    ui.label("");
                    ui.vertical(|ui| {
                        ui.radio_value(&mut self.coding, StatusCoding::NonzeroIsCensored, "Non-zero = still running");
                        ui.radio_value(&mut self.coding, StatusCoding::NonzeroIsEvent, "Non-zero = failed");
                    });
                    ui.end_row();
                }
                Layout::Intervals => {
                    ui.label("Start:");
                    kit::column_choice(ui, "aft_start", table, &mut self.start, numeric, None);
                    ui.end_row();
                    ui.label("");
                    kit::note(ui, "Last time seen working; empty = failed before the first look.");
                    ui.end_row();
                    ui.label("End:");
                    kit::column_choice(ui, "aft_end", table, &mut self.end, numeric, None);
                    ui.end_row();
                    ui.label("");
                    kit::note(ui, "Time found failed; empty = still running at the start time; equal = exact failure.");
                    ui.end_row();
                }
            }
            ui.label("Frequency (optional):");
            kit::column_choice(
                ui,
                "aft_frequency",
                table,
                &mut self.frequency,
                numeric,
                Some("None: one unit per row"),
            );
            ui.end_row();
            ui.label("Distribution:");
            ui.horizontal_wrapped(|ui| {
                for d in AftDistribution::ALL {
                    ui.radio_value(&mut self.distribution, d, d.label());
                }
            });
            ui.end_row();
        });
        ui.separator();
        let used: Vec<usize> = match self.layout {
            Layout::Times => [self.time, self.censor, self.frequency].into_iter().flatten().collect(),
            Layout::Intervals => [self.start, self.end, self.frequency].into_iter().flatten().collect(),
        };
        let chosen = self.model.ui(ui, "aft", table, |i| !used.contains(&i));
        ui.horizontal(|ui| {
            ui.label("Curves and predictions for:");
            curve_choice(ui, "aft", table, &mut self.curve_by, &chosen);
        });
        kit::more_options(ui, |ui| {
            egui::Grid::new("aft_settings").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                ui.label("Percentiles (% failing):");
                ui.add(egui::TextEdit::singleline(&mut self.percentiles).desired_width(kit::combo_width(ui)));
                ui.end_row();
                ui.label("Failure probability at times:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.times)
                        .desired_width(kit::combo_width(ui))
                        .hint_text("e.g. 500, 1000"),
                );
                ui.end_row();
                ui.label("Response Explorer: time to % failing:");
                ui.add(egui::DragValue::new(&mut self.profile_percent).range(0.01..=99.99).speed(0.5).suffix("%"));
                ui.end_row();
                confidence(ui, &mut self.confidence);
            });
        });
        let layout = match self.layout {
            Layout::Times => {
                let time = self.time.ok_or("Choose a time column")?;
                LifeDataLayout::Times { time, censor: self.censor, coding: self.coding }
            }
            Layout::Intervals => {
                let start = self.start.ok_or("Choose a start column")?;
                let end = self.end.ok_or("Choose an end column")?;
                LifeDataLayout::Intervals { start, end }
            }
        };
        if chosen.is_empty() {
            return Err("Choose at least one predictor".into());
        }
        let Some(percentiles) = parse_numbers(&self.percentiles).filter(|v| v.iter().all(|p| *p > 0.0 && *p < 100.0))
        else {
            return Err("Percentiles must be numbers between 0 and 100".into());
        };
        let Some(times) = parse_numbers(&self.times).filter(|v| v.iter().all(|t| *t > 0.0)) else {
            return Err("Times must be positive numbers".into());
        };
        let request = SurvivalRegressionRequest {
            frequency: self.frequency,
            effects: self.model.request_effects(&chosen),
            distribution: self.distribution,
            confidence: self.confidence,
            curve_by: self.curve_by,
            probabilities: percentiles.iter().map(|p| p / 100.0).collect(),
            times,
            profile_probability: self.profile_percent / 100.0,
            ..SurvivalRegressionRequest::new(layout, chosen.clone())
        };
        let cols = request.columns();
        if (1..cols.len()).any(|i| cols[..i].contains(&cols[i])) {
            return Err("A column is chosen twice".into());
        }
        let data = match layout {
            LifeDataLayout::Times { time, .. } => kit::name(table, time).to_owned(),
            LifeDataLayout::Intervals { start, end } => {
                format!("{}–{} intervals", kit::name(table, start), kit::name(table, end))
            }
        };
        let summary = format!(
            "{} regression of {data} on {}",
            self.distribution.label(),
            kit::count(self.model.effects.len(), "effect", "effects")
        );
        kit::ready(SavedReport::SurvivalRegression { request, profilers: Default::default() }, summary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effects_map_to_positions_and_back() {
        let t = app_model::sample::leukemia_covariates();
        let mut draft = EffectsDraft::new();
        draft.effects = vec![vec![(3, 1)], vec![(0, 1)], vec![(0, 1), (3, 1)]];
        let effects = draft.request_effects(&[0, 3]);
        assert_eq!(effects, vec![vec![(0, 1)], vec![(1, 1)], vec![(0, 1), (1, 1)]]);
        let back = EffectsDraft::from_request(&[0, 3], &effects, &t);
        assert!(back.hierarchical);
        assert_eq!(kit::chosen_columns(&back.predictors), vec![0, 3]);
        // Main effects alone are stored as none.
        draft.effects = vec![vec![(0, 1)], vec![(3, 1)]];
        assert!(draft.request_effects(&[0, 3]).is_empty());
    }

    #[test]
    fn saved_requests_fill_the_dialogs() {
        let t = app_model::sample::leukemia_covariates();
        let r = ProportionalHazardsRequest {
            strata: Some(4),
            times: vec![10.0],
            profile: HazardProfile::Survival,
            profile_time: Some(12.0),
            ..ProportionalHazardsRequest::new(1, Some(2), vec![0, 3])
        };
        let d = ProportionalHazardsDialog::from_saved(&r, &t);
        assert_eq!((d.time, d.censor, d.strata), (Some(1), Some(2), Some(4)));
        assert!(!d.profile_at_median);
        assert_eq!(d.times, "10");
        let c = app_model::sample::capacitor_life_test();
        let r = SurvivalRegressionRequest {
            frequency: Some(4),
            distribution: AftDistribution::Lognormal,
            ..SurvivalRegressionRequest::new(LifeDataLayout::Intervals { start: 2, end: 3 }, vec![0, 1])
        };
        let d = SurvivalRegressionDialog::from_saved(&r, &c);
        assert_eq!(d.layout, Layout::Intervals);
        assert_eq!((d.start, d.end, d.frequency), (Some(2), Some(3), Some(4)));
        assert_eq!(d.percentiles, "10, 50");
    }
}
