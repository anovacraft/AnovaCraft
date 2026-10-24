//! Visualize ▸ Figure page…: several plots on one page with panel letters,
//! saved as one PNG, SVG, PDF or EPS file (the engine is
//! [`stat_viz::figure_page`]).
//!
//! Plots join the page from their right-click menus ("Add to figure page",
//! next to the "Save plot as" items, and on the graph canvas's output
//! menu). Report plots and graph canvases are kept as a copy of what they
//! show, redrawn at the size of their cell whenever the page changes, so
//! their text keeps its point size; other pictures (3-D views, trees,
//! diagrams, word clouds) are kept as they were drawn and shrunk to fit.
//! The page lasts while the app is open.
//!
//! The window holds the page settings (size in millimetres or inches, with
//! journal-column and paper presets; columns; space between panels; a
//! title; the letter style and size; whether plots keep their own titles),
//! the list of panels (each with its letter, the columns it spans, and
//! buttons to move or remove it), a preview, and the file settings: format,
//! pixel density for PNG, CMYK colour for PDF and EPS, the font (sans serif
//! or serif), and a transparent background.

use crate::dialog_kit::ComboExt as _;
use std::sync::Arc;

use eframe::egui::{self, RichText};
use stat_viz::export::VectorFormat;
use stat_viz::figure_page::{
    Drawing, LengthUnit, PageLayout, PageOutput, PanelLetters, export_page, page_pixels, pixel_size,
};

use crate::file_io;

/// The size a page asks a plot to be drawn at, in points, and whether the
/// plot keeps its own title.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PanelRequest {
    /// Width in points.
    pub width: f64,
    /// Height in points.
    pub height: f64,
    /// Whether the plot's own title is drawn.
    pub titled: bool,
}

/// Draws a plot at a requested size in a requested format (the page asks
/// for [`VectorFormat::Drawing`]); `None` when there is nothing to draw.
pub(crate) type PanelMaker = Arc<dyn Fn(VectorFormat, PanelRequest) -> Option<Vec<u8>> + Send + Sync>;

/// A plot sent to the page: its name, a copy as it was drawn, and (for
/// plots that can be redrawn at any size) how to redraw it.
#[derive(Clone)]
pub(crate) struct PanelSource {
    /// Shown in the panel list.
    pub name: String,
    /// The plot as drawn when it was added.
    pub snapshot: Drawing,
    /// Redraws the plot at a cell's size.
    pub maker: Option<PanelMaker>,
}

/// Plots added since the window last looked, kept in egui's memory so any
/// plot menu can add one.
#[derive(Clone, Default)]
struct Inbox(Vec<PanelSource>);

fn inbox_id() -> egui::Id {
    egui::Id::new("figure_page_inbox")
}

/// "Add to figure page": draws the plot with `make` (to keep a copy) and
/// sends it to the figure page with `maker` (`None` for pictures kept as
/// drawn). Returns the outcome to show, or `None` when not clicked.
pub(crate) fn add_item(
    ui: &mut egui::Ui,
    name: &str,
    make: &dyn Fn(VectorFormat) -> Option<Vec<u8>>,
    maker: Option<&dyn Fn() -> PanelMaker>,
) -> Option<(bool, String)> {
    let hover = if maker.is_some() {
        "Put this plot on the figure page (Visualize ▸ Figure page…), redrawn to fit its panel"
    } else {
        "Put this picture on the figure page (Visualize ▸ Figure page…), shrunk to fit its panel"
    };
    if !ui.button("Add to figure page").on_hover_text(hover).clicked() {
        return None;
    }
    ui.close_menu();
    let Some(snapshot) = make(VectorFormat::Drawing).as_deref().and_then(Drawing::from_bytes) else {
        return Some((false, "There is no plot to add".to_owned()));
    };
    let source = PanelSource { name: name.to_owned(), snapshot, maker: maker.map(|m| m()) };
    ui.ctx().data_mut(|d| d.get_temp_mut_or_default::<Inbox>(inbox_id()).0.push(source));
    Some((true, format!("Added “{name}” to the figure page")))
}

/// A page size to start from.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Preset {
    name: &'static str,
    /// Width and height in millimetres (`None` keeps the height).
    size: (f64, Option<f64>),
}

/// Typical journal column widths (one column, one and a half, two) and
/// paper sizes.
const PRESETS: [Preset; 7] = [
    Preset { name: "One journal column (89 mm wide)", size: (89.0, None) },
    Preset { name: "One and a half columns (120 mm wide)", size: (120.0, None) },
    Preset { name: "Two journal columns (183 mm wide)", size: (183.0, None) },
    Preset { name: "A4 portrait", size: (210.0, Some(297.0)) },
    Preset { name: "A4 landscape", size: (297.0, Some(210.0)) },
    Preset { name: "US Letter portrait", size: (215.9, Some(279.4)) },
    Preset { name: "US Letter landscape", size: (279.4, Some(215.9)) },
];

/// One panel of the page.
#[derive(Clone)]
struct Panel {
    id: u64,
    source: PanelSource,
    /// Columns spanned.
    span: usize,
}

/// The figure page and its window.
pub struct FigurePageWindow {
    /// The window is shown.
    pub open: bool,
    unit: LengthUnit,
    layout: PageLayout,
    /// Plots keep their own titles.
    titled: bool,
    panels: Vec<Panel>,
    next_id: u64,
    output: PageOutput,
    /// The drawings for the current settings, and what they were made for.
    drawings: Option<(String, Vec<Option<Drawing>>)>,
    /// The preview picture and what it shows.
    preview: Option<(String, egui::TextureHandle)>,
    status: Option<(bool, String)>,
}

impl Default for FigurePageWindow {
    fn default() -> Self {
        Self {
            open: false,
            unit: LengthUnit::Millimetres,
            layout: PageLayout::default(),
            titled: true,
            panels: Vec::new(),
            next_id: 0,
            output: PageOutput::default(),
            drawings: None,
            preview: None,
            status: None,
        }
    }
}

/// Formats the page can be saved in.
const FORMATS: [VectorFormat; 4] = VectorFormat::ALL;

impl FigurePageWindow {
    /// Names of the panels, in order.
    #[cfg(test)]
    pub(crate) fn panel_names(&self) -> Vec<String> {
        self.panels.iter().map(|p| p.source.name.clone()).collect()
    }

    /// Take the plots added from plot menus; the window opens when one arrives.
    fn collect(&mut self, ctx: &egui::Context) {
        let added = ctx.data_mut(|d| d.remove_temp::<Inbox>(inbox_id())).unwrap_or_default().0;
        for source in added {
            let letter = self.layout.letters.letter(self.panels.len());
            let name = source.name.clone();
            self.panels.push(Panel { id: self.next_id, source, span: 1 });
            self.next_id += 1;
            self.open = true;
            self.status = Some((
                true,
                if letter.is_empty() {
                    format!("Added “{name}”")
                } else {
                    format!("Added “{name}” as panel {letter}")
                },
            ));
        }
    }

    /// The page settings.
    #[cfg(test)]
    pub(crate) fn layout(&self) -> &PageLayout {
        &self.layout
    }

    /// Panel `i` as drawn for its cell.
    #[cfg(test)]
    pub(crate) fn drawn(&mut self, i: usize) -> Option<Drawing> {
        self.drawings().get(i).cloned().flatten()
    }

    pub(crate) fn spans(&self) -> Vec<usize> {
        self.panels.iter().map(|p| p.span).collect()
    }

    /// What the drawings depend on.
    fn key(&self) -> String {
        let ids: Vec<u64> = self.panels.iter().map(|p| p.id).collect();
        format!("{:?} {:?} {} {:?}", self.layout, self.spans(), self.titled, ids)
    }

    /// Each panel drawn for its cell: redrawn at the cell's size when it can
    /// be, otherwise its copy (shrunk to fit when the page is drawn).
    fn drawings(&mut self) -> &[Option<Drawing>] {
        let key = self.key();
        if self.drawings.as_ref().is_none_or(|(k, _)| *k != key) {
            let cells = self.layout.cells(&self.spans());
            let made = self
                .panels
                .iter()
                .zip(&cells)
                .map(|(p, cell)| {
                    let request =
                        PanelRequest { width: cell[2] - cell[0], height: cell[3] - cell[1], titled: self.titled };
                    let redrawn = p.source.maker.as_ref().and_then(|m| m(VectorFormat::Drawing, request));
                    match redrawn.as_deref().and_then(Drawing::from_bytes) {
                        Some(d) => Some(d),
                        None => Some(p.source.snapshot.clone()),
                    }
                })
                .collect();
            self.drawings = Some((key, made));
        }
        self.drawings.as_ref().map_or(&[], |(_, d)| d.as_slice())
    }

    /// The page as a file in the chosen format.
    pub(crate) fn page_bytes(&mut self) -> Vec<u8> {
        let (layout, spans, output) = (self.layout.clone(), self.spans(), self.output);
        let drawings: Vec<Option<&Drawing>> = self.drawings().iter().map(Option::as_ref).collect();
        export_page(&layout, &spans, &drawings, output)
    }

    /// Show the window (when open), first taking any plots just added.
    pub fn show(&mut self, ctx: &egui::Context) {
        self.collect(ctx);
        if !self.open {
            return;
        }
        let mut open = true;
        egui::Window::new("Figure page")
            .id(egui::Id::new("figure_page_window"))
            .open(&mut open)
            .default_size([980.0, 660.0])
            .resizable(true)
            .show(ctx, |ui| self.contents(ui));
        if !open {
            self.open = false;
        }
    }

    fn contents(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(330.0);
                egui::ScrollArea::vertical().id_salt("figure_page_settings").show(ui, |ui| {
                    self.page_settings(ui);
                    ui.separator();
                    self.panel_list(ui);
                    ui.separator();
                    self.file_settings(ui);
                });
            });
            ui.separator();
            ui.vertical(|ui| self.preview(ui));
        });
    }

    /// A length field in the page's unit, editing `points`.
    fn length(ui: &mut egui::Ui, unit: LengthUnit, points: &mut f64, range: (f64, f64), name: &str) {
        let mut v = unit.from_points(*points);
        let step = if unit == LengthUnit::Millimetres { 0.5 } else { 0.02 };
        let decimals = if unit == LengthUnit::Millimetres { 1 } else { 2 };
        let field = egui::DragValue::new(&mut v)
            .range(unit.from_points(range.0)..=unit.from_points(range.1))
            .speed(step)
            .fixed_decimals(decimals)
            .suffix(format!(" {}", unit.label()));
        let response = ui.add(field);
        let name = name.to_owned();
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, &name));
        if response.changed() {
            *points = unit.to_points(v);
        }
    }

    fn page_settings(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Page").strong());
        egui::Grid::new("figure_page_size").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            ui.label("Start from");
            egui::ComboBox::from_id_salt("figure_page_preset").selected_text("Choose a size…").width(200.0).show_list(
                ui,
                |ui| {
                    for p in PRESETS {
                        if ui.selectable_label(false, p.name).clicked() {
                            self.layout.width = LengthUnit::Millimetres.to_points(p.size.0);
                            if let Some(h) = p.size.1 {
                                self.layout.height = LengthUnit::Millimetres.to_points(h);
                            }
                        }
                    }
                },
            );
            ui.end_row();
            ui.label("Units");
            ui.horizontal(|ui| {
                ui.radio_value(&mut self.unit, LengthUnit::Millimetres, "Millimetres");
                ui.radio_value(&mut self.unit, LengthUnit::Inches, "Inches");
            });
            ui.end_row();
            let side = (PageLayout::MIN_SIDE, PageLayout::MAX_SIDE);
            ui.label("Width");
            Self::length(ui, self.unit, &mut self.layout.width, side, "Page width");
            ui.end_row();
            ui.label("Height");
            Self::length(ui, self.unit, &mut self.layout.height, side, "Page height");
            ui.end_row();
            ui.label("Columns");
            let r = ui.add(egui::DragValue::new(&mut self.layout.columns).range(1..=6).speed(0.05));
            r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, "Columns"));
            ui.end_row();
            ui.label("Space between");
            Self::length(ui, self.unit, &mut self.layout.gap, (0.0, 72.0), "Space between panels");
            ui.end_row();
            ui.label("Title");
            ui.add(egui::TextEdit::singleline(&mut self.layout.title).hint_text("None").desired_width(200.0));
            ui.end_row();
            ui.label("Panel letters");
            egui::ComboBox::from_id_salt("figure_page_letters").selected_text(self.layout.letters.label()).show_list(
                ui,
                |ui| {
                    for l in PanelLetters::ALL {
                        ui.selectable_value(&mut self.layout.letters, l, l.label());
                    }
                },
            );
            ui.end_row();
            ui.label("Letter size");
            let r = ui.add_enabled(
                self.layout.letters != PanelLetters::None,
                egui::DragValue::new(&mut self.layout.letter_size).range(4.0..=48.0).speed(0.25).suffix(" pt"),
            );
            r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, "Letter size"));
            ui.end_row();
        });
        ui.checkbox(&mut self.titled, "Keep each plot's own title")
            .on_hover_text("Off leaves the panel letters to name the plots, as most journals ask");
    }

    fn panel_list(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Panels").strong());
        if self.panels.is_empty() {
            ui.add(
                egui::Label::new(
                    RichText::new(
                        "No plots yet. Right-click any plot (or a graph's output menu) and choose Add to figure page.",
                    )
                    .weak(),
                )
                .wrap(),
            );
            return;
        }
        let columns = self.layout.columns.max(1);
        let mut action: Option<(usize, i32)> = None;
        let n = self.panels.len();
        egui::Grid::new("figure_page_panels").num_columns(4).spacing([6.0, 4.0]).show(ui, |ui| {
            for (i, panel) in self.panels.iter_mut().enumerate() {
                let letter = self.layout.letters.letter(i);
                ui.label(RichText::new(if letter.is_empty() { format!("{}", i + 1) } else { letter }).strong());
                let name = ui
                    .allocate_ui_with_layout(
                        egui::vec2(150.0, 18.0),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.set_min_width(150.0);
                            ui.add(egui::Label::new(&panel.source.name).truncate())
                        },
                    )
                    .inner;
                if panel.source.maker.is_none() {
                    name.on_hover_text("Kept as drawn and shrunk to fit its panel");
                } else {
                    name.on_hover_text("Redrawn to fit its panel");
                }
                panel.span = panel.span.clamp(1, columns);
                let r = ui.add_enabled(
                    columns > 1,
                    egui::DragValue::new(&mut panel.span).range(1..=columns).speed(0.05).suffix(" col"),
                );
                let label = format!("Columns spanned by panel {}", i + 1);
                r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, &label));
                ui.horizontal(|ui| {
                    let buttons: [(&str, bool, i32, &str); 3] =
                        [("⬆", i > 0, -1, "earlier"), ("⬇", i + 1 < n, 1, "later"), ("✕", true, 0, "off the page")];
                    for (icon, enabled, step, what) in buttons {
                        let r = ui.add_enabled(enabled, egui::Button::new(icon).small());
                        let name = if step == 0 {
                            format!("Remove panel {}", i + 1)
                        } else {
                            format!("Move panel {} {what}", i + 1)
                        };
                        r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, &name));
                        if r.on_hover_text(if step == 0 {
                            "Take this plot off the page".to_owned()
                        } else {
                            format!("Move {what}")
                        })
                        .clicked()
                        {
                            action = Some((i, step));
                        }
                    }
                });
                ui.end_row();
            }
        });
        match action {
            Some((i, 0)) => {
                self.panels.remove(i);
            }
            Some((i, d)) => {
                let j = (i as i64 + i64::from(d)) as usize;
                self.panels.swap(i, j);
            }
            None => {}
        }
        if ui.button("Clear page").on_hover_text("Remove every panel").clicked() {
            self.panels.clear();
        }
    }

    fn file_settings(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("File").strong());
        egui::Grid::new("figure_page_file").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            ui.label("Format");
            egui::ComboBox::from_id_salt("figure_page_format").selected_text(self.output.format.label()).show_list(
                ui,
                |ui| {
                    for f in FORMATS {
                        ui.selectable_value(&mut self.output.format, f, f.label());
                    }
                },
            );
            ui.end_row();
            let png = self.output.format == VectorFormat::Png;
            ui.label("Resolution");
            ui.horizontal(|ui| {
                let r = ui.add_enabled(
                    png,
                    egui::DragValue::new(&mut self.output.dpi).range(72.0..=1200.0).speed(5.0).suffix(" ppi"),
                );
                let r = r.on_hover_text("Pixels per inch of a PNG page (vector formats have no resolution)");
                r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, "Pixels per inch"));
                if png {
                    let (w, h) = pixel_size(self.layout.width, self.layout.height, self.output.dpi);
                    ui.label(RichText::new(format!("{w} × {h} px")).weak());
                }
            });
            ui.end_row();
            ui.label("Font");
            ui.horizontal(|ui| {
                for font in stat_viz::plot_style::PlotFont::ALL {
                    ui.radio_value(&mut self.output.font, font, font.label());
                }
            })
            .response
            .on_hover_text("Sans serif: Helvetica in PDF and EPS (Liberation Sans in PNG); Serif: Times (Liberation Serif)");
            ui.end_row();
        });
        let vector_ink = matches!(self.output.format, VectorFormat::Pdf | VectorFormat::Eps);
        ui.add_enabled(vector_ink, egui::Checkbox::new(&mut self.output.cmyk, "CMYK colour"))
            .on_hover_text(
                "Write colours as cyan, magenta, yellow and black for print (PDF and EPS). Converted by the simple \
                 complement rule without a colour profile, so a print shop may adjust them.",
            )
            .on_disabled_hover_text("PNG and SVG hold RGB colour only; choose PDF or EPS for CMYK");
        ui.checkbox(&mut self.output.transparent, "Transparent background").on_hover_text(
            "Leave the page clear instead of white, for placing on coloured slides (EPS has no transparency, so \
             translucent marks are still blended with white)",
        );
        ui.add_space(4.0);
        let ready = !self.panels.is_empty();
        if ui.add_enabled(ready, egui::Button::new(format!("Export page as {}…", self.output.format.label()))).clicked()
        {
            self.export();
        }
        if let Some((ok, text)) = &self.status {
            let color = if *ok { ui.visuals().weak_text_color() } else { ui.visuals().error_fg_color };
            ui.add(egui::Label::new(RichText::new(text).color(color).small()).wrap());
        }
    }

    fn export(&mut self) {
        let format = self.output.format;
        let label = format!("{} figure", format.label());
        let name =
            if self.layout.title.trim().is_empty() { "Figure page" } else { self.layout.title.trim() }.to_owned();
        let Some(path) = file_io::pick_save_file("Export figure page", &label, format.extension(), &name) else {
            return;
        };
        let bytes = self.page_bytes();
        self.status = Some(match std::fs::write(&path, bytes) {
            Ok(()) => (true, format!("Saved {}", path.display())),
            Err(e) => (false, format!("Could not write {}: {e}", path.display())),
        });
    }

    fn preview(&mut self, ui: &mut egui::Ui) {
        let (w, h) = (self.layout.width.max(1.0), self.layout.height.max(1.0));
        let unit = self.unit;
        ui.label(
            RichText::new(format!("{:.1} × {:.1} {}", unit.from_points(w), unit.from_points(h), unit.label())).weak(),
        );
        let room = ui.available_size() - egui::vec2(8.0, 8.0);
        let fit = (f64::from(room.x.max(120.0)) / w).min(f64::from(room.y.max(120.0)) / h);
        // Draw at the screen's pixel density so the preview is sharp.
        let ppp = f64::from(ui.ctx().pixels_per_point());
        let dpi = (72.0 * fit * ppp).clamp(36.0, 300.0);
        let key = format!("{} {dpi:.1}", self.key());
        if self.preview.as_ref().is_none_or(|(k, _)| *k != key) {
            let (layout, spans) = (self.layout.clone(), self.spans());
            let drawings: Vec<Option<&Drawing>> = self.drawings().iter().map(Option::as_ref).collect();
            let (pw, ph, rgba) = page_pixels(&layout, &spans, &drawings, dpi);
            let image = egui::ColorImage::from_rgba_unmultiplied([pw as usize, ph as usize], &rgba);
            let texture = ui.ctx().load_texture("figure_page_preview", image, egui::TextureOptions::LINEAR);
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
            response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, "Figure page preview"));
        }
    }
}
