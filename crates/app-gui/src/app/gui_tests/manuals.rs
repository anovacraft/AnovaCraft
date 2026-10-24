//! Help ▸ Quick guide and Help ▸ Reference manual open the PDFs built into
//! the app (the viewer launch is recorded instead of run in tests).

use super::super::manuals::{self, Manual, LAUNCHED};
use super::*;

/// The file the last click would have opened, checked to hold `manual`'s
/// embedded bytes in the manuals folder.
fn assert_opened(gui: &Gui, manual: Manual) {
    let path = LAUNCHED.with(|l| l.borrow().last().cloned()).expect("a manual was opened");
    assert_eq!(path.parent(), manuals::folder().as_deref(), "written to the manuals folder");
    assert!(path.file_name().unwrap().to_string_lossy().starts_with(manual.title()), "{path:?}");
    assert_eq!(std::fs::read(&path).unwrap(), manual.pdf(), "{path:?}");
    let message = gui.app().message.clone().map(|(m, _)| m).unwrap_or_default();
    assert!(message.contains(manual.title()), "{message}");
}

/// Both Help menu items write their PDF and open it; so do the start page's
/// buttons.
#[test]
fn help_opens_the_built_in_manuals() {
    let mut gui = Gui::with_sample("Longley");
    gui.menu(&["Help", "Quick guide"]);
    assert_opened(&gui, Manual::QuickGuide);
    gui.menu(&["Help", "Reference manual"]);
    assert_opened(&gui, Manual::Reference);

    let mut gui = Gui::start();
    gui.click("Quick guide");
    assert_opened(&gui, Manual::QuickGuide);
    gui.click("Reference manual");
    assert_opened(&gui, Manual::Reference);
}

/// Help ▸ About AnovaCraft names the app's licence and opens the embedded
/// third-party notices, which list the bundled fonts and the Rust crates.
#[test]
fn about_opens_the_open_source_licences() {
    let mut gui = Gui::start();
    gui.menu(&["Help", "About AnovaCraft"]);
    gui.run();
    let painted = gui.painted_text().join(" ");
    assert!(painted.contains("Apache License 2.0 or the MIT licence"), "About names the app's licence: {painted}");
    assert!(painted.contains("not affiliated with any commercial statistics package"), "independence notice: {painted}");
    gui.click("Open-source licences");
    assert!(gui.app().help_window == Some(HelpWindow::Licences));
    assert!(gui.painted_text().iter().any(|t| t.contains("AnovaCraft: third-party notices")));
    assert!(!gui.shows("About AnovaCraft"), "the notices replace the About window");
}
