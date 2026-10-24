//! Headless tests of the running application.
//!
//! Each test starts the whole app with `egui_kittest`, which drives egui
//! without a window or GPU: it clicks widgets found through the
//! accessibility tree, types text, and reads back what is on screen and the
//! app's own state.
//!
//! Set `ANOVACRAFT_GUI_SHOTS` to a folder to also save a PNG of the window
//! at each [`Gui::shot`] (rendered with wgpu, so it needs a graphics driver
//! or a software one such as Mesa's lavapipe).

mod accelerated;
mod add_several;
mod agreement;
mod annotations;
mod axis_look;
mod background;
mod bayes;
mod canvas_interaction;
mod charts;
mod crash_safety;
mod curve_templates;
mod dashboards;
mod data_table;
mod data_and_scripts;
mod design_tools;
mod detection_pk_fatigue;
mod dialog_kit;
mod dialogs;
mod documents;
mod editing_and_axes;
mod evaluation;
mod external;
mod factorial;
mod figure_page;
mod folders;
mod forecasting;
mod functional;
mod gauge;
mod graph_axes;
mod images;
mod latent;
mod manual;
mod manuals;
mod maps;
mod menu_layout;
mod monitoring;
mod model_comparisons;
mod mixed_extras;
mod multiple_imputation;
mod multiple_testing;
mod neural;
mod open_data;
mod option_additions;
mod plot_export;
mod plot_styles;
mod plot_annotations;
mod plots;
mod plots3d;
mod polish;
mod projects;
mod reference_lines;
mod reliability_extras;
mod repeated_nested;
mod report_scope;
mod reports;
mod resampling;
mod scripts;
mod screening_extras;
mod sequential;
mod signals;
mod signal;
mod image_reports;
mod sizing;
mod space_filling;
mod splines;
mod stepwise_several;
mod split_plot;
mod survival_models;
mod sweep;
mod tabulation;
mod text;
mod time_series;
mod tour;
mod treatment_effects;
mod viewer;
mod welcome;
mod windows;

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{Node, Queryable};

use super::*;

/// The running app under test.
struct Gui {
    harness: Harness<'static, StatsApp>,
}

impl Gui {
    /// The app as it starts with no files to open (on the start page), in a
    /// window of the default size.
    fn start() -> Self {
        let mut builder = Harness::builder().with_size([1360.0, 860.0]);
        if std::env::var_os(SHOTS).is_some() {
            builder = builder.wgpu();
        }
        let mut gui = Self { harness: builder.build_eframe(|cc| StatsApp::new(cc, Vec::new())) };
        gui.run();
        gui
    }

    /// The app with one new, empty data table open (it starts on the start page).
    fn blank() -> Self {
        let mut gui = Self::start();
        gui.perform(Action::NewTable);
        gui
    }

    /// The app with sample dataset `name` (the start of its catalog name) open.
    fn with_sample(name: &str) -> Self {
        let mut gui = Self::start();
        let n = app_model::sample::catalog()
            .iter()
            .position(|s| s.name.starts_with(name))
            .unwrap_or_else(|| panic!("no sample named {name}"));
        gui.perform(Action::OpenSample(n));
        gui
    }

    fn app(&self) -> &StatsApp {
        self.harness.state()
    }

    fn app_mut(&mut self) -> &mut StatsApp {
        self.harness.state_mut()
    }

    /// Run frames until the UI settles (bounded: some views keep animating).
    fn run(&mut self) {
        self.harness.run_steps(4);
    }

    /// Do what a menu item or toolbar button would ask for, then let the UI settle.
    fn perform(&mut self, action: Action) {
        let ctx = self.harness.ctx.clone();
        self.app_mut().perform(&ctx, action);
        self.run();
    }

    /// Apply edits to the active table, as the grid would.
    fn grid(&mut self, requests: Vec<GridRequest>) {
        self.perform(Action::Grid(requests));
    }

    /// The control (not plain text) labelled `label`; panics if there is
    /// none or more than one.
    fn widget<'a>(&'a self, label: &'a str) -> Node<'a> {
        self.harness.get_by(move |n| n.role() != Role::Label && n.label().as_deref() == Some(label))
    }

    /// Click the control labelled `label`, then let the UI settle.
    fn click(&mut self, label: &str) {
        self.widget(label).simulate_click();
        self.run();
    }

    /// Press the control labelled `label` through the accessibility tree
    /// (no pointer), for controls scrolled out of sight in a menu.
    fn press(&mut self, label: &str) {
        self.widget(label).click();
        self.run();
    }

    /// Choose `path` in the menu bar: the first entry opens a menu, later
    /// ones are submenus (opened by hovering) and finally the item.
    fn menu(&mut self, path: &[&str]) {
        let (item, menus) = path.split_last().expect("a menu path");
        for (i, menu) in menus.iter().enumerate() {
            if i == 0 {
                // The menu bar is drawn first, before any button of the same name.
                let is_menu = |n: &Node<'_>| n.role() == Role::Button && n.label().as_deref() == Some(*menu);
                self.harness.get_all_by(is_menu).next().expect("no such menu").simulate_click();
            } else {
                self.widget(menu).hover();
            }
            self.run();
        }
        self.click(item);
    }

    /// The window titled `title`.
    fn window<'a>(&'a self, title: &'a str) -> Node<'a> {
        self.harness.get_by_role_and_label(Role::Window, title)
    }

    /// Open the `n`th drop-down list (from 0) of window `window` and choose `option`.
    fn choose(&mut self, window: &str, n: usize, option: &str) {
        self.window(window).get_all_by_role(Role::ComboBox).nth(n).expect("no such drop-down list").simulate_click();
        self.run();
        // The list's entry, not a check box or plain button (such as Run) of
        // the same name in the dialog: entries are selectable, so toggled.
        let entry = |n: &Node<'_>| {
            !matches!(n.role(), Role::Label | Role::CheckBox)
                && !(n.role() == Role::Button && n.toggled().is_none())
                && n.label().as_deref() == Some(option)
        };
        self.harness.get_by(entry).simulate_click();
        self.run();
    }

    /// Set a number field (a drag value) to `value`, as a screen reader would.
    fn set_number(&mut self, field: egui::accesskit::NodeId, value: f64) {
        use egui::accesskit::{Action, ActionData, ActionRequest};
        let request =
            ActionRequest { action: Action::SetValue, target: field, data: Some(ActionData::NumericValue(value)) };
        self.harness.input_mut().events.push(egui::Event::AccessKitActionRequest(request));
        self.run();
    }

    /// Drag with the primary button from `from` to `to` (screen points), in steps.
    fn drag(&mut self, from: egui::Pos2, to: egui::Pos2) {
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        self.harness.input_mut().events.push(egui::Event::PointerMoved(from));
        self.harness.step();
        self.harness.input_mut().events.push(button(from, true));
        self.harness.step();
        for i in 1..=6 {
            let at = from.lerp(to, i as f32 / 6.0);
            self.harness.input_mut().events.push(egui::Event::PointerMoved(at));
            self.harness.step();
        }
        self.harness.input_mut().events.push(button(to, false));
        self.run();
    }

    /// Whether any widget's label is `label`.
    fn shows(&self, label: &str) -> bool {
        self.harness.query_all_by_label(label).next().is_some()
    }

    /// Every piece of text painted in the last frame, including text that
    /// is not a widget (plot labels, egui's own warnings).
    fn painted_text(&self) -> Vec<String> {
        fn collect(shape: &egui::Shape, out: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(t) => out.push(t.galley.text().to_owned()),
                egui::Shape::Vec(v) => v.iter().for_each(|s| collect(s, out)),
                _ => {}
            }
        }
        let mut out = Vec::new();
        for clipped in &self.harness.output().shapes {
            collect(&clipped.shape, &mut out);
        }
        out
    }

    /// Every piece of text painted in the last frame with where it was drawn
    /// and the clip rectangle it was drawn in (text turned on its side is
    /// left out).
    fn painted_text_rects(&self) -> Vec<(String, egui::Rect, egui::Rect)> {
        fn collect(shape: &egui::Shape, clip: egui::Rect, out: &mut Vec<(String, egui::Rect, egui::Rect)>) {
            match shape {
                egui::Shape::Text(t) if t.angle == 0.0 => {
                    // The ink, not the layout box (which can start at a row's indent).
                    let rect = t.galley.mesh_bounds.translate(t.pos.to_vec2());
                    out.push((t.galley.text().to_owned(), rect, clip));
                }
                egui::Shape::Vec(v) => v.iter().for_each(|s| collect(s, clip, out)),
                _ => {}
            }
        }
        let mut out = Vec::new();
        for clipped in &self.harness.output().shapes {
            collect(&clipped.shape, clipped.clip_rect, &mut out);
        }
        out
    }

    /// Show the results view of the active document.
    fn results(&mut self) {
        let active = self.app().active;
        self.app_mut().documents[active].workspace = Workspace::Results;
        self.run();
    }

    /// Save a picture of the window as `<ANOVACRAFT_GUI_SHOTS>/<name>.png`,
    /// when that variable is set.
    fn shot(&mut self, name: &str) {
        let Some(dir) = std::env::var_os(SHOTS) else { return };
        // A manual picture taken by this test (see `manual::TEST_SHOTS`):
        // only those when the manual's pictures are being taken, drawn at
        // the manual's window size in light colours.
        let manual = manual::TEST_SHOTS.iter().find(|t| t.1 == name).map(|t| t.0);
        let for_manual = std::env::var_os("MANUAL_TEST_SHOTS").is_some();
        if for_manual && manual.is_none() {
            return;
        }
        let size = self.harness.ctx.screen_rect().size();
        let name = match manual.filter(|_| for_manual) {
            Some(picture) => {
                self.app_mut().settings.tips_dismissed = true;
                apply_color_scheme(&self.harness.ctx.clone(), ColorScheme::Light);
                self.harness.set_size(egui::vec2(manual::SHOT_SIZE[0], manual::SHOT_SIZE[1]));
                self.harness.run_steps(6);
                picture
            }
            None => name,
        };
        let image = self.harness.render().unwrap_or_else(|e| panic!("could not render {name}: {e}"));
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).expect("screenshot folder");
        image.save(dir.join(format!("{name}.png"))).expect("screenshot written");
        if manual.is_some() && for_manual {
            self.harness.set_size(size);
            self.run();
        }
    }
}

/// Environment variable naming the folder for screenshots.
const SHOTS: &str = "ANOVACRAFT_GUI_SHOTS";

/// A new, empty folder for one test's files.
fn temp_dir(test: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("anovacraft-gui-{}-{test}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temporary folder");
    dir
}

#[test]
fn starts_on_the_start_page() {
    let mut gui = Gui::start();
    assert!(gui.app().documents.is_empty(), "no empty Untitled table");
    assert!(gui.shows("Describe"));
    assert!(gui.shows("New data table"));
    gui.shot("start");
}
