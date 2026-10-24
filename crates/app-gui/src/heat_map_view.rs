//! A heat map section (for example a correlation matrix), painted as a grid
//! of cells. Each cell's fill comes from the palette's diverging ramp (hue
//! for the sign, contrast with the neutral for the size), and its value is
//! printed in it, so the colour is never the only cue. A key under the grid
//! shows the ramp with its end values; hovering a cell names its row and
//! column.

use app_model::HeatMapSpec;
use eframe::egui::{self, Align2, FontFamily, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, vec2};

use crate::theme;

/// Side of a cell in points at text scale 1.
const CELL: f32 = 46.0;
/// Width of the key's ramp.
const KEY_WIDTH: f32 = 180.0;

/// Draw the heat map.
pub(crate) fn show(ui: &mut egui::Ui, h: &HeatMapSpec) {
    let pal = theme::plot_palette(ui);
    let scale = theme::text_scale(ui);
    let text_color = ui.visuals().text_color();
    let label_font = FontId::new(12.0 * scale, FontFamily::Proportional);
    let number_font = FontId::new(11.5 * scale, FontFamily::Monospace);
    let cell = CELL * scale;
    let galleys = |labels: &[String]| -> Vec<std::sync::Arc<egui::Galley>> {
        labels.iter().map(|l| ui.painter().layout_no_wrap(l.clone(), label_font.clone(), text_color)).collect()
    };
    let row_labels = galleys(&h.rows);
    let col_labels = galleys(&h.columns);
    let label_w = row_labels.iter().map(|g| g.size().x).fold(0.0_f32, f32::max) + 8.0;
    let header_h = col_labels.iter().map(|g| g.size().x).fold(0.0_f32, f32::max) + 8.0;
    let key_h = 44.0 * scale;
    let grid_w = cell * h.columns.len() as f32;
    let grid_h = cell * h.rows.len() as f32;
    // The key's caption runs to the right of its ramp, and its end values are
    // centred under the ramp's ends: leave room for both.
    let small = FontId::new(10.5 * scale, FontFamily::Proportional);
    let limit = app_model::format::format_number(h.limit, 3);
    let caption = ui.painter().layout_no_wrap(
        format!("{} (value printed in each cell)", h.measure),
        small.clone(),
        pal.tick_text,
    );
    let end_w = ui.painter().layout_no_wrap(format!("−{limit}"), small.clone(), pal.tick_text).size().x;
    let key_left = 4.0 + end_w / 2.0;
    let key_w = key_left + KEY_WIDTH * scale + 10.0 + caption.size().x + 4.0;
    let size = vec2((label_w + grid_w).max(key_w), header_h + grid_h + key_h);
    egui::ScrollArea::horizontal().id_salt(("heat_map", h.rows.len(), &h.measure, &h.columns)).show(ui, |ui| {
        let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
        let painter = ui.painter_at(rect);
        let origin = Pos2::new(rect.left() + label_w, rect.top() + header_h);
        // Column labels, turned to read upwards.
        for (j, g) in col_labels.iter().enumerate() {
            let x = origin.x + (j as f32 + 0.5) * cell - g.size().y / 2.0;
            let shape = egui::epaint::TextShape::new(Pos2::new(x, origin.y - 4.0), g.clone(), text_color)
                .with_angle(-std::f32::consts::FRAC_PI_2);
            painter.add(shape);
        }
        let mut hovered = None;
        for (i, (row, g)) in h.values.iter().zip(&row_labels).enumerate() {
            let y = origin.y + i as f32 * cell;
            painter.galley(
                Pos2::new(origin.x - 6.0 - g.size().x, y + (cell - g.size().y) / 2.0),
                g.clone(),
                text_color,
            );
            for (j, &v) in row.iter().enumerate() {
                let r = Rect::from_min_size(Pos2::new(origin.x + j as f32 * cell, y), vec2(cell, cell));
                let fill = pal.diverging(h.ramp_position(v));
                painter.rect_filled(r.shrink(1.0), 2.0, fill);
                let text = h.text.get(i).and_then(|t| t.get(j)).map_or("", String::as_str);
                if !text.is_empty() {
                    painter.text(r.center(), Align2::CENTER_CENTER, text, number_font.clone(), pal.ink_on(fill));
                }
                if response.hover_pos().is_some_and(|p| r.contains(p)) {
                    hovered = Some((i, j, r));
                }
            }
        }
        if let Some((i, j, r)) = hovered {
            painter.rect_stroke(r.shrink(1.0), 2.0, Stroke::new(2.0_f32, pal.axis_label), StrokeKind::Inside);
            let text = h.text.get(i).and_then(|t| t.get(j)).cloned().unwrap_or_default();
            response.on_hover_text(format!("{} × {}: {} = {text}", h.rows[i], h.columns[j], h.measure));
        }
        // The key: the ramp from −limit to +limit with its end and middle values.
        let key_top = origin.y + grid_h + 12.0 * scale;
        let key =
            Rect::from_min_size(Pos2::new(rect.left() + key_left, key_top), vec2(KEY_WIDTH * scale, 10.0 * scale));
        let steps = 48;
        for k in 0..steps {
            let t0 = k as f32 / steps as f32;
            let x0 = key.left() + t0 * key.width();
            let piece = Rect::from_min_max(
                Pos2::new(x0, key.top()),
                Pos2::new(x0 + key.width() / steps as f32 + 0.5, key.bottom()),
            );
            painter.rect_filled(piece, 0.0, pal.diverging(2.0 * f64::from(t0) - 1.0 + 1.0 / steps as f64));
        }
        painter.rect_stroke(key, 0.0, Stroke::new(1.0_f32, pal.container_border), StrokeKind::Outside);
        for (x, label) in
            [(key.left(), format!("−{limit}")), (key.center().x, "0".to_owned()), (key.right(), format!("+{limit}"))]
        {
            painter.text(Pos2::new(x, key.bottom() + 2.0), Align2::CENTER_TOP, label, small.clone(), pal.tick_text);
        }
        painter.galley(Pos2::new(key.right() + 10.0, key.center().y - caption.size().y / 2.0), caption, pal.tick_text);
    });
}
