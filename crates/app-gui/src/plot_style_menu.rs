//! "Copy plot style" and "Paste plot style" in plot menus.
//!
//! The copied style is kept for the session in the context's data, so any
//! plot in any window can take it. A paste is a request a report hands to
//! the app ([`StyleRequest`]), which carries it out on the report, or on
//! every report of the project (see `app_model::style_paste`).

use app_model::style_paste::{CopiedStyle, StyleParts};
use eframe::egui;

/// Where a pasted style goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PasteTarget {
    /// One plot of the report, by its style key (`None`: the graph canvas).
    Plot(Option<String>),
    /// Every plot of the report.
    Report,
    /// Every plot of every report of the report's project.
    Project,
}

/// What a report's plot menu asked for (taken by the app).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StyleRequest {
    /// Copy the style of the plot with this style key (`None`: the graph
    /// canvas).
    Copy(Option<String>),
    /// Paste the copied style.
    Paste(PasteTarget),
}

fn copied_id() -> egui::Id {
    egui::Id::new("copied_plot_style")
}

fn parts_id() -> egui::Id {
    egui::Id::new("plot_style_parts")
}

/// The copied style, if any.
pub fn copied(ctx: &egui::Context) -> Option<CopiedStyle> {
    ctx.data(|d| d.get_temp::<CopiedStyle>(copied_id()))
}

/// Keep `style` as the copied style.
pub fn set_copied(ctx: &egui::Context, style: CopiedStyle) {
    ctx.data_mut(|d| d.insert_temp(copied_id(), style));
}

/// The parts a paste carries (ticked in the paste menu).
pub fn parts(ctx: &egui::Context) -> StyleParts {
    ctx.data(|d| d.get_temp::<StyleParts>(parts_id())).unwrap_or_default()
}

/// "Copy plot style" and the "Paste plot style" submenu for a plot with
/// style key `key` (`None`: a graph canvas), writing what was chosen into
/// `request`.
pub fn menu_items(ui: &mut egui::Ui, key: Option<&str>, request: &mut Option<StyleRequest>) {
    if ui
        .button("Copy plot style")
        .on_hover_text("Keep this plot's axes, ticks, grid, font, reference lines and key, to paste onto other plots")
        .clicked()
    {
        *request = Some(StyleRequest::Copy(key.map(str::to_owned)));
        ui.close_menu();
    }
    let copied = copied(ui.ctx());
    ui.add_enabled_ui(copied.is_some(), |ui| {
        ui.menu_button("Paste plot style", |ui| {
            let Some(copied) = &copied else { return };
            ui.label(egui::RichText::new(format!("From: {}", copied.from)).weak());
            let mut parts = parts(ui.ctx());
            let before = parts;
            for ((label, hover), flag) in StyleParts::LABELS.iter().zip(parts.flags_mut()) {
                ui.checkbox(flag, *label).on_hover_text(*hover);
            }
            if parts != before {
                ui.ctx().data_mut(|d| d.insert_temp(parts_id(), parts));
            }
            ui.separator();
            ui.add_enabled_ui(!parts.is_empty(), |ui| {
                let mut paste = |ui: &mut egui::Ui, label: &str, hover: &str, target: PasteTarget| {
                    if ui.button(label).on_hover_text(hover).clicked() {
                        *request = Some(StyleRequest::Paste(target));
                        ui.close_menu();
                    }
                };
                paste(ui, "Onto this plot", "Give this plot the ticked parts of the copied style", PasteTarget::Plot(key.map(str::to_owned)));
                paste(
                    ui,
                    "Onto every plot in this report",
                    "Every plot of this report, the collapsed ones too",
                    PasteTarget::Report,
                );
                paste(
                    ui,
                    "Onto every plot in this project",
                    "Every plot of every report and graph of this project's tables",
                    PasteTarget::Project,
                );
            });
        })
        .response
        .on_disabled_hover_text("Copy a plot's style first (right-click a plot ▸ Copy plot style)");
    });
}

fn canvas_request_id(report: u64) -> egui::Id {
    egui::Id::new(("canvas_style_request", report))
}

/// [`menu_items`] for the graph canvas of report `report`; the request is
/// kept until the report takes it ([`take_canvas_request`]).
pub fn canvas_menu_items(ui: &mut egui::Ui, report: u64) {
    let mut request = None;
    menu_items(ui, None, &mut request);
    if let Some(r) = request {
        ui.ctx().data_mut(|d| d.insert_temp(canvas_request_id(report), r));
    }
}

/// A request made in the canvas menu of report `report`.
pub fn take_canvas_request(ctx: &egui::Context, report: u64) -> Option<StyleRequest> {
    let id = canvas_request_id(report);
    ctx.data_mut(|d| {
        let r = d.get_temp::<StyleRequest>(id);
        d.remove::<StyleRequest>(id);
        r
    })
}
