//! A report's own row filter and column picker.

use super::*;

/// Plant growth with a single distribution report of Height.
fn plant_distribution() -> (Gui, usize) {
    let mut gui = Gui::with_sample("Plant growth");
    gui.harness.set_size(egui::vec2(1400.0, 1600.0));
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    let height = gui.app().documents[active].table.find("Height").unwrap();
    gui.app_mut().add_report(ReportRequest::Distribution(app_model::DistributionRequest {
        column: height,
        quantile_method: Default::default(), censoring: None }));
    gui.results();
    (gui, active)
}

/// The report's single "⋯ Options" button.
fn open_options(gui: &mut Gui) {
    gui.click("⋯ Options");
}

/// Filter rows… adds a filter strip; a condition on a categorical column
/// with one level unticked recomputes the report from the other rows, notes
/// how many it left out, and leaves the table's row states alone.
#[test]
fn row_filter_narrows_one_report_only() {
    let (mut gui, active) = plant_distribution();
    open_options(&mut gui);
    gui.click("Filter rows…");
    assert!(gui.shows("Row filter"));
    gui.click("+ Add condition");
    let fertilizer = |n: &Node<'_>| n.role() == Role::Button && n.label().as_deref() == Some("Fertilizer");
    gui.harness.get_all_by(fertilizer).last().expect("the column in the menu").simulate_click();
    gui.run();
    gui.click("Filter Fertilizer");
    gui.click("A");
    gui.harness.press_key(egui::Key::Escape);
    gui.run();

    let doc = &gui.app().documents[active];
    let report = &doc.reports[0];
    let expected =
        app_model::FilterCondition::Levels { column: "Fertilizer".into(), levels: vec!["B".into(), "C".into()] };
    assert_eq!(report.scope().filter, vec![expected]);
    assert_eq!(doc.rows.counts().excluded, 0, "the table's rows are not excluded");
    assert!(gui.shows("20 of 60 rows filtered out"), "the note counts the rows left out");
    // The report equals the same analysis with fertilizer A's rows excluded.
    let fertilizer_a: Vec<usize> = (0..doc.table.n_rows())
        .filter(|&r| doc.table.column(doc.table.find("Fertilizer").unwrap()).unwrap().cell_text(r) == "A")
        .collect();
    let mut excluded = doc.rows.clone();
    excluded.set_excluded(fertilizer_a, true);
    let by_hand = app_model::run_report(&report.request(), &doc.table, &excluded).unwrap();
    assert_eq!(report.for_export().sections.unwrap(), by_hand.sections);

    // Saved with the report, and in its script.
    let saved = report.saved();
    let json = app_model::project::report_to_json(&saved, &doc.table);
    assert!(json.get("row_filter").is_some());
    assert_eq!(app_model::project::report_from_json(&json, &doc.table).unwrap(), saved);
    gui.shot("report-row-filter");

    // Clear takes the filter away.
    gui.click("Clear");
    let report = &gui.app().documents[active].reports[0];
    assert!(report.scope().filter.is_empty());
    assert!(!gui.shows("Row filter"));
    assert!(!gui.shows("20 of 60 rows filtered out"));
}

/// A column used by a report's filter counts as used by the report, so it
/// cannot be deleted while the report is open, and a rename is followed.
#[test]
fn filter_columns_are_in_use_and_follow_renames() {
    let (mut gui, active) = plant_distribution();
    let scope = app_model::ReportScope {
        filter: vec![app_model::FilterCondition::Range { column: "Light hours".into(), min: 0.0, max: 100.0 }],
        ..Default::default()
    };
    let doc = &mut gui.app_mut().documents[active];
    let saved = doc.reports[0].saved().with_scope(scope);
    doc.reports[0].replace_all(saved, &doc.table, &doc.rows);
    let light = doc.table.find("Light hours").unwrap();
    assert!(doc.reports[0].uses_column(light, &doc.table));
    gui.grid(vec![GridRequest::RenameColumn(light, "Daylight".into())]);
    let report = &gui.app().documents[active].reports[0];
    assert_eq!(report.scope().filter_columns(), vec!["Daylight"]);
}

/// The column picker steps a report through other columns of the same
/// kind, keeping its other settings.
#[test]
fn column_switcher_steps_through_columns() {
    let (mut gui, active) = plant_distribution();
    open_options(&mut gui);
    gui.click("Column picker");
    gui.harness.press_key(egui::Key::Escape);
    gui.run();
    let column = |gui: &Gui| {
        let saved = gui.app().documents[active].reports[0].request();
        let app_model::SavedReport::Distribution { request, .. } = saved else { panic!("not a distribution") };
        request.column
    };
    let table = &gui.app().documents[active].table;
    let (light, house, height) =
        (table.find("Light hours").unwrap(), table.find("Greenhouse").unwrap(), table.find("Height").unwrap());
    assert_eq!(column(&gui), height);
    assert!(gui.shows("3 of 3"), "Height is the last of the numeric columns");
    gui.click("Next");
    assert_eq!(column(&gui), light, "Next wraps to the first choice");
    assert!(gui.app().documents[active].reports[0].title().contains("Light hours"));
    gui.click("Next");
    assert_eq!(column(&gui), house);
    gui.click("Previous");
    assert_eq!(column(&gui), light);
    gui.shot("report-column-switcher");
    // The switcher is saved with the report.
    let scope = gui.app().documents[active].reports[0].saved().scope().cloned().expect("a scope");
    assert_eq!(scope.switcher.as_deref().map(<[String]>::len), Some(3));
}

/// A capability study's switcher keeps specification limits per column,
/// edited beside it.
#[test]
fn capability_switcher_keeps_limits_per_column() {
    let mut gui = Gui::with_sample("Plant growth");
    gui.harness.set_size(egui::vec2(1400.0, 1600.0));
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    let table = &gui.app().documents[active].table;
    let (light, height) = (table.find("Light hours").unwrap(), table.find("Height").unwrap());
    let limits = stat_quality::SpecLimits { lower: None, upper: Some(40.0), target: None };
    gui.app_mut().add_report(ReportRequest::Capability(app_model::CapabilityRequest::new(height, limits)));
    gui.results();
    open_options(&mut gui);
    gui.click("Column picker");
    gui.harness.press_key(egui::Key::Escape);
    gui.run();
    assert!(gui.shows("Specification limits for Height:"));
    let request = |gui: &Gui| {
        let saved = gui.app().documents[active].reports[0].request();
        let app_model::SavedReport::Capability { request } = saved else { panic!("not capability") };
        request
    };
    // Next wraps to Light hours, which starts with Height's limits.
    gui.click("Next");
    assert_eq!((request(&gui).column, request(&gui).limits), (light, limits));
    assert!(gui.shows("Specification limits for Light hours:"));
    // Each limit applies as it is typed.
    gui.widget("Lower").type_text("6");
    gui.run();
    gui.widget("Target").type_text("12");
    gui.run();
    let own = stat_quality::SpecLimits { lower: Some(6.0), upper: Some(40.0), target: Some(12.0) };
    assert_eq!(request(&gui).limits, own);
    gui.shot("report-capability-switcher");
    // Back to Height and forward again: each column has its own limits.
    gui.click("Previous");
    assert_eq!((request(&gui).column, request(&gui).limits), (height, limits));
    gui.click("Next");
    assert_eq!((request(&gui).column, request(&gui).limits), (light, own));
}

/// Plot settings are kept with the report: changed settings go into its
/// scope and project form, a report made from that form draws with them,
/// an undo-style replacement brings them back, and settings returned to the
/// defaults are forgotten.
#[test]
fn plot_settings_are_kept_with_the_report() {
    use app_model::axis_settings::GridLines;
    let (mut gui, active) = plant_distribution();
    let kind = app_model::report::SectionKind::Histogram;
    let key = app_model::plot_style_key(kind, 0);
    {
        let axes = gui.app_mut().documents[active].reports[0].plot_axes_mut(kind).expect("a histogram");
        axes.y.grid = GridLines::None;
        axes.x.title = Some("Plant height (cm)".into());
    }
    gui.run();
    let doc = &gui.app().documents[active];
    let style = doc.reports[0].scope().plot_style(&key).expect("the changed settings are kept").clone();
    assert_eq!(style.axes.y.grid, GridLines::None);
    assert_eq!(style.axes.x.title.as_deref(), Some("Plant height (cm)"));
    assert!(gui.painted_text_rects().iter().any(|(t, ..)| t == "Plant height (cm)"), "the title is drawn");

    // Saved with the report and read back the same.
    let saved = doc.reports[0].saved();
    let json = app_model::project::report_to_json(&saved, &doc.table);
    assert!(json.get("plot_styles").is_some());
    let back = app_model::project::report_from_json(&json, &doc.table).unwrap();
    assert_eq!(back, saved);

    // A report made from the saved form (as on reopening) draws with them.
    let (table, rows) = (doc.table.clone(), doc.rows.clone());
    let copy = crate::report_view::Report::new(9_999, back, &table, &rows);
    gui.app_mut().documents[active].reports.push(copy);
    gui.run();
    let axes = gui.app_mut().documents[active].reports[1].plot_axes_mut(kind).unwrap().clone();
    assert_eq!(axes.y.grid, GridLines::None);
    assert_eq!(axes.x.title.as_deref(), Some("Plant height (cm)"));
    gui.app_mut().documents[active].reports.pop();

    // Back at the defaults: nothing is kept.
    {
        let axes = gui.app_mut().documents[active].reports[0].plot_axes_mut(kind).unwrap();
        axes.y.grid = GridLines::MajorOnly;
        axes.x.title = None;
    }
    gui.run();
    assert!(gui.app().documents[active].reports[0].scope().plot_styles.is_empty());

    // Replacing the report with the styled form (what Undo does) draws the
    // plot in that style again.
    gui.app_mut().documents[active].reports[0].replace_all(saved, &table, &rows);
    gui.run();
    let axes = gui.app_mut().documents[active].reports[0].plot_axes_mut(kind).unwrap().clone();
    assert_eq!(axes.y.grid, GridLines::None);
    assert!(gui.app().documents[active].reports[0].scope().plot_style(&key).is_some());
}
