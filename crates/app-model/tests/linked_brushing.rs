//! End-to-end check of linked brushing, using the same calls the GUI makes:
//! fit a model on the sample data, take the fitted-line plot's points, box-select
//! in plot coordinates, and confirm the data table's rows reflect the selection —
//! and that excluding the brushed rows refits the model without them.

use app_model::brushing::{PlotRegion, brush_click, brush_region, nearest_row, rows_in_region};
use app_model::{LinearModelAnalysis, LinearModelSpec, ReportOptions, SectionContent, SectionKind, sample};
use stat_data::{RowStateManager, SelectionMode};

fn fitted_line_points(a: &LinearModelAnalysis) -> Vec<app_model::RowPoint> {
    let sections = a.sections(&ReportOptions::default());
    let s = sections.iter().find(|s| s.kind == SectionKind::FittedLine).expect("fitted-line section");
    let SectionContent::Plot(p) = &s.content else { panic!("expected a plot") };
    p.points.clone()
}

#[test]
fn box_selection_in_plot_highlights_table_rows() {
    let table = sample::longley();
    let mut states = RowStateManager::new(table.n_rows());
    let gnp = table.find("GNP").unwrap();
    let employed = table.find("Employed").unwrap();
    let spec = LinearModelSpec { response: employed, predictors: vec![gnp], effects: Vec::new(), splines: Vec::new() };
    let analysis = LinearModelAnalysis::run(&table, &states, &spec).unwrap();
    let points = fitted_line_points(&analysis);

    // Brush every point with GNP between 350 000 and 450 000.
    let region = PlotRegion::from_corners([350_000.0, f64::MIN], [450_000.0, f64::MAX]);
    assert!(brush_region(&points, &region, SelectionMode::Replace, &mut states));

    // The data table highlights exactly the rows whose GNP is in that range —
    // computed independently from the table, not from the plot.
    let gnp_values = table.column(gnp).unwrap().as_numeric().unwrap();
    let expected: Vec<usize> =
        (0..table.n_rows()).filter(|&r| (350_000.0..=450_000.0).contains(&gnp_values[r])).collect();
    assert_eq!(expected, vec![6, 7, 8, 9, 10, 11], "1953–1958");
    for row in 0..table.n_rows() {
        assert_eq!(states.is_selected(row), expected.contains(&row), "row {row}");
    }
    assert_eq!(states.counts().selected, 6);
}

#[test]
fn table_selection_highlights_plot_points_and_modes_combine() {
    let table = sample::longley();
    let mut states = RowStateManager::new(table.n_rows());
    let spec = LinearModelSpec { response: 0, predictors: vec![2], effects: Vec::new(), splines: Vec::new() };
    let analysis = LinearModelAnalysis::run(&table, &states, &spec).unwrap();
    let points = fitted_line_points(&analysis);

    // Clicking rows 2 and 5 in the table (second with shift held) …
    brush_click(Some(2), SelectionMode::Replace, &mut states);
    brush_click(Some(5), SelectionMode::Extend, &mut states);
    // … marks exactly those points as selected in the plot.
    let highlighted: Vec<usize> = points.iter().filter(|p| states.is_selected(p.row)).map(|p| p.row).collect();
    assert_eq!(highlighted, vec![2, 5]);

    // Clicking near a point in screen space selects that point's row.
    let screen: Vec<(usize, [f32; 2])> = points.iter().map(|p| (p.row, [p.row as f32 * 20.0, 100.0])).collect();
    let hit = nearest_row(screen, [181.0, 102.0], 6.0);
    assert_eq!(hit, Some(9));
    brush_click(hit, SelectionMode::Toggle, &mut states);
    assert_eq!(states.selected_rows(), vec![2, 5, 9]);
}

#[test]
fn hidden_rows_cannot_be_brushed_and_excluded_rows_trigger_refit() {
    let table = sample::longley();
    let mut states = RowStateManager::new(table.n_rows());
    let spec = LinearModelSpec { response: 0, predictors: vec![6], effects: Vec::new(), splines: Vec::new() }; // Employed ~ Year
    let analysis = LinearModelAnalysis::run(&table, &states, &spec).unwrap();
    let points = fitted_line_points(&analysis);

    states.set_hidden([0, 1], true);
    let everything = PlotRegion::from_corners([f64::MIN, f64::MIN], [f64::MAX, f64::MAX]);
    let brushed = rows_in_region(&points, &everything, &states);
    assert_eq!(brushed.len(), 14);
    assert!(!brushed.contains(&0) && !brushed.contains(&1));

    // Brush 1960–1962 and exclude them: the analysis becomes stale, and refitting
    // uses the remaining 13 years.
    let late = PlotRegion::from_corners([1959.5, f64::MIN], [1962.5, f64::MAX]);
    brush_region(&points, &late, SelectionMode::Replace, &mut states);
    assert_eq!(states.selected_rows(), vec![13, 14, 15]);
    states.exclude_selected();
    assert!(analysis.is_stale(&states));
    let refit = LinearModelAnalysis::run(&table, &states, &spec).unwrap();
    assert_eq!(refit.fit().summary().n_observations, 13);
    assert_ne!(refit.fit().coefficients(), analysis.fit().coefficients());
}

#[test]
fn brushing_a_control_chart_selects_subgroup_rows() {
    use app_model::brushing::{chart_point_selected, chart_rows_in_region};
    use app_model::{ControlChartAnalysis, ControlChartOptions, ControlChartRequest};
    use stat_quality::ChartKind;

    let table = sample::longley();
    let mut states = RowStateManager::new(table.n_rows());
    let employed = table.find("Employed").unwrap();
    let req = ControlChartRequest::new(employed, ChartKind::MeanAndRange, 4);
    let chart = ControlChartAnalysis::run(&table, &states, &req).unwrap();
    let sections = chart.sections(&ControlChartOptions::default());
    let SectionContent::ControlChart(means) = &sections[1].content else { panic!("expected a control chart") };
    assert_eq!(means.points.len(), 4, "16 years in subgroups of 4");

    // Box-select samples 2 and 3 (any value): selects rows 4–11 in the data table.
    let region = PlotRegion::from_corners([1.5, f64::MIN], [3.5, f64::MAX]);
    let rows = chart_rows_in_region(&means.points, &region, &states);
    assert_eq!(rows, (4..12).collect::<Vec<_>>());
    states.select(rows, SelectionMode::Replace);
    for r in 0..table.n_rows() {
        assert_eq!(states.is_selected(r), (4..12).contains(&r));
    }

    // Selecting one row in the table highlights the chart point of its subgroup.
    states.select([13], SelectionMode::Replace);
    let highlighted: Vec<usize> =
        means.points.iter().filter(|p| chart_point_selected(p, &states)).map(|p| p.sample).collect();
    assert_eq!(highlighted, vec![4]);
}

#[test]
fn brushing_a_histogram_bar_selects_its_rows() {
    use app_model::brushing::{bar_at, selected_in_bar};
    use app_model::{DistributionAnalysis, DistributionOptions, DistributionRequest};

    let table = sample::longley();
    let mut states = RowStateManager::new(table.n_rows());
    let col = table.find("Unemployed").unwrap();
    let req = DistributionRequest { column: col, quantile_method: Default::default(), censoring: None };
    let a = DistributionAnalysis::run(&table, &states, &req).unwrap();
    let sections = a.sections(&DistributionOptions::default());
    let hist = sections.iter().find(|s| s.kind == SectionKind::Histogram).unwrap();
    let SectionContent::Bars(h) = &hist.content else { panic!("expected bars") };
    assert_eq!(h.bars.iter().map(|b| b.count()).sum::<usize>(), 16, "every year counted once");

    // Click the tallest bar: its rows — and only those — become selected.
    let tallest = h.bars.iter().max_by_key(|b| b.count()).unwrap();
    let hit = bar_at(&h.bars, tallest.center, 0.5, 0.3).unwrap();
    states.select(hit.rows.iter().copied(), SelectionMode::Replace);
    let values = table.column(col).unwrap().as_numeric().unwrap();
    let (lo, hi) = (tallest.center - tallest.width / 2.0, tallest.center + tallest.width / 2.0);
    for r in 0..table.n_rows() {
        let in_bin = (lo..hi).contains(&values[r]) || (values[r] == hi && hit.rows.contains(&r));
        assert_eq!(states.is_selected(r), in_bin, "row {r} value {}", values[r]);
    }
    assert_eq!(selected_in_bar(tallest, &states), tallest.count());
}
