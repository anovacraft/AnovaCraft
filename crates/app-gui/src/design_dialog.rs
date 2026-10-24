//! Dialog for creating a designed experiment. The design is built by
//! [`app_model::doe`] and previewed as the settings change; "Create" opens
//! it as a new dataset with an empty response column.

use crate::dialog_kit::ComboExt as _;
use app_model::Project;
use app_model::doe::{DesignKind, DesignRequest, DsdOptions, create_design, design_project, dsd_comparison_table};
use eframe::egui::{self, RichText};
use stat_data::DataTable;
use stat_doe::factorial::{fractional_factorial, resolution_label, tabulated_fractions};
use stat_doe::screening::MAX_DSD_CENTER_RUNS;
use stat_doe::{AxialDistance, ChangeLevel, Factor, ModelFormula, Units, default_factor_names};
use stat_doe::{ScreeningFactor, categorical_screening};
use stat_doe::{SpaceFillingKind, space_filling};

use crate::dialog_kit::{self as kit, Dialog, Outcome, Readiness, Remember};
use crate::theme;

/// Design types offered, in menu order.
pub const KIND_NAMES: [&str; 15] = [
    "Full factorial (2 levels)",
    "Full factorial (3 levels)",
    "Fractional factorial",
    "Plackett–Burman",
    "Definitive screening",
    "Central composite",
    "Box–Behnken",
    "D-optimal (coordinate exchange)",
    "I-optimal (coordinate exchange)",
    "Full factorial (mixed levels)",
    "Latin hypercube (maximin)",
    "Maximin distance",
    "Minimax distance",
    "Uniform (centered L2 discrepancy)",
    "Screening with categorical factors",
];

/// The screening design with categorical factors' index in [`KIND_NAMES`].
pub const CATEGORICAL_SCREENING: usize = 14;

/// The mixed-level factorial's index in [`KIND_NAMES`].
const MIXED_LEVELS: usize = 9;

/// The first space-filling design's index in [`KIND_NAMES`]; the four
/// space-filling designs follow in the order of [`SpaceFillingKind::ALL`].
pub const SPACE_FILLING: usize = 10;

/// The space-filling design at index `kind` of [`KIND_NAMES`], if it is one.
fn space_filling_kind(kind: usize) -> Option<SpaceFillingKind> {
    kind.checked_sub(SPACE_FILLING).and_then(|i| SpaceFillingKind::ALL.get(i).copied())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AlphaChoice {
    Rotatable,
    Spherical,
    FaceCentered,
    Custom,
}

#[derive(Clone)]
struct FactorRow {
    name: String,
    low: f64,
    high: f64,
    /// Number of evenly spaced levels from low to high (mixed-level factorial).
    levels: usize,
    /// How hard the factor is to change (D-optimal designs: anything but
    /// easy makes a split-plot design).
    changes: ChangeLevel,
    /// Categorical, with `levels` levels (screening with categorical
    /// factors); otherwise continuous.
    categorical: bool,
}

/// A design preview: its description, number of runs and factor names.
type DesignPreview = (String, usize, Vec<String>);

#[derive(Clone)]
pub struct DesignDialog {
    kind: usize,
    factors: Vec<FactorRow>,
    fraction: usize,
    alpha: AlphaChoice,
    custom_alpha: f64,
    inscribed: bool,
    default_centers: bool,
    center_points: usize,
    formula: String,
    runs: usize,
    starts: usize,
    seed: u64,
    /// Constraints of the optimal designs, one per line in actual units.
    constraints: String,
    /// Split-plot designs: whole plots, subplots and the prior variance
    /// ratios η_w and η_s.
    whole_plots: usize,
    subplots: usize,
    whole_plot_ratio: f64,
    subplot_ratio: f64,
    randomize: bool,
    random_seed: u64,
    coded: bool,
    response: String,
    /// Definitive screening: extra runs beyond the smallest design and the
    /// number of center runs in all.
    dsd_extra_runs: usize,
    dsd_center_runs: usize,
    /// The definitive screening comparison table, for the factor count and
    /// options it was computed for.
    dsd_comparison: Option<((usize, DsdOptions), Result<DataTable, String>)>,
    /// Screening with categorical factors: the default run count, and two-
    /// factor interactions in the model.
    default_runs: bool,
    interactions: bool,
    /// The request the preview was computed for, and the preview.
    preview: Option<(DesignRequest, Result<DesignPreview, String>)>,
}

impl DesignDialog {
    /// A dialog starting at design type `kind` (an index into [`KIND_NAMES`]).
    pub fn new(kind: usize) -> Self {
        let k = match kind {
            3 => 7,
            4 => 6,
            CATEGORICAL_SCREENING => 4,
            _ => 3,
        };
        let mut d = Self {
            kind: kind.min(KIND_NAMES.len() - 1),
            factors: Vec::new(),
            fraction: 1,
            alpha: AlphaChoice::Rotatable,
            custom_alpha: 1.5,
            inscribed: false,
            default_centers: true,
            center_points: 0,
            formula: String::new(),
            runs: 12,
            starts: 20,
            seed: 1,
            constraints: String::new(),
            whole_plots: 4,
            subplots: 8,
            whole_plot_ratio: 1.0,
            subplot_ratio: 1.0,
            randomize: true,
            random_seed: 1,
            coded: false,
            response: "Y".to_owned(),
            dsd_extra_runs: 0,
            dsd_center_runs: 1,
            dsd_comparison: None,
            default_runs: true,
            interactions: false,
            preview: None,
        };
        d.set_factor_count(k);
        if d.kind == CATEGORICAL_SCREENING {
            // A start: the second factor categorical with three levels.
            d.factors[1].categorical = true;
            d.factors[1].levels = 3;
        }
        d.formula = d.preset(&ModelFormula::interactions(k));
        d
    }

    /// The D-optimal dialog set up for a split-plot design: the first factor
    /// hard to change, 16 runs in 4 whole plots.
    pub fn split_plot() -> Self {
        let mut d = Self::new(7);
        d.factors[0].changes = ChangeLevel::Hard;
        d.runs = 16;
        d
    }

    /// Whether some factor is hard or very hard to change.
    fn restricted(&self) -> bool {
        self.kind == 7 && self.factors.iter().any(|f| f.changes != ChangeLevel::Easy)
    }

    /// Whether the design has a subplot stratum (very hard and hard factors).
    fn split_split(&self) -> bool {
        self.restricted()
            && self.factors.iter().any(|f| f.changes == ChangeLevel::VeryHard)
            && self.factors.iter().any(|f| f.changes == ChangeLevel::Hard)
    }

    fn names(&self) -> Vec<String> {
        self.factors.iter().map(|f| f.name.clone()).collect()
    }

    fn preset(&self, formula: &ModelFormula) -> String {
        formula.describe(&self.names())
    }

    fn set_factor_count(&mut self, k: usize) {
        let defaults = default_factor_names(k);
        self.factors.truncate(k);
        while self.factors.len() < k {
            let name = defaults
                .iter()
                .find(|n| !self.factors.iter().any(|f| f.name == **n))
                .cloned()
                .unwrap_or_else(|| format!("X{}", self.factors.len() + 1));
            self.factors.push(FactorRow {
                name,
                low: -1.0,
                high: 1.0,
                levels: 2,
                changes: ChangeLevel::Easy,
                categorical: false,
            });
        }
    }

    fn kind(&self) -> DesignKind {
        match self.kind {
            0 => DesignKind::TwoLevelFactorial,
            1 => DesignKind::ThreeLevelFactorial,
            2 => DesignKind::FractionalFactorial { p: self.fraction },
            3 => DesignKind::PlackettBurman,
            4 => DesignKind::DefinitiveScreening { extra_runs: self.dsd_extra_runs },
            CATEGORICAL_SCREENING => DesignKind::CategoricalScreening {
                levels: self.screening_levels(),
                runs: if self.default_runs { self.screening_default_runs() } else { self.runs },
                interactions: self.interactions,
                starts: self.starts,
                seed: self.seed,
            },
            5 => DesignKind::CentralComposite {
                alpha: match self.alpha {
                    AlphaChoice::Rotatable => AxialDistance::Rotatable,
                    AlphaChoice::Spherical => AxialDistance::Spherical,
                    AlphaChoice::FaceCentered => AxialDistance::FaceCentered,
                    AlphaChoice::Custom => AxialDistance::Custom(self.custom_alpha),
                },
                inscribed: self.inscribed && self.alpha != AlphaChoice::FaceCentered,
            },
            6 => DesignKind::BoxBehnken,
            7 if self.restricted() => DesignKind::SplitPlot {
                formula: self.formula.clone(),
                runs: self.runs,
                whole_plots: self.whole_plots,
                subplots: self.subplots,
                changes: self.factors.iter().map(|f| f.changes).collect(),
                whole_plot_ratio: self.whole_plot_ratio,
                subplot_ratio: self.subplot_ratio,
                starts: self.starts,
                seed: self.seed,
            },
            7 => DesignKind::DOptimal {
                formula: self.formula.clone(),
                runs: self.runs,
                starts: self.starts,
                seed: self.seed,
                constraints: self.constraints.clone(),
            },
            MIXED_LEVELS => DesignKind::GeneralFactorial { levels: self.factors.iter().map(|f| f.levels).collect() },
            i if space_filling_kind(i).is_some() => DesignKind::SpaceFilling {
                criterion: space_filling_kind(i).unwrap_or(SpaceFillingKind::MaximinLatinHypercube),
                runs: self.runs,
                seed: self.seed,
            },
            _ => DesignKind::IOptimal {
                formula: self.formula.clone(),
                runs: self.runs,
                starts: self.starts,
                seed: self.seed,
                constraints: self.constraints.clone(),
            },
        }
    }

    fn request(&self) -> DesignRequest {
        let kind = self.kind();
        // CCD and Box–Behnken have their own default number of centre runs;
        // the others default to none.
        let own_default = matches!(kind, DesignKind::CentralComposite { .. } | DesignKind::BoxBehnken);
        let center_points = if matches!(kind, DesignKind::DefinitiveScreening { .. }) {
            Some(self.dsd_center_runs)
        } else if !kind.accepts_center_points() || (own_default && self.default_centers) {
            None
        } else {
            Some(self.center_points)
        };
        DesignRequest {
            kind,
            factors: self.factors.iter().map(|f| Factor::new(f.name.trim(), f.low, f.high)).collect(),
            center_points,
            randomize: self.randomize.then_some(self.random_seed),
            units: if self.coded { Units::Coded } else { Units::Actual },
            response: self.response.clone(),
        }
    }

    fn refresh_preview(&mut self) {
        let request = self.request();
        if self.preview.as_ref().is_some_and(|(r, _)| *r == request) {
            return;
        }
        let result = create_design(&request)
            .map(|(_, d)| (d.description.clone(), d.n_runs(), d.notes.clone()))
            .map_err(|e| e.to_string());
        self.preview = Some((request, result));
    }

    /// Show the dialog; Create returns the design table and a one-line
    /// description.
    pub fn show(&mut self, ctx: &egui::Context) -> Outcome<(Project, String)> {
        let dialog = Dialog::new("Designed experiment").creates().default_width(560.0);
        match dialog.show(ctx, |ui| {
            // The settings scroll; the action row stays below them, always in
            // view (it was cut off when a taller design type was chosen in a
            // window sized for a shorter one).
            let height = (ctx.screen_rect().height() * 0.8 - kit::em(ui) * 6.0).max(kit::em(ui) * 8.0);
            egui::ScrollArea::vertical().max_height(height).show(ui, |ui| self.contents(ui)).inner
        }) {
            Outcome::Reset => {
                *self = Self::new(self.kind);
                Outcome::Open
            }
            Outcome::Run(request, place) => match design_project(&request) {
                Ok(created) => Outcome::Run(created, place),
                Err(e) => {
                    self.preview = Some((request, Err(e.to_string())));
                    Outcome::Open
                }
            },
            Outcome::Open => Outcome::Open,
            Outcome::Cancel => Outcome::Cancel,
        }
    }

    /// The settings and preview; ready with the design's request, and its
    /// description and run count as the summary.
    fn contents(&mut self, ui: &mut egui::Ui) -> Readiness<DesignRequest> {
        ui.horizontal(|ui| {
            ui.label("Design type:");
            egui::ComboBox::from_id_salt("design_kind")
                .selected_text(KIND_NAMES[self.kind])
                .width(kit::em(ui) * 17.0)
                .show_list(ui, |ui| {
                    for (i, name) in KIND_NAMES.iter().enumerate() {
                        ui.selectable_value(&mut self.kind, i, *name);
                    }
                });
        });
        kit::note(ui, kind_help(self.kind));
        ui.separator();

        // Factors.
        let mut k = self.factors.len();
        ui.horizontal(|ui| {
            ui.label("Number of factors:");
            if ui.add(egui::DragValue::new(&mut k).range(1..=30)).changed() {
                self.set_factor_count(k);
            }
        });
        egui::ScrollArea::vertical().id_salt("design_factors").max_height(kit::em(ui) * 12.0).show(ui, |ui| {
            let mixed = self.kind == MIXED_LEVELS;
            let changes = self.kind == 7;
            let typed = self.kind == CATEGORICAL_SCREENING;
            let columns = if mixed || changes {
                4
            } else if typed {
                5
            } else {
                3
            };
            egui::Grid::new("design_factor_grid").num_columns(columns).spacing([10.0, 4.0]).striped(true).show(
                ui,
                |ui| {
                    ui.strong("Factor");
                    if typed {
                        ui.strong("Type");
                        ui.strong("Levels");
                    }
                    ui.strong(if mixed { "Lowest" } else { "Low (−1)" });
                    ui.strong(if mixed { "Highest" } else { "High (+1)" });
                    if mixed {
                        ui.strong("Levels");
                    }
                    if changes {
                        ui.strong("Changes").on_hover_text(
                            "How hard the factor is to change: hard factors stay fixed within each whole plot \
                             (a split-plot design); with very hard factors too, very hard ones stay fixed within \
                             whole plots and hard ones within subplots",
                        );
                    }
                    ui.end_row();
                    for (i, f) in self.factors.iter_mut().enumerate() {
                        ui.add(egui::TextEdit::singleline(&mut f.name).desired_width(kit::em(ui) * 9.0));
                        if typed {
                            egui::ComboBox::from_id_salt(("design_factor_type", i))
                                .selected_text(if f.categorical { "Categorical" } else { "Continuous" })
                                .width(kit::em(ui) * 6.5)
                                .show_list(ui, |ui| {
                                    ui.selectable_value(&mut f.categorical, false, "Continuous");
                                    ui.selectable_value(&mut f.categorical, true, "Categorical");
                                })
                                .response
                                .widget_info(|| {
                                    egui::WidgetInfo::labeled(
                                        egui::WidgetType::ComboBox,
                                        true,
                                        format!("Type {}", i + 1),
                                    )
                                });
                            if f.categorical {
                                if f.levels < 2 {
                                    f.levels = 3;
                                }
                                ui.add(
                                    egui::DragValue::new(&mut f.levels).range(2..=categorical_screening::MAX_LEVELS),
                                )
                                .widget_info(|| {
                                    egui::WidgetInfo::labeled(
                                        egui::WidgetType::DragValue,
                                        true,
                                        format!("Levels {}", i + 1),
                                    )
                                });
                                // Low and high do not apply to a categorical factor.
                                ui.label(RichText::new("L1, L2, …").weak());
                                ui.label("");
                                ui.end_row();
                                continue;
                            }
                            ui.label(RichText::new("2").weak());
                        }
                        ui.add(egui::DragValue::new(&mut f.low).speed(0.1));
                        ui.add(egui::DragValue::new(&mut f.high).speed(0.1));
                        if mixed {
                            ui.add(egui::DragValue::new(&mut f.levels).range(2..=12));
                        }
                        if changes {
                            egui::ComboBox::from_id_salt(("design_changes", i))
                                .selected_text(f.changes.label())
                                .width(kit::em(ui) * 6.0)
                                .show_list(ui, |ui| {
                                    for c in ChangeLevel::ALL {
                                        ui.selectable_value(&mut f.changes, c, c.label());
                                    }
                                })
                                .response
                                .widget_info(|| {
                                    egui::WidgetInfo::labeled(
                                        egui::WidgetType::ComboBox,
                                        true,
                                        format!("Changes {}", i + 1),
                                    )
                                });
                        }
                        ui.end_row();
                    }
                },
            );
        });
        ui.separator();

        // Options for the chosen type.
        let k = self.factors.len();
        let optimal = matches!(self.kind, 7 | 8 | CATEGORICAL_SCREENING);
        match self.kind {
            2 => {
                let options: Vec<usize> =
                    tabulated_fractions().into_iter().filter(|&(kk, _)| kk == k).map(|(_, p)| p).collect();
                if options.is_empty() {
                    ui.colored_label(
                        theme::ERROR,
                        format!("No tabulated fractions for {k} factors (3 to 11 are supported)."),
                    );
                } else {
                    if !options.contains(&self.fraction) {
                        self.fraction = options[0];
                    }
                    let label = |p: usize| {
                        let res =
                            fractional_factorial(k, p).map(|f| resolution_label(f.resolution)).unwrap_or_default();
                        format!("1/{} fraction: {} runs, resolution {res}", 1usize << p, 1usize << (k - p))
                    };
                    ui.horizontal(|ui| {
                        ui.label("Fraction:");
                        egui::ComboBox::from_id_salt("design_fraction").selected_text(label(self.fraction)).show_list(
                            ui,
                            |ui| {
                                for &p in &options {
                                    ui.selectable_value(&mut self.fraction, p, label(p));
                                }
                            },
                        );
                    });
                }
            }
            5 => {
                ui.horizontal_wrapped(|ui| {
                    ui.label("Axial distance α:");
                    ui.radio_value(&mut self.alpha, AlphaChoice::Rotatable, "Rotatable");
                    ui.radio_value(&mut self.alpha, AlphaChoice::Spherical, "Spherical (√k)");
                    ui.radio_value(&mut self.alpha, AlphaChoice::FaceCentered, "Face-centered (1)");
                    ui.radio_value(&mut self.alpha, AlphaChoice::Custom, "Custom:");
                    ui.add_enabled(
                        self.alpha == AlphaChoice::Custom,
                        egui::DragValue::new(&mut self.custom_alpha).speed(0.01).range(0.1..=10.0),
                    );
                });
                ui.add_enabled(
                    self.alpha != AlphaChoice::FaceCentered,
                    egui::Checkbox::new(
                        &mut self.inscribed,
                        "Inscribed: keep the axial points within the factor limits",
                    ),
                );
            }
            7 | 8 => {
                ui.label("Model (terms joined by +; use * for interactions and ^2 for squares):");
                ui.add(egui::TextEdit::singleline(&mut self.formula).desired_width(f32::INFINITY));
                ui.horizontal(|ui| {
                    ui.label("Presets:");
                    if ui.small_button("Main effects").clicked() {
                        self.formula = self.preset(&ModelFormula::main_effects(k));
                    }
                    if ui.small_button("+ Interactions").clicked() {
                        self.formula = self.preset(&ModelFormula::interactions(k));
                    }
                    if ui.small_button("Quadratic").clicked() {
                        self.formula = self.preset(&ModelFormula::quadratic(k));
                    }
                });
                let terms = ModelFormula::parse(&self.formula, &self.names()).map(|f| f.n_terms());
                ui.horizontal(|ui| {
                    ui.label("Runs:");
                    ui.add(egui::DragValue::new(&mut self.runs).range(1..=500));
                    match terms {
                        Ok(p) => {
                            ui.label(RichText::new(format!("(the model has {p} terms, so at least {p} runs)")).weak());
                            if ui.small_button("Minimum + 4").clicked() {
                                self.runs = p + 4;
                            }
                        }
                        Err(_) => {
                            ui.label(RichText::new("(model not understood yet)").weak());
                        }
                    }
                });
                if self.restricted() {
                    self.plot_settings(ui);
                } else {
                    constraints_box(ui, &mut self.constraints);
                }
            }
            4 => self.dsd_settings(ui),
            CATEGORICAL_SCREENING => self.screening_settings(ui),
            i if space_filling_kind(i).is_some() => {
                ui.horizontal(|ui| {
                    ui.label("Runs:");
                    ui.add(egui::DragValue::new(&mut self.runs).range(2..=space_filling::MAX_RUNS));
                    if ui
                        .small_button(format!("10 × factors ({})", 10 * k))
                        .on_hover_text("A common starting size for computer experiments (Loeppky, Sacks & Welch 2009)")
                        .clicked()
                    {
                        self.runs = (10 * k).clamp(2, space_filling::MAX_RUNS);
                    }
                    ui.label("Seed:");
                    ui.add(crate::dialog_kit::seed_value(&mut self.seed));
                });
                kit::note(
                    ui,
                    "Factors are spread over their low-to-high ranges; space-filling designs have no center points.",
                );
            }
            _ => {}
        }
        let kind = self.kind();
        if kind.accepts_center_points() {
            ui.horizontal(|ui| {
                ui.label("Center points:");
                let own_default = matches!(kind, DesignKind::CentralComposite { .. } | DesignKind::BoxBehnken);
                if own_default {
                    ui.checkbox(&mut self.default_centers, "Default");
                }
                ui.add_enabled(
                    !(own_default && self.default_centers),
                    egui::DragValue::new(&mut self.center_points).range(0..=50),
                );
            });
        }

        // Output and search settings.
        kit::more_options(ui, |ui| {
            if optimal {
                ui.horizontal(|ui| {
                    ui.label("Random starts:");
                    ui.add(egui::DragValue::new(&mut self.starts).range(1..=500));
                    ui.label("Seed:");
                    ui.add(crate::dialog_kit::seed_value(&mut self.seed));
                });
            }
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.randomize, "Randomize run order, seed");
                ui.add_enabled(self.randomize, crate::dialog_kit::seed_value(&mut self.random_seed));
            });
            ui.horizontal(|ui| {
                ui.label("Factor columns in:");
                ui.radio_value(&mut self.coded, false, "Actual units");
                ui.radio_value(&mut self.coded, true, "Coded units (−1 … +1)");
            });
            ui.horizontal(|ui| {
                ui.label("Response column:");
                ui.add(egui::TextEdit::singleline(&mut self.response).desired_width(kit::em(ui) * 9.0));
            });
        });

        // Preview: the design's notes; its description and run count are
        // the summary under the settings.
        self.refresh_preview();
        match &self.preview {
            Some((request, Ok((description, n, notes)))) => {
                if !notes.is_empty() {
                    ui.separator();
                    egui::ScrollArea::vertical().id_salt("design_notes").max_height(kit::em(ui) * 9.0).show(ui, |ui| {
                        for note in notes {
                            ui.label(RichText::new(note).small().monospace());
                        }
                    });
                }
                // Most descriptions already give the run count.
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

impl DesignDialog {
    /// Levels of each factor for a screening design with categorical
    /// factors: `None` for continuous ones.
    fn screening_levels(&self) -> Vec<Option<usize>> {
        self.factors.iter().map(|f| f.categorical.then_some(f.levels.max(2))).collect()
    }

    /// The default run count of the screening design with categorical
    /// factors as now set up.
    fn screening_default_runs(&self) -> usize {
        let kinds: Vec<ScreeningFactor> = self
            .screening_levels()
            .iter()
            .map(|l| l.map_or(ScreeningFactor::Continuous, |levels| ScreeningFactor::Categorical { levels }))
            .collect();
        categorical_screening::default_runs(&kinds, self.interactions)
    }

    /// Model and runs of a screening design with categorical factors.
    fn screening_settings(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Model:");
            ui.radio_value(&mut self.interactions, false, "Main effects");
            ui.radio_value(&mut self.interactions, true, "Main effects and two-factor interactions");
        });
        let default = self.screening_default_runs();
        ui.horizontal(|ui| {
            ui.label("Runs:");
            ui.checkbox(&mut self.default_runs, format!("Default ({default})"));
            if self.default_runs {
                self.runs = default;
            }
            ui.add_enabled(!self.default_runs, egui::DragValue::new(&mut self.runs).range(1..=500))
                .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, "Runs"));
        });
        kit::note(
            ui,
            "Continuous factors run at their low and high settings; categorical factors at each of their levels, \
             which come out as a text column L1, L2, …. The runs are chosen by coordinate exchange to maximize \
             det(XᵀX), with each categorical factor coded by orthonormal contrasts, so a design that balances every \
             pair of factors scores 100%.",
        );
    }

    /// Extra runs and center runs of a definitive screening design, and the
    /// table comparing nearby choices.
    fn dsd_settings(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Extra runs:");
            ui.add(egui::DragValue::new(&mut self.dsd_extra_runs).range(0..=40))
                .on_hover_text(
                    "Runs beyond the smallest design, added in pairs from a design for more factors whose extra \
                     columns are left out: more error degrees of freedom and less correlated quadratic effects",
                )
                .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, "Extra runs"));
            ui.label("Center runs:");
            ui.add(egui::DragValue::new(&mut self.dsd_center_runs).range(0..=MAX_DSD_CENTER_RUNS))
                .on_hover_text("Runs with every factor at its middle setting, in all (the usual design has one)")
                .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, "Center runs"));
        });
        let k = self.factors.len();
        let options = DsdOptions { extra_runs: self.dsd_extra_runs, center_runs: self.dsd_center_runs };
        if self.dsd_comparison.as_ref().is_none_or(|(key, _)| *key != (k, options)) {
            let table = dsd_comparison_table(k, options).map_err(|e| e.to_string());
            self.dsd_comparison = Some(((k, options), table));
        }
        let Some((_, comparison)) = &self.dsd_comparison else { return };
        egui::CollapsingHeader::new("Compare run sizes").id_salt("dsd_compare").show(ui, |ui| match comparison {
            Ok(table) => {
                kit::note(
                    ui,
                    "Nearby choices of extra and center runs: error degrees of freedom left after fitting the main \
                     effects (and the quadratic effects too), the D-efficiency of the main effects, the largest \
                     correlations among quadratic effects and between two-factor interactions, and the mean standard \
                     error of a quadratic effect in units of σ. Your choice is in bold.",
                );
                comparison_grid(ui, table, options);
            }
            Err(e) => {
                ui.colored_label(theme::ERROR, sentence_fragment(e));
            }
        });
    }

    /// Whole plots, subplots and variance ratios of a split-plot design.
    fn plot_settings(&mut self, ui: &mut egui::Ui) {
        let split_split = self.split_split();
        ui.horizontal(|ui| {
            ui.label("Whole plots:");
            ui.add(egui::DragValue::new(&mut self.whole_plots).range(1..=self.runs.max(1)))
                .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, "Whole plots"));
            if split_split {
                ui.label("Subplots:");
                ui.add(egui::DragValue::new(&mut self.subplots).range(1..=self.runs.max(1)))
                    .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, "Subplots"));
            }
        });
        ui.horizontal(|ui| {
            ui.label("Variance ratio η (whole plot ÷ run):");
            ui.add(egui::DragValue::new(&mut self.whole_plot_ratio).speed(0.1).range(0.0..=100.0));
            if split_split {
                ui.label("subplot ÷ run:");
                ui.add(egui::DragValue::new(&mut self.subplot_ratio).speed(0.1).range(0.0..=100.0));
            }
        });
        kit::note(
            ui,
            if split_split {
                "Very hard factors stay fixed within each whole plot and hard factors within each subplot; runs are \
                 shared out as evenly as possible. The table gets Whole Plot and Subplot columns, and the order is \
                 randomized within those restrictions."
            } else {
                "Hard factors stay fixed within each whole plot; runs are shared out as evenly as possible. The \
                 table gets a Whole Plot column, and the order is randomized within whole plots. η is your prior \
                 guess of the whole-plot variance relative to the run-to-run variance (1 when unknown)."
            },
        );
    }
}

/// The definitive screening comparison as a grid, the row of `chosen` in
/// bold.
fn comparison_grid(ui: &mut egui::Ui, table: &DataTable, chosen: DsdOptions) {
    let n = table.n_rows();
    let value = |j: usize, r: usize| table.column(j).and_then(|c| c.as_numeric()).map_or(f64::NAN, |v| v[r]);
    egui::ScrollArea::horizontal().id_salt("dsd_compare_scroll").show(ui, |ui| {
        egui::Grid::new("dsd_compare_grid").striped(true).spacing([12.0, 3.0]).show(ui, |ui| {
            for c in table.columns() {
                ui.label(RichText::new(&c.name).small().strong());
            }
            ui.end_row();
            for r in 0..n {
                let bold = value(0, r) as usize == chosen.extra_runs && value(1, r) as usize == chosen.center_runs;
                for j in 0..table.n_cols() {
                    let v = value(j, r);
                    let text = match j {
                        0..=4 => format!("{v:.0}"),
                        5 => format!("{v:.1}"),
                        _ => format!("{v:.3}"),
                    };
                    let text = RichText::new(text).monospace();
                    ui.label(if bold { text.strong() } else { text });
                }
                ui.end_row();
            }
        });
    });
}

/// The optional constraints of an optimal design: a multi-line box
/// (accessible name "Constraints") with a short note on the syntax.
pub(crate) fn constraints_box(ui: &mut egui::Ui, text: &mut String) {
    ui.label("Constraints (optional, one per line, in the factors' units):");
    ui.add(
        egui::TextEdit::multiline(text)
            .desired_rows(3)
            .desired_width(f32::INFINITY)
            .code_editor()
            .hint_text("2*Temp + Time <= 400\nexclude Temp > 180 and Time > 25"),
    )
    .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Constraints"));
    kit::note(
        ui,
        "Linear limits such as 2*Temp + Time <= 400 or Temp - Time >= 10 (also <, >, ≤, ≥), and combinations \
         to leave out: exclude Temp > 180 and Time > 25. Every run satisfies them; lines starting with # are \
         ignored.",
    );
}

/// An error message as a "⚠" note: first letter capitalised, no full stop.
pub(crate) fn sentence_fragment(message: &str) -> String {
    let text = message.trim().trim_end_matches('.');
    let mut chars = text.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

impl Remember for DesignDialog {
    fn memory_key(&self) -> String {
        if self.restricted() { format!("{} (split-plot)", KIND_NAMES[self.kind]) } else { KIND_NAMES[self.kind].into() }
    }

    fn set_edit(&mut self, _report: Option<u64>) {}
}
fn kind_help(kind: usize) -> &'static str {
    match kind {
        0 => "Every combination of low and high settings: 2ᵏ runs; estimates all interactions.",
        1 => "Every combination of three levels: 3ᵏ runs; supports quadratic models.",
        2 => "A fraction of the 2ᵏ factorial; fewer runs at the cost of aliasing (see resolution).",
        3 => "Two-level screening of many factors in a multiple of 4 runs; main effects only.",
        4 => {
            "Three-level screening in 2m + 1 runs: main effects clear of interactions and curvature. Extra runs and \
              center runs buy error degrees of freedom; compare the choices below."
        }
        CATEGORICAL_SCREENING => {
            "Screening of two-level continuous factors together with categorical factors of any number of levels, \
              chosen by coordinate exchange (D-optimal)."
        }
        5 => "Factorial, axial and center points for fitting a quadratic response surface.",
        6 => "Three-level response-surface design that avoids the corners of the region.",
        7 => {
            "Runs chosen by coordinate exchange to maximize det(XᵀX) for the model you specify: the most precise \
              coefficient estimates. Mark factors that are hard to change for a split-plot design."
        }
        10 => {
            "A Latin hypercube (each factor run once in each of n equal slices of its range) whose points are spread \
              apart by simulated annealing on the φ₅₀ criterion: for computer experiments."
        }
        11 => {
            "Runs placed as far apart as possible (largest minimum distance between runs), not restricted to a \
              lattice: for computer experiments."
        }
        12 => {
            "Runs placed so that no part of the region is far from a run (smallest fill distance), by clustering: for \
              computer experiments."
        }
        13 => {
            "A lattice design whose points are spread as evenly as possible (smallest centered L2 discrepancy), by \
              threshold accepting: for computer experiments."
        }
        MIXED_LEVELS => {
            "Every combination of each factor's levels, a different number per factor, spaced evenly from the \
              lowest to the highest setting."
        }
        _ => {
            "Runs chosen by coordinate exchange to minimize the average prediction variance over the region for the \
              model you specify: the most precise predictions (preferred for response surfaces)."
        }
    }
}
