//! Renaming tables, projects and folders in place: in the sidebar's list of open
//! projects and in the tabs along the bottom edge, a double-click or
//! right-click ▸ Rename turns the name into a text box. Enter (or clicking
//! elsewhere) keeps the new name, Escape keeps the old one.

use eframe::egui;

use super::Action;

/// What is being renamed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum RenameTarget {
    /// The table of the document with this id (undoable like any edit).
    Table(u64),
    /// The project with this id.
    Project(u64),
    /// Folder `.1` of project `.0` (see [`super::folders`]).
    Folder(u64, u64),
}

/// Where the name is being edited: a table or project shows in both places,
/// and only the one that was double-clicked becomes a text box.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Place {
    /// The sidebar's list of open projects.
    Sidebar,
    /// The tabs along the bottom edge.
    Tabs,
}

type State = Option<(RenameTarget, Place, String, bool)>;

/// Where the name being edited is kept between frames: the target, the text
/// so far, and whether the box still has to take the keyboard focus.
fn state_id() -> egui::Id {
    egui::Id::new("rename_in_place")
}

/// Start renaming `target`, whose name is now `current`.
pub(super) fn start(ctx: &egui::Context, target: RenameTarget, place: Place, current: &str) {
    ctx.data_mut(|d| d.insert_temp::<State>(state_id(), Some((target, place, current.to_owned(), true))));
}

/// The text box for renaming `target`, `width` wide, when it is being
/// renamed; `false` (nothing drawn) otherwise. Enter or leaving the box asks
/// for the rename, Escape drops it.
pub(super) fn field(
    ui: &mut egui::Ui,
    target: RenameTarget,
    place: Place,
    width: f32,
    actions: &mut Vec<Action>,
) -> bool {
    let id = state_id();
    let Some((t, p, mut text, focus)) = ui.ctx().data(|d| d.get_temp::<State>(id)).flatten() else {
        return false;
    };
    if (t, p) != (target, place) {
        return false;
    }
    let response = ui.add(egui::TextEdit::singleline(&mut text).desired_width(width).id_salt(("rename", target)));
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "New name"));
    if focus {
        response.request_focus();
    }
    let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
    let finished = response.lost_focus() && !focus;
    if escape {
        ui.ctx().data_mut(|d| d.insert_temp::<State>(id, None));
    } else if finished {
        ui.ctx().data_mut(|d| d.insert_temp::<State>(id, None));
        actions.push(Action::Rename(target, text));
    } else {
        ui.ctx().data_mut(|d| d.insert_temp::<State>(id, Some((target, place, text, false))));
    }
    true
}

/// The Rename item of a right-click menu for `target` (named `current`).
pub(super) fn menu_item(ui: &mut egui::Ui, target: RenameTarget, place: Place, current: &str, what: &str) {
    if ui.button("Rename…").on_hover_text(format!("Give {what} another name (or double-click its name)")).clicked() {
        start(ui.ctx(), target, place, current);
        ui.close_menu();
    }
}
