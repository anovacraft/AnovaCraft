//! Dialog for an analysis of means (means, proportions or rates; one- or
//! two-way for means).

use app_model::{AnomKind, AnomRequest, SavedReport, SubgroupSize};
use eframe::egui;
use stat_data::DataTable;

use crate::dialog_kit::{self as kit, Dialog, Outcome, Readiness, Remember};

#[derive(Clone)]
pub struct AnomDialog {
    edit: Option<u64>,
    kind: AnomKind,
    response: Option<usize>,
    /// Trials or exposure from a column (`true`) or a fixed number.
    size_from_column: bool,
    size_column: Option<usize>,
    size_constant: f64,
    group: Option<usize>,
    second: Option<usize>,
    alpha: f64,
}

impl Default for AnomDialog {
    fn default() -> Self {
        Self {
            edit: None,
            kind: AnomKind::Means,
            response: None,
            size_from_column: true,
            size_column: None,
            size_constant: 50.0,
            group: None,
            second: None,
            alpha: AnomRequest::DEFAULT_ALPHA,
        }
    }
}

impl AnomDialog {
    /// The dialog filled from a saved analysis of means.
    pub fn from_saved(saved: &SavedReport, table: &DataTable) -> Option<Self> {
        let SavedReport::Anom { request: r } = saved else { return None };
        let mut d = Self { kind: r.kind, response: kit::valid(table, r.response), alpha: r.alpha, ..Self::default() };
        match r.size {
            SubgroupSize::None => {}
            SubgroupSize::Column(c) => d.size_column = kit::valid(table, c),
            SubgroupSize::Constant(v) => {
                d.size_from_column = false;
                d.size_constant = v;
            }
        }
        d.group = r.group.and_then(|g| kit::valid(table, g));
        d.second = r.second.and_then(|s| kit::valid(table, s));
        Some(d)
    }

    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<AnomRequest> {
        let dialog = Dialog::new("Analysis of means").editing(self.edit);
        match dialog.show(ctx, |ui| self.contents(ui, table)) {
            Outcome::Reset => {
                *self = Self { edit: self.edit, ..Self::default() };
                Outcome::Open
            }
            outcome => outcome,
        }
    }

    fn contents(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<AnomRequest> {
        let numeric = |i: usize| kit::is_numeric(table, i);
        ui.horizontal(|ui| {
            ui.label("Data:");
            for kind in AnomKind::ALL {
                ui.radio_value(&mut self.kind, kind, kind.label());
            }
        });
        let needs_size = self.kind != AnomKind::Means;
        egui::Grid::new("anom_roles").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
            ui.label(match self.kind {
                AnomKind::Means => "Response:",
                AnomKind::Proportions => "Events per row:",
                AnomKind::Rates => "Counts per row:",
            })
            .on_hover_text(match self.kind {
                AnomKind::Means => "The measurements whose group means are compared",
                AnomKind::Proportions => "Units with the attribute in each row",
                AnomKind::Rates => "Events counted in each row",
            });
            kit::column_choice(ui, "anom_response", table, &mut self.response, numeric, None);
            ui.end_row();
            if needs_size {
                ui.label(if self.kind == AnomKind::Proportions { "Trials per row:" } else { "Exposure per row:" })
                    .on_hover_text(if self.kind == AnomKind::Proportions {
                        "Units inspected in each row"
                    } else {
                        "Area, time or units over which each row's events were counted"
                    });
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut self.size_from_column, true, "From a column");
                        ui.add_enabled_ui(self.size_from_column, |ui| {
                            kit::column_choice(ui, "anom_size", table, &mut self.size_column, numeric, None);
                        });
                    });
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut self.size_from_column, false, "The same for every row:");
                        ui.add_enabled(
                            !self.size_from_column,
                            egui::DragValue::new(&mut self.size_constant).range(1e-9..=1e9).speed(1.0),
                        );
                    });
                });
                ui.end_row();
            }
            ui.label("Groups:");
            let none = needs_size.then_some("Each row its own group");
            kit::column_choice(ui, "anom_group", table, &mut self.group, |_| true, none);
            ui.end_row();
            if !needs_size {
                ui.label("Second factor:").on_hover_text("A two-way analysis of means needs a balanced layout");
                kit::column_choice(ui, "anom_second", table, &mut self.second, |_| true, Some("None (one-way)"));
                ui.end_row();
            }
        });
        kit::more_options(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Significance level α:");
                ui.add(egui::DragValue::new(&mut self.alpha).range(1e-4..=0.5).speed(0.001).max_decimals(4));
            });
        });
        let Some(response) = self.response else {
            return Err(match self.kind {
                AnomKind::Means => "Choose a response",
                AnomKind::Proportions => "Choose the column of events",
                AnomKind::Rates => "Choose the column of counts",
            }
            .into());
        };
        let size = if !needs_size {
            SubgroupSize::None
        } else if self.size_from_column {
            match self.size_column {
                Some(c) => SubgroupSize::Column(c),
                None => {
                    return Err(if self.kind == AnomKind::Proportions {
                        "Choose the column of trials"
                    } else {
                        "Choose the column of exposures"
                    }
                    .into());
                }
            }
        } else {
            SubgroupSize::Constant(self.size_constant)
        };
        if !needs_size && self.group.is_none() {
            return Err("Choose the group column".into());
        }
        let second = if needs_size { None } else { self.second };
        let size_column = if let SubgroupSize::Column(c) = size { Some(c) } else { None };
        if !kit::all_different(&[Some(response), size_column, self.group, second]) {
            return Err("A column is chosen twice".into());
        }
        let what = match self.kind {
            AnomKind::Means => "means",
            AnomKind::Proportions => "proportions",
            AnomKind::Rates => "rates",
        };
        let groups = match (self.group, second) {
            (Some(g), Some(s)) => format!("by {} and {}", kit::name(table, g), kit::name(table, s)),
            (Some(g), None) => format!("by {}", kit::name(table, g)),
            (None, _) => "by row".to_owned(),
        };
        let summary = format!("Analysis of {what} of {} {groups}", kit::name(table, response));
        kit::ready(
            AnomRequest { kind: self.kind, response, size, group: self.group, second, alpha: self.alpha },
            summary,
        )
    }
}

impl Remember for AnomDialog {
    fn memory_key(&self) -> String {
        "Analysis of means".into()
    }

    fn set_edit(&mut self, report: Option<u64>) {
        self.edit = report;
    }

    fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        if let Some(&r) = columns.iter().find(|&&c| kit::is_numeric(table, c)) {
            self.response = Some(r);
            self.group = columns.iter().copied().find(|&c| c != r);
        }
    }
}
