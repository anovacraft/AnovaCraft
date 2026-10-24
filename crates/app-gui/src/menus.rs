//! Drop-down menu buttons that toggle: clicking the button of an open menu
//! closes it, without the menu flashing back for a frame.
//!
//! egui's own menu button closes the menu when a press lands outside the
//! menu's area (the button is outside it) and then, seeing a click on the
//! button of a closed menu, reopens it on the release. This wrapper remembers
//! the open menu when a press on its button begins, and puts it back just before
//! the release is handled, so egui sees "button of the open menu clicked" and
//! closes it, never drawing it again.

use eframe::egui::{Id, InnerResponse, Rect, Ui, WidgetText, menu::BarState};

/// A top-level drop-down menu (in the menu bar or a toolbar row). Use egui's
/// `menu_button` for submenus inside another menu.
pub fn menu_button<R>(
    ui: &mut Ui,
    title: impl Into<WidgetText>,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> InnerResponse<Option<R>> {
    let ctx = ui.ctx().clone();
    let title: WidgetText = title.into();
    // egui keeps the open menu of a row of menu buttons under the row's id.
    let bar_id = ui.id();
    let key = bar_id.with(("toggle_menu", title.text()));
    let before = BarState::load(&ctx, bar_id);

    // Releasing a press that began on the button while its menu was open:
    // restore that open state so egui treats the click as "close".
    let saved = ctx.data(|d| d.get_temp::<(Id, Rect, BarState)>(key));
    if let Some((_, rect, state)) = &saved {
        let released_on_button =
            ctx.input(|i| i.pointer.any_released() && i.pointer.interact_pos().is_some_and(|p| rect.contains(p)));
        if released_on_button {
            state.clone().store(&ctx, bar_id);
        }
    }

    let result = ui.menu_button(title, add_contents);
    let button = &result.response;

    let (pressed_here, down) = ctx.input(|i| {
        let here =
            i.pointer.any_pressed() && i.pointer.press_origin().is_some_and(|p| button.interact_rect.contains(p));
        (here, i.pointer.any_down())
    });
    let open_before = (**before).as_ref().is_some_and(|m| m.id == button.id);
    if pressed_here && down && open_before {
        ctx.data_mut(|d| d.insert_temp(key, (button.id, button.interact_rect, before)));
    } else if !down {
        ctx.data_mut(|d| d.remove::<(Id, Rect, BarState)>(key));
        // Safety net: if the menu is somehow open again after closing it with
        // its own button, close it.
        if button.clicked() && saved.is_some_and(|s| s.0 == button.id) {
            let mut state = BarState::load(&ctx, bar_id);
            if (**state).as_ref().is_some_and(|m| m.id == button.id) {
                **state = None;
                state.store(&ctx, bar_id);
                ctx.request_repaint();
            }
        }
    }
    result
}
