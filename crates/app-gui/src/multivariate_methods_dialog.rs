//! Dialogs for the Multivariate menu's column-set methods: correlations,
//! the scatterplot matrix, outlier distances and item analysis. Each picks
//! a set of numeric columns plus a few settings.

use app_model::{CorrelationRequest, ItemAnalysisRequest, MissingRows, OutlierDistance, OutlierRequest, SavedReport};
use eframe::egui;
use stat_data::DataTable;

use crate::dialog_kit::{self as kit, Dialog, Outcome, Readiness, Remember};
use crate::report_view::ReportRequest;

/// Which method the dialog sets up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MethodKind {
    Correlations,
    ScatterMatrix,
    OutlierDistances,
    ItemAnalysis,
}

#[derive(Clone)]
pub struct MethodDialog {
    /// The report being edited (`None`: a new report).
    edit: Option<u64>,
    kind: MethodKind,
    chosen: Vec<bool>,
    /// Items to reverse-score (item analysis).
    reversed: Vec<bool>,
    missing: MissingRows,
    level: f64,
    spearman: bool,
    partial: bool,
    covariance: bool,
    distance: OutlierDistance,
    alpha: f64,
    /// Give the response scale's ends instead of using the observed range.
    fixed_scale: bool,
    scale: (f64, f64),
    /// Colour column of the scatterplot matrix.
    color: Option<usize>,
}

impl MethodDialog {
    pub fn new(kind: MethodKind, table: &DataTable) -> Self {
        Self {
            edit: None,
            kind,
            chosen: vec![false; table.n_cols()],
            reversed: vec![false; table.n_cols()],
            missing: MissingRows::Pairwise,
            level: 0.95,
            spearman: false,
            partial: true,
            covariance: false,
            distance: OutlierDistance::Mahalanobis,
            alpha: OutlierRequest::DEFAULT_ALPHA,
            fixed_scale: true,
            scale: (1.0, 5.0),
            color: None,
        }
    }

    /// The dialog filled from a saved correlations, outlier-distance or
    /// item-analysis report. (A scatterplot matrix is a graph, changed on
    /// its graph canvas.)
    pub fn from_saved(saved: &SavedReport, table: &DataTable) -> Option<Self> {
        let n = table.n_cols();
        let d = match saved {
            SavedReport::Correlations { request } => Self {
                chosen: kit::mask(n, &request.columns),
                missing: request.missing,
                level: request.level,
                spearman: request.spearman,
                partial: request.partial,
                covariance: request.covariance,
                ..Self::new(MethodKind::Correlations, table)
            },
            SavedReport::OutlierDistances { request } => Self {
                chosen: kit::mask(n, &request.columns),
                distance: request.distance,
                alpha: request.alpha,
                ..Self::new(MethodKind::OutlierDistances, table)
            },
            SavedReport::ItemAnalysis { request } => {
                let mut d = Self {
                    chosen: kit::mask(n, &request.items),
                    reversed: kit::mask(n, &request.reversed),
                    level: request.level,
                    ..Self::new(MethodKind::ItemAnalysis, table)
                };
                match request.scale {
                    Some(scale) => d.scale = scale,
                    // Without reversed items the scale is not recorded.
                    None => d.fixed_scale = request.reversed.is_empty(),
                }
                d
            }
            _ => return None,
        };
        Some(d)
    }

    fn title(&self) -> &'static str {
        match self.kind {
            MethodKind::Correlations => "Correlations",
            MethodKind::ScatterMatrix => "Scatterplot matrix",
            MethodKind::OutlierDistances => "Outlier distances",
            MethodKind::ItemAnalysis => "Item analysis",
        }
    }

    fn intro(&self) -> &'static str {
        match self.kind {
            MethodKind::Correlations => {
                "Pearson correlations of every pair with intervals and tests, a heat map, and optionally \
                 Spearman's ρ, partial correlations and covariances."
            }
            MethodKind::ScatterMatrix => {
                "Every pair of columns in a grid of linked panels, with density ellipses off the diagonal and \
                 histograms on it. Opens as a graph canvas, so layers and colours can be changed there."
            }
            MethodKind::OutlierDistances => {
                "How far each row lies from the centre of all rows, allowing for the correlations between the \
                 columns, with reference limits for multivariate normal data."
            }
            MethodKind::ItemAnalysis => {
                "Reliability of a scale made by adding several items (questionnaire answers): Cronbach's alpha \
                 and how each item fits with the others."
            }
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<ReportRequest> {
        let dialog = Dialog::new(self.title()).editing(self.edit);
        let outcome = dialog.show(ctx, |ui| {
            ui.set_max_width(kit::em(ui) * 30.0);
            kit::note(ui, self.intro());
            ui.add_space(4.0);
            self.contents(ui, table)
        });
        match outcome {
            Outcome::Reset => {
                *self = Self { edit: self.edit, ..Self::new(self.kind, table) };
                Outcome::Open
            }
            outcome => outcome,
        }
    }

    fn contents(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<ReportRequest> {
        let name = |i: usize| kit::name(table, i).to_owned();
        let continuous_only = self.kind == MethodKind::ScatterMatrix;
        let usable = |i: usize| if continuous_only { kit::is_continuous(table, i) } else { kit::is_numeric(table, i) };
        let items = self.kind == MethodKind::ItemAnalysis;
        ui.label(if items { "Item columns:" } else { "Columns:" });
        kit::column_list(ui, "method_columns", table, &mut self.chosen, usable);
        let columns: Vec<usize> = kit::chosen_columns(&self.chosen).into_iter().filter(|&i| usable(i)).collect();
        ui.separator();
        // Why the settings cannot be used, if they cannot.
        let mut invalid = None;
        let (request, min) = match self.kind {
            MethodKind::Correlations => {
                ui.horizontal(|ui| {
                    ui.label("Rows with a missing value:");
                    for m in [MissingRows::Pairwise, MissingRows::Listwise] {
                        ui.radio_value(&mut self.missing, m, m.label());
                    }
                });
                kit::more_options(ui, |ui| {
                    level_ui(ui, &mut self.level, "Confidence level of the intervals:");
                    ui.checkbox(&mut self.spearman, "Spearman's rank correlations");
                    ui.checkbox(&mut self.partial, "Partial correlations (each pair given the other columns)");
                    ui.checkbox(&mut self.covariance, "Covariance matrix");
                });
                let request = CorrelationRequest {
                    missing: self.missing,
                    level: self.level,
                    spearman: self.spearman,
                    partial: self.partial,
                    covariance: self.covariance,
                    ..CorrelationRequest::new(columns.clone())
                };
                (ReportRequest::Correlations(request), 2)
            }
            MethodKind::ScatterMatrix => {
                let chosen = self.chosen.clone();
                ui.horizontal(|ui| {
                    ui.label("Colour and marker by:");
                    let allow = |i: usize| !chosen.get(i).copied().unwrap_or(false);
                    kit::column_choice(ui, "matrix_color", table, &mut self.color, allow, Some("None"));
                });
                let spec = stat_viz::GraphSpec {
                    encoding: stat_viz::EncodingPlan {
                        x: columns.iter().map(|&i| name(i)).collect(),
                        color: self.color.map(name),
                        ..Default::default()
                    },
                    ellipse: Some(stat_viz::EllipseElement::default()),
                    form: stat_viz::GraphForm::ScatterMatrix,
                    ..stat_viz::GraphSpec::default()
                };
                (ReportRequest::Graph(Box::new(spec)), 2)
            }
            MethodKind::OutlierDistances => {
                ui.horizontal(|ui| {
                    ui.label("Plot:");
                    for d in OutlierDistance::ALL {
                        ui.radio_value(&mut self.distance, d, d.label());
                    }
                });
                kit::more_options(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Upper-tail probability α of the limit:");
                        ui.add(egui::DragValue::new(&mut self.alpha).range(0.0001..=0.5).speed(0.001));
                    });
                });
                let request = OutlierRequest {
                    distance: self.distance,
                    alpha: self.alpha,
                    ..OutlierRequest::new(columns.clone())
                };
                (ReportRequest::OutlierDistances(request), 1)
            }
            MethodKind::ItemAnalysis => {
                ui.label("Reverse-scored items (negatively worded):")
                    .on_hover_text("Score these items the other way round: low + high − answer");
                if columns.is_empty() {
                    kit::note(ui, "Choose the items first.");
                } else {
                    let items_chosen = columns.clone();
                    kit::column_list(ui, "method_reversed", table, &mut self.reversed, |i| items_chosen.contains(&i));
                }
                let reversed: Vec<usize> =
                    columns.iter().copied().filter(|&i| self.reversed.get(i).copied().unwrap_or(false)).collect();
                if !reversed.is_empty() {
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut self.fixed_scale, "Answers run from");
                        ui.add_enabled(self.fixed_scale, egui::DragValue::new(&mut self.scale.0).speed(1.0));
                        ui.label("to");
                        ui.add_enabled(self.fixed_scale, egui::DragValue::new(&mut self.scale.1).speed(1.0));
                    })
                    .response
                    .on_hover_text("Otherwise the smallest and largest answers to any item are the scale's ends");
                }
                kit::more_options(ui, |ui| level_ui(ui, &mut self.level, "Confidence level of alpha's interval:"));
                // The scale's ends only matter for reversing; with no reversed
                // item they are not used (nor checked).
                let scale = (self.fixed_scale && !reversed.is_empty()).then_some(self.scale);
                if scale.is_some_and(|(low, high)| low >= high) {
                    invalid = Some("Make the scale's low end smaller than its high end");
                }
                let request = ItemAnalysisRequest {
                    reversed,
                    scale,
                    level: self.level,
                    ..ItemAnalysisRequest::new(columns.clone())
                };
                (ReportRequest::ItemAnalysis(request), 2)
            }
        };
        if columns.len() < min {
            return Err(match (items, min) {
                (true, _) => "Choose at least two items",
                (false, 1) => "Choose at least one column",
                _ => "Choose at least two columns",
            }
            .into());
        }
        if let Some(why) = invalid {
            return Err(why.into());
        }
        let n = kit::count(columns.len(), "column", "columns");
        let summary = match &request {
            ReportRequest::Correlations(_) => format!("Correlations of {n}"),
            ReportRequest::Graph(_) => match self.color {
                Some(c) => format!("Scatterplot matrix of {n}, coloured by {}", kit::name(table, c)),
                None => format!("Scatterplot matrix of {n}"),
            },
            ReportRequest::OutlierDistances(_) => format!("{} of {n}", self.distance.label()),
            ReportRequest::ItemAnalysis(r) if r.reversed.is_empty() => {
                format!("Item analysis of {}", kit::count(columns.len(), "item", "items"))
            }
            ReportRequest::ItemAnalysis(r) => format!(
                "Item analysis of {}, {} reversed",
                kit::count(columns.len(), "item", "items"),
                r.reversed.len()
            ),
            _ => String::new(),
        };
        kit::ready(request, summary)
    }
}

impl Remember for MethodDialog {
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

fn level_ui(ui: &mut egui::Ui, level: &mut f64, label: &str) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(egui::DragValue::new(level).range(0.5..=0.999).speed(0.001));
    });
}
