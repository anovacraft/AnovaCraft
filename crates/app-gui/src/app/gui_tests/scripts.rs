//! The Script window.

use super::*;

/// Ask the app to quit, as the window's close button would.
fn request_quit(gui: &mut Gui) {
    let input = gui.harness.input_mut();
    input.viewports.entry(egui::ViewportId::ROOT).or_default().events.push(egui::ViewportEvent::Close);
    gui.run();
}

/// Closing the Script window while it asks about unsaved changes counts as
/// Cancel; the unanswered question used to stop quitting from going on.
#[test]
fn closing_the_script_window_mid_question_does_not_block_quitting() {
    let mut gui = Gui::start();
    gui.menu(&["Scripts", "Script window"]);
    gui.harness.get_by_role(Role::MultilineTextInput).type_text("print(1);");
    gui.run();
    gui.click("New");
    assert!(gui.shows("Save the changes to this script before starting a new script?"));
    gui.click("Close window");
    request_quit(&mut gui);
    assert!(!gui.app().quit_confirmed, "the unsaved script is asked about first");
    assert!(gui.shows("Save the changes to this script before quitting?"));
    gui.click("Don't Save");
    assert!(gui.app().quit_confirmed);
}
