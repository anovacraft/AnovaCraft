//! Exploratory sweeps over the whole app, run on demand:
//!
//! ```sh
//! ANOVACRAFT_GUI_SHOTS=gui-shots cargo test -p app-gui sweep -- --ignored --nocapture
//! ```
//!
//! They open every sample, report and menu item and fail on any panic,
//! saving pictures of each screen for a person to look through. Set
//! `ANOVACRAFT_SWEEP_SAMPLES` to words from sample names (comma-separated) to
//! sweep only the samples whose names contain one of them.

use std::panic::{AssertUnwindSafe, catch_unwind};

use super::*;

/// Run `f` for each item, collecting the ones that panicked.
fn each<T: std::fmt::Debug>(items: impl IntoIterator<Item = T>, mut f: impl FnMut(&T)) {
    let mut failed = Vec::new();
    for item in items {
        if catch_unwind(AssertUnwindSafe(|| f(&item))).is_err() {
            failed.push(format!("{item:?}"));
        }
    }
    assert!(failed.is_empty(), "panicked: {failed:#?}");
}

fn slug(text: &str) -> String {
    let s: String =
        text.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    s.split('-').filter(|p| !p.is_empty()).take(6).collect::<Vec<_>>().join("-")
}

/// Every sample with its reports, in the results view, light and dark.
#[test]
#[ignore = "exploratory sweep; run on demand"]
fn sweep_samples() {
    let names: Vec<&str> = app_model::sample::catalog().iter().map(|s| s.name).collect();
    let filter: Vec<String> = std::env::var("ANOVACRAFT_SWEEP_SAMPLES")
        .map(|f| f.split(',').map(|w| w.trim().to_lowercase()).filter(|w| !w.is_empty()).collect())
        .unwrap_or_default();
    let wanted = |name: &str| filter.is_empty() || filter.iter().any(|w| name.to_lowercase().contains(w));
    each(names.into_iter().enumerate().filter(|(_, name)| wanted(name)), |(n, name)| {
        let mut gui = Gui::start();
        gui.harness.set_size(egui::vec2(1500.0, 2400.0));
        gui.perform(Action::OpenSample(*n));
        let active = gui.app().active;
        gui.app_mut().documents[active].workspace = Workspace::Results;
        gui.run();
        let titles: Vec<String> = gui.app().documents[active].reports.iter().map(|r| r.title()).collect();
        let base = format!("sample-{n:02}-{}", slug(name));
        for dark in [false, true] {
            let scheme = if dark { ColorScheme::Dark } else { ColorScheme::Light };
            apply_color_scheme(&gui.harness.ctx.clone(), scheme);
            gui.run();
            // One report at a time, opened with all its sections.
            for (r, title) in titles.iter().enumerate() {
                gui.press("Collapse all");
                let header = gui
                    .harness
                    .query_all_by(|n| n.role() != Role::Label && n.label().as_deref() == Some(title.as_str()))
                    .next()
                    .map(|n| n.id());
                let Some(header) = header else {
                    println!("{base}: no header for report {title}");
                    continue;
                };
                gui.harness.get_by(|n| n.id() == header).click();
                gui.run();
                gui.shot(&format!("{base}-r{r}-{}", if dark { "dark" } else { "light" }));
            }
        }
    });
}

/// Labels of the controls (not plain text) on screen.
fn controls(gui: &Gui) -> Vec<String> {
    gui.harness
        .query_all_by(|n| n.role() != Role::Label && n.role() != Role::Window)
        .filter_map(|n| n.label())
        .filter(|l| !l.is_empty())
        .collect()
}

fn windows(gui: &Gui) -> Vec<String> {
    gui.harness.query_all_by_role(Role::Window).filter_map(|n| n.label()).collect()
}

/// Menu items that would open a native file dialog or leave the app.
fn unsafe_item(label: &str) -> bool {
    ["Open", "Save", "Export", "Import", "folder", "Quit", "pipeline", "clipboard", "Paste", "Copy", "Print", "Record"]
        .iter()
        .any(|w| label.contains(w))
}

const TOP_MENUS: [&str; 12] = [
    "File",
    "Edit",
    "Data",
    "Describe",
    "Compare",
    "Model",
    "Multivariate",
    "Design",
    "Quality",
    "Reliability",
    "Visualize",
    "Help",
];

const RUN_BUTTONS: [&str; 9] =
    ["Run", "Create", "Sort", "Remove duplicates", "Apply", "Apply anyway", "Recode", "Add single plan", "OK"];

/// A path through the menus to an item.
type MenuPath = Vec<String>;

/// Open the menus along `path` in `gui`; false if one is not there.
fn open_menus(gui: &mut Gui, path: &[String]) -> bool {
    for (i, menu) in path.iter().enumerate() {
        let is_item = |n: &Node<'_>| n.role() == Role::Button && n.label().as_deref() == Some(menu.as_str());
        // The menu bar comes first in the tree; popup menus come last.
        let node =
            if i == 0 { gui.harness.query_all_by(is_item).next() } else { gui.harness.query_all_by(is_item).last() };
        let Some(node) = node else { return false };
        if i == 0 {
            node.simulate_click()
        } else {
            node.hover()
        }
        gui.run();
    }
    true
}

/// Every menu item reachable from the menu bar (two levels of submenus).
fn menu_items(sample: usize) -> Vec<MenuPath> {
    let fresh = || {
        let mut gui = Gui::start();
        gui.perform(Action::OpenSample(sample));
        gui
    };
    let mut items = Vec::new();
    let mut queue: Vec<MenuPath> = TOP_MENUS.iter().map(|m| vec![(*m).to_owned()]).collect();
    while let Some(path) = queue.pop() {
        // What shows with the parent menus open, then with this entry opened too.
        let mut parent = fresh();
        if !open_menus(&mut parent, &path[..path.len() - 1]) {
            continue;
        }
        let parent_before: std::collections::HashSet<String> = controls(&parent).into_iter().collect();
        drop(parent);
        let mut gui = fresh();
        if !open_menus(&mut gui, &path) {
            continue;
        }
        let new: Vec<String> = controls(&gui).into_iter().filter(|l| !parent_before.contains(l)).collect();
        if path.len() == 1 || (!new.is_empty() && path.len() < 4) {
            for label in new {
                if unsafe_item(&label) {
                    continue;
                }
                let mut p = path.clone();
                p.push(label);
                // Hover reveals a submenu; an item is clicked later.
                queue.push(p);
            }
            if path.len() > 1 {
                items.push(path);
            }
        } else {
            items.push(path);
        }
    }
    items.sort();
    items.dedup();
    items
}

/// Choose option `pick` (modulo their number) in each drop-down list of `window`.
fn fill_window(gui: &mut Gui, window: &str, seed: usize) {
    let combos = gui
        .harness
        .query_all_by(|n| n.role() == Role::Window && n.label().as_deref() == Some(window))
        .next()
        .map_or(0, |w| w.query_all_by_role(Role::ComboBox).count());
    for c in 0..combos {
        let Some(w) =
            gui.harness.query_all_by(|n| n.role() == Role::Window && n.label().as_deref() == Some(window)).next()
        else {
            return;
        };
        let Some(combo) = w.query_all_by_role(Role::ComboBox).nth(c) else { return };
        let before: std::collections::HashSet<String> = controls(gui).into_iter().collect();
        combo.click();
        gui.run();
        let options: Vec<String> = controls(gui).into_iter().filter(|l| !before.contains(l)).collect();
        if options.is_empty() {
            gui.harness.press_key(egui::Key::Escape);
            gui.run();
            continue;
        }
        let choice = &options[(seed + c) % options.len()];
        if let Some(n) = gui
            .harness
            .query_all_by(|n| n.role() != Role::Label && n.label().as_deref() == Some(choice.as_str()))
            .last()
        {
            n.click();
        }
        gui.run();
        gui.harness.press_key(egui::Key::Escape);
        gui.run();
    }
    // Tick a few check boxes in the window.
    let boxes: Vec<_> = gui
        .harness
        .query_all_by(|n| n.role() == Role::Window && n.label().as_deref() == Some(window))
        .next()
        .map(|w| w.query_all_by_role(Role::CheckBox).map(|n| n.id()).collect())
        .unwrap_or_default();
    for (i, id) in boxes.into_iter().enumerate() {
        if (i + seed) % 3 == 0 {
            if let Some(n) = gui.harness.query_by(|n| n.id() == id) {
                n.click();
            }
            gui.run();
        }
    }
}

/// Click the window's run button, if one is enabled.
fn run_window(gui: &mut Gui, window: &str) -> bool {
    let Some(w) = gui.harness.query_all_by(|n| n.role() == Role::Window && n.label().as_deref() == Some(window)).next()
    else {
        return false;
    };
    let button = w
        .query_all_by(|n| {
            n.role() == Role::Button && !n.is_disabled() && n.label().is_some_and(|l| RUN_BUTTONS.contains(&l.as_str()))
        })
        .next();
    match button {
        Some(b) => {
            b.click();
            gui.run();
            gui.run();
            true
        }
        None => false,
    }
}

/// Reports any case slower than a few seconds when it finishes (a freeze in
/// the running app).
struct Slow(std::time::Instant, String);

impl Drop for Slow {
    fn drop(&mut self) {
        if self.0.elapsed().as_secs() >= 4 {
            println!("slow: {} took {:?}", self.1, self.0.elapsed());
        }
    }
}

/// Every menu item on several samples: open it, fill its dialog a few ways and run it.
#[test]
#[ignore = "exploratory sweep; run on demand"]
fn sweep_menus() {
    let catalog = app_model::sample::catalog();
    let samples: Vec<usize> =
        ["Longley", "Plant growth", "Customer churn", "Fill weights", "Bearing life", "Gauge study"]
            .iter()
            .map(|name| catalog.iter().position(|s| s.name.starts_with(name)).expect("sample"))
            .collect();
    let items = menu_items(samples[0]);
    println!("{} menu items", items.len());
    let mut ran = 0;
    let cases: Vec<(usize, MenuPath, usize)> = samples
        .iter()
        .flat_map(|&s| items.iter().flat_map(move |p| (0..3).map(move |seed| (s, p.clone(), seed))))
        .collect();
    each(cases, |(sample, path, seed)| {
        let started = std::time::Instant::now();
        let _slow = Slow(started, format!("{sample} {path:?} seed {seed}"));
        let mut gui = Gui::start();
        gui.perform(Action::OpenSample(*sample));
        let before = windows(&gui);
        if !open_menus(&mut gui, &path[..path.len() - 1]) {
            println!("menu path not found: {path:?}");
            return;
        }
        let item = path.last().unwrap();
        let Some(node) =
            gui.harness.query_all_by(|n| n.role() != Role::Label && n.label().as_deref() == Some(item.as_str())).last()
        else {
            println!("menu item not found: {path:?}");
            return;
        };
        node.click();
        gui.run();
        let new: Vec<String> = windows(&gui).into_iter().filter(|w| !before.contains(w)).collect();
        for window in &new {
            if *seed == 0 && *sample == samples[0] {
                gui.shot(&format!("menu-{}", slug(&path.join(" "))));
            }
            fill_window(&mut gui, window, *seed);
            if run_window(&mut gui, window) {
                ran += 1;
                if *seed == 1 {
                    gui.shot(&format!("ran-{}-s{sample}", slug(&path.join(" "))));
                }
            }
        }
    });
    println!("ran {ran} dialogs");
}

/// Every menu item's dialog as it first opens, on one sample (no filling or
/// running), saved for a person to look through:
///
/// ```sh
/// ANOVACRAFT_GUI_SHOTS=gui-shots cargo test -p app-gui sweep_dialogs_first_open -- --ignored
/// ```
#[test]
#[ignore = "exploratory sweep; run on demand"]
fn sweep_dialogs_first_open() {
    let sample = std::env::var("ANOVACRAFT_SWEEP_SAMPLES").unwrap_or_else(|_| "Longley".into());
    let catalog = app_model::sample::catalog();
    let n = catalog.iter().position(|s| s.name.starts_with(sample.as_str())).expect("sample");
    let items = menu_items(n);
    let mut base = Gui::start();
    base.perform(Action::OpenSample(n));
    // The window without a dialog, to compare the others with.
    base.shot("first-open-baseline");
    each(items, |path| {
        let mut gui = Gui::start();
        gui.perform(Action::OpenSample(n));
        let before = windows(&gui);
        if !open_menus(&mut gui, &path[..path.len() - 1]) {
            return;
        }
        let item = path.last().unwrap();
        let Some(node) =
            gui.harness.query_all_by(|n| n.role() != Role::Label && n.label().as_deref() == Some(item.as_str())).last()
        else {
            return;
        };
        node.click();
        gui.run();
        if windows(&gui).iter().any(|w| !before.contains(w)) {
            gui.shot(&format!("first-open-{}", slug(&path.join(" "))));
        }
    });
}
