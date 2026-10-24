//! Dialog for Multivariate ▸ Principal coordinates…: distances measured from
//! columns (any of the distance measures) or a distance matrix stored as
//! columns, with an optional correction for negative eigenvalues.

use crate::dialog_kit::ComboExt as _;
use app_model::{DistanceMetric, DistanceSpec, NegativeCorrection, PcoaInput, PcoaRequest, SavedReport};
use eframe::egui;
use stat_data::DataTable;

use crate::dialog_kit::{self as kit, Readiness};

/// Columns and settings of a principal coordinates analysis.
#[derive(Clone)]
pub struct PcoaDialog {
    /// The columns are a distance matrix rather than variables.
    matrix: bool,
    chosen: Vec<bool>,
    metric: DistanceMetric,
    standardize: bool,
    correction: NegativeCorrection,
    color: Option<usize>,
}

impl Default for PcoaDialog {
    fn default() -> Self {
        Self {
            matrix: false,
            chosen: Vec::new(),
            metric: DistanceMetric::Gower,
            standardize: false,
            correction: NegativeCorrection::None,
            color: None,
        }
    }
}

/// Whether `metric` compares categories as well as numbers.
pub(crate) fn takes_categories(metric: DistanceMetric) -> bool {
    matches!(metric, DistanceMetric::Gower | DistanceMetric::Hamming | DistanceMetric::SimpleMatching)
}

/// One line on what a metric suits, for its menu entry.
pub(crate) fn metric_hint(metric: DistanceMetric) -> &'static str {
    match metric {
        DistanceMetric::Euclidean => "Straight-line distance of numeric columns",
        DistanceMetric::Manhattan => "Sum of absolute differences of numeric columns",
        DistanceMetric::Gower => {
            "Mixed types: numbers scaled by their range, categories matched; missing values allowed"
        }
        DistanceMetric::Jaccard => "Presence–absence: shared presences, ignoring joint absences",
        DistanceMetric::BrayCurtis => "Abundances (counts): dissimilarity of non-negative values",
        DistanceMetric::Hamming => "Number of columns that differ",
        DistanceMetric::SimpleMatching => "Share of columns that differ",
    }
}

impl PcoaDialog {
    /// The dialog filled from a saved analysis.
    pub fn from_saved(request: &PcoaRequest, table: &DataTable) -> Self {
        let mut d = Self {
            correction: request.correction,
            color: request.color.and_then(|c| kit::valid(table, c)),
            ..Self::default()
        };
        match &request.input {
            PcoaInput::Variables(spec) => {
                d.chosen = kit::mask(table.n_cols(), &spec.columns);
                d.metric = spec.metric;
                d.standardize = spec.standardize;
            }
            PcoaInput::Matrix(columns) => {
                d.matrix = true;
                d.chosen = kit::mask(table.n_cols(), columns);
            }
        }
        d
    }

    /// Selected columns become the variables.
    pub fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        self.chosen = kit::mask(table.n_cols(), columns);
    }

    /// Draw the settings; ready when they are complete and valid.
    pub fn ui(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<SavedReport> {
        kit::note(
            ui,
            "A map of the rows from any distance between them (classical scaling of a distance matrix), with \
             every eigenvalue, negative ones included, and the share of the variation on each axis.",
        );
        ui.horizontal(|ui| {
            ui.label("Distances:");
            ui.radio_value(&mut self.matrix, false, "Measured from columns");
            ui.radio_value(&mut self.matrix, true, "The columns are a distance matrix");
        });
        let metric = self.metric;
        let matrix = self.matrix;
        let allow = |i: usize| (!matrix && takes_categories(metric)) || kit::is_numeric(table, i);
        if !self.matrix {
            ui.horizontal(|ui| {
                ui.label("Distance:");
                egui::ComboBox::from_id_salt("pcoa_metric")
                    .selected_text(self.metric.label())
                    .width(kit::combo_width(ui))
                    .show_list(ui, |ui| {
                        for m in DistanceMetric::ALL {
                            ui.selectable_value(&mut self.metric, m, m.label()).on_hover_text(metric_hint(m));
                        }
                    });
            });
        }
        ui.label(if self.matrix {
            "Distance matrix columns (one per row, in row order):"
        } else if takes_categories(self.metric) {
            "Columns to compare (any type):"
        } else {
            "Numeric columns to compare:"
        });
        kit::column_list(ui, "pcoa_columns", table, &mut self.chosen, allow);
        let columns: Vec<usize> = kit::chosen_columns(&self.chosen).into_iter().filter(|&i| allow(i)).collect();
        kit::more_options(ui, |ui| {
            if !self.matrix && matches!(self.metric, DistanceMetric::Euclidean | DistanceMetric::Manhattan) {
                ui.checkbox(&mut self.standardize, "Standardize columns first (recommended when units differ)");
            }
            egui::Grid::new("pcoa_settings").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                ui.label("Negative eigenvalues:");
                egui::ComboBox::from_id_salt("pcoa_correction")
                    .selected_text(self.correction.label())
                    .width(kit::combo_width(ui))
                    .show_list(ui, |ui| {
                        for c in NegativeCorrection::ALL {
                            ui.selectable_value(&mut self.correction, c, c.label());
                        }
                    });
                ui.end_row();
                ui.label("Colour points by (optional):");
                kit::column_choice(ui, "pcoa_color", table, &mut self.color, |_| true, Some("None"));
                ui.end_row();
            });
        });
        if columns.is_empty() {
            return Err(if self.matrix {
                "Choose the distance matrix columns"
            } else {
                "Choose the columns to compare"
            }
            .into());
        }
        let input = if self.matrix {
            PcoaInput::Matrix(columns.clone())
        } else {
            let standardize =
                self.standardize && matches!(self.metric, DistanceMetric::Euclidean | DistanceMetric::Manhattan);
            PcoaInput::Variables(DistanceSpec { columns: columns.clone(), metric: self.metric, standardize })
        };
        let request = PcoaRequest { input, correction: self.correction, color: self.color };
        let n = kit::count(columns.len(), "column", "columns");
        let summary = if self.matrix {
            format!("Principal coordinates of a {}-object distance matrix", columns.len())
        } else {
            format!("Principal coordinates of {} distances of {n}", self.metric.label())
        };
        kit::ready(SavedReport::PrincipalCoordinates { request }, summary)
    }
}
