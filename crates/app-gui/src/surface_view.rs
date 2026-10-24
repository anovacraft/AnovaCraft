//! The 3-D view of a contour and surface plot of data: the surface made
//! through the observations, shaded or as a wireframe, with contour lines on
//! it and on the floor, and the observations as points that can be selected.
//!
//! The grid and its contours come from [`app_model::DataSurfaceAnalysis::mesh`];
//! drawing, rotation, brushing and figure files are shared with the 3-D
//! surface profiler ([`crate::surface_profiler::plot`]). The view's settings
//! are display options of the report, saved with the project.

use app_model::{Camera, DataSurfaceAnalysis, DataSurfaceOptions};
use eframe::egui::{self, RichText};
use stat_data::RowStateManager;

use crate::surface_profiler::{SurfaceKind, SurfaceProfilerState, plot};
use crate::theme;

/// Draw the surface of `analysis` with its controls, keeping `options` and
/// the view `state` in step.
pub fn show(
    ui: &mut egui::Ui,
    analysis: &DataSurfaceAnalysis,
    options: &mut DataSurfaceOptions,
    state: &mut SurfaceProfilerState,
    rows: &mut RowStateManager,
    id: u64,
    style: Option<&stat_viz::ContourStyle>,
) {
    // The report's options are the source of truth; the view state follows.
    // Its colours are the contour plot's (set in that plot's Plot settings).
    let style = style.copied().unwrap_or_default();
    state.colors = (style.scale, style.reverse);
    state.camera = options.camera;
    state.levels = options.levels;
    state.filled = options.filled;
    state.surface_contours = options.surface_contours;
    state.floor_map = options.floor_map;
    ui.horizontal_wrapped(|ui| {
        ui.label("Draw as:");
        ui.radio_value(&mut options.filled, true, "Filled surface");
        ui.radio_value(&mut options.filled, false, "Wireframe");
        ui.separator();
        ui.label("Contour levels:");
        ui.add(egui::Slider::new(&mut options.levels, 0..=30))
            .on_hover_text("About this many, at round values (also used by the contour plot)");
        ui.checkbox(&mut options.surface_contours, "Contours on surface");
        ui.checkbox(&mut options.floor_map, "Contour map on floor");
        ui.checkbox(&mut options.points, "Show data points");
    });
    ui.horizontal_wrapped(|ui| {
        if ui.small_button("Reset view").clicked() {
            options.camera = Camera::default();
        }
        if ui.small_button("View from above").on_hover_text("Looks straight down: a filled contour map").clicked() {
            options.camera = Camera::TOP;
        }
        if ui.small_button("Side view").on_hover_text("Looks along the depth axis").clicked() {
            options.camera = Camera { azimuth: 0.0, elevation: 0.0 };
        }
        ui.label(
            RichText::new(format!(
                "Turned {:.0}°, tilted {:.0}° · drag the plot to rotate · double-click to reset",
                options.camera.azimuth, options.camera.elevation
            ))
            .weak()
            .small(),
        );
    });
    state.camera = options.camera;
    state.levels = options.levels;
    state.filled = options.filled;
    state.surface_contours = options.surface_contours;
    state.floor_map = options.floor_map;
    let mut mesh = match analysis.mesh(options.levels) {
        Ok(m) => m,
        Err(e) => {
            ui.colored_label(theme::ERROR, e.to_string());
            return;
        }
    };
    // Hidden rows are not drawn, so they cannot be selected here either.
    mesh.points.retain(|p| !rows.is_hidden(p.row));
    let kind = SurfaceKind::Data { exact: analysis.interpolates() };
    plot(ui, &mesh, state, options.points, rows, id, kind);
    // Dragging turned the view: keep it with the report.
    options.camera = state.camera;
}
