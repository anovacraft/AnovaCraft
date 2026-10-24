//! Report views: sections, tables and their layout.

use super::*;

/// Reports with several sections of one kind (a quantile plot per fitted
/// distribution) gave them all the same widget ids, which egui flags in red
/// over the report and which made the plots share their view.
#[test]
fn sections_of_one_kind_have_their_own_ids() {
    let mut gui = Gui::with_sample("Seal strength");
    gui.harness.set_size(egui::vec2(1500.0, 2400.0));
    gui.results();
    let clashes: Vec<String> =
        gui.painted_text().into_iter().filter(|t| t.contains("widget ID") || t.contains("Double use")).collect();
    assert!(clashes.is_empty(), "id clashes: {clashes:?}");
    let active = gui.app().active;
    let plots = gui.app().documents[active].reports[0].plot_keys();
    let quantile = plots.iter().filter(|(kind, _)| *kind == app_model::SectionKind::ProbabilityPlot).count();
    assert!(quantile > 1, "each quantile plot keeps its own view state ({quantile})");
    gui.shot("report-sections-of-one-kind");
}

/// In the desirability goals table, rows without a target put the upper
/// limit, importance and shape in one cell, so every later column sat one
/// place to the left of its heading.
#[test]
fn desirability_goals_stay_in_their_columns() {
    let mut gui = Gui::with_sample("Coating formulation");
    gui.harness.set_size(egui::vec2(1500.0, 2400.0));
    gui.results();
    let left = |label: &str| {
        let starts = |n: &Node<'_>| n.label().or_else(|| n.value()).is_some_and(|l| l.starts_with(label));
        let n = gui.harness.query_all_by(starts).next().unwrap_or_else(|| panic!("no {label}"));
        n.raw_bounds().expect("a position").x0
    };
    // Predictions of Hardness (no target) and Gloss (target) under their heading.
    let heading = left("Predicted");
    for predicted in ["69.84", "87.33"] {
        let x = left(predicted);
        assert!((x - heading).abs() < 4.0, "prediction {predicted} at x = {x}, heading at {heading}");
    }
    gui.shot("report-desirability-goals");
}

/// Tree diagram nodes centred class shares wider than the node, cutting off
/// both ends (the row count and the last class); now the text is shortened
/// at its end so it starts with the count.
#[test]
fn tree_node_text_fits_its_box() {
    let mut gui = Gui::with_sample("Loan applications");
    gui.harness.set_size(egui::vec2(1500.0, 2400.0));
    gui.results();
    let texts = gui.painted_text();
    let root = texts.iter().find(|t| t.starts_with("n = 362 ·")).expect("the root node's summary");
    assert!(root.ends_with('…'), "{root}");
    gui.shot("report-tree-nodes");
}

/// A long status message (a sample's description) ran off the window and
/// under the row counts; it is now shortened to the space left of them.
#[test]
fn status_message_stops_before_the_row_counts() {
    let gui = Gui::with_sample("Clinic wait times");
    let find = |prefix: &str| {
        let n = gui
            .harness
            .query_all_by(|n| n.label().or_else(|| n.value()).is_some_and(|l| l.starts_with(prefix)))
            .last()
            .unwrap_or_else(|| panic!("no {prefix}"));
        n.raw_bounds().expect("a position")
    };
    let message = find("✔ Right-skewed");
    let hint = find("Selecting rows");
    let counts = find("150 rows");
    assert!(message.x1 <= hint.x0, "message ends at {} but the hint starts at {}", message.x1, hint.x0);
    assert!(hint.x1 <= counts.x0 && counts.x1 <= 1360.0, "hint {hint:?}, counts {counts:?}");
}

/// A continuous column by a categorical one with three or more groups shows
/// pairwise comparisons, Tukey–Kramer by default; the options menu switches
/// the method in place.
#[test]
fn bivariate_pairwise_comparisons_follow_the_options_menu() {
    let mut gui = Gui::with_sample("Silicon resistivity");
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui.app_mut().add_report(ReportRequest::Bivariate(app_model::BivariateRequest::new(0, 1)));
    gui.results();
    let heading = |gui: &Gui, method: &str| {
        gui.harness.query_all_by(|n| n.label().is_some_and(|l| l.starts_with(&format!("All Pairs: {method}")))).count()
            > 0
    };
    assert!(heading(&gui, "Tukey–Kramer"));
    gui.click("⋯ Options");
    gui.widget("Pairwise comparisons: Tukey–Kramer").hover();
    gui.run();
    gui.click("Games–Howell");
    gui.harness.press_key(egui::Key::Escape);
    gui.run();
    assert!(heading(&gui, "Games–Howell"), "the section follows the menu");
    let saved = gui.app().documents[active].reports[0].saved();
    let app_model::SavedReport::Bivariate { request } = saved else { panic!() };
    assert_eq!(request.pairwise, Some(app_model::ComparisonMethod::GamesHowell), "kept with the report");
    gui.shot("report-bivariate-pairwise");
}

/// A report's Export… button opens the export dialog for that report; it
/// writes the report's open sections (a section collapsed on screen is left
/// out) in the chosen format, and remembers the format. File ▸ Export
/// reports… (and ⌘/Ctrl+Shift+E) opens it for every report of the table.
/// The save dialog is answered by the test.
#[test]
fn reports_export_in_every_format() {
    use crate::file_io::test_save_answer;
    use app_model::report_export::ReportFormat;
    let dir = temp_dir("report-export");
    let mut gui = Gui::with_sample("Plant growth");
    gui.harness.set_size(egui::vec2(1500.0, 2400.0));
    gui.results();
    let active = gui.app().active;
    let first = gui.app().documents[active].reports[0].for_export();
    let sections = first.sections.expect("the model fits");
    // Collapse a section whose title only the first report has.
    let collapsed = sections
        .iter()
        .map(|s| s.title.clone())
        .find(|t| {
            gui.harness.query_all_by(|n| n.role() != Role::Label && n.label().as_deref() == Some(t.as_str())).count()
                == 1
        })
        .expect("a section title used once");
    gui.click(&collapsed);
    assert_eq!(gui.app().documents[active].reports[0].for_export().closed.len(), 1);

    // The report's Export… button: every report by default, collapsed sections included.
    let export = |n: &Node<'_>| n.role() == Role::Button && n.label().as_deref() == Some("Export…");
    let n = gui.app().documents[active].reports.len();
    gui.harness.get_all_by(export).next().expect("a report").simulate_click();
    gui.run();
    assert!(gui.shows("Export reports"));
    assert!(gui.shows(&format!("Only this report: {}", first.title)));
    gui.click("Web page (HTML) (.html)");
    gui.shot("report-export-dialog");
    let all = dir.join("everything.html");
    test_save_answer::set(Some(dir.join("everything")));
    gui.click("Save as…");
    assert!(!test_save_answer::pending(), "the export asked where to save");
    let html = std::fs::read_to_string(&all).expect("the page was written (.html added)");
    assert!(html.contains(&format!("<h3>{}</h3>", app_model::html_export::escape(&collapsed))), "collapsed kept");
    let message = |gui: &Gui| gui.app().message.clone().map(|(m, _)| m).unwrap_or_default();
    assert_eq!(message(&gui), format!("Saved {n} reports as {}", all.display()));
    assert_eq!(gui.app().settings.report_format, ReportFormat::Html, "remembered");

    // Only this report, leaving out what is collapsed.
    gui.harness.get_all_by(export).next().expect("a report").simulate_click();
    gui.run();
    gui.click(&format!("Only this report: {}", first.title));
    gui.click("Leave out collapsed sections and reports");
    let one = dir.join("model.html");
    test_save_answer::set(Some(dir.join("model")));
    gui.click("Save as…");
    let html = std::fs::read_to_string(&one).expect("the page was written");
    assert!(html.contains(&format!("<h1>{}</h1>", app_model::html_export::escape(&first.title))));
    assert!(!html.contains(&format!("<h2>{}</h2>", app_model::html_export::escape(&collapsed))), "collapsed");
    assert!(html.contains("<svg") && html.contains("<table>"));
    assert_eq!(message(&gui), format!("Saved the report as {}", one.display()));

    // Every report of the table, from the File menu, as PDF and Word.
    for (format, name) in [(ReportFormat::Pdf, "all.pdf"), (ReportFormat::Word, "all.docx")] {
        gui.menu(&["File", "Export reports…"]);
        assert!(!gui.shows(&format!("Only this report: {}", first.title)), "no report to choose");
        gui.click(&format!("{} (.{})", format.label(), format.extension()));
        test_save_answer::set(Some(dir.join(name)));
        gui.click("Save as…");
        let bytes = std::fs::read(dir.join(name)).expect("the file was written");
        assert!(bytes.starts_with(if format == ReportFormat::Pdf { b"%PDF" } else { b"PK\x03\x04" }));
        assert!(!gui.shows("Export reports"), "the dialog closed");
    }
    assert_eq!(message(&gui), format!("Saved {n} reports as {}", dir.join("all.docx").display()));

    // Collapsed reports are left out with what is collapsed: here, all of them.
    gui.click("Collapse all");
    assert!(gui.app().documents[active].reports.iter().all(|r| r.for_export().collapsed));
    gui.menu(&["File", "Export reports…"]);
    gui.click("Leave out collapsed sections and reports");
    test_save_answer::set(Some(dir.join("open.docx")));
    gui.click("Save as…");
    assert!(message(&gui).contains("nothing open to export"), "{}", message(&gui));
    assert!(!dir.join("open.docx").exists());
    assert!(test_save_answer::pending(), "no file was asked for");
    test_save_answer::set(None);

    // The shortcut opens the dialog; Cancel closes it, writing nothing.
    gui.harness.press_key_modifiers(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::E);
    gui.run();
    assert!(gui.shows("Export reports"));
    gui.click("Cancel");
    assert!(!gui.shows("Export reports"));
    let _ = std::fs::remove_dir_all(dir);
}

/// A multi-vari chart draws each cell's values side by side in row order,
/// joined like a short run chart; the options menu's Replicates choice
/// stacks them on the cell's vertical instead, and back.
#[test]
fn multi_vari_replicates_follow_the_options_menu() {
    let mut gui = Gui::with_sample("Shaft diameters");
    gui.results();
    let active = gui.app().active;
    let chart = |gui: &Gui| {
        let report = gui.app().documents[active].reports[0].for_export();
        let sections = report.sections.expect("the chart runs");
        let app_model::SectionContent::Plot(p) = &sections[0].content else { panic!("a plot") };
        (p.point_runs.len(), p.points.iter().map(|q| q.x.to_bits()).collect::<std::collections::BTreeSet<_>>().len())
    };
    let options = |gui: &Gui| match gui.app().documents[active].reports[0].saved() {
        app_model::SavedReport::MultiVari { options, .. } => options,
        _ => panic!("not a multi-vari chart"),
    };
    // 3 batches × 5 shafts, three positions each: 15 runs over 45 places.
    assert_eq!(options(&gui).replicates, app_model::ReplicateLayout::InOrder);
    let (runs, places) = chart(&gui);
    assert_eq!((runs, places), (15, 45), "in order by default");
    gui.shot("report-multi-vari-in-order");
    let open_options = |gui: &mut Gui| {
        let button = |n: &Node<'_>| n.role() == Role::Button && n.label().as_deref() == Some("⋯ Options");
        gui.harness.get_all_by(button).next().expect("a report").simulate_click();
        gui.run();
    };
    open_options(&mut gui);
    gui.click("Stacked");
    gui.harness.press_key(egui::Key::Escape);
    gui.run();
    assert_eq!(options(&gui).replicates, app_model::ReplicateLayout::Stacked);
    assert_eq!(chart(&gui), (0, 15), "stacked: one vertical per cell, no runs");
    open_options(&mut gui);
    gui.click("In order (side by side)");
    gui.harness.press_key(egui::Key::Escape);
    gui.run();
    assert_eq!(chart(&gui), (15, 45));
}
