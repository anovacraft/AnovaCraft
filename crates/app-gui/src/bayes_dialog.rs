//! Dialogs for the Bayesian analyses (Compare ▸ Bayesian and Model ▸
//! Bayesian): estimation of a mean, proportion or rate with conjugate
//! priors, Bayes factor tests, the Bayesian linear model or ANOVA, and the
//! Bayesian regression or hierarchical model sampled by NUTS
//! ([`app_model::bayes_analysis`], [`app_model::bayes_models`]).
//!
//! Every report runs through [`app_model::ComparisonReport`], so its
//! settings are also edited in place from the report's options menu
//! ([`report_settings`]).

use crate::dialog_kit::ComboExt as _;
use app_model::bayes_analysis::sampling_label;
use app_model::bayes_models::{FAMILIES, SCALE_PRIORS, family_label, scale_prior_label};
use app_model::hierarchy::set_effect;
use app_model::{
    BayesEstimationRequest, BayesFactorRequest, BayesLinearRequest, BayesModelRequest, BayesPrior, BayesRequest,
    BayesTestKind, EstimatedParameter, RandomEffectSpec, SavedReport,
};
use eframe::egui::{self, RichText};
use stat_bayes::bayes_factor::{Direction, Sampling};
use stat_bayes::glm::{CoefficientPrior, GlmFamily, Priors};
use stat_bayes::linear::{ContinuousPrior, ModelPrior};
use stat_bayes::univariate::Contrast;
use stat_data::DataTable;

use crate::dialog_kit::{self as kit, Dialog, Outcome, Readiness, Remember};
use crate::report_view::ReportRequest;

/// Which Bayesian analysis the dialog sets up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BayesTool {
    /// Posterior of a mean, proportion or rate (one group, two groups, paired).
    Estimation,
    /// Bayes factor t, correlation and contingency-table tests.
    Factor,
    /// Linear model or ANOVA with model comparison.
    Linear,
    /// Generalized linear or hierarchical model by MCMC.
    Model,
}

/// The menu entries of the Bayesian submenu: label, hover text and tool.
pub const MENU: [(&str, &str, BayesTool); 4] = [
    (
        "Bayesian estimation…",
        "Posterior distribution of a mean, proportion or event rate, or of the difference between two groups: \
         credible intervals, probabilities of exceeding a value and a region of practical equivalence",
        BayesTool::Estimation,
    ),
    (
        "Bayes factor tests…",
        "Evidence for and against no effect: one-sample, paired and two-sample t tests, correlation and \
         contingency tables, with prior robustness plots",
        BayesTool::Factor,
    ),
    (
        "Bayesian linear model or ANOVA…",
        "Every model that keeps heredity compared by Bayes factors; inclusion probabilities and model-averaged \
         estimates of the effects",
        BayesTool::Linear,
    ),
    (
        "Bayesian regression or hierarchical model…",
        "Normal, logistic, Poisson or negative binomial regression with group effects, sampled by MCMC; \
         convergence diagnostics, trace plots, LOO and a posterior predictive check",
        BayesTool::Model,
    ),
];

/// One-group, two-group or paired estimation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Design {
    One,
    Two,
    Paired,
}

/// The Bayes factor test chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Test {
    OneSample,
    Paired,
    TwoSample,
    Correlation,
    Contingency,
}

impl Test {
    const ALL: [(Self, &'static str); 5] = [
        (Self::OneSample, "One-sample t"),
        (Self::Paired, "Paired t"),
        (Self::TwoSample, "Two-sample t"),
        (Self::Correlation, "Correlation"),
        (Self::Contingency, "Contingency table"),
    ];

    fn default_scale(self) -> f64 {
        match self {
            Self::OneSample | Self::Paired | Self::TwoSample => app_model::bayes_analysis::DEFAULT_T_SCALE,
            Self::Correlation | Self::Contingency => 1.0,
        }
    }
}

/// One group-effects term being edited.
#[derive(Debug, Clone, PartialEq)]
struct TermDraft {
    group: Option<usize>,
    within: Option<usize>,
    deeper: Vec<usize>,
    intercept: bool,
    slopes: Vec<usize>,
}

impl Default for TermDraft {
    fn default() -> Self {
        Self { group: None, within: None, deeper: Vec::new(), intercept: true, slopes: Vec::new() }
    }
}

/// Dialog for the Bayesian analyses.
#[derive(Clone)]
pub struct BayesDialog {
    edit: Option<u64>,
    tool: BayesTool,
    level: f64,
    // Shared column roles.
    response: Option<usize>,
    second: Option<usize>,
    group: Option<usize>,
    frequency: Option<usize>,
    // Estimation.
    parameter: EstimatedParameter,
    design: Design,
    denominator: Option<usize>,
    event: Option<String>,
    prior: BayesPrior,
    equal_variances: bool,
    contrast: Contrast,
    reference: Option<f64>,
    rope: Option<(f64, f64)>,
    // Bayes factors.
    test: Test,
    null: f64,
    scale: f64,
    direction: Direction,
    sampling: Sampling,
    // Models.
    predictors: Vec<bool>,
    effects: Vec<Vec<(usize, u8)>>,
    linear: BayesLinearRequest,
    family: GlmFamily,
    use_trials: bool,
    trials: Option<usize>,
    offset: Option<usize>,
    known_sd: Option<usize>,
    terms: Vec<TermDraft>,
    model: BayesModelRequest,
}

impl BayesDialog {
    /// A dialog for `tool`.
    pub fn new(tool: BayesTool, table: &DataTable) -> Self {
        let estimation = BayesEstimationRequest::new(EstimatedParameter::Mean, 0);
        Self {
            edit: None,
            tool,
            level: 0.95,
            response: None,
            second: None,
            group: None,
            frequency: None,
            parameter: EstimatedParameter::Mean,
            design: Design::One,
            denominator: None,
            event: None,
            prior: estimation.prior,
            equal_variances: estimation.equal_variances,
            contrast: estimation.contrast,
            reference: None,
            rope: None,
            test: Test::TwoSample,
            null: 0.0,
            scale: Test::TwoSample.default_scale(),
            direction: Direction::TwoSided,
            sampling: Sampling::JointMultinomial,
            predictors: vec![false; table.n_cols()],
            effects: Vec::new(),
            linear: BayesLinearRequest::new(0, Vec::new()),
            family: GlmFamily::Normal,
            use_trials: false,
            trials: None,
            offset: None,
            known_sd: None,
            terms: Vec::new(),
            model: BayesModelRequest::new(0, Vec::new(), GlmFamily::Normal),
        }
    }

    /// The dialog filled from a saved Bayesian report.
    pub fn from_saved(saved: &SavedReport, table: &DataTable) -> Option<Self> {
        let SavedReport::Bayesian { request } = saved else { return None };
        let valid = |c: usize| kit::valid(table, c);
        let d = match request {
            BayesRequest::Estimation(r) => Self {
                level: r.level,
                response: valid(r.response),
                second: r.second.and_then(valid),
                group: r.group.and_then(valid),
                frequency: r.frequency.and_then(valid),
                parameter: r.parameter,
                design: if r.group.is_some() {
                    Design::Two
                } else if r.second.is_some() {
                    Design::Paired
                } else {
                    Design::One
                },
                denominator: r.denominator.and_then(valid),
                event: r.event.clone(),
                prior: r.prior,
                equal_variances: r.equal_variances,
                contrast: r.contrast,
                reference: r.reference,
                rope: r.rope,
                ..Self::new(BayesTool::Estimation, table)
            },
            BayesRequest::Factor(r) => {
                let fresh = Self::new(BayesTool::Factor, table);
                let base = Self { level: r.level, scale: r.scale, direction: r.direction, ..fresh };
                match &r.test {
                    BayesTestKind::OneSample { column, null } => {
                        Self { test: Test::OneSample, response: valid(*column), null: *null, ..base }
                    }
                    BayesTestKind::Paired { first, second } => {
                        Self { test: Test::Paired, response: valid(*first), second: valid(*second), ..base }
                    }
                    BayesTestKind::TwoSample { response, group } => {
                        Self { test: Test::TwoSample, response: valid(*response), group: valid(*group), ..base }
                    }
                    BayesTestKind::Correlation { x, y } => {
                        Self { test: Test::Correlation, response: valid(*x), second: valid(*y), ..base }
                    }
                    BayesTestKind::Contingency { rows, columns, frequency, sampling } => Self {
                        test: Test::Contingency,
                        response: valid(*rows),
                        second: valid(*columns),
                        frequency: frequency.and_then(valid),
                        sampling: *sampling,
                        ..base
                    },
                }
            }
            BayesRequest::Linear(r) => {
                let predictors: Vec<usize> = r.predictors.iter().copied().filter(|&c| c < table.n_cols()).collect();
                Self {
                    level: r.level,
                    response: valid(r.response),
                    predictors: kit::mask(table.n_cols(), &predictors),
                    effects: column_effects(&r.predictors, &r.effects, table),
                    linear: r.clone(),
                    ..Self::new(BayesTool::Linear, table)
                }
            }
            BayesRequest::Model(r) => {
                let predictors: Vec<usize> = r.predictors.iter().copied().filter(|&c| c < table.n_cols()).collect();
                Self {
                    level: r.level,
                    response: valid(r.response),
                    predictors: kit::mask(table.n_cols(), &predictors),
                    effects: column_effects(&r.predictors, &r.effects, table),
                    family: r.family,
                    use_trials: r.trials.is_some(),
                    trials: r.trials.and_then(valid),
                    offset: r.offset.and_then(valid),
                    known_sd: r.known_sd.and_then(valid),
                    terms: r
                        .random
                        .iter()
                        .map(|t| TermDraft {
                            group: valid(t.group),
                            within: t.within.first().copied().and_then(valid),
                            deeper: t.within.iter().skip(1).copied().filter(|&c| c < table.n_cols()).collect(),
                            intercept: t.intercept,
                            slopes: t.slopes.iter().copied().filter(|&c| c < table.n_cols()).collect(),
                        })
                        .collect(),
                    model: (**r).clone(),
                    ..Self::new(BayesTool::Model, table)
                }
            }
        };
        Some(d)
    }

    fn title(&self) -> &'static str {
        match self.tool {
            BayesTool::Estimation => "Bayesian estimation",
            BayesTool::Factor => "Bayes factor tests",
            BayesTool::Linear => "Bayesian linear model or ANOVA",
            BayesTool::Model => "Bayesian regression or hierarchical model",
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<Box<ReportRequest>> {
        let dialog = Dialog::new(self.title()).editing(self.edit);
        let outcome = dialog.show(ctx, |ui| {
            ui.set_min_width(kit::em(ui) * 36.0);
            let ready = match self.tool {
                BayesTool::Estimation => self.estimation(ui, table),
                BayesTool::Factor => self.factor(ui, table),
                BayesTool::Linear => self.linear_model(ui, table),
                BayesTool::Model => self.glm(ui, table),
            };
            ready.map(|r| kit::Ready {
                output: Box::new(ReportRequest::Comparison(SavedReport::Bayesian { request: r.output })),
                summary: r.summary,
            })
        });
        match outcome {
            Outcome::Reset => {
                *self = Self { edit: self.edit, ..Self::new(self.tool, table) };
                Outcome::Open
            }
            outcome => outcome,
        }
    }

    fn estimation(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<BayesRequest> {
        let numeric = |i: usize| kit::is_numeric(table, i);
        ui.horizontal(|ui| {
            ui.label("Estimate:");
            for (p, label) in [
                (EstimatedParameter::Mean, "Mean"),
                (EstimatedParameter::Proportion, "Proportion"),
                (EstimatedParameter::Rate, "Event rate"),
            ] {
                ui.radio_value(&mut self.parameter, p, label);
            }
        });
        let mean = self.parameter == EstimatedParameter::Mean;
        if !mean && self.design == Design::Paired {
            self.design = Design::One;
        }
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.design, Design::One, "One group");
            ui.radio_value(&mut self.design, Design::Two, "Two groups");
            if mean {
                ui.radio_value(&mut self.design, Design::Paired, "Paired columns");
            }
        });
        let labelled = self.parameter == EstimatedParameter::Proportion && self.denominator.is_none();
        let allow = |i: usize| labelled || numeric(i);
        if self.response.is_some_and(|r| !allow(r)) {
            self.response = None;
        }
        let response_label = match (self.parameter, self.design) {
            (EstimatedParameter::Mean, Design::Paired) => "First column:",
            (EstimatedParameter::Mean, _) => "Values:",
            (EstimatedParameter::Proportion, _) if labelled => "Outcome:",
            (EstimatedParameter::Proportion, _) => "Successes:",
            (EstimatedParameter::Rate, _) => "Event count:",
        };
        egui::Grid::new("bayes_est_cols").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label(response_label);
            kit::column_choice(ui, "be_response", table, &mut self.response, allow, None);
            ui.end_row();
            match self.design {
                Design::Two => {
                    ui.label("Group (two levels):");
                    kit::column_choice(ui, "be_group", table, &mut self.group, |_| true, None);
                    ui.end_row();
                }
                Design::Paired => {
                    ui.label("Second column:");
                    kit::column_choice(ui, "be_second", table, &mut self.second, numeric, None);
                    ui.end_row();
                }
                Design::One => {}
            }
            match self.parameter {
                EstimatedParameter::Mean => {}
                EstimatedParameter::Proportion => {
                    ui.label("Trials per row:");
                    kit::column_choice(
                        ui,
                        "be_trials",
                        table,
                        &mut self.denominator,
                        numeric,
                        Some("None: one trial per row"),
                    );
                    ui.end_row();
                    if self.denominator.is_none() {
                        ui.label("Event:");
                        let values = self.response.map(|c| distinct(table, c)).unwrap_or_default();
                        if self.event.as_ref().is_some_and(|e| !values.contains(e)) {
                            self.event = None;
                        }
                        let shown = self.event.clone().unwrap_or_else(|| "Second value in sorted order".into());
                        egui::ComboBox::from_id_salt("be_event").selected_text(shown).show_list(ui, |ui| {
                            ui.selectable_value(&mut self.event, None, "Second value in sorted order");
                            for v in values {
                                ui.selectable_value(&mut self.event, Some(v.clone()), v);
                            }
                        });
                        ui.end_row();
                        ui.label("Frequency:");
                        kit::column_choice(
                            ui,
                            "be_freq",
                            table,
                            &mut self.frequency,
                            numeric,
                            Some("None: one per row"),
                        );
                        ui.end_row();
                    }
                }
                EstimatedParameter::Rate => {
                    ui.label("Exposure per row:");
                    kit::column_choice(
                        ui,
                        "be_exposure",
                        table,
                        &mut self.denominator,
                        numeric,
                        Some("None: one unit per row"),
                    );
                    ui.end_row();
                }
            }
        });
        if self.parameter == EstimatedParameter::Mean {
            self.denominator = None;
        }
        prior_ui(ui, self.parameter, &mut self.prior);
        if self.design == Design::Two {
            match self.parameter {
                EstimatedParameter::Mean => {
                    ui.checkbox(&mut self.equal_variances, "Assume equal variances");
                }
                EstimatedParameter::Proportion => contrast_ui(ui, &mut self.contrast),
                EstimatedParameter::Rate => {}
            }
        }
        kit::more_options(ui, |ui| {
            if self.design != Design::Two {
                reference_ui(ui, self.parameter, &mut self.reference);
            }
            rope_ui(ui, &mut self.rope);
            level_ui(ui, &mut self.level);
        });
        let Some(response) = self.response else { return Err("Choose the response column".into()) };
        let mut request = BayesEstimationRequest {
            prior: self.prior,
            equal_variances: self.equal_variances,
            contrast: if self.parameter == EstimatedParameter::Proportion {
                self.contrast
            } else {
                Contrast::Difference
            },
            reference: self.reference,
            rope: self.rope,
            level: self.level,
            denominator: self.denominator,
            ..BayesEstimationRequest::new(self.parameter, response)
        };
        match self.design {
            Design::One => {}
            Design::Two => request.group = Some(self.group.ok_or("Choose the group column")?),
            Design::Paired => request.second = Some(self.second.ok_or("Choose the second column")?),
        }
        if labelled {
            request.event = self.event.clone();
            request.frequency = self.frequency;
        }
        if !kit::all_different(&request.columns().into_iter().map(Some).collect::<Vec<_>>()) {
            return Err("A column is chosen twice".into());
        }
        let what = match self.parameter {
            EstimatedParameter::Mean => "mean",
            EstimatedParameter::Proportion => "proportion",
            EstimatedParameter::Rate => "rate",
        };
        let summary = match self.design {
            Design::One => format!("Posterior {what} of {}", kit::name(table, response)),
            Design::Two => format!(
                "Posterior {what} of {} in each group of {}",
                kit::name(table, response),
                kit::name(table, request.group.unwrap_or(0))
            ),
            Design::Paired => format!(
                "Posterior mean difference {} − {}",
                kit::name(table, response),
                kit::name(table, request.second.unwrap_or(0))
            ),
        };
        kit::ready(BayesRequest::Estimation(request), summary)
    }

    fn factor(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<BayesRequest> {
        let numeric = |i: usize| kit::is_numeric(table, i);
        let any = |_: usize| true;
        ui.horizontal_wrapped(|ui| {
            ui.label("Test:");
            for (t, label) in Test::ALL {
                if ui.radio_value(&mut self.test, t, label).changed() {
                    self.scale = t.default_scale();
                }
            }
        });
        let (first, second): (&str, Option<&str>) = match self.test {
            Test::OneSample => ("Values:", None),
            Test::Paired => ("First column:", Some("Second column:")),
            Test::TwoSample => ("Values:", Some("Group (two levels):")),
            Test::Correlation => ("X:", Some("Y:")),
            Test::Contingency => ("Rows:", Some("Columns:")),
        };
        let first_allow = |i: usize| self.test == Test::Contingency || numeric(i);
        if self.response.is_some_and(|r| !first_allow(r)) {
            self.response = None;
        }
        let test = self.test;
        egui::Grid::new("bayes_bf_cols").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label(first);
            kit::column_choice(ui, "bf_first", table, &mut self.response, first_allow, None);
            ui.end_row();
            if let Some(label) = second {
                ui.label(label);
                if test == Test::TwoSample {
                    kit::column_choice(ui, "bf_group", table, &mut self.group, any, None);
                } else {
                    let allow = |i: usize| test == Test::Contingency || numeric(i);
                    kit::column_choice(ui, "bf_second", table, &mut self.second, allow, None);
                }
                ui.end_row();
            }
            if test == Test::Contingency {
                ui.label("Frequency:");
                kit::column_choice(ui, "bf_freq", table, &mut self.frequency, numeric, Some("None: one per row"));
                ui.end_row();
            }
        });
        if test == Test::OneSample {
            ui.horizontal(|ui| {
                ui.label("Hypothesized mean:");
                ui.add(egui::DragValue::new(&mut self.null).speed(0.1));
            });
        }
        if test == Test::Contingency {
            sampling_ui(ui, &mut self.sampling);
        } else {
            direction_ui(ui, &mut self.direction, test == Test::Correlation);
        }
        kit::more_options(ui, |ui| {
            scale_ui(ui, test, &mut self.scale);
            level_ui(ui, &mut self.level);
        });
        let Some(a) = self.response else { return Err("Choose the first column".into()) };
        let b = if test == Test::TwoSample { self.group } else { self.second };
        let need_b =
            || b.ok_or_else(|| format!("Choose the {}", second.unwrap_or("").trim_end_matches(':').to_lowercase()));
        let kind = match test {
            Test::OneSample => BayesTestKind::OneSample { column: a, null: self.null },
            Test::Paired => BayesTestKind::Paired { first: a, second: need_b()? },
            Test::TwoSample => BayesTestKind::TwoSample { response: a, group: need_b()? },
            Test::Correlation => BayesTestKind::Correlation { x: a, y: need_b()? },
            Test::Contingency => BayesTestKind::Contingency {
                rows: a,
                columns: need_b()?,
                frequency: self.frequency,
                sampling: self.sampling,
            },
        };
        let request = BayesFactorRequest {
            scale: self.scale,
            direction: self.direction,
            level: self.level,
            ..BayesFactorRequest::new(kind)
        };
        if !kit::all_different(&request.columns().into_iter().map(Some).collect::<Vec<_>>()) {
            return Err("A column is chosen twice".into());
        }
        let name = |i: usize| kit::name(table, i).to_owned();
        let summary = match &request.test {
            BayesTestKind::OneSample { column, null } => format!("Bayes factor: mean of {} = {null}", name(*column)),
            BayesTestKind::Paired { first, second } => {
                format!("Bayes factor: {} − {} = 0", name(*first), name(*second))
            }
            BayesTestKind::TwoSample { response, group } => {
                format!("Bayes factor: {} equal across {}", name(*response), name(*group))
            }
            BayesTestKind::Correlation { x, y } => format!("Bayes factor: {} and {} uncorrelated", name(*x), name(*y)),
            BayesTestKind::Contingency { rows, columns, .. } => {
                format!("Bayes factor: {} independent of {}", name(*rows), name(*columns))
            }
        };
        kit::ready(BayesRequest::Factor(request), summary)
    }

    /// Predictors and their effects; the chosen predictor columns.
    fn effects_ui(&mut self, ui: &mut egui::Ui, table: &DataTable, id: &str) -> Vec<usize> {
        let n = table.n_cols();
        let response = self.response;
        let continuous = |i: usize| kit::is_continuous(table, i);
        kit::column_list(ui, (id, "predictors"), table, &mut self.predictors, |i| Some(i) != response);
        let chosen: Vec<usize> =
            kit::chosen_columns(&self.predictors).into_iter().filter(|&i| i < n && Some(i) != response).collect();
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
            if ui.small_button("Main effects").clicked() {
                self.effects = chosen.iter().map(|&c| vec![(c, 1)]).collect();
            }
            if ui.small_button("+ Two-factor interactions").clicked() {
                for e in candidates.iter().filter(|e| e.len() == 2) {
                    set_effect(&mut self.effects, e, true, true);
                }
            }
        });
        let label = |e: &[(usize, u8)]| {
            e.iter()
                .map(|&(c, p)| {
                    let name = kit::name(table, c);
                    if p == 1 {
                        name.to_owned()
                    } else {
                        format!("{name}{}", stat_core::text::superscript(&p.to_string()))
                    }
                })
                .collect::<Vec<_>>()
                .join(" × ")
        };
        egui::ScrollArea::vertical().id_salt((id, "effects")).max_height(kit::em(ui) * 6.0).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                for e in &candidates {
                    let mut on = self.effects.contains(e);
                    if ui.checkbox(&mut on, label(e)).changed() {
                        set_effect(&mut self.effects, e, on, true);
                    }
                }
            });
        });
        kit::note(ui, "An interaction or power brings in the effects it is built from.");
        chosen
    }

    /// Effects as positions in `chosen`, or empty for main effects alone.
    fn positions(&self, chosen: &[usize]) -> Vec<Vec<(usize, u8)>> {
        let position = |c: usize| chosen.iter().position(|&x| x == c).unwrap_or(0);
        let mut effects: Vec<Vec<(usize, u8)>> =
            self.effects.iter().map(|e| e.iter().map(|&(c, p)| (position(c), p)).collect()).collect();
        effects.sort_by_key(|e| (e.iter().map(|f| u32::from(f.1)).sum::<u32>(), e.clone()));
        let mains = effects.len() == chosen.len() && effects.iter().all(|e| matches!(e.as_slice(), [(_, 1)]));
        if mains { Vec::new() } else { effects }
    }

    fn linear_model(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<BayesRequest> {
        let numeric = |i: usize| kit::is_numeric(table, i);
        ui.horizontal(|ui| {
            ui.label("Response:");
            kit::column_choice(ui, "bl_response", table, &mut self.response, numeric, None);
        });
        ui.separator();
        ui.label(RichText::new("Effects").strong());
        let chosen = self.effects_ui(ui, table, "bl");
        let r = &mut self.linear;
        ui.checkbox(&mut r.full_model_only, "Fit only the full model (no model comparison)");
        kit::more_options(ui, |ui| {
            linear_prior_ui(ui, r);
            level_ui(ui, &mut self.level);
            ui.horizontal(|ui| {
                ui.label("Draws per model:")
                    .on_hover_text("Importance draws for models with several categorical effects");
                ui.add(egui::DragValue::new(&mut r.draws).range(1000..=200_000).speed(100));
                ui.label("Seed:");
                ui.add(kit::seed_value(&mut r.seed));
            });
        });
        let Some(response) = self.response else { return Err("Choose a response".into()) };
        if chosen.is_empty() {
            return Err("Choose at least one predictor".into());
        }
        let request = BayesLinearRequest {
            response,
            predictors: chosen.clone(),
            effects: self.positions(&chosen),
            level: self.level,
            ..self.linear.clone()
        };
        let summary = format!(
            "Bayesian model comparison for {} on {}",
            kit::name(table, response),
            kit::count(self.effects.len(), "effect", "effects")
        );
        kit::ready(BayesRequest::Linear(request), summary)
    }

    fn glm(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<BayesRequest> {
        let n = table.n_cols();
        let numeric = |i: usize| kit::is_numeric(table, i);
        let continuous = |i: usize| kit::is_continuous(table, i);
        ui.horizontal_wrapped(|ui| {
            ui.label("Response distribution:");
            for f in FAMILIES {
                ui.radio_value(&mut self.family, f, family_label(f));
            }
        });
        let two_values = self.family == GlmFamily::Binomial && !self.use_trials;
        let allow_response = |i: usize| two_values || numeric(i);
        if self.response.is_some_and(|r| !allow_response(r)) {
            self.response = None;
        }
        ui.horizontal(|ui| {
            ui.label("Response:");
            kit::column_choice(ui, "bg_response", table, &mut self.response, allow_response, None);
        });
        let response = self.response;
        if self.family == GlmFamily::Binomial {
            ui.horizontal(|ui| {
                ui.radio_value(&mut self.use_trials, false, "Two values (the second in sorted order is the event)");
                ui.radio_value(&mut self.use_trials, true, "Successes out of trials in:");
                ui.add_enabled_ui(self.use_trials, |ui| {
                    let allow = |i: usize| numeric(i) && Some(i) != response;
                    kit::column_choice(ui, "bg_trials", table, &mut self.trials, allow, None);
                });
            });
        }
        ui.separator();
        ui.label(RichText::new("Effects").strong());
        let chosen = self.effects_ui(ui, table, "bg");
        if chosen.is_empty() {
            kit::note(ui, "No predictors: the fixed part is the intercept alone.");
        }
        ui.separator();
        ui.label(RichText::new("Group effects").strong()).on_hover_text(
            "Effects that vary from level to level of a grouping column, such as schools, sites or patients",
        );
        let mut remove = None;
        for (k, t) in self.terms.iter_mut().enumerate() {
            ui.horizontal_wrapped(|ui| {
                ui.label(format!("{}.", k + 1));
                kit::column_choice(ui, ("bg_group", k), table, &mut t.group, |i| Some(i) != response, None);
                ui.label("within").on_hover_text("Nest the grouping within another column");
                let group = t.group;
                let allow = |i: usize| Some(i) != response && Some(i) != group;
                kit::column_choice(ui, ("bg_within", k), table, &mut t.within, allow, Some("(not nested)"));
                t.within = t.within.filter(|w| Some(*w) != t.group);
                ui.checkbox(&mut t.intercept, "Intercept");
                let shown = if t.slopes.is_empty() {
                    "Slopes: none".to_owned()
                } else {
                    format!("Slopes: {}", t.slopes.iter().map(|&c| kit::name(table, c)).collect::<Vec<_>>().join(", "))
                };
                ui.menu_button(shown, |ui| {
                    let mut mask = kit::mask(n, &t.slopes);
                    let allow = |i: usize| continuous(i) && Some(i) != response;
                    if kit::column_list(ui, ("bg_slopes", k), table, &mut mask, allow) {
                        t.slopes = kit::chosen_columns(&mask);
                    }
                })
                .response
                .on_hover_text("Continuous columns whose slope varies by level");
                if ui.small_button("×").on_hover_text("Remove this group effect").clicked() {
                    remove = Some(k);
                }
            });
        }
        if let Some(k) = remove {
            self.terms.remove(k);
        }
        if ui.small_button("+ Add group effect").clicked() {
            self.terms.push(TermDraft::default());
        }
        if self.family == GlmFamily::Normal {
            ui.horizontal(|ui| {
                ui.label("Known SD per row (optional):")
                    .on_hover_text("Meta-analysis: each row's standard error; no residual SD is estimated");
                let allow = |i: usize| numeric(i) && Some(i) != response;
                kit::column_choice(ui, "bg_known_sd", table, &mut self.known_sd, allow, Some("None: estimate the SD"));
            });
        } else {
            ui.horizontal(|ui| {
                ui.label("Offset (optional):")
                    .on_hover_text("Added to the linear predictor with coefficient 1, e.g. the log of exposure time");
                let allow = |i: usize| numeric(i) && Some(i) != response;
                kit::column_choice(ui, "bg_offset", table, &mut self.offset, allow, Some("None"));
            });
        }
        let m = &mut self.model;
        kit::more_options(ui, |ui| {
            priors_ui(ui, &mut m.priors);
            ui.separator();
            sampler_ui(ui, m);
            level_ui(ui, &mut self.level);
        });
        let response = response.ok_or("Choose a response")?;
        let mut random = Vec::with_capacity(self.terms.len());
        let single = self.terms.len() == 1;
        for (k, t) in self.terms.iter().enumerate() {
            let which = if single { String::new() } else { format!(" for group effect {}", k + 1) };
            let group = t.group.ok_or_else(|| format!("Choose a grouping column{which}"))?;
            if !t.intercept && t.slopes.is_empty() {
                return Err(format!("Choose an intercept or at least one slope{which}"));
            }
            random.push(RandomEffectSpec {
                group,
                within: t.within.into_iter().chain(t.deeper.iter().copied()).collect(),
                intercept: t.intercept,
                slopes: t.slopes.clone(),
                correlated: false,
            });
        }
        if random.iter().any(|r| r.group == response || r.within.contains(&response) || r.slopes.contains(&response)) {
            return Err("The response cannot also be a grouping, nesting or slope column".into());
        }
        let trials = if self.family == GlmFamily::Binomial && self.use_trials {
            Some(self.trials.filter(|&t| t != response).ok_or("Choose a trials column other than the response")?)
        } else {
            None
        };
        let offset = if self.family == GlmFamily::Normal { None } else { self.offset.filter(|&o| o != response) };
        let known_sd = if self.family == GlmFamily::Normal { self.known_sd.filter(|&s| s != response) } else { None };
        let request = BayesModelRequest {
            response,
            predictors: chosen.clone(),
            effects: self.positions(&chosen),
            random,
            family: self.family,
            trials,
            offset,
            known_sd,
            level: self.level,
            ..self.model.clone()
        };
        let family = family_label(self.family).split(" (").next().unwrap_or("").to_owned();
        let groups: Vec<&str> = request.random.iter().map(|r| kit::name(table, r.group)).collect();
        let mut summary = format!(
            "Bayesian {} model of {} on {}",
            family.to_lowercase(),
            kit::name(table, response),
            kit::count(self.effects.len(), "effect", "effects")
        );
        if !groups.is_empty() {
            summary.push_str(&format!(", group effects by {}", groups.join(", ")));
        }
        kit::ready(BayesRequest::Model(Box::new(request)), summary)
    }
}

impl Remember for BayesDialog {
    fn memory_key(&self) -> String {
        self.title().into()
    }

    fn set_edit(&mut self, report: Option<u64>) {
        self.edit = report;
    }

    fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        match self.tool {
            BayesTool::Linear | BayesTool::Model => self.predictors = kit::mask(table.n_cols(), columns),
            BayesTool::Estimation | BayesTool::Factor => self.response = columns.first().copied(),
        }
    }
}

/// Saved effects (positions in `predictors`) as column products.
fn column_effects(predictors: &[usize], effects: &[Vec<(usize, u8)>], table: &DataTable) -> Vec<Vec<(usize, u8)>> {
    let valid = |c: usize| kit::valid(table, c);
    if effects.is_empty() {
        return predictors.iter().copied().filter_map(valid).map(|c| vec![(c, 1)]).collect();
    }
    effects
        .iter()
        .filter_map(|e| e.iter().map(|&(k, p)| predictors.get(k).copied().and_then(valid).map(|c| (c, p))).collect())
        .collect()
}

fn distinct(table: &DataTable, c: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if let Some(col) = table.column(c) {
        for r in 0..table.n_rows() {
            let t = col.level_text(r);
            if !t.is_empty() && !out.contains(&t) {
                out.push(t);
            }
        }
    }
    out.sort();
    out
}

fn level_ui(ui: &mut egui::Ui, value: &mut f64) {
    ui.horizontal(|ui| {
        ui.label("Credible level:");
        ui.add(egui::DragValue::new(value).speed(0.005).range(0.5..=0.999));
    });
}

fn prior_ui(ui: &mut egui::Ui, parameter: EstimatedParameter, prior: &mut BayesPrior) {
    let (default, flat) = match parameter {
        EstimatedParameter::Mean => ("Noninformative (Jeffreys)", None),
        EstimatedParameter::Proportion => ("Jeffreys, Beta(½, ½)", Some("Uniform, Beta(1, 1)")),
        EstimatedParameter::Rate => ("Jeffreys, Gamma(½, 0)", Some("Flat, Gamma(1, 0)")),
    };
    if flat.is_none() && *prior == BayesPrior::Flat {
        *prior = BayesPrior::Default;
    }
    let informative = matches!(prior, BayesPrior::Informative { .. });
    ui.horizontal_wrapped(|ui| {
        ui.label("Prior:");
        if ui.radio(*prior == BayesPrior::Default, default).clicked() {
            *prior = BayesPrior::Default;
        }
        if let Some(flat) = flat
            && ui.radio(*prior == BayesPrior::Flat, flat).clicked()
        {
            *prior = BayesPrior::Flat;
        }
        if ui.radio(informative, "Informative").clicked() && !informative {
            *prior = match parameter {
                EstimatedParameter::Mean => BayesPrior::Informative { mean: 0.0, spread: 1.0 },
                EstimatedParameter::Proportion => BayesPrior::Informative { mean: 0.5, spread: 0.2 },
                EstimatedParameter::Rate => BayesPrior::Informative { mean: 1.0, spread: 1.0 },
            };
        }
    });
    if let BayesPrior::Informative { mean, spread } = prior {
        ui.horizontal(|ui| {
            ui.label("Prior mean:");
            let speed = if parameter == EstimatedParameter::Proportion { 0.01 } else { 0.1 };
            let mut value = egui::DragValue::new(mean).speed(speed);
            if parameter == EstimatedParameter::Proportion {
                value = value.range(0.001..=0.999);
            } else if parameter == EstimatedParameter::Rate {
                value = value.range(1e-6..=f64::INFINITY);
            }
            ui.add(value);
            if parameter == EstimatedParameter::Mean {
                ui.label("worth").on_hover_text("The prior counts as this many observations at the prior mean");
                ui.add(egui::DragValue::new(spread).speed(0.1).range(0.01..=1e6));
                ui.label("observations");
            } else {
                ui.label("Prior SD:");
                ui.add(egui::DragValue::new(spread).speed(0.01).range(1e-6..=f64::INFINITY));
            }
        });
    }
}

fn contrast_ui(ui: &mut egui::Ui, contrast: &mut Contrast) {
    ui.horizontal(|ui| {
        ui.label("Compare by:");
        ui.radio_value(contrast, Contrast::Difference, "Difference");
        ui.radio_value(contrast, Contrast::Ratio, "Ratio");
        ui.radio_value(contrast, Contrast::OddsRatio, "Odds ratio");
    });
}

fn reference_ui(ui: &mut egui::Ui, parameter: EstimatedParameter, reference: &mut Option<f64>) {
    let default = match parameter {
        EstimatedParameter::Mean => 0.0,
        EstimatedParameter::Proportion => 0.5,
        EstimatedParameter::Rate => 1.0,
    };
    let mut value = reference.unwrap_or(default);
    ui.horizontal(|ui| {
        ui.label("Reference value:").on_hover_text("The value the posterior probabilities compare against");
        if ui.add(egui::DragValue::new(&mut value).speed(0.01)).changed() {
            *reference = if (value - default).abs() < 1e-12 { None } else { Some(value) };
        }
    });
}

fn rope_ui(ui: &mut egui::Ui, rope: &mut Option<(f64, f64)>) {
    let mut on = rope.is_some();
    let (mut lo, mut hi) = rope.unwrap_or((-0.1, 0.1));
    ui.horizontal(|ui| {
        ui.checkbox(&mut on, "Region of practical equivalence from")
            .on_hover_text("Values this close to the reference count as no practical difference");
        ui.add_enabled(on, egui::DragValue::new(&mut lo).speed(0.01));
        ui.label("to");
        ui.add_enabled(on, egui::DragValue::new(&mut hi).speed(0.01));
    });
    if hi < lo {
        std::mem::swap(&mut lo, &mut hi);
    }
    *rope = on.then_some((lo, hi));
}

fn direction_ui(ui: &mut egui::Ui, direction: &mut Direction, correlation: bool) {
    let (greater, less) = if correlation { ("ρ > 0", "ρ < 0") } else { ("Effect > 0", "Effect < 0") };
    ui.horizontal(|ui| {
        ui.label("Alternative:");
        ui.radio_value(direction, Direction::TwoSided, "Two-sided");
        ui.radio_value(direction, Direction::Greater, greater);
        ui.radio_value(direction, Direction::Less, less);
    });
}

fn sampling_ui(ui: &mut egui::Ui, sampling: &mut Sampling) {
    ui.label("How the table was collected:");
    for s in [Sampling::Poisson, Sampling::JointMultinomial, Sampling::FixedRows, Sampling::FixedColumns] {
        ui.radio_value(sampling, s, sampling_label(s));
    }
}

fn scale_ui(ui: &mut egui::Ui, test: Test, scale: &mut f64) {
    let (label, hint) = match test {
        Test::OneSample | Test::Paired | Test::TwoSample => {
            ("Prior scale r:", "Scale of the Cauchy prior on the standardized effect size (√2⁄2 ≈ 0.707 is medium)")
        }
        Test::Correlation => ("Prior width κ:", "Width of the stretched beta prior on ρ (1 is uniform)"),
        Test::Contingency => {
            ("Prior concentration a:", "Dirichlet concentration of the cell probabilities (1 is uniform)")
        }
    };
    ui.horizontal(|ui| {
        ui.label(label).on_hover_text(hint);
        ui.add(egui::DragValue::new(scale).speed(0.01).range(0.01..=10.0));
    });
}

fn linear_prior_ui(ui: &mut egui::Ui, r: &mut BayesLinearRequest) {
    ui.horizontal(|ui| {
        ui.label("Continuous effects:");
        let zs = matches!(r.continuous_prior, ContinuousPrior::ZellnerSiow { .. });
        if ui.radio(zs, "Zellner–Siow, scale").clicked() && !zs {
            r.continuous_prior = ContinuousPrior::ZellnerSiow { r: app_model::bayes_models::DEFAULT_CONTINUOUS_SCALE };
        }
        if let ContinuousPrior::ZellnerSiow { r: s } = &mut r.continuous_prior {
            ui.add(egui::DragValue::new(s).speed(0.01).range(0.01..=10.0));
        }
        if ui.radio(!zs, "Hyper-g, a").clicked() && zs {
            r.continuous_prior = ContinuousPrior::HyperG { a: 3.0 };
        }
        if let ContinuousPrior::HyperG { a } = &mut r.continuous_prior {
            ui.add(egui::DragValue::new(a).speed(0.01).range(2.01..=4.0));
        }
    });
    ui.horizontal(|ui| {
        ui.label("Categorical effects scale:").on_hover_text("½ is medium");
        ui.add(egui::DragValue::new(&mut r.r_categorical).speed(0.01).range(0.01..=10.0));
    });
    ui.horizontal(|ui| {
        ui.label("Prior over models:");
        ui.radio_value(&mut r.model_prior, ModelPrior::Uniform, "Every model equally likely");
        ui.radio_value(&mut r.model_prior, ModelPrior::BetaBinomial, "Beta-binomial on the number of effects");
    });
}

fn priors_ui(ui: &mut egui::Ui, p: &mut Priors) {
    ui.label(RichText::new("Priors").strong());
    egui::Grid::new("bayes_priors").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
        ui.label("Coefficients:");
        ui.horizontal(|ui| {
            let t = matches!(p.coefficient, CoefficientPrior::StudentT(_));
            if ui.radio(p.coefficient == CoefficientPrior::Normal, "Normal").clicked() {
                p.coefficient = CoefficientPrior::Normal;
            }
            if ui.radio(t, "Student t").clicked() && !t {
                p.coefficient = CoefficientPrior::StudentT(3.0);
            }
            if let CoefficientPrior::StudentT(df) = &mut p.coefficient {
                ui.add(egui::DragValue::new(df).speed(0.1).range(1.0..=100.0).suffix(" DF"));
            }
            if ui.radio(p.coefficient == CoefficientPrior::Cauchy, "Cauchy").clicked() {
                p.coefficient = CoefficientPrior::Cauchy;
            }
            ui.label("scale");
            ui.add(egui::DragValue::new(&mut p.coefficient_scale).speed(0.05).range(0.001..=1e6));
        });
        ui.end_row();
        ui.label("Intercept:");
        ui.horizontal(|ui| {
            let mut fixed = p.intercept_location.is_some();
            if ui.checkbox(&mut fixed, "Centred at").on_hover_text("Otherwise centred at the response's mean").changed()
            {
                p.intercept_location = fixed.then_some(0.0);
            }
            if let Some(loc) = &mut p.intercept_location {
                ui.add(egui::DragValue::new(loc).speed(0.1));
            }
            ui.label("scale");
            ui.add(egui::DragValue::new(&mut p.intercept_scale).speed(0.05).range(0.001..=1e6));
        });
        ui.end_row();
        ui.label("Standard deviations:");
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("bayes_sd_prior").selected_text(scale_prior_label(p.sd)).show_list(ui, |ui| {
                for s in SCALE_PRIORS {
                    ui.selectable_value(&mut p.sd, s, scale_prior_label(s));
                }
            });
            ui.label("scale");
            ui.add(egui::DragValue::new(&mut p.sd_scale).speed(0.05).range(0.001..=1e6));
        });
        ui.end_row();
    });
    ui.checkbox(&mut p.autoscale, "Scale the priors to the data")
        .on_hover_text("Multiply the scales by the response's SD (normal) and divide by each predictor's SD");
}

fn sampler_ui(ui: &mut egui::Ui, m: &mut BayesModelRequest) {
    ui.label(RichText::new("Sampler").strong());
    ui.horizontal_wrapped(|ui| {
        ui.label("Chains:");
        ui.add(egui::DragValue::new(&mut m.chains).range(1..=16));
        ui.label("Warm-up:");
        ui.add(egui::DragValue::new(&mut m.warmup).range(100..=100_000).speed(10));
        ui.label("Draws per chain:");
        ui.add(egui::DragValue::new(&mut m.draws).range(100..=100_000).speed(10));
    });
    ui.horizontal(|ui| {
        ui.label("Target acceptance:").on_hover_text("Raise towards 0.99 if the report warns of divergent transitions");
        ui.add(egui::DragValue::new(&mut m.target_accept).speed(0.005).range(0.6..=0.999));
        ui.label("Seed:");
        ui.add(kit::seed_value(&mut m.seed));
    });
}

/// In-report settings of a Bayesian report; `labels` are the report's
/// parameter names (for the parameter shown in the sampler plots).
pub fn report_settings(ui: &mut egui::Ui, request: &mut BayesRequest, labels: &[String]) {
    match request {
        BayesRequest::Estimation(r) => {
            prior_ui(ui, r.parameter, &mut r.prior);
            if r.group.is_some() {
                match r.parameter {
                    EstimatedParameter::Mean => {
                        ui.checkbox(&mut r.equal_variances, "Assume equal variances");
                    }
                    EstimatedParameter::Proportion => contrast_ui(ui, &mut r.contrast),
                    EstimatedParameter::Rate => {}
                }
            } else {
                reference_ui(ui, r.parameter, &mut r.reference);
            }
            rope_ui(ui, &mut r.rope);
            level_ui(ui, &mut r.level);
        }
        BayesRequest::Factor(r) => {
            let test = match r.test {
                BayesTestKind::OneSample { .. } => Test::OneSample,
                BayesTestKind::Paired { .. } => Test::Paired,
                BayesTestKind::TwoSample { .. } => Test::TwoSample,
                BayesTestKind::Correlation { .. } => Test::Correlation,
                BayesTestKind::Contingency { .. } => Test::Contingency,
            };
            match &mut r.test {
                BayesTestKind::OneSample { null, .. } => {
                    ui.horizontal(|ui| {
                        ui.label("Hypothesized mean:");
                        ui.add(egui::DragValue::new(null).speed(0.1));
                    });
                }
                BayesTestKind::Contingency { sampling, .. } => sampling_ui(ui, sampling),
                _ => {}
            }
            if test != Test::Contingency {
                direction_ui(ui, &mut r.direction, test == Test::Correlation);
            }
            scale_ui(ui, test, &mut r.scale);
            level_ui(ui, &mut r.level);
        }
        BayesRequest::Linear(r) => {
            linear_prior_ui(ui, r);
            level_ui(ui, &mut r.level);
        }
        BayesRequest::Model(r) => {
            if !labels.is_empty() {
                let shown = r.plot_parameter.clone().unwrap_or_else(|| labels[0].clone());
                ui.horizontal(|ui| {
                    ui.label("Parameter in the sampler plots:");
                    egui::ComboBox::from_id_salt("bayes_plot_parameter").selected_text(shown).show_list(ui, |ui| {
                        for l in labels {
                            ui.selectable_value(&mut r.plot_parameter, Some(l.clone()), l);
                        }
                    });
                });
            }
            level_ui(ui, &mut r.level);
            kit::note(ui, "Change the priors and sampler from the report's dialog (Edit settings).");
        }
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
                Column::text("g", vec!["a".into(), "a".into(), "b".into(), "b".into()]),
            ],
        )
        .unwrap()
    }

    #[test]
    fn saved_requests_reopen_unchanged() {
        let t = table();
        let mut model = BayesModelRequest::new(0, vec![1], GlmFamily::Poisson);
        model.random = vec![RandomEffectSpec { correlated: false, ..RandomEffectSpec::intercept(2) }];
        model.offset = Some(1);
        model.chains = 2;
        let requests = [
            BayesRequest::Estimation(BayesEstimationRequest {
                group: Some(2),
                rope: Some((-0.5, 0.5)),
                ..BayesEstimationRequest::new(EstimatedParameter::Mean, 0)
            }),
            BayesRequest::Factor(BayesFactorRequest::new(BayesTestKind::Correlation { x: 0, y: 1 })),
            BayesRequest::Linear(BayesLinearRequest::new(0, vec![1, 2])),
            BayesRequest::Model(Box::new(model)),
        ];
        for request in requests {
            let saved = SavedReport::Bayesian { request: request.clone() };
            let d = BayesDialog::from_saved(&saved, &t).unwrap();
            let rebuilt = match d.tool {
                BayesTool::Estimation => {
                    let r = BayesEstimationRequest {
                        group: d.group,
                        rope: d.rope,
                        ..BayesEstimationRequest::new(d.parameter, d.response.unwrap())
                    };
                    BayesRequest::Estimation(r)
                }
                BayesTool::Factor => BayesRequest::Factor(BayesFactorRequest::new(BayesTestKind::Correlation {
                    x: d.response.unwrap(),
                    y: d.second.unwrap(),
                })),
                BayesTool::Linear => BayesRequest::Linear(BayesLinearRequest {
                    effects: d.positions(&[1, 2]),
                    ..BayesLinearRequest::new(d.response.unwrap(), vec![1, 2])
                }),
                BayesTool::Model => {
                    assert_eq!(d.terms.len(), 1);
                    assert_eq!(d.offset, Some(1));
                    assert_eq!(d.model.chains, 2);
                    request.clone()
                }
            };
            assert_eq!(rebuilt, request);
        }
    }
}
