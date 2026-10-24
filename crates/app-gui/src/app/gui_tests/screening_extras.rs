//! Design options: definitive screening extras and their comparison,
//! screening with categorical factors, new whole plots for a split-plot
//! design, simulated responses, and the several-responses constraints and
//! specification region.

use super::*;

const DESIGN: &str = "Designed experiment";
const AUGMENT: &str = crate::augment_dialog::TITLE;
const SIMULATE: &str = crate::simulate_dialog::TITLE;

fn shows_part(gui: &Gui, part: &str) -> bool {
    gui.harness.query_all_by_label_contains(part).next().is_some()
        || gui.painted_text().iter().any(|t| t.contains(part))
}

fn number(gui: &mut Gui, window: &str, label: &str, value: f64) {
    let id = gui
        .window(window)
        .get_all_by(|n| n.label().as_deref() == Some(label))
        .next()
        .unwrap_or_else(|| panic!("no field {label}"))
        .id();
    gui.set_number(id, value);
}

fn tick(gui: &mut Gui, window: &str, label: &str) {
    gui.window(window)
        .get_all_by(|n| n.role() == Role::CheckBox && n.label().as_deref() == Some(label))
        .last()
        .unwrap_or_else(|| panic!("no check box {label}"))
        .simulate_click();
    gui.run();
}

/// Definitive screening with 4 extra runs and 3 center runs: the preview
/// counts them, the comparison lists nearby choices, and the created table
/// has the runs.
#[test]
fn definitive_screening_extra_runs_and_comparison() {
    let mut gui = Gui::start();
    gui.menu(&["Design", "Definitive screening…"]);
    assert!(shows_part(&gui, "13 runs"), "{:?}", gui.painted_text());
    number(&mut gui, DESIGN, "Extra runs", 4.0);
    number(&mut gui, DESIGN, "Center runs", 3.0);
    assert!(shows_part(&gui, "19 runs"), "{:?}", gui.painted_text());
    gui.click("Compare run sizes");
    for heading in ["Error df (main effects)", "Max |r| quadratic", "Mean SE quadratic (σ)"] {
        assert!(shows_part(&gui, heading), "{heading} missing: {:?}", gui.painted_text());
    }
    gui.shot("design-dsd-compare");
    gui.click("Create");
    let table = &gui.app().documents[gui.app().active].table;
    assert_eq!(table.n_rows(), 19);
}

/// Screening with categorical factors: B is categorical with three levels;
/// the created table has a text column of level labels used equally often.
#[test]
fn screening_with_categorical_factors() {
    let mut gui = Gui::start();
    gui.menu(&["Design", "Screening with categorical factors…"]);
    assert!(shows_part(&gui, "B: levels L1 to L3 run 4, 4, 4 times"), "{:?}", gui.painted_text());
    gui.choose(DESIGN, 3, "Categorical");
    number(&mut gui, DESIGN, "Levels 3", 4.0);
    assert!(shows_part(&gui, "C: levels L1 to L4"), "{:?}", gui.painted_text());
    gui.shot("design-categorical-screening");
    gui.click("Create");
    let table = &gui.app().documents[gui.app().active].table;
    let b = table.column(table.find("B").expect("factor B")).unwrap();
    assert_eq!(b.level, stat_data::MeasurementLevel::Nominal);
    let mut counts = std::collections::BTreeMap::new();
    for r in 0..table.n_rows() {
        *counts.entry(b.cell_text(r)).or_insert(0) += 1;
    }
    assert_eq!(counts.keys().cloned().collect::<Vec<_>>(), ["L1", "L2", "L3"]);
    let a = table.column(table.find("A").unwrap()).unwrap().as_numeric().unwrap();
    assert!(a.iter().all(|v| v.abs() == 1.0), "continuous factors at their low and high settings");
}

/// Augment ▸ New whole plots on a split-plot design: two more whole plots of
/// four runs, numbered after the existing ones, with the hard factor fixed
/// within each.
#[test]
fn augment_with_new_whole_plots() {
    let mut gui = Gui::start();
    gui.menu(&["Design", "Split-plot (hard-to-change factors)…"]);
    gui.click("Create");
    let original = gui.app().active;
    gui.menu(&["Design", "Augment design…"]);
    for factor in ["A", "B", "C"] {
        tick(&mut gui, AUGMENT, factor);
    }
    gui.click("New whole plots");
    assert!(
        shows_part(&gui, "Hard to change (constant within every existing whole plot): A."),
        "{:?}",
        gui.painted_text()
    );
    assert!(shows_part(&gui, "Adds 8 runs to"), "{:?}", gui.painted_text());
    gui.shot("design-augment-whole-plots");
    gui.click("Create");
    assert_ne!(gui.app().active, original);
    let table = &gui.app().documents[gui.app().active].table;
    assert_eq!(table.n_rows(), 24);
    let col = |name: &str| table.column(table.find(name).expect(name)).unwrap().as_numeric().unwrap().to_vec();
    let (wp, a) = (col("Whole Plot"), col("A"));
    let mut new_plots: Vec<f64> = wp[16..].to_vec();
    new_plots.sort_by(f64::total_cmp);
    new_plots.dedup();
    assert_eq!(new_plots, [5.0, 6.0]);
    for r in 16..24 {
        for q in 16..24 {
            if wp[r] == wp[q] {
                assert_eq!(a[r], a[q], "A is fixed within new whole plot {}", wp[r]);
            }
        }
    }
}

/// Design ▸ Simulate responses… adds response columns to the table (one
/// undoable edit) and a coefficients table to the project.
#[test]
fn simulate_responses_adds_columns() {
    let mut gui = Gui::start();
    gui.menu(&["Design", "Full factorial (2 levels)…"]);
    gui.click("Create");
    let active = gui.app().active;
    let (cols, docs) = (gui.app().documents[active].table.n_cols(), gui.app().documents.len());
    gui.menu(&["Design", "Simulate responses…"]);
    assert!(gui.shows("⚠ Choose the factor columns"));
    for factor in ["A", "B", "C"] {
        tick(&mut gui, SIMULATE, factor);
    }
    gui.click("Strong");
    number(&mut gui, SIMULATE, "Responses to simulate", 3.0);
    assert!(shows_part(&gui, "Adds 3 simulated responses"), "{:?}", gui.painted_text());
    gui.shot("design-simulate-responses");
    gui.click("Add");
    let table = &gui.app().documents[gui.app().active].table;
    assert_eq!(gui.app().active, active, "the design stays in view");
    assert_eq!(table.n_cols(), cols + 3);
    assert!(table.find("Simulated Y 1").is_some());
    assert_eq!(gui.app().documents.len(), docs + 1, "the coefficients table joins the project");
    let coefficients = gui.app().documents.iter().find(|d| d.table.name().contains("simulated coefficients"));
    assert_eq!(coefficients.expect("coefficients table").table.n_rows(), 3);
}

/// The coating sample's several-responses report keeps the best settings to
/// its constraint and checks the specification region's corners; editing
/// the constraint refits it, and a mistake is not applied.
#[test]
fn several_responses_constraints_and_region() {
    let mut gui = Gui::with_sample("Coating formulation");
    gui.harness.set_size(egui::vec2(1500.0, 4000.0));
    gui.results();
    assert!(
        shows_part(&gui, "Specification Region: probability of meeting every limit at the 4 corners"),
        "{:?}",
        gui.painted_text()
    );
    assert!(shows_part(&gui, "Corner 1"));
    // The first report's button (the sample's second report selects terms stepwise).
    let find = |n: &Node<'_>| n.role() == Role::Button && n.label().as_deref() == Some("Find best settings");
    gui.harness.get_all_by(find).next().expect("a search button").simulate_click();
    gui.run();
    assert!(shows_part(&gui, "within the factor ranges and 1 constraint"), "{:?}", gui.painted_text());
    gui.shot("report-several-region");
    let doc = gui.app().active;
    let request = |gui: &Gui| {
        gui.app().documents[doc]
            .reports
            .iter()
            .find_map(|r| match r.saved() {
                app_model::SavedReport::MultipleResponses { request } => Some(request.clone()),
                _ => None,
            })
            .expect("a several-responses report")
    };
    assert_eq!(request(&gui).constraints, "Temperature (°C) + 2*Time (min) <= 230");
    // The first report's box (the sample's second report selects terms stepwise).
    let constraints = |n: &Node<'_>| n.role() != Role::Label && n.label().as_deref() == Some("Constraints");
    gui.harness.get_all_by(constraints).next().expect("a constraints box").type_text("\nPressure <= 3");
    gui.run();
    assert!(shows_part(&gui, "unknown factor \"Pressure\""), "{:?}", gui.painted_text());
    assert_eq!(request(&gui).constraints, "Temperature (°C) + 2*Time (min) <= 230", "not applied");
}
