//! Dialogs for the multivariate and model-fitting analyses: principal
//! components, clustering, generalized linear models, penalized regression
//! and several responses explored together.

use crate::dialog_kit::ComboExt as _;
use app_model::multiple_responses::{FactorSetting, SimulationSetup};
use app_model::DistanceMetric;
use app_model::{
    ChooseClusters, ClusterDistance, ClusterMethod, ClusterRequest, CorrespondenceRequest, Desirability, Family, GapReference,
    GlmRequest, Heredity, Link, Linkage, ModelForm, MultipleResponsesRequest, PcaRequest, RegularizedRequest,
    SavedReport, StartingCenters, StepCriterion, StepDirection, TermSelection,
};
use eframe::egui;
use stat_data::DataTable;

use crate::dialog_kit::{self as kit, Dialog, Outcome, Readiness, Remember};
use crate::report_view::ReportRequest;

/// Which analysis the dialog sets up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase2Kind {
    Pca,
    Cluster,
    Glm,
    Regularized,
    Correspondence,
    SeveralResponses,
}

/// Where k-means starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Starts {
    /// Seeded k-means++ starts only.
    Seeded,
    /// Only the centers given.
    Given,
    /// The centers given and the seeded starts; the best solution is kept.
    GivenAndSeeded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Penalty {
    Lasso,
    Ridge,
    ElasticNet,
}

/// What a saved several-responses report set up in the report itself
/// (goals, factor settings, simulation), kept when it is edited with the
/// same responses and factors.
#[derive(Clone)]
struct Explored {
    responses: Vec<usize>,
    predictors: Vec<usize>,
    goals: Vec<Option<Desirability>>,
    settings: Vec<FactorSetting>,
    simulation: Option<SimulationSetup>,
    constraints: String,
}

#[derive(Clone)]
pub struct Phase2Dialog {
    /// The report being edited (`None`: a new report).
    edit: Option<u64>,
    kind: Phase2Kind,
    /// Columns chosen (PCA / clustering) or predictors (models).
    chosen: Vec<bool>,
    response: Option<usize>,
    /// Second column (correspondence analysis).
    second: Option<usize>,
    standardize: bool,
    hierarchical: bool,
    k: usize,
    linkage: Linkage,
    seed: u64,
    /// K-means: compare numbers of clusters (gap statistic, silhouettes).
    choose: Option<ChooseClusters>,
    /// K-means: seeded starts, given centers, or both.
    starts: Starts,
    /// K-means: the given centers (k rows, one value per chosen column).
    centers: Vec<Vec<f64>>,
    /// Hierarchical clustering: how rows are compared.
    distance: ClusterDistance,
    family: Family,
    link: Link,
    /// Binomial GLM: the response value modelled as the event (`None`: the
    /// second of its two values in natural order).
    event: Option<String>,
    /// The response whose values last suggested a distribution, so the
    /// suggestion is made once per response and never undoes a choice.
    suggested_for: Option<usize>,
    penalty: Penalty,
    alpha: f64,
    folds: usize,
    /// GLM: two-factor interactions, as column pairs.
    interactions: Vec<(usize, usize)>,
    /// GLM: continuous predictors entered as splines.
    splines: crate::model_terms_ui::ColumnSplines,
    /// GLM: comparisons within slices and slopes by group.
    slices: Option<app_model::PairAdjust>,
    slopes: Option<app_model::PairAdjust>,
    /// Responses chosen (several responses).
    responses: Vec<bool>,
    form: ModelForm,
    explored: Option<Explored>,
    /// Several responses: each response's terms chosen by its own stepwise
    /// search (with `selection`'s settings) instead of fitting every term.
    stepwise_terms: bool,
    selection: TermSelection,
}

/// The rows of `t` as k-means centers for columns named `names`: every
/// name a numeric column of `t`, 1 to 50 rows, all values present.
fn center_rows(t: &DataTable, names: &[&str]) -> Option<Vec<Vec<f64>>> {
    if names.is_empty() || !(1..=50).contains(&t.n_rows()) {
        return None;
    }
    let cols: Vec<&[f64]> = names
        .iter()
        .map(|n| t.find(n).and_then(|j| t.column(j)).and_then(|c| c.as_numeric()))
        .collect::<Option<_>>()?;
    let rows: Vec<Vec<f64>> = (0..t.n_rows()).map(|r| cols.iter().map(|c| c[r]).collect()).collect();
    rows.iter().flatten().all(|v| v.is_finite()).then_some(rows)
}

/// Direction and criterion of a per-response stepwise search.
pub fn selection_choices(ui: &mut egui::Ui, selection: &mut TermSelection) {
    ui.horizontal(|ui| {
        ui.label("Direction:");
        for d in [StepDirection::Forward, StepDirection::Backward, StepDirection::Mixed] {
            ui.radio_value(&mut selection.direction, d, d.label());
        }
    });
    ui.horizontal(|ui| {
        ui.label("Criterion:");
        let (enter, remove) = match selection.criterion {
            StepCriterion::PValue { enter, remove } => (enter, remove),
            _ => (0.15, 0.15),
        };
        let p_values = StepCriterion::PValue { enter, remove };
        let is_p = matches!(selection.criterion, StepCriterion::PValue { .. });
        if ui.radio(is_p, "p-values").clicked() {
            selection.criterion = p_values;
        }
        ui.radio_value(&mut selection.criterion, StepCriterion::Aic, "AIC");
        ui.radio_value(&mut selection.criterion, StepCriterion::Bic, "BIC");
    });
}

/// Heredity and p-value thresholds of a per-response stepwise search.
pub fn selection_thresholds(ui: &mut egui::Ui, selection: &mut TermSelection) {
    ui.horizontal(|ui| {
        ui.label("Heredity:")
            .on_hover_text("Whether an interaction or power may be in a model without the effects it is built from");
        for h in Heredity::ALL {
            ui.radio_value(&mut selection.heredity, h, h.label());
        }
    });
    if let StepCriterion::PValue { enter, remove } = &mut selection.criterion {
        ui.horizontal(|ui| {
            ui.label("Enter if p <");
            ui.add(egui::DragValue::new(enter).range(0.001..=0.5).speed(0.005));
            ui.label("Remove if p >");
            ui.add(egui::DragValue::new(remove).range(0.001..=0.5).speed(0.005));
        });
        *remove = remove.max(*enter);
    }
}

/// Whether column `i` is categorical (text, or a nominal or ordinal number).
fn is_categorical(table: &DataTable, i: usize) -> bool {
    !kit::is_continuous(table, i)
}

impl Phase2Dialog {
    pub fn new(kind: Phase2Kind, table: &DataTable) -> Self {
        Self {
            edit: None,
            kind,
            chosen: vec![false; table.n_cols()],
            response: None,
            second: None,
            standardize: true,
            hierarchical: false,
            k: 3,
            linkage: Linkage::Ward,
            seed: 1,
            choose: None,
            starts: Starts::Seeded,
            centers: Vec::new(),
            distance: ClusterDistance::Euclidean,
            family: Family::Binomial,
            link: Family::Binomial.default_link(),
            event: None,
            suggested_for: None,
            penalty: Penalty::Lasso,
            alpha: 0.5,
            folds: 10,
            interactions: Vec::new(),
            splines: Vec::new(),
            slices: None,
            slopes: None,
            responses: vec![false; table.n_cols()],
            form: ModelForm::Quadratic,
            explored: None,
            stepwise_terms: false,
            selection: TermSelection::default(),
        }
    }

    /// The dialog filled from a saved principal components, clustering,
    /// generalized linear model, penalized regression, correspondence or
    /// several-responses report.
    pub fn from_saved(saved: &SavedReport, table: &DataTable) -> Option<Self> {
        let n = table.n_cols();
        let d = match saved {
            SavedReport::Pca { request } => Self {
                chosen: kit::mask(n, &request.columns),
                standardize: request.standardize,
                ..Self::new(Phase2Kind::Pca, table)
            },
            SavedReport::Cluster { request } => {
                let mut d = Self {
                    chosen: kit::mask(n, &request.columns),
                    standardize: request.standardize,
                    choose: request.choose,
                    distance: request.distance,
                    ..Self::new(Phase2Kind::Cluster, table)
                };
                if let Some(c) = &request.centers {
                    d.starts = if c.also_seeded { Starts::GivenAndSeeded } else { Starts::Given };
                    d.centers.clone_from(&c.centers);
                }
                match request.method {
                    ClusterMethod::KMeans { k, seed } => (d.hierarchical, d.k, d.seed) = (false, k, seed),
                    ClusterMethod::Hierarchical { linkage, k } => (d.hierarchical, d.k, d.linkage) = (true, k, linkage),
                }
                d
            }
            SavedReport::Glm { request, .. } => Self {
                response: kit::valid(table, request.response),
                chosen: kit::mask(n, &request.predictors),
                family: request.family,
                link: request.link.unwrap_or_else(|| request.family.default_link()),
                event: request.event.clone(),
                suggested_for: kit::valid(table, request.response),
                interactions: request
                    .effects
                    .iter()
                    .filter_map(|e| match e.as_slice() {
                        [(a, 1), (b, 1)] => Some((request.predictors[*a], request.predictors[*b])),
                        _ => None,
                    })
                    .collect(),
                splines: crate::model_terms_ui::by_column(&request.splines, &request.predictors),
                slices: request.slices,
                slopes: request.slopes,
                ..Self::new(Phase2Kind::Glm, table)
            },
            SavedReport::Regularized { request, .. } => {
                let penalty = if request.alpha >= 1.0 {
                    Penalty::Lasso
                } else if request.alpha <= 0.0 {
                    Penalty::Ridge
                } else {
                    Penalty::ElasticNet
                };
                let alpha = if penalty == Penalty::ElasticNet { request.alpha } else { 0.5 };
                Self {
                    response: kit::valid(table, request.response),
                    chosen: kit::mask(n, &request.predictors),
                    penalty,
                    alpha,
                    folds: request.folds,
                    seed: request.seed,
                    ..Self::new(Phase2Kind::Regularized, table)
                }
            }
            SavedReport::Correspondence { request } => Self {
                response: kit::valid(table, request.row),
                second: kit::valid(table, request.column),
                ..Self::new(Phase2Kind::Correspondence, table)
            },
            SavedReport::MultipleResponses { request } => Self {
                responses: kit::mask(n, &request.responses),
                chosen: kit::mask(n, &request.predictors),
                form: request.form,
                explored: Some(Explored {
                    responses: request.responses.clone(),
                    predictors: request.predictors.clone(),
                    goals: request.goals.clone(),
                    settings: request.settings.clone(),
                    simulation: request.simulation.clone(),
                    constraints: request.constraints.clone(),
                }),
                stepwise_terms: request.selection.is_some(),
                selection: request.selection.clone().unwrap_or_default(),
                ..Self::new(Phase2Kind::SeveralResponses, table)
            },
            _ => return None,
        };
        Some(d)
    }

    fn title(&self) -> &'static str {
        match self.kind {
            Phase2Kind::Pca => "Principal components",
            Phase2Kind::Cluster => "Clustering",
            Phase2Kind::Glm => "Generalized linear model",
            Phase2Kind::Regularized => "Penalized regression",
            Phase2Kind::Correspondence => "Correspondence analysis",
            Phase2Kind::SeveralResponses => "Several responses",
        }
    }

    /// Show the dialog over `table`; the other open tables `others` (name,
    /// table) are offered as sources of k-means starting centers.
    pub fn show_with(
        &mut self,
        ctx: &egui::Context,
        table: &DataTable,
        others: &[(&str, &DataTable)],
    ) -> Outcome<ReportRequest> {
        let dialog = Dialog::new(self.title()).editing(self.edit);
        let outcome = dialog.show(ctx, |ui| match self.kind {
            Phase2Kind::Pca | Phase2Kind::Cluster => self.columns_ui(ui, table, others),
            Phase2Kind::Glm | Phase2Kind::Regularized => self.model_ui(ui, table),
            Phase2Kind::Correspondence => self.correspondence_ui(ui, table),
            Phase2Kind::SeveralResponses => self.several_ui(ui, table),
        });
        match outcome {
            Outcome::Reset => {
                *self = Self { edit: self.edit, ..Self::new(self.kind, table) };
                Outcome::Open
            }
            outcome => outcome,
        }
    }

    fn several_ui(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<ReportRequest> {
        kit::note(
            ui,
            "Fits a model to each response (the same terms, or each response's own terms chosen stepwise), \
             then explores them together: linked slice plots, desirability goals, a search for the best \
             settings, and simulation of variation.",
        );
        let (chosen, responses) = (self.chosen.clone(), self.responses.clone());
        let on = |mask: &[bool], i: usize| mask.get(i).copied().unwrap_or(false);
        // Fixed widths: a list otherwise takes all the width it is offered.
        let width = kit::em(ui) * 17.0;
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(width);
                ui.label("Responses (continuous):");
                kit::column_list(ui, "several_responses", table, &mut self.responses, |i| {
                    kit::is_continuous(table, i) && !on(&chosen, i)
                });
            });
            ui.vertical(|ui| {
                ui.set_width(width);
                ui.label("Factors:");
                kit::column_list(ui, "several_factors", table, &mut self.chosen, |i| !on(&responses, i));
            });
        });
        ui.separator();
        let custom = !self.selection.candidates.is_empty();
        ui.horizontal_wrapped(|ui| {
            ui.label(if self.stepwise_terms { "Candidate terms:" } else { "Model for every response:" });
            ui.add_enabled_ui(!(self.stepwise_terms && custom), |ui| {
                for form in ModelForm::ALL {
                    ui.radio_value(&mut self.form, form, form.label());
                }
            });
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Terms:");
            ui.radio_value(&mut self.stepwise_terms, false, "Fit every term")
                .on_hover_text("Every response gets the same model");
            ui.radio_value(&mut self.stepwise_terms, true, "Select terms stepwise for each response")
                .on_hover_text("A separate stepwise search for each response keeps only the terms that response needs");
        });
        if self.stepwise_terms {
            selection_choices(ui, &mut self.selection);
            if custom {
                ui.horizontal(|ui| {
                    kit::note(
                        ui,
                        &format!(
                            "Candidates: {} effects chosen in the stepwise dialog.",
                            self.selection.candidates.len()
                        ),
                    );
                    if ui.small_button("Use the model form instead").clicked() {
                        self.selection.candidates.clear();
                    }
                });
            }
        }
        kit::more_options(ui, |ui| {
            kit::note(
                ui,
                "Quadratic adds every two-factor interaction and the square of each continuous factor \
                 (needs at least three levels per continuous factor).",
            );
            if self.stepwise_terms {
                selection_thresholds(ui, &mut self.selection);
            }
        });
        let responses: Vec<usize> = kit::chosen_columns(&self.responses)
            .into_iter()
            .filter(|&i| kit::is_continuous(table, i) && !on(&self.chosen, i))
            .collect();
        let predictors: Vec<usize> = kit::chosen_columns(&self.chosen)
            .into_iter()
            .filter(|&i| i < table.n_cols() && !on(&self.responses, i))
            .collect();
        if responses.is_empty() {
            return Err("Choose at least one response".into());
        }
        if predictors.is_empty() {
            return Err("Choose at least one factor".into());
        }
        // Goals, settings and simulation set up in an edited report stay
        // while they still fit its responses and factors.
        let kept = self.explored.as_ref();
        let same_responses = kept.is_some_and(|k| k.responses == responses);
        let same_factors = kept.is_some_and(|k| k.predictors == predictors);
        let form = match self.form {
            ModelForm::MainEffects => "main-effects",
            ModelForm::Interactions => "interaction",
            ModelForm::Quadratic => "quadratic",
        };
        let (n_responses, n_factors) =
            (kit::count(responses.len(), "response", "responses"), kit::count(predictors.len(), "factor", "factors"));
        // Chosen candidates name predictor positions: kept only while the
        // factors are the same.
        let custom = self.stepwise_terms && !self.selection.candidates.is_empty();
        if custom && !same_factors {
            return Err("The factors changed: use the model form, or choose candidates in the stepwise dialog".into());
        }
        let summary = if self.stepwise_terms {
            let from = if custom { "chosen candidates".to_owned() } else { format!("the {form} terms") };
            format!("Stepwise terms for each of {n_responses} on {n_factors}, from {from}")
        } else {
            let mut form = form.to_owned();
            form[..1].make_ascii_uppercase();
            format!("{form} models of {n_responses} on {n_factors}")
        };
        let request = MultipleResponsesRequest {
            goals: kept.filter(|_| same_responses).map(|k| k.goals.clone()).unwrap_or_default(),
            settings: kept.filter(|_| same_factors).map(|k| k.settings.clone()).unwrap_or_default(),
            simulation: kept.filter(|_| same_responses && same_factors).and_then(|k| k.simulation.clone()),
            constraints: kept.filter(|_| same_factors).map(|k| k.constraints.clone()).unwrap_or_default(),
            responses,
            predictors,
            form: self.form,
            selection: self.stepwise_terms.then(|| self.selection.clone()),
        };
        kit::ready(ReportRequest::MultipleResponses(request), summary)
    }

    fn correspondence_ui(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<ReportRequest> {
        kit::note(ui, "Cross-tabulates two categorical columns and maps their categories together.");
        egui::Grid::new("ca_columns").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label("Rows:");
            kit::column_choice(ui, "ca_rows", table, &mut self.response, |i| is_categorical(table, i), None);
            ui.end_row();
            ui.label("Columns:");
            kit::column_choice(ui, "ca_columns", table, &mut self.second, |i| is_categorical(table, i), None);
            ui.end_row();
        });
        let Some(row) = self.response else { return Err("Choose a column for the rows".into()) };
        let Some(column) = self.second else { return Err("Choose a column for the columns".into()) };
        if row == column {
            return Err("Rows and columns must be different columns".into());
        }
        let summary = format!("Correspondence analysis of {} by {}", kit::name(table, row), kit::name(table, column));
        kit::ready(ReportRequest::Correspondence(CorrespondenceRequest { row, column }), summary)
    }

    /// The k-means starting-centers editor: one row per center, one value
    /// per chosen column, typed or read from an open table whose columns
    /// have the same names (one row per center).
    fn centers_ui(&mut self, ui: &mut egui::Ui, table: &DataTable, columns: &[usize], others: &[(&str, &DataTable)]) {
        ui.horizontal(|ui| {
            ui.label("Starting centers:");
            ui.radio_value(&mut self.starts, Starts::Seeded, "Seeded starts")
                .on_hover_text("Ten k-means++ starts from the random seed; the best solution is kept");
            ui.radio_value(&mut self.starts, Starts::Given, "Given centers")
                .on_hover_text("Start once, from the centers below");
            ui.radio_value(&mut self.starts, Starts::GivenAndSeeded, "Given and seeded")
                .on_hover_text("The centers below as one more start beside the seeded ones; the best is kept");
        });
        if self.starts == Starts::Seeded || columns.is_empty() {
            return;
        }
        // One center per cluster, one value per column; new cells start at
        // evenly spaced quantiles of the column.
        let quantile = |j: usize, i: usize| {
            let mut v: Vec<f64> = table
                .column(columns[j])
                .and_then(|c| c.as_numeric())
                .map(|x| x.iter().copied().filter(|v| v.is_finite()).collect())
                .unwrap_or_default();
            v.sort_by(f64::total_cmp);
            if v.is_empty() {
                return 0.0;
            }
            let p = (i as f64 + 0.5) / self.k as f64;
            v[((p * v.len() as f64) as usize).min(v.len() - 1)]
        };
        self.centers.truncate(self.k);
        while self.centers.len() < self.k {
            let i = self.centers.len();
            self.centers.push((0..columns.len()).map(|j| quantile(j, i)).collect());
        }
        for (i, row) in self.centers.iter_mut().enumerate() {
            row.truncate(columns.len());
            while row.len() < columns.len() {
                row.push(quantile(row.len(), i));
            }
        }
        ui.indent("kmeans_centers", |ui| {
            egui::Grid::new("kmeans_center_grid").num_columns(columns.len() + 1).spacing([6.0, 3.0]).show(ui, |ui| {
                ui.label("Center");
                for &c in columns {
                    ui.label(kit::name(table, c));
                }
                ui.end_row();
                for (i, row) in self.centers.iter_mut().enumerate() {
                    ui.label((i + 1).to_string());
                    for (j, v) in row.iter_mut().enumerate() {
                        ui.add(egui::DragValue::new(v).speed(0.01).max_decimals(6)).widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::WidgetType::DragValue,
                                true,
                                format!("Center {} {}", i + 1, j + 1),
                            )
                        });
                    }
                    ui.end_row();
                }
            });
            let names: Vec<&str> = columns.iter().map(|&c| kit::name(table, c)).collect();
            let sources: Vec<(&str, Vec<Vec<f64>>)> =
                others.iter().filter_map(|(name, t)| center_rows(t, &names).map(|rows| (*name, rows))).collect();
            if !sources.is_empty() {
                ui.horizontal(|ui| {
                    ui.label("Read from table:");
                    for (name, rows) in &sources {
                        if ui
                            .button(*name)
                            .on_hover_text(format!(
                                "Use its {} rows as the centers (columns matched by name)",
                                rows.len()
                            ))
                            .clicked()
                        {
                            self.centers.clone_from(rows);
                            self.k = rows.len();
                        }
                    }
                });
            } else {
                kit::note(
                    ui,
                    "To read the centers from a table, open one with these column names and one row per center.",
                );
            }
            kit::note(ui, "The number of clusters is the number of centers; centers are in the data's units.");
        });
    }

    fn columns_ui(
        &mut self,
        ui: &mut egui::Ui,
        table: &DataTable,
        others: &[(&str, &DataTable)],
    ) -> Readiness<ReportRequest> {
        let pca = self.kind == Phase2Kind::Pca;
        // Hierarchical clustering on Gower, Hamming or simple-matching
        // distances also takes categorical columns; a stored distance matrix
        // and the other metrics take numbers.
        let distance = if !pca && self.hierarchical { self.distance } else { ClusterDistance::Euclidean };
        let any_type = matches!(
            distance,
            ClusterDistance::Metric(DistanceMetric::Gower | DistanceMetric::Hamming | DistanceMetric::SimpleMatching)
        );
        let allow = |i: usize| match distance {
            ClusterDistance::Euclidean => kit::is_continuous(table, i),
            ClusterDistance::Matrix => kit::is_numeric(table, i),
            ClusterDistance::Metric(_) => any_type || kit::is_numeric(table, i),
        };
        ui.label(match distance {
            ClusterDistance::Euclidean => "Continuous columns to analyse:",
            ClusterDistance::Matrix => "Distance matrix columns (one per row, in row order):",
            ClusterDistance::Metric(_) if any_type => "Columns to compare (any type):",
            ClusterDistance::Metric(_) => "Numeric columns to compare:",
        });
        kit::column_list(ui, "phase2_columns", table, &mut self.chosen, allow);
        let columns: Vec<usize> = kit::chosen_columns(&self.chosen).into_iter().filter(|&i| allow(i)).collect();
        ui.separator();
        let request = if pca {
            ui.checkbox(&mut self.standardize, "Standardize (use correlations; recommended when units differ)");
            ReportRequest::Pca(PcaRequest { columns: columns.clone(), standardize: self.standardize })
        } else {
            ui.horizontal(|ui| {
                ui.radio_value(&mut self.hierarchical, false, "k-means");
                ui.radio_value(&mut self.hierarchical, true, "Hierarchical");
            });
            ui.horizontal(|ui| {
                ui.label("Number of clusters:");
                ui.add(egui::DragValue::new(&mut self.k).range(1..=50));
            });
            if self.hierarchical {
                ui.horizontal(|ui| {
                    ui.label("Distance:");
                    let label = |d: ClusterDistance| match d {
                        ClusterDistance::Euclidean => "Euclidean".to_owned(),
                        ClusterDistance::Metric(m) => m.label().to_owned(),
                        ClusterDistance::Matrix => "The columns are a distance matrix".to_owned(),
                    };
                    egui::ComboBox::from_id_salt("cluster_distance")
                        .selected_text(label(self.distance))
                        .width(kit::combo_width(ui))
                        .show_list(ui, |ui| {
                            let mut options = vec![ClusterDistance::Euclidean];
                            options.extend(
                                DistanceMetric::ALL
                                    .into_iter()
                                    .filter(|m| *m != DistanceMetric::Euclidean)
                                    .map(ClusterDistance::Metric),
                            );
                            options.push(ClusterDistance::Matrix);
                            for d in options {
                                ui.selectable_value(&mut self.distance, d, label(d));
                            }
                        });
                });
            }
            kit::more_options(ui, |ui| {
                if self.hierarchical {
                    ui.horizontal(|ui| {
                        ui.label("Linkage:");
                        for l in [Linkage::Ward, Linkage::Average, Linkage::Complete, Linkage::Single] {
                            ui.radio_value(&mut self.linkage, l, l.label());
                        }
                    });
                } else {
                    ui.horizontal(|ui| {
                        ui.label("Random seed:");
                        ui.add(crate::dialog_kit::seed_value(&mut self.seed));
                    });
                }
                ui.checkbox(&mut self.standardize, "Standardize columns first (recommended when units differ)");
            });
            if !self.hierarchical {
                self.centers_ui(ui, table, &columns, others);
            }
            if !self.hierarchical {
                let mut on = self.choose.is_some();
                ui.checkbox(&mut on, "Compare numbers of clusters (gap statistic and silhouettes)")
                    .on_hover_text("Fits k = 1, 2, … and suggests a number of clusters");
                match (on, self.choose.is_some()) {
                    (true, false) => self.choose = Some(ChooseClusters::default()),
                    (false, true) => self.choose = None,
                    _ => {}
                }
                if let Some(c) = &mut self.choose {
                    ui.indent("choose_k", |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Up to k =");
                            ui.add(egui::DragValue::new(&mut c.max_k).range(2..=30));
                            ui.label("Reference data sets:");
                            ui.add(egui::DragValue::new(&mut c.references).range(5..=500));
                        });
                        ui.horizontal(|ui| {
                            ui.label("Reference data drawn over:");
                            ui.radio_value(&mut c.reference, GapReference::DataRange, "the data range");
                            ui.radio_value(
                                &mut c.reference,
                                GapReference::PrincipalComponents,
                                "the principal-component box",
                            );
                        });
                    });
                }
            }
            let method = if self.hierarchical {
                ClusterMethod::Hierarchical { linkage: self.linkage, k: self.k }
            } else {
                ClusterMethod::KMeans { k: self.k, seed: self.seed }
            };
            ReportRequest::Cluster(ClusterRequest {
                distance,
                columns: columns.clone(),
                method,
                standardize: self.standardize,
                choose: if self.hierarchical { None } else { self.choose },
                centers: match self.starts {
                    _ if self.hierarchical => None,
                    Starts::Seeded => None,
                    Starts::Given | Starts::GivenAndSeeded => Some(StartingCenters {
                        centers: self.centers.iter().take(self.k).cloned().collect(),
                        also_seeded: self.starts == Starts::GivenAndSeeded,
                    }),
                },
            })
        };
        if pca && columns.len() < 2 {
            return Err("Choose at least two columns".into());
        }
        if columns.is_empty() {
            return Err("Choose at least one column".into());
        }
        let n = kit::count(columns.len(), "column", "columns");
        let summary = if pca {
            let on = if self.standardize { "correlations" } else { "covariances" };
            format!("Principal components of {n}, on {on}")
        } else if self.hierarchical {
            let on = match self.distance {
                ClusterDistance::Euclidean => String::new(),
                ClusterDistance::Metric(m) => format!(", {} distances", m.label()),
                ClusterDistance::Matrix => ", stored distances".to_owned(),
            };
            format!("Hierarchical ({}) clustering of {n} into {} clusters{on}", self.linkage.label(), self.k)
        } else {
            format!("k-means clustering of {n} into {} clusters", self.k)
        };
        kit::ready(request, summary)
    }

    fn model_ui(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<ReportRequest> {
        let glm = self.kind == Phase2Kind::Glm;
        // A GLM response may be any column (a binomial one any two-valued
        // column); penalized regression needs a numeric one.
        let allow_response = |i: usize| glm || kit::is_numeric(table, i);
        if self.response.is_some_and(|r| !allow_response(r)) {
            self.response = None;
        }
        ui.horizontal(|ui| {
            ui.label("Response:");
            kit::column_choice(ui, "phase2_response", table, &mut self.response, allow_response, None);
        });
        // The response's values (up to three distinct), for the GLM's
        // distribution suggestion and event choice.
        let values = self.response.map(|r| distinct_values(table, r, 3)).unwrap_or_default();
        if glm {
            if let Some(r) = self.response.filter(|&r| self.suggested_for != Some(r)) {
                self.suggested_for = Some(r);
                self.event = None;
                if let Some(family) = suggested_family(table, r, &values) {
                    self.family = family;
                }
            }
            ui.horizontal_wrapped(|ui| {
                ui.label("Response distribution:");
                ui.radio_value(&mut self.family, Family::Binomial, "Binomial (yes/no)");
                ui.radio_value(&mut self.family, Family::Poisson, "Poisson (counts)");
                ui.radio_value(&mut self.family, Family::Gaussian, "Normal");
                ui.radio_value(&mut self.family, Family::Gamma, "Gamma (positive, skewed)");
            });
            if !self.family.links().contains(&self.link) {
                self.link = self.family.default_link();
            }
            if self.family == Family::Binomial && values.len() == 2 {
                if self.event.as_ref().is_some_and(|e| !values.contains(e)) {
                    self.event = None;
                }
                let modelled = self.event.clone().unwrap_or_else(|| values[1].clone());
                ui.horizontal(|ui| {
                    ui.label("Event:");
                    egui::ComboBox::from_id_salt("glm_event").selected_text(&modelled).show_list(ui, |ui| {
                        for v in &values {
                            if ui.selectable_label(*v == modelled, v).clicked() {
                                // The default stays unnamed, so the request (and
                                // a saved project) only records a real choice.
                                self.event = (*v != values[1]).then(|| v.clone());
                            }
                        }
                    });
                    let other = if modelled == values[0] { &values[1] } else { &values[0] };
                    ui.label(egui::RichText::new(format!("models P(= {modelled}) against {other}")).weak());
                });
            }
        }
        ui.label("Predictors:");
        let response = self.response;
        kit::column_list(ui, "phase2_predictors", table, &mut self.chosen, |i| Some(i) != response);
        let predictors: Vec<usize> = kit::chosen_columns(&self.chosen)
            .into_iter()
            .filter(|&i| i < table.n_cols() && Some(i) != self.response)
            .collect();
        if glm {
            self.interactions.retain(|(a, b)| predictors.contains(a) && predictors.contains(b));
            kit::more_options(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label("Link:");
                    for &link in self.family.links() {
                        let label = if link == self.family.default_link() {
                            format!("{} (default)", link.label())
                        } else {
                            link.label().to_owned()
                        };
                        ui.radio_value(&mut self.link, link, label);
                    }
                });
                if predictors.len() >= 2 {
                    ui.label("Two-factor interactions:");
                    egui::ScrollArea::vertical().id_salt("glm_interactions").max_height(kit::em(ui) * 6.0).show(
                        ui,
                        |ui| {
                            for (i, &a) in predictors.iter().enumerate() {
                                for &b in &predictors[i + 1..] {
                                    let mut on = self.interactions.contains(&(a, b));
                                    let label = format!("{} × {}", kit::name(table, a), kit::name(table, b));
                                    if ui.checkbox(&mut on, label).changed() {
                                        if on {
                                            self.interactions.push((a, b));
                                        } else {
                                            self.interactions.retain(|p| *p != (a, b));
                                        }
                                    }
                                }
                            }
                        },
                    );
                }
                let interactions = self.interactions.clone();
                let eligible: Vec<usize> = predictors
                    .iter()
                    .copied()
                    .filter(|&c| kit::is_continuous(table, c) && !interactions.iter().any(|(a, b)| *a == c || *b == c))
                    .collect();
                crate::model_terms_ui::spline_settings(ui, table, &eligible, &mut self.splines);
                crate::model_terms_ui::comparison_settings(ui, &mut self.slices, &mut self.slopes);
            });
        } else {
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("Penalty:");
                ui.radio_value(&mut self.penalty, Penalty::Lasso, "Lasso");
                ui.radio_value(&mut self.penalty, Penalty::Ridge, "Ridge");
                ui.radio_value(&mut self.penalty, Penalty::ElasticNet, "Elastic net, α =");
                ui.add_enabled(
                    self.penalty == Penalty::ElasticNet,
                    egui::DragValue::new(&mut self.alpha).speed(0.01).range(0.01..=0.99),
                );
            });
            kit::more_options(ui, |ui| {
                kit::note(ui, "Lasso can drop terms entirely; ridge shrinks all terms; the elastic net mixes the two.");
                ui.horizontal(|ui| {
                    ui.label("Cross-validation folds:");
                    ui.add(egui::DragValue::new(&mut self.folds).range(2..=50));
                    ui.label("Seed:");
                    ui.add(crate::dialog_kit::seed_value(&mut self.seed));
                });
            });
        }
        let Some(response) = self.response else { return Err("Choose a response".into()) };
        if glm && self.family == Family::Binomial && values.len() != 2 {
            return Err(format!(
                "A binomial response needs exactly two values; {} has {}",
                kit::name(table, response),
                if values.len() > 2 { "more".to_owned() } else { values.len().to_string() }
            ));
        }
        if glm && self.family != Family::Binomial && !kit::is_numeric(table, response) {
            return Err(format!("A {} response must be numeric", self.family.name()));
        }
        if predictors.is_empty() {
            return Err("Choose at least one predictor".into());
        }
        let on =
            format!("{} on {}", kit::name(table, response), kit::count(predictors.len(), "predictor", "predictors"));
        if glm {
            let link = (self.link != self.family.default_link()).then_some(self.link);
            let mut summary = format!("{} model of {on}", capitalized(self.family.name()));
            if let Some(link) = link {
                summary += &format!(", {} link", link.label());
            }
            let event = if self.family == Family::Binomial { self.event.clone() } else { None };
            if let Some(event) = &event {
                summary += &format!(", event {event}");
            }
            let position = |c: usize| predictors.iter().position(|&p| p == c).unwrap_or(0);
            let effects = if self.interactions.is_empty() {
                Vec::new()
            } else {
                let mut e: Vec<Vec<(usize, u8)>> = (0..predictors.len()).map(|i| vec![(i, 1)]).collect();
                e.extend(self.interactions.iter().map(|&(a, b)| vec![(position(a), 1), (position(b), 1)]));
                summary += &format!(", {}", kit::count(self.interactions.len(), "interaction", "interactions"));
                e
            };
            if let Some((c, _)) =
                self.splines.iter().find(|(c, _)| self.interactions.iter().any(|(a, b)| a == c || b == c))
            {
                return Err(format!("{} is a spline: take it out of interactions", kit::name(table, *c)));
            }
            let splines = crate::model_terms_ui::by_position(&self.splines, &predictors);
            let n_splines = splines.iter().flatten().count();
            if n_splines > 0 {
                summary += &format!(", {}", kit::count(n_splines, "spline", "splines"));
            }
            let request = GlmRequest {
                response,
                predictors,
                family: self.family,
                link,
                event,
                effects,
                splines,
                slices: self.slices,
                slopes: self.slopes,
            };
            kit::ready(ReportRequest::Glm(request), summary)
        } else {
            let alpha = match self.penalty {
                Penalty::Lasso => 1.0,
                Penalty::Ridge => 0.0,
                Penalty::ElasticNet => self.alpha,
            };
            let summary = match self.penalty {
                Penalty::Lasso => format!("Lasso of {on}"),
                Penalty::Ridge => format!("Ridge regression of {on}"),
                Penalty::ElasticNet => format!("Elastic net (α = {alpha}) of {on}"),
            };
            let request = RegularizedRequest { response, predictors, alpha, folds: self.folds, seed: self.seed };
            kit::ready(ReportRequest::Regularized(request), summary)
        }
    }
}

/// Up to `max` distinct non-empty values of column `i`, in natural order
/// (more than `max` are cut at `max`).
fn distinct_values(table: &DataTable, i: usize, max: usize) -> Vec<String> {
    let Some(col) = table.column(i) else { return Vec::new() };
    let mut seen: Vec<String> = Vec::new();
    for r in 0..table.n_rows() {
        let v = col.level_text(r);
        if !v.trim().is_empty() && !seen.contains(&v) {
            seen.push(v);
            if seen.len() == max {
                break;
            }
        }
    }
    seen.sort_by(|a, b| stat_data::natural_order(a, b));
    seen
}

/// The distribution a GLM response's values point to: binomial for exactly
/// two values or any text, Poisson for non-negative whole numbers, normal for
/// other numbers. `values` holds the column's first distinct values.
fn suggested_family(table: &DataTable, i: usize, values: &[String]) -> Option<Family> {
    let col = table.column(i)?;
    let Some(numbers) = col.as_numeric() else { return Some(Family::Binomial) };
    if values.len() == 2 {
        return Some(Family::Binomial);
    }
    let finite = numbers.iter().filter(|x| x.is_finite());
    if finite.clone().all(|&x| x >= 0.0 && x.fract() == 0.0) { Some(Family::Poisson) } else { Some(Family::Gaussian) }
}

/// "binomial" → "Binomial".
fn capitalized(s: &str) -> String {
    let mut chars = s.chars();
    chars.next().map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
}

impl Remember for Phase2Dialog {
    fn memory_key(&self) -> String {
        self.title().to_owned()
    }

    fn set_edit(&mut self, report: Option<u64>) {
        self.edit = report;
    }

    fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        match self.kind {
            Phase2Kind::Pca | Phase2Kind::Cluster | Phase2Kind::Glm | Phase2Kind::Regularized => {
                self.chosen = kit::mask(table.n_cols(), columns);
            }
            Phase2Kind::SeveralResponses => {
                let continuous: Vec<usize> =
                    columns.iter().copied().filter(|&c| kit::is_continuous(table, c)).collect();
                self.responses = kit::mask(table.n_cols(), &continuous);
            }
            Phase2Kind::Correspondence => {
                let mut categorical = columns.iter().copied().filter(|&c| is_categorical(table, c));
                self.response = categorical.next();
                self.second = categorical.next();
            }
        }
    }
}
