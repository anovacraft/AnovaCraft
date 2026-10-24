//! A plot's figure output: the Copy plot and Save plot as… items of its
//! right-click menu, the Save plot dialog (size on paper, resolution,
//! background, font and text size, with a preview), the Transparent
//! background and Plot font items, and the figure look they share
//! ([`ExportLook`], kept in the plot's style).
//!
//! A plot's look is kept by its owner: report plots in their report's plot
//! styles, graph canvases in their graph. Menus and the dialog post a
//! changed look under the plot's [`target`]; the owner takes it on its next
//! frame ([`take_change`]). Pictures that keep no style (3-D views, trees,
//! diagrams, word clouds) keep their look for the session
//! ([`session_look`]).
//!
//! Figures drawn with the default look are written exactly as before; any
//! other look redraws the plot at its size on paper (or its on-screen size)
//! and writes it through [`stat_viz::figure_page::look_figure`].

use std::collections::HashMap;

use eframe::egui::{self, RichText};
use stat_viz::export::VectorFormat;
use stat_viz::figure_page::{Drawing, LengthUnit, figure_pixels, look_page_size, pixel_size};
use stat_viz::plot_style::{ExportLook, FigureSize, PlotFont};

use crate::dialog_kit::ComboExt as _;
use crate::figure_page_window::{PanelMaker, PanelRequest};
use crate::file_io;

/// The key under which a plot's look changes are posted: `parts` name the
/// plot (its report and place in it, or its graph).
pub(crate) fn target(parts: impl std::hash::Hash) -> egui::Id {
    egui::Id::new(("figure_look", parts))
}

/// Looks changed in menus and the dialog, until their plots take them.
#[derive(Clone, Default)]
struct Changes(Vec<(egui::Id, ExportLook)>);

fn changes_id() -> egui::Id {
    egui::Id::new("figure_look_changes")
}

fn post_change(ctx: &egui::Context, target: egui::Id, look: ExportLook) {
    ctx.data_mut(|d| {
        let changes = &mut d.get_temp_mut_or_default::<Changes>(changes_id()).0;
        changes.retain(|(t, _)| *t != target);
        changes.push((target, look));
    });
}

/// The look posted for `target` since it last looked, if any.
pub(crate) fn take_change(ctx: &egui::Context, target: egui::Id) -> Option<ExportLook> {
    ctx.data_mut(|d| {
        let changes = &mut d.get_temp_mut_or_default::<Changes>(changes_id()).0;
        let at = changes.iter().position(|(t, _)| *t == target)?;
        Some(changes.remove(at).1)
    })
}

/// Looks of plots that keep no style of their own, for the session.
#[derive(Clone, Default)]
struct SessionLooks(HashMap<egui::Id, ExportLook>);

/// The session look of `target` (taking any change posted for it).
pub(crate) fn session_look(ctx: &egui::Context, target: egui::Id) -> ExportLook {
    let id = egui::Id::new("figure_session_looks");
    if let Some(look) = take_change(ctx, target) {
        ctx.data_mut(|d| d.get_temp_mut_or_default::<SessionLooks>(id).0.insert(target, look));
    }
    ctx.data(|d| d.get_temp::<SessionLooks>(id).and_then(|s| s.0.get(&target).cloned())).unwrap_or_default()
}

/// A plot's figure for the output items.
pub(crate) struct FigureSource<'a> {
    /// The report showing the plot (where outcomes are shown).
    pub report: u64,
    /// File name and dialog title.
    pub name: &'a str,
    /// Draws the figure as on screen in a format (`None`: nothing to draw).
    pub make: &'a dyn Fn(VectorFormat) -> Option<Vec<u8>>,
    /// Makes an owned copy that redraws the figure at any size (`None` for
    /// pictures kept as drawn).
    pub maker: Option<&'a dyn Fn() -> PanelMaker>,
}

/// `maker` as a redraw at a size in points.
fn redraw_with(maker: &PanelMaker) -> impl Fn(f64, f64) -> Option<Drawing> + '_ {
    move |width, height| {
        maker(VectorFormat::Drawing, PanelRequest { width, height, titled: true })
            .as_deref()
            .and_then(Drawing::from_bytes)
    }
}

/// The figure in `format` drawn as `look` says.
fn figure_bytes(look: &ExportLook, format: VectorFormat, natural: &Drawing, maker: Option<&PanelMaker>) -> Vec<u8> {
    let redraw = maker.map(redraw_with);
    stat_viz::figure_page::look_figure(look, format, natural, redraw.as_ref().map(|r| r as &dyn Fn(f64, f64) -> _))
}

/// The figure of `source` in `format` as `look` says (`None` when there is
/// nothing to draw).
fn source_bytes(source: &FigureSource<'_>, look: &ExportLook, format: VectorFormat) -> Option<Vec<u8>> {
    if look.draws_as_plain() {
        return (source.make)(format);
    }
    let natural = (source.make)(VectorFormat::Drawing).as_deref().and_then(Drawing::from_bytes)?;
    let maker = source.maker.map(|m| m());
    Some(figure_bytes(look, format, &natural, maker.as_ref()))
}

/// Put the figure on the clipboard as PNG, PDF and SVG.
fn copy(draw: &dyn Fn(VectorFormat) -> Option<Vec<u8>>) -> (bool, String) {
    let (Some(png), Some(pdf), Some(svg)) = (draw(VectorFormat::Png), draw(VectorFormat::Pdf), draw(VectorFormat::Svg))
    else {
        return (false, "There is no plot to copy".to_owned());
    };
    #[cfg(not(target_arch = "wasm32"))]
    {
        let entries = crate::plot_clipboard::payload(crate::plot_clipboard::Platform::current(), png, pdf, svg);
        match crate::plot_clipboard::put(entries) {
            Ok(text) => (true, text),
            Err(text) => (false, text),
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (png, pdf, svg);
        (false, "Copying plots is not available in the browser".to_owned())
    }
}

/// The output items of a plot's right-click menu: Copy plot, Save plot as
/// each format, Save plot as… (the dialog), Transparent background and Plot
/// font. `prefix` starts the save items ("Save plot as"); `about` starts
/// their hover text. `look` is the plot's look, changes to which are posted
/// under `target`. Returns the outcome of a copy or save to show.
pub(crate) fn figure_items(
    ui: &mut egui::Ui,
    source: &FigureSource<'_>,
    prefix: &str,
    about: &str,
    look: &ExportLook,
    target: egui::Id,
) -> Option<(bool, String)> {
    let mut status = None;
    if ui
        .button("Copy plot")
        .on_hover_text(
            "Put the plot on the clipboard as PDF, PNG and SVG at once, so a slide, document or drawing program \
             pastes the best format it reads (sized and styled as Save plot as… says)",
        )
        .clicked()
    {
        ui.close_menu();
        status = Some(copy(&|f| source_bytes(source, look, f)));
    }
    let picture = if look.draws_as_plain() {
        "PNG is a 300 ppi picture; the others are vector figures.".to_owned()
    } else {
        format!("PNG is a {:.0} ppi picture; the others are vector figures.", look.pixels_per_inch())
    };
    for format in VectorFormat::ALL {
        if !ui.button(format!("{prefix} {}…", format.label())).on_hover_text(format!("{about} {picture}")).clicked() {
            continue;
        }
        ui.close_menu();
        let label = format!("{} figure", format.label());
        let Some(path) = file_io::pick_save_file("Save plot", &label, format.extension(), source.name) else {
            continue;
        };
        status = Some(match source_bytes(source, look, format) {
            Some(bytes) => write(&path, &bytes),
            None => (false, "There is no plot to save".to_owned()),
        });
    }
    if ui
        .button(format!("{prefix}…"))
        .on_hover_text(
            "Choose the format, the size on paper (millimetres or inches), the resolution, the background, the font \
             and the text size, with a preview; the plot keeps these settings",
        )
        .clicked()
    {
        ui.close_menu();
        match (source.make)(VectorFormat::Drawing).as_deref().and_then(Drawing::from_bytes) {
            Some(natural) => open_dialog(ui.ctx(), source, natural, look, target),
            None => status = Some((false, "There is no plot to save".to_owned())),
        }
    }
    let mut changed = look.clone();
    ui.checkbox(&mut changed.transparent, "Transparent background").on_hover_text(
        "Saved and copied figures leave the background clear instead of white, for coloured slides (EPS has no \
         transparency, so translucent marks are still blended with white)",
    );
    ui.menu_button("Plot font", |ui| font_controls(ui, &mut changed))
        .response
        .on_hover_text("The typeface and text size of the plot's titles and labels, on screen and in figures");
    if changed != *look {
        post_change(ui.ctx(), target, changed);
    }
    status
}

/// Font choice and text size.
fn font_controls(ui: &mut egui::Ui, look: &mut ExportLook) {
    for font in PlotFont::ALL {
        let hover = match font {
            PlotFont::Sans => "Helvetica in PDF and EPS (Liberation Sans in PNG); the interface font on screen",
            PlotFont::Serif => "Times in PDF and EPS (Liberation Serif in PNG and on screen)",
        };
        ui.radio_value(&mut look.font, font, font.label()).on_hover_text(hover);
    }
    ui.horizontal(|ui| {
        ui.label("Text size");
        let mut percent = look.text_scale() * 100.0;
        let r = ui.add(egui::DragValue::new(&mut percent).range(50.0..=300.0).speed(1.0).suffix(" %"));
        r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, "Text size"));
        let r = r.on_hover_text(
            "On screen: titles and tick labels. In figures: text, marks and lines together, as a figure laid out \
             smaller and enlarged",
        );
        if r.changed() {
            look.text_size = (percent / 100.0).clamp(0.5, 3.0);
        }
    });
}

fn write(path: &std::path::Path, bytes: &[u8]) -> (bool, String) {
    match std::fs::write(path, bytes) {
        Ok(()) => (true, format!("Saved {}", path.display())),
        Err(e) => (false, format!("Could not write {}: {e}", path.display())),
    }
}

// ---------------------------------------------------------------------------
// The Save plot dialog
// ---------------------------------------------------------------------------

/// Widths to start from, in millimetres (journal columns).
const WIDTHS: [(&str, f64); 3] = [
    ("One journal column (89 mm)", 89.0),
    ("One and a half columns (120 mm)", 120.0),
    ("Two journal columns (183 mm)", 183.0),
];

/// The open Save plot dialog.
#[derive(Clone)]
struct SaveDialog {
    target: egui::Id,
    report: u64,
    name: String,
    /// The plot as drawn on screen.
    natural: Drawing,
    /// Redraws the plot at any size.
    maker: Option<PanelMaker>,
    look: ExportLook,
    format: VectorFormat,
    preview: Option<(String, egui::TextureHandle)>,
    status: Option<(bool, String)>,
}

fn dialog_id() -> egui::Id {
    egui::Id::new("save_plot_dialog")
}

fn open_dialog(ctx: &egui::Context, source: &FigureSource<'_>, natural: Drawing, look: &ExportLook, target: egui::Id) {
    let dialog = SaveDialog {
        target,
        report: source.report,
        name: source.name.to_owned(),
        natural,
        maker: source.maker.map(|m| m()),
        look: look.clone(),
        format: VectorFormat::Pdf,
        preview: None,
        status: None,
    };
    ctx.data_mut(|d| d.insert_temp(dialog_id(), Some(dialog)));
}

/// Whether the Save plot dialog is open.
#[cfg(test)]
pub(crate) fn dialog_open(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<Option<SaveDialog>>(dialog_id()).flatten()).is_some()
}

/// Show the Save plot dialog when it is open (call once per frame).
pub(crate) fn show_dialog(ctx: &egui::Context) {
    let Some(mut dialog) = ctx.data(|d| d.get_temp::<Option<SaveDialog>>(dialog_id()).flatten()) else {
        return;
    };
    let mut open = true;
    let mut close = false;
    egui::Window::new("Save plot")
        .id(egui::Id::new("save_plot_window"))
        .open(&mut open)
        .default_size([760.0, 480.0])
        .resizable(true)
        .collapsible(false)
        .show(ctx, |ui| close = dialog.contents(ui));
    let keep = open && !close;
    ctx.data_mut(|d| d.insert_temp(dialog_id(), keep.then_some(dialog)));
}

impl SaveDialog {
    /// The page size in points.
    fn page(&self) -> (f64, f64) {
        look_page_size(&self.look, (self.natural.width, self.natural.height))
    }

    /// The contents; `true` when the dialog should close.
    fn contents(&mut self, ui: &mut egui::Ui) -> bool {
        let mut close = false;
        ui.label(RichText::new(&self.name).strong());
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(330.0);
                self.settings(ui);
                ui.add_space(6.0);
                close = self.buttons(ui);
            });
            ui.separator();
            ui.vertical(|ui| self.preview(ui));
        });
        close
    }

    fn settings(&mut self, ui: &mut egui::Ui) {
        let natural_unit = LengthUnit::Millimetres;
        egui::Grid::new("save_plot_settings").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            ui.label("Format");
            egui::ComboBox::from_id_salt("save_plot_format").selected_text(self.format.label()).show_list(ui, |ui| {
                for f in VectorFormat::ALL {
                    ui.selectable_value(&mut self.format, f, f.label());
                }
            });
            ui.end_row();
            ui.label("Size");
            let mut on_paper = self.look.size.is_some();
            if ui
                .checkbox(&mut on_paper, "Set the size on paper")
                .on_hover_text("Off keeps the plot's on-screen proportions at about its on-screen size")
                .changed()
            {
                self.look.size = on_paper.then(|| {
                    let (w, h) = (self.natural.width, self.natural.height);
                    let mm = |p: f64| (natural_unit.from_points(p) * 10.0).round() / 10.0;
                    FigureSize { width: mm(w), height: mm(h), unit: natural_unit }
                });
            }
            ui.end_row();
            if let Some(size) = &mut self.look.size {
                ui.label("Start from");
                egui::ComboBox::from_id_salt("save_plot_width")
                    .selected_text("Choose a width…")
                    .width(200.0)
                    .show_list(ui, |ui| {
                        for (name, mm) in WIDTHS {
                            if ui.selectable_label(false, name).clicked() {
                                let ratio = size.height / size.width;
                                let width = size.unit.from_points(LengthUnit::Millimetres.to_points(mm));
                                size.width = width;
                                if self.look.lock_ratio {
                                    size.height = width * ratio;
                                }
                            }
                        }
                    });
                ui.end_row();
                ui.label("Units");
                ui.horizontal(|ui| {
                    for unit in LengthUnit::ALL {
                        let name = if unit == LengthUnit::Millimetres { "Millimetres" } else { "Inches" };
                        if ui.radio(size.unit == unit, name).clicked() && size.unit != unit {
                            let convert = |v: f64| unit.from_points(size.unit.to_points(v));
                            *size = FigureSize { width: convert(size.width), height: convert(size.height), unit };
                        }
                    }
                });
                ui.end_row();
                let (lo, hi) = (FigureSize::MIN_POINTS, FigureSize::MAX_POINTS);
                let range = size.unit.from_points(lo)..=size.unit.from_points(hi);
                let (speed, decimals) = if size.unit == LengthUnit::Millimetres { (0.5, 1) } else { (0.02, 2) };
                let suffix = format!(" {}", size.unit.label());
                let ratio = size.height / size.width;
                ui.label("Width");
                let r = ui.add(
                    egui::DragValue::new(&mut size.width)
                        .range(range.clone())
                        .speed(speed)
                        .fixed_decimals(decimals)
                        .suffix(suffix.clone()),
                );
                r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, "Figure width"));
                if r.changed() && self.look.lock_ratio && ratio.is_finite() {
                    size.height = size.width * ratio;
                }
                ui.end_row();
                ui.label("Height");
                let r = ui.add(
                    egui::DragValue::new(&mut size.height)
                        .range(range)
                        .speed(speed)
                        .fixed_decimals(decimals)
                        .suffix(suffix),
                );
                r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, "Figure height"));
                if r.changed() && self.look.lock_ratio && ratio > 0.0 {
                    size.width = size.height / ratio;
                }
                ui.end_row();
                ui.label("");
                ui.checkbox(&mut self.look.lock_ratio, "Keep proportions")
                    .on_hover_text("Changing the width changes the height with it, and the other way round");
                ui.end_row();
            }
            let png = self.format == VectorFormat::Png;
            ui.label("Resolution");
            ui.horizontal(|ui| {
                let r = ui.add_enabled(
                    png,
                    egui::DragValue::new(&mut self.look.ppi).range(36.0..=1200.0).speed(5.0).suffix(" ppi"),
                );
                let r = r.on_hover_text("Pixels per inch of a PNG figure (vector formats have no resolution)");
                r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, "Pixels per inch"));
                if png {
                    let (w, h) = self.page();
                    let (pw, ph) = pixel_size(w, h, self.look.pixels_per_inch());
                    ui.label(RichText::new(format!("{pw} × {ph} px")).weak());
                }
            });
            ui.end_row();
            ui.label("Font");
            ui.vertical(|ui| font_controls(ui, &mut self.look));
            ui.end_row();
        });
        ui.checkbox(&mut self.look.transparent, "Transparent background").on_hover_text(
            "Leave the background clear instead of white (EPS has no transparency, so translucent marks are still \
             blended with white)",
        );
        if self.maker.is_none() {
            ui.label(
                RichText::new("This picture is kept as drawn and scaled to the size; text size does not apply.")
                    .weak()
                    .small(),
            );
        }
    }

    fn buttons(&mut self, ui: &mut egui::Ui) -> bool {
        let mut close = false;
        ui.horizontal(|ui| {
            if ui.button(format!("Save as {}…", self.format.label())).clicked() {
                let label = format!("{} figure", self.format.label());
                if let Some(path) = file_io::pick_save_file("Save plot", &label, self.format.extension(), &self.name) {
                    let status = write(&path, &self.bytes(self.format));
                    post_change(ui.ctx(), self.target, self.look.clone());
                    close = status.0;
                    if close {
                        // The outcome shows under the report's options button.
                        let id = crate::report_view::figure_status_id(self.report);
                        ui.ctx().data_mut(|d| d.insert_temp(id, status.clone()));
                    }
                    self.status = Some(status);
                }
            }
            if ui.button("Copy plot").on_hover_text("Put the figure on the clipboard as PDF, PNG and SVG").clicked() {
                self.status = Some(copy(&|f| Some(self.bytes(f))));
                post_change(ui.ctx(), self.target, self.look.clone());
            }
            if ui.button("Cancel").clicked() {
                close = true;
            }
        });
        if let Some((ok, text)) = &self.status {
            let color = if *ok { ui.visuals().weak_text_color() } else { ui.visuals().error_fg_color };
            ui.add(egui::Label::new(RichText::new(text).color(color).small()).wrap());
        }
        close
    }

    fn bytes(&self, format: VectorFormat) -> Vec<u8> {
        figure_bytes(&self.look, format, &self.natural, self.maker.as_ref())
    }

    /// The drawing placed on the page for the current settings.
    fn drawing(&self) -> Drawing {
        let (w, h) = self.page();
        let k = self.look.text_scale();
        self.maker.as_ref().and_then(|m| redraw_with(m)(w / k, h / k)).unwrap_or_else(|| self.natural.clone())
    }

    fn preview(&mut self, ui: &mut egui::Ui) {
        let (w, h) = self.page();
        let unit = self.look.size.map_or(LengthUnit::Millimetres, |s| s.unit);
        ui.label(
            RichText::new(format!("{:.1} × {:.1} {}", unit.from_points(w), unit.from_points(h), unit.label())).weak(),
        );
        let room = ui.available_size() - egui::vec2(8.0, 8.0);
        let fit = (f64::from(room.x.max(160.0)) / w).min(f64::from(room.y.max(160.0)) / h);
        let ppp = f64::from(ui.ctx().pixels_per_point());
        let dpi = (72.0 * fit * ppp).clamp(36.0, 300.0);
        let key = format!("{:?} {dpi:.1}", self.look);
        if self.preview.as_ref().is_none_or(|(k, _)| *k != key) {
            let (pw, ph, rgba) = figure_pixels(&self.drawing(), w, h, dpi, self.look.font);
            let image = egui::ColorImage::from_rgba_unmultiplied([pw as usize, ph as usize], &rgba);
            let texture = ui.ctx().load_texture("save_plot_preview", image, egui::TextureOptions::LINEAR);
            self.preview = Some((key, texture));
        }
        if let Some((_, texture)) = &self.preview {
            let size = egui::vec2((w * fit) as f32, (h * fit) as f32);
            let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
            ui.painter().image(
                texture.id(),
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
            ui.painter().rect_stroke(
                rect,
                0.0,
                ui.visuals().widgets.noninteractive.bg_stroke,
                egui::StrokeKind::Outside,
            );
            response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, "Figure preview"));
        }
    }
}
