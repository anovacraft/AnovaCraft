//! Split-plot designs: the D-optimal dialog's per-factor "Changes" choice,
//! the created table's plot columns, and the mixed model filled in for a
//! declared split-plot design.

use super::*;

const DESIGN: &str = "Designed experiment";

fn shows_part(gui: &Gui, part: &str) -> bool {
    gui.harness.query_all_by_label_contains(part).next().is_some()
        || gui.painted_text().iter().any(|t| t.contains(part))
}

/// Design ▸ Split-plot opens the D-optimal dialog with factor A hard to
/// change; the created table has a Whole Plot column within which A is
/// constant, is declared as a split-plot design and carries the script that
/// fits its model. Marking B very hard turns it into a split-split-plot
/// design with subplots.
#[test]
fn split_plot_design_dialog_creates_whole_plots() {
    let mut gui = Gui::start();
    gui.menu(&["Design", "Split-plot (hard-to-change factors)…"]);
    assert!(
        shows_part(&gui, "Split-plot D-optimal, 3 factors, 7-term model (16 runs in 4 whole plots)"),
        "{:?}",
        gui.painted_text()
    );
    assert!(shows_part(&gui, "Hard to change (fixed within each whole plot): A."));
    assert!(gui.harness.query_all_by_label("Whole plots").next().is_some());
    gui.shot("split-plot-dialog");
    gui.click("Create");
    let active = gui.app().active;
    let doc = &gui.app().documents[active];
    let table = &doc.table;
    let col = |name: &str| table.column(table.find(name).expect(name)).unwrap().as_numeric().unwrap().to_vec();
    let (wp, a) = (col("Whole Plot"), col("A"));
    assert_eq!(wp.len(), 16);
    for r in 0..16 {
        for q in 0..16 {
            if wp[r] == wp[q] {
                assert_eq!(a[r], a[q], "A is fixed within whole plot {}", wp[r]);
            }
        }
    }
    assert_eq!(doc.reports.len(), 1);
    let app_model::SavedReport::DeclaredDesign { request } = doc.reports[0].saved() else { panic!("not declared") };
    assert_eq!(request.whole_plot, table.find("Whole Plot"));
    let scripts = &gui.app().project(doc.project).unwrap().scripts;
    assert_eq!(scripts.len(), 1);
    assert_eq!(scripts[0].name, app_model::doe::SPLIT_PLOT_SCRIPT);
    assert!(scripts[0].source.contains("\"mixed-model\""));

    // Very hard B and hard A: a split-split-plot design.
    gui.menu(&["Design", "Split-plot (hard-to-change factors)…"]);
    gui.choose(DESIGN, 2, "Very hard");
    assert!(gui.harness.query_all_by_label("Subplots").next().is_some(), "subplots are asked for");
    assert!(shows_part(&gui, "Split-split-plot D-optimal"), "{:?}", gui.painted_text());
    assert!(shows_part(&gui, "Very hard to change (fixed within each whole plot): B."));
    gui.click("Create");
    let table = &gui.app().documents[gui.app().active].table;
    assert!(table.find("Subplot").is_some());
}

/// The furnace sample opens with the declared design and its mixed model;
/// Model ▸ Mixed model on the declared design is filled in with the whole
/// plots as a random effect and the design's model.
#[test]
fn declared_split_plot_fills_in_the_mixed_model() {
    let mut gui = Gui::with_sample("Furnace coatings");
    let active = gui.app().active;
    gui.harness.set_size(egui::vec2(1500.0, 3000.0));
    gui.results();
    for heading in [
        "Restricted Randomization",
        "Analysis of a Split-Plot Design",
        "Tests by Fixed Effect (F tests, Satterthwaite denominator degrees of freedom)",
        "Random Effects (variance components, Satterthwaite limits)",
    ] {
        assert!(shows_part(&gui, heading), "{heading} missing: {:?}", gui.painted_text());
    }
    gui.shot("split-plot-report");
    // Keep only the declaration, then ask for a mixed model.
    gui.app_mut().documents[active].reports.truncate(1);
    gui.menu(&["Model", "Mixed model…"]);
    assert!(shows_part(&gui, "random effects by Whole Plot"), "{:?}", gui.painted_text());
    // The button, not the check box of the column called Run.
    let run = |n: &Node<'_>| n.role() == Role::Button && n.label().as_deref() == Some("Run");
    gui.harness.get_by(run).simulate_click();
    gui.run();
    let reports = &gui.app().documents[active].reports;
    assert_eq!(reports.len(), 2);
    let request = reports
        .iter()
        .find_map(|r| match r.saved() {
            app_model::SavedReport::Mixed { request, .. } => Some(request),
            _ => None,
        })
        .expect("a mixed model");
    let table = &gui.app().documents[active].table;
    assert_eq!(request.random[0].group, table.find("Whole Plot").unwrap());
    assert_eq!(request.effects.len(), 6, "main effects and two-factor interactions");
    assert!(request.reml);
}
