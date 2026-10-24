//! Reports computed in the background: updating, cancel and compute again.

use super::*;
use crate::report_view::ReportJobs;

/// Plant growth with a distribution report of Height, then reports computed
/// on `workers` background threads (none: jobs wait in the queue).
fn plant_distribution_in_background(workers: usize) -> (Gui, usize) {
    let mut gui = Gui::with_sample("Plant growth");
    gui.harness.set_size(egui::vec2(1400.0, 1600.0));
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    let height = gui.app().documents[active].table.find("Height").unwrap();
    gui.app_mut().add_report(ReportRequest::Distribution(app_model::DistributionRequest {
        column: height,
        quantile_method: Default::default(),
        censoring: None,
    }));
    gui.results();
    let ctx = gui.harness.ctx.clone();
    gui.app_mut().report_jobs = Some(ReportJobs::with_workers(&ctx, workers));
    (gui, active)
}

/// Run frames until no report is computing (bounded).
fn finish_jobs(gui: &mut Gui, active: usize) {
    for _ in 0..500 {
        gui.run();
        let busy = gui.app().report_jobs.as_ref().is_some_and(ReportJobs::busy);
        if !busy && !gui.app().documents[active].reports.iter().any(Report::is_updating) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("the background jobs did not finish");
}

/// The report's sections, and the same analysis computed directly.
fn shown_and_expected(
    gui: &Gui,
    active: usize,
) -> (Vec<app_model::report::ReportSection>, Vec<app_model::report::ReportSection>) {
    let doc = &gui.app().documents[active];
    let report = &doc.reports[0];
    let by_hand = app_model::run_report(&report.request(), &doc.table, &doc.rows).unwrap();
    (report.for_export().sections.unwrap(), by_hand.sections)
}

/// Excluding rows recomputes the report on a worker thread and installs the
/// result, which equals the analysis of the remaining rows.
#[test]
fn excluding_rows_updates_the_report_in_the_background() {
    let (mut gui, active) = plant_distribution_in_background(2);
    gui.app_mut().documents[active].rows.set_excluded(0..20, true);
    finish_jobs(&mut gui, active);
    let (shown, expected) = shown_and_expected(&gui, active);
    assert_eq!(shown, expected);
    assert!(!gui.shows("Updating…"));
}

/// A slow update shows "Updating…" over the previous result; Cancel keeps
/// that result until "Compute again" or a further change starts it again.
#[test]
fn cancel_keeps_the_previous_result_until_computed_again() {
    let (mut gui, active) = plant_distribution_in_background(0);
    let before = gui.app().documents[active].reports[0].for_export().sections.unwrap();
    gui.app_mut().documents[active].rows.set_excluded(0..20, true);
    gui.run();
    assert!(gui.shows("Updating…"), "the header says the report is updating");
    assert!(gui.app().documents[active].reports[0].is_updating());
    assert_eq!(gui.app().documents[active].reports[0].for_export().sections.unwrap(), before);

    gui.click("Cancel");
    assert!(gui.shows("Computation cancelled"));
    assert!(!gui.shows("Updating…"));
    gui.run();
    assert!(!gui.app().documents[active].reports[0].is_updating(), "a cancelled report is not queued again");
    assert_eq!(gui.app().documents[active].reports[0].for_export().sections.unwrap(), before);

    // A further change starts it again.
    gui.app_mut().documents[active].rows.set_excluded(20..25, true);
    gui.run();
    assert!(gui.shows("Updating…"));
    gui.click("Cancel");
    assert!(gui.shows("Computation cancelled"));

    // With workers running, Compute again brings the report up to date.
    let ctx = gui.harness.ctx.clone();
    gui.app_mut().report_jobs = Some(ReportJobs::with_workers(&ctx, 2));
    gui.click("Compute again");
    finish_jobs(&mut gui, active);
    assert!(!gui.shows("Computation cancelled"));
    let (shown, expected) = shown_and_expected(&gui, active);
    assert_eq!(shown, expected);
    assert_ne!(shown, before);
}
