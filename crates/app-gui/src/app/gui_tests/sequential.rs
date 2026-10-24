//! Model ▸ Gaussian process and Design ▸ Propose next runs. The Gaussian
//! process dialog says what is missing, runs, and its report shows the
//! length scales, contour maps and Response Explorer; the proposal dialog
//! runs on the reaction sample, its report appends the proposed runs to the
//! table (undoably), and its batch size is edited in place.

use super::*;

const SAMPLE: &str = "Reaction yield (simulated)";

fn reports(gui: &Gui) -> usize {
    gui.app().documents[gui.app().active].reports.len()
}

/// Tick the `nth` check box named `label` (column lists repeat names).
fn tick(gui: &mut Gui, label: &str, nth: usize) {
    let tick = |n: &Node<'_>| n.role() == Role::CheckBox && n.label().as_deref() == Some(label);
    gui.harness.get_all_by(tick).nth(nth).expect("a check box").simulate_click();
    gui.run();
}

fn says(gui: &Gui, part: &str) -> bool {
    gui.harness.query_all_by_label_contains(part).next().is_some()
}

fn open_options(gui: &mut Gui) {
    let options = |n: &Node<'_>| n.role() == Role::Button && n.label().as_deref() == Some("⋯ Options");
    gui.harness.get_all_by(options).next().expect("a report").simulate_click();
    gui.run();
}

#[test]
fn gaussian_process_dialog_runs() {
    const TITLE: &str = "Gaussian process";
    let mut gui = Gui::with_sample(SAMPLE);
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui.menu(&["Model", "Gaussian process…"]);
    gui.shot("dialog-gaussian-process-first-open");
    assert!(gui.shows("⚠ Choose a response"), "{:?}", gui.painted_text());
    assert!(gui.widget("Run").is_disabled());
    gui.choose(TITLE, 0, "Yield");
    for name in ["Temperature", "Time", "Catalyst", "Solvent"] {
        tick(&mut gui, name, 0);
    }
    let summary = "Gaussian process (Matérn 5/2) for Yield on 4 predictors";
    assert!(says(&gui, summary), "{:?}", gui.painted_text());
    gui.shot("dialog-gaussian-process-ready");
    gui.click("Run");
    assert!(gui.harness.query_by_role_and_label(Role::Window, TITLE).is_none(), "the dialog closes");
    assert_eq!(reports(&gui), 1);
    gui.harness.set_size(egui::vec2(1500.0, 5000.0));
    gui.results();
    gui.shot("report-gaussian-process");
    for part in [
        "Gaussian Process (Matérn 5/2): Yield ~ Temperature + Time + Catalyst + Solvent",
        "Gaussian Process Summary",
        "Length Scales",
        "Actual by Leave-one-out Predicted",
        "Predicted Yield",
        "Standard Error of the Predicted Mean",
        "Response Explorer",
    ] {
        assert!(says(&gui, part), "{part}: {:?}", gui.painted_text());
    }
    open_options(&mut gui);
    assert!(gui.shows("3-D surface profiler"), "{:?}", gui.painted_text());
    gui.click("Edit analysis…");
    assert!(gui.harness.query_by_role_and_label(Role::Window, TITLE).is_some(), "the dialog reopens");
    assert!(says(&gui, summary), "filled from the report: {:?}", gui.painted_text());
}

#[test]
fn proposals_join_the_table() {
    const TITLE: &str = "Propose next runs";
    let mut gui = Gui::with_sample(SAMPLE);
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    let rows_before = gui.app().documents[active].table.n_rows();
    gui.menu(&["Design", "Propose next runs…"]);
    assert!(gui.harness.query_by_role_and_label(Role::Window, TITLE).is_some());
    gui.shot("dialog-propose-runs-first-open");
    assert!(gui.shows("⚠ Choose at least one response"), "{:?}", gui.painted_text());
    // Responses on the left; the factors' list (right) repeats the numeric names.
    tick(&mut gui, "Yield", 0);
    assert!(gui.shows("⚠ Choose at least one factor"));
    for name in ["Temperature", "Time", "Catalyst"] {
        tick(&mut gui, name, 1);
    }
    tick(&mut gui, "Solvent", 0);
    gui.widget("Constraints").type_text("Temperature + 0.5*Time <= 125");
    gui.run();
    assert!(says(&gui, "Propose 4 runs for Yield over 4 factors"), "{:?}", gui.painted_text());
    gui.shot("dialog-propose-runs-ready");
    gui.click("Run");
    assert!(gui.harness.query_by_role_and_label(Role::Window, TITLE).is_none(), "the dialog closes");
    assert_eq!(reports(&gui), 1);
    let doc = &gui.app().documents[active];
    let app_model::SavedReport::SequentialOptimization { request } = doc.reports[0].saved() else {
        panic!("not a sequential optimization")
    };
    assert_eq!(request.factors.len(), 4);
    assert_eq!(request.round, doc.table.find("Round"), "the round column is found by name");
    assert_eq!(request.constraints, "Temperature + 0.5*Time <= 125");
    gui.harness.set_size(egui::vec2(1500.0, 5000.0));
    gui.results();
    gui.shot("report-propose-runs");
    for part in [
        "Sequential Optimization: Yield",
        "Sequential Optimization Summary",
        "Best Run So Far",
        "Proposed Runs (4 runs",
        "Best Yield by Round",
        "Stop Rule",
    ] {
        assert!(says(&gui, part), "{part}: {:?}", gui.painted_text());
    }

    // The batch size is a live setting of the report.
    open_options(&mut gui);
    assert!(gui.shows("Add proposed runs to the table"), "{:?}", gui.painted_text());
    gui.click("Add proposed runs to the table");
    let doc = &gui.app().documents[active];
    assert_eq!(doc.table.n_rows(), rows_before + 4, "four new runs at the end");
    let round = doc.table.column(doc.table.find("Round").unwrap()).unwrap().as_numeric().unwrap();
    assert!(round[rows_before..].iter().all(|&r| r == 3.0), "{round:?}");
    let temp = doc.table.column(doc.table.find("Temperature").unwrap()).unwrap().as_numeric().unwrap().to_vec();
    let time = doc.table.column(doc.table.find("Time").unwrap()).unwrap().as_numeric().unwrap().to_vec();
    for r in rows_before..rows_before + 4 {
        assert!(temp[r] + 0.5 * time[r] <= 125.05, "run {r} keeps to the heat budget");
    }
    // The yield is a formula column, so the new runs are measured at once,
    // and the report proposes the next round.
    let y = doc.table.column(doc.table.find("Yield").unwrap()).unwrap().as_numeric().unwrap();
    assert!(y[rows_before..].iter().all(|v| v.is_finite()));
    assert!(says(&gui, "Undo removes them"), "{:?}", gui.painted_text());
    gui.app_mut().undo_redo(true);
    gui.run();
    assert_eq!(gui.app().documents[active].table.n_rows(), rows_before, "undo removes the runs");
}

/// Changing the batch size in the report's menu re-runs it in place.
#[test]
fn proposal_batch_is_a_live_setting() {
    let mut gui = Gui::with_sample(SAMPLE);
    gui.harness.set_size(egui::vec2(1500.0, 5000.0));
    gui.results();
    let active = gui.app().active;
    let id = gui.app().documents[active].reports[0].id();
    open_options(&mut gui);
    assert!(gui.shows("Settings (the report updates as you change them)"));
    gui.click("Upper confidence bound, with");
    let doc = &gui.app().documents[active];
    assert_eq!(doc.reports[0].id(), id);
    let app_model::SavedReport::SequentialOptimization { request } = doc.reports[0].saved() else { panic!() };
    assert!(matches!(request.criterion, stat_predictive::AcquisitionCriterion::UpperConfidenceBound { .. }));
}
