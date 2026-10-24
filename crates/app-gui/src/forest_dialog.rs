//! Dialog for a random forest: response, predictors, validation, the number
//! of trees, predictors tried per split, bootstrap sample size and tree
//! limits.

use app_model::{Criterion, ForestRequest, SavedReport};
use eframe::egui::{self, RichText};
use stat_data::DataTable;

use crate::dialog_kit::{self as kit, Dialog, Outcome, Readiness, Remember};
use crate::tree_dialog::{ValidationChoice, check_roles, heading, is_categorical, roles_ui, used_columns};

#[derive(Clone)]
pub struct ForestDialog {
    edit: Option<u64>,
    response: Option<usize>,
    chosen: Vec<bool>,
    validation: ValidationChoice,
    trees: usize,
    /// `None`: the default (√p for classification, p/3 for regression).
    features_per_split: Option<usize>,
    sample_fraction: f64,
    criterion: Criterion,
    min_split: usize,
    min_leaf: usize,
    max_depth: usize,
    seed: u64,
}

impl ForestDialog {
    pub fn new(table: &DataTable) -> Self {
        let d = ForestRequest::new(0, Vec::new());
        Self {
            edit: None,
            response: None,
            chosen: vec![false; table.n_cols()],
            validation: ValidationChoice::for_forest(),
            trees: d.trees,
            features_per_split: None,
            sample_fraction: d.sample_fraction,
            criterion: d.criterion,
            min_split: d.min_split,
            min_leaf: d.min_leaf,
            max_depth: d.max_depth,
            seed: d.seed,
        }
    }

    /// The dialog filled from a saved random forest.
    pub fn from_saved(saved: &SavedReport, table: &DataTable) -> Option<Self> {
        let SavedReport::RandomForest { request: r, .. } = saved else { return None };
        Some(Self {
            edit: None,
            response: kit::valid(table, r.response),
            chosen: kit::mask(table.n_cols(), &r.predictors),
            validation: ValidationChoice::for_forest().with_method(&r.validation, table),
            trees: r.trees,
            features_per_split: (r.features_per_split > 0).then_some(r.features_per_split),
            sample_fraction: r.sample_fraction,
            criterion: r.criterion,
            min_split: r.min_split,
            min_leaf: r.min_leaf,
            max_depth: r.max_depth,
            seed: r.seed,
        })
    }

    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<ForestRequest> {
        let dialog = Dialog::new("Random forest").editing(self.edit);
        match dialog.show(ctx, |ui| self.contents(ui, table)) {
            Outcome::Reset => {
                *self = Self { edit: self.edit, ..Self::new(table) };
                Outcome::Open
            }
            outcome => outcome,
        }
    }

    fn contents(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<ForestRequest> {
        kit::note(
            ui,
            "Grows many unpruned trees, each on a bootstrap sample of the training rows and trying a random \
             subset of predictors at every split, and averages their predictions. Rows a tree's sample left \
             out (out-of-bag rows) give honest error estimates and variable importance.",
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
        ui.horizontal(|ui| {
            ui.label("Trees:");
            ui.add(egui::DragValue::new(&mut self.trees).range(1..=5_000));
        });
        self.validation.ui(ui, table, &used);
        let classification = self.response.is_some_and(|r| is_categorical(table, r));
        kit::more_options(ui, |ui| {
            heading(ui, "Forest");
            ui.horizontal(|ui| {
                ui.label("Bootstrap sample (share of training rows):");
                ui.add(egui::DragValue::new(&mut self.sample_fraction).speed(0.01).range(0.05..=1.0));
            });
            ui.horizontal(|ui| {
                let p = predictors.len().max(1);
                let default = if classification { ((p as f64).sqrt().floor() as usize).max(1) } else { (p / 3).max(1) };
                let mut custom = self.features_per_split.is_some();
                ui.label("Predictors tried per split:");
                ui.checkbox(&mut custom, "Set").on_hover_text(
                    "Off: the square root of the predictor count for a categorical response, a third of it for a \
                     continuous one",
                );
                if custom {
                    let n = self.features_per_split.get_or_insert(default);
                    ui.add(egui::DragValue::new(n).range(1..=p));
                } else {
                    self.features_per_split = None;
                    ui.label(RichText::new(format!("{default} (default)")).weak());
                }
            });
            ui.add_enabled_ui(classification, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Split criterion:");
                    ui.radio_value(&mut self.criterion, Criterion::Gini, "Gini impurity");
                    ui.radio_value(&mut self.criterion, Criterion::Entropy, "Entropy");
                });
            })
            .response
            .on_disabled_hover_text("Regression trees split to reduce the sum of squares");
            ui.horizontal(|ui| {
                ui.label("Split nodes of at least");
                ui.add(egui::DragValue::new(&mut self.min_split).range(2..=10_000));
                ui.label("rows; leaves of at least");
                ui.add(egui::DragValue::new(&mut self.min_leaf).range(1..=5_000));
            });
            ui.horizontal(|ui| {
                ui.label("Depth up to");
                ui.add(egui::DragValue::new(&mut self.max_depth).range(1..=30));
                ui.label("Seed:");
                ui.add(crate::dialog_kit::seed_value(&mut self.seed));
            });
            self.validation.more_ui(ui, table, &used);
        });
        let (response, validation) = check_roles(self.response, &predictors, &self.validation)?;
        let summary = format!(
            "Random forest of {} on {}, {}",
            kit::name(table, response),
            kit::count(predictors.len(), "predictor", "predictors"),
            kit::count(self.trees, "tree", "trees")
        );
        kit::ready(
            ForestRequest {
                response,
                predictors,
                validation,
                trees: self.trees,
                features_per_split: self.features_per_split.unwrap_or(0),
                sample_fraction: self.sample_fraction,
                min_split: self.min_split,
                min_leaf: self.min_leaf,
                max_depth: self.max_depth,
                criterion: self.criterion,
                seed: self.seed,
            },
            summary,
        )
    }
}

impl Remember for ForestDialog {
    fn memory_key(&self) -> String {
        "Random forest".into()
    }

    fn set_edit(&mut self, report: Option<u64>) {
        self.edit = report;
    }

    fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        self.chosen = kit::mask(table.n_cols(), columns);
    }
}
