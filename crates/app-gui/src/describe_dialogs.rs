//! Dialogs for the Describe menu: one-column distributions, distribution
//! identification and two-column (bivariate) analyses.

use crate::dialog_kit::ComboExt as _;
use app_model::{
    BivariateRequest, CensorDirection, Censoring, DistributionFitRequest, DistributionRequest, FitFamily, SavedReport,
};
use eframe::egui;
use stat_core::descriptive::QuantileMethod;
use stat_data::DataTable;

use crate::dialog_kit::{self as kit, Dialog, Outcome, Readiness, Remember};

const QUANTILE_METHODS: [(QuantileMethod, &str); 9] = [
    (QuantileMethod::MedianUnbiased, "Type 8 — median-unbiased (recommended)"),
    (QuantileMethod::NormalUnbiased, "Type 9 — unbiased for normal data"),
    (QuantileMethod::Linear, "Type 7 — linear, 1 + p(n − 1)"),
    (QuantileMethod::Weibull, "Type 6 — p(n + 1)"),
    (QuantileMethod::Midpoint, "Type 5 — midpoint knots"),
    (QuantileMethod::LinearCdf, "Type 4 — linear empirical CDF"),
    (QuantileMethod::NearestEven, "Type 3 — nearest even order statistic"),
    (QuantileMethod::AveragedInverseCdf, "Type 2 — inverse CDF, averaged"),
    (QuantileMethod::InverseCdf, "Type 1 — inverse empirical CDF"),
];

/// Choose one or more columns; each gets its own distribution report.
#[derive(Clone)]
pub struct DistributionDialog {
    edit: Option<u64>,
    chosen: Vec<bool>,
    method: QuantileMethod,
    censoring: CensorMode,
    /// The indicator or upper-end column.
    censor_column: Option<usize>,
    direction: CensorDirection,
}

/// How the dialog marks censored values.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum CensorMode {
    /// Every value exact.
    None,
    /// A column flags censored values.
    Indicator,
    /// The chosen column is the lower end, another the upper end.
    Interval,
}

impl CensorMode {
    const ALL: [Self; 3] = [Self::None, Self::Indicator, Self::Interval];

    fn label(self) -> &'static str {
        match self {
            Self::None => "None (every value is exact)",
            Self::Indicator => "A column flags censored values",
            Self::Interval => "Lower and upper ends in two columns",
        }
    }
}

impl DistributionDialog {
    pub fn new(table: &DataTable) -> Self {
        Self {
            edit: None,
            chosen: vec![false; table.n_cols()],
            method: QuantileMethod::default(),
            censoring: CensorMode::None,
            censor_column: None,
            direction: CensorDirection::Left,
        }
    }

    /// The dialog filled from a saved distribution report.
    pub fn from_saved(saved: &SavedReport, table: &DataTable) -> Option<Self> {
        let SavedReport::Distribution { request, .. } = saved else { return None };
        let (censoring, censor_column, direction) = match request.censoring {
            None => (CensorMode::None, None, CensorDirection::Left),
            Some(Censoring::Indicator { column, direction }) => {
                (CensorMode::Indicator, kit::valid(table, column), direction)
            }
            Some(Censoring::Interval { upper }) => (CensorMode::Interval, kit::valid(table, upper), CensorDirection::Left),
        };
        Some(Self {
            edit: None,
            chosen: kit::mask(table.n_cols(), &[request.column]),
            method: request.quantile_method,
            censoring,
            censor_column,
            direction,
        })
    }

    /// One request per chosen column, in table order. Editing a report
    /// replaces it with the first and adds the others.
    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<Vec<DistributionRequest>> {
        let dialog = Dialog::new("Distribution").editing(self.edit);
        match dialog.show(ctx, |ui| self.contents(ui, table)) {
            Outcome::Reset => {
                *self = Self { edit: self.edit, ..Self::new(table) };
                Outcome::Open
            }
            outcome => outcome,
        }
    }

    fn contents(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<Vec<DistributionRequest>> {
        ui.label("Columns (one report each):");
        kit::column_list(ui, "distribution_columns", table, &mut self.chosen, |_| true);
        ui.add_space(4.0);
        ui.label("Censored values (non-detects and detection limits):");
        egui::ComboBox::from_id_salt("censor_mode").selected_text(self.censoring.label()).width(kit::em(ui) * 20.0).show_list(
            ui,
            |ui| {
                for m in CensorMode::ALL {
                    ui.selectable_value(&mut self.censoring, m, m.label());
                }
            },
        );
        match self.censoring {
            CensorMode::None => {}
            CensorMode::Indicator => {
                egui::Grid::new("censor_indicator").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                    ui.label("Censored when nonzero:");
                    kit::column_choice(ui, "censor_column", table, &mut self.censor_column, |i| kit::is_numeric(table, i), None);
                    ui.end_row();
                    ui.label("A flagged value is:");
                    egui::ComboBox::from_id_salt("censor_direction")
                        .selected_text(self.direction.label())
                        .width(kit::em(ui) * 20.0)
                        .show_list(ui, |ui| {
                            for d in CensorDirection::ALL {
                                ui.selectable_value(&mut self.direction, d, d.label());
                            }
                        });
                    ui.end_row();
                });
            }
            CensorMode::Interval => {
                egui::Grid::new("censor_interval").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                    ui.label("Upper end:");
                    kit::column_choice(ui, "censor_upper", table, &mut self.censor_column, |i| kit::is_numeric(table, i), None);
                    ui.end_row();
                });
                kit::note(
                    ui,
                    "The chosen columns hold the lower ends. Equal ends are an exact value, a missing lower end \
                     a value below the upper end, a missing upper end a value above the lower end.",
                );
            }
        }
        kit::more_options(ui, |ui| {
            ui.label("Quantile definition (Hyndman & Fan 1996):");
            let current = QUANTILE_METHODS.iter().find(|m| m.0 == self.method).map_or("", |m| m.1);
            egui::ComboBox::from_id_salt("quantile_method").selected_text(current).width(kit::em(ui) * 20.0).show_list(
                ui,
                |ui| {
                    for (m, label) in QUANTILE_METHODS {
                        ui.selectable_value(&mut self.method, m, label);
                    }
                },
            );
        });
        let columns: Vec<usize> =
            kit::chosen_columns(&self.chosen).into_iter().filter(|&c| c < table.n_cols()).collect();
        if columns.is_empty() {
            return Err("Choose at least one column".into());
        }
        let summary = match columns.as_slice() {
            [c] => format!("Distribution of {}", kit::name(table, *c)),
            _ => format!("{} distribution reports", columns.len()),
        };
        let censoring = match (self.censoring, self.censor_column.filter(|&c| c < table.n_cols())) {
            (CensorMode::None, _) => None,
            (_, None) => return Err("Choose the column that marks censored values".into()),
            (CensorMode::Indicator, Some(column)) => Some(Censoring::Indicator { column, direction: self.direction }),
            (CensorMode::Interval, Some(upper)) => Some(Censoring::Interval { upper }),
        };
        if let Some(c) = censoring {
            if columns.contains(&c.column()) {
                return Err(format!("{} cannot be both analysed and mark the censoring", kit::name(table, c.column())));
            }
            if let Some(&bad) = columns.iter().find(|&&c| !kit::is_numeric(table, c)) {
                return Err(format!("{} must be numeric to have censored values", kit::name(table, bad)));
            }
        }
        let summary = if censoring.is_some() { format!("{summary}, with censored values") } else { summary };
        let requests = columns
            .into_iter()
            .map(|column| DistributionRequest { column, quantile_method: self.method, censoring })
            .collect();
        kit::ready(requests, summary)
    }
}

impl Remember for DistributionDialog {
    fn memory_key(&self) -> String {
        "Distribution".into()
    }

    fn set_edit(&mut self, report: Option<u64>) {
        self.edit = report;
    }

    fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        self.chosen = kit::mask(table.n_cols(), columns);
    }
}

/// Choose an X and a Y column.
#[derive(Clone, Default)]
pub struct BivariateDialog {
    edit: Option<u64>,
    x: Option<usize>,
    y: Option<usize>,
    /// Pairwise comparisons of a saved report being edited (kept as they were).
    pairwise: Option<Option<app_model::ComparisonMethod>>,
}

impl BivariateDialog {
    /// The dialog filled from a saved bivariate report.
    pub fn from_saved(saved: &SavedReport, table: &DataTable) -> Option<Self> {
        let SavedReport::Bivariate { request } = saved else { return None };
        Some(Self {
            edit: None,
            x: kit::valid(table, request.x),
            y: kit::valid(table, request.y),
            pairwise: Some(request.pairwise),
        })
    }

    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<BivariateRequest> {
        let dialog = Dialog::new("Bivariate").editing(self.edit);
        match dialog.show(ctx, |ui| self.contents(ui, table)) {
            Outcome::Reset => {
                *self = Self { edit: self.edit, ..Self::default() };
                Outcome::Open
            }
            outcome => outcome,
        }
    }

    fn contents(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<BivariateRequest> {
        kit::note(
            ui,
            "Two continuous columns: correlations and a scatter plot.\n\
             Two categorical columns: contingency table and tests of independence.\n\
             One of each: group means, one-way ANOVA and values by group.",
        );
        egui::Grid::new("bivariate_columns").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label("X (rows):");
            kit::column_choice(ui, "bivariate_x", table, &mut self.x, |_| true, None);
            ui.end_row();
            ui.label("Y (columns):");
            kit::column_choice(ui, "bivariate_y", table, &mut self.y, |_| true, None);
            ui.end_row();
        });
        let (Some(x), Some(y)) = (self.x, self.y) else {
            return Err(if self.x.is_none() { "Choose an X column" } else { "Choose a Y column" }.into());
        };
        if x == y {
            return Err("X and Y must be different columns".into());
        }
        let mut request = BivariateRequest::new(x, y);
        if let Some(pairwise) = self.pairwise {
            request.pairwise = pairwise;
        }
        let kind = match (kit::is_continuous(table, x), kit::is_continuous(table, y)) {
            (true, true) => "correlation and scatter plot",
            (false, false) => "contingency table",
            _ => "group comparison",
        };
        kit::ready(request, format!("{} by {}: {kind}", kit::name(table, y), kit::name(table, x)))
    }
}

impl Remember for BivariateDialog {
    fn memory_key(&self) -> String {
        "Bivariate".into()
    }

    fn set_edit(&mut self, report: Option<u64>) {
        self.edit = report;
    }

    fn prefill(&mut self, columns: &[usize], _table: &DataTable) {
        self.x = columns.first().copied();
        self.y = columns.get(1).copied();
    }
}

/// Choose a continuous column and the distributions to fit to it.
#[derive(Clone)]
pub struct DistributionFitDialog {
    edit: Option<u64>,
    column: Option<usize>,
    families: Vec<(FitFamily, bool)>,
    transformations: bool,
    plots: usize,
}

impl DistributionFitDialog {
    pub fn new(table: &DataTable, current: Option<usize>) -> Self {
        let d = DistributionFitRequest::new(0);
        let column = current.filter(|&c| kit::is_continuous(table, c));
        Self {
            edit: None,
            column,
            families: FitFamily::ALL.iter().map(|&f| (f, true)).collect(),
            transformations: d.transformations,
            plots: d.plots,
        }
    }

    /// The dialog filled from a saved distribution identification.
    pub fn from_saved(saved: &SavedReport, table: &DataTable) -> Option<Self> {
        let SavedReport::DistributionFit { request } = saved else { return None };
        Some(Self {
            edit: None,
            column: kit::valid(table, request.column),
            families: FitFamily::ALL.iter().map(|&f| (f, request.families.contains(&f))).collect(),
            transformations: request.transformations,
            plots: request.plots,
        })
    }

    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<DistributionFitRequest> {
        let dialog = Dialog::new("Identify distribution").editing(self.edit);
        match dialog.show(ctx, |ui| self.contents(ui, table)) {
            Outcome::Reset => {
                *self = Self { edit: self.edit, ..Self::new(table, None) };
                Outcome::Open
            }
            outcome => outcome,
        }
    }

    fn contents(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<DistributionFitRequest> {
        kit::note(
            ui,
            "Fits each chosen distribution to one continuous column by maximum likelihood and ranks them by AIC, \
             with an Anderson–Darling test of each fit.",
        );
        ui.horizontal(|ui| {
            ui.label("Column:");
            kit::column_choice(ui, "fit_column", table, &mut self.column, |i| kit::is_continuous(table, i), None);
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label("Distributions:");
            if ui.small_button("All").clicked() {
                self.families.iter_mut().for_each(|f| f.1 = true);
            }
            if ui.small_button("None").clicked() {
                self.families.iter_mut().for_each(|f| f.1 = false);
            }
        });
        egui::Grid::new("fit_families").num_columns(2).show(ui, |ui| {
            for (i, (family, on)) in self.families.iter_mut().enumerate() {
                ui.checkbox(on, family.label());
                if i % 2 == 1 {
                    ui.end_row();
                }
            }
        });
        kit::more_options(ui, |ui| {
            ui.checkbox(&mut self.transformations, "Also try Johnson and Box–Cox transformations to normality");
            ui.horizontal(|ui| {
                ui.label("Quantile plots for the best");
                ui.add(egui::DragValue::new(&mut self.plots).range(0..=FitFamily::ALL.len()));
                ui.label("fits");
            });
        });
        let families: Vec<FitFamily> = self.families.iter().filter(|f| f.1).map(|f| f.0).collect();
        let Some(column) = self.column else { return Err("Choose a continuous column".into()) };
        if families.is_empty() {
            return Err("Choose at least one distribution".into());
        }
        let summary = format!(
            "{} fitted to {}",
            kit::count(families.len(), "distribution", "distributions"),
            kit::name(table, column)
        );
        kit::ready(
            DistributionFitRequest { column, families, transformations: self.transformations, plots: self.plots },
            summary,
        )
    }
}

impl Remember for DistributionFitDialog {
    fn memory_key(&self) -> String {
        "Identify distribution".into()
    }

    fn set_edit(&mut self, report: Option<u64>) {
        self.edit = report;
    }

    fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        if let Some(&c) = columns.iter().find(|&&c| kit::is_continuous(table, c)) {
            self.column = Some(c);
        }
    }
}
