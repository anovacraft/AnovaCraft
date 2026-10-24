//! Dialog for creating a mixture experiment (simplex-lattice,
//! simplex-centroid, or extreme vertices of a region with lower and upper
//! bounds per component). The design is built by [`app_model::doe`] and previewed
//! as the settings change, with the Scheffé models it can support; "Create"
//! opens it as a new dataset with an empty response column.

use app_model::doe::{
    MAX_MIXTURE_REPLICATES, MixtureDesignRequest, MixtureKind, create_mixture_design, mixture_model_support,
};
use eframe::egui::{self, RichText};
use stat_data::DataTable;
use stat_doe::default_factor_names;
use stat_doe::mixture::MAX_COMPONENTS;

use crate::design_dialog::sentence_fragment;
use crate::dialog_kit::{self as kit, Dialog, Outcome, Readiness, Remember};

/// How the component columns are expressed.
#[derive(Debug, Clone, Copy, PartialEq)]
enum TotalChoice {
    Proportions,
    Percent,
    Amount,
}

/// Which mixture design the dialog builds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DesignChoice {
    Lattice,
    Centroid,
    Vertices,
}

/// What the preview shows: description, runs, and one line per Scheffé model.
type Preview = Result<(String, usize, Vec<(String, bool)>), String>;

#[derive(Clone)]
pub struct MixtureDesignDialog {
    choice: DesignChoice,
    /// Extreme vertices: lower and upper bound of each component as a
    /// proportion of the total.
    lower: Vec<f64>,
    upper: Vec<f64>,
    /// Extreme vertices: 0 vertices only, 1 add edge centroids, 2 add face centroids.
    vertex_centroids: usize,
    overall_centroid: bool,
    degree: usize,
    add_centroid: bool,
    axial: bool,
    components: Vec<String>,
    replicates: usize,
    total: TotalChoice,
    amount: f64,
    randomize: bool,
    random_seed: u64,
    response: String,
    preview: Option<(MixtureDesignRequest, Preview)>,
}

impl MixtureDesignDialog {
    /// A dialog for a simplex-lattice (`centroid_design` false) or
    /// simplex-centroid design, with three components.
    pub fn new(centroid_design: bool) -> Self {
        Self {
            choice: if centroid_design { DesignChoice::Centroid } else { DesignChoice::Lattice },
            lower: vec![0.0; 3],
            upper: vec![1.0; 3],
            vertex_centroids: 1,
            overall_centroid: true,
            degree: 2,
            add_centroid: false,
            axial: false,
            components: default_factor_names(3),
            replicates: 1,
            total: TotalChoice::Proportions,
            amount: 1000.0,
            randomize: true,
            random_seed: 1,
            response: "Y".to_owned(),
            preview: None,
        }
    }

    /// A dialog for an extreme-vertices design over a constrained region,
    /// starting from three components with bounds that cut the simplex.
    pub fn extreme_vertices() -> Self {
        Self {
            choice: DesignChoice::Vertices,
            lower: vec![0.1, 0.1, 0.0],
            upper: vec![0.7, 0.6, 0.3],
            ..Self::new(false)
        }
    }

    fn total_value(&self) -> f64 {
        match self.total {
            TotalChoice::Proportions => 1.0,
            TotalChoice::Percent => 100.0,
            TotalChoice::Amount => self.amount,
        }
    }

    fn request(&self) -> MixtureDesignRequest {
        let vertices = self.choice == DesignChoice::Vertices;
        let scaled = |v: &[f64]| if vertices { v.iter().map(|x| x * self.total_value()).collect() } else { Vec::new() };
        MixtureDesignRequest {
            kind: match self.choice {
                DesignChoice::Centroid => MixtureKind::Centroid,
                DesignChoice::Lattice => MixtureKind::Lattice { degree: self.degree, centroid: self.add_centroid },
                DesignChoice::Vertices => {
                    MixtureKind::ExtremeVertices { centroids: self.vertex_centroids, overall: self.overall_centroid }
                }
            },
            lower: scaled(&self.lower),
            upper: scaled(&self.upper),
            components: self.components.clone(),
            axial: self.axial,
            replicates: self.replicates,
            total: self.total_value(),
            randomize: self.randomize.then_some(self.random_seed),
            response: self.response.clone(),
        }
    }

    fn refresh_preview(&mut self) {
        let request = self.request();
        if self.preview.as_ref().is_some_and(|(r, _)| *r == request) {
            return;
        }
        let result = create_mixture_design(&request)
            .map(|(_, d)| {
                let support = mixture_model_support(&d)
                    .into_iter()
                    .map(|s| {
                        let name = s.model.label();
                        if s.estimable {
                            let df = if s.error_df == 0 {
                                "0 error df: it fits exactly (add replicates or check blends to test it)".to_owned()
                            } else {
                                format!("{} error df", s.error_df)
                            };
                            (format!("{name} model: {} terms, {df}", s.terms), true)
                        } else {
                            (format!("{name} model: {} terms, not estimable from these blends", s.terms), false)
                        }
                    })
                    .collect();
                (d.description.clone(), d.n_runs(), support)
            })
            .map_err(|e| e.to_string());
        self.preview = Some((request, result));
    }

    /// Show the dialog; Create returns the design table and a one-line
    /// description.
    pub fn show(&mut self, ctx: &egui::Context) -> Outcome<(DataTable, String)> {
        let dialog = Dialog::new("Mixture experiment").creates().default_width(520.0);
        match dialog.show(ctx, |ui| {
            let height = (ctx.screen_rect().height() * 0.8 - kit::em(ui) * 6.0).max(kit::em(ui) * 8.0);
            egui::ScrollArea::vertical().max_height(height).show(ui, |ui| self.contents(ui)).inner
        }) {
            Outcome::Reset => {
                *self = match self.choice {
                    DesignChoice::Vertices => Self::extreme_vertices(),
                    choice => Self::new(choice == DesignChoice::Centroid),
                };
                Outcome::Open
            }
            Outcome::Run(request, place) => match create_mixture_design(&request) {
                Ok((table, design)) => {
                    Outcome::Run((table, format!("{} (fit it with Model → Mixture model)", design.description)), place)
                }
                Err(e) => {
                    self.preview = Some((request, Err(e.to_string())));
                    Outcome::Open
                }
            },
            Outcome::Open => Outcome::Open,
            Outcome::Cancel => Outcome::Cancel,
        }
    }

    fn contents(&mut self, ui: &mut egui::Ui) -> Readiness<MixtureDesignRequest> {
        ui.horizontal(|ui| {
            ui.label("Design type:");
            ui.radio_value(&mut self.choice, DesignChoice::Lattice, "Simplex lattice");
            ui.radio_value(&mut self.choice, DesignChoice::Centroid, "Simplex centroid");
            ui.radio_value(&mut self.choice, DesignChoice::Vertices, "Extreme vertices");
        });
        kit::note(
            ui,
            match self.choice {
                DesignChoice::Centroid => {
                    "Equal parts of every non-empty subset of components (pure blends, 50:50 pairs, …, the \
                     overall centroid): 2^q − 1 blends; supports the special cubic model."
                }
                DesignChoice::Lattice => {
                    "Every blend whose proportions are multiples of 1/m: supports the Scheffé model of degree m \
                     (m = 2 for quadratic, 3 for cubic)."
                }
                DesignChoice::Vertices => {
                    "The corners of the region left by a lower and upper bound on each component (McLean and \
                     Anderson's algorithm), optionally with edge and face centroids and the overall centroid."
                }
            },
        );
        ui.separator();

        // Components.
        let mut q = self.components.len();
        ui.horizontal(|ui| {
            ui.label("Number of components:");
            if ui.add(egui::DragValue::new(&mut q).range(2..=MAX_COMPONENTS)).changed() {
                let defaults = default_factor_names(q);
                self.components.truncate(q);
                self.lower.resize(q, 0.0);
                self.upper.resize(q, 1.0);
                while self.components.len() < q {
                    let name = defaults
                        .iter()
                        .find(|n| !self.components.contains(n))
                        .cloned()
                        .unwrap_or_else(|| format!("X{}", self.components.len() + 1));
                    self.components.push(name);
                }
            }
        });
        let vertices = self.choice == DesignChoice::Vertices;
        let total = self.total_value();
        let columns = if vertices { 4 } else { 2 };
        egui::ScrollArea::vertical().id_salt("mixture_components").max_height(kit::em(ui) * 11.0).show(ui, |ui| {
            egui::Grid::new("mixture_component_grid").num_columns(columns).spacing([10.0, 4.0]).striped(true).show(
                ui,
                |ui| {
                    if vertices {
                        ui.label("");
                        ui.label("Name");
                        ui.label("Lower bound");
                        ui.label("Upper bound");
                        ui.end_row();
                    }
                    for (j, c) in self.components.iter_mut().enumerate() {
                        ui.label(format!("Component {}", j + 1));
                        ui.add(egui::TextEdit::singleline(c).desired_width(kit::em(ui) * 10.0));
                        if vertices {
                            for bound in [&mut self.lower[j], &mut self.upper[j]] {
                                let mut v = *bound * total;
                                let step = total / 100.0;
                                if ui.add(egui::DragValue::new(&mut v).range(0.0..=total).speed(step)).changed() {
                                    *bound = v / total;
                                }
                            }
                        }
                        ui.end_row();
                    }
                },
            );
        });
        if vertices {
            kit::note(ui, "Bounds are in the units chosen under \"Components as\" in More options.");
        }
        ui.separator();

        // Options.
        if self.choice == DesignChoice::Vertices {
            ui.horizontal(|ui| {
                ui.label("Add centroids of:");
                ui.radio_value(&mut self.vertex_centroids, 0, "none");
                ui.radio_value(&mut self.vertex_centroids, 1, "edges");
                ui.radio_value(&mut self.vertex_centroids, 2, "edges and faces");
            });
            ui.checkbox(&mut self.overall_centroid, "Add the overall centroid (the average of the vertices)");
        } else if self.choice == DesignChoice::Lattice {
            ui.horizontal(|ui| {
                ui.label("Lattice degree m:");
                ui.add(egui::DragValue::new(&mut self.degree).range(1..=6));
                ui.label(RichText::new(format!("(steps of 1/{})", self.degree)).weak());
            });
            ui.checkbox(&mut self.add_centroid, "Add the overall centroid (equal parts of every component)");
        }
        ui.checkbox(&mut self.axial, "Add axial check blends (interior points for testing the model's fit)");
        ui.horizontal(|ui| {
            ui.label("Replicates:");
            ui.add(egui::DragValue::new(&mut self.replicates).range(1..=MAX_MIXTURE_REPLICATES));
            ui.label(RichText::new("(repeating every blend gives a pure-error estimate)").weak());
        });

        // Output.
        kit::more_options(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("Components as:");
                ui.radio_value(&mut self.total, TotalChoice::Proportions, "Proportions (sum 1)");
                ui.radio_value(&mut self.total, TotalChoice::Percent, "Percentages (sum 100)");
                ui.radio_value(&mut self.total, TotalChoice::Amount, "Amounts summing to");
                ui.add_enabled(
                    self.total == TotalChoice::Amount,
                    egui::DragValue::new(&mut self.amount).range(0.001..=1e9).speed(1.0),
                );
            });
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.randomize, "Randomize run order, seed");
                ui.add_enabled(self.randomize, crate::dialog_kit::seed_value(&mut self.random_seed));
            });
            ui.horizontal(|ui| {
                ui.label("Response column:");
                ui.add(egui::TextEdit::singleline(&mut self.response).desired_width(kit::em(ui) * 9.0));
            });
        });

        // Preview: the models the blends support; the description and run
        // count are the summary under the settings.
        self.refresh_preview();
        match &self.preview {
            Some((request, Ok((description, n, support)))) => {
                ui.separator();
                for (line, ok) in support {
                    let mark = if *ok { "Supported:" } else { "Not supported:" };
                    let text = RichText::new(format!("{mark} {line}")).small();
                    ui.label(if *ok { text } else { text.weak() });
                }
                let summary = if description.contains(&format!("{n} runs")) {
                    description.clone()
                } else {
                    format!("{description} — {n} runs")
                };
                kit::ready(request.clone(), summary)
            }
            Some((_, Err(e))) => Err(sentence_fragment(e)),
            None => Err("Set up the design".into()),
        }
    }
}

impl Remember for MixtureDesignDialog {
    fn memory_key(&self) -> String {
        match self.choice {
            DesignChoice::Lattice => "Simplex lattice",
            DesignChoice::Centroid => "Simplex centroid",
            DesignChoice::Vertices => "Extreme vertices",
        }
        .into()
    }

    fn set_edit(&mut self, _report: Option<u64>) {}
}
