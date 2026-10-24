//! Dialog for a count chart: p, np, c, u, Laney p′ and u′, and the
//! rare-event g and t charts, with an optional phase column and run rules.

use app_model::{AttributeChartKind, AttributeChartRequest, RuleSet, SavedReport, SubgroupSize};
use eframe::egui;
use stat_data::DataTable;

use crate::dialog_kit::{self as kit, Dialog, Outcome, Readiness, Remember};

const TITLE: &str = "Count and rare-event chart";

#[derive(Clone)]
pub struct AttributeDialog {
    edit: Option<u64>,
    kind: AttributeChartKind,
    count: Option<usize>,
    /// Sizes from a column (`true`) or a fixed number.
    size_from_column: bool,
    size_column: Option<usize>,
    size_constant: f64,
    /// Optional phase column (any type).
    phase: Option<usize>,
    /// Apply every run rule (otherwise only points outside the limits).
    run_rules: bool,
}

impl Default for AttributeDialog {
    fn default() -> Self {
        Self {
            edit: None,
            kind: AttributeChartKind::P,
            count: None,
            size_from_column: true,
            size_column: None,
            size_constant: 50.0,
            phase: None,
            run_rules: false,
        }
    }
}

impl AttributeDialog {
    /// The dialog filled from a saved count chart.
    pub fn from_saved(saved: &SavedReport, table: &DataTable) -> Option<Self> {
        let SavedReport::Attribute { request } = saved else { return None };
        let mut d = Self {
            kind: request.kind,
            count: kit::valid(table, request.count),
            phase: request.phase.and_then(|c| kit::valid(table, c)),
            run_rules: request.rules != RuleSet::LIMITS_ONLY,
            ..Self::default()
        };
        match request.size {
            SubgroupSize::None => {}
            SubgroupSize::Column(c) => d.size_column = kit::valid(table, c),
            SubgroupSize::Constant(n) => (d.size_from_column, d.size_constant) = (false, n),
        }
        Some(d)
    }

    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<AttributeChartRequest> {
        let dialog = Dialog::new(TITLE).editing(self.edit);
        match dialog.show(ctx, |ui| self.contents(ui, table)) {
            Outcome::Reset => {
                *self = Self { edit: self.edit, ..Self::default() };
                Outcome::Open
            }
            outcome => outcome,
        }
    }

    fn contents(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<AttributeChartRequest> {
        let numeric = |i: usize| kit::is_numeric(table, i);
        ui.label("Chart type:");
        egui::Grid::new("attribute_kinds").num_columns(2).spacing([12.0, 2.0]).show(ui, |ui| {
            for (i, kind) in AttributeChartKind::ALL.into_iter().enumerate() {
                ui.radio_value(&mut self.kind, kind, kind.label());
                if i % 2 == 1 {
                    ui.end_row();
                }
            }
        });
        let hint = match self.kind {
            AttributeChartKind::C => Some("A c chart assumes the same inspection unit every time."),
            AttributeChartKind::LaneyP | AttributeChartKind::LaneyU => Some(
                "Widens the limits by the between-subgroup variation of the standardized values; \
                 for large subgroups whose ordinary limits flag too much.",
            ),
            AttributeChartKind::G | AttributeChartKind::T => {
                Some("Each row is the gap since the previous event; short gaps mean more frequent events.")
            }
            _ => None,
        };
        if let Some(hint) = hint {
            kit::note(ui, hint);
        }
        ui.add_space(6.0);
        let count_label = match self.kind {
            AttributeChartKind::P | AttributeChartKind::Np | AttributeChartKind::LaneyP => {
                "Nonconforming units per subgroup (one row each):"
            }
            AttributeChartKind::C | AttributeChartKind::U | AttributeChartKind::LaneyU => {
                "Nonconformities per subgroup (one row each):"
            }
            AttributeChartKind::G => "Units (or opportunities) between consecutive events (one row each):",
            AttributeChartKind::T => "Time between consecutive events (one row each):",
        };
        ui.label(count_label);
        kit::column_choice(ui, "attribute_count", table, &mut self.count, numeric, None);
        let needs_size = self.kind.needs_size();
        ui.add_enabled_ui(needs_size, |ui| {
            ui.add_space(6.0);
            ui.label("Units inspected per subgroup:");
            ui.horizontal(|ui| {
                ui.radio_value(&mut self.size_from_column, true, "From a column");
                if kit::column_choice(ui, "attribute_size", table, &mut self.size_column, numeric, None) {
                    self.size_from_column = true;
                }
            });
            ui.horizontal(|ui| {
                ui.radio_value(&mut self.size_from_column, false, "The same for every subgroup:");
                ui.add(egui::DragValue::new(&mut self.size_constant).range(1.0..=1e9).speed(1.0));
            });
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label("Phase:");
            kit::column_choice(ui, "attribute_phase", table, &mut self.phase, |_| true, Some("None"));
        })
        .response
        .on_hover_text("Each run of consecutive rows with the same value gets its own center line and limits");
        kit::more_options(ui, |ui| {
            ui.checkbox(&mut self.run_rules, "Apply run rules (patterns as well as points outside the limits)");
        });
        let Some(count) = self.count else {
            return Err(match self.kind {
                AttributeChartKind::G | AttributeChartKind::T => "Choose the column of gaps between events",
                _ => "Choose a count column",
            }
            .into());
        };
        let size = if !needs_size {
            SubgroupSize::None
        } else if self.size_from_column {
            match self.size_column {
                Some(c) => SubgroupSize::Column(c),
                None => return Err("Choose the column of units inspected".into()),
            }
        } else {
            SubgroupSize::Constant(self.size_constant.round())
        };
        let mut request = AttributeChartRequest::new(self.kind, count, size);
        request.phase = self.phase;
        request.rules = if self.run_rules { RuleSet::ALL } else { RuleSet::LIMITS_ONLY };
        let mut summary = format!("{} chart of {}", self.kind.short(), kit::name(table, count));
        match size {
            SubgroupSize::Column(c) => summary.push_str(&format!(", units inspected from {}", kit::name(table, c))),
            SubgroupSize::Constant(n) => summary.push_str(&format!(", {n} units per subgroup")),
            SubgroupSize::None => {}
        }
        if let Some(p) = self.phase {
            summary.push_str(&format!(", phases by {}", kit::name(table, p)));
        }
        kit::ready(request, summary)
    }
}

impl Remember for AttributeDialog {
    fn memory_key(&self) -> String {
        TITLE.into()
    }

    fn set_edit(&mut self, report: Option<u64>) {
        self.edit = report;
    }

    fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        self.count = columns.iter().copied().find(|&c| kit::is_numeric(table, c));
    }
}
