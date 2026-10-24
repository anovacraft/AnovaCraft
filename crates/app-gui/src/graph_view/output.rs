//! The graph's entries in its report options menu: key placement, point
//! labels, fit statistics, vector export, the summary table, and saving the
//! graph as a reusable pipeline file.

use eframe::egui::{self, RichText};
use stat_viz::export::{Figure, GridRules, RightView, export_graph_full, panel_views, summary_table, to_tsv};
use stat_viz::{GraphSpec, LegendPlacement};

use super::{GraphView, title};
use crate::file_io;

use app_model::report_run::graph_page_size as page_size;

/// The key under which the graph of report `id` takes figure look changes.
pub(in super::super) fn look_target(id: u64) -> egui::Id {
    crate::plot_export::target(("graph", id))
}

/// The options menu section for a graph.
pub(in super::super) fn options_menu(ui: &mut egui::Ui, spec: &mut GraphSpec, view: &mut GraphView) {
    ui.label(RichText::new("Graph").strong());
    ui.horizontal(|ui| {
        ui.label("Key:");
        for p in LegendPlacement::ALL {
            ui.radio_value(&mut spec.legend, p, p.label());
        }
    });
    if spec.encoding.label.is_some() {
        if let Some(p) = &mut spec.points {
            ui.checkbox(&mut p.label_selected_only, "Label only the selected rows");
        }
    } else {
        ui.label(RichText::new("Point labels: put a column on the Point labels shelf.").weak().small());
    }
    if let Some(f) = &mut spec.fit {
        ui.horizontal(|ui| {
            ui.label("Fit statistics on the graph:");
            ui.checkbox(&mut f.labels.equation, "equation");
            ui.checkbox(&mut f.labels.r_squared, "R²");
            ui.checkbox(&mut f.labels.p_value, "p-value");
        });
    }
    if spec.fit.is_some() || spec.ellipse.is_some() {
        ui.label(RichText::new("Drag a statistics label on the graph to move it off the data.").weak().small());
        if ui
            .add_enabled(!spec.label_positions.is_empty(), egui::Button::new("Reset label positions"))
            .on_hover_text("Put moved statistics labels back beside their lines")
            .clicked()
        {
            spec.label_positions.clear();
            ui.close_menu();
        }
    }
    ui.checkbox(&mut spec.axes.markup_labels, "Read math markup in column names, levels and labels")
        .on_hover_text(crate::plot_axes::MARKUP_HINT);
    ui.separator();
    let name = title(spec);
    let drawn =
        view.drawn.as_ref().and_then(|d| d.frame.as_ref().ok().map(|f| (f, &d.layers, &d.ranges, d.right.as_ref())));
    ui.add_enabled_ui(drawn.is_some(), |ui| {
        if let Some((frame, layers, ranges, right)) = drawn {
            let (xv, yv) = (&view.x_views, &view.y_views);
            let views = panel_views(frame, ranges, &|n: &str| xv.get(n).copied(), &|n: &str| yv.get(n).copied());
            let on = |g| g != app_model::axis_settings::GridLines::None;
            let axes = &view.canvas.axes;
            let grid = GridRules { vertical: on(axes.x.grid), horizontal: on(axes.y.grid) };
            let right = right.map(|axis| (axis.clone(), view.y2_view.unwrap_or(axis.range)));
            let draw = |spec: &GraphSpec, format, title: &str, (width, height): (f64, f64)| {
                let r = right.as_ref().map(|(axis, range)| RightView { axis, range: *range });
                export_graph_full(spec, frame, layers, &views, Figure { title, format, width, height }, grid, r)
            };
            let make = |format| Some(draw(spec, format, &name, page_size(frame.n_rows)));
            // The page redraws the graph at its panel's size from a copy.
            let maker = || -> crate::figure_page_window::PanelMaker {
                let (spec, frame, layers, views, name, right) =
                    (spec.clone(), frame.clone(), layers.clone(), views.clone(), name.clone(), right.clone());
                std::sync::Arc::new(move |format, r: crate::figure_page_window::PanelRequest| {
                    let title = if r.titled { name.as_str() } else { "" };
                    let figure = Figure { title, format, width: r.width, height: r.height };
                    let right = right.as_ref().map(|(axis, range)| RightView { axis, range: *range });
                    Some(export_graph_full(&spec, &frame, &layers, &views, figure, grid, right))
                })
            };
            let look = spec.axes.style.as_ref().map(|s| s.export.clone()).unwrap_or_default();
            let source = crate::plot_export::FigureSource { report: view.id, name: &name, make: &make, maker: Some(&maker) };
            let about = "The graph as shown (panels, layers, key).";
            if let Some(status) =
                crate::plot_export::figure_items(ui, &source, "Save as", about, &look, look_target(view.id))
            {
                view.status = Some(status);
            }
            if let Some(status) = crate::figure_page_window::add_item(ui, &name, &make, Some(&maker)) {
                view.status = Some(status);
            }
        }
        if ui
            .button("Copy summary table")
            .on_hover_text("Rows, mean, SD, minimum, median and maximum per position and subgroup, tab-separated")
            .clicked()
        {
            if let Some((frame, ..)) = drawn {
                let table = summary_table(frame);
                ui.ctx().copy_text(to_tsv(&table));
                view.status = Some((true, format!("Copied a summary table of {} row(s)", table.len() - 1)));
            }
            ui.close_menu();
        }
    });
    if ui
        .button("Save as graph pipeline…")
        .on_hover_text(
            "The graph's settings (columns by name) as a JSON file; open it on any dataset with the same column \
             names from Visualize ▸ Graph from pipeline file",
        )
        .clicked()
    {
        ui.close_menu();
        // Saved as `name.graph.json`.
        if let Some(path) =
            file_io::pick_save_file("Save graph pipeline", "Graph pipeline", "json", &format!("{name}.graph"))
        {
            view.status = Some(match std::fs::write(&path, app_model::graph_json::pipeline_text(spec)) {
                Ok(()) => (true, format!("Saved {}", path.display())),
                Err(e) => (false, format!("Could not write {}: {e}", path.display())),
            });
        }
    }
}

/// Ask for a graph pipeline file and read it. `Err` carries a message.
pub fn open_pipeline() -> Option<Result<GraphSpec, String>> {
    let path = file_io::pick_open_file("Open graph pipeline", "Graph pipeline", &["json"])?;
    Some(
        std::fs::read_to_string(&path)
            .map_err(|e| format!("Could not read {}: {e}", path.display()))
            .and_then(|text| app_model::graph_json::pipeline_from_text(&text).map_err(|e| e.to_string())),
    )
}
