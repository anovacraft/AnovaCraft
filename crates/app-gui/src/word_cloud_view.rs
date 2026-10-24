//! A word cloud section: the most frequent terms with type sized by their
//! counts, laid out by `app_model::word_cloud`. Hovering a word names its
//! count; clicking selects the documents containing it in every linked view
//! (shift or ⌘/Ctrl adds them). Words of documents already selected are
//! underlined in the selection colour, so the selection never rests on
//! colour alone.

use app_model::WordCloudSpec;
use app_model::report::ReportSection;
use eframe::egui::{self, Align2, FontFamily, FontId, Pos2, Sense, Stroke, vec2};
use stat_data::{RowStateManager, SelectionMode};

use crate::theme;

/// Draw the cloud, scaled to the available width (never above the text
/// scale), with its right-click menu of figure exports.
pub(crate) fn show(
    ui: &mut egui::Ui,
    w: &WordCloudSpec,
    rows: &mut RowStateManager,
    report: u64,
    section: &ReportSection,
) {
    if w.words.is_empty() {
        ui.label("No term to show.");
        return;
    }
    let pal = theme::plot_palette(ui);
    let text_scale = theme::text_scale(ui);
    let visuals = ui.visuals().clone();
    ui.label(
        egui::RichText::new("Click a word to select the documents containing it (shift or ⌘/Ctrl adds)")
            .small()
            .color(visuals.weak_text_color()),
    );
    let (ew, eh) = w.extent();
    let available = ui.available_width().max(200.0);
    let scale = (available / ew.max(1.0) as f32).min(1.3 * text_scale);
    let size = vec2(ew as f32 * scale, eh as f32 * scale);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let painter = ui.painter_at(rect);
    let origin = Pos2::new(rect.left() - w.bounds[0] as f32 * scale, rect.top() - w.bounds[1] as f32 * scale);
    let strong = w.words.len().div_ceil(10);
    let n = rows.len();
    let mut hovered = None;
    for (i, word) in w.words.iter().enumerate() {
        let center = origin + vec2(word.center[0] as f32, word.center[1] as f32) * scale;
        let font = FontId::new(word.size as f32 * scale, FontFamily::Proportional);
        let color = if i < strong { pal.fit_line } else { visuals.text_color() };
        let r = painter.text(center, Align2::CENTER_CENTER, &word.text, font, color);
        if word.rows.iter().any(|&row| row < n && rows.is_selected(row)) {
            let y = r.bottom() - 1.0;
            painter.line_segment(
                [Pos2::new(r.left(), y), Pos2::new(r.right(), y)],
                Stroke::new((1.5 * scale).max(1.5), pal.selected),
            );
        }
        if response.hover_pos().is_some_and(|p| r.expand(1.0).contains(p)) {
            hovered = Some((i, r));
        }
    }
    if let Some((i, r)) = hovered {
        painter.rect_stroke(r.expand(2.0), 3.0, Stroke::new(1.0_f32, pal.axis_label), egui::StrokeKind::Outside);
        let word = &w.words[i];
        let clicked = response.clicked();
        response.clone().on_hover_text(format!(
            "{}: {} {} in {} documents · click to select them",
            word.text,
            word.count,
            w.measure.to_lowercase(),
            word.rows.len()
        ));
        if clicked {
            let mode = if ui.input(|inp| inp.modifiers.shift || inp.modifiers.command) {
                SelectionMode::Extend
            } else {
                SelectionMode::Replace
            };
            rows.select(word.rows.iter().copied().filter(|&r| r < n), mode);
        }
    }
    response.context_menu(|ui| {
        crate::report_view::save_figure_items(ui, report, &section.title, "The word cloud as shown.", &|format| {
            app_model::report_figure::section_figure(section, None, format)
        });
    });
}
