//! Dialogs for images as tables (`app_model::image_table`): opening an
//! image file (channels, pixel size and, for an image larger than 1024
//! pixels on a side, Reduce / Crop / Keep full size with a preview to place
//! the crop), and File ▸ Export data as ▸ Image (a value column written as a
//! grey PNG or 16-bit TIFF between chosen limits).

use std::path::PathBuf;

use app_model::image_table::{
    ChannelChoice, DecodedImage, ImageFileFormat, ImageInfo, ImageOpenOptions, MAX_FULL_SIDE, MAX_SIDE, PixelGrid,
    SizeChoice, centred_crop, full_size_allowed, image_table, reduction_factor, table_bytes, value_range,
};
use eframe::egui::{self, RichText};
use stat_data::{DataTable, RowStateManager};

use crate::dialog_kit::ComboExt as _;
use crate::dialog_kit::{self as kit, Dialog, Outcome};

/// Longest side of the preview picture, in pixels of the picture.
const PREVIEW_SIDE: u32 = 512;

/// Which of the three ways a large image opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SizeMode {
    Reduce,
    Crop,
    Full,
}

/// The Open image dialog.
pub struct ImageOpenDialog {
    path: PathBuf,
    image: DecodedImage,
    channels: ChannelChoice,
    pixel_size: f64,
    mode: SizeMode,
    /// Crop rectangle in image pixels: left, top, width, height.
    crop: [usize; 4],
    preview: Option<egui::TextureHandle>,
    error: Option<String>,
}

/// What the dialog produced this frame.
pub enum ImageOpenOutcome {
    Open,
    Cancel,
    /// Open this table (made from the file), with what it was made from.
    Done(Box<OpenedImage>),
}

/// An image read as a table.
pub struct OpenedImage {
    pub path: PathBuf,
    pub table: DataTable,
    pub info: ImageInfo,
}

impl ImageOpenDialog {
    /// The dialog for the image file at `path`.
    ///
    /// # Errors
    /// When the file cannot be read or decoded.
    pub fn from_file(path: PathBuf) -> Result<Self, String> {
        let image = DecodedImage::open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Self::new(path, image))
    }

    /// The dialog for an image already decoded.
    pub fn new(path: PathBuf, image: DecodedImage) -> Self {
        let (w, h) = image.size();
        let SizeChoice::Crop { left, top, width, height } = centred_crop(w, h) else { unreachable!() };
        Self {
            path,
            image,
            channels: ChannelChoice::Grey,
            pixel_size: 1.0,
            mode: SizeMode::Reduce,
            crop: [left, top, width, height],
            preview: None,
            error: None,
        }
    }

    /// The image file.
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    /// Whether the image opens with no choices to make (it fits within
    /// 1024 × 1024 and is grey), so Settings' "open plain files directly"
    /// may skip the dialog.
    pub fn is_plain(&self) -> bool {
        let (w, h) = self.image.size();
        w <= MAX_SIDE && h <= MAX_SIDE && !self.image.is_colour()
    }

    fn size_choice(&self) -> SizeChoice {
        match self.mode {
            SizeMode::Reduce => SizeChoice::Reduce,
            SizeMode::Full => SizeChoice::Full,
            SizeMode::Crop => {
                let [left, top, width, height] = self.crop;
                SizeChoice::Crop { left, top, width, height }
            }
        }
    }

    fn options(&self) -> ImageOpenOptions {
        ImageOpenOptions { channels: self.channels, size: self.size_choice(), pixel_size: self.pixel_size }
    }

    /// Make the table now (also used when the dialog is skipped).
    ///
    /// # Errors
    /// As [`image_table`].
    pub fn finish(&self) -> Result<(DataTable, ImageInfo), String> {
        let file = self.path.file_name().map_or_else(String::new, |f| f.to_string_lossy().into_owned());
        let name = app_model::file_formats::name_from_path(&self.path);
        image_table(&self.image, &name, &file, &self.options()).map_err(|e| e.to_string())
    }

    pub fn show(&mut self, ctx: &egui::Context) -> ImageOpenOutcome {
        let em = ctx.style().text_styles.get(&egui::TextStyle::Body).map_or(14.0, |f| f.size);
        let file = self.path.file_name().map_or_else(String::new, |f| f.to_string_lossy().into_owned());
        let dialog =
            Dialog::new(format!("Open image {file}")).id_salt("open-image").creates().action("Open").default_width(em * 40.0);
        match dialog.show(ctx, |ui| self.contents(ui)) {
            Outcome::Open => ImageOpenOutcome::Open,
            Outcome::Cancel => ImageOpenOutcome::Cancel,
            Outcome::Reset => {
                let image = self.image.clone();
                *self = Self::new(self.path.clone(), image);
                ImageOpenOutcome::Open
            }
            Outcome::Run((), _) => match self.finish() {
                Ok((table, info)) => ImageOpenOutcome::Done(Box::new(OpenedImage { path: self.path.clone(), table, info })),
                Err(e) => {
                    self.error = Some(e);
                    ImageOpenOutcome::Open
                }
            },
        }
    }

    fn contents(&mut self, ui: &mut egui::Ui) -> kit::Readiness<()> {
        let (w, h) = self.image.size();
        ui.label(format!("{} image, {w} × {h} pixels, {}.", self.image.format, self.image.describe()));
        if self.image.has_alpha() {
            kit::note(ui, "Transparency is not read.");
        }
        let large = w > MAX_SIDE || h > MAX_SIDE;
        if large {
            ui.add_space(4.0);
            ui.label(RichText::new(format!("The image is larger than {MAX_SIDE} × {MAX_SIDE} pixels.")).strong());
            let k = reduction_factor(w, h);
            ui.radio_value(
                &mut self.mode,
                SizeMode::Reduce,
                format!("Reduce: average blocks of {k} × {k} pixels ({} × {})", w / k, h / k),
            )
            .on_hover_text("x and y stay in the original pixel units, so distances and frequencies keep their units");
            ui.radio_value(&mut self.mode, SizeMode::Crop, "Crop: a region at full resolution (drag it in the preview)");
            let full_ok = full_size_allowed(w, h);
            ui.add_enabled_ui(full_ok, |ui| {
                ui.radio_value(&mut self.mode, SizeMode::Full, "Keep full size")
                    .on_disabled_hover_text(format!("Images up to {MAX_FULL_SIDE} × {MAX_FULL_SIDE} pixels only"));
            });
            if !full_ok && self.mode == SizeMode::Full {
                self.mode = SizeMode::Reduce;
            }
        }
        self.preview_picture(ui, large);
        if self.mode == SizeMode::Crop && large {
            let [left, top, cw, ch] = &mut self.crop;
            ui.horizontal(|ui| {
                ui.label("Left:");
                ui.add(egui::DragValue::new(left).range(0..=w - 1));
                ui.label("Top:");
                ui.add(egui::DragValue::new(top).range(0..=h - 1));
                ui.label("Width:");
                ui.add(egui::DragValue::new(cw).range(1..=w.min(MAX_FULL_SIDE)));
                ui.label("Height:");
                ui.add(egui::DragValue::new(ch).range(1..=h.min(MAX_FULL_SIDE)));
            });
            *cw = (*cw).min(w - *left);
            *ch = (*ch).min(h - *top);
        }
        ui.add_space(4.0);
        if self.image.is_colour() {
            ui.horizontal(|ui| {
                ui.label("Columns:");
                ui.radio_value(&mut self.channels, ChannelChoice::Grey, "Grey (intensity)")
                    .on_hover_text("One intensity column: the luminance 0.2126 R + 0.7152 G + 0.0722 B");
                ui.radio_value(&mut self.channels, ChannelChoice::Rgb, "Red, green, blue and luminance");
            });
        }
        ui.horizontal(|ui| {
            ui.label("Pixel size:");
            ui.add(egui::DragValue::new(&mut self.pixel_size).speed(0.01).range(1e-9..=1e9))
                .on_hover_text("x and y are pixel positions times this (1 counts pixels)");
        });
        let plan = self.image.plan(self.size_choice());
        let values = if self.image.is_colour() && self.channels == ChannelChoice::Rgb { 4 } else { 1 };
        let bytes = table_bytes(plan.cells(), values + 2);
        let mb = bytes as f64 / 1e6;
        let size_note = format!(
            "The table will have {} rows (one per {}) and {} columns, about {mb:.0} MB.",
            plan.cells(),
            if plan.block > 1 { "block" } else { "pixel" },
            values + 2
        );
        if mb > 200.0 {
            ui.colored_label(crate::theme::stale_colors(ui.visuals().dark_mode).0, format!("⚠ {size_note}"));
        } else {
            kit::note(ui, &size_note);
        }
        if let Some(e) = &self.error {
            ui.colored_label(crate::theme::ERROR, e.as_str());
        }
        let (gw, gh) = plan.grid;
        kit::ready((), format!("Open a table of {gw} × {gh} = {} pixels", plan.cells()))
    }

    /// The preview, with the crop rectangle drawn on it; dragging in the
    /// preview moves the crop.
    fn preview_picture(&mut self, ui: &mut egui::Ui, large: bool) {
        let texture = match &self.preview {
            Some(t) => t.clone(),
            None => {
                let (tw, th, rgba) = self.image.thumbnail(PREVIEW_SIDE);
                let image = egui::ColorImage::from_rgba_unmultiplied([tw as usize, th as usize], &rgba);
                let t = ui.ctx().load_texture("open-image-preview", image, egui::TextureOptions::LINEAR);
                self.preview = Some(t.clone());
                t
            }
        };
        let (w, h) = self.image.size();
        let box_side = kit::em(ui) * 18.0;
        let scale = box_side / w.max(h) as f32;
        let size = egui::vec2(w as f32 * scale, h as f32 * scale);
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
        ui.painter().image(texture.id(), rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
        if !(large && self.mode == SizeMode::Crop) {
            return;
        }
        if let Some(pos) = response.interact_pointer_pos().filter(|_| response.dragged() || response.clicked()) {
            // Centre the crop on the pointer, kept inside the image.
            let [_, _, cw, ch] = self.crop;
            let cx = ((pos.x - rect.left()) / scale) as f64;
            let cy = ((pos.y - rect.top()) / scale) as f64;
            let place = |c: f64, len: usize, side: usize| (c - len as f64 / 2.0).round().clamp(0.0, (side - len) as f64) as usize;
            self.crop[0] = place(cx, cw, w);
            self.crop[1] = place(cy, ch, h);
        }
        let [left, top, cw, ch] = self.crop;
        let r = egui::Rect::from_min_size(
            rect.min + egui::vec2(left as f32 * scale, top as f32 * scale),
            egui::vec2(cw as f32 * scale, ch as f32 * scale),
        );
        let stroke = egui::Stroke::new(2.0_f32, ui.visuals().selection.stroke.color);
        ui.painter().rect_stroke(r, 0.0, stroke, egui::StrokeKind::Middle);
    }
}

/// File ▸ Export data as ▸ Image…: a value column written as a grey image.
pub struct ImageExportDialog {
    pub document: u64,
    x: Option<usize>,
    y: Option<usize>,
    value: Option<usize>,
    format: ImageFileFormat,
    /// Black and white values; `None` until set from the column's range.
    limits: Option<(f64, f64)>,
    limits_for: Option<usize>,
}

impl ImageExportDialog {
    pub fn new(document: u64, table: &DataTable) -> Self {
        let (x, y, value) = app_model::image_table::guess_columns(table);
        Self { document, x, y, value, format: ImageFileFormat::Png8, limits: None, limits_for: None }
    }

    /// The grid to write and its limits and format, when the user asks for it.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        table: &DataTable,
        rows: &RowStateManager,
    ) -> Outcome<(PixelGrid, (f64, f64), ImageFileFormat)> {
        let dialog = Dialog::new("Export image").id_salt("export-image").creates().action("Export…");
        dialog.show(ctx, |ui| self.contents(ui, table, rows))
    }

    fn contents(
        &mut self,
        ui: &mut egui::Ui,
        table: &DataTable,
        rows: &RowStateManager,
    ) -> kit::Readiness<(PixelGrid, (f64, f64), ImageFileFormat)> {
        let numeric = |i: usize| kit::is_numeric(table, i);
        kit::note(
            ui,
            "Each row is a pixel placed by its x and y on an evenly spaced grid (y downwards, as image rows); \
             the value is written as a grey level, black at the low limit and white at the high one. Excluded \
             rows and empty cells are black.",
        );
        egui::Grid::new("image_export_columns").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label("Across (x):");
            kit::column_choice(ui, "ie_x", table, &mut self.x, numeric, None);
            ui.end_row();
            ui.label("Down (y):");
            kit::column_choice(ui, "ie_y", table, &mut self.y, numeric, None);
            ui.end_row();
            ui.label("Value:");
            kit::column_choice(ui, "ie_value", table, &mut self.value, numeric, None);
            ui.end_row();
        });
        ui.horizontal(|ui| {
            ui.label("Format:");
            egui::ComboBox::from_id_salt("image_export_format")
                .selected_text(self.format.label())
                .width(kit::combo_width(ui) * 1.6)
                .show_list(ui, |ui| {
                    for f in ImageFileFormat::ALL {
                        ui.selectable_value(&mut self.format, f, f.label());
                    }
                });
        });
        let (Some(x), Some(y), Some(v)) = (self.x, self.y, self.value) else {
            return Err("Choose the x, y and value columns".into());
        };
        if !kit::all_different(&[Some(x), Some(y), Some(v)]) {
            return Err("Choose three different columns".into());
        }
        let col = |i: usize| table.column(i).and_then(|c| c.as_numeric()).unwrap_or(&[]);
        let grid = PixelGrid::from_columns(col(x), col(y), col(v), &|r| !(r < rows.len() && rows.is_excluded(r)))
            .map_err(|e| e.to_string())?;
        if self.limits_for != Some(v) || self.limits.is_none() {
            self.limits = Some(value_range(&grid.values));
            self.limits_for = Some(v);
        }
        let (mut lo, mut hi) = self.limits.unwrap_or((0.0, 1.0));
        ui.horizontal(|ui| {
            ui.label("Black at:");
            ui.add(egui::DragValue::new(&mut lo).speed(0.1));
            ui.label("white at:");
            ui.add(egui::DragValue::new(&mut hi).speed(0.1));
            if ui.button("Data range").on_hover_text("The least and greatest values").clicked() {
                (lo, hi) = value_range(&grid.values);
            }
        });
        self.limits = Some((lo, hi));
        if hi <= lo {
            return Err("The white value must be above the black value".into());
        }
        let summary = format!("Write a {} × {} image of {}", grid.cols, grid.rows, kit::name(table, v));
        kit::ready((grid, (lo, hi), self.format), summary)
    }
}
