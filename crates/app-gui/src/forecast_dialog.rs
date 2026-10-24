//! Dialogs for the Time Series menu's forecasting models: Exponential
//! smoothing… and ARIMA…. Both share the series
//! settings — the columns forecast (one report each), an optional time
//! column, the seasonal period and the forecast horizon and interval levels —
//! drawn by [`SeriesSettings`]; each model adds its own settings.

use crate::dialog_kit::ComboExt as _;
use app_model::time_series_data::default_period;
use app_model::{
    ArimaOrders, ArimaRequest, BoxCoxChoice, ErrorType, EtsSpec, FixedSmoothing, ForecastSettings, SavedReport,
    SeasonalType, SeriesInput, SmoothingModel, SmoothingRequest, TransferRole, TrendType,
};
use eframe::egui;
use stat_data::DataTable;

use crate::dialog_kit::{self as kit, Dialog, Outcome, Readiness, Remember};

/// The series part of a forecasting dialog.
#[derive(Clone)]
pub struct SeriesSettings {
    chosen: Vec<bool>,
    time: Option<usize>,
    auto_period: bool,
    period: usize,
    horizon: usize,
    levels: String,
}

fn levels_text(levels: &[f64]) -> String {
    levels.iter().map(|l| app_model::format::format_number(l * 100.0, 6)).collect::<Vec<_>>().join(", ")
}

fn parse_levels(text: &str) -> Option<Vec<f64>> {
    let v: Option<Vec<f64>> = text
        .split([',', ';', ' '])
        .filter(|s| !s.trim().is_empty())
        .map(|s| {
            s.trim().trim_end_matches('%').parse::<f64>().ok().filter(|p| *p > 0.0 && *p < 100.0).map(|p| p / 100.0)
        })
        .collect();
    v.filter(|v| !v.is_empty())
}

impl SeriesSettings {
    /// Defaults: the table's first date-time column as the time column.
    fn new(table: &DataTable) -> Self {
        let time = (0..table.n_cols()).find(|&c| table.column(c).is_some_and(|c| c.temporal().is_some()));
        Self {
            chosen: vec![false; table.n_cols()],
            time,
            auto_period: true,
            period: default_period(table, time).max(2),
            horizon: ForecastSettings::DEFAULT_HORIZON,
            levels: levels_text(&ForecastSettings::DEFAULT_LEVELS),
        }
    }

    fn from_saved(series: &SeriesInput, forecast: &ForecastSettings, table: &DataTable) -> Self {
        Self {
            chosen: kit::mask(table.n_cols(), &[series.column]),
            time: series.time.and_then(|t| kit::valid(table, t)),
            auto_period: series.period.is_none(),
            period: series.period.unwrap_or_else(|| default_period(table, series.time).max(2)),
            horizon: forecast.horizon,
            levels: levels_text(&forecast.levels),
        }
    }

    fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        let numeric: Vec<usize> = columns
            .iter()
            .copied()
            .filter(|&c| kit::is_numeric(table, c) && table.column(c).is_some_and(|c| c.temporal().is_none()))
            .collect();
        self.chosen = kit::mask(table.n_cols(), &numeric);
        if let Some(&t) = columns.iter().find(|&&c| table.column(c).is_some_and(|c| c.temporal().is_some())) {
            self.time = Some(t);
        }
    }

    /// Draw the series settings.
    fn ui(&mut self, ui: &mut egui::Ui, table: &DataTable) {
        let time = self.time;
        ui.label("Series to forecast (one report each):");
        kit::column_list(ui, "forecast_series", table, &mut self.chosen, |i| {
            kit::is_numeric(table, i) && Some(i) != time
        });
        let suggested = default_period(table, self.time);
        egui::Grid::new("forecast_series_settings").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label("Time (optional):");
            kit::column_choice(
                ui,
                "forecast_time",
                table,
                &mut self.time,
                |i| kit::is_numeric(table, i),
                Some("None (number the rows)"),
            );
            ui.end_row();
            ui.label("Seasonal period:");
            ui.vertical(|ui| {
                let auto = if suggested > 1 {
                    format!("From the time column ({suggested} per cycle)")
                } else {
                    "From the time column (no cycle recognised: 1)".to_owned()
                };
                ui.radio_value(&mut self.auto_period, true, auto);
                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.auto_period, false, "Observations per cycle:");
                    ui.add_enabled(!self.auto_period, egui::DragValue::new(&mut self.period).range(1..=1000));
                });
            });
            ui.end_row();
            ui.label("Forecast steps ahead:");
            ui.add(egui::DragValue::new(&mut self.horizon).range(1..=1000));
            ui.end_row();
            ui.label("Prediction intervals (%):");
            ui.add(egui::TextEdit::singleline(&mut self.levels).desired_width(kit::em(ui) * 8.0).hint_text("80, 95"));
            ui.end_row();
        });
    }

    /// The chosen series columns and forecast settings, or what is missing.
    fn read(&self) -> Result<SeriesChoice, String> {
        let columns: Vec<usize> =
            kit::chosen_columns(&self.chosen).into_iter().filter(|&c| Some(c) != self.time).collect();
        if columns.is_empty() {
            return Err("Choose the series to forecast".into());
        }
        let levels =
            parse_levels(&self.levels).ok_or("Interval levels are percentages between 0 and 100, e.g. 80, 95")?;
        let period = (!self.auto_period).then_some(self.period);
        Ok((columns, self.time, period, ForecastSettings { horizon: self.horizon, levels }))
    }
}

/// Chosen series columns, time column, period (`None`: from the time
/// column) and forecast settings.
type SeriesChoice = (Vec<usize>, Option<usize>, Option<usize>, ForecastSettings);

/// The custom model choice.
fn custom_spec_ui(ui: &mut egui::Ui, spec: &mut EtsSpec) {
    egui::Grid::new("ets_custom").num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
        ui.label("Errors:");
        ui.horizontal(|ui| {
            ui.radio_value(&mut spec.error, ErrorType::Additive, "Additive");
            ui.radio_value(&mut spec.error, ErrorType::Multiplicative, "Multiplicative");
        });
        ui.end_row();
        ui.label("Trend:");
        ui.horizontal(|ui| {
            ui.radio_value(&mut spec.trend, TrendType::None, "None");
            ui.radio_value(&mut spec.trend, TrendType::Additive, "Additive");
            ui.radio_value(&mut spec.trend, TrendType::DampedAdditive, "Damped");
        });
        ui.end_row();
        ui.label("Seasonality:");
        ui.horizontal(|ui| {
            ui.radio_value(&mut spec.seasonal, SeasonalType::None, "None");
            ui.radio_value(&mut spec.seasonal, SeasonalType::Additive, "Additive");
            ui.radio_value(&mut spec.seasonal, SeasonalType::Multiplicative, "Multiplicative");
        });
        ui.end_row();
    });
}

/// Exponential smoothing settings.
#[derive(Clone)]
pub struct SmoothingDialog {
    series: SeriesSettings,
    /// `None`: custom.
    model: Option<SmoothingModel>,
    custom: EtsSpec,
    allow_multiplicative: bool,
    allow_damped: bool,
    /// Fixed α, β, γ, φ: (held fixed, value).
    fixed: [(bool, f64); 4],
}

impl SmoothingDialog {
    fn new(table: &DataTable) -> Self {
        Self {
            series: SeriesSettings::new(table),
            model: Some(SmoothingModel::Automatic),
            custom: EtsSpec::new(ErrorType::Additive, TrendType::Additive, SeasonalType::None),
            allow_multiplicative: true,
            allow_damped: true,
            fixed: [(false, 0.3), (false, 0.1), (false, 0.1), (false, 0.9)],
        }
    }

    fn from_request(r: &SmoothingRequest, table: &DataTable) -> Self {
        let mut d = Self::new(table);
        d.series = SeriesSettings::from_saved(&r.series, &r.forecast, table);
        match r.model {
            SmoothingModel::Custom(spec) => {
                d.model = None;
                d.custom = spec;
            }
            m => d.model = Some(m),
        }
        d.allow_multiplicative = r.allow_multiplicative;
        d.allow_damped = r.allow_damped;
        for (slot, v) in d.fixed.iter_mut().zip([r.fixed.alpha, r.fixed.beta, r.fixed.gamma, r.fixed.phi]) {
            if let Some(v) = v {
                *slot = (true, v);
            }
        }
        d
    }

    fn ui(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<Vec<SavedReport>> {
        kit::note(
            ui,
            "Exponential smoothing forecasts from a level, an optional trend and optional seasonal effects, each \
             updated as observations arrive; the parameters are estimated by maximum likelihood.",
        );
        self.series.ui(ui, table);
        ui.separator();
        let label = |m: Option<SmoothingModel>| m.map_or("Custom combination".to_owned(), SmoothingModel::label);
        ui.horizontal(|ui| {
            ui.label("Model:");
            egui::ComboBox::from_id_salt("smoothing_model")
                .selected_text(label(self.model))
                .width(kit::em(ui) * 20.0)
                .show_list(ui, |ui| {
                    for m in SmoothingModel::NAMED {
                        ui.selectable_value(&mut self.model, Some(m), m.label());
                    }
                    ui.selectable_value(&mut self.model, None, label(None));
                });
        });
        if self.model.is_none() {
            custom_spec_ui(ui, &mut self.custom);
        }
        let model = self.model.unwrap_or(SmoothingModel::Custom(self.custom));
        match model.spec() {
            None => {
                ui.checkbox(&mut self.allow_multiplicative, "Consider multiplicative errors and seasonality")
                    .on_hover_text("Used only when every value is above zero");
                ui.checkbox(&mut self.allow_damped, "Consider damped trends");
            }
            Some(spec) => {
                kit::note(ui, &format!("{} — {}", spec.label(), spec.name()));
                let has = [
                    true,
                    spec.trend != TrendType::None,
                    spec.seasonal != SeasonalType::None,
                    spec.trend == TrendType::DampedAdditive,
                ];
                kit::more_options(ui, |ui| {
                    ui.label("Hold parameters at a value instead of estimating them:");
                    egui::Grid::new("smoothing_fixed").num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
                        let names = ["α (level)", "β (slope)", "γ (seasonal)", "φ (damping)"];
                        for (k, name) in names.iter().enumerate() {
                            if !has[k] {
                                continue;
                            }
                            let (on, value) = &mut self.fixed[k];
                            ui.checkbox(on, *name);
                            ui.add_enabled(
                                *on,
                                egui::DragValue::new(value).range(0.0001..=1.0).speed(0.01).max_decimals(4),
                            );
                            ui.end_row();
                        }
                    });
                });
            }
        }
        let (columns, time, period, forecast) = self.series.read()?;
        if model.spec().is_some_and(|s| s.seasonal != SeasonalType::None)
            && period.unwrap_or_else(|| default_period(table, time)) < 2
        {
            return Err("A seasonal model needs a seasonal period of at least 2".into());
        }
        let fixed_value = |k: usize| self.fixed[k].0.then_some(self.fixed[k].1);
        let fixed = if model == SmoothingModel::Automatic {
            FixedSmoothing::default()
        } else {
            FixedSmoothing { alpha: fixed_value(0), beta: fixed_value(1), gamma: fixed_value(2), phi: fixed_value(3) }
        };
        let reports: Vec<SavedReport> = columns
            .iter()
            .map(|&response| SavedReport::ExponentialSmoothing {
                request: SmoothingRequest {
                    series: SeriesInput { column: response, time, period },
                    model,
                    allow_multiplicative: self.allow_multiplicative,
                    allow_damped: self.allow_damped,
                    fixed,
                    forecast: forecast.clone(),
                },
            })
            .collect();
        let what = if columns.len() == 1 {
            kit::name(table, columns[0]).to_owned()
        } else {
            kit::count(columns.len(), "series", "series")
        };
        let summary = format!(
            "{} of {what}, {} ahead",
            match model {
                SmoothingModel::Automatic => "Best ETS model".to_owned(),
                m => m.spec().map_or_else(String::new, |s| s.label()),
            },
            kit::count(forecast.horizon, "step", "steps")
        );
        kit::ready(reports, summary)
    }
}

/// ARIMA settings.
#[derive(Clone)]
pub struct ArimaDialog {
    series: SeriesSettings,
    automatic: bool,
    stepwise: bool,
    allow_constant: bool,
    regular: [usize; 3],
    seasonal: [usize; 3],
    constant: bool,
    /// 0 none, 1 fixed λ, 2 Guerrero.
    box_cox: u8,
    lambda: f64,
    /// Input series (transfer functions), indexed by column.
    inputs: Vec<bool>,
    /// Each column's transfer-function settings, indexed by column.
    input_orders: Vec<InputOrders>,
}

/// The transfer-function settings of one input column.
#[derive(Clone, Copy)]
struct InputOrders {
    delay_from_data: bool,
    delay: usize,
    numerator: usize,
    denominator: usize,
}

impl Default for InputOrders {
    fn default() -> Self {
        let r = TransferRole::new(0);
        Self { delay_from_data: true, delay: 0, numerator: r.numerator, denominator: r.denominator }
    }
}

impl ArimaDialog {
    fn new(table: &DataTable) -> Self {
        Self {
            series: SeriesSettings::new(table),
            automatic: true,
            stepwise: true,
            allow_constant: true,
            regular: [1, 1, 1],
            seasonal: [0, 0, 0],
            constant: false,
            box_cox: 0,
            lambda: 0.0,
            inputs: vec![false; table.n_cols()],
            input_orders: vec![InputOrders::default(); table.n_cols()],
        }
    }

    fn from_request(r: &ArimaRequest, table: &DataTable) -> Self {
        let mut d = Self::new(table);
        d.series = SeriesSettings::from_saved(&r.series, &r.forecast, table);
        match r.orders {
            ArimaOrders::Automatic { stepwise, allow_constant } => {
                d.automatic = true;
                d.stepwise = stepwise;
                d.allow_constant = allow_constant;
            }
            ArimaOrders::Chosen { regular: (p, dd, q), seasonal: (sp, sd, sq), constant } => {
                d.automatic = false;
                d.regular = [p, dd, q];
                d.seasonal = [sp, sd, sq];
                d.constant = constant;
            }
        }
        match r.box_cox {
            BoxCoxChoice::None => d.box_cox = 0,
            BoxCoxChoice::Lambda(l) => {
                d.box_cox = 1;
                d.lambda = l;
            }
            BoxCoxChoice::Guerrero => d.box_cox = 2,
        }
        for i in &r.inputs {
            if let Some(c) = kit::valid(table, i.column) {
                d.inputs[c] = true;
                d.input_orders[c] = InputOrders {
                    delay_from_data: i.delay.is_none(),
                    delay: i.delay.unwrap_or(0),
                    numerator: i.numerator,
                    denominator: i.denominator,
                };
            }
        }
        d
    }

    /// The input series section: which columns, and each one's delay and orders.
    fn inputs_ui(&mut self, ui: &mut egui::Ui, table: &DataTable) {
        let any = self.inputs.iter().any(|&b| b);
        egui::CollapsingHeader::new("Input series (transfer functions)")
            .id_salt(ui.id().with("arima_inputs_section"))
            .default_open(any)
            .show(ui, |ui| {
                kit::note(
                    ui,
                    "Columns whose past values drive the series. Each acts after a delay through weights on its \
                     recent values (numerator) and a geometric decay (denominator). Rows after the series with \
                     the inputs filled in supply the future inputs the forecasts need.",
                );
                let (time, series) = (self.series.time, self.series.chosen.clone());
                kit::column_list(ui, "arima_inputs", table, &mut self.inputs, |i| {
                    kit::is_numeric(table, i)
                        && Some(i) != time
                        && !series.get(i).copied().unwrap_or(false)
                        && table.column(i).is_some_and(|c| c.temporal().is_none())
                });
                if self.input_orders.len() < table.n_cols() {
                    self.input_orders.resize(table.n_cols(), InputOrders::default());
                }
                let chosen: Vec<usize> = (0..table.n_cols()).filter(|&c| self.inputs.get(c) == Some(&true)).collect();
                if chosen.is_empty() {
                    return;
                }
                egui::Grid::new("arima_input_orders").num_columns(4).spacing([12.0, 4.0]).show(ui, |ui| {
                    ui.label("Input");
                    ui.label("Delay (observations)");
                    ui.label("Numerator order");
                    ui.label("Denominator order");
                    ui.end_row();
                    for c in chosen {
                        let o = &mut self.input_orders[c];
                        ui.label(kit::name(table, c));
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut o.delay_from_data, "From the data");
                            ui.add_enabled(!o.delay_from_data, egui::DragValue::new(&mut o.delay).range(0..=100))
                                .on_hover_text(format!("Delay of {}", kit::name(table, c)));
                        });
                        ui.add(egui::DragValue::new(&mut o.numerator).range(0..=6))
                            .on_hover_text(format!("Numerator order of {}", kit::name(table, c)));
                        ui.add(egui::DragValue::new(&mut o.denominator).range(0..=3))
                            .on_hover_text(format!("Denominator order of {}", kit::name(table, c)));
                        ui.end_row();
                    }
                });
                kit::note(
                    ui,
                    "A delay from the data is the first lag where the input, prewhitened, correlates \
                     significantly with the series.",
                );
            });
    }

    fn ui(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<Vec<SavedReport>> {
        kit::note(
            ui,
            "A seasonal ARIMA model: differencing for trend and seasonality, then autoregressive and moving-average \
             terms, fitted by exact maximum likelihood.",
        );
        self.series.ui(ui, table);
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Orders:");
            ui.radio_value(&mut self.automatic, true, "Automatic search");
            ui.radio_value(&mut self.automatic, false, "Choose");
        });
        if self.automatic {
            ui.checkbox(&mut self.stepwise, "Stepwise search (otherwise every model up to order 5)");
            ui.checkbox(&mut self.allow_constant, "Consider a mean or drift term");
            kit::note(ui, "Differencing is chosen by KPSS tests and the strength of seasonality; orders by AICc.");
        } else {
            egui::Grid::new("arima_orders").num_columns(4).spacing([12.0, 4.0]).show(ui, |ui| {
                ui.label("");
                ui.label("Autoregressive");
                ui.label("Differences");
                ui.label("Moving average");
                ui.end_row();
                ui.label("Regular (p, d, q):");
                for (k, name) in ["p", "d", "q"].iter().enumerate() {
                    let max = if k == 1 { 2 } else { 10 };
                    ui.add(egui::DragValue::new(&mut self.regular[k]).range(0..=max)).on_hover_text(*name);
                }
                ui.end_row();
                ui.label("Seasonal (P, D, Q):");
                for (k, name) in ["P", "D", "Q"].iter().enumerate() {
                    let max = if k == 1 { 1 } else { 4 };
                    ui.add(egui::DragValue::new(&mut self.seasonal[k]).range(0..=max)).on_hover_text(*name);
                }
                ui.end_row();
            });
            let total_d = self.regular[1] + self.seasonal[1];
            let what = match total_d {
                0 => "Include a mean",
                1 => "Include a drift (trend per observation)",
                _ => "Include a constant (not allowed with d + D ≥ 2)",
            };
            ui.add_enabled(total_d <= 1, egui::Checkbox::new(&mut self.constant, what));
            if total_d > 1 {
                self.constant = false;
            }
        }
        ui.separator();
        self.inputs_ui(ui, table);
        kit::more_options(ui, |ui| {
            ui.label("Box–Cox transformation before fitting (positive data only):");
            ui.horizontal(|ui| {
                ui.radio_value(&mut self.box_cox, 0, "None");
                ui.radio_value(&mut self.box_cox, 2, "λ by Guerrero's method");
                ui.radio_value(&mut self.box_cox, 1, "λ =");
                ui.add_enabled(self.box_cox == 1, egui::DragValue::new(&mut self.lambda).range(-2.0..=3.0).speed(0.05));
            });
            kit::note(ui, "Forecasts are transformed back; the point forecast is then the median.");
        });
        let (columns, time, period, forecast) = self.series.read()?;
        let orders = if self.automatic {
            ArimaOrders::Automatic { stepwise: self.stepwise, allow_constant: self.allow_constant }
        } else {
            let [p, d, q] = self.regular;
            let [sp, sd, sq] = self.seasonal;
            if sp + sd + sq > 0 && period.unwrap_or_else(|| default_period(table, time)) < 2 {
                return Err("A seasonal part needs a seasonal period of at least 2".into());
            }
            ArimaOrders::Chosen { regular: (p, d, q), seasonal: (sp, sd, sq), constant: self.constant }
        };
        let box_cox = match self.box_cox {
            1 => BoxCoxChoice::Lambda(self.lambda),
            2 => BoxCoxChoice::Guerrero,
            _ => BoxCoxChoice::None,
        };
        let inputs: Vec<TransferRole> = (0..table.n_cols())
            .filter(|&c| self.inputs.get(c) == Some(&true) && kit::is_numeric(table, c))
            .map(|c| {
                let o = self.input_orders.get(c).copied().unwrap_or_default();
                TransferRole {
                    column: c,
                    delay: (!o.delay_from_data).then_some(o.delay),
                    numerator: o.numerator,
                    denominator: o.denominator,
                }
            })
            .collect();
        if let Some(i) = inputs.iter().find(|i| columns.contains(&i.column) || Some(i.column) == time) {
            return Err(format!("{} cannot be both forecast and an input", kit::name(table, i.column)));
        }
        let reports: Vec<SavedReport> = columns
            .iter()
            .map(|&response| SavedReport::Arima {
                request: ArimaRequest {
                    series: SeriesInput { column: response, time, period },
                    orders,
                    box_cox,
                    forecast: forecast.clone(),
                    inputs: inputs.clone(),
                },
            })
            .collect();
        let what = if columns.len() == 1 {
            kit::name(table, columns[0]).to_owned()
        } else {
            kit::count(columns.len(), "series", "series")
        };
        let model = match orders {
            ArimaOrders::Automatic { .. } => "Best ARIMA model".to_owned(),
            ArimaOrders::Chosen { regular: (p, d, q), seasonal: (sp, sd, sq), .. } if sp + sd + sq > 0 => {
                format!("ARIMA({p},{d},{q})({sp},{sd},{sq})")
            }
            ArimaOrders::Chosen { regular: (p, d, q), .. } => format!("ARIMA({p},{d},{q})"),
        };
        let with = match inputs.len() {
            0 => String::new(),
            1 => format!(" with input {}", kit::name(table, inputs[0].column)),
            n => format!(" with {n} inputs"),
        };
        kit::ready(reports, format!("{model} of {what}{with}, {} ahead", kit::count(forecast.horizon, "step", "steps")))
    }
}

/// Which forecasting dialog.
#[derive(Clone)]
enum ForecastKind {
    Smoothing(Box<SmoothingDialog>),
    Arima(Box<ArimaDialog>),
}

/// A Series and Signals ▸ Forecasting models dialog.
#[derive(Clone)]
pub struct ForecastDialog {
    edit: Option<u64>,
    kind: ForecastKind,
}

impl ForecastDialog {
    /// Exponential smoothing.
    pub fn smoothing(table: &DataTable) -> Self {
        Self { edit: None, kind: ForecastKind::Smoothing(Box::new(SmoothingDialog::new(table))) }
    }

    /// ARIMA.
    pub fn arima(table: &DataTable) -> Self {
        Self { edit: None, kind: ForecastKind::Arima(Box::new(ArimaDialog::new(table))) }
    }

    /// The dialog filled from a saved forecasting report.
    pub fn from_saved(saved: &SavedReport, table: &DataTable) -> Option<Self> {
        let kind = match saved {
            SavedReport::ExponentialSmoothing { request } => {
                ForecastKind::Smoothing(Box::new(SmoothingDialog::from_request(request, table)))
            }
            SavedReport::Arima { request } => ForecastKind::Arima(Box::new(ArimaDialog::from_request(request, table))),
            _ => return None,
        };
        Some(Self { edit: None, kind })
    }

    fn title(&self) -> &'static str {
        match self.kind {
            ForecastKind::Smoothing(_) => "Exponential smoothing",
            ForecastKind::Arima(_) => "ARIMA",
        }
    }

    /// One report per series; editing a report replaces it with the first.
    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<Vec<SavedReport>> {
        let dialog = Dialog::new(self.title()).editing(self.edit);
        let outcome = dialog.show(ctx, |ui| match &mut self.kind {
            ForecastKind::Smoothing(d) => d.ui(ui, table),
            ForecastKind::Arima(d) => d.ui(ui, table),
        });
        match outcome {
            Outcome::Reset => {
                self.kind = match self.kind {
                    ForecastKind::Smoothing(_) => ForecastKind::Smoothing(Box::new(SmoothingDialog::new(table))),
                    ForecastKind::Arima(_) => ForecastKind::Arima(Box::new(ArimaDialog::new(table))),
                };
                Outcome::Open
            }
            outcome => outcome,
        }
    }
}

impl Remember for ForecastDialog {
    fn memory_key(&self) -> String {
        self.title().into()
    }

    fn set_edit(&mut self, report: Option<u64>) {
        self.edit = report;
    }

    fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        match &mut self.kind {
            ForecastKind::Smoothing(d) => d.series.prefill(columns, table),
            ForecastKind::Arima(d) => d.series.prefill(columns, table),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_parse_as_percentages() {
        assert_eq!(parse_levels("80, 95%"), Some(vec![0.8, 0.95]));
        assert_eq!(parse_levels("0"), None);
        assert_eq!(parse_levels(""), None);
        assert_eq!(levels_text(&[0.8, 0.95]), "80, 95");
    }
}
