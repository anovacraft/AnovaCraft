//! Data ▸ Impute missing values: columns, a method and its settings. The
//! two multiple-imputation methods add the number of imputations, a
//! stacked table of them, and (chained equations) each column's model, the
//! predictor matrix and the visit order.

use crate::dialog_kit::ComboExt as _;
use app_model::multiple_imputation::{FcsMethod, METHODS, MiEngine, MiSettings, method_label};
use app_model::{ImputationMethod, ImputationRequest, SavedReport};
use eframe::egui;
use stat_data::{DataTable, MeasurementLevel};

use crate::dialog_kit::{self as kit, Dialog, Outcome, Readiness, Remember};

#[derive(Clone)]
pub struct ImputationDialog {
    /// The report being edited (`None`: a new report).
    edit: Option<u64>,
    chosen: Vec<bool>,
    method: ImputationMethod,
    /// Choose the rank on held-out values (else `rank`).
    auto_rank: bool,
    rank: usize,
    shrinkage: f64,
    regularize: bool,
    standardize: bool,
    max_iterations: usize,
    tolerance: f64,
    seed: u64,
    /// Multiple imputation: number of imputations.
    imputations: usize,
    /// Iterations of each chain.
    mi_iterations: usize,
    /// Donors for predictive mean matching.
    donors: usize,
    /// Each table column's chained-equations model (`None`: the default).
    methods: Vec<Option<FcsMethod>>,
    /// `(imputed, predictor)` table-column pairs left out of the predictor
    /// matrix.
    dropped: Vec<(usize, usize)>,
    /// Table columns in the order chained equations visits them (columns
    /// not listed follow in table order).
    order: Vec<usize>,
    /// Also make the stacked table when the report is made.
    make_table: bool,
}

/// What the dialog runs: the report, and whether to make its stacked table.
pub struct ImputationRun {
    pub request: ImputationRequest,
    pub stacked_table: bool,
}

impl ImputationDialog {
    pub fn new(table: &DataTable) -> Self {
        let d = ImputationRequest::new(Vec::new());
        Self {
            edit: None,
            chosen: vec![false; table.n_cols()],
            method: d.method,
            auto_rank: true,
            rank: 2,
            shrinkage: d.shrinkage,
            regularize: d.regularize,
            standardize: d.standardize,
            max_iterations: d.max_iterations,
            tolerance: d.tolerance,
            seed: d.seed,
            imputations: d.multiple.imputations,
            mi_iterations: d.multiple.iterations,
            donors: d.multiple.donors,
            methods: vec![None; table.n_cols()],
            dropped: Vec::new(),
            order: Vec::new(),
            make_table: true,
        }
    }

    /// The dialog filled from a saved imputation report.
    pub fn from_saved(saved: &SavedReport, table: &DataTable) -> Option<Self> {
        let SavedReport::Imputation { request } = saved else { return None };
        let mi = &request.multiple;
        let mut methods = vec![None; table.n_cols()];
        for (&c, m) in request.columns.iter().zip(&mi.methods) {
            if let Some(slot) = methods.get_mut(c) {
                *slot = *m;
            }
        }
        let mut dropped = Vec::new();
        for (j, row) in mi.predictors.iter().flatten().enumerate() {
            for (k, &on) in row.iter().enumerate() {
                if j != k && !on {
                    dropped.push((request.columns[j], request.columns[k]));
                }
            }
        }
        let order = mi.visit_order.iter().flatten().filter_map(|&j| request.columns.get(j).copied()).collect();
        Some(Self {
            imputations: mi.imputations,
            mi_iterations: mi.iterations,
            donors: mi.donors,
            methods,
            dropped,
            order,
            make_table: false,
            chosen: kit::mask(table.n_cols(), &request.columns),
            method: request.method,
            auto_rank: request.rank.is_none(),
            rank: request.rank.unwrap_or(2),
            shrinkage: request.shrinkage,
            regularize: request.regularize,
            standardize: request.standardize,
            max_iterations: request.max_iterations,
            tolerance: request.tolerance,
            seed: request.seed,
            ..Self::new(table)
        })
    }

    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<ImputationRun> {
        let outcome = Dialog::new("Impute missing values").editing(self.edit).show(ctx, |ui| {
            ui.set_max_width(kit::em(ui) * 34.0);
            kit::note(
                ui,
                "Fill in the missing values of columns from the values each row does have. The report shows \
                 where values are missing; save the completed columns from its options menu. Multiple \
                 imputation makes several completed copies whose differences carry the uncertainty about the \
                 missing values, for analyses pooled across them.",
            );
            ui.add_space(4.0);
            self.contents(ui, table)
        });
        match outcome {
            Outcome::Reset => {
                *self = Self { edit: self.edit, ..Self::new(table) };
                Outcome::Open
            }
            outcome => outcome,
        }
    }

    fn contents(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<ImputationRun> {
        let method = self.method;
        let usable = |i: usize| usable_column(table, i, method);
        ui.label(if method == ImputationMethod::ChainedEquations {
            "Columns (numeric or categorical):"
        } else {
            "Columns (numeric):"
        });
        kit::column_list(ui, "imputation_columns", table, &mut self.chosen, usable);
        let columns: Vec<usize> = kit::chosen_columns(&self.chosen).into_iter().filter(|&i| usable(i)).collect();
        ui.separator();
        ui.label("Method:");
        for m in ImputationMethod::ALL {
            let hint = match m {
                ImputationMethod::NormalEm => {
                    "Fit a multivariate normal distribution by maximum likelihood (EM algorithm) and fill each \
                     value with its expected value given the row's other values"
                }
                ImputationMethod::LowRank => {
                    "Fit the table by its column means plus a few principal directions (singular value \
                     decomposition), refitting until the filled-in values settle"
                }
                ImputationMethod::Mean => {
                    "Fill with the column's mean (keeps the mean, shrinks spread and correlation)"
                }
                ImputationMethod::Median => "Fill with the column's median",
                ImputationMethod::ChainedEquations => {
                    "Make several completed copies, filling one column at a time from a model of it given the \
                     others (predictive mean matching, Bayesian regression, logistic or multinomial models) and \
                     cycling until the values settle. Takes continuous and categorical columns."
                }
                ImputationMethod::NormalModel => {
                    "Make several completed copies by drawing the missing values and the parameters of a \
                     multivariate normal distribution in turn (data augmentation), starting from the EM fit. \
                     Continuous columns only."
                }
            };
            ui.radio_value(&mut self.method, m, m.label()).on_hover_text(hint);
        }
        if self.method.is_multiple() {
            self.multiple_settings(ui, table, &columns);
        }
        let iterative = matches!(self.method, ImputationMethod::NormalEm | ImputationMethod::LowRank);
        if self.method == ImputationMethod::LowRank {
            ui.horizontal(|ui| {
                ui.label("Rank:");
                ui.radio_value(&mut self.auto_rank, true, "Choose on held-out values").on_hover_text(
                    "Hide 10% of the observed values and keep the rank (1 to 10) that predicts them best",
                );
                ui.radio_value(&mut self.auto_rank, false, "Fixed:");
                let most = columns.len().saturating_sub(1).max(1);
                ui.add_enabled(!self.auto_rank, egui::DragValue::new(&mut self.rank).range(1..=most));
            });
        }
        if iterative {
            kit::more_options(ui, |ui| {
                if self.method == ImputationMethod::LowRank {
                    ui.add_enabled(
                        self.shrinkage == 0.0,
                        egui::Checkbox::new(&mut self.regularize, "Regularize the fit (recommended)"),
                    )
                    .on_hover_text(
                        "Shrink the singular values by the estimated noise variance (regularized iterative PCA), \
                         so the refitting settles instead of drifting. A shrinkage λ above 0 replaces it.",
                    );
                    ui.horizontal(|ui| {
                        ui.label("Shrinkage λ of the singular values:");
                        ui.add(egui::DragValue::new(&mut self.shrinkage).range(0.0..=1e6).speed(0.05));
                    })
                    .response
                    .on_hover_text("0 keeps the leading singular values as they are (soft-impute when above 0)");
                    ui.checkbox(&mut self.standardize, "Standardize the columns first");
                    if self.auto_rank {
                        ui.horizontal(|ui| {
                            ui.label("Seed for the held-out values:");
                            ui.add(crate::dialog_kit::seed_value(&mut self.seed));
                        });
                    }
                }
                ui.horizontal(|ui| {
                    ui.label("Most iterations:");
                    ui.add(egui::DragValue::new(&mut self.max_iterations).range(1..=100_000));
                });
                ui.horizontal(|ui| {
                    ui.label("Convergence tolerance:");
                    ui.add(egui::DragValue::new(&mut self.tolerance).range(1e-14..=1e-2).speed(1e-9));
                });
            });
        }
        let min = if self.method == ImputationMethod::LowRank { 2 } else { 1 };
        if columns.len() < min {
            return Err(if min == 2 { "Choose at least two columns" } else { "Choose at least one column" }.into());
        }
        if self.method == ImputationMethod::LowRank && !self.auto_rank && self.rank >= columns.len() {
            return Err("Make the rank smaller than the number of columns".into());
        }
        let missing: usize = columns
            .iter()
            .filter_map(|&c| table.column(c))
            .map(|c| (0..table.n_rows()).filter(|&r| c.cell_text(r).trim().is_empty()).count())
            .sum();
        if self.method.is_multiple() && missing == 0 {
            return Err("The chosen columns have no missing values".into());
        }
        let request = ImputationRequest {
            method: self.method,
            rank: (!self.auto_rank).then_some(self.rank),
            shrinkage: self.shrinkage,
            regularize: self.regularize,
            standardize: self.standardize,
            max_iterations: self.max_iterations,
            tolerance: self.tolerance,
            seed: self.seed,
            multiple: self.mi_settings(&columns),
            ..ImputationRequest::new(columns.clone())
        };
        let mut summary = format!(
            "{} of {}, {} missing",
            self.method.label(),
            kit::count(columns.len(), "column", "columns"),
            kit::count(missing, "value", "values")
        );
        if self.method.is_multiple() {
            summary.push_str(&format!(", {} imputations", self.imputations));
        }
        let stacked_table = self.method.is_multiple() && self.make_table;
        kit::ready(ImputationRun { request, stacked_table }, summary)
    }

    /// The multiple-imputation settings for the chosen `columns`.
    fn mi_settings(&self, columns: &[usize]) -> MiSettings {
        let engine = if self.method == ImputationMethod::NormalModel {
            MiEngine::NormalModel
        } else {
            MiEngine::ChainedEquations
        };
        let methods: Vec<Option<FcsMethod>> = columns.iter().map(|&c| self.methods.get(c).copied().flatten()).collect();
        let predictors = columns.iter().any(|&j| columns.iter().any(|&k| self.dropped.contains(&(j, k)))).then(|| {
            columns
                .iter()
                .map(|&j| columns.iter().map(|&k| j != k && !self.dropped.contains(&(j, k))).collect())
                .collect()
        });
        let visit = self.visit_order(columns);
        let natural: Vec<usize> = (0..columns.len()).collect();
        MiSettings {
            engine,
            imputations: self.imputations,
            iterations: self.mi_iterations,
            donors: self.donors,
            methods: if methods.iter().any(Option::is_some) { methods } else { Vec::new() },
            predictors,
            visit_order: (visit != natural).then_some(visit),
        }
    }

    /// Positions in `columns` in the order they are visited.
    fn visit_order(&self, columns: &[usize]) -> Vec<usize> {
        let mut out: Vec<usize> = self.order.iter().filter_map(|c| columns.iter().position(|x| x == c)).collect();
        out.dedup();
        for j in 0..columns.len() {
            if !out.contains(&j) {
                out.push(j);
            }
        }
        out
    }

    fn multiple_settings(&mut self, ui: &mut egui::Ui, table: &DataTable, columns: &[usize]) {
        let chained = self.method == ImputationMethod::ChainedEquations;
        let engine = if chained { MiEngine::ChainedEquations } else { MiEngine::NormalModel };
        ui.horizontal(|ui| {
            ui.label("Imputations:");
            ui.add(egui::DragValue::new(&mut self.imputations).range(2..=MiSettings::MAX_IMPUTATIONS))
                .on_hover_text("Number of completed copies (m); 20 or more keeps pooled p-values stable");
            ui.label("Iterations:");
            if ui
                .add(egui::DragValue::new(&mut self.mi_iterations).range(1..=1000))
                .on_hover_text("Cycles of each chain before its copy is taken; check the trace plots")
                .changed()
                && self.mi_iterations == 0
            {
                self.mi_iterations = engine.default_iterations();
            }
        });
        ui.checkbox(&mut self.make_table, "Make the stacked table").on_hover_text(
            "Also add a table to the project with the original rows (Imputation 0) and every completed copy under \
             them. Reports on it pool their results across the imputations.",
        );
        kit::more_options(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Seed:");
                ui.add(crate::dialog_kit::seed_value(&mut self.seed))
                    .on_hover_text("The same seed makes the same imputations again");
            });
            if !chained {
                return;
            }
            ui.horizontal(|ui| {
                ui.label("Donors for predictive mean matching:");
                ui.add(egui::DragValue::new(&mut self.donors).range(1..=50));
            });
            if columns.is_empty() {
                return;
            }
            ui.separator();
            ui.label("Model of each column:");
            egui::Grid::new("mi_column_methods").num_columns(2).show(ui, |ui| {
                for &c in columns {
                    let Some(col) = table.column(c) else { continue };
                    ui.label(&col.name);
                    let kind = column_kind(table, c);
                    let current = self.methods[c];
                    let shown = current.map_or("Default", method_label);
                    egui::ComboBox::from_id_salt(("mi_method", c)).selected_text(shown).show_list(ui, |ui| {
                        ui.selectable_value(&mut self.methods[c], None, "Default").on_hover_text(
                            "Predictive mean matching for continuous columns, logistic regression for two \
                             categories, multinomial logit for more; not imputed without missing values",
                        );
                        for m in METHODS.into_iter().filter(|m| kind.is_none_or(|k| m.suits(k))) {
                            ui.selectable_value(&mut self.methods[c], Some(m), method_label(m));
                        }
                    });
                    ui.end_row();
                }
            });
            if columns.len() >= 2 {
                ui.separator();
                ui.label("Predictors (row: column imputed; tick the columns that predict it):")
                    .on_hover_text("Every other chosen column predicts each column unless unticked here");
                egui::Grid::new("mi_predictors").show(ui, |ui| {
                    ui.label("");
                    for &c in columns {
                        ui.label(short(&table.column(c).map_or(String::new(), |c| c.name.clone())));
                    }
                    ui.end_row();
                    for &j in columns {
                        let name = table.column(j).map_or(String::new(), |c| c.name.clone());
                        ui.label(&name);
                        for &k in columns {
                            if j == k {
                                ui.label("—");
                                continue;
                            }
                            let mut on = !self.dropped.contains(&(j, k));
                            let predictor = table.column(k).map_or(String::new(), |c| c.name.clone());
                            let r = ui.checkbox(&mut on, "").on_hover_text(format!("{predictor} predicts {name}"));
                            let label = format!("{predictor} predicts {name}");
                            r.widget_info(|| {
                                egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, on, label.clone())
                            });
                            if r.changed() {
                                if on {
                                    self.dropped.retain(|p| *p != (j, k));
                                } else {
                                    self.dropped.push((j, k));
                                }
                            }
                        }
                        ui.end_row();
                    }
                });
                ui.separator();
                ui.label("Visit order:");
                let order: Vec<usize> = self.visit_order(columns).into_iter().map(|j| columns[j]).collect();
                let mut moved = None;
                for (i, &c) in order.iter().enumerate() {
                    ui.horizontal(|ui| {
                        let name = table.column(c).map_or(String::new(), |c| c.name.clone());
                        if ui
                            .add_enabled(i > 0, egui::Button::new("▲").small())
                            .on_hover_text(format!("Visit {name} earlier"))
                            .clicked()
                        {
                            moved = Some((i, i - 1));
                        }
                        if ui
                            .add_enabled(i + 1 < order.len(), egui::Button::new("▼").small())
                            .on_hover_text(format!("Visit {name} later"))
                            .clicked()
                        {
                            moved = Some((i, i + 1));
                        }
                        ui.label(format!("{}. {name}", i + 1));
                    });
                }
                if let Some((a, b)) = moved {
                    let mut order = order;
                    order.swap(a, b);
                    self.order = order;
                }
            }
        });
    }
}

/// A column header of the predictor matrix: the name, cut to stay narrow.
fn short(name: &str) -> String {
    let cut: String = name.chars().take(10).collect();
    if cut.chars().count() < name.chars().count() { format!("{cut}…") } else { cut }
}

/// Whether column `i` can be imputed by `method`: numeric columns for the
/// single-value methods, continuous ones for the multivariate normal model,
/// any but date-time columns for chained equations.
fn usable_column(table: &DataTable, i: usize, method: ImputationMethod) -> bool {
    let Some(col) = table.column(i) else { return false };
    match method {
        ImputationMethod::ChainedEquations => !matches!(col.data, stat_data::ColumnData::DateTime { .. }),
        ImputationMethod::NormalModel => col.as_numeric().is_some() && col.level == MeasurementLevel::Continuous,
        _ => kit::is_numeric(table, i),
    }
}

/// The kind chained equations gives column `c` (`None` when it has fewer
/// than two categories, which it reports itself).
fn column_kind(table: &DataTable, c: usize) -> Option<app_model::multiple_imputation::ColumnKind> {
    use app_model::multiple_imputation::ColumnKind;
    let col = table.column(c)?;
    if col.as_numeric().is_some() && col.level == MeasurementLevel::Continuous {
        return Some(ColumnKind::Continuous);
    }
    let mut levels: Vec<String> =
        (0..table.n_rows()).map(|r| col.cell_text(r).trim().to_owned()).filter(|t| !t.is_empty()).collect();
    levels.sort();
    levels.dedup();
    (levels.len() >= 2).then_some(ColumnKind::Categorical { levels: levels.len() })
}

impl Remember for ImputationDialog {
    fn memory_key(&self) -> String {
        "Impute missing values".to_owned()
    }

    fn set_edit(&mut self, report: Option<u64>) {
        self.edit = report;
    }

    fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        self.chosen = kit::mask(table.n_cols(), columns);
        self.methods.resize(table.n_cols(), None);
    }
}
