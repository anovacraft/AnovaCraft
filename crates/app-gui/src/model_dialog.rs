//! Dialog for specifying a linear model, a response surface, a stepwise or
//! best-subsets search (a stepwise search may run separately for several
//! responses, opening them together in a several-responses report), an
//! ordinal or nominal logistic regression, or a multivariate analysis of
//! variance of several responses.

use crate::dialog_kit::ComboExt as _;
use app_model::MultivariateStatistic;
use app_model::best_subsets_analysis::SUBSET_CRITERIA;
use app_model::hierarchy::{hierarchical_closure, set_effect};
use app_model::{BestSubsetsRequest, CategoricalLogisticRequest, CategoricalModel, SavedReport, SubsetCriterion};
use app_model::{Heredity, LinearModelSpec, StepCriterion, StepDirection, StepwiseRequest, SurfaceRequest};
use app_model::{ManovaContrast, ManovaRequest, ModelForm, MultipleResponsesRequest, TermSelection};
use eframe::egui;
use stat_data::DataTable;

use crate::dialog_kit::{self as kit, Dialog, Outcome, Readiness, Remember};
use crate::report_view::ReportRequest;

#[derive(Clone)]
pub struct ModelDialog {
    /// The report being edited (`None`: a new report).
    edit: Option<u64>,
    response: Option<usize>,
    /// Indexed by column.
    predictors: Vec<bool>,
    /// Response-surface mode: continuous factors only, second-order model.
    surface: bool,
    /// Selected effects as `(column, power)` products (columns, not positions,
    /// so they survive changes to the predictor list).
    effects: Vec<Vec<(usize, u8)>>,
    /// Predictors chosen as of the last frame: only a newly chosen
    /// predictor enters as a main effect, so a main effect unticked from
    /// the effect list stays out.
    entered: Vec<usize>,
    /// Stepwise mode: the effects are candidates for a search.
    stepwise: bool,
    direction: StepDirection,
    /// 0 p-values, 1 AIC, 2 BIC.
    criterion: usize,
    enter: f64,
    remove: f64,
    /// Keep the model hierarchical: an interaction or power brings the
    /// effects it is built from, and removing an effect removes what is
    /// built on it.
    hierarchical: bool,
    /// Stepwise and best subsets: parent effects an interaction or power
    /// needs in the model.
    heredity: Heredity,
    /// Best-subsets mode: every subset of the effects is searched.
    best_subsets: bool,
    /// Best subsets: effects in every model, as `(column, power)` products.
    forced: Vec<Vec<(usize, u8)>>,
    /// Best subsets: models kept per size.
    models_per_size: usize,
    /// Best subsets: largest model size (`None`: every candidate).
    max_size: Option<usize>,
    /// Best subsets: criterion for the best model overall.
    subset_criterion: SubsetCriterion,
    /// Categorical-response mode: ordinal or nominal logistic regression.
    categorical: bool,
    /// Linear model: continuous predictors entered as splines.
    splines: crate::model_terms_ui::ColumnSplines,
    categorical_model: CategoricalModel,
    /// The response the category order was read from.
    order_of: Option<usize>,
    /// Response categories, lowest first for the ordinal model.
    order: Vec<String>,
    /// Nominal model: reference category.
    reference: Option<String>,
    /// MANOVA mode: several responses on the same effects.
    manova: bool,
    /// MANOVA (and stepwise for several responses) responses, indexed by column.
    responses: Vec<bool>,
    /// Stepwise mode: a separate search for each of several responses,
    /// explored together (a several-responses report).
    several: bool,
    /// The several-responses report being edited: its goals, settings,
    /// simulation and constraints stay while the responses and factors do.
    kept_several: Option<MultipleResponsesRequest>,
    /// MANOVA: statistic ranking the Effect Pareto.
    statistic: MultivariateStatistic,
    /// MANOVA: contrasts to test.
    contrasts: Vec<ManovaContrast>,
    /// MANOVA: factor of the canonical plot (`None`: the first categorical one).
    plot_factor: Option<usize>,
    /// MANOVA contrast being written: its factor, a weight per level, a name.
    contrast_factor: Option<usize>,
    contrast_weights: Vec<f64>,
    contrast_name: String,
}

/// Distinct non-empty values of column `c`, in natural order.
fn categories_of(table: &DataTable, c: usize) -> Vec<String> {
    let Some(col) = table.column(c) else { return Vec::new() };
    let mut out: Vec<String> = (0..table.n_rows()).map(|r| col.level_text(r)).filter(|v| !v.trim().is_empty()).collect();
    out.sort_by(|a, b| stat_data::natural_order(a, b));
    out.dedup();
    out
}

/// Every candidate effect for `chosen` predictors: main effects, two-factor
/// interactions, and squares and cubes of continuous predictors.
pub(crate) fn candidate_effects(chosen: &[usize], continuous: &dyn Fn(usize) -> bool) -> Vec<Vec<(usize, u8)>> {
    let mut out: Vec<Vec<(usize, u8)>> = chosen.iter().map(|&c| vec![(c, 1)]).collect();
    for (a, &i) in chosen.iter().enumerate() {
        for &j in &chosen[a + 1..] {
            out.push(vec![(i, 1), (j, 1)]);
        }
    }
    for &c in chosen.iter().filter(|&&c| continuous(c)) {
        out.push(vec![(c, 2)]);
        out.push(vec![(c, 3)]);
    }
    out
}

/// An effect's name: "A", "A × B", "A²".
fn effect_label(table: &DataTable, e: &[(usize, u8)]) -> String {
    e.iter()
        .map(|&(c, p)| {
            let name = kit::name(table, c);
            if p == 1 { name.to_owned() } else { format!("{name}{}", stat_core::text::superscript(&p.to_string())) }
        })
        .collect::<Vec<_>>()
        .join(" × ")
}

/// "3 effects (2 interactions, 1 power)".
fn effects_summary(effects: &[Vec<(usize, u8)>]) -> String {
    let interactions = effects.iter().filter(|e| e.len() > 1).count();
    let powers = effects.iter().filter(|e| e.len() == 1 && e[0].1 > 1).count();
    let mut parts = Vec::new();
    if interactions > 0 {
        parts.push(kit::count(interactions, "interaction", "interactions"));
    }
    if powers > 0 {
        parts.push(kit::count(powers, "power", "powers"));
    }
    let all = kit::count(effects.len(), "effect", "effects");
    if parts.is_empty() { all } else { format!("{all} ({})", parts.join(", ")) }
}

impl ModelDialog {
    pub fn new(table: &DataTable) -> Self {
        Self {
            edit: None,
            response: None,
            predictors: vec![false; table.n_cols()],
            surface: false,
            effects: Vec::new(),
            entered: Vec::new(),
            stepwise: false,
            direction: StepDirection::Mixed,
            criterion: 0,
            enter: 0.15,
            remove: 0.15,
            hierarchical: true,
            heredity: Heredity::Strong,
            best_subsets: false,
            forced: Vec::new(),
            models_per_size: 3,
            max_size: None,
            subset_criterion: SubsetCriterion::Bic,
            categorical: false,
            splines: Vec::new(),
            categorical_model: CategoricalModel::Ordinal,
            order_of: None,
            order: Vec::new(),
            reference: None,
            manova: false,
            responses: vec![false; table.n_cols()],
            several: false,
            kept_several: None,
            statistic: MultivariateStatistic::Pillai,
            contrasts: Vec::new(),
            plot_factor: None,
            contrast_factor: None,
            contrast_weights: Vec::new(),
            contrast_name: String::new(),
        }
    }

    /// The dialog for a multivariate analysis of variance.
    pub fn manova(table: &DataTable) -> Self {
        Self { manova: true, ..Self::new(table) }
    }

    /// The dialog for ordinal or nominal logistic regression of a
    /// categorical response.
    pub fn categorical(table: &DataTable) -> Self {
        Self { categorical: true, ..Self::new(table) }
    }

    /// The dialog for best-subsets regression among candidate effects.
    pub fn best_subsets(table: &DataTable) -> Self {
        Self { stepwise: true, best_subsets: true, ..Self::new(table) }
    }

    /// The dialog for stepwise selection among candidate effects.
    pub fn stepwise(table: &DataTable) -> Self {
        Self { stepwise: true, ..Self::new(table) }
    }

    /// The dialog for a second-order response surface.
    pub fn surface(table: &DataTable) -> Self {
        Self { surface: true, ..Self::new(table) }
    }

    /// A fresh dialog in the same mode.
    fn fresh(&self, table: &DataTable) -> Self {
        Self {
            edit: self.edit,
            surface: self.surface,
            stepwise: self.stepwise,
            best_subsets: self.best_subsets,
            categorical: self.categorical,
            manova: self.manova,
            ..Self::new(table)
        }
    }

    /// The dialog filled from a saved linear model, response surface,
    /// stepwise or best-subsets search, or categorical logistic regression.
    pub fn from_saved(saved: &SavedReport, table: &DataTable) -> Option<Self> {
        let mut d = match saved {
            SavedReport::Linear { spec, .. } => {
                let mut d = Self::new(table);
                d.load(spec.response, &spec.predictors, &spec.effects, table);
                d.splines = crate::model_terms_ui::by_column(&spec.splines, &spec.predictors);
                d
            }
            SavedReport::Surface { request, .. } => {
                let mut d = Self::surface(table);
                d.load(request.response, &request.factors, &[], table);
                d
            }
            SavedReport::Stepwise { request, .. } => {
                let mut d = Self::stepwise(table);
                let spec = &request.candidates;
                d.load(spec.response, &spec.predictors, &spec.effects, table);
                d.direction = request.direction;
                (d.criterion, d.enter, d.remove) = match request.criterion {
                    StepCriterion::PValue { enter, remove } => (0, enter, remove),
                    StepCriterion::Aic => (1, d.enter, d.remove),
                    StepCriterion::Bic => (2, d.enter, d.remove),
                };
                d.heredity = request.heredity;
                d
            }
            SavedReport::BestSubsets { request } => {
                let mut d = Self::best_subsets(table);
                let spec = &request.candidates;
                d.load(spec.response, &spec.predictors, &spec.effects, table);
                d.forced = request
                    .forced
                    .iter()
                    .filter_map(|e| e.iter().map(|&(k, p)| Some((*spec.predictors.get(k)?, p))).collect())
                    .collect();
                d.heredity = request.heredity;
                d.models_per_size = request.models_per_size;
                d.max_size = request.max_size;
                d.subset_criterion = request.criterion;
                d
            }
            SavedReport::CategoricalLogistic { request } => {
                let mut d = Self::categorical(table);
                d.load(request.response, &request.predictors, &request.effects, table);
                d.categorical_model = request.model;
                d.order_of = d.response;
                d.order = if request.order.is_empty() {
                    d.response.map(|r| categories_of(table, r)).unwrap_or_default()
                } else {
                    request.order.clone()
                };
                d.reference = request.reference.clone();
                d
            }
            // Candidates chosen here come back here; a model form's terms
            // go to the several-responses dialog.
            SavedReport::MultipleResponses { request }
                if request.selection.as_ref().is_some_and(|s| !s.candidates.is_empty()) =>
            {
                let selection = request.selection.as_ref()?;
                let mut d = Self::stepwise(table);
                d.load(usize::MAX, &request.predictors, &selection.candidates, table);
                d.several = true;
                d.responses = kit::mask(table.n_cols(), &request.responses);
                d.direction = selection.direction;
                (d.criterion, d.enter, d.remove) = match selection.criterion {
                    StepCriterion::PValue { enter, remove } => (0, enter, remove),
                    StepCriterion::Aic => (1, d.enter, d.remove),
                    StepCriterion::Bic => (2, d.enter, d.remove),
                };
                d.heredity = selection.heredity;
                d.kept_several = Some((**request).clone());
                d
            }
            SavedReport::Manova { request } => {
                let mut d = Self::manova(table);
                d.load(usize::MAX, &request.predictors, &request.effects, table);
                d.responses = kit::mask(table.n_cols(), &request.responses);
                d.statistic = request.statistic;
                d.contrasts = request.contrasts.clone();
                d.plot_factor = request.plot_factor;
                d
            }
            _ => return None,
        };
        d.edit = None;
        Some(d)
    }

    /// Set the response, predictors and effects (as positions in
    /// `predictors`; empty: main effects) from a saved request, dropping
    /// columns the table no longer has.
    fn load(&mut self, response: usize, predictors: &[usize], effects: &[Vec<(usize, u8)>], table: &DataTable) {
        self.response = kit::valid(table, response);
        self.predictors = kit::mask(table.n_cols(), predictors);
        self.effects = if effects.is_empty() {
            predictors.iter().filter(|&&c| c < table.n_cols()).map(|&c| vec![(c, 1)]).collect()
        } else {
            effects
                .iter()
                .filter_map(|e| {
                    e.iter()
                        .map(|&(k, p)| predictors.get(k).filter(|&&c| c < table.n_cols()).map(|&c| (c, p)))
                        .collect()
                })
                .collect()
        };
        self.entered = predictors.iter().copied().filter(|&c| c < table.n_cols()).collect();
        // A saved model that is not hierarchical stays as it was.
        let mut closed = hierarchical_closure(&self.effects);
        let mut mine = self.effects.clone();
        closed.sort();
        mine.sort();
        self.hierarchical = closed == mine;
    }

    fn title(&self) -> &'static str {
        if self.categorical {
            "Ordinal or nominal logistic regression"
        } else if self.manova {
            "Multivariate analysis of variance"
        } else if self.surface {
            "Response surface"
        } else if self.best_subsets {
            "Best subsets"
        } else if self.stepwise {
            "Stepwise selection"
        } else {
            "Linear model"
        }
    }

    /// Model choice, category order (ordinal) and reference category
    /// (nominal) for a categorical response.
    fn categorical_options(&mut self, ui: &mut egui::Ui, table: &DataTable) {
        let Some(response) = self.response else { return };
        if self.order_of != Some(response) {
            self.order_of = Some(response);
            self.order = categories_of(table, response);
            self.reference = None;
            let nominal = table.column(response).is_some_and(|c| c.level == stat_data::MeasurementLevel::Nominal);
            self.categorical_model = if nominal { CategoricalModel::Nominal } else { CategoricalModel::Ordinal };
        }
        if self.order.len() < 2 {
            ui.label(egui::RichText::new("The response needs at least two categories.").weak().small());
            return;
        }
        if self.order.len() > 30 {
            ui.label(
                egui::RichText::new(format!(
                    "{} categories: a model for so many is rarely useful (is this the right column?)",
                    self.order.len()
                ))
                .weak()
                .small(),
            );
        }
        ui.horizontal(|ui| {
            ui.label("Model:");
            ui.radio_value(&mut self.categorical_model, CategoricalModel::Ordinal, "Ordinal (ordered categories)")
                .on_hover_text("Proportional odds: one set of effects shifts the response up or down the scale");
            ui.radio_value(&mut self.categorical_model, CategoricalModel::Nominal, "Nominal (unordered categories)")
                .on_hover_text("Baseline-category logits: each category has its own effects against a reference");
        });
        match self.categorical_model {
            CategoricalModel::Ordinal => {
                ui.label("Category order (lowest first):");
                let mut swap = None;
                egui::ScrollArea::vertical().id_salt("category_order").max_height(140.0).show(ui, |ui| {
                    let n = self.order.len();
                    for (k, level) in self.order.iter().enumerate() {
                        ui.horizontal(|ui| {
                            // Named for screen readers (and tests): the arrows alone say nothing.
                            let named = |response: egui::Response, name: String| {
                                response.widget_info(|| {
                                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name.clone())
                                });
                                response.on_hover_text(name)
                            };
                            let up = ui.add_enabled(k > 0, egui::Button::new("▲").small());
                            if named(up, format!("Move {level} lower")).clicked() {
                                swap = Some(k - 1);
                            }
                            let down = ui.add_enabled(k + 1 < n, egui::Button::new("▼").small());
                            if named(down, format!("Move {level} higher")).clicked() {
                                swap = Some(k);
                            }
                            ui.label(format!("{}. {level}", k + 1));
                        });
                    }
                });
                if let Some(k) = swap {
                    self.order.swap(k, k + 1);
                }
            }
            CategoricalModel::Nominal => {
                ui.horizontal(|ui| {
                    ui.label("Reference category:");
                    let shown = self.reference.clone().unwrap_or_else(|| self.order[0].clone());
                    egui::ComboBox::from_id_salt("reference_category").selected_text(shown).show_list(ui, |ui| {
                        for level in &self.order {
                            let on = self.reference.as_deref().unwrap_or(&self.order[0]) == level;
                            if ui.selectable_label(on, level).clicked() {
                                self.reference = Some(level.clone());
                            }
                        }
                    });
                });
            }
        }
        ui.add_space(6.0);
    }

    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<ReportRequest> {
        let dialog = Dialog::new(self.title()).editing(self.edit);
        match dialog.show(ctx, |ui| self.contents(ui, table)) {
            Outcome::Reset => {
                *self = self.fresh(table);
                Outcome::Open
            }
            outcome => outcome,
        }
    }

    /// A stepwise search for each of several responses.
    fn several_mode(&self) -> bool {
        self.several && self.stepwise && !self.best_subsets
    }

    /// Predictors chosen (not the response), in table order.
    fn chosen(&self, table: &DataTable) -> Vec<usize> {
        (0..table.n_cols().min(self.predictors.len()))
            .filter(|&i| self.predictors[i] && (self.several_mode() || Some(i) != self.response))
            .filter(|&i| !(self.manova || self.several_mode()) || !self.responses.get(i).copied().unwrap_or(false))
            .filter(|&i| !self.surface || kit::is_continuous(table, i))
            .collect()
    }

    fn contents(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<ReportRequest> {
        // The response must be continuous numeric (any column for a
        // categorical response); any other column can be a predictor:
        // continuous columns enter as they are, text and nominal / ordinal
        // columns as categorical factors.
        let categorical = self.categorical;
        let (surface, stepwise, best_subsets) = (self.surface, self.stepwise, self.best_subsets);
        let is_categorical = |i: usize| !kit::is_continuous(table, i);
        if self.manova {
            ui.label("Responses (numeric, two or more):");
            let predictors = self.predictors.clone();
            kit::column_list(ui, "manova_responses", table, &mut self.responses, |i| {
                kit::is_continuous(table, i) && !predictors.get(i).copied().unwrap_or(false)
            });
        } else if self.several_mode() {
            ui.label("Responses (numeric; each gets its own search):");
            let predictors = self.predictors.clone();
            kit::column_list(ui, "stepwise_responses", table, &mut self.responses, |i| {
                kit::is_continuous(table, i) && !predictors.get(i).copied().unwrap_or(false)
            });
        } else {
            ui.label(if categorical { "Response (categories):" } else { "Response (numeric):" });
            kit::column_choice(
                ui,
                "model_response",
                table,
                &mut self.response,
                |i| categorical || kit::is_continuous(table, i),
                None,
            );
        }
        if stepwise && !best_subsets {
            let was = self.several;
            ui.checkbox(&mut self.several, "Several responses, each with its own search").on_hover_text(
                "Runs the search separately for each response and opens the selected models together, with \
                 linked slice plots, desirability goals and a search for the best settings",
            );
            if self.several && !was {
                // The single response becomes the first of the list.
                if let Some(r) = self.response.filter(|&r| r < self.responses.len()) {
                    self.responses[r] = true;
                }
            }
        }
        if categorical {
            self.categorical_options(ui, table);
        }
        ui.add_space(6.0);
        if surface {
            ui.label("Factors (continuous; the full second-order model is fitted):");
        } else {
            ui.label("Predictors:");
        }
        let response = if self.several_mode() { None } else { self.response };
        let responses = if self.manova || self.several_mode() { self.responses.clone() } else { Vec::new() };
        kit::column_list(ui, "model_predictors", table, &mut self.predictors, |i| {
            Some(i) != response
                && !responses.get(i).copied().unwrap_or(false)
                && (!surface || kit::is_continuous(table, i))
        });
        let chosen = self.chosen(table);
        // Effects: keep those of chosen predictors; newly chosen predictors
        // enter as main effects (a main effect unticked later stays out).
        self.effects.retain(|e| e.iter().all(|(c, _)| chosen.contains(c)));
        for &c in &chosen {
            if !self.entered.contains(&c) && !self.effects.iter().any(|e| e.iter().any(|(x, _)| *x == c)) {
                self.effects.push(vec![(c, 1)]);
            }
        }
        self.entered.clone_from(&chosen);
        if !surface && !chosen.is_empty() {
            let continuous = |c: usize| !is_categorical(c);
            let candidates = candidate_effects(&chosen, &continuous);
            ui.add_space(6.0);
            ui.label(if stepwise { "Candidate effects:" } else { "Model effects:" });
            // In a stepwise search the heredity setting plays this role.
            let keep = if stepwise { self.heredity != Heredity::None } else { self.hierarchical };
            ui.horizontal(|ui| {
                if ui.small_button("Main effects").clicked() {
                    self.effects = chosen.iter().map(|&c| vec![(c, 1)]).collect();
                }
                if ui.small_button("+ Two-factor interactions").clicked() {
                    for e in candidates.iter().filter(|e| e.len() == 2) {
                        set_effect(&mut self.effects, e, true, keep);
                    }
                }
                if ui.small_button("+ Squares").clicked() {
                    for e in candidates.iter().filter(|e| e.len() == 1 && e[0].1 == 2) {
                        set_effect(&mut self.effects, e, true, keep);
                    }
                }
            });
            egui::ScrollArea::vertical().id_salt("model_effects").max_height(kit::em(ui) * 10.0).show(ui, |ui| {
                for e in &candidates {
                    let mut on = self.effects.contains(e);
                    if ui.checkbox(&mut on, effect_label(table, e)).changed() {
                        set_effect(&mut self.effects, e, on, keep);
                    }
                }
            });
        }
        self.forced.retain(|f| self.effects.contains(f));
        if stepwise && !best_subsets {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label("Direction:");
                for d in [StepDirection::Forward, StepDirection::Backward, StepDirection::Mixed] {
                    ui.radio_value(&mut self.direction, d, d.label());
                }
            });
            ui.horizontal(|ui| {
                ui.label("Criterion:");
                ui.radio_value(&mut self.criterion, 0, "p-values");
                ui.radio_value(&mut self.criterion, 1, "AIC");
                ui.radio_value(&mut self.criterion, 2, "BIC");
            });
        }
        if best_subsets && !chosen.is_empty() {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label("Best overall by:");
                for c in SUBSET_CRITERIA {
                    ui.radio_value(&mut self.subset_criterion, c, c.label());
                }
            });
        }
        if !surface && !chosen.is_empty() {
            kit::more_options(ui, |ui| self.more_options(ui, table));
        }
        self.remove = self.remove.max(self.enter);
        self.build(table, &chosen)
    }

    /// Settings most fits leave alone: hierarchy or heredity, step
    /// thresholds, best-subsets limits and forced effects.
    fn more_options(&mut self, ui: &mut egui::Ui, table: &DataTable) {
        if self.manova {
            self.manova_options(ui, table);
        }
        let (stepwise, best_subsets) = (self.stepwise, self.best_subsets);
        if !(self.manova || self.categorical || stepwise || best_subsets) {
            // Continuous predictors in the model as main effects only.
            let chosen = self.chosen(table);
            let eligible: Vec<usize> = chosen
                .into_iter()
                .filter(|&c| {
                    kit::is_continuous(table, c)
                        && self.effects.contains(&vec![(c, 1)])
                        && self.effects.iter().filter(|e| e.iter().any(|(x, _)| *x == c)).count() == 1
                })
                .collect();
            crate::model_terms_ui::spline_settings(ui, table, &eligible, &mut self.splines);
            ui.separator();
        }
        if stepwise {
            ui.horizontal(|ui| {
                ui.label("Heredity:").on_hover_text(
                    "Whether an interaction or power may be in the model without the effects it is built from",
                );
                for h in Heredity::ALL {
                    ui.radio_value(&mut self.heredity, h, h.label());
                }
            });
        } else if ui
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
        if stepwise && !best_subsets {
            ui.add_enabled_ui(self.criterion == 0, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Enter if p <");
                    ui.add(egui::DragValue::new(&mut self.enter).range(0.001..=0.5).speed(0.005));
                    ui.label("Remove if p >");
                    ui.add(egui::DragValue::new(&mut self.remove).range(0.001..=0.5).speed(0.005));
                });
            });
        }
        if best_subsets {
            ui.label("Effects in every model:");
            egui::ScrollArea::vertical().id_salt("forced_effects").max_height(kit::em(ui) * 6.0).show(ui, |ui| {
                for e in self.effects.clone() {
                    let mut on = self.forced.contains(&e);
                    if ui.checkbox(&mut on, effect_label(table, &e)).changed() {
                        if on {
                            self.forced.push(e);
                        } else {
                            self.forced.retain(|f| *f != e);
                        }
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label("Models kept per size:");
                ui.add(egui::DragValue::new(&mut self.models_per_size).range(1..=20));
                let mut limit = self.max_size.is_some();
                if ui.checkbox(&mut limit, "Largest model size:").changed() {
                    self.max_size = limit.then_some(self.effects.len().max(1));
                }
                if let Some(m) = &mut self.max_size {
                    ui.add(egui::DragValue::new(m).range(1..=self.effects.len().max(1)));
                }
            });
            let n = self.effects.len().saturating_sub(self.forced.len());
            if n > 25 {
                kit::note(
                    ui,
                    &format!(
                        "{n} free candidates: the search may take a long time; consider fewer effects or a largest \
                         model size"
                    ),
                );
            }
        }
        kit::note(
            ui,
            "Continuous predictors are centered in interactions and powers; categorical ones in interactions use \
             sum-to-zero coding.",
        );
    }

    /// MANOVA settings: the statistic ranking effects, the canonical plot's
    /// factor, and contrasts among a factor's levels.
    fn manova_options(&mut self, ui: &mut egui::Ui, table: &DataTable) {
        ui.horizontal(|ui| {
            ui.label("Rank effects by:")
                .on_hover_text("The statistic whose p-values order the Effect Pareto; the report shows all four");
            for s in MultivariateStatistic::ALL {
                ui.radio_value(&mut self.statistic, s, s.label());
            }
        });
        let factors: Vec<usize> = self.chosen(table).into_iter().filter(|&i| !kit::is_continuous(table, i)).collect();
        self.contrasts.retain(|c| factors.contains(&c.factor));
        if self.plot_factor.is_some_and(|f| !factors.contains(&f)) {
            self.plot_factor = None;
        }
        if factors.is_empty() {
            return;
        }
        ui.horizontal(|ui| {
            ui.label("Canonical plot of:");
            let shown = self.plot_factor.map_or("First categorical factor", |f| kit::name(table, f));
            egui::ComboBox::from_id_salt("manova_plot_factor").selected_text(shown).show_list(ui, |ui| {
                ui.selectable_value(&mut self.plot_factor, None, "First categorical factor");
                for &f in &factors {
                    ui.selectable_value(&mut self.plot_factor, Some(f), kit::name(table, f));
                }
            });
        });
        ui.add_space(4.0);
        ui.label("Contrasts among levels:").on_hover_text(
            "Weights on a factor's level means (least-squares means) that add to zero, e.g. 1 and −1 to compare \
             two levels",
        );
        let mut remove = None;
        for (k, c) in self.contrasts.iter().enumerate() {
            ui.horizontal(|ui| {
                let weights: Vec<String> = c
                    .weights
                    .iter()
                    .filter(|(_, w)| *w != 0.0)
                    .map(|(l, w)| format!("{} {l}", format!("{w:+}").replace('-', "−")))
                    .collect();
                ui.label(format!("{}: {} ({})", c.name, kit::name(table, c.factor), weights.join(" ")));
                let button = ui.small_button("✕");
                let name = format!("Remove contrast {}", c.name);
                button.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name.clone()));
                if button.on_hover_text("Remove this contrast").clicked() {
                    remove = Some(k);
                }
            });
        }
        if let Some(k) = remove {
            self.contrasts.remove(k);
        }
        if self.contrast_factor.is_none_or(|f| !factors.contains(&f)) {
            self.contrast_factor = Some(factors[0]);
            self.contrast_weights.clear();
        }
        let factor = self.contrast_factor.unwrap_or(factors[0]);
        ui.horizontal(|ui| {
            ui.label("New contrast on:");
            egui::ComboBox::from_id_salt("manova_contrast_factor").selected_text(kit::name(table, factor)).show_list(
                ui,
                |ui| {
                    for &f in &factors {
                        if ui.selectable_value(&mut self.contrast_factor, Some(f), kit::name(table, f)).clicked() {
                            self.contrast_weights.clear();
                        }
                    }
                },
            );
        });
        let levels = categories_of(table, factor);
        self.contrast_weights.resize(levels.len(), 0.0);
        egui::Grid::new("manova_contrast_weights").num_columns(2).show(ui, |ui| {
            for (level, w) in levels.iter().zip(&mut self.contrast_weights) {
                ui.label(level);
                let drag = ui.add(egui::DragValue::new(w).speed(0.1));
                let name = format!("Weight of {level}");
                drag.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, name.clone()));
                ui.end_row();
            }
        });
        let total: f64 = self.contrast_weights.iter().sum();
        let nonzero: Vec<(String, f64)> =
            levels.iter().cloned().zip(self.contrast_weights.iter().copied()).filter(|(_, w)| *w != 0.0).collect();
        ui.horizontal(|ui| {
            ui.label("Name:");
            ui.add(egui::TextEdit::singleline(&mut self.contrast_name).desired_width(kit::em(ui) * 10.0).hint_text(
                match nonzero.as_slice() {
                    [(a, _), (b, _)] => format!("{a} vs {b}"),
                    _ => format!("Contrast {}", self.contrasts.len() + 1),
                },
            ));
            let ready = !nonzero.is_empty() && total.abs() < 1e-9;
            let add = ui.add_enabled(ready, egui::Button::new("Add contrast"));
            let add = if ready { add } else { add.on_disabled_hover_text("Give weights that add to zero") };
            if add.clicked() {
                let name = if self.contrast_name.trim().is_empty() {
                    match nonzero.as_slice() {
                        [(a, _), (b, _)] => format!("{a} vs {b}"),
                        _ => format!("Contrast {}", self.contrasts.len() + 1),
                    }
                } else {
                    self.contrast_name.trim().to_owned()
                };
                self.contrasts.push(ManovaContrast { name, factor, weights: nonzero.clone() });
                self.contrast_weights.iter_mut().for_each(|w| *w = 0.0);
                self.contrast_name.clear();
            }
        });
        ui.add_space(4.0);
    }

    /// The request, or what is missing.
    fn build(&self, table: &DataTable, chosen: &[usize]) -> Readiness<ReportRequest> {
        if self.manova {
            return self.build_manova(table, chosen);
        }
        if self.several_mode() {
            return self.build_several(table, chosen);
        }
        let Some(response) = self.response else { return Err("Choose a response".into()) };
        if chosen.is_empty() {
            return Err(
                if self.surface { "Choose at least one factor" } else { "Choose at least one predictor" }.into()
            );
        }
        if !self.surface && self.effects.is_empty() {
            return Err("Choose at least one model effect".into());
        }
        // A predictor whose every effect was unticked is not in the model.
        let used = if self.surface { chosen.to_vec() } else { self.in_effects(chosen) };
        let chosen = used.as_slice();
        if self.categorical && self.order.len() < 2 {
            return Err("The response needs at least two categories".into());
        }
        let y = kit::name(table, response);
        if self.surface {
            let k = chosen.len();
            let terms = 1 + 2 * k + k * (k - 1) / 2;
            return kit::ready(
                ReportRequest::Surface(SurfaceRequest { response, factors: chosen.to_vec() }),
                format!("Second-order surface of {y} in {} ({terms} terms)", kit::count(k, "factor", "factors")),
            );
        }
        // Effects as positions in the predictor list, in a stable order:
        // main effects, then interactions, then powers.
        let position = |c: usize| chosen.iter().position(|&x| x == c).unwrap_or(0);
        let mut effects: Vec<Vec<(usize, u8)>> =
            self.effects.iter().map(|e| e.iter().map(|&(c, p)| (position(c), p)).collect()).collect();
        effects.sort_by_key(|e| (e.iter().map(|f| u32::from(f.1)).sum::<u32>(), e.clone()));
        let mains = effects.len() == chosen.len() && effects.iter().all(|e| matches!(e.as_slice(), [(_, 1)]));
        let described = effects_summary(&self.effects);
        if self.categorical {
            let ordinal = self.categorical_model == CategoricalModel::Ordinal;
            let model = if ordinal { "Ordinal" } else { "Nominal" };
            let n = self.order.len();
            return kit::ready(
                ReportRequest::CategoricalLogistic(CategoricalLogisticRequest {
                    response,
                    predictors: chosen.to_vec(),
                    effects: if mains { Vec::new() } else { effects },
                    model: self.categorical_model,
                    order: if ordinal { self.order.clone() } else { Vec::new() },
                    reference: if ordinal { None } else { self.reference.clone() },
                }),
                format!("{model} logistic regression of {y} ({n} categories) on {described}"),
            );
        }
        let spec = LinearModelSpec {
            response,
            predictors: chosen.to_vec(),
            effects: if mains { Vec::new() } else { effects.clone() },
            splines: Vec::new(),
        };
        if self.best_subsets {
            let forced = self.forced.iter().map(|e| e.iter().map(|&(c, p)| (position(c), p)).collect()).collect();
            // Forced effects are matched against the candidates' effects,
            // so the candidates are always listed explicitly.
            let spec = LinearModelSpec { effects, ..spec };
            return kit::ready(
                ReportRequest::BestSubsets(BestSubsetsRequest {
                    candidates: spec,
                    forced,
                    heredity: self.heredity,
                    models_per_size: self.models_per_size,
                    max_size: self.max_size,
                    criterion: self.subset_criterion,
                }),
                format!("Best subsets for {y} among {described}"),
            );
        }
        if self.stepwise {
            let criterion = self.step_criterion();
            return kit::ready(
                ReportRequest::Stepwise(StepwiseRequest {
                    candidates: spec,
                    direction: self.direction,
                    criterion,
                    heredity: self.heredity,
                }),
                format!("{} stepwise selection for {y} among {described}", self.direction.label()),
            );
        }
        let splines = crate::model_terms_ui::by_position(&self.splines, chosen);
        if let Some((c, _)) = self.splines.iter().find(|(c, _)| {
            chosen.contains(c) && self.effects.iter().any(|e| e.iter().any(|(x, _)| x == c) && *e != [(*c, 1)])
        }) {
            return Err(format!("{} is a spline: take it out of interactions and powers", kit::name(table, *c)));
        }
        let n_splines = splines.iter().flatten().count();
        let described = if n_splines > 0 {
            format!("{described}, {}", kit::count(n_splines, "spline", "splines"))
        } else {
            described
        };
        kit::ready(
            ReportRequest::Linear(LinearModelSpec { splines, ..spec }),
            format!("Linear model of {y} on {described}"),
        )
    }
}

impl ModelDialog {
    /// The predictors of `chosen` that some selected effect uses.
    fn in_effects(&self, chosen: &[usize]) -> Vec<usize> {
        chosen.iter().copied().filter(|&c| self.effects.iter().any(|e| e.iter().any(|&(x, _)| x == c))).collect()
    }

    /// The criterion chosen for a stepwise search.
    fn step_criterion(&self) -> StepCriterion {
        match self.criterion {
            0 => StepCriterion::PValue { enter: self.enter, remove: self.remove },
            1 => StepCriterion::Aic,
            _ => StepCriterion::Bic,
        }
    }

    /// A stepwise search for each of several responses, as a
    /// several-responses request, or what is missing.
    fn build_several(&self, table: &DataTable, chosen: &[usize]) -> Readiness<ReportRequest> {
        let responses: Vec<usize> = kit::chosen_columns(&self.responses)
            .into_iter()
            .filter(|&i| kit::is_continuous(table, i) && !chosen.contains(&i))
            .collect();
        if responses.is_empty() {
            return Err("Choose at least one response".into());
        }
        if chosen.is_empty() {
            return Err("Choose at least one predictor".into());
        }
        if self.effects.is_empty() {
            return Err("Choose at least one candidate effect".into());
        }
        let used = self.in_effects(chosen);
        let chosen = used.as_slice();
        let position = |c: usize| chosen.iter().position(|&x| x == c).unwrap_or(0);
        let mut candidates: Vec<Vec<(usize, u8)>> =
            self.effects.iter().map(|e| e.iter().map(|&(c, p)| (position(c), p)).collect()).collect();
        candidates.sort_by_key(|e| (e.iter().map(|f| u32::from(f.1)).sum::<u32>(), e.clone()));
        // What the edited report set up in the report itself stays while it fits.
        let kept = self.kept_several.as_ref();
        let same_responses = kept.is_some_and(|k| k.responses == responses);
        let same_factors = kept.is_some_and(|k| k.predictors == chosen);
        let summary = format!(
            "{} stepwise selection for each of {} among {}",
            self.direction.label(),
            kit::count(responses.len(), "response", "responses"),
            effects_summary(&self.effects)
        );
        let request = MultipleResponsesRequest {
            goals: kept.filter(|_| same_responses).map(|k| k.goals.clone()).unwrap_or_default(),
            settings: kept.filter(|_| same_factors).map(|k| k.settings.clone()).unwrap_or_default(),
            simulation: kept.filter(|_| same_responses && same_factors).and_then(|k| k.simulation.clone()),
            constraints: kept.filter(|_| same_factors).map(|k| k.constraints.clone()).unwrap_or_default(),
            responses,
            predictors: chosen.to_vec(),
            form: kept.map_or(ModelForm::MainEffects, |k| k.form),
            selection: Some(TermSelection {
                direction: self.direction,
                criterion: self.step_criterion(),
                heredity: self.heredity,
                candidates,
            }),
        };
        kit::ready(ReportRequest::MultipleResponses(request), summary)
    }

    /// The MANOVA request, or what is missing.
    fn build_manova(&self, table: &DataTable, chosen: &[usize]) -> Readiness<ReportRequest> {
        let responses: Vec<usize> = kit::chosen_columns(&self.responses)
            .into_iter()
            .filter(|&i| kit::is_continuous(table, i) && !chosen.contains(&i))
            .collect();
        if responses.len() < 2 {
            return Err("Choose at least two responses".into());
        }
        if chosen.is_empty() {
            return Err("Choose at least one predictor".into());
        }
        if self.effects.is_empty() {
            return Err("Choose at least one model effect".into());
        }
        let used = self.in_effects(chosen);
        let chosen = used.as_slice();
        let position = |c: usize| chosen.iter().position(|&x| x == c).unwrap_or(0);
        let mut effects: Vec<Vec<(usize, u8)>> =
            self.effects.iter().map(|e| e.iter().map(|&(c, p)| (position(c), p)).collect()).collect();
        effects.sort_by_key(|e| (e.iter().map(|f| u32::from(f.1)).sum::<u32>(), e.clone()));
        let mains = effects.len() == chosen.len() && effects.iter().all(|e| matches!(e.as_slice(), [(_, 1)]));
        let request = ManovaRequest {
            effects: if mains { Vec::new() } else { effects },
            statistic: self.statistic,
            contrasts: self.contrasts.clone(),
            plot_factor: self.plot_factor,
            ..ManovaRequest::new(responses.clone(), chosen.to_vec())
        };
        let mut summary = format!(
            "MANOVA of {} on {}",
            kit::count(responses.len(), "response", "responses"),
            effects_summary(&self.effects)
        );
        if !self.contrasts.is_empty() {
            summary.push_str(&format!(", {}", kit::count(self.contrasts.len(), "contrast", "contrasts")));
        }
        kit::ready(ReportRequest::Manova(request), summary)
    }
}

impl Remember for ModelDialog {
    fn memory_key(&self) -> String {
        self.title().to_owned()
    }

    fn set_edit(&mut self, report: Option<u64>) {
        self.edit = report;
    }

    fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        self.predictors = kit::mask(table.n_cols(), columns);
    }
}
