//! Contour maps on report plots: the filled bands, level lines and values
//! drawn under a plot's points, the colour bar beside it, and the contour
//! part of the plot's Plot settings menu.
//!
//! The map's geometry comes from [`app_model::report::ContourMap`]; its
//! style is the report's (`ReportScope::contour`), so every contour plot of
//! a report shares it and it is saved with the project.

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use app_model::report::ContourMap;
use eframe::egui::{self, Color32, RichText};
use egui_plot::{Line, PlotPoint, PlotPoints, PlotUi, Text};
use stat_viz::palette::ColorScale;
use stat_viz::render::{FillMesh, fill_item};
use stat_viz::{ColorBar, ContourDrawing, ContourStyle};

use crate::dialog_kit::ComboExt as _;
use crate::theme::PlotPalette;

/// Level lines over a filled map: dark and translucent, so they read on
/// every band colour.
const INK: Color32 = Color32::from_rgba_premultiplied(11, 13, 18, 140);

/// A map ready to draw: what it draws and its bands as one mesh.
#[derive(Debug)]
pub struct Contours {
    /// Bands, lines, labels and colour bar.
    pub drawing: ContourDrawing,
    mesh: Arc<FillMesh>,
    filled: bool,
}

/// The drawing of a map, kept between frames while the surface and style
/// stay the same (`key` identifies them).
#[derive(Debug, Default)]
pub struct ContourCache(Option<(u64, Arc<Contours>)>);

impl ContourCache {
    /// The drawing of `map` in the dark variant when `dark`.
    pub fn get(&mut self, map: &ContourMap, dark: bool) -> Arc<Contours> {
        let key = key(map, dark);
        if let Some((k, c)) = &self.0 {
            if *k == key {
                return Arc::clone(c);
            }
        }
        let drawing = map.drawing(dark);
        let mesh = FillMesh::new(drawing.bands.iter().map(|(color, band)| (band.pieces.as_slice(), color32(*color))));
        let c = Arc::new(Contours { drawing, mesh: Arc::new(mesh), filled: map.style.fill });
        self.0 = Some((key, Arc::clone(&c)));
        c
    }
}

pub(crate) fn color32(c: stat_viz::Rgba) -> Color32 {
    let [r, g, b, a] = c.0;
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

/// A hash of everything the drawing depends on.
fn key(map: &ContourMap, dark: bool) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    dark.hash(&mut h);
    map.levels.hash(&mut h);
    map.value_name.hash(&mut h);
    format!("{:?}", map.style).hash(&mut h);
    map.field.triangles.len().hash(&mut h);
    for v in map.field.values.iter().chain(map.field.points.iter().flatten()) {
        v.to_bits().hash(&mut h);
    }
    h.finish()
}

impl Contours {
    /// Whether the bands are filled (points then get a rim).
    pub fn filled(&self) -> bool {
        self.filled
    }

    /// Draw the bands and lines (under everything else).
    pub fn draw_under(&self, plot: &mut PlotUi, pal: &PlotPalette) {
        if !self.mesh.is_empty() {
            plot.add(fill_item(Arc::clone(&self.mesh), ""));
        }
        let ink = if self.filled { INK } else { pal.chart_trace };
        for (_, lines) in &self.drawing.lines {
            for l in lines {
                plot.line(Line::new(PlotPoints::from(l.clone())).color(ink).width(1.0_f32));
            }
        }
    }

    /// Write the level values on their lines, on a backing so they read on
    /// any colour.
    pub fn draw_labels(&self, plot: &mut PlotUi, pal: &PlotPalette) {
        let backing = pal.plot_bg.gamma_multiply(0.85);
        for (at, _, text) in &self.drawing.labels {
            plot.text(
                Text::new(PlotPoint::new(at[0], at[1]), RichText::new(text.as_str()).small().background_color(backing))
                    .color(pal.chart_trace)
                    .anchor(egui::Align2::CENTER_CENTER),
            );
        }
    }

    /// Width the colour bar takes beside the plot (0 without one).
    pub fn bar_width(&self, ui: &egui::Ui) -> f32 {
        self.drawing.bar.as_ref().map_or(0.0, |bar| color_bar_width(ui, bar))
    }

    /// Paint the colour bar to the right of `plot` (the plot area's
    /// rectangle).
    pub fn paint_bar(&self, ui: &egui::Ui, plot: egui::Rect) {
        if let Some(bar) = &self.drawing.bar {
            paint_color_bar(ui, bar, plot);
        }
    }
}

/// Width colour bar `bar` takes beside a plot.
pub fn color_bar_width(ui: &egui::Ui, bar: &ColorBar) -> f32 {
    let font = egui::TextStyle::Small.resolve(ui.style());
    let widest = bar
        .labels
        .iter()
        .map(|l| ui.fonts(|f| f.layout_no_wrap(l.clone(), font.clone(), Color32::WHITE).size().x))
        .fold(0.0_f32, f32::max);
    BAR_GAP + BAR_SWATCH + 5.0 + widest + 24.0
}

/// Paint colour bar `bar` to the right of `plot` (the plot area's
/// rectangle): a box per band, low at the bottom, the values at the
/// boundaries and the title reading upwards.
pub fn paint_color_bar(ui: &egui::Ui, bar: &ColorBar, plot: egui::Rect) {
    let n = bar.colors.len();
    if n == 0 {
        return;
    }
    let painter = ui.painter();
    let text_color = ui.visuals().text_color();
    let weak = ui.visuals().weak_text_color();
    let font = egui::TextStyle::Small.resolve(ui.style());
    let h = (plot.height() / n as f32).min(30.0);
    let total = h * n as f32;
    let top = plot.center().y - total / 2.0;
    let x = plot.right() + BAR_GAP;
    let box_top = |k: usize| top + (n - 1 - k) as f32 * h;
    for (k, c) in bar.colors.iter().enumerate() {
        let r = egui::Rect::from_min_size(egui::pos2(x, box_top(k)), egui::vec2(BAR_SWATCH, h));
        painter.rect_filled(r, 0.0_f32, color32(*c));
    }
    let frame = egui::Rect::from_min_size(egui::pos2(x, top), egui::vec2(BAR_SWATCH, total));
    painter.rect_stroke(frame, 0.0_f32, egui::Stroke::new(1.0_f32, weak), egui::StrokeKind::Outside);
    let mut widest: f32 = 0.0;
    for (k, label) in bar.labels.iter().enumerate() {
        let y = box_top(k);
        painter.line_segment(
            [egui::pos2(x + BAR_SWATCH, y), egui::pos2(x + BAR_SWATCH + 3.0, y)],
            egui::Stroke::new(1.0_f32, weak),
        );
        let r = painter.text(
            egui::pos2(x + BAR_SWATCH + 5.0, y),
            egui::Align2::LEFT_CENTER,
            label,
            font.clone(),
            text_color,
        );
        widest = widest.max(r.width());
    }
    if !bar.title.is_empty() {
        let galley = painter.layout_no_wrap(bar.title.clone(), font, text_color);
        let size = galley.size();
        // Turned a quarter anticlockwise: it reads upwards, centred on the bar.
        let at = egui::pos2(x + BAR_SWATCH + 9.0 + widest, plot.center().y + size.x / 2.0);
        painter.add(egui::epaint::TextShape::new(at, galley, text_color).with_angle(-std::f32::consts::FRAC_PI_2));
    }
}

const BAR_GAP: f32 = 12.0;
const BAR_SWATCH: f32 = 14.0;

/// The contour part of a plot's Plot settings menu, editing the report's
/// `style` (`None` is the default style). `map` gives the surface's range
/// for the fixed-range fields.
pub fn settings(ui: &mut egui::Ui, style: &mut Option<ContourStyle>, map: &ContourMap) {
    let mut s = style.unwrap_or_default();
    ui.label(RichText::new("Contours").strong());
    ui.checkbox(&mut s.fill, "Fill between levels")
        .on_hover_text("Colour the bands between levels, with a colour bar beside the plot");
    ui.checkbox(&mut s.lines, "Level lines");
    ui.checkbox(&mut s.labels, "Level values").on_hover_text("Write each level's value on its longest line");
    ui.add_enabled_ui(s.fill, |ui| {
        ui.horizontal(|ui| {
            ui.label("Colours:");
            egui::ComboBox::from_id_salt("contour_scale").selected_text(s.scale.label()).show_list(ui, |ui| {
                for scale in ColorScale::ALL {
                    ui.selectable_value(&mut s.scale, scale, scale.label());
                }
            });
        });
        if s.scale == ColorScale::Spectrum {
            crate::dialog_kit::note(
                ui,
                "Spectrum is not colour-blind safe: keep the level lines and values on so the map reads without it.",
            );
        }
        ui.checkbox(&mut s.reverse, "Reverse colours");
        ui.horizontal(|ui| {
            ui.label("Opacity:");
            ui.add(egui::Slider::new(&mut s.opacity, 0.2..=1.0).fixed_decimals(2));
        });
    });
    ui.horizontal(|ui| {
        let mut fixed = s.levels.is_some();
        if ui.checkbox(&mut fixed, "Number of levels:").on_hover_text("About this many, at round values").changed() {
            s.levels = fixed.then_some(map.levels);
        }
        if let Some(n) = &mut s.levels {
            ui.add(egui::DragValue::new(n).range(1..=50));
        } else {
            ui.label(RichText::new(format!("automatic ({})", map.levels)).weak());
        }
    });
    let (lo, hi) = map.field.value_range().unwrap_or((0.0, 1.0));
    ui.horizontal(|ui| {
        let mut fixed = s.range.is_some();
        if ui
            .checkbox(&mut fixed, "Fixed range:")
            .on_hover_text("Levels and colours span this range instead of the surface's, so maps can be compared")
            .changed()
        {
            s.range = fixed.then_some((lo, hi));
        }
        if let Some((a, b)) = &mut s.range {
            let speed = ((hi - lo).abs() / 200.0).max(1e-9);
            ui.add(egui::DragValue::new(a).speed(speed)).on_hover_text("Low end");
            ui.label("to");
            ui.add(egui::DragValue::new(b).speed(speed)).on_hover_text("High end");
            if *b <= *a {
                *b = *a + speed;
            }
        } else {
            ui.label(RichText::new("the surface's").weak());
        }
    });
    if s.scale == ColorScale::Diverging && s.fill {
        ui.horizontal(|ui| {
            let mut set = s.centre.is_some();
            if ui
                .checkbox(&mut set, "Centre at:")
                .on_hover_text("The value shown in the neutral middle colour, such as a target")
                .changed()
            {
                s.centre = set.then_some(0.5 * (lo + hi));
            }
            if let Some(c) = &mut s.centre {
                ui.add(egui::DragValue::new(c).speed(((hi - lo).abs() / 200.0).max(1e-9)));
            } else {
                ui.label(RichText::new("the middle of the range").weak());
            }
        });
    }
    ui.horizontal(|ui| {
        if ui.button("Lines only").on_hover_text("No fill: level lines and values").clicked() {
            s = ContourStyle::lines_only();
        }
        if ui.button("Default").on_hover_text("Filled with the sequential colours, lines and values").clicked() {
            s = ContourStyle::default();
        }
    });
    *style = (s != ContourStyle::default()).then_some(s);
}
