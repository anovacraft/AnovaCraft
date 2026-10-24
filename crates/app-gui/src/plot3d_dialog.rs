//! Dialogs for the 3-D scatter plot (Visualize ▸ 3-D scatter plot…), the
//! contour and surface plot of data (Visualize ▸ Contour and surface plot…)
//! and the image plot (Visualize ▸ Image plot…).

use crate::dialog_kit::ComboExt as _;
use app_model::data_surface::{MAX_GRID, MIN_GRID};
use app_model::image_plot::ImagePlotRequest;
use app_model::{DataSurfaceRequest, SavedReport, Scatter3dRequest, SurfaceFit};
use stat_viz::palette::ColorScale;
use eframe::egui;
use stat_data::DataTable;

use crate::dialog_kit::{self as kit, Dialog, Outcome, Readiness, Remember};
use crate::report_view::ReportRequest;

/// Which plot the dialog sets up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plot3dKind {
    Scatter,
    Surface,
    /// A value drawn as a picture over x and y (image tables).
    Image,
}

/// Dialog of the two data plots in three dimensions.
#[derive(Clone)]
pub struct Plot3dDialog {
    edit: Option<u64>,
    kind: Plot3dKind,
    x: Option<usize>,
    y: Option<usize>,
    z: Option<usize>,
    color: Option<usize>,
    size: Option<usize>,
    shape: Option<usize>,
    method: SurfaceFit,
    grid: usize,
    /// Colours and axes of an image plot (its columns are `x`, `y`, `z`).
    image: ImagePlotRequest,
}

impl Plot3dDialog {
    pub fn new(kind: Plot3dKind) -> Self {
        Self {
            edit: None,
            kind,
            x: None,
            y: None,
            z: None,
            color: None,
            size: None,
            shape: None,
            method: SurfaceFit::Triangulation,
            grid: 40,
            image: ImagePlotRequest::new(0, 0, 0),
        }
    }

    /// The image plot dialog on `table`, its columns guessed from an image
    /// table's names (`x`, `y`, `intensity` or `luminance`).
    pub fn image(table: &DataTable) -> Self {
        let (x, y, z) = app_model::image_table::guess_columns(table);
        Self { x, y, z, ..Self::new(Plot3dKind::Image) }
    }

    /// The dialog filled from a saved 3-D scatter plot, surface of data or
    /// image plot.
    pub fn from_saved(saved: &SavedReport, table: &DataTable) -> Option<Self> {
        let v = |c: usize| kit::valid(table, c);
        let o = |c: Option<usize>| c.and_then(v);
        Some(match saved {
            SavedReport::Scatter3d { request, .. } => Self {
                x: v(request.x),
                y: v(request.y),
                z: v(request.z),
                color: o(request.color),
                size: o(request.size),
                shape: o(request.shape),
                ..Self::new(Plot3dKind::Scatter)
            },
            SavedReport::DataSurface { request, .. } => Self {
                x: v(request.x),
                y: v(request.y),
                z: v(request.z),
                method: request.method,
                grid: request.grid,
                ..Self::new(Plot3dKind::Surface)
            },
            SavedReport::ImagePlot { request } => Self {
                x: v(request.x),
                y: v(request.y),
                z: v(request.value),
                image: *request,
                ..Self::new(Plot3dKind::Image)
            },
            _ => return None,
        })
    }

    fn title(&self) -> &'static str {
        match self.kind {
            Plot3dKind::Scatter => "3-D scatter plot",
            Plot3dKind::Surface => "Contour and surface plot",
            Plot3dKind::Image => "Image plot",
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<ReportRequest> {
        let outcome = Dialog::new(self.title()).editing(self.edit).show(ctx, |ui| self.contents(ui, table));
        match outcome {
            Outcome::Reset => {
                *self = Self { edit: self.edit, ..Self::new(self.kind) };
                Outcome::Open
            }
            outcome => outcome,
        }
    }

    fn contents(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<ReportRequest> {
        let numeric = |i: usize| kit::is_numeric(table, i);
        let (across, up) = match self.kind {
            Plot3dKind::Scatter => ("Up (z):", "Rows are placed by three numeric columns in a box you can turn."),
            Plot3dKind::Surface => (
                "Response:",
                "A surface of the response is made through the observations over the two columns, and drawn \
                 as a contour plot and a 3-D surface you can turn.",
            ),
            Plot3dKind::Image => (
                "Value:",
                "The values are drawn as a picture, one cell per row, placed by two columns on an evenly spaced \
                 grid (an image opened as a table has columns x, y and intensity).",
            ),
        };
        let (across_label, depth_label) =
            if self.kind == Plot3dKind::Image { ("Across (x):", "Down (y):") } else { ("Across (x):", "Depth (y):") };
        kit::note(ui, up);
        egui::Grid::new("plot3d_columns").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label(across_label);
            kit::column_choice(ui, "p3_x", table, &mut self.x, numeric, None);
            ui.end_row();
            ui.label(depth_label);
            kit::column_choice(ui, "p3_y", table, &mut self.y, numeric, None);
            ui.end_row();
            ui.label(across);
            kit::column_choice(ui, "p3_z", table, &mut self.z, numeric, None);
            ui.end_row();
            if self.kind == Plot3dKind::Scatter {
                ui.label("Colour:");
                kit::column_choice(ui, "p3_color", table, &mut self.color, |_| true, Some("None"));
                ui.end_row();
                ui.label("Size:");
                kit::column_choice(ui, "p3_size", table, &mut self.size, numeric, Some("None"));
                ui.end_row();
                ui.label("Shape:");
                kit::column_choice(ui, "p3_shape", table, &mut self.shape, |_| true, Some("None"));
                ui.end_row();
            }
        });
        if self.kind == Plot3dKind::Scatter {
            ui.label(
                egui::RichText::new(
                    "A continuous colour column is shown on a colour ramp, any other with one colour and marker \
                     per level. Marker area follows the size column.",
                )
                .weak()
                .small(),
            );
        } else if self.kind == Plot3dKind::Image {
            image_settings(ui, &mut self.image);
        } else {
            ui.horizontal(|ui| {
                ui.label("Surface:");
                egui::ComboBox::from_id_salt("p3_method")
                    .selected_text(self.method.label())
                    .width(kit::combo_width(ui) * 1.6)
                    .show_list(ui, |ui| {
                        for m in SurfaceFit::ALL {
                            ui.selectable_value(&mut self.method, m, m.label());
                        }
                    });
            });
            let help = match self.method {
                SurfaceFit::Triangulation => {
                    "Joins the observations into triangles (Delaunay) and makes each triangle flat: the surface \
                     passes through every point and stops at the edge of the data."
                }
                SurfaceFit::Spline => {
                    "The smoothest surface (least bending) that passes through every point; it extends to the \
                     corners of the plot."
                }
                SurfaceFit::SmoothingSpline => {
                    "A thin-plate spline that trades closeness to the points for smoothness, with the balance \
                     chosen by generalized cross-validation; use it for noisy measurements."
                }
            };
            ui.label(egui::RichText::new(help).weak().small());
            kit::more_options(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Grid divisions:");
                    ui.add(egui::DragValue::new(&mut self.grid).range(MIN_GRID..=MAX_GRID))
                        .on_hover_text("Divisions along each axis of the grid the surface is drawn on");
                });
            });
        }
        let (Some(x), Some(y), Some(z)) = (self.x, self.y, self.z) else {
            return Err(match self.kind {
                Plot3dKind::Image => "Choose the across, down and value columns".into(),
                _ => "Choose the across, depth and up columns".into(),
            });
        };
        if !kit::all_different(&[Some(x), Some(y), Some(z)]) {
            return Err("Choose three different columns".into());
        }
        let name = |i: usize| kit::name(table, i).to_owned();
        match self.kind {
            Plot3dKind::Scatter => {
                let request = Scatter3dRequest { color: self.color, size: self.size, shape: self.shape, x, y, z };
                let mut summary = format!("3-D scatter plot of {} by {} and {}", name(z), name(x), name(y));
                let channels: Vec<String> = [("colour", self.color), ("size", self.size), ("shape", self.shape)]
                    .into_iter()
                    .filter_map(|(k, c)| c.map(|c| format!("{k} {}", name(c))))
                    .collect();
                if !channels.is_empty() {
                    summary.push_str(&format!(" ({})", channels.join(", ")));
                }
                kit::ready(ReportRequest::Scatter3d(request), summary)
            }
            Plot3dKind::Surface => {
                let request = DataSurfaceRequest { x, y, z, method: self.method, grid: self.grid };
                let summary = format!("Contour and surface plot of {} over {} and {}", name(z), name(x), name(y));
                kit::ready(ReportRequest::DataSurface(request), summary)
            }
            Plot3dKind::Image => {
                let request = ImagePlotRequest { x, y, value: z, ..self.image };
                let summary = format!("Image plot of {} over {} and {}", name(z), name(x), name(y));
                kit::ready(ReportRequest::Comparison(SavedReport::ImagePlot { request }), summary)
            }
        }
    }
}

impl Remember for Plot3dDialog {
    fn memory_key(&self) -> String {
        self.title().into()
    }

    fn set_edit(&mut self, report: Option<u64>) {
        self.edit = report;
    }

    fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        if self.kind == Plot3dKind::Image && columns.is_empty() {
            // An image table names its columns.
            let (x, y, value) = app_model::image_table::guess_columns(table);
            (self.x, self.y, self.z) = (x, y, value);
            return;
        }
        let numeric: Vec<usize> = columns.iter().copied().filter(|&c| kit::is_numeric(table, c)).collect();
        self.x = numeric.first().copied();
        self.y = numeric.get(1).copied();
        self.z = numeric.get(2).copied();
    }
}

/// Colours, limits and axes of an image plot, in its dialog and in the
/// report's settings.
pub fn image_settings(ui: &mut egui::Ui, r: &mut ImagePlotRequest) {
    ui.horizontal(|ui| {
        ui.label("Colours:");
        egui::ComboBox::from_id_salt("image_scale")
            .selected_text(r.scale.label())
            .width(kit::combo_width(ui) * 1.6)
            .show_list(ui, |ui| {
                for s in ColorScale::ALL {
                    ui.selectable_value(&mut r.scale, s, s.label());
                }
            });
    });
    ui.checkbox(&mut r.reversed, "Reverse the colours")
        .on_hover_text("Run the colours from the high end to the low end; the grey scale then shows low values black");
    let mut fixed = r.limits.is_some();
    ui.checkbox(&mut fixed, "Fixed colour range")
        .on_hover_text("Map these values to the ends of the colours; otherwise the least and greatest values");
    if fixed {
        let (mut lo, mut hi) = r.limits.unwrap_or((0.0, 255.0));
        ui.horizontal(|ui| {
            ui.label("From:");
            ui.add(egui::DragValue::new(&mut lo).speed(1.0));
            ui.label("to:");
            ui.add(egui::DragValue::new(&mut hi).speed(1.0));
        });
        if hi <= lo {
            hi = lo + 1.0;
        }
        r.limits = Some((lo, hi));
    } else {
        r.limits = None;
    }
    ui.horizontal(|ui| {
        ui.label("Vertical axis:");
        ui.radio_value(&mut r.y_down, true, "Downwards (image rows)");
        ui.radio_value(&mut r.y_down, false, "Upwards");
    });
    ui.checkbox(&mut r.equal_aspect, "Equal scale (square pixels stay square)");
}
