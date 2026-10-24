//! Dialog for Reliability ▸ Warranty returns…: shipment and return counts in
//! a triangular (one row per shipment period) or long layout, the
//! distributions to compare and the forecast settings.

use crate::dialog_kit::ComboExt as _;
use app_model::{SavedReport, WarrantyLayout, WarrantyRequest};
use eframe::egui;
use stat_core::distribution_fit::FitFamily;
use stat_data::DataTable;

use crate::dialog_kit::{self as kit, Readiness};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Layout {
    Triangular,
    Long,
}

/// Families offered: positive, without a threshold.
fn offered() -> Vec<FitFamily> {
    FitFamily::ALL.iter().copied().filter(|f| !f.has_threshold() && f.needs_positive()).collect()
}

/// Columns and settings of a warranty-returns analysis.
#[derive(Clone)]
pub struct WarrantyDialog {
    layout: Layout,
    shipped: Option<usize>,
    returns: Vec<bool>,
    label: Option<usize>,
    ship_period: Option<usize>,
    return_period: Option<usize>,
    count: Option<usize>,
    families: Vec<bool>,
    detail: Option<FitFamily>,
    horizon: usize,
    planned: String,
    has_warranty: bool,
    warranty: f64,
    confidence: f64,
}

impl Default for WarrantyDialog {
    fn default() -> Self {
        Self {
            layout: Layout::Triangular,
            shipped: None,
            returns: Vec::new(),
            label: None,
            ship_period: None,
            return_period: None,
            count: None,
            families: offered().iter().map(|f| WarrantyRequest::DEFAULT_FAMILIES.contains(f)).collect(),
            detail: Some(FitFamily::Weibull),
            horizon: WarrantyRequest::DEFAULT_HORIZON,
            planned: String::new(),
            has_warranty: false,
            warranty: 12.0,
            confidence: WarrantyRequest::DEFAULT_CONFIDENCE,
        }
    }
}

fn parse_numbers(text: &str) -> Option<Vec<f64>> {
    text.split([',', ';', ' '])
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.trim().parse::<f64>().ok().filter(|v| v.is_finite() && *v >= 0.0))
        .collect()
}

impl WarrantyDialog {
    /// The dialog filled from a saved request.
    pub fn from_request(r: &WarrantyRequest, table: &DataTable) -> Self {
        let valid = |c: usize| kit::valid(table, c);
        let mut d = Self {
            families: offered().iter().map(|f| r.families.contains(f)).collect(),
            detail: r.detail,
            horizon: r.horizon,
            planned: r.planned.iter().map(|v| app_model::format::format_number(*v, 10)).collect::<Vec<_>>().join(", "),
            has_warranty: r.warranty.is_some(),
            warranty: r.warranty.unwrap_or(12.0),
            confidence: r.confidence,
            ..Self::default()
        };
        match &r.layout {
            WarrantyLayout::Triangular { shipped, returns, label } => {
                d.layout = Layout::Triangular;
                d.shipped = valid(*shipped);
                d.returns = kit::mask(table.n_cols(), returns);
                d.label = label.and_then(valid);
            }
            WarrantyLayout::Long { ship_period, return_period, returns, shipped } => {
                d.layout = Layout::Long;
                d.ship_period = valid(*ship_period);
                d.return_period = valid(*return_period);
                d.count = valid(*returns);
                d.shipped = valid(*shipped);
            }
        }
        d
    }

    /// Selected numeric columns: the first is the units shipped, the rest
    /// the return columns.
    pub fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        let numeric: Vec<usize> = columns.iter().copied().filter(|&c| kit::is_numeric(table, c)).collect();
        if let Some((&first, rest)) = numeric.split_first() {
            self.shipped = Some(first);
            self.returns = kit::mask(table.n_cols(), rest);
        }
    }

    /// Draw the settings; ready when they are complete and valid.
    pub fn ui(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<SavedReport> {
        kit::note(
            ui,
            "Units shipped per period and units returned by period of shipment and period of return: a life \
             distribution fitted to the returns (ages in periods) forecasts the returns still to come.",
        );
        let numeric = |i: usize| kit::is_numeric(table, i);
        ui.horizontal(|ui| {
            ui.label("Data:");
            ui.radio_value(&mut self.layout, Layout::Triangular, "One row per shipment period");
            ui.radio_value(&mut self.layout, Layout::Long, "One row per count");
        });
        egui::Grid::new("warranty_columns").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| match self.layout {
            Layout::Triangular => {
                ui.label("Units shipped:");
                kit::column_choice(ui, "warranty_shipped", table, &mut self.shipped, numeric, None);
                ui.end_row();
                ui.label("Period label (optional):");
                kit::column_choice(ui, "warranty_label", table, &mut self.label, |_| true, Some("None"));
                ui.end_row();
            }
            Layout::Long => {
                ui.label("Shipment period:");
                kit::column_choice(ui, "warranty_ship_period", table, &mut self.ship_period, numeric, None);
                ui.end_row();
                ui.label("Return period:");
                kit::column_choice(ui, "warranty_return_period", table, &mut self.return_period, numeric, None);
                ui.end_row();
                ui.label("Units returned:");
                kit::column_choice(ui, "warranty_count", table, &mut self.count, numeric, None);
                ui.end_row();
                ui.label("Units shipped:");
                kit::column_choice(ui, "warranty_shipped_long", table, &mut self.shipped, numeric, None);
                ui.end_row();
                ui.label("");
                kit::note(
                    ui,
                    "Periods are whole numbers; the units shipped in a period repeat on its rows, and a row with \
                         an empty return period gives only the shipment.",
                );
                ui.end_row();
            }
        });
        if self.layout == Layout::Triangular {
            ui.label("Returns in each calendar period (in order; the first is the first row's period):");
            let exclude = [self.shipped, self.label];
            kit::column_list(ui, "warranty_returns", table, &mut self.returns, |i| {
                numeric(i) && !exclude.contains(&Some(i))
            });
        }
        let families = offered();
        ui.label("Distributions to compare:");
        egui::Grid::new("warranty_families").num_columns(3).spacing([12.0, 2.0]).show(ui, |ui| {
            for (i, family) in families.iter().enumerate() {
                ui.checkbox(&mut self.families[i], family.label());
                if i % 3 == 2 {
                    ui.end_row();
                }
            }
        });
        let chosen: Vec<FitFamily> =
            families.iter().zip(&self.families).filter(|(_, on)| **on).map(|(f, _)| *f).collect();
        if self.detail.is_some_and(|d| !chosen.contains(&d)) {
            self.detail = None;
        }
        egui::Grid::new("warranty_forecast").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label("Forecast with:");
            let label = |d: Option<FitFamily>| d.map_or("The best by AIC".to_owned(), |f| f.label().to_owned());
            egui::ComboBox::from_id_salt("warranty_detail")
                .selected_text(label(self.detail))
                .width(kit::combo_width(ui))
                .show_list(ui, |ui| {
                    ui.selectable_value(&mut self.detail, None, label(None));
                    for &f in &chosen {
                        ui.selectable_value(&mut self.detail, Some(f), f.label());
                    }
                });
            ui.end_row();
            ui.label("Periods to forecast:");
            ui.add(egui::DragValue::new(&mut self.horizon).range(1..=240));
            ui.end_row();
            ui.label("Planned shipments:");
            ui.add(
                egui::TextEdit::singleline(&mut self.planned)
                    .desired_width(kit::combo_width(ui))
                    .hint_text("Units in future periods 1, 2, …"),
            );
            ui.end_row();
        });
        kit::more_options(ui, |ui| {
            egui::Grid::new("warranty_settings").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                ui.label("Warranty length:");
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.has_warranty, "Returns accepted up to");
                    ui.add_enabled(
                        self.has_warranty,
                        egui::DragValue::new(&mut self.warranty).range(1.0..=1200.0).speed(1.0),
                    );
                    ui.label("periods of service");
                });
                ui.end_row();
                ui.label("Confidence level:");
                ui.add(egui::DragValue::new(&mut self.confidence).range(0.5..=0.999).speed(0.005).max_decimals(3));
                ui.end_row();
            });
        });
        let shipped = self.shipped.ok_or("Choose the units-shipped column")?;
        let layout = match self.layout {
            Layout::Triangular => {
                let returns = kit::chosen_columns(&self.returns);
                if returns.is_empty() {
                    return Err("Choose the return columns, one per calendar period".into());
                }
                WarrantyLayout::Triangular { shipped, returns, label: self.label }
            }
            Layout::Long => WarrantyLayout::Long {
                ship_period: self.ship_period.ok_or("Choose the shipment-period column")?,
                return_period: self.return_period.ok_or("Choose the return-period column")?,
                returns: self.count.ok_or("Choose the units-returned column")?,
                shipped,
            },
        };
        if chosen.is_empty() {
            return Err("Choose at least one distribution".into());
        }
        let Some(planned) = parse_numbers(&self.planned) else {
            return Err("Planned shipments must be non-negative numbers".into());
        };
        let request = WarrantyRequest {
            families: chosen,
            detail: self.detail,
            horizon: self.horizon,
            planned,
            warranty: self.has_warranty.then_some(self.warranty),
            confidence: self.confidence,
            ..WarrantyRequest::new(layout)
        };
        let cols = request.columns();
        if (1..cols.len()).any(|i| cols[..i].contains(&cols[i])) {
            return Err("Choose a different column for each role".into());
        }
        let summary = format!(
            "Returns of {} forecast {} ahead",
            kit::name(table, shipped),
            kit::count(self.horizon, "period", "periods")
        );
        kit::ready(SavedReport::Warranty { request }, summary)
    }
}
