//! A tour of the most used screens, saved as numbered pictures for design
//! reviews and user feedback, run on demand:
//!
//! ```sh
//! ANOVACRAFT_GUI_SHOTS=feedback-shots cargo test -p app-gui feedback_tour -- --ignored --nocapture
//! ```
//!
//! Each step starts the app afresh with a sample dataset; a step that cannot
//! find its control is reported and skipped rather than failing the tour.

use std::panic::{AssertUnwindSafe, catch_unwind};

use super::*;

/// A fresh app on sample `name` with the getting-started tips hidden.
fn on(name: &str) -> Gui {
    let mut gui = Gui::with_sample(name);
    gui.app_mut().settings.tips_dismissed = true;
    apply_color_scheme(&gui.harness.ctx.clone(), ColorScheme::Light);
    gui.run();
    gui
}

/// Show the data grid of the active document.
fn data(gui: &mut Gui) {
    let active = gui.app().active;
    gui.app_mut().documents[active].workspace = Workspace::Data;
    gui.run();
}

/// Close the active document's reports.
fn clear_reports(gui: &mut Gui) {
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui.run();
}

/// Results view with every report closed except the `n`th (from 0), opened
/// with all its sections.
fn report(gui: &mut Gui, n: usize) {
    gui.results();
    let active = gui.app().active;
    let Some(title) = gui.app().documents[active].reports.get(n).map(|r| r.title()) else { return };
    gui.press("Collapse all");
    let header = gui
        .harness
        .query_all_by(|node| node.role() != Role::Label && node.label().as_deref() == Some(title.as_str()))
        .next()
        .map(|node| node.id());
    if let Some(header) = header {
        gui.harness.get_by(|node| node.id() == header).click();
        gui.run();
    }
}

/// Open the menus along `path` (top menu, then submenus by hovering) and leave them open.
fn open_menu(gui: &mut Gui, path: &[&str]) {
    for (i, menu) in path.iter().enumerate() {
        let is_item = |n: &Node<'_>| n.role() == Role::Button && n.label().as_deref() == Some(*menu);
        let node = if i == 0 { gui.harness.get_all_by(is_item).next() } else { gui.harness.get_all_by(is_item).last() };
        let node = node.unwrap_or_else(|| panic!("no menu {menu}"));
        if i == 0 {
            node.simulate_click()
        } else {
            node.hover()
        }
        gui.run();
    }
}

/// Make the window `h` points tall (reports are long).
fn size(gui: &mut Gui, h: f32) {
    gui.harness.set_size(egui::vec2(1360.0, h));
    gui.run();
}

fn dark(gui: &mut Gui) {
    apply_color_scheme(&gui.harness.ctx.clone(), ColorScheme::Dark);
    gui.run();
}

/// The screens a typical session passes through, in the order a new user meets them.
#[test]
#[ignore = "picture tour for feedback; run on demand"]
fn feedback_tour() {
    type Step = (&'static str, Box<dyn Fn() -> Gui>);
    let mut steps: Vec<Step> = vec![
        // Getting started and the data.
        (
            "welcome-page",
            Box::new(|| {
                let mut gui = Gui::start();
                apply_color_scheme(&gui.harness.ctx.clone(), ColorScheme::Light);
                gui.run();
                gui
            }),
        ),
        (
            "data-view-first-use-tips",
            Box::new(|| {
                let mut gui = Gui::with_sample("Longley");
                apply_color_scheme(&gui.harness.ctx.clone(), ColorScheme::Light);
                data(&mut gui);
                gui
            }),
        ),
        (
            "data-view",
            Box::new(|| {
                let mut gui = on("Longley");
                data(&mut gui);
                gui
            }),
        ),
        (
            "side-by-side-view",
            Box::new(|| {
                let mut gui = on("Plant growth");
                let active = gui.app().active;
                gui.app_mut().documents[active].workspace = Workspace::SideBySide;
                gui.run();
                gui
            }),
        ),
        (
            "several-documents",
            Box::new(|| {
                let mut gui = on("Longley");
                for name in ["Fill weights", "Customer churn"] {
                    let n = app_model::sample::catalog().iter().position(|s| s.name.starts_with(name)).expect("sample");
                    gui.perform(Action::OpenSample(n));
                }
                data(&mut gui);
                gui
            }),
        ),
    ];
    // Every top menu, open.
    for (name, path) in [
        ("menu-file", &["File"][..]),
        ("menu-file-sample-data", &["File", "Open sample data"][..]),
        ("menu-edit", &["Edit"][..]),
        ("menu-data", &["Data"][..]),
        ("menu-describe", &["Describe"][..]),
        ("menu-compare", &["Compare"][..]),
        ("menu-model", &["Model"][..]),
        ("menu-multivariate", &["Multivariate"][..]),
        ("menu-design", &["Design"][..]),
        ("menu-quality", &["Quality"][..]),
        ("menu-reliability", &["Reliability"][..]),
        ("menu-visualize", &["Visualize"][..]),
        ("menu-scripts", &["Scripts"][..]),
        ("menu-help", &["Help"][..]),
    ] {
        steps.push((
            name,
            Box::new(move || {
                let mut gui = on("Longley");
                size(&mut gui, 1100.0);
                open_menu(&mut gui, path);
                gui
            }),
        ));
    }
    // The dialogs people open most, as they first appear.
    for (name, sample, path) in [
        ("dialog-distribution", "Michelson", &["Describe", "Distribution…"][..]),
        ("dialog-two-or-more-groups", "Silicon", &["Compare", "Two or more groups…"][..]),
        ("dialog-linear-model", "Longley", &["Model", "Linear model…"][..]),
        ("dialog-generalized-linear-model", "Customer churn", &["Model", "Generalized linear model…"][..]),
        ("dialog-principal-components", "Flower", &["Multivariate", "Principal components…"][..]),
        ("dialog-fractional-factorial", "Longley", &["Design", "Fractional factorial…"][..]),
        ("dialog-control-chart", "Fill weights", &["Quality", "Control chart…"][..]),
        ("dialog-process-capability", "Fill weights", &["Quality", "Process capability…"][..]),
        ("dialog-survival-curves", "Leukemia", &["Reliability", "Survival curves…"][..]),
    ] {
        steps.push((
            name,
            Box::new(move || {
                let mut gui = on(sample);
                gui.menu(path);
                gui
            }),
        ));
    }
    // Reports from the samples, one report open at a time.
    for (name, sample, n) in [
        ("report-distribution", "Michelson", 0),
        ("report-linear-regression", "Longley", 0),
        ("report-regression-diagnostics", "Norris", 0),
        ("report-one-way-anova", "Silicon", 0),
        ("report-ancova", "Plant growth", 0),
        ("report-paired-t-test", "Sleep drug", 0),
        ("report-contingency-table", "Customer survey", 0),
        ("report-logistic-regression", "Customer churn", 0),
        ("report-factorial-with-explorer", "Catalyst", 0),
        ("report-response-surface", "Reaction yield", 0),
        ("report-desirability", "Coating formulation", 0),
        ("report-principal-components", "Flower", 0),
        ("report-clustering", "Flower", 1),
        ("report-control-chart", "Fill weights", 0),
        ("report-control-chart-by-phase", "Fill weights", 1),
        ("report-gauge-rr", "Gauge study", 0),
        ("report-survival-curves", "Leukemia", 0),
        ("report-life-distribution", "Bearing life", 0),
        ("report-decision-tree", "Loan applications", 0),
        ("report-stepwise", "Hald", 0),
    ] {
        steps.push((
            name,
            Box::new(move || {
                let mut gui = on(sample);
                size(&mut gui, 1400.0);
                report(&mut gui, n);
                gui
            }),
        ));
    }
    steps.extend::<Vec<Step>>(vec![
        (
            "report-all-collapsed",
            Box::new(|| {
                let mut gui = on("Flower");
                gui.results();
                gui.press("Collapse all");
                gui
            }),
        ),
        (
            "report-options-menu",
            Box::new(|| {
                let mut gui = on("Longley");
                report(&mut gui, 0);
                let options = |n: &Node<'_>| n.role() == Role::Button && n.label().as_deref() == Some("⋯ Options");
                gui.harness.get_all_by(options).next().expect("options").simulate_click();
                gui.run();
                gui
            }),
        ),
        (
            "graph-canvas",
            Box::new(|| {
                let mut gui = on("Flower");
                clear_reports(&mut gui);
                gui.menu(&["Visualize", "Graph canvas…"]);
                gui.results();
                gui
            }),
        ),
        (
            "graph-canvas-with-plot",
            Box::new(|| {
                let mut gui = on("Longley");
                clear_reports(&mut gui);
                let spec = stat_viz::GraphSpec {
                    encoding: stat_viz::EncodingPlan {
                        x: vec!["GNP".into()],
                        y: vec!["Employed".into()],
                        ..Default::default()
                    },
                    points: Some(stat_viz::PointsElement::default()),
                    ..stat_viz::GraphSpec::default()
                };
                gui.app_mut().add_report(ReportRequest::Graph(Box::new(spec)));
                gui.results();
                gui
            }),
        ),
        (
            "contour-and-surface",
            Box::new(|| {
                let mut gui = on("Hillside");
                size(&mut gui, 1400.0);
                report(&mut gui, 0);
                gui
            }),
        ),
        (
            "script-window",
            Box::new(|| {
                let mut gui = on("Longley");
                gui.menu(&["Scripts", "Script window"]);
                gui
            }),
        ),
        (
            "settings-window",
            Box::new(|| {
                let mut gui = on("Longley");
                gui.menu(&["File", "Settings…"]);
                gui
            }),
        ),
        (
            "keyboard-shortcuts",
            Box::new(|| {
                let mut gui = on("Longley");
                gui.menu(&["Help", "Keyboard shortcuts"]);
                gui
            }),
        ),
        // Dark theme versions of a few screens.
        (
            "dark-data-view",
            Box::new(|| {
                let mut gui = on("Longley");
                data(&mut gui);
                dark(&mut gui);
                gui
            }),
        ),
        (
            "dark-report-linear-regression",
            Box::new(|| {
                let mut gui = on("Longley");
                size(&mut gui, 1400.0);
                report(&mut gui, 0);
                dark(&mut gui);
                gui
            }),
        ),
        (
            "dark-report-control-chart",
            Box::new(|| {
                let mut gui = on("Fill weights");
                size(&mut gui, 1400.0);
                report(&mut gui, 0);
                dark(&mut gui);
                gui
            }),
        ),
        (
            "dark-dialog-linear-model",
            Box::new(|| {
                let mut gui = on("Longley");
                dark(&mut gui);
                gui.menu(&["Model", "Linear model…"]);
                gui
            }),
        ),
    ]);
    let mut skipped = Vec::new();
    for (i, (name, step)) in steps.iter().enumerate() {
        let file = format!("{:02}-{name}", i + 1);
        if catch_unwind(AssertUnwindSafe(|| step().shot(&file))).is_err() {
            skipped.push(file);
        }
    }
    println!("{} steps, skipped: {skipped:?}", steps.len());
}
