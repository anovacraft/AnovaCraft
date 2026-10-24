//! Dialog for a linear or generalized linear mixed model: response and its
//! distribution, fixed effects, random-effects terms (grouping column,
//! nesting, intercept, slopes) and the estimation method.

use app_model::hierarchy::{hierarchical_closure, set_effect};
use app_model::{MixedFamily, MixedRequest, RandomEffectSpec, SavedReport};
use eframe::egui::{self, RichText};
use stat_data::DataTable;

use crate::dialog_kit::{self as kit, Dialog, Outcome, Readiness, Remember};

/// One random-effects term being edited.
#[derive(Debug, Clone, PartialEq)]
struct TermDraft {
    group: Option<usize>,
    within: Option<usize>,
    /// Further nesting columns of a saved term (the dialog shows the first).
    deeper: Vec<usize>,
    intercept: bool,
    slopes: Vec<usize>,
    correlated: bool,
}

impl Default for TermDraft {
    fn default() -> Self {
        Self { group: None, within: None, deeper: Vec::new(), intercept: true, slopes: Vec::new(), correlated: true }
    }
}

#[derive(Clone)]
pub struct MixedDialog {
    /// The report being edited (`None`: a new report).
    edit: Option<u64>,
    family: MixedFamily,
    response: Option<usize>,
    /// Binomial: the response counts successes out of this column's trials.
    trials: Option<usize>,
    use_trials: bool,
    offset: Option<usize>,
    /// Indexed by column.
    predictors: Vec<bool>,
    /// Fixed effects as `(column, power)` products.
    effects: Vec<Vec<(usize, u8)>>,
    terms: Vec<TermDraft>,
    reml: bool,
    quadrature: bool,
    points: usize,
    /// Keep the fixed effects hierarchical.
    hierarchical: bool,
    /// Comparisons within slices and slopes by group.
    slices: Option<app_model::PairAdjust>,
    slopes: Option<app_model::PairAdjust>,
    frequency: Option<usize>,
    weight: Option<usize>,
    /// Normal family: a residual variance per level of this column.
    residual_groups: Option<usize>,
    /// Normal family: robust (CR2) standard errors.
    robust: bool,
    /// Normal family: between–within degrees of freedom.
    between_within: bool,
    /// Ordinal family: category order of a saved request (kept as is).
    order: Vec<String>,
}

/// The family's name without its link: "Poisson".
fn family_name(f: MixedFamily) -> &'static str {
    let label = f.label();
    label.split(" (").next().unwrap_or(label)
}

impl MixedDialog {
    pub fn new(table: &DataTable) -> Self {
        Self {
            edit: None,
            family: MixedFamily::Normal,
            response: None,
            trials: None,
            use_trials: false,
            offset: None,
            predictors: vec![false; table.n_cols()],
            effects: Vec::new(),
            terms: vec![TermDraft::default()],
            reml: true,
            quadrature: false,
            points: 7,
            hierarchical: true,
            slices: None,
            slopes: None,
            frequency: None,
            weight: None,
            residual_groups: None,
            robust: false,
            between_within: false,
            order: Vec::new(),
        }
    }

    /// The dialog filled from a saved mixed model.
    pub fn from_saved(saved: &SavedReport, table: &DataTable) -> Option<Self> {
        let SavedReport::Mixed { request, .. } = saved else { return None };
        let valid = |c: usize| kit::valid(table, c);
        let predictors: Vec<usize> = request.predictors.iter().copied().filter(|&c| c < table.n_cols()).collect();
        let effects: Vec<Vec<(usize, u8)>> = if request.effects.is_empty() {
            predictors.iter().map(|&c| vec![(c, 1)]).collect()
        } else {
            request
                .effects
                .iter()
                .filter_map(|e| {
                    e.iter().map(|&(k, p)| request.predictors.get(k).copied().and_then(valid).map(|c| (c, p))).collect()
                })
                .collect()
        };
        let mut closed = hierarchical_closure(&effects);
        let mut mine = effects.clone();
        closed.sort();
        mine.sort();
        let terms: Vec<TermDraft> = request
            .random
            .iter()
            .map(|r| TermDraft {
                group: valid(r.group),
                within: r.within.first().copied().and_then(valid),
                deeper: r.within.iter().skip(1).copied().filter(|&c| c < table.n_cols()).collect(),
                intercept: r.intercept,
                slopes: r.slopes.iter().copied().filter(|&c| c < table.n_cols()).collect(),
                correlated: r.correlated,
            })
            .collect();
        Some(Self {
            family: request.family,
            response: valid(request.response),
            trials: request.trials.and_then(valid),
            use_trials: request.trials.is_some(),
            offset: request.offset.and_then(valid),
            predictors: kit::mask(table.n_cols(), &predictors),
            effects,
            terms: if terms.is_empty() { vec![TermDraft::default()] } else { terms },
            reml: request.reml,
            quadrature: request.quadrature_points > 1,
            points: if request.quadrature_points > 1 { request.quadrature_points } else { 7 },
            hierarchical: closed == mine,
            slices: request.slices,
            slopes: request.slopes,
            frequency: request.frequency.and_then(valid),
            weight: request.weight.and_then(valid),
            residual_groups: request.residual_groups.and_then(valid),
            robust: request.robust,
            between_within: request.df_method == app_model::DfMethod::BetweenWithin,
            order: request.order.clone(),
            ..Self::new(table)
        })
    }

    /// The request, or what is missing.
    fn request(&self, chosen: &[usize]) -> Result<MixedRequest, String> {
        let response = self.response.ok_or("Choose a response")?;
        let single = self.terms.len() == 1;
        let which = |k: usize| if single { String::new() } else { format!(" for random effect {}", k + 1) };
        let mut random = Vec::with_capacity(self.terms.len());
        for (k, t) in self.terms.iter().enumerate() {
            let group = t.group.ok_or_else(|| format!("Choose a grouping column{}", which(k)))?;
            if !t.intercept && t.slopes.is_empty() {
                return Err(format!("Choose an intercept or at least one slope{}", which(k)));
            }
            random.push(RandomEffectSpec {
                group,
                within: t.within.into_iter().chain(t.deeper.iter().copied()).collect(),
                intercept: t.intercept,
                slopes: t.slopes.clone(),
                correlated: t.correlated,
            });
        }
        if random.is_empty() {
            return Err("Add a random effect".into());
        }
        // A column chosen before the response was changed to it stays selected
        // in its own drop-down list; the response cannot play another role.
        if random.iter().any(|r| r.group == response || r.within.contains(&response) || r.slopes.contains(&response)) {
            return Err("The response cannot also be a grouping, nesting or slope column".into());
        }
        let binomial_trials = self.family == MixedFamily::Binomial && self.use_trials;
        if binomial_trials {
            match self.trials {
                None => return Err("Choose a trials column".into()),
                Some(t) if t == response => return Err("The response cannot also be the trials column".into()),
                Some(_) => {}
            }
        }
        if self.family != MixedFamily::Normal && self.offset == Some(response) {
            return Err("The response cannot also be the offset column".into());
        }
        let normal = self.family == MixedFamily::Normal;
        let weight = if self.family == MixedFamily::Binomial { None } else { self.weight };
        let residual_groups = if normal { self.residual_groups } else { None };
        if [self.frequency, weight, residual_groups].contains(&Some(response)) {
            return Err("The response cannot also be the frequency, weight or residual-variance column".into());
        }
        // Effects as positions in the predictor list: main effects first.
        let position = |c: usize| chosen.iter().position(|&x| x == c).unwrap_or(0);
        let mut effects: Vec<Vec<(usize, u8)>> =
            self.effects.iter().map(|e| e.iter().map(|&(c, p)| (position(c), p)).collect()).collect();
        effects.sort_by_key(|e| (e.iter().map(|f| u32::from(f.1)).sum::<u32>(), e.clone()));
        let mains = effects.len() == chosen.len() && effects.iter().all(|e| matches!(e.as_slice(), [(_, 1)]));
        let agq = self.quadrature && random.len() == 1 && random[0].intercept && random[0].slopes.is_empty();
        Ok(MixedRequest {
            response,
            predictors: chosen.to_vec(),
            effects: if mains { Vec::new() } else { effects },
            random,
            family: self.family,
            trials: if binomial_trials { self.trials } else { None },
            offset: if self.family == MixedFamily::Normal { None } else { self.offset },
            reml: self.reml,
            quadrature_points: if agq { self.points } else { 1 },
            slices: self.slices,
            slopes: self.slopes,
            frequency: self.frequency,
            weight,
            robust: normal && self.robust,
            residual_groups,
            df_method: if normal && self.between_within {
                app_model::DfMethod::BetweenWithin
            } else {
                app_model::DfMethod::Satterthwaite
            },
            order: if self.family == MixedFamily::Ordinal { self.order.clone() } else { Vec::new() },
        })
    }

    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<MixedRequest> {
        let dialog = Dialog::new("Mixed model").editing(self.edit);
        match dialog.show(ctx, |ui| self.contents(ui, table)) {
            Outcome::Reset => {
                *self = Self { edit: self.edit, ..Self::new(table) };
                Outcome::Open
            }
            outcome => outcome,
        }
    }

    fn contents(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<MixedRequest> {
        let n = table.n_cols();
        let name = |i: usize| kit::name(table, i).to_owned();
        let numeric = |i: usize| kit::is_numeric(table, i);
        let continuous = |i: usize| kit::is_continuous(table, i);
        // A steady width while options appear and disappear.
        ui.set_min_width(kit::em(ui) * 40.0);
        ui.horizontal_wrapped(|ui| {
            ui.label("Response distribution:");
            for f in MixedFamily::ALL {
                ui.radio_value(&mut self.family, f, f.label());
            }
        });
        let two_values = self.family == MixedFamily::Binomial && !self.use_trials;
        let categories = self.family == MixedFamily::Ordinal;
        let allow_response = |i: usize| two_values || categories || numeric(i);
        if self.response.is_some_and(|r| !allow_response(r)) {
            self.response = None;
        }
        ui.horizontal(|ui| {
            ui.label("Response:");
            kit::column_choice(ui, "mixed_response", table, &mut self.response, allow_response, None);
        });
        let response = self.response;
        if categories {
            kit::note(
                ui,
                "Ordered categories, lowest first in sorted order; positive coefficients favour higher categories.",
            );
        }
        if self.family == MixedFamily::Binomial {
            ui.horizontal(|ui| {
                ui.radio_value(&mut self.use_trials, false, "Two values (the second in sorted order is the event)");
                ui.radio_value(&mut self.use_trials, true, "Successes out of trials in:");
                ui.add_enabled_ui(self.use_trials, |ui| {
                    let allow = |i: usize| numeric(i) && Some(i) != response;
                    kit::column_choice(ui, "mixed_trials", table, &mut self.trials, allow, None);
                });
            });
        }
        ui.separator();
        ui.label(RichText::new("Fixed effects").strong());
        kit::column_list(ui, "mixed_predictors", table, &mut self.predictors, |i| Some(i) != response);
        let chosen: Vec<usize> =
            kit::chosen_columns(&self.predictors).into_iter().filter(|&i| i < n && Some(i) != response).collect();
        self.effects.retain(|e| e.iter().all(|(c, _)| chosen.contains(c)));
        for &c in &chosen {
            if !self.effects.iter().any(|e| e.iter().any(|(x, _)| *x == c)) {
                self.effects.push(vec![(c, 1)]);
            }
        }
        if chosen.is_empty() {
            kit::note(ui, "No predictors: the fixed part is the intercept alone.");
        } else {
            let candidates = crate::model_dialog::candidate_effects(&chosen, &continuous);
            ui.horizontal(|ui| {
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
            egui::ScrollArea::vertical().id_salt("mixed_effects").max_height(kit::em(ui) * 7.0).show(ui, |ui| {
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
                    "Adding an interaction or power adds the effects it is built from; removing an effect \
                     removes the interactions and powers built on it",
                )
                .changed()
                && self.hierarchical
            {
                self.effects = hierarchical_closure(&self.effects);
            }
        }
        ui.separator();
        ui.label(RichText::new("Random effects").strong())
            .on_hover_text("Effects that vary from level to level of a grouping column, such as subjects or batches");
        let mut remove = None;
        let single = self.terms.len() == 1;
        for (k, t) in self.terms.iter_mut().enumerate() {
            ui.horizontal_wrapped(|ui| {
                ui.label(format!("{}.", k + 1));
                kit::column_choice(ui, ("mixed_group", k), table, &mut t.group, |i| Some(i) != response, None);
                ui.label("within")
                    .on_hover_text("Nest the grouping within another column: its levels become the combinations");
                let group = t.group;
                let allow = |i: usize| Some(i) != response && Some(i) != group;
                kit::column_choice(ui, ("mixed_within", k), table, &mut t.within, allow, Some("(not nested)"));
                t.within = t.within.filter(|w| Some(*w) != t.group);
                ui.checkbox(&mut t.intercept, "Intercept");
                let shown = if t.slopes.is_empty() {
                    "Slopes: none".to_owned()
                } else {
                    format!("Slopes: {}", t.slopes.iter().map(|&c| name(c)).collect::<Vec<_>>().join(", "))
                };
                ui.menu_button(shown, |ui| {
                    let mut mask = kit::mask(n, &t.slopes);
                    let allow = |i: usize| continuous(i) && Some(i) != response;
                    if kit::column_list(ui, ("mixed_slopes", k), table, &mut mask, allow) {
                        t.slopes = kit::chosen_columns(&mask);
                    }
                })
                .response
                .on_hover_text("Continuous columns whose slope varies by level");
                let several = usize::from(t.intercept) + t.slopes.len() > 1;
                ui.add_enabled(several, egui::Checkbox::new(&mut t.correlated, "Correlated"))
                    .on_hover_text("Estimate correlations between this term's intercept and slopes");
                if !single && ui.small_button("×").on_hover_text("Remove this random effect").clicked() {
                    remove = Some(k);
                }
            });
        }
        if let Some(k) = remove {
            self.terms.remove(k);
        }
        if ui.small_button("+ Add random effect").clicked() {
            self.terms.push(TermDraft::default());
        }
        if self.family != MixedFamily::Normal {
            ui.horizontal(|ui| {
                ui.label("Offset (optional):")
                    .on_hover_text("Added to the linear predictor with coefficient 1, e.g. the log of exposure time");
                let allow = |i: usize| numeric(i) && Some(i) != response;
                kit::column_choice(ui, "mixed_offset", table, &mut self.offset, allow, Some("None"));
            });
        }
        ui.horizontal(|ui| {
            ui.label("Frequency (optional):").on_hover_text("Each row counts this many times (whole numbers)");
            let allow = |i: usize| numeric(i) && Some(i) != response;
            kit::column_choice(ui, "mixed_frequency", table, &mut self.frequency, allow, Some("None"));
            if self.family != MixedFamily::Binomial {
                ui.label("Weight (optional):").on_hover_text(
                    "Prior weights: a normal response's residual variance is divided by the weight; count and \
                     ordinal rows count in proportion to it",
                );
                let allow = |i: usize| numeric(i) && Some(i) != response;
                kit::column_choice(ui, "mixed_weight", table, &mut self.weight, allow, Some("None"));
            }
        });
        if self.family == MixedFamily::Normal {
            ui.horizontal(|ui| {
                ui.label("Residual variance differs by (optional):").on_hover_text(
                    "A separate residual variance for each level of this column, with a test that they are equal",
                );
                let allow = |i: usize| Some(i) != response;
                kit::column_choice(ui, "mixed_residual_groups", table, &mut self.residual_groups, allow, Some("None"));
            });
        }
        kit::more_options(ui, |ui| {
            if self.family == MixedFamily::Normal {
                ui.horizontal(|ui| {
                    ui.label("Denominator degrees of freedom:");
                    ui.radio_value(&mut self.between_within, false, "Satterthwaite");
                    ui.radio_value(&mut self.between_within, true, "Between–within").on_hover_text(
                        "Containment degrees of freedom for nested groupings, as in classical split-plot analysis",
                    );
                });
                ui.checkbox(&mut self.robust, "Robust standard errors, clustered on the outermost grouping")
                    .on_hover_text(
                        "Sandwich standard errors with the CR2 small-sample correction and Bell–McCaffrey degrees \
                         of freedom; they stay valid when the variance structure is wrong",
                    );
                ui.horizontal(|ui| {
                    ui.label("Estimation:");
                    ui.radio_value(&mut self.reml, true, "REML")
                        .on_hover_text("Restricted maximum likelihood: unbiased variance estimates (recommended)");
                    ui.radio_value(&mut self.reml, false, "Maximum likelihood").on_hover_text(
                        "Needed to compare models with different fixed effects by AIC or likelihood ratio",
                    );
                });
            } else {
                let eligible = self.terms.len() == 1 && self.terms[0].intercept && self.terms[0].slopes.is_empty();
                ui.horizontal(|ui| {
                    ui.label("Estimation:");
                    ui.radio_value(&mut self.quadrature, false, "Laplace approximation");
                    ui.add_enabled_ui(eligible, |ui| {
                        ui.radio_value(&mut self.quadrature, true, "Adaptive quadrature with")
                            .on_hover_text("More accurate for small groups of binary or count data");
                        ui.add(egui::DragValue::new(&mut self.points).range(2..=25));
                        ui.label("points");
                    });
                });
                if !eligible {
                    kit::note(ui, "Adaptive quadrature needs a single random intercept (no slopes).");
                }
            }
            crate::model_terms_ui::comparison_settings(ui, &mut self.slices, &mut self.slopes);
        });
        let request = self.request(&chosen)?;
        let fixed = kit::count(self.effects.len(), "fixed effect", "fixed effects");
        let groups: Vec<String> = request.random.iter().map(|r| name(r.group)).collect();
        let family = if self.family == MixedFamily::Normal {
            "Mixed model".to_owned()
        } else {
            format!("{} mixed model", family_name(self.family))
        };
        let summary = format!(
            "{family} of {} on {fixed}, random effects by {}",
            kit::name(table, request.response),
            groups.join(", ")
        );
        kit::ready(request, summary)
    }
}

impl Remember for MixedDialog {
    fn memory_key(&self) -> String {
        "Mixed model".into()
    }

    fn set_edit(&mut self, report: Option<u64>) {
        self.edit = report;
    }

    fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        self.predictors = kit::mask(table.n_cols(), columns);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    fn table() -> DataTable {
        DataTable::new(
            "t",
            vec![
                Column::numeric("y", vec![1.0, 2.0, 3.0, 4.0]),
                Column::numeric("x", vec![1.0, 2.0, 1.0, 2.0]),
                Column::text("subject", vec!["a".into(), "a".into(), "b".into(), "b".into()]),
            ],
        )
        .unwrap()
    }

    #[test]
    fn the_response_cannot_also_be_a_random_effect_column() {
        let mut d = MixedDialog::new(&table());
        d.terms[0].group = Some(2);
        d.response = Some(1);
        assert!(d.request(&[]).is_ok());
        // The grouping column picked first, then the response changed to it.
        d.response = Some(2);
        assert!(d.request(&[]).is_err());
        d.response = Some(0);
        d.terms[0].slopes = vec![0];
        assert!(d.request(&[]).is_err());
        d.terms[0].slopes = vec![1];
        assert!(d.request(&[]).is_ok());
    }

    #[test]
    fn the_response_cannot_also_be_the_trials_or_offset_column() {
        let mut d = MixedDialog::new(&table());
        d.terms[0].group = Some(2);
        d.response = Some(0);
        d.family = MixedFamily::Binomial;
        d.use_trials = true;
        d.trials = Some(0);
        assert!(d.request(&[]).is_err());
        d.trials = Some(1);
        assert!(d.request(&[]).is_ok());
        d.family = MixedFamily::Poisson;
        d.offset = Some(0);
        assert!(d.request(&[]).is_err());
    }
}
