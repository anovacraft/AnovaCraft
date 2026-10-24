//! Drop zones drawn over the graph's panels while a column is dragged:
//! along the bottom edge for the horizontal axis, the left edge for the
//! vertical axis, the top edge to split across, the right edge to split
//! down, and the centre for colour. The zone under the pointer is
//! highlighted and named; letting go there assigns the column. The shelves
//! stay the precise (and keyboard-reachable) way to do the same.

use eframe::egui;
use stat_viz::{EncodingPlan, Role};

use super::ColumnDrag;
use super::shelves::shelf;
use crate::theme;

/// The role of each zone, in drawing order.
const ZONES: [Role; 5] = [Role::X, Role::Y, Role::GroupX, Role::GroupY, Role::Color];

/// Depth of the edge zones: a fifth of the panels' shorter side, kept
/// between 28 and 72 points.
fn band(rect: egui::Rect) -> f32 {
    (rect.width().min(rect.height()) * 0.2).clamp(28.0, 72.0)
}

/// The zone of `role` within the panels' `rect`. The left and right zones
/// run between the top and bottom ones; the centre is what is left.
pub(super) fn zone_rect(rect: egui::Rect, role: Role) -> egui::Rect {
    let t = band(rect);
    let (l, r, top, b) = (rect.left(), rect.right(), rect.top(), rect.bottom());
    match role {
        Role::X => egui::Rect::from_min_max(egui::pos2(l, b - t), egui::pos2(r, b)),
        Role::GroupX => egui::Rect::from_min_max(egui::pos2(l, top), egui::pos2(r, top + t)),
        Role::Y => egui::Rect::from_min_max(egui::pos2(l, top + t), egui::pos2(l + t, b - t)),
        Role::GroupY => egui::Rect::from_min_max(egui::pos2(r - t, top + t), egui::pos2(r, b - t)),
        _ => egui::Rect::from_min_max(egui::pos2(l + t, top + t), egui::pos2(r - t, b - t)),
    }
}

/// The zone under `pos`, if it is over the panels.
pub(super) fn zone_at(rect: egui::Rect, pos: egui::Pos2) -> Option<Role> {
    if !rect.contains(pos) {
        return None;
    }
    ZONES.into_iter().find(|&role| zone_rect(rect, role).contains(pos))
}

/// Put a column dropped on the zone of `role` into the plan: on an axis it
/// takes the first column's place (or is added when the axis is empty), so
/// the graph shows it at once; on the other zones it is assigned as from
/// the shelf. A column dragged from a shelf leaves that shelf (a move); one
/// from the variables list stays wherever else it is used.
pub(super) fn apply_drop(plan: &mut EncodingPlan, role: Role, drag: &ColumnDrag) {
    if plan.get(role).first() == Some(&drag.name.as_str()) {
        return;
    }
    if let Some(from) = drag.from.filter(|&f| f != role) {
        plan.unassign(from, &drag.name);
    }
    if matches!(role, Role::X | Role::Y) && !plan.get(role).is_empty() {
        // Already further along this axis: it takes the first place.
        plan.unassign(role, &drag.name);
        if plan.get(role).is_empty() {
            plan.assign(role, &drag.name);
        } else {
            plan.replace(role, 0, &drag.name);
        }
    } else {
        plan.assign(role, &drag.name);
    }
}

/// Draw the zones over `rect` while a column is dragged, and return the
/// role and column when it is let go over one. Nothing is drawn while no
/// column is being dragged.
pub(super) fn drop_zones(ui: &egui::Ui, rect: egui::Rect, id: u64) -> Option<(Role, ColumnDrag)> {
    let drag = egui::DragAndDrop::payload::<ColumnDrag>(ui.ctx())?;
    let pos = ui.ctx().pointer_latest_pos().filter(|p| ui.clip_rect().contains(*p));
    let hovered = pos.and_then(|p| zone_at(rect, p));
    let painter = ui
        .ctx()
        .layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new(("graph_drop_zones", id))))
        .with_clip_rect(ui.clip_rect());
    let accent = theme::DIVIDER_ACTIVE;
    let text_color = ui.visuals().strong_text_color();
    let backdrop = ui.visuals().panel_fill.gamma_multiply(0.9);
    for role in ZONES {
        let zone = zone_rect(rect, role).shrink(2.0);
        let on = hovered == Some(role);
        let (fill, width) =
            if on { (accent.gamma_multiply(0.30), 2.0_f32) } else { (accent.gamma_multiply(0.08), 1.0_f32) };
        painter.rect(zone, 4.0, fill, egui::Stroke::new(width, accent), egui::StrokeKind::Inside);
        let (label, _) = shelf(role);
        let text = if on {
            match role {
                Role::X | Role::Y => format!("{label}: {}", drag.name),
                _ => format!("{label} by {}", drag.name),
            }
        } else {
            label.to_owned()
        };
        let font = egui::FontId::proportional(if on { 13.0 } else { 11.0 });
        let galley = painter.layout_no_wrap(text, font, text_color);
        // Side zones are narrow: their names sit at the zone's centre all
        // the same, on a backdrop so they read over the data.
        let at = zone.center() - galley.size() / 2.0;
        let back = egui::Rect::from_min_size(at, galley.size()).expand2(egui::vec2(4.0, 2.0));
        painter.rect_filled(back, 3.0, backdrop);
        painter.galley(at, galley, text_color);
    }
    let released = ui.input(|i| i.pointer.any_released());
    if released {
        if let Some(role) = hovered {
            let drag = egui::DragAndDrop::take_payload::<ColumnDrag>(ui.ctx())?;
            return Some((role, (*drag).clone()));
        }
    }
    ui.ctx().request_repaint();
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect() -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(600.0, 400.0))
    }

    #[test]
    fn zones_tile_the_panels() {
        let r = rect();
        assert_eq!(zone_at(r, egui::pos2(300.0, 395.0)), Some(Role::X));
        assert_eq!(zone_at(r, egui::pos2(5.0, 200.0)), Some(Role::Y));
        assert_eq!(zone_at(r, egui::pos2(300.0, 5.0)), Some(Role::GroupX));
        assert_eq!(zone_at(r, egui::pos2(595.0, 200.0)), Some(Role::GroupY));
        assert_eq!(zone_at(r, egui::pos2(300.0, 200.0)), Some(Role::Color));
        // Corners belong to the top and bottom zones.
        assert_eq!(zone_at(r, egui::pos2(2.0, 398.0)), Some(Role::X));
        assert_eq!(zone_at(r, egui::pos2(598.0, 2.0)), Some(Role::GroupX));
        assert_eq!(zone_at(r, egui::pos2(700.0, 200.0)), None);
        // Bands keep a usable depth on small and large panels alike.
        assert_eq!(band(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(100.0, 100.0))), 28.0);
        assert_eq!(band(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(2000.0, 1000.0))), 72.0);
    }

    #[test]
    fn drops_replace_an_axis_and_move_from_shelves() {
        let drag = |name: &str, from: Option<Role>| ColumnDrag { name: name.into(), from };
        let mut plan = EncodingPlan { x: vec!["a".into(), "b".into()], y: vec!["c".into()], ..EncodingPlan::default() };
        // From the variables list: replaces the first horizontal column.
        apply_drop(&mut plan, Role::X, &drag("d", None));
        assert_eq!(plan.x, ["d", "b"]);
        // Already along the axis: it takes the first place.
        apply_drop(&mut plan, Role::X, &drag("b", None));
        assert_eq!(plan.x, ["b"]);
        // From a shelf: moved (the vertical axis loses it).
        apply_drop(&mut plan, Role::X, &drag("c", Some(Role::Y)));
        assert_eq!(plan.x, ["c"]);
        assert!(plan.y.is_empty());
        // An empty axis takes the column.
        apply_drop(&mut plan, Role::Y, &drag("a", None));
        assert_eq!(plan.y, ["a"]);
        // The other zones assign as their shelves do.
        apply_drop(&mut plan, Role::Color, &drag("g", None));
        apply_drop(&mut plan, Role::GroupX, &drag("h", None));
        apply_drop(&mut plan, Role::GroupY, &drag("k", None));
        assert_eq!(plan.color.as_deref(), Some("g"));
        assert_eq!(plan.group_x.as_deref(), Some("h"));
        assert_eq!(plan.group_y.as_deref(), Some("k"));
        // Dropping the column already first is no change.
        let before = plan.clone();
        apply_drop(&mut plan, Role::Y, &drag("a", None));
        assert_eq!(plan, before);
    }
}
