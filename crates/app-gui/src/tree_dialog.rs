//! Dialog for a decision tree: response, predictors, how rows are held back
//! for validation, and the tree's growth and pruning settings. The
//! validation choices ([`ValidationChoice`]) and the response and predictor
//! roles ([`roles_ui`]) are shared by every predictive model's dialog.

use crate::dialog_kit::ComboExt as _;
use app_model::{Criterion, Pruning, SavedReport, TreeRequest, ValidationMethod};
use eframe::egui;
use stat_data::DataTable;

use crate::dialog_kit::{self as kit, Dialog, Outcome, Readiness, Remember};

/// Which validation method the dialog shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Method {
    None,
    Holdout,
    KFold,
    Column,
}

/// The validation part of a predictive model's dialog.
#[derive(Clone)]
pub struct ValidationChoice {
    method: Method,
    validation: f64,
    test: f64,
    seed: u64,
    stratify: bool,
    group: Option<usize>,
    folds: usize,
    column: Option<usize>,
    /// What held-out rows tune, for the hover texts.
    purpose: Purpose,
}

/// The model a [`ValidationChoice`] serves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Purpose {
    /// Validation rows choose a tree's size.
    Tree,
    /// Nothing is tuned; out-of-bag rows give held-out measures.
    Forest,
    /// Validation rows choose the number of boosting rounds.
    Boost,
    /// Validation rows (or folds) choose the number of PLS factors.
    Pls,
    /// Validation rows (or folds) tune an SVM's cost and γ.
    Svm,
    /// Nothing is tuned; held-out rows only measure a discriminant rule.
    Discriminant,
    /// Validation rows (or folds) choose the number of MARS terms.
    Mars,
    /// Validation rows (or folds) tune a neural network's penalty.
    Neural,
    /// Validation rows (or folds) tune every model and rank them.
    Search,
    /// Nothing is tuned; held-out rows only measure a Gaussian process
    /// (leave-one-out predictions come without them).
    GaussianProcess,
}

impl Default for ValidationChoice {
    fn default() -> Self {
        Self {
            method: Method::Holdout,
            validation: 0.25,
            test: 0.0,
            seed: 1,
            stratify: true,
            group: None,
            folds: 10,
            column: None,
            purpose: Purpose::Tree,
        }
    }
}

impl ValidationChoice {
    /// The choices for a random forest: every row trains by default, since
    /// out-of-bag rows already give held-out measures.
    pub fn for_forest() -> Self {
        Self { method: Method::None, purpose: Purpose::Forest, ..Self::default() }
    }

    /// The choices for boosted trees: validation rows choose the number of
    /// rounds.
    pub fn for_boosting() -> Self {
        Self { purpose: Purpose::Boost, ..Self::default() }
    }

    /// The choices for partial least squares: validation rows choose the
    /// number of factors.
    pub fn for_pls() -> Self {
        Self { purpose: Purpose::Pls, ..Self::default() }
    }

    /// The choices for a support vector machine: validation rows tune the
    /// cost and γ.
    pub fn for_svm() -> Self {
        Self { purpose: Purpose::Svm, ..Self::default() }
    }

    /// The choices for discriminant analysis: every row trains by default
    /// (the classical analysis); held-out rows only measure the rule.
    pub fn for_discriminant() -> Self {
        Self { method: Method::None, purpose: Purpose::Discriminant, ..Self::default() }
    }

    /// The choices for multivariate adaptive regression splines: every row
    /// trains by default (GCV chooses the number of terms).
    pub fn for_mars() -> Self {
        Self { method: Method::None, purpose: Purpose::Mars, ..Self::default() }
    }

    /// The choices for a neural network: validation rows tune the penalty.
    pub fn for_neural() -> Self {
        Self { purpose: Purpose::Neural, ..Self::default() }
    }

    /// The choices for a Gaussian process: every row trains by default
    /// (leave-one-out predictions measure it).
    pub fn for_gaussian_process() -> Self {
        Self { method: Method::None, purpose: Purpose::GaussianProcess, ..Self::default() }
    }

    /// The choices for a model search: validation rows tune and rank the
    /// models.
    pub fn for_search() -> Self {
        Self { purpose: Purpose::Search, ..Self::default() }
    }

    /// The same choices set to a saved request's `method` (settings the
    /// method does not use keep their defaults; columns the table no longer
    /// has are dropped).
    pub fn with_method(mut self, method: &ValidationMethod, table: &DataTable) -> Self {
        match *method {
            ValidationMethod::None => self.method = Method::None,
            ValidationMethod::Column(c) => {
                self.method = Method::Column;
                self.column = kit::valid(table, c);
            }
            ValidationMethod::Holdout { validation, test, seed, stratify, group } => {
                self.method = Method::Holdout;
                self.validation = validation;
                self.test = test;
                self.seed = seed;
                self.stratify = stratify;
                self.group = group.and_then(|g| kit::valid(table, g));
            }
            ValidationMethod::KFold { folds, seed } => {
                self.method = Method::KFold;
                self.folds = folds;
                self.seed = seed;
            }
        }
        self
    }

    /// The hover text for the model served, in the order tree, forest,
    /// boosting, partial least squares, support vector machine,
    /// discriminant analysis, MARS, neural network, model search, Gaussian
    /// process.
    fn hint(&self, texts: [&'static str; 10]) -> &'static str {
        texts[match self.purpose {
            Purpose::Tree => 0,
            Purpose::Forest => 1,
            Purpose::Boost => 2,
            Purpose::Pls => 3,
            Purpose::Svm => 4,
            Purpose::Discriminant => 5,
            Purpose::Mars => 6,
            Purpose::Neural => 7,
            Purpose::Search => 8,
            Purpose::GaussianProcess => 9,
        }]
    }

    /// Draw the method and its shares, folds or role column; `used` are the
    /// response and predictor columns (not offered as the role column).
    pub fn ui(&mut self, ui: &mut egui::Ui, table: &DataTable, used: &[usize]) {
        heading(ui, "Validation");
        ui.radio_value(&mut self.method, Method::Holdout, "Hold back random rows").on_hover_text(self.hint([
            "Validation rows choose the tree's size; test rows are kept out of both fitting and tuning",
            "Validation and test rows are kept out of fitting and get their own fit measures",
            "Validation rows choose the number of rounds (boosting stops once they stop improving); test rows are kept out of both fitting and tuning",
            "Validation rows choose the number of factors; test rows are kept out of both fitting and tuning",
            "Validation rows choose the cost and γ; test rows are kept out of both fitting and tuning",
            "Validation and test rows are kept out of fitting and get their own fit measures and classification tables",
            "Validation rows choose the number of terms; test rows are kept out of both fitting and tuning",
            "Validation rows choose the penalty; test rows are kept out of both fitting and tuning",
            "Validation rows tune each model and rank them; test rows are kept out of fitting, tuning and ranking",
            "Validation and test rows are kept out of fitting and get their own fit measures",
        ]));
        if self.method == Method::Holdout {
            ui.indent("holdout", |ui| {
                ui.horizontal(|ui| {
                    ui.label("Validation share:");
                    ui.add(egui::DragValue::new(&mut self.validation).speed(0.01).range(0.0..=0.8));
                    ui.label("Test share:");
                    ui.add(egui::DragValue::new(&mut self.test).speed(0.01).range(0.0..=0.8));
                });
            });
        }
        ui.radio_value(&mut self.method, Method::KFold, "K-fold cross-validation").on_hover_text(self.hint([
            "Every row trains; folds choose the size and give held-out fit measures",
            "One forest per fold gives held-out fit measures; the report shows a forest grown on every row",
            "Every row trains; folds choose the number of rounds and give held-out fit measures",
            "Every row trains; folds choose the number of factors and give held-out fit measures",
            "Every row trains; folds choose the cost and γ and give held-out fit measures",
            "Every row trains; each fold is also classified by the rule fitted without it, for held-out fit measures",
            "Every row trains; folds repeat both passes and choose the number of terms, giving held-out fit measures",
            "Every row trains; folds choose the penalty and give held-out fit measures",
            "Every row trains; folds tune each model and give the held-out error that ranks them",
            "Every row trains; each fold is predicted by a process fitted without it, for held-out fit measures",
        ]));
        if self.method == Method::KFold {
            ui.indent("kfold", |ui| {
                ui.horizontal(|ui| {
                    ui.label("Folds:");
                    ui.add(egui::DragValue::new(&mut self.folds).range(2..=50));
                });
            });
        }
        ui.radio_value(&mut self.method, Method::Column, "Roles from a column")
            .on_hover_text("A column holding 0/1/2 or Training/Validation/Test for each row");
        if self.method == Method::Column {
            ui.indent("role_column", |ui| {
                kit::column_choice(ui, "validation_column", table, &mut self.column, |i| !used.contains(&i), None);
            });
        }
        ui.radio_value(&mut self.method, Method::None, "None (all rows train)").on_hover_text(self.hint([
            "The size is still chosen by 10-fold cross-validation, but no held-out measures are shown",
            "Each tree's out-of-bag rows (those its bootstrap sample left out) still give held-out measures",
            "Every round is kept, with no held-out measures",
            "The number of factors is still chosen by 10-fold cross-validation, but no held-out measures are shown",
            "The cost and γ are still tuned by 5-fold cross-validation, but no held-out measures are shown",
            "The classical analysis: the fit measures describe the rows the rule was fitted to (optimistic)",
            "Generalized cross-validation (GCV) chooses the number of terms; no held-out measures are shown",
            "The penalty is still tuned by 5-fold cross-validation, but no held-out measures are shown",
            "The models are tuned and ranked by 5-fold cross-validation",
            "Leave-one-out predictions (exact, from the one fit) give held-out fit measures",
        ]));
    }

    /// Draw the settings most people leave alone (the draw's seed, strata
    /// and groups), for the dialog's More options.
    pub fn more_ui(&mut self, ui: &mut egui::Ui, table: &DataTable, used: &[usize]) {
        if !matches!(self.method, Method::Holdout | Method::KFold) {
            return;
        }
        heading(ui, "Validation rows");
        ui.horizontal(|ui| {
            ui.label(if self.method == Method::KFold { "Seed of the folds:" } else { "Seed of the draw:" });
            ui.add(crate::dialog_kit::seed_value(&mut self.seed));
        });
        if self.method == Method::Holdout {
            ui.checkbox(&mut self.stratify, "Stratify by the response")
                .on_hover_text("Each role gets the same share of every class (quartiles of a continuous response)");
            ui.horizontal(|ui| {
                ui.label("Keep groups together:");
                kit::column_choice(
                    ui,
                    "validation_group",
                    table,
                    &mut self.group,
                    |i| !used.contains(&i),
                    Some("No grouping"),
                );
            });
        }
    }

    /// The method chosen, or what is missing.
    pub fn method(&self, used: &[usize]) -> Result<ValidationMethod, String> {
        Ok(match self.method {
            Method::None => ValidationMethod::None,
            Method::KFold => ValidationMethod::KFold { folds: self.folds, seed: self.seed },
            Method::Column => match self.column {
                None => return Err("Choose the column of validation roles".into()),
                Some(c) if used.contains(&c) => {
                    return Err("The validation role column cannot be the response or a predictor".into());
                }
                Some(c) => ValidationMethod::Column(c),
            },
            Method::Holdout => {
                if self.validation + self.test >= 1.0 {
                    return Err("Leave some rows for training: validation and test shares sum to 1 or more".into());
                }
                ValidationMethod::Holdout {
                    validation: self.validation,
                    test: self.test,
                    seed: self.seed,
                    stratify: self.stratify,
                    group: self.group.filter(|g| !used.contains(g)),
                }
            }
        })
    }
}

/// A section heading inside a dialog.
pub fn heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(4.0);
    ui.label(egui::RichText::new(text).strong());
}

/// Whether column `i` is categorical for a predictive model (text, or a
/// number marked nominal or ordinal).
pub fn is_categorical(table: &DataTable, i: usize) -> bool {
    table.column(i).is_some_and(|c| c.as_numeric().is_none() || c.level != stat_data::MeasurementLevel::Continuous)
}

/// The response choice and predictor list of a predictive model's dialog;
/// `allow` filters the predictors. Returns the predictors chosen (never the
/// response), in table order.
pub fn roles_ui(
    ui: &mut egui::Ui,
    table: &DataTable,
    response: &mut Option<usize>,
    chosen: &mut Vec<bool>,
    predictors_label: &str,
    allow: impl Fn(usize) -> bool,
) -> Vec<usize> {
    ui.horizontal(|ui| {
        ui.label("Response:");
        kit::column_choice(ui, "response", table, response, |_| true, None);
    });
    ui.label(predictors_label);
    let r = *response;
    kit::column_list(ui, "predictors", table, chosen, |i| Some(i) != r && allow(i));
    kit::chosen_columns(chosen).into_iter().filter(|&i| i < table.n_cols() && Some(i) != r && allow(i)).collect()
}

/// The response and the validation method, or what is missing.
pub fn check_roles(
    response: Option<usize>,
    predictors: &[usize],
    validation: &ValidationChoice,
) -> Result<(usize, ValidationMethod), String> {
    let Some(response) = response else { return Err("Choose a response".into()) };
    if predictors.is_empty() {
        return Err("Choose at least one predictor".into());
    }
    let used: Vec<usize> = std::iter::once(response).chain(predictors.iter().copied()).collect();
    Ok((response, validation.method(&used)?))
}

/// The response and predictors as the columns a [`ValidationChoice`] must
/// not use.
pub fn used_columns(response: Option<usize>, predictors: &[usize]) -> Vec<usize> {
    response.into_iter().chain(predictors.iter().copied()).collect()
}

#[derive(Clone)]
pub struct TreeDialog {
    edit: Option<u64>,
    response: Option<usize>,
    chosen: Vec<bool>,
    validation: ValidationChoice,
    criterion: Criterion,
    min_split: usize,
    min_leaf: usize,
    max_depth: usize,
    pruning: Pruning,
}

impl TreeDialog {
    pub fn new(table: &DataTable) -> Self {
        let d = TreeRequest::new(0, Vec::new());
        Self {
            edit: None,
            response: None,
            chosen: vec![false; table.n_cols()],
            validation: ValidationChoice::default(),
            criterion: d.criterion,
            min_split: d.min_split,
            min_leaf: d.min_leaf,
            max_depth: d.max_depth,
            pruning: d.pruning,
        }
    }

    /// The dialog filled from a saved decision tree.
    pub fn from_saved(saved: &SavedReport, table: &DataTable) -> Option<Self> {
        let SavedReport::DecisionTree { request: r, .. } = saved else { return None };
        Some(Self {
            edit: None,
            response: kit::valid(table, r.response),
            chosen: kit::mask(table.n_cols(), &r.predictors),
            validation: ValidationChoice::default().with_method(&r.validation, table),
            criterion: r.criterion,
            min_split: r.min_split,
            min_leaf: r.min_leaf,
            max_depth: r.max_depth,
            pruning: r.pruning,
        })
    }

    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<TreeRequest> {
        let dialog = Dialog::new("Decision tree").editing(self.edit);
        match dialog.show(ctx, |ui| self.contents(ui, table)) {
            Outcome::Reset => {
                *self = Self { edit: self.edit, ..Self::new(table) };
                Outcome::Open
            }
            outcome => outcome,
        }
    }

    fn contents(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<TreeRequest> {
        kit::note(
            ui,
            "Splits the rows again and again on the predictor that best separates the response, then prunes \
             the tree back to the size that predicts held-out rows best. A categorical response gives a \
             classification tree; a continuous one a regression tree.",
        );
        let predictors = roles_ui(
            ui,
            table,
            &mut self.response,
            &mut self.chosen,
            "Predictors (missing values follow surrogate splits):",
            |_| true,
        );
        let used = used_columns(self.response, &predictors);
        self.validation.ui(ui, table, &used);
        let classification = self.response.is_some_and(|r| is_categorical(table, r));
        kit::more_options(ui, |ui| {
            heading(ui, "Growth and pruning");
            ui.add_enabled_ui(classification, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Split criterion:");
                    ui.radio_value(&mut self.criterion, Criterion::Gini, "Gini impurity");
                    ui.radio_value(&mut self.criterion, Criterion::Entropy, "Entropy");
                });
            })
            .response
            .on_disabled_hover_text("A regression tree splits to reduce the sum of squares");
            ui.horizontal(|ui| {
                ui.label("Split nodes of at least");
                ui.add(egui::DragValue::new(&mut self.min_split).range(2..=10_000));
                ui.label("rows; leaves of at least");
                ui.add(egui::DragValue::new(&mut self.min_leaf).range(1..=5_000));
            });
            ui.horizontal(|ui| {
                ui.label("Depth up to");
                ui.add(egui::DragValue::new(&mut self.max_depth).range(1..=30));
            });
            ui.horizontal(|ui| {
                ui.label("Pruned size:");
                egui::ComboBox::from_id_salt("tree_pruning").selected_text(self.pruning.label()).show_list(ui, |ui| {
                    for p in Pruning::ALL {
                        ui.selectable_value(&mut self.pruning, p, p.label());
                    }
                });
            });
            self.validation.more_ui(ui, table, &used);
        });
        let (response, validation) = check_roles(self.response, &predictors, &self.validation)?;
        let summary = format!(
            "{} tree of {} on {}",
            if classification { "Classification" } else { "Regression" },
            kit::name(table, response),
            kit::count(predictors.len(), "predictor", "predictors")
        );
        kit::ready(
            TreeRequest {
                response,
                predictors,
                validation,
                criterion: self.criterion,
                min_split: self.min_split,
                min_leaf: self.min_leaf,
                max_depth: self.max_depth,
                pruning: self.pruning,
            },
            summary,
        )
    }
}

impl Remember for TreeDialog {
    fn memory_key(&self) -> String {
        "Decision tree".into()
    }

    fn set_edit(&mut self, report: Option<u64>) {
        self.edit = report;
    }

    fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        self.chosen = kit::mask(table.n_cols(), columns);
    }
}
