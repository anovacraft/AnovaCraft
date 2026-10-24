//! Dialogs for Gaussian mixture clustering, latent class analysis and
//! dimension reduction (classical and metric scaling, t-SNE, UMAP) in the
//! Multivariate menu.

use app_model::{
    CovarianceStructure, EmbeddingMethod, EmbeddingRequest, GaussianMixtureRequest, LatentClassRequest, MixtureStart,
    SavedReport,
};
use eframe::egui;
use stat_data::DataTable;

use crate::dialog_kit::{self as kit, Dialog, Outcome, Readiness, Remember};
use crate::report_view::ReportRequest;

/// Which analysis the dialog sets up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapKind {
    GaussianMixture,
    LatentClass,
    Embedding,
}

#[derive(Clone)]
pub struct MapDialog {
    /// The report being edited (`None`: a new report).
    edit: Option<u64>,
    kind: MapKind,
    chosen: Vec<bool>,
    // Gaussian mixture settings.
    structures: [bool; 6],
    min_components: usize,
    max_components: usize,
    standardize_mixture: bool,
    start: MixtureStart,
    starts: usize,
    // Latent class settings.
    min_classes: usize,
    max_classes: usize,
    class_starts: usize,
    smoothing: f64,
    // Embedding settings.
    method: EmbeddingMethod,
    distances: bool,
    standardize: bool,
    dims: usize,
    perplexity: f64,
    neighbours: usize,
    min_dist: f64,
    custom_iterations: bool,
    iterations: usize,
    color: Option<usize>,
    seed: u64,
}

impl MapDialog {
    pub fn new(kind: MapKind, table: &DataTable) -> Self {
        Self {
            edit: None,
            kind,
            chosen: vec![false; table.n_cols()],
            structures: [true; 6],
            min_components: 1,
            max_components: GaussianMixtureRequest::DEFAULT_MAX,
            standardize_mixture: false,
            start: MixtureStart::KMeans,
            starts: 5,
            min_classes: 1,
            max_classes: LatentClassRequest::DEFAULT_MAX,
            class_starts: 10,
            smoothing: 0.0,
            method: EmbeddingMethod::Tsne,
            distances: false,
            standardize: true,
            dims: 2,
            perplexity: 30.0,
            neighbours: 15,
            min_dist: 0.1,
            custom_iterations: false,
            iterations: 1000,
            color: None,
            seed: 1,
        }
    }

    /// The dialog filled from a saved Gaussian mixture or embedding report.
    pub fn from_saved(saved: &SavedReport, table: &DataTable) -> Option<Self> {
        let n = table.n_cols();
        Some(match saved {
            SavedReport::GaussianMixture { request: r } => Self {
                chosen: kit::mask(n, &r.columns),
                structures: CovarianceStructure::ALL.map(|s| r.structures.contains(&s)),
                min_components: r.min_components,
                max_components: r.max_components,
                standardize_mixture: r.standardize,
                start: r.start,
                starts: r.starts,
                seed: r.seed,
                ..Self::new(MapKind::GaussianMixture, table)
            },
            SavedReport::LatentClass { request: r } => Self {
                chosen: kit::mask(n, &r.items),
                min_classes: r.min_classes,
                max_classes: r.max_classes,
                class_starts: r.starts,
                smoothing: r.smoothing,
                seed: r.seed,
                ..Self::new(MapKind::LatentClass, table)
            },
            SavedReport::Embedding { request: r } => Self {
                chosen: kit::mask(n, &r.columns),
                method: r.method,
                distances: r.distances,
                standardize: r.standardize,
                dims: r.dims,
                perplexity: r.perplexity,
                neighbours: r.neighbours,
                min_dist: r.min_dist,
                custom_iterations: r.iterations.is_some(),
                iterations: r.iterations.unwrap_or(1000),
                color: r.color,
                seed: r.seed,
                ..Self::new(MapKind::Embedding, table)
            },
            _ => return None,
        })
    }

    fn title(&self) -> &'static str {
        match self.kind {
            MapKind::GaussianMixture => "Gaussian mixture clustering",
            MapKind::LatentClass => "Latent class analysis",
            MapKind::Embedding => "Dimension reduction",
        }
    }

    fn intro(&self) -> &'static str {
        match self.kind {
            MapKind::GaussianMixture => {
                "Groups the rows as a mixture of normal distributions, each with its own mean and spread. \
                 Several covariance shapes and numbers of groups are fitted; the smallest BIC chooses the model."
            }
            MapKind::LatentClass => {
                "Finds hidden classes of rows from several categorical items, assuming the items are \
                 independent within a class. Each number of classes in the range is fitted; the smallest BIC \
                 chooses the model. Blank answers are skipped."
            }
            MapKind::Embedding => {
                "Places the rows on a two-dimensional map so that similar rows lie close together: \
                 multidimensional scaling keeps distances, t-SNE and UMAP keep neighbourhoods."
            }
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<ReportRequest> {
        let dialog = Dialog::new(self.title()).editing(self.edit);
        let outcome = dialog.show(ctx, |ui| {
            ui.set_max_width(kit::em(ui) * 32.0);
            kit::note(ui, self.intro());
            ui.add_space(4.0);
            match self.kind {
                MapKind::GaussianMixture => self.mixture(ui, table),
                MapKind::LatentClass => self.latent_class(ui, table),
                MapKind::Embedding => self.embedding(ui, table),
            }
        });
        match outcome {
            Outcome::Reset => {
                *self = Self { edit: self.edit, ..Self::new(self.kind, table) };
                Outcome::Open
            }
            outcome => outcome,
        }
    }

    fn mixture(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<ReportRequest> {
        ui.label("Columns:");
        kit::column_list(ui, "mixture_columns", table, &mut self.chosen, |i| kit::is_numeric(table, i));
        let columns: Vec<usize> =
            kit::chosen_columns(&self.chosen).into_iter().filter(|&i| kit::is_numeric(table, i)).collect();
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Numbers of components from");
            ui.add(egui::DragValue::new(&mut self.min_components).range(1..=30));
            ui.label("to");
            ui.add(egui::DragValue::new(&mut self.max_components).range(1..=30));
        });
        ui.label("Covariance structures to compare:");
        egui::Grid::new("mixture_structures").num_columns(2).show(ui, |ui| {
            for (i, s) in CovarianceStructure::ALL.into_iter().enumerate() {
                ui.checkbox(&mut self.structures[i], format!("{} — {}", s.key(), s.label().to_lowercase()));
                if i % 2 == 1 {
                    ui.end_row();
                }
            }
        });
        kit::more_options(ui, |ui| {
            ui.checkbox(&mut self.standardize_mixture, "Standardize columns before fitting")
                .on_hover_text("Estimates are still reported in the columns' own units");
            ui.horizontal(|ui| {
                ui.label("First start from:");
                for s in [MixtureStart::KMeans, MixtureStart::Hierarchical] {
                    ui.radio_value(&mut self.start, s, s.label());
                }
            });
            ui.horizontal(|ui| {
                ui.label("Starts per model:");
                ui.add(egui::DragValue::new(&mut self.starts).range(1..=50));
                ui.label("Random seed:");
                ui.add(crate::dialog_kit::seed_value(&mut self.seed));
            });
        });
        if columns.is_empty() {
            return Err("Choose at least one column".into());
        }
        let structures: Vec<CovarianceStructure> =
            CovarianceStructure::ALL.into_iter().zip(self.structures).filter(|(_, on)| *on).map(|(s, _)| s).collect();
        if structures.is_empty() {
            return Err("Tick at least one covariance structure".into());
        }
        if self.min_components > self.max_components {
            return Err("Make the smallest number of components no larger than the largest".into());
        }
        let request = GaussianMixtureRequest {
            structures,
            min_components: self.min_components,
            max_components: self.max_components,
            standardize: self.standardize_mixture,
            start: self.start,
            starts: self.starts,
            seed: self.seed,
            ..GaussianMixtureRequest::new(columns.clone())
        };
        let summary = format!(
            "Gaussian mixtures of {}, {} to {} components",
            kit::count(columns.len(), "column", "columns"),
            self.min_components,
            self.max_components
        );
        kit::ready(ReportRequest::GaussianMixture(request), summary)
    }

    fn latent_class(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<ReportRequest> {
        ui.label("Items (nominal or ordinal columns):");
        kit::column_list(ui, "latent_class_items", table, &mut self.chosen, |i| !kit::is_continuous(table, i));
        let items: Vec<usize> =
            kit::chosen_columns(&self.chosen).into_iter().filter(|&i| !kit::is_continuous(table, i)).collect();
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Numbers of classes from");
            ui.add(egui::DragValue::new(&mut self.min_classes).range(1..=20));
            ui.label("to");
            ui.add(egui::DragValue::new(&mut self.max_classes).range(1..=20));
        });
        kit::more_options(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Random starts per number of classes:");
                ui.add(egui::DragValue::new(&mut self.class_starts).range(1..=100));
            });
            ui.horizontal(|ui| {
                ui.label("Smoothing (added to every level count):");
                ui.add(egui::DragValue::new(&mut self.smoothing).range(0.0..=10.0).speed(0.05))
                    .on_hover_text("0 gives maximum likelihood; a small value keeps probabilities off 0 and 1");
            });
            ui.horizontal(|ui| {
                ui.label("Random seed:");
                ui.add(crate::dialog_kit::seed_value(&mut self.seed));
            });
        });
        if items.len() < 2 {
            return Err("Choose at least two items".into());
        }
        if self.min_classes > self.max_classes {
            return Err("Make the smallest number of classes no larger than the largest".into());
        }
        let request = LatentClassRequest {
            min_classes: self.min_classes,
            max_classes: self.max_classes,
            starts: self.class_starts,
            smoothing: self.smoothing,
            seed: self.seed,
            ..LatentClassRequest::new(items.clone())
        };
        let summary = format!(
            "Latent classes of {}, {} to {} classes",
            kit::count(items.len(), "item", "items"),
            self.min_classes,
            self.max_classes
        );
        kit::ready(ReportRequest::LatentClass(request), summary)
    }

    fn embedding(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<ReportRequest> {
        ui.horizontal_wrapped(|ui| {
            ui.label("Method:");
            for m in EmbeddingMethod::ALL {
                ui.radio_value(&mut self.method, m, m.label());
            }
        });
        ui.horizontal(|ui| {
            ui.label("The columns hold:");
            ui.radio_value(&mut self.distances, false, "variables");
            ui.radio_value(&mut self.distances, true, "a distance matrix")
                .on_hover_text("One column per row: column i holds each row's distance to row i");
        });
        ui.label(if self.distances { "Distance matrix columns:" } else { "Columns:" });
        kit::column_list(ui, "embedding_columns", table, &mut self.chosen, |i| kit::is_numeric(table, i));
        let columns: Vec<usize> =
            kit::chosen_columns(&self.chosen).into_iter().filter(|&i| kit::is_numeric(table, i)).collect();
        ui.separator();
        match self.method {
            EmbeddingMethod::Tsne => {
                ui.horizontal(|ui| {
                    ui.label("Perplexity (about the number of close neighbours):");
                    ui.add(egui::DragValue::new(&mut self.perplexity).range(2.0..=200.0).speed(0.5));
                });
            }
            EmbeddingMethod::Umap => {
                ui.horizontal(|ui| {
                    ui.label("Neighbours:");
                    ui.add(egui::DragValue::new(&mut self.neighbours).range(2..=200));
                    ui.label("Minimum distance:");
                    ui.add(egui::DragValue::new(&mut self.min_dist).range(0.0..=0.99).speed(0.01));
                });
            }
            _ => {}
        }
        let chosen = self.chosen.clone();
        ui.horizontal(|ui| {
            ui.label("Colour and marker by:");
            let allow = |i: usize| !chosen.get(i).copied().unwrap_or(false);
            kit::column_choice(ui, "embedding_color", table, &mut self.color, allow, Some("None"));
        });
        kit::more_options(ui, |ui| {
            if !self.distances {
                ui.checkbox(&mut self.standardize, "Standardize columns (recommended when units differ)");
            }
            ui.horizontal(|ui| {
                ui.label("Dimensions:");
                for d in 1..=3 {
                    ui.radio_value(&mut self.dims, d, d.to_string());
                }
            });
            if self.method != EmbeddingMethod::ClassicalScaling {
                ui.horizontal(|ui| {
                    let what = match self.method {
                        EmbeddingMethod::Umap => "Set the number of epochs:",
                        EmbeddingMethod::MetricScaling => "Set the most iterations:",
                        _ => "Set the number of iterations:",
                    };
                    ui.checkbox(&mut self.custom_iterations, what);
                    ui.add_enabled(
                        self.custom_iterations,
                        egui::DragValue::new(&mut self.iterations).range(10..=10_000),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("Random seed:");
                    ui.add(crate::dialog_kit::seed_value(&mut self.seed));
                });
            }
        });
        if columns.is_empty() {
            return Err("Choose at least one column".into());
        }
        if self.distances && columns.len() != table.n_rows() {
            return Err(format!(
                "A distance matrix needs one column per row: {} rows, {} columns chosen",
                table.n_rows(),
                columns.len()
            ));
        }
        let request = EmbeddingRequest {
            method: self.method,
            distances: self.distances,
            standardize: self.standardize,
            dims: self.dims,
            perplexity: self.perplexity,
            neighbours: self.neighbours,
            min_dist: self.min_dist,
            iterations: self.custom_iterations.then_some(self.iterations),
            seed: self.seed,
            color: self.color,
            ..EmbeddingRequest::new(self.method, columns.clone())
        };
        let what = if self.distances {
            format!("a {}-row distance matrix", columns.len())
        } else {
            kit::count(columns.len(), "column", "columns")
        };
        kit::ready(ReportRequest::Embedding(request), format!("{} map of {what}", self.method.label()))
    }
}

impl Remember for MapDialog {
    fn memory_key(&self) -> String {
        self.title().to_owned()
    }

    fn set_edit(&mut self, report: Option<u64>) {
        self.edit = report;
    }

    fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        self.chosen = kit::mask(table.n_cols(), columns);
    }
}
