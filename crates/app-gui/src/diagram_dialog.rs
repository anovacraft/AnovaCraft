//! Dialogs for the Pareto chart, the cause-and-effect diagram and the
//! multi-vari chart (Quality menu).

use app_model::{CauseEffectRequest, MultiVariRequest, ParetoChartOptions, ParetoRequest, SavedReport};
use eframe::egui;
use stat_data::DataTable;

use crate::dialog_kit::{self as kit, Dialog, Outcome, Readiness, Remember};

/// Pareto chart: category, optional frequency and grouping columns, and
/// the "Other" threshold.
#[derive(Clone, Default)]
pub struct ParetoDialog {
    edit: Option<u64>,
    category: Option<usize>,
    frequency: Option<usize>,
    group: Option<usize>,
    options: ParetoChartOptions,
}

impl ParetoDialog {
    /// The dialog filled from a saved Pareto chart, with its chart options.
    pub fn from_saved(saved: &SavedReport, table: &DataTable) -> Option<Self> {
        let SavedReport::Pareto { request: r, options } = saved else { return None };
        Some(Self {
            edit: None,
            category: kit::valid(table, r.category),
            frequency: r.frequency.and_then(|f| kit::valid(table, f)),
            group: r.group.and_then(|g| kit::valid(table, g)),
            options: *options,
        })
    }

    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<(ParetoRequest, ParetoChartOptions)> {
        let dialog = Dialog::new("Pareto chart").editing(self.edit);
        match dialog.show(ctx, |ui| self.contents(ui, table)) {
            Outcome::Reset => {
                *self = Self { edit: self.edit, ..Self::default() };
                Outcome::Open
            }
            outcome => outcome,
        }
    }

    fn contents(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<(ParetoRequest, ParetoChartOptions)> {
        egui::Grid::new("pareto_roles").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
            ui.label("Category:").on_hover_text("The category of each row, e.g. its defect type");
            kit::column_choice(ui, "pareto_category", table, &mut self.category, |_| true, None);
            ui.end_row();
            ui.label("Frequency:").on_hover_text("How many times each row counts; without one, each row counts once");
            kit::column_choice(
                ui,
                "pareto_frequency",
                table,
                &mut self.frequency,
                |i| kit::is_numeric(table, i),
                Some("None (each row counts once)"),
            );
            ui.end_row();
            ui.label("A chart for each level of:");
            kit::column_choice(ui, "pareto_group", table, &mut self.group, |_| true, Some("None (one chart)"));
            ui.end_row();
        });
        kit::more_options(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Pool categories below")
                    .on_hover_text("Categories with a smaller share of the total share one \"Other\" bar, placed last");
                ui.add(egui::DragValue::new(&mut self.options.other_percent).range(0.0..=50.0).speed(0.5).suffix(" %"));
                ui.label("into \"Other\" (0 = off)");
            });
            ui.checkbox(&mut self.options.percent_scale, "Bar heights as percentages of the total");
        });
        let Some(category) = self.category else { return Err("Choose the category column".into()) };
        if self.frequency == Some(category) {
            return Err("Category and frequency must be different columns".into());
        }
        if self.group == Some(category) {
            return Err("Category and grouping column must differ".into());
        }
        let mut summary = format!("Pareto chart of {}", kit::name(table, category));
        if let Some(f) = self.frequency {
            summary += &format!(" weighted by {}", kit::name(table, f));
        }
        if let Some(g) = self.group {
            summary += &format!(", one for each {}", kit::name(table, g));
        }
        kit::ready((ParetoRequest { category, frequency: self.frequency, group: self.group }, self.options), summary)
    }
}

impl Remember for ParetoDialog {
    fn memory_key(&self) -> String {
        "Pareto chart".into()
    }

    fn set_edit(&mut self, report: Option<u64>) {
        self.edit = report;
    }

    fn prefill(&mut self, columns: &[usize], _table: &DataTable) {
        self.category = columns.first().copied();
    }
}

/// Cause-and-effect diagram: one to three cause columns and the effect.
#[derive(Clone, Default)]
pub struct CauseEffectDialog {
    edit: Option<u64>,
    branch: Option<usize>,
    cause: Option<usize>,
    sub_cause: Option<usize>,
    effect: String,
}

impl CauseEffectDialog {
    /// The dialog filled from a saved cause-and-effect diagram.
    pub fn from_saved(saved: &SavedReport, table: &DataTable) -> Option<Self> {
        let SavedReport::CauseEffect { request: r } = saved else { return None };
        let at = |k: usize| r.causes.get(k).and_then(|&c| kit::valid(table, c));
        Some(Self { edit: None, branch: at(0), cause: at(1), sub_cause: at(2), effect: r.effect.clone() })
    }

    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<CauseEffectRequest> {
        let dialog = Dialog::new("Cause-and-effect diagram").editing(self.edit);
        match dialog.show(ctx, |ui| self.contents(ui, table)) {
            Outcome::Reset => {
                *self = Self { edit: self.edit, ..Self::default() };
                Outcome::Open
            }
            outcome => outcome,
        }
    }

    fn contents(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<CauseEffectRequest> {
        egui::Grid::new("fishbone_roles").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
            ui.label("Effect:").on_hover_text("The problem at the head of the diagram");
            ui.add(
                egui::TextEdit::singleline(&mut self.effect)
                    .hint_text("e.g. Late deliveries")
                    .desired_width(kit::combo_width(ui)),
            );
            ui.end_row();
            ui.label("Main branches:").on_hover_text("The categories of causes");
            kit::column_choice(ui, "fishbone_branch", table, &mut self.branch, |_| true, None);
            ui.end_row();
            ui.label("Causes on each branch:");
            kit::column_choice(ui, "fishbone_cause", table, &mut self.cause, |_| true, Some("None"));
            ui.end_row();
            ui.label("Sub-causes of each cause:");
            ui.add_enabled_ui(self.cause.is_some(), |ui| {
                kit::column_choice(ui, "fishbone_sub", table, &mut self.sub_cause, |_| true, Some("None"));
            });
            ui.end_row();
        });
        kit::note(ui, "Each row is one path: branch, cause, sub-cause. Repeated paths are merged.");
        let causes: Vec<usize> =
            [self.branch, self.cause, self.cause.and(self.sub_cause)].into_iter().map_while(|c| c).collect();
        if causes.is_empty() {
            return Err("Choose the column of main branches".into());
        }
        let roles: Vec<Option<usize>> = causes.iter().map(|&c| Some(c)).collect();
        if !kit::all_different(&roles) {
            return Err("Branches, causes and sub-causes must be different columns".into());
        }
        let path: Vec<&str> = causes.iter().map(|&c| kit::name(table, c)).collect();
        let summary = format!("Cause-and-effect diagram: {}", path.join(" › "));
        kit::ready(CauseEffectRequest { causes, effect: self.effect.trim().to_owned() }, summary)
    }
}

impl Remember for CauseEffectDialog {
    fn memory_key(&self) -> String {
        "Cause-and-effect diagram".into()
    }

    fn set_edit(&mut self, report: Option<u64>) {
        self.edit = report;
    }

    fn prefill(&mut self, columns: &[usize], _table: &DataTable) {
        self.branch = columns.first().copied();
        self.cause = columns.get(1).copied();
        self.sub_cause = columns.get(2).copied();
    }
}

/// Multi-vari chart: a response and one to four factors, outermost first.
#[derive(Clone, Default)]
pub struct MultiVariDialog {
    edit: Option<u64>,
    response: Option<usize>,
    factors: [Option<usize>; 4],
}

impl MultiVariDialog {
    /// The dialog filled from a saved multi-vari chart.
    pub fn from_saved(saved: &SavedReport, table: &DataTable) -> Option<Self> {
        let SavedReport::MultiVari { request: r, .. } = saved else { return None };
        let mut factors = [None; 4];
        for (slot, &f) in factors.iter_mut().zip(&r.factors) {
            *slot = kit::valid(table, f);
        }
        Some(Self { edit: None, response: kit::valid(table, r.response), factors })
    }

    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<MultiVariRequest> {
        let dialog = Dialog::new("Multi-vari chart").editing(self.edit);
        match dialog.show(ctx, |ui| self.contents(ui, table)) {
            Outcome::Reset => {
                *self = Self { edit: self.edit, ..Self::default() };
                Outcome::Open
            }
            outcome => outcome,
        }
    }

    fn contents(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<MultiVariRequest> {
        egui::Grid::new("multivari_roles").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
            ui.label("Response:");
            kit::column_choice(
                ui,
                "multivari_response",
                table,
                &mut self.response,
                |i| kit::is_numeric(table, i),
                None,
            );
            ui.end_row();
            let labels = ["Factor 1 (panels):", "Factor 2:", "Factor 3:", "Factor 4 (innermost):"];
            for (k, label) in labels.iter().enumerate() {
                let enabled = k == 0 || self.factors[k - 1].is_some();
                ui.label(*label);
                ui.add_enabled_ui(enabled, |ui| {
                    let none = (k > 0).then_some("None");
                    kit::column_choice(ui, ("multivari_factor", k), table, &mut self.factors[k], |_| true, none);
                });
                ui.end_row();
            }
        });
        kit::note(
            ui,
            "Factors go outermost first, nested or crossed. Means of each factor are joined within each level of \
             the factor above it; the ranges view charts the spread within the innermost cells.",
        );
        let factors: Vec<usize> = self.factors.iter().map_while(|f| *f).collect();
        let Some(response) = self.response else { return Err("Choose a response".into()) };
        if factors.is_empty() {
            return Err("Choose the first factor".into());
        }
        let roles: Vec<Option<usize>> = std::iter::once(response).chain(factors.iter().copied()).map(Some).collect();
        if !kit::all_different(&roles) {
            return Err("A column is chosen twice".into());
        }
        let names: Vec<&str> = factors.iter().map(|&f| kit::name(table, f)).collect();
        let summary = format!("Multi-vari chart of {} by {}", kit::name(table, response), names.join(", "));
        kit::ready(MultiVariRequest { response, factors }, summary)
    }
}

impl Remember for MultiVariDialog {
    fn memory_key(&self) -> String {
        "Multi-vari chart".into()
    }

    fn set_edit(&mut self, report: Option<u64>) {
        self.edit = report;
    }

    fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        let Some(&r) = columns.iter().find(|&&c| kit::is_numeric(table, c)) else { return };
        self.response = Some(r);
        for (slot, f) in self.factors.iter_mut().zip(columns.iter().copied().filter(|&c| c != r)) {
            *slot = Some(f);
        }
    }
}
