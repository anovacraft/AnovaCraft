//! Dialog for augmenting the design held in the active table: choose the
//! factor and response columns and a method (fold-over, center runs, axial
//! runs, D-optimal runs or new whole plots of a split-plot design); the preview shows how many runs are added and
//! the notes, and Create opens the augmented design as a new document
//! ([`app_model::design_augment`]).

use crate::dialog_kit::ComboExt as _;
use app_model::DeclaredFactor;
use app_model::design_augment::{AugmentMethod, AugmentRequest, augment_table};
use eframe::egui::{self, RichText};
use stat_data::DataTable;
use stat_doe::ModelFormula;

use crate::design_dialog::sentence_fragment;
use crate::dialog_kit::{self as kit, Dialog, Outcome, Readiness, Remember};

/// The dialog's title.
pub const TITLE: &str = "Augment design";

/// Methods, in the order shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Method {
    FoldOver,
    Center,
    Axial,
    Optimal,
    WholePlots,
}

/// A preview: rows added and notes, or why the request cannot be met.
type Preview = Result<(usize, Vec<String>), String>;

#[derive(Clone)]
pub struct AugmentDialog {
    /// Opened from a menu that cannot restore remembered settings.
    recall: bool,
    factors: Vec<bool>,
    responses: Vec<bool>,
    /// Low and high settings given per factor column (unset: the data's range).
    ranges: Vec<DeclaredFactor>,
    method: Method,
    /// Column to fold on (`None`: every factor).
    fold: Option<usize>,
    centers: usize,
    alpha: f64,
    axial_centers: usize,
    formula: String,
    runs: usize,
    starts: usize,
    seed: u64,
    constraints: String,
    /// New whole plots: the whole-plot column, how many to add, runs in
    /// each and the variance ratio η.
    whole_plot: Option<usize>,
    new_whole_plots: usize,
    runs_per_whole_plot: usize,
    whole_plot_ratio: f64,
    randomize: bool,
    random_seed: u64,
    /// Put the augmented table in the current table's project.
    keep_in_project: bool,
    /// The request and table revision the preview was computed for.
    preview: Option<((AugmentRequest, String, u64), Preview)>,
}

impl AugmentDialog {
    /// A new dialog for `table`, with no columns chosen.
    pub fn new(table: &DataTable) -> Self {
        Self {
            recall: false,
            factors: vec![false; table.n_cols()],
            responses: vec![false; table.n_cols()],
            ranges: Vec::new(),
            method: Method::FoldOver,
            fold: None,
            centers: 3,
            alpha: 1.0,
            axial_centers: 2,
            formula: String::new(),
            runs: 4,
            starts: 20,
            seed: 1,
            constraints: String::new(),
            whole_plot: table.find("Whole Plot"),
            new_whole_plots: 2,
            runs_per_whole_plot: 4,
            whole_plot_ratio: 1.0,
            randomize: true,
            random_seed: 1,
            keep_in_project: true,
            preview: None,
        }
    }

    /// A new dialog from the Design menu, which cannot reach remembered
    /// settings: the app swaps them in before showing it.
    pub fn opening(table: &DataTable) -> Self {
        Self { recall: true, ..Self::new(table) }
    }

    /// Whether the app should restore remembered settings (once).
    pub fn needs_recall(&mut self) -> bool {
        std::mem::take(&mut self.recall)
    }

    fn chosen(&self, table: &DataTable) -> (Vec<usize>, Vec<usize>) {
        let keep = |m: &[bool]| kit::chosen_columns(m).into_iter().filter(|&c| c < table.n_cols()).collect();
        (keep(&self.factors), keep(&self.responses))
    }

    fn request(&self, table: &DataTable) -> Result<AugmentRequest, String> {
        let (factors, responses) = self.chosen(table);
        if factors.is_empty() {
            return Err("Choose the factor columns".into());
        }
        let factors: Vec<DeclaredFactor> = factors
            .iter()
            .map(|&column| {
                self.ranges.iter().find(|r| r.column == column).copied().unwrap_or(DeclaredFactor {
                    column,
                    low: None,
                    high: None,
                })
            })
            .collect();
        let method = match self.method {
            Method::FoldOver => {
                AugmentMethod::FoldOver { factor: self.fold.and_then(|c| factors.iter().position(|f| f.column == c)) }
            }
            Method::Center => AugmentMethod::CenterRuns { count: self.centers },
            Method::Axial => AugmentMethod::Axial { alpha: self.alpha, centers: self.axial_centers },
            Method::Optimal => AugmentMethod::Optimal {
                formula: self.formula.clone(),
                runs: self.runs,
                starts: self.starts,
                seed: self.seed,
                constraints: self.constraints.clone(),
            },
            Method::WholePlots => AugmentMethod::WholePlots {
                whole_plot: self.whole_plot.filter(|&c| c < table.n_cols()).ok_or("Choose the whole plot column")?,
                formula: self.formula.clone(),
                count: self.new_whole_plots,
                runs: self.runs_per_whole_plot,
                ratio: self.whole_plot_ratio,
                starts: self.starts,
                seed: self.seed,
            },
        };
        Ok(AugmentRequest { factors, responses, method, randomize: self.randomize.then_some(self.random_seed) })
    }

    fn refresh_preview(&mut self, table: &DataTable, request: &AugmentRequest) {
        let key = (request.clone(), table.name().to_owned(), table.revision());
        if self.preview.as_ref().is_some_and(|(k, _)| *k == key) {
            return;
        }
        let result = augment_table(table, request).map(|o| (o.added, o.notes)).map_err(|e| e.to_string());
        self.preview = Some((key, result));
    }

    /// Show the dialog; Create returns the augmented table, a one-line
    /// description, and whether it goes in the current table's project.
    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<(DataTable, String, bool)> {
        let dialog = Dialog::new(TITLE).creates().default_width(600.0);
        match dialog.show(ctx, |ui| {
            let height = (ctx.screen_rect().height() * 0.8 - kit::em(ui) * 6.0).max(kit::em(ui) * 8.0);
            egui::ScrollArea::vertical().max_height(height).show(ui, |ui| self.contents(ui, table)).inner
        }) {
            Outcome::Reset => {
                *self = Self::new(table);
                Outcome::Open
            }
            Outcome::Run(request, place) => match augment_table(table, &request) {
                Ok(out) => {
                    let description = format!(
                        "“{}” with {} added run{} (column {} marks them)",
                        out.table.name(),
                        out.added,
                        if out.added == 1 { "" } else { "s" },
                        out.block_column
                    );
                    Outcome::Run((out.table, description, self.keep_in_project), place)
                }
                Err(e) => {
                    let key = (request, table.name().to_owned(), table.revision());
                    self.preview = Some((key, Err(e.to_string())));
                    Outcome::Open
                }
            },
            Outcome::Open => Outcome::Open,
            Outcome::Cancel => Outcome::Cancel,
        }
    }

    fn contents(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<AugmentRequest> {
        kit::note(
            ui,
            "Add runs to the design in this table, keeping the runs already made. The result is a new table: \
             every original row, then the new runs, with a Block column (1 original, 2 added).",
        );
        // Response and factor columns (what is analysed on the left, as in every dialog).
        let (responses, factors) = (self.responses.clone(), self.factors.clone());
        let width = kit::em(ui) * 17.0;
        let plot = if self.method == Method::WholePlots { self.whole_plot } else { None };
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(width);
                ui.strong("Responses");
                let allow = |i: usize| {
                    kit::is_numeric(table, i) && !factors.get(i).copied().unwrap_or(false) && plot != Some(i)
                };
                kit::column_list(ui, "augment_responses", table, &mut self.responses, allow);
            });
            ui.vertical(|ui| {
                ui.set_width(width);
                ui.strong("Factors");
                let allow = |i: usize| !responses.get(i).copied().unwrap_or(false) && plot != Some(i);
                kit::column_list(ui, "augment_factors", table, &mut self.factors, allow);
            });
        });
        kit::note(
            ui,
            "A numeric factor is coded −1 and +1 at its smallest and largest values (change them under More \
             options); a categorical factor needs two levels.",
        );
        ui.separator();
        let (factor_columns, _) = self.chosen(table);
        let names: Vec<String> = factor_columns.iter().map(|&c| kit::name(table, c).to_owned()).collect();
        ui.horizontal_wrapped(|ui| {
            ui.label("Add:");
            ui.radio_value(&mut self.method, Method::FoldOver, "Fold-over");
            ui.radio_value(&mut self.method, Method::Center, "Center runs");
            ui.radio_value(&mut self.method, Method::Axial, "Axial runs");
            ui.radio_value(&mut self.method, Method::Optimal, "D-optimal runs");
            ui.radio_value(&mut self.method, Method::WholePlots, "New whole plots");
        });
        match self.method {
            Method::FoldOver => {
                kit::note(
                    ui,
                    "Repeats the two-level runs with signs reversed. Folding every factor frees the main effects \
                     from two-factor interactions; folding one factor frees it and its interactions.",
                );
                if self.fold.is_some_and(|c| !factor_columns.contains(&c)) {
                    self.fold = None;
                }
                ui.horizontal(|ui| {
                    ui.label("Reverse the signs of:");
                    let shown = self.fold.map_or("Every factor".to_owned(), |c| kit::name(table, c).to_owned());
                    egui::ComboBox::from_id_salt("augment_fold").selected_text(shown).show_list(ui, |ui| {
                        ui.selectable_value(&mut self.fold, None, "Every factor");
                        for &c in &factor_columns {
                            ui.selectable_value(&mut self.fold, Some(c), kit::name(table, c));
                        }
                    });
                });
            }
            Method::Center => {
                ui.horizontal(|ui| {
                    ui.label("Center runs:");
                    ui.add(egui::DragValue::new(&mut self.centers).range(1..=50));
                });
            }
            Method::Axial => {
                kit::note(ui, "Turns a two-level factorial into a central composite design.");
                ui.horizontal(|ui| {
                    ui.label("Axial distance α (coded units):");
                    ui.add(egui::DragValue::new(&mut self.alpha).speed(0.01).range(0.1..=10.0));
                    if ui.small_button("Face-centered (1)").clicked() {
                        self.alpha = 1.0;
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Center runs:");
                    ui.add(egui::DragValue::new(&mut self.axial_centers).range(0..=50));
                });
            }
            Method::Optimal | Method::WholePlots => {
                if self.method == Method::WholePlots {
                    self.whole_plot_settings(ui, table);
                } else {
                    kit::note(
                        ui,
                        "Adds runs chosen by coordinate exchange to maximize det(XᵀX) of all runs for the model, \
                         keeping the existing runs fixed (for example to add terms the design cannot estimate).",
                    );
                }
                ui.label("Model (terms joined by +; use * for interactions and ^2 for squares):");
                if self.formula.trim().is_empty() && !names.is_empty() {
                    self.formula = ModelFormula::interactions(names.len()).describe(&names);
                }
                ui.add(egui::TextEdit::singleline(&mut self.formula).desired_width(f32::INFINITY))
                    .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Model"));
                ui.horizontal(|ui| {
                    ui.label("Presets:");
                    let k = names.len();
                    if ui.small_button("Main effects").clicked() {
                        self.formula = ModelFormula::main_effects(k).describe(&names);
                    }
                    if ui.small_button("+ Interactions").clicked() {
                        self.formula = ModelFormula::interactions(k).describe(&names);
                    }
                    if ui.small_button("Quadratic").clicked() {
                        self.formula = ModelFormula::quadratic(k).describe(&names);
                    }
                });
                if self.method == Method::Optimal {
                    ui.horizontal(|ui| {
                        ui.label("Runs to add:");
                        ui.add(egui::DragValue::new(&mut self.runs).range(1..=500));
                    });
                    crate::design_dialog::constraints_box(ui, &mut self.constraints);
                }
            }
        }
        kit::more_options(ui, |ui| {
            if matches!(self.method, Method::Optimal | Method::WholePlots) {
                ui.horizontal(|ui| {
                    ui.label("Random starts:");
                    ui.add(egui::DragValue::new(&mut self.starts).range(1..=500));
                    ui.label("Seed:");
                    ui.add(crate::dialog_kit::seed_value(&mut self.seed));
                });
            }
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.randomize, "Randomize the order of the new runs, seed");
                ui.add_enabled(self.randomize, crate::dialog_kit::seed_value(&mut self.random_seed));
            });
            self.range_settings(ui, table, &factor_columns);
        });
        kit::keep_in_project(ui, &mut self.keep_in_project);

        let request = self.request(table)?;
        self.refresh_preview(table, &request);
        match &self.preview {
            Some((_, Ok((added, notes)))) => {
                ui.separator();
                egui::ScrollArea::vertical().id_salt("augment_notes").max_height(kit::em(ui) * 9.0).show(ui, |ui| {
                    for note in notes {
                        ui.label(RichText::new(note).small().monospace());
                    }
                });
                let summary = format!("Adds {} to {}", kit::count(*added, "run", "runs"), table.name());
                kit::ready(request, summary)
            }
            Some((_, Err(e))) => Err(sentence_fragment(e)),
            None => Err("Set up the augmentation".into()),
        }
    }

    /// The whole-plot column, and the number and size of the new whole plots.
    fn whole_plot_settings(&mut self, ui: &mut egui::Ui, table: &DataTable) {
        kit::note(
            ui,
            "Adds whole plots to a split-plot design, keeping the runs already made. Factors constant within every \
             existing whole plot are the hard-to-change ones and stay fixed within each new whole plot; the new \
             settings are chosen by coordinate exchange to maximize det(XᵀV⁻¹X) of all runs.",
        );
        if self.whole_plot.is_some_and(|c| c >= table.n_cols() || self.factors.get(c).copied().unwrap_or(false)) {
            self.whole_plot = None;
        }
        ui.horizontal(|ui| {
            ui.label("Whole plot column:");
            let shown = self.whole_plot.map_or("Choose…".to_owned(), |c| kit::name(table, c).to_owned());
            egui::ComboBox::from_id_salt("augment_whole_plot")
                .selected_text(shown)
                .show_list(ui, |ui| {
                    for c in 0..table.n_cols() {
                        if self.factors.get(c).copied().unwrap_or(false)
                            || self.responses.get(c).copied().unwrap_or(false)
                        {
                            continue;
                        }
                        ui.selectable_value(&mut self.whole_plot, Some(c), kit::name(table, c));
                    }
                })
                .response
                .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, "Whole plot column"));
        });
        ui.horizontal(|ui| {
            ui.label("New whole plots:");
            ui.add(egui::DragValue::new(&mut self.new_whole_plots).range(1..=100))
                .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, "New whole plots"));
            ui.label("Runs in each:");
            ui.add(egui::DragValue::new(&mut self.runs_per_whole_plot).range(1..=100))
                .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, "Runs in each"));
        });
        ui.horizontal(|ui| {
            ui.label("Variance ratio η (whole plot ÷ run):");
            ui.add(egui::DragValue::new(&mut self.whole_plot_ratio).speed(0.1).range(0.0..=100.0));
        });
    }

    /// Low and high settings of each numeric factor (unchecked: the data's range).
    fn range_settings(&mut self, ui: &mut egui::Ui, table: &DataTable, factors: &[usize]) {
        let numeric: Vec<usize> = factors.iter().copied().filter(|&c| kit::is_continuous(table, c)).collect();
        if numeric.is_empty() {
            return;
        }
        ui.label("Factor settings coded −1 and +1 (unchecked: the data's smallest and largest values):");
        egui::Grid::new("augment_ranges").num_columns(3).show(ui, |ui| {
            for &c in &numeric {
                let at = match self.ranges.iter().position(|r| r.column == c) {
                    Some(i) => i,
                    None => {
                        self.ranges.push(DeclaredFactor { column: c, low: None, high: None });
                        self.ranges.len() - 1
                    }
                };
                let values = table.column(c).and_then(|col| col.as_numeric()).unwrap_or(&[]);
                let finite = values.iter().copied().filter(|v| v.is_finite());
                let (lo, hi) = finite.fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(v), b.max(v)));
                ui.label(kit::name(table, c));
                let DeclaredFactor { low, high, .. } = &mut self.ranges[at];
                for (label, value, default) in [("Low", low, lo), ("High", high, hi)] {
                    ui.horizontal(|ui| {
                        let mut used = value.is_some();
                        ui.checkbox(&mut used, label);
                        let mut v = value.unwrap_or(if default.is_finite() { default } else { 0.0 });
                        ui.add_enabled(used, egui::DragValue::new(&mut v).speed(0.01));
                        *value = used.then_some(v);
                    });
                }
                ui.end_row();
            }
        });
    }
}

impl Remember for AugmentDialog {
    fn memory_key(&self) -> String {
        TITLE.into()
    }

    fn set_edit(&mut self, _report: Option<u64>) {}

    fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        self.factors = kit::mask(table.n_cols(), columns);
    }
}
