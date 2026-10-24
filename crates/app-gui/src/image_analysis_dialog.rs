//! Dialogs for Series and Signals ▸ Signals and spectra ▸ Images: image spectrum,
//! frequency filter, and registration and deconvolution. An image is a
//! long-format table (one row per pixel) with x and y columns; one report
//! per chosen value column, with an optional group column for several
//! images stacked in one table.

use app_model::image_analysis::{
    BandChoice, DeblurMethod, ImageFilterSettings, ImageRestoreSettings, ImageSpectrumSettings, Images, NotchPoint,
    PaddingChoice, RestoreOperation, ShapeChoice, TaperChoice,
};
use app_model::{ImageAnalysisRequest, ImageInput, ImageMethod, SavedReport};
use eframe::egui;
use stat_data::{DataTable, RowStateManager};

use crate::dialog_kit::{self as kit, ComboExt as _, Dialog, Outcome, Readiness, Remember};

/// Which Images item the dialog is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageKind {
    Spectrum,
    Filter,
    Restore,
}

impl ImageKind {
    /// Every kind, in menu order.
    pub const ALL: [Self; 3] = [Self::Spectrum, Self::Filter, Self::Restore];

    /// Dialog title.
    pub fn title(self) -> &'static str {
        match self {
            Self::Spectrum => "Image spectrum",
            Self::Filter => "Image frequency filter",
            Self::Restore => "Image registration and deblurring",
        }
    }

    /// Menu item label.
    pub fn menu_label(self) -> String {
        format!("{}…", self.title())
    }

    /// Menu item hover text.
    pub fn hint(self) -> &'static str {
        match self {
            Self::Spectrum => {
                "2-D Fourier spectrum of an image (one row per pixel): log-magnitude and phase, the radially \
                 averaged and directional power, and the strongest repeating patterns with their spacing and angle"
            }
            Self::Filter => {
                "Smooth, sharpen or keep a band of spatial frequencies, or notch out a repeating pattern, by \
                 filtering the 2-D Fourier transform"
            }
            Self::Restore => {
                "Undo a Gaussian blur (Wiener, least squares or Richardson–Lucy), find and correct the shift \
                 between two images, or show the image's 2-D autocorrelation"
            }
        }
    }

    fn of(method: &ImageMethod) -> Self {
        match method {
            ImageMethod::Spectrum(_) => Self::Spectrum,
            ImageMethod::Filter(_) => Self::Filter,
            ImageMethod::Restore(_) => Self::Restore,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OperationChoice {
    Deblur,
    Register,
    Autocorrelation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeblurChoice {
    Wiener,
    Smoothness,
    RichardsonLucy,
}

/// Width and height of the first image and its Nyquist frequency, or why it
/// cannot be read.
type Geometry = Result<(usize, usize, f64), String>;
type GeometryKey = (usize, usize, usize, Option<usize>, usize);

#[derive(Clone)]
pub struct ImageAnalysisDialog {
    edit: Option<u64>,
    kind: ImageKind,
    chosen: Vec<bool>,
    x: Option<usize>,
    y: Option<usize>,
    group: Option<usize>,
    spectrum: ImageSpectrumSettings,
    /// The radial profiles' directions as typed (degrees, comma-separated).
    profile_text: String,
    filter: ImageFilterSettings,
    set_low: bool,
    low: f64,
    high: f64,
    set_radius: bool,
    radius: f64,
    operation: OperationChoice,
    deblur: DeblurChoice,
    lambda: f64,
    iterations: usize,
    sigma_x: f64,
    sigma_y: f64,
    other: Option<usize>,
    upsample: usize,
    remove_mean: bool,
    geometry: Option<(GeometryKey, Geometry)>,
}

impl ImageAnalysisDialog {
    pub fn new(kind: ImageKind, table: &DataTable) -> Self {
        let find = |names: &[&str]| {
            (0..table.n_cols()).find(|&i| {
                table.column(i).is_some_and(|c| names.iter().any(|n| c.name.eq_ignore_ascii_case(n)) && c.as_numeric().is_some())
            })
        };
        let x = find(&["x"]).or_else(|| (0..table.n_cols()).find(|&i| kit::name(table, i).to_lowercase().starts_with("x ")));
        let y = find(&["y"]).or_else(|| (0..table.n_cols()).find(|&i| kit::name(table, i).to_lowercase().starts_with("y ")));
        let mut chosen = vec![false; table.n_cols()];
        // An opened image's value column, when the table has one.
        if let Some(v) = find(&["intensity", "luminance"]) {
            chosen[v] = true;
        }
        Self {
            edit: None,
            kind,
            chosen,
            x,
            y,
            group: None,
            spectrum: ImageSpectrumSettings::default(),
            profile_text: directions_text(&ImageSpectrumSettings::default().profile_directions),
            filter: ImageFilterSettings::default(),
            set_low: false,
            low: f64::NAN,
            high: f64::NAN,
            set_radius: false,
            radius: f64::NAN,
            operation: OperationChoice::Deblur,
            deblur: DeblurChoice::Wiener,
            lambda: 0.01,
            iterations: 30,
            sigma_x: 2.0,
            sigma_y: 2.0,
            other: None,
            upsample: 20,
            remove_mean: true,
            geometry: None,
        }
    }

    /// The dialog filled from a saved image report.
    pub fn from_saved(saved: &SavedReport, table: &DataTable) -> Option<Self> {
        let SavedReport::ImageAnalysis { request } = saved else { return None };
        let mut d = Self::new(ImageKind::of(&request.method), table);
        let input = &request.input;
        d.chosen = kit::mask(table.n_cols(), &[input.value]);
        d.x = kit::valid(table, input.x);
        d.y = kit::valid(table, input.y);
        d.group = input.group.and_then(|c| kit::valid(table, c));
        match &request.method {
            ImageMethod::Spectrum(s) => {
                d.spectrum = s.clone();
                d.profile_text = directions_text(&s.profile_directions);
            }
            ImageMethod::Filter(f) => {
                d.filter = f.clone();
                d.set_low = f.low.is_some();
                d.low = f.low.unwrap_or(f64::NAN);
                d.high = f.high.unwrap_or(f64::NAN);
                d.set_radius = f.notch_radius.is_some();
                d.radius = f.notch_radius.unwrap_or(f64::NAN);
            }
            ImageMethod::Restore(r) => match &r.operation {
                RestoreOperation::Deblur { method, sigma_x, sigma_y } => {
                    d.operation = OperationChoice::Deblur;
                    (d.sigma_x, d.sigma_y) = (*sigma_x, *sigma_y);
                    match *method {
                        DeblurMethod::Wiener { lambda } => (d.deblur, d.lambda) = (DeblurChoice::Wiener, lambda),
                        DeblurMethod::Smoothness { lambda } => (d.deblur, d.lambda) = (DeblurChoice::Smoothness, lambda),
                        DeblurMethod::RichardsonLucy { iterations } => {
                            (d.deblur, d.iterations) = (DeblurChoice::RichardsonLucy, iterations);
                        }
                    }
                }
                RestoreOperation::Register { other, upsample } => {
                    d.operation = OperationChoice::Register;
                    d.other = kit::valid(table, *other);
                    d.upsample = *upsample;
                }
                RestoreOperation::Autocorrelation { remove_mean } => {
                    d.operation = OperationChoice::Autocorrelation;
                    d.remove_mean = *remove_mean;
                }
            },
        }
        Some(d)
    }

    /// One request per chosen value column, in table order.
    pub fn show(&mut self, ctx: &egui::Context, table: &DataTable) -> Outcome<Vec<ImageAnalysisRequest>> {
        let outcome = Dialog::new(self.kind.title()).editing(self.edit).show(ctx, |ui| {
            ui.set_max_width(kit::em(ui) * 34.0);
            kit::note(
                ui,
                match self.kind {
                    ImageKind::Spectrum => {
                        "Find the repeating patterns in an image: how strong each spatial frequency is, in which \
                         direction it runs, and how detail falls off from coarse to fine."
                    }
                    ImageKind::Filter => {
                        "Keep or remove ranges of spatial frequency: smooth out fine noise, take away slow \
                         shading, or notch out a regular pattern such as tool marks or a screen."
                    }
                    ImageKind::Restore => {
                        "Sharpen an image blurred by a known amount, line up a second image with this one, or \
                         see the distances at which the image repeats."
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

    /// Size and Nyquist frequency of the first chosen image (all rows included).
    fn geometry(&mut self, table: &DataTable, column: Option<usize>) -> Geometry {
        let (Some(column), Some(x), Some(y)) = (column, self.x, self.y) else {
            return Err("Choose the image's value, x and y columns".into());
        };
        let key = (column, x, y, self.group, table.n_rows());
        if let Some((k, g)) = &self.geometry {
            if *k == key {
                return g.clone();
            }
        }
        let input = ImageInput { value: column, x, y, group: self.group };
        let g = Images::extract(table, &RowStateManager::new(table.n_rows()), &input, 4)
            .map(|im| {
                let g = &im.images[0].grid;
                (g.cols, g.rows, (0.5 / g.dx).min(0.5 / g.dy))
            })
            .map_err(|e| e.to_string());
        self.geometry = Some((key, g.clone()));
        g
    }

    fn contents(&mut self, ui: &mut egui::Ui, table: &DataTable) -> Readiness<Vec<ImageAnalysisRequest>> {
        let numeric = |i: usize| kit::is_numeric(table, i) && table.column(i).is_some_and(|c| c.temporal().is_none());
        let (x, y) = (self.x, self.y);
        ui.label("Value columns (one report each):");
        kit::column_list(ui, ("image_values", self.kind.title()), table, &mut self.chosen, |i| {
            numeric(i) && Some(i) != x && Some(i) != y
        });
        ui.horizontal(|ui| {
            ui.label("x (across):");
            kit::column_choice(ui, "image_x", table, &mut self.x, numeric, None);
            ui.label("y (down):");
            kit::column_choice(ui, "image_y", table, &mut self.y, numeric, None);
        });
        ui.label("Group column (one image per level, stacked rows; optional):");
        kit::column_choice(ui, "image_group", table, &mut self.group, |_| true, Some("None: one image"));
        let columns: Vec<usize> =
            kit::chosen_columns(&self.chosen).into_iter().filter(|&c| c < table.n_cols() && numeric(c)).collect();
        let geometry = self.geometry(table, columns.first().copied());
        match &geometry {
            Ok((w, h, ny)) => {
                ui.label(
                    egui::RichText::new(format!(
                        "{w} × {h} pixels; spatial frequencies up to {} (the Nyquist frequency)",
                        app_model::format::format_number(*ny, 6)
                    ))
                    .weak()
                    .small(),
                );
            }
            Err(e) => {
                ui.label(egui::RichText::new(e).weak().small());
            }
        }
        ui.separator();
        let nyquist = geometry.as_ref().map(|g| g.2).ok();
        match self.kind {
            ImageKind::Spectrum => self.spectrum_options(ui),
            ImageKind::Filter => self.filter_options(ui, nyquist),
            ImageKind::Restore => self.restore_options(ui, table, numeric),
        }
        if columns.is_empty() {
            return Err("Choose at least one value column".into());
        }
        let (Some(x), Some(y)) = (self.x, self.y) else { return Err("Choose the x and y columns".into()) };
        if x == y || columns.contains(&x) || columns.contains(&y) {
            return Err("The x, y and value columns must all differ".into());
        }
        let nyquist = geometry.map(|g| g.2)?;
        if self.kind == ImageKind::Spectrum && self.spectrum.profiles {
            self.spectrum.profile_directions = parse_directions(&self.profile_text)?;
            if self.spectrum.profile_directions.is_empty() && !(self.spectrum.profile_peaks && self.spectrum.peaks > 0) {
                return Err("Give at least one profile direction, or profile the dominant frequencies".into());
            }
            if self.spectrum.profile_directions.len() > 12 {
                return Err("Profile at most 12 directions".into());
            }
        }
        if self.kind == ImageKind::Filter {
            let f = &self.filter;
            if f.band == BandChoice::None && f.notches.is_empty() && f.auto_notches == 0 {
                return Err("Choose a band filter or at least one notch".into());
            }
            if f.band != BandChoice::None && self.set_low {
                let edges = if f.band.two_edges() { vec![self.low, self.high] } else { vec![self.low] };
                if edges.iter().any(|&e| !(e > 0.0 && e <= nyquist)) {
                    return Err(format!(
                        "Cut-offs must lie between 0 and the Nyquist frequency {}",
                        app_model::format::format_number(nyquist, 6)
                    ));
                }
                if edges.len() == 2 && self.low >= self.high {
                    return Err("The lower edge must be below the upper edge".into());
                }
            }
        }
        if self.kind == ImageKind::Restore && self.operation == OperationChoice::Register {
            match self.other {
                None => return Err("Choose the image to align".into()),
                Some(o) if columns.contains(&o) || o == x || o == y => {
                    return Err("The image to align must differ from the chosen and position columns".into());
                }
                _ => {}
            }
        }
        let method = self.method();
        let group = self.group;
        let requests: Vec<ImageAnalysisRequest> = columns
            .iter()
            .map(|&value| ImageAnalysisRequest { input: ImageInput { value, x, y, group }, method: method.clone() })
            .collect();
        let summary = match columns.as_slice() {
            [c] => format!("{} of {}", self.kind.title(), kit::name(table, *c)),
            _ => format!("{} reports ({})", columns.len(), self.kind.title().to_lowercase()),
        };
        kit::ready(requests, summary)
    }

    fn method(&self) -> ImageMethod {
        match self.kind {
            ImageKind::Spectrum => ImageMethod::Spectrum(self.spectrum.clone()),
            ImageKind::Filter => {
                let mut f = self.filter.clone();
                f.low = (self.set_low && self.low.is_finite()).then_some(self.low);
                f.high = (self.set_low && f.band.two_edges() && self.high.is_finite()).then_some(self.high);
                f.notch_radius = (self.set_radius && self.radius.is_finite()).then_some(self.radius);
                ImageMethod::Filter(f)
            }
            ImageKind::Restore => {
                let operation = match self.operation {
                    OperationChoice::Deblur => RestoreOperation::Deblur {
                        method: match self.deblur {
                            DeblurChoice::Wiener => DeblurMethod::Wiener { lambda: self.lambda },
                            DeblurChoice::Smoothness => DeblurMethod::Smoothness { lambda: self.lambda },
                            DeblurChoice::RichardsonLucy => DeblurMethod::RichardsonLucy { iterations: self.iterations },
                        },
                        sigma_x: self.sigma_x,
                        sigma_y: self.sigma_y,
                    },
                    OperationChoice::Register => {
                        RestoreOperation::Register { other: self.other.unwrap_or(0), upsample: self.upsample }
                    }
                    OperationChoice::Autocorrelation => RestoreOperation::Autocorrelation { remove_mean: self.remove_mean },
                };
                ImageMethod::Restore(ImageRestoreSettings { operation })
            }
        }
    }

    fn spectrum_options(&mut self, ui: &mut egui::Ui) {
        let s = &mut self.spectrum;
        ui.horizontal(|ui| {
            ui.label("Window:");
            egui::ComboBox::from_id_salt("image_window").selected_text(s.taper.label()).show_list(ui, |ui| {
                for t in TaperChoice::ALL {
                    ui.selectable_value(&mut s.taper, t, t.label());
                }
            });
        })
        .response
        .on_hover_text("Tapering the edges stops the jump between opposite borders showing as a bright cross");
        padding_choice(ui, "image_spectrum_padding", &mut s.padding);
        ui.checkbox(&mut s.remove_mean, "Remove the mean first");
        ui.horizontal(|ui| {
            ui.checkbox(&mut s.phase, "Phase spectrum");
            ui.checkbox(&mut s.radial, "Radial spectrum and power law");
        });
        ui.horizontal(|ui| {
            ui.checkbox(&mut s.angular, "Angular spectrum, sectors:");
            ui.add_enabled(s.angular, egui::DragValue::new(&mut s.sectors).range(2..=720));
        });
        ui.horizontal(|ui| {
            ui.checkbox(&mut s.profiles, "Radial profiles at directions (°):").on_hover_text(
                "Cuts through the spectrum from its centre outwards: 0° runs across the image, 90° up it, \
                 45° up and to the right",
            );
            ui.add_enabled(
                s.profiles,
                egui::TextEdit::singleline(&mut self.profile_text).desired_width(120.0).hint_text("0, 90"),
            )
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Profile directions"));
        });
        ui.add_enabled_ui(s.profiles, |ui| {
            ui.horizontal(|ui| {
                ui.label("Average a wedge of ±").on_hover_text(
                    "0 takes a straight cut through the spectrum; a wider wedge averages the cells within that \
                     many degrees of the direction",
                );
                ui.add(egui::DragValue::new(&mut s.profile_half_width).range(0.0..=90.0).speed(0.5).max_decimals(2));
                ui.label("degrees");
            });
            ui.checkbox(&mut s.profile_peaks, "Also along each dominant frequency's direction");
        });
        ui.horizontal(|ui| {
            ui.label("Dominant frequencies listed:");
            ui.add(egui::DragValue::new(&mut s.peaks).range(0..=50));
        });
        ui.horizontal(|ui| {
            ui.label("Ignore frequencies below")
                .on_hover_text("Skips slow shading near the centre of the spectrum in the peaks, fits and angles");
            let mut pct = s.min_frequency_fraction * 100.0;
            if ui.add(egui::DragValue::new(&mut pct).range(0.0..=50.0).suffix(" %").max_decimals(2)).changed() {
                s.min_frequency_fraction = pct / 100.0;
            }
            ui.label("of the Nyquist frequency");
        });
    }

    fn filter_options(&mut self, ui: &mut egui::Ui, nyquist: Option<f64>) {
        let f = &mut self.filter;
        ui.horizontal(|ui| {
            ui.label("Band filter:");
            egui::ComboBox::from_id_salt("image_band").selected_text(f.band.label()).show_list(ui, |ui| {
                for b in BandChoice::ALL {
                    ui.selectable_value(&mut f.band, b, b.label());
                }
            });
        });
        if f.band != BandChoice::None {
            if let Some(ny) = nyquist {
                if !self.low.is_finite() {
                    self.low = 0.25 * ny;
                }
                if !self.high.is_finite() {
                    self.high = 0.5 * ny;
                }
            }
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.set_low, if f.band.two_edges() { "Edges:" } else { "Cut-off:" })
                    .on_hover_text("Without it: a quarter of the Nyquist frequency (bands: a quarter to a half)");
                ui.add_enabled_ui(self.set_low, |ui| {
                    let top = nyquist.unwrap_or(f64::INFINITY);
                    edge(ui, &mut self.low, top);
                    if f.band.two_edges() {
                        ui.label("to");
                        edge(ui, &mut self.high, top);
                    }
                });
            });
            ui.horizontal(|ui| {
                ui.label("Shape:");
                shape_choice(ui, "image_shape", &mut f.shape);
                if f.shape == ShapeChoice::Butterworth {
                    ui.label("order");
                    ui.add(egui::DragValue::new(&mut f.order).range(0.5..=20.0).speed(0.1).max_decimals(2));
                }
            });
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Notch the strongest");
            ui.add(egui::DragValue::new(&mut f.auto_notches).range(0..=20));
            ui.label("frequencies");
        })
        .response
        .on_hover_text("Removes regular patterns: each notch takes out one frequency and its mirror image");
        ui.label("Notches at given frequencies (horizontal, vertical):");
        let mut remove = None;
        for (k, p) in f.notches.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.label(format!("{}.", k + 1));
                ui.add(egui::DragValue::new(&mut p.fx).speed(0.001).max_decimals(6));
                ui.add(egui::DragValue::new(&mut p.fy).speed(0.001).max_decimals(6));
                if ui.small_button("Remove").clicked() {
                    remove = Some(k);
                }
            });
        }
        if let Some(k) = remove {
            f.notches.remove(k);
        }
        if ui.button("Add a notch").clicked() {
            f.notches.push(NotchPoint { fx: nyquist.map_or(0.1, |n| n / 4.0), fy: 0.0 });
        }
        if !f.notches.is_empty() || f.auto_notches > 0 {
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.set_radius, "Notch radius:")
                    .on_hover_text("Without it: two frequency steps of the image");
                if !self.radius.is_finite() {
                    self.radius = nyquist.map_or(0.01, |n| n / 50.0);
                }
                ui.add_enabled(self.set_radius, egui::DragValue::new(&mut self.radius).range(0.0..=1e9).speed(0.0005).max_decimals(6));
                ui.label("shape:");
                shape_choice(ui, "image_notch_shape", &mut f.notch_shape);
            });
            ui.checkbox(&mut f.notch_pass, "Keep only the notched frequencies (shows the pattern itself)");
        }
        padding_choice(ui, "image_filter_padding", &mut f.padding);
    }

    fn restore_options(&mut self, ui: &mut egui::Ui, table: &DataTable, numeric: impl Fn(usize) -> bool) {
        ui.radio_value(&mut self.operation, OperationChoice::Deblur, "Deblur (Gaussian blur of known size)");
        ui.radio_value(&mut self.operation, OperationChoice::Register, "Align a second image with this one");
        ui.radio_value(&mut self.operation, OperationChoice::Autocorrelation, "2-D autocorrelation");
        ui.separator();
        match self.operation {
            OperationChoice::Deblur => {
                ui.horizontal(|ui| {
                    ui.label("Blur standard deviation (pixels): across");
                    ui.add(egui::DragValue::new(&mut self.sigma_x).range(0.1..=50.0).speed(0.05).max_decimals(3));
                    ui.label("down");
                    ui.add(egui::DragValue::new(&mut self.sigma_y).range(0.1..=50.0).speed(0.05).max_decimals(3));
                });
                ui.radio_value(&mut self.deblur, DeblurChoice::Wiener, "Wiener filter")
                    .on_hover_text("Divides by the blur in frequency, held back where the blur leaves only noise");
                ui.radio_value(&mut self.deblur, DeblurChoice::Smoothness, "Constrained least squares (smooth result)")
                    .on_hover_text("As the Wiener filter, but penalises roughness, which suppresses speckle");
                ui.radio_value(&mut self.deblur, DeblurChoice::RichardsonLucy, "Richardson–Lucy iterations")
                    .on_hover_text("Keeps values non-negative; more iterations sharpen more and amplify noise");
                ui.horizontal(|ui| match self.deblur {
                    DeblurChoice::RichardsonLucy => {
                        ui.label("Iterations:");
                        ui.add(egui::DragValue::new(&mut self.iterations).range(1..=10_000));
                    }
                    _ => {
                        ui.label("Noise allowance λ:")
                            .on_hover_text("Larger values give a smoother, less sharp result; 0 divides by the blur outright");
                        ui.add(egui::DragValue::new(&mut self.lambda).range(0.0..=10.0).speed(0.001).max_decimals(5));
                    }
                });
            }
            OperationChoice::Register => {
                ui.label("Image to align (another value column on the same pixels):");
                kit::column_choice(ui, "image_other", table, &mut self.other, numeric, None);
                ui.horizontal(|ui| {
                    ui.label("Precision: 1 /");
                    ui.add(egui::DragValue::new(&mut self.upsample).range(1..=1000));
                    ui.label("pixel");
                });
            }
            OperationChoice::Autocorrelation => {
                ui.checkbox(&mut self.remove_mean, "Remove the mean first");
            }
        }
    }
}

fn padding_choice(ui: &mut egui::Ui, id: &str, padding: &mut PaddingChoice) {
    ui.horizontal(|ui| {
        ui.label("Padding:");
        egui::ComboBox::from_id_salt(id).selected_text(padding.label()).show_list(ui, |ui| {
            for p in PaddingChoice::ALL {
                ui.selectable_value(padding, p, p.label());
            }
        });
    })
    .response
    .on_hover_text("Mirroring the image before the transform avoids ringing at its borders");
}

fn shape_choice(ui: &mut egui::Ui, id: &str, shape: &mut ShapeChoice) {
    egui::ComboBox::from_id_salt(id).selected_text(shape.label()).show_list(ui, |ui| {
        for s in ShapeChoice::ALL {
            ui.selectable_value(shape, s, s.label());
        }
    });
}

/// Profile directions as typed in the dialog.
fn directions_text(directions: &[f64]) -> String {
    directions.iter().map(|&d| app_model::format::format_number(d, 6)).collect::<Vec<_>>().join(", ")
}

/// Directions typed as degrees separated by commas, spaces or semicolons.
fn parse_directions(text: &str) -> Result<Vec<f64>, String> {
    text.split([',', ';', ' '])
        .filter(|t| !t.trim().is_empty())
        .map(|t| {
            let t = t.trim().trim_end_matches('°');
            t.parse::<f64>()
                .ok()
                .filter(|d| d.is_finite())
                .ok_or_else(|| format!("{t:?} is not a direction: type degrees separated by commas, as in 0, 45, 90"))
        })
        .collect()
}

/// A frequency edge; until an image sets the Nyquist frequency it has no
/// value yet, shown as a dash.
fn edge(ui: &mut egui::Ui, value: &mut f64, top: f64) {
    if value.is_finite() {
        ui.add(egui::DragValue::new(value).range(0.0..=top).speed(0.001).max_decimals(6));
    } else {
        ui.weak("— (choose an image first)");
    }
}

/// The report's options menu items: display choices. Edits the request in
/// place; the report recomputes.
pub fn options_menu(ui: &mut egui::Ui, request: &mut ImageAnalysisRequest) {
    match &mut request.method {
        ImageMethod::Spectrum(s) => {
            ui.checkbox(&mut s.phase, "Phase spectrum");
            ui.checkbox(&mut s.radial, "Radial spectrum and power law");
            ui.checkbox(&mut s.angular, "Angular spectrum");
            ui.checkbox(&mut s.profiles, "Radial profiles");
            ui.menu_button("Window", |ui| {
                for t in TaperChoice::ALL {
                    if ui.radio(s.taper == t, t.label()).clicked() {
                        s.taper = t;
                        ui.close_menu();
                    }
                }
            });
        }
        ImageMethod::Filter(f) => {
            ui.checkbox(&mut f.notch_pass, "Keep only the notched frequencies");
            ui.menu_button("Padding", |ui| {
                for p in PaddingChoice::ALL {
                    if ui.radio(f.padding == p, p.label()).clicked() {
                        f.padding = p;
                        ui.close_menu();
                    }
                }
            });
        }
        ImageMethod::Restore(r) => {
            if let RestoreOperation::Autocorrelation { remove_mean } = &mut r.operation {
                ui.checkbox(remove_mean, "Remove the mean first");
            }
        }
    }
}

impl Remember for ImageAnalysisDialog {
    fn memory_key(&self) -> String {
        self.kind.title().to_owned()
    }

    fn set_edit(&mut self, report: Option<u64>) {
        self.edit = report;
    }

    fn prefill(&mut self, columns: &[usize], table: &DataTable) {
        let columns: Vec<usize> = columns.iter().copied().filter(|&c| Some(c) != self.x && Some(c) != self.y).collect();
        if !columns.is_empty() {
            self.chosen = kit::mask(table.n_cols(), &columns);
        }
    }
}
