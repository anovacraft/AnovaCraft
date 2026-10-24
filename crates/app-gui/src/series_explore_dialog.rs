//! Dialogs for Series and Signals ▸ Explore: series description, decomposition,
//! trend models and cross-correlation. One report per chosen series column;
//! every dialog takes an optional time column and the seasonal period.

use app_model::time_series_data::default_period;
use app_model::{
    AdfDeterministic, BoxCoxChoice, CrossSettings, DecomposeMethod, DecomposeSettings, DescribeSettings, ExploreMethod,
    Prewhiten, SavedReport, SeriesExploreRequest, SeriesInput, TrendChoice, TrendSettings,
};
use eframe::egui;
use stat_data::DataTable;

use crate::dialog_kit::{self as kit, Dialog, Outcome, Readiness, Remember};

/// Which Explore item the dialog is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExploreKind {
    Describe,
    Decompose,
    Trend,
    CrossCorrelation,
}

impl ExploreKind {
    fn title(self) -> &'static str {
        match self {
            Self::Describe => "Series description",
            Self::Decompose => "Decomposition",
            Self::Trend => "Trend models",
            Self::CrossCorrelation => "Cross-correlation",
        }
    }

    fn of(method: &ExploreMethod) -> Self {
        match method {
            ExploreMethod::Describe(_) => Self::Describe,
            ExploreMethod::Decompose(_) => Self::Decompose,
            ExploreMethod::Trend(_) => Self::Trend,
            ExploreMethod::CrossCorrelation(_) => Self::CrossCorrelation,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BoxCoxMode {
    None,
    Guerrero,
    Fixed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrewhitenMode {
    None,
    Aic,
    Fixed,
}

/// Columns and settings of one Series and Signals ▸ Explore analysis.
#[derive(Clone)]
pub struct SeriesExploreDialog {
    edit: Option<u64>,
    kind: ExploreKind,
    chosen: Vec<bool>,
    input: Option<usize>,
    time: Option<usize>,
    period_auto: bool,
    period: usize,
    auto_lag: bool,
    max_lag: usize,
    box_cox: BoxCoxMode,
    lambda: f64,
    describe: DescribeSettings,
    decompose: DecomposeSettings,
    periodic: bool,
    window: usize,
    trend: TrendSettings,
    prewhiten: PrewhitenMode,
    order: usize,
    confidence: f64,
}

impl SeriesExploreDialog {
    /// A fresh dialog for `kind`.
    pub fn new(kind: ExploreKind, table: &DataTable) -> Self {
        // A date-time column, when the table has one, is the likely time axis.
        let time = (0..table.n_cols()).find(|&i| table.column(i).is_some_and(|c| c.temporal().is_some()));
        Self {
            edit: None,
            kind,
            chosen: vec![false; table.n_cols()],
            input: None,
            time,
            period_auto: true,
            period: 12,
            auto_lag: true,
            max_lag: 24,
            box_cox: BoxCoxMode::None,
            lambda: 0.0,
            describe: DescribeSettings::default(),
            decompose: DecomposeSettings::default(),
            periodic: true,
            window: 13,
            trend: TrendSettings::default(),
            prewhiten: PrewhitenMode::None,
            order: 1,
            confidence: 0.95,
        }
    }

    /// The dialog filled from a saved Explore report.
    pub fn from_saved(saved: &SavedReport, table: &DataTable) -> Option<Self> {
        let SavedReport::SeriesExplore { request } = saved else { return None };
        let mut d = Self::new(ExploreKind::of(&request.method), table);
        d.chosen = kit::mask(table.n_cols(), &[request.series.column]);
        d.time = request.series.time.and_then(|c| kit::valid(table, c));
        d.period_auto = request.series.period.is_none();
        d.period = request.series.period.unwrap_or_else(|| default_period(table, d.time).max(2));
        match &request.method {
            ExploreMethod::Describe(s) => {
                d.describe = s.clone();
                (d.box_cox, d.lambda) = match s.box_cox {
                    BoxCoxChoice::None => (BoxCoxMode::None, 0.0),
                    BoxCoxChoice::Guerrero => (BoxCoxMode::Guerrero, 0.0),
                    BoxCoxChoice::Lambda(l) => (BoxCoxMode::Fixed, l),
                };
                d.auto_lag = s.max_lag.is_none();
                d.max_lag = s.max_lag.unwrap_or(24);
                d.confidence = s.confidence;
            }
            ExploreMethod::Decompose(s) => {
                d.decompose = *s;
                d.periodic = s.seasonal_window.is_none();
                d.window = s.seasonal_window.unwrap_or(13);
            }
            ExploreMethod::Trend(s) => {
                d.trend = *s;
                d.confidence = s.confidence;
            }
            ExploreMethod::CrossCorrelation(s) => {
                d.input = kit::valid(table, s.input);
                d.auto_lag = s.max_lag.is_none();
                d.max_lag = s.max_lag.unwrap_or(12);
                (d.prewhiten, d.order) = match s.prewhiten {
                    Prewhiten::None => (PrewhitenMode::None, 1),
                    Prewhiten::Aic => (PrewhitenMode::Aic, 1),
                    Prewhiten::Order(p) => (PrewhitenMode::Fixed, p),
                };
                d.confidence = s.confidence;
            }
        }
        Some(d)
    }

    /// One request per chosen series column, in table order. Editing a
    /// report replaces it with the first and adds the others.
    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<Vec<SeriesExploreRequest>> {
        let outcome = Dialog::new(self.kind.title()).editing(self.edit).show(ctx, |ui| {
            ui.set_max_width(kit::em(ui) * 32.0);
            kit::note(
                ui,
                match self.kind {
                    ExploreKind::Describe => {
                        "Plot a series in table order and see how its values depend on earlier ones: \
                     autocorrelations, white-noise tests, tests for a unit root, and (from the report's options) \
                     its spectrum."
                    }
                    ExploreKind::Decompose => {
                        "Split a seasonal series into a smooth trend, a repeating seasonal pattern and what is left \
                     over."
                    }
                    ExploreKind::Trend => {
                        "Fit a curve against time, or average the latest values, and forecast the next periods."
                    }
                    ExploreKind::CrossCorrelation => {
                        "Correlate an input series with an output series at a range of delays, to see whether and \
                     when one follows the other."
                    }
                },
            );
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

    fn contents(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<Vec<SeriesExploreRequest>> {
        let series_ok = |i: usize| kit::is_numeric(table, i) && table.column(i).is_some_and(|c| c.temporal().is_none());
        let cross = self.kind == ExploreKind::CrossCorrelation;
        if cross {
            ui.label("Input series (the one that may lead):");
            kit::column_choice(ui, "series_input", table, &mut self.input, series_ok, None);
            ui.label("Output series (one report each):");
        } else {
            ui.label("Series columns (one report each):");
        }
        kit::column_list(ui, ("series_columns", self.kind.title()), table, &mut self.chosen, series_ok);
        ui.label("Time column (labels the time axis; optional):");
        let time_ok = |i: usize| kit::is_numeric(table, i);
        kit::column_choice(ui, "series_time", table, &mut self.time, time_ok, Some("None: number the observations"));
        let implied = default_period(table, self.time);
        ui.horizontal(|ui| {
            ui.label("Observations per seasonal cycle:");
            ui.radio_value(&mut self.period_auto, true, format!("From the time column ({implied})"))
                .on_hover_text("Monthly dates give 12, quarterly 4, weekly 52, daily 7, hourly 24; otherwise 1");
            ui.radio_value(&mut self.period_auto, false, "Set:");
            ui.add_enabled(!self.period_auto, egui::DragValue::new(&mut self.period).range(1..=366));
        });
        let period = if self.period_auto { implied } else { self.period };
        ui.separator();
        match self.kind {
            ExploreKind::Describe => self.describe_options(ui, period),
            ExploreKind::Decompose => self.decompose_options(ui),
            ExploreKind::Trend => self.trend_options(ui),
            ExploreKind::CrossCorrelation => self.cross_options(ui),
        }
        let columns: Vec<usize> =
            kit::chosen_columns(&self.chosen).into_iter().filter(|&c| c < table.n_cols() && series_ok(c)).collect();
        if cross && self.input.is_none() {
            return Err("Choose the input series".into());
        }
        if columns.is_empty() {
            return Err(
                if cross { "Choose at least one output series" } else { "Choose at least one series column" }.into()
            );
        }
        if cross && self.input.is_some_and(|i| columns.contains(&i)) {
            return Err("The input series cannot also be an output series".into());
        }
        if self.time.is_some_and(|t| columns.contains(&t)) {
            return Err("The time column cannot also be a series".into());
        }
        if self.kind == ExploreKind::Decompose && period < 2 {
            return Err("A decomposition needs at least 2 observations per cycle: set the period".into());
        }
        if self.kind == ExploreKind::Describe && self.describe.seasonal_differences > 0 && period < 2 {
            return Err("Seasonal differencing needs at least 2 observations per cycle".into());
        }
        let method = self.method();
        let requests: Vec<SeriesExploreRequest> = columns
            .iter()
            .map(|&column| SeriesExploreRequest {
                series: SeriesInput { column, time: self.time, period: (!self.period_auto).then_some(self.period) },
                method: method.clone(),
            })
            .collect();
        let what = match &method {
            ExploreMethod::Trend(t) => format!("{} trend", t.model.label()),
            ExploreMethod::Decompose(d) => format!("{} decomposition", d.method.label()),
            m => m.label().to_owned(),
        };
        let summary = match columns.as_slice() {
            [c] if cross => {
                format!("{what} of {} with {}", kit::name(table, self.input.unwrap_or(0)), kit::name(table, *c))
            }
            [c] => format!("{what} of {}", kit::name(table, *c)),
            _ => format!("{} reports ({what})", columns.len()),
        };
        kit::ready(requests, summary)
    }

    fn method(&self) -> ExploreMethod {
        match self.kind {
            ExploreKind::Describe => ExploreMethod::Describe(DescribeSettings {
                box_cox: match self.box_cox {
                    BoxCoxMode::None => BoxCoxChoice::None,
                    BoxCoxMode::Guerrero => BoxCoxChoice::Guerrero,
                    BoxCoxMode::Fixed => BoxCoxChoice::Lambda(self.lambda),
                },
                max_lag: (!self.auto_lag).then_some(self.max_lag),
                confidence: self.confidence,
                ..self.describe.clone()
            }),
            ExploreKind::Decompose => ExploreMethod::Decompose(DecomposeSettings {
                seasonal_window: (!self.periodic).then_some(self.window),
                ..self.decompose
            }),
            ExploreKind::Trend => ExploreMethod::Trend(TrendSettings { confidence: self.confidence, ..self.trend }),
            ExploreKind::CrossCorrelation => ExploreMethod::CrossCorrelation(CrossSettings {
                input: self.input.unwrap_or(0),
                max_lag: (!self.auto_lag).then_some(self.max_lag),
                prewhiten: match self.prewhiten {
                    PrewhitenMode::None => Prewhiten::None,
                    PrewhitenMode::Aic => Prewhiten::Aic,
                    PrewhitenMode::Fixed => Prewhiten::Order(self.order),
                },
                confidence: self.confidence,
            }),
        }
    }

    fn lag_choice(&mut self, ui: &mut egui::Ui, label: &str, hint: &str) {
        ui.horizontal(|ui| {
            ui.label(label);
            ui.radio_value(&mut self.auto_lag, true, "Automatic").on_hover_text(hint);
            ui.radio_value(&mut self.auto_lag, false, "Up to");
            ui.add_enabled(!self.auto_lag, egui::DragValue::new(&mut self.max_lag).range(1..=500));
        });
    }

    fn confidence_choice(&mut self, ui: &mut egui::Ui, label: &str) {
        ui.horizontal(|ui| {
            ui.label(label);
            ui.add(egui::DragValue::new(&mut self.confidence).range(0.5..=0.999).speed(0.005).fixed_decimals(3));
        });
    }

    fn describe_options(&mut self, ui: &mut egui::Ui, period: usize) {
        ui.label("Transformation before the analysis:");
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.box_cox, BoxCoxMode::None, "None");
            ui.radio_value(&mut self.box_cox, BoxCoxMode::Guerrero, "Box–Cox, λ by Guerrero's method").on_hover_text(
                "Chooses the λ in [−1, 2] that makes the spread of each seasonal cycle most nearly proportional \
                 to its level; needs positive values",
            );
        });
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.box_cox, BoxCoxMode::Fixed, "Box–Cox with λ =")
                .on_hover_text("(y^λ − 1)/λ; λ = 0 takes the natural logarithm");
            ui.add_enabled(
                self.box_cox == BoxCoxMode::Fixed,
                egui::DragValue::new(&mut self.lambda).range(-3.0..=3.0).speed(0.05),
            );
        });
        ui.horizontal(|ui| {
            ui.label("Differences:");
            ui.add(egui::DragValue::new(&mut self.describe.differences).range(0..=3))
                .on_hover_text("d: replace each value by its change from the one before, d times");
            ui.label("Seasonal differences:");
            ui.add_enabled(period >= 2, egui::DragValue::new(&mut self.describe.seasonal_differences).range(0..=2))
                .on_hover_text(format!(
                    "D: replace each value by its change from the same season one cycle ({period}) earlier"
                ))
                .on_disabled_hover_text("Needs at least 2 observations per seasonal cycle");
        });
        self.lag_choice(ui, "ACF and PACF lags:", "10·log₁₀(n), and at least two seasonal cycles");
        ui.checkbox(&mut self.describe.bartlett, "Bartlett limits on the ACF").on_hover_text(
            "Limits that widen with the lag, for judging whether a moving average of lower order explains the \
             correlations, instead of the white-noise limits ±z/√n",
        );
        ui.checkbox(&mut self.describe.unit_root, "Unit-root and stationarity tests (ADF, KPSS)");
        ui.add_enabled_ui(self.describe.unit_root, |ui| {
            ui.horizontal(|ui| {
                ui.label("Dickey–Fuller terms:");
                for (d, label) in [
                    (AdfDeterministic::None, "None"),
                    (AdfDeterministic::Constant, "Constant"),
                    (AdfDeterministic::ConstantTrend, "Constant and trend"),
                ] {
                    ui.radio_value(&mut self.describe.adf, d, label);
                }
            });
        });
        ui.checkbox(&mut self.describe.spectrum, "Spectral density (periodogram and smoothed spectrum)");
        kit::more_options(ui, |ui| self.confidence_choice(ui, "Confidence level of the limits:"));
    }

    fn decompose_options(&mut self, ui: &mut egui::Ui) {
        ui.label("Method:");
        for m in DecomposeMethod::ALL {
            let hint = match m {
                DecomposeMethod::ClassicalAdditive => {
                    "Trend by a centred moving average over one cycle; seasonal swings of a fixed size"
                }
                DecomposeMethod::ClassicalMultiplicative => {
                    "Trend by a centred moving average; seasonal swings that grow with the level (seasonal \
                     factors); needs positive values"
                }
                DecomposeMethod::Stl => {
                    "Locally weighted regression (Cleveland et al. 1990): the trend reaches the ends of the series \
                     and the seasonal pattern may change slowly"
                }
            };
            ui.radio_value(&mut self.decompose.method, m, m.label()).on_hover_text(hint);
        }
        if self.decompose.method == DecomposeMethod::Stl {
            ui.horizontal(|ui| {
                ui.label("Seasonal pattern:");
                ui.radio_value(&mut self.periodic, true, "The same every cycle");
                ui.radio_value(&mut self.periodic, false, "Changing slowly, span");
                ui.add_enabled(!self.periodic, egui::DragValue::new(&mut self.window).range(7..=999))
                    .on_hover_text("Cycles smoothed together for each season (odd, at least 7); larger is smoother");
            });
            ui.checkbox(&mut self.decompose.robust, "Robust to outliers (downweight unusual values)");
        }
    }

    fn trend_options(&mut self, ui: &mut egui::Ui) {
        ui.label("Model:");
        for m in TrendChoice::ALL {
            let hint = match m {
                TrendChoice::Linear => "y = β₀ + β₁t: a steady change per period",
                TrendChoice::Quadratic => "y = β₀ + β₁t + β₂t²: a change that speeds up or slows down",
                TrendChoice::Exponential => "y = b₀·b₁ᵗ: a steady percentage growth; needs positive values",
                TrendChoice::Logistic => {
                    "y = c/(1 + a·e^(−bt)): growth that slows towards a ceiling c (adoption, saturation); needs \
                     positive values"
                }
                TrendChoice::MovingAverage => "Forecast every period by the mean of the latest observations",
            };
            ui.radio_value(&mut self.trend.model, m, m.label()).on_hover_text(hint);
        }
        if self.trend.model == TrendChoice::MovingAverage {
            ui.horizontal(|ui| {
                ui.label("Observations averaged:");
                ui.add(egui::DragValue::new(&mut self.trend.span).range(1..=500));
            });
        }
        ui.horizontal(|ui| {
            ui.label("Periods to forecast:");
            ui.add(egui::DragValue::new(&mut self.trend.horizon).range(1..=1000));
        });
        kit::more_options(ui, |ui| self.confidence_choice(ui, "Prediction interval level:"));
    }

    fn cross_options(&mut self, ui: &mut egui::Ui) {
        self.lag_choice(ui, "Lags either way:", "10·log₁₀(n/2)");
        ui.label("Prewhitening:");
        ui.radio_value(&mut self.prewhiten, PrewhitenMode::None, "None (correlate the series as they are)");
        ui.radio_value(&mut self.prewhiten, PrewhitenMode::Aic, "Autoregression on the input, order by AIC")
            .on_hover_text(
                "Filter both series by an autoregression fitted to the input, so the input's own autocorrelation \
                 does not smear the peak",
            );
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.prewhiten, PrewhitenMode::Fixed, "Autoregression of order");
            ui.add_enabled(self.prewhiten == PrewhitenMode::Fixed, egui::DragValue::new(&mut self.order).range(0..=50));
        });
        kit::more_options(ui, |ui| self.confidence_choice(ui, "Confidence level of the limits:"));
    }
}

impl Remember for SeriesExploreDialog {
    fn memory_key(&self) -> String {
        self.kind.title().to_owned()
    }

    fn set_edit(&mut self, report: Option<u64>) {
        self.edit = report;
    }

    fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        self.chosen = kit::mask(table.n_cols(), columns);
    }
}

/// Spans offered for smoothing the periodogram.
const SMOOTHING: [(&[usize], &str); 5] = [
    (&[], "None (raw periodogram)"),
    (&[3], "Light (span 3)"),
    (&[3, 3], "Moderate (spans 3, 3)"),
    (&[5, 5], "Strong (spans 5, 5)"),
    (&[7, 7], "Very strong (spans 7, 7)"),
];

/// The report's options menu items: what the report shows. Edits the
/// request in place; the report recomputes.
pub fn options_menu(ui: &mut egui::Ui, request: &mut SeriesExploreRequest) {
    match &mut request.method {
        ExploreMethod::Describe(d) => {
            ui.checkbox(&mut d.unit_root, "Unit-root and stationarity tests");
            ui.add_enabled_ui(d.unit_root, |ui| {
                ui.menu_button("Dickey–Fuller terms", |ui| {
                    for (det, label) in [
                        (AdfDeterministic::None, "None"),
                        (AdfDeterministic::Constant, "Constant"),
                        (AdfDeterministic::ConstantTrend, "Constant and trend"),
                    ] {
                        if ui.radio(d.adf == det, label).clicked() {
                            d.adf = det;
                            ui.close_menu();
                        }
                    }
                });
            });
            ui.checkbox(&mut d.bartlett, "Bartlett limits on the ACF");
            ui.checkbox(&mut d.spectrum, "Spectral density");
            ui.add_enabled_ui(d.spectrum, |ui| {
                ui.menu_button("Spectrum smoothing", |ui| {
                    for (spans, label) in SMOOTHING {
                        if ui.radio(d.spans == spans, label).on_hover_text("Modified Daniell kernels").clicked() {
                            d.spans = spans.to_vec();
                            ui.close_menu();
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Spectrum against:");
                    ui.radio_value(&mut d.period_axis, false, "Frequency");
                    ui.radio_value(&mut d.period_axis, true, "Period");
                });
            });
        }
        ExploreMethod::Trend(t) => {
            ui.horizontal(|ui| {
                ui.label("Periods to forecast:");
                ui.add(egui::DragValue::new(&mut t.horizon).range(1..=1000));
            });
        }
        ExploreMethod::CrossCorrelation(c) => {
            ui.menu_button("Prewhitening", |ui| {
                for (p, label) in
                    [(Prewhiten::None, "None"), (Prewhiten::Aic, "Autoregression on the input, order by AIC")]
                {
                    if ui.radio(c.prewhiten == p, label).clicked() {
                        c.prewhiten = p;
                        ui.close_menu();
                    }
                }
                if let Prewhiten::Order(p) = c.prewhiten {
                    ui.label(format!("Now: autoregression of order {p} (change it with Edit analysis…)"));
                }
            });
        }
        ExploreMethod::Decompose(_) => {}
    }
}
