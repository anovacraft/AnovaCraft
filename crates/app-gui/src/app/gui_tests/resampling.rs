//! Resampling: Compare ▸ Permutation test… and "Bootstrap this table…" from
//! a report table's right-click menu.

use super::*;

/// Right-click the control labelled `label`, then let the UI settle.
fn right_click(gui: &mut Gui, label: &str) {
    let b = gui.widget(label).raw_bounds().expect("the control has a position");
    let at = egui::pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32);
    let button = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Secondary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    gui.harness.input_mut().events.push(egui::Event::PointerMoved(at));
    gui.harness.step();
    gui.harness.input_mut().events.push(button(true));
    gui.harness.step();
    gui.harness.input_mut().events.push(button(false));
    gui.run();
}

/// The permutation-test dialog runs a two-group test of medians and a
/// correlation test, each opening its report with the null distribution.
#[test]
fn permutation_test_dialog_runs_two_groups_and_correlation() {
    const TITLE: &str = crate::resampling_dialogs::PERMUTATION_TITLE;
    let mut gui = Gui::with_sample("Repair times");
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui.menu(&["Compare", "Permutation test…"]);
    assert!(gui.widget("Run").is_disabled(), "no columns chosen yet");
    gui.choose(TITLE, 0, "Repair time (h)");
    gui.choose(TITLE, 1, "Repair time (h)");
    assert!(gui.shows("⚠ A column is chosen twice"));
    gui.choose(TITLE, 1, "Crew");
    gui.click("Difference of medians");
    gui.click("Run");
    assert!(gui.harness.query_by_role_and_label(Role::Window, TITLE).is_none(), "the dialog closes");
    let reports = &gui.app().documents[active].reports;
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].title(), "Permutation Test: Repair time (h) by Crew");
    let app_model::SavedReport::PermutationTest { request } = reports[0].saved() else {
        panic!("not a permutation test")
    };
    assert!(matches!(
        request.design,
        app_model::PermutationDesign::TwoGroups {
            statistic: app_model::permutation_analysis::GroupStatistic::MedianDifference,
            ..
        }
    ));

    gui.menu(&["Compare", "Permutation test…"]);
    gui.click("Correlation");
    gui.click("Spearman correlation ρ");
    gui.choose(TITLE, 0, "Parts replaced");
    gui.choose(TITLE, 1, "Repair time (h)");
    gui.click("Statistic higher");
    gui.click("Run");
    assert_eq!(gui.app().documents[active].reports.len(), 2);
    gui.harness.set_size(egui::vec2(1500.0, 2400.0));
    gui.results();
    assert!(gui.shows("Permutation Distribution"));
    let text = gui.painted_text();
    assert!(text.iter().any(|t| t == "Observed"), "the observed value is marked");
    assert!(text.iter().any(|t| t.starts_with("Monte Carlo: 9999 random arrangements")), "{text:?}");
    gui.shot("permutation-test-report");
}

/// Right-clicking a report's table offers "Bootstrap this table…"; the
/// dialog lists the table's numeric columns and rows, and running it opens a
/// bootstrap report of the chosen cells.
#[test]
fn bootstrap_this_table_from_the_table_menu() {
    const TITLE: &str = crate::resampling_dialogs::BOOTSTRAP_TITLE;
    let mut gui = Gui::with_sample("Repair times");
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    let column = gui.app().documents[active].table.find("Repair time (h)").unwrap();
    gui.app_mut().add_report(ReportRequest::Distribution(app_model::DistributionRequest {
        column,
        quantile_method: Default::default(), censoring: None }));
    gui.harness.set_size(egui::vec2(1500.0, 2400.0));
    gui.results();
    // The report's ⋯ Options menu lists its tables too.
    gui.click("⋯ Options");
    gui.widget("Bootstrap a table").hover();
    gui.run();
    assert!(gui.shows("Summary Statistics…"));
    gui.harness.press_key(egui::Key::Escape);
    gui.run();

    right_click(&mut gui, "Summary Statistics");
    gui.click("Bootstrap this table…");
    assert!(gui.harness.query_by_role_and_label(Role::Window, TITLE).is_some(), "the dialog opens");
    assert_eq!(
        gui.widget("Value").toggled(),
        Some(egui::accesskit::Toggled::True),
        "the first numeric column is chosen"
    );
    // Keep only the mean and the standard deviation.
    for row in ["N", "Standard error of mean", "Lower 95% CI for mean", "Upper 95% CI for mean", "Skewness (G₁)"] {
        gui.press(row);
    }
    for row in ["Excess kurtosis (G₂)", "Coefficient of variation (%)", "Minimum", "Maximum", "Sum"] {
        gui.press(row);
    }
    gui.shot("bootstrap-dialog");
    gui.click("Run");
    assert!(gui.harness.query_by_role_and_label(Role::Window, TITLE).is_none(), "the dialog closes");
    let reports = &gui.app().documents[active].reports;
    assert_eq!(reports.len(), 2);
    let boot = reports.iter().find_map(|r| match r.saved() {
        app_model::SavedReport::Bootstrap { request } => Some((request, r.title())),
        _ => None,
    });
    let (request, title) = boot.expect("a bootstrap report");
    assert_eq!(title, "Bootstrap: Distribution: Repair time (h) · Summary Statistics");
    assert_eq!(request.section, "Summary Statistics");
    assert_eq!(request.columns, ["Value"]);
    assert_eq!(request.rows, ["Mean", "Standard deviation"]);
    gui.run();
    assert!(gui.shows("Bootstrap Estimates"));
    assert!(gui.shows("Bootstrap Distribution: Mean · Value"));
    gui.shot("bootstrap-report");
}
