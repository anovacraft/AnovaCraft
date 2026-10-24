//! The tree diagram of a decision-tree report: one box per node with the
//! rule leading to it, its count and prediction and (for a classification
//! tree) a bar of class shares. Clicking a node selects its training rows in
//! every linked view (shift or ⌘/Ctrl adds them to the selection); the
//! "−"/"+" handle under a split node hides or shows its branches.

use std::collections::BTreeSet;

use app_model::{SectionKind, TreeDiagram};
use eframe::egui::{self, Align2, CornerRadius, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, vec2};
use stat_data::{RowStateManager, SelectionMode};
use stat_viz::export::VectorFormat;

use crate::theme;

/// Width of one leaf slot, in screen points.
const SLOT: f32 = 172.0;
/// Height of one tree level.
const LEVEL: f32 = 92.0;
/// Node box size.
const BOX: egui::Vec2 = vec2(158.0, 60.0);
/// Radius of the hide/show handle.
const HANDLE: f32 = 7.0;

/// `text`, cut at the end with "…" so it fits in `width` points.
pub(crate) fn elide(ui: &egui::Ui, text: &str, font: &FontId, width: f32) -> String {
    let fits =
        |t: &str| ui.fonts(|f| f.layout_no_wrap(t.to_owned(), font.clone(), egui::Color32::WHITE).size().x) <= width;
    if fits(text) {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    let (mut lo, mut hi) = (0, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let candidate: String = chars[..mid].iter().collect::<String>().trim_end().to_owned() + "…";
        if fits(&candidate) { lo = mid } else { hi = mid - 1 }
    }
    chars[..lo].iter().collect::<String>().trim_end().to_owned() + "…"
}

/// Memory key of the set of nodes whose branches are hidden.
fn collapsed_id(report: u64, kind: SectionKind) -> egui::Id {
    egui::Id::new(("tree_collapsed", report, kind))
}

/// Nodes of the tree in report `report`'s section `kind` whose branches are
/// hidden on screen (figures follow the screen).
pub(crate) fn collapsed(ctx: &egui::Context, report: u64, kind: SectionKind) -> BTreeSet<usize> {
    ctx.data(|d| d.get_temp(collapsed_id(report, kind))).unwrap_or_default()
}

/// The tree as a figure, with the branches hidden on screen hidden.
pub(crate) fn figure(
    ctx: &egui::Context,
    d: &TreeDiagram,
    report: u64,
    kind: SectionKind,
    title: &str,
    format: VectorFormat,
) -> Vec<u8> {
    let hidden = collapsed(ctx, report, kind);
    app_model::report_figure::tree_figure(d, &|i| hidden.contains(&i), title, format)
}

/// Draw the tree diagram and handle clicks.
pub(crate) fn show(
    ui: &mut egui::Ui,
    d: &TreeDiagram,
    rows: &mut RowStateManager,
    report: u64,
    kind: SectionKind,
    title: &str,
) {
    let mut hidden = collapsed(ui.ctx(), report, kind);
    hidden.retain(|&i| i < d.nodes.len() && !d.nodes[i].children.is_empty());
    let (places, slots, deepest) = d.layout(&|i| hidden.contains(&i));
    let pal = theme::plot_palette(ui);
    let visuals = ui.visuals().clone();
    let text = visuals.text_color();
    let weak = visuals.weak_text_color();
    let ink = visuals.widgets.noninteractive.fg_stroke.color;
    let fill = visuals.extreme_bg_color;
    ui.horizontal_wrapped(|ui| {
        ui.label(
            egui::RichText::new(
                "Click a node to select its training rows (shift or ⌘/Ctrl adds) · − / + hides or shows a branch",
            )
            .small()
            .color(weak),
        );
        if !hidden.is_empty() && ui.small_button("Show all branches").clicked() {
            hidden.clear();
        }
    });
    let size = vec2(slots.max(1) as f32 * SLOT, (deepest + 1) as f32 * LEVEL);
    let mut toggled = None;
    let mut clicked_node = None;
    let response = egui::ScrollArea::horizontal()
        .id_salt(("tree_scroll", report, kind))
        .show(ui, |ui| {
            let (rect, response) = ui.allocate_exact_size(size, Sense::click());
            let painter = ui.painter_at(rect);
            let centre =
                |p: [f64; 2]| Pos2::new(rect.left() + p[0] as f32 * SLOT, rect.top() + (p[1] as f32 + 0.5) * LEVEL);
            let small = FontId::proportional(11.0 * theme::text_scale(ui));
            let strong = FontId::proportional(12.0 * theme::text_scale(ui));
            let pointer = response.hover_pos();
            let mut hovered = None;
            // Connectors first, behind the boxes.
            for (i, node) in d.nodes.iter().enumerate() {
                let Some(p) = places[i] else { continue };
                if hidden.contains(&i) {
                    continue;
                }
                let c = centre(p);
                for &child in &node.children {
                    let Some(q) = places[child] else { continue };
                    let q = centre(q);
                    let (top, bottom) = (c.y + BOX.y / 2.0, q.y - BOX.y / 2.0);
                    let mid = (top + bottom) / 2.0;
                    let stroke = Stroke::new(1.0_f32, ink);
                    painter.line(
                        vec![Pos2::new(c.x, top), Pos2::new(c.x, mid), Pos2::new(q.x, mid), Pos2::new(q.x, bottom)],
                        stroke,
                    );
                }
            }
            for (i, node) in d.nodes.iter().enumerate() {
                let Some(p) = places[i] else { continue };
                let c = centre(p);
                let r = Rect::from_center_size(c, BOX);
                let selected = node.rows.iter().filter(|&&row| row < rows.len() && rows.is_selected(row)).count();
                let over = pointer.is_some_and(|q| r.contains(q));
                if over {
                    hovered = Some(i);
                }
                let border = if selected > 0 {
                    Stroke::new(2.0_f32, pal.selected)
                } else if over {
                    Stroke::new(1.5_f32, text)
                } else {
                    Stroke::new(1.0_f32, ink)
                };
                painter.rect(r, CornerRadius::same(4), fill, border, StrokeKind::Inside);
                let clip = painter.with_clip_rect(r.shrink(3.0).intersect(rect));
                // Text wider than the box is shortened at its end (the full
                // text is in the tooltip), so it never loses its start.
                let room = r.width() - 8.0;
                let rule = elide(ui, &node.rule, &strong, room);
                clip.text(Pos2::new(c.x, r.top() + 5.0), Align2::CENTER_TOP, rule, strong.clone(), text);
                let summary = if selected > 0 {
                    format!("{} · {selected} selected", node.summary)
                } else {
                    node.summary.clone()
                };
                let summary = elide(ui, &summary, &small, room);
                clip.text(Pos2::new(c.x, r.top() + 22.0), Align2::CENTER_TOP, summary, small.clone(), weak);
                if node.shares.is_empty() {
                    if let Some(leaf) = node.leaf {
                        clip.text(
                            Pos2::new(c.x, r.bottom() - 5.0),
                            Align2::CENTER_BOTTOM,
                            format!("Leaf {leaf}"),
                            small.clone(),
                            weak,
                        );
                    }
                } else {
                    // Class shares: one segment per class, named in the key.
                    let bar = Rect::from_min_max(
                        Pos2::new(r.left() + 8.0, r.bottom() - 15.0),
                        Pos2::new(r.right() - 8.0, r.bottom() - 7.0),
                    );
                    let mut x = bar.left();
                    for (k, &share) in node.shares.iter().enumerate() {
                        let w = share as f32 * bar.width();
                        if w > 0.0 {
                            let seg = Rect::from_min_max(Pos2::new(x, bar.top()), Pos2::new(x + w, bar.bottom()));
                            painter.rect_filled(seg, CornerRadius::ZERO, theme::plot_palette(ui).group_style(k).0);
                        }
                        x += w;
                    }
                }
                // Hide/show handle under a split node.
                if !node.children.is_empty() {
                    let h = Pos2::new(c.x, r.bottom() + HANDLE + 2.0);
                    let near = pointer.is_some_and(|q| q.distance(h) <= HANDLE + 2.0);
                    painter.circle(h, HANDLE, fill, Stroke::new(if near { 1.5_f32 } else { 1.0 }, ink));
                    let sign = if hidden.contains(&i) { "+" } else { "−" };
                    painter.text(h, Align2::CENTER_CENTER, sign, small.clone(), text);
                    if near && response.clicked() {
                        toggled = Some(i);
                    }
                }
            }
            if toggled.is_none() && response.clicked() {
                clicked_node = hovered;
            }
            if let Some(i) = hovered {
                let node = &d.nodes[i];
                response.clone().on_hover_ui_at_pointer(|ui| {
                    ui.strong(&node.rule);
                    ui.label(&node.summary);
                    for (class, share) in d.classes.iter().zip(&node.shares) {
                        ui.label(format!("{class}: {:.1}%", share * 100.0));
                    }
                    ui.label(format!("{} training rows · click to select them", node.rows.len()));
                });
            }
            response
        })
        .inner;
    // Key for the class colours (each class also named in the hover text).
    if !d.classes.is_empty() {
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new("Class shares:").small().color(weak));
            for (k, class) in d.classes.iter().enumerate() {
                let (r, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
                ui.painter().rect_filled(
                    r.shrink(1.0),
                    CornerRadius::same(2),
                    theme::plot_palette(ui).group_style(k).0,
                );
                ui.label(egui::RichText::new(class).small());
            }
            ui.label(egui::RichText::new("(left to right in each bar)").small().color(weak));
        });
    }
    if let Some(i) = toggled {
        if !hidden.remove(&i) {
            hidden.insert(i);
        }
    }
    if let Some(i) = clicked_node {
        let mode = if ui.input(|inp| inp.modifiers.shift || inp.modifiers.command) {
            SelectionMode::Extend
        } else {
            SelectionMode::Replace
        };
        let n = rows.len();
        rows.select(d.nodes[i].rows.iter().copied().filter(|&r| r < n), mode);
    }
    ui.ctx().data_mut(|m| m.insert_temp(collapsed_id(report, kind), hidden));
    let ctx = ui.ctx().clone();
    response.context_menu(|ui| {
        crate::report_view::save_figure_items(
            ui,
            report,
            title,
            "The tree as shown, with hidden branches left out.",
            &|format| Some(figure(&ctx, d, report, kind, title, format)),
        );
    });
}
