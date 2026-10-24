//! The cause-and-effect (fishbone) diagram of a report, painted from the
//! shared layout ([`app_model::CauseEffectDiagram::layout`]) so the screen
//! and saved figures agree. Clicking a branch, cause or sub-cause selects
//! the rows behind it in every linked view (shift or ⌘/Ctrl adds them);
//! labels whose rows are selected get a tinted backing and a marker.

use app_model::{CauseEffectDiagram, SectionKind};
use eframe::egui::{self, CornerRadius, FontFamily, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, vec2};
use stat_data::{RowStateManager, SelectionMode};

use crate::theme;

/// Screen pixels per layout point.
const SCALE: f32 = 1.3;

/// Draw the diagram and handle clicks.
pub(crate) fn show(
    ui: &mut egui::Ui,
    d: &CauseEffectDiagram,
    rows: &mut RowStateManager,
    report: u64,
    kind: SectionKind,
    title: &str,
) {
    let layout = d.layout();
    let pal = theme::plot_palette(ui);
    let visuals = ui.visuals().clone();
    let text = visuals.text_color();
    let weak = visuals.weak_text_color();
    let ink = visuals.widgets.noninteractive.fg_stroke.color;
    let fill = visuals.extreme_bg_color;
    ui.label(
        egui::RichText::new("Click a branch, cause or sub-cause to select its rows (shift or ⌘/Ctrl adds)")
            .small()
            .color(weak),
    );
    let scale = SCALE * theme::text_scale(ui);
    let size = vec2(layout.width as f32 * scale, layout.height as f32 * scale);
    let mut clicked = None;
    let response = egui::ScrollArea::horizontal()
        .id_salt(("fishbone_scroll", report, kind))
        .show(ui, |ui| {
            let (rect, response) = ui.allocate_exact_size(size, Sense::click());
            let painter = ui.painter_at(rect);
            let at = |p: [f64; 2]| Pos2::new(rect.left() + p[0] as f32 * scale, rect.bottom() - p[1] as f32 * scale);
            for line in &layout.lines {
                let points: Vec<Pos2> = line.points.iter().map(|&p| at(p)).collect();
                painter.line(points, Stroke::new(line.width as f32 * scale * 0.8, ink));
            }
            painter.add(egui::Shape::convex_polygon(layout.arrow.iter().map(|&p| at(p)).collect(), ink, Stroke::NONE));
            let [x0, y0, x1, y1] = layout.head;
            let head = Rect::from_two_pos(at([x0, y0]), at([x1, y1]));
            painter.rect(head, CornerRadius::same(4), fill, Stroke::new(1.5_f32, ink), StrokeKind::Inside);
            let pointer = response.hover_pos();
            let mut hovered = None;
            for label in &layout.labels {
                let font = FontId::new(label.size as f32 * scale, FontFamily::Proportional);
                let color = if label.node.is_empty() || label.bold { text } else { text.gamma_multiply(0.9) };
                let galley = painter.layout_no_wrap(label.text.clone(), font, color);
                let gs = galley.size();
                let anchor = at(label.at);
                let min = Pos2::new(
                    anchor.x - label.align as f32 * gs.x,
                    // Layout y is up: valign 1 sits on the anchor (text above it).
                    anchor.y - label.valign as f32 * gs.y,
                );
                let r = Rect::from_min_size(min, gs);
                let node = d.node(&label.node);
                let selected =
                    node.is_some_and(|n| n.rows.iter().any(|&row| row < rows.len() && rows.is_selected(row)));
                let over = node.is_some() && pointer.is_some_and(|q| r.expand(2.0).contains(q));
                if selected {
                    painter.rect_filled(r.expand(2.0), CornerRadius::same(3), pal.selected_fill);
                    painter.circle_filled(Pos2::new(r.left() - 5.0, r.center().y), 2.5, pal.selected);
                }
                if over {
                    painter.rect_stroke(
                        r.expand(2.0),
                        CornerRadius::same(3),
                        Stroke::new(1.0_f32, text),
                        StrokeKind::Outside,
                    );
                    hovered = Some(label.node.clone());
                }
                if label.bold {
                    // Bold by drawing twice, offset by a hair (no bold face is bundled at every size).
                    painter.galley(min + vec2(0.4, 0.0), galley.clone(), color);
                }
                painter.galley(min, galley, color);
            }
            if response.clicked() {
                clicked = hovered.clone();
            }
            if let Some(path) = &hovered {
                if let Some(n) = d.node(path) {
                    let level = d.levels.get(path.len() - 1).map_or("", String::as_str);
                    response.clone().on_hover_ui_at_pointer(|ui| {
                        ui.strong(&n.label);
                        ui.label(format!("{level} · {} rows · click to select them", n.rows.len()));
                    });
                }
            }
            response
        })
        .inner;
    if let Some(path) = clicked {
        if let Some(n) = d.node(&path) {
            let mode = if ui.input(|i| i.modifiers.shift || i.modifiers.command) {
                SelectionMode::Extend
            } else {
                SelectionMode::Replace
            };
            let count = rows.len();
            rows.select(n.rows.iter().copied().filter(|&r| r < count), mode);
        }
    }
    response.context_menu(|ui| {
        crate::report_view::save_figure_items(ui, report, title, "The diagram as shown.", &|format| {
            Some(app_model::report_figure::fishbone_figure(d, title, format))
        });
    });
}
