//! Reference lines and bands on report plots and graph canvases, error bar
//! styling and significance bracket pairs.

use super::*;
use stat_viz::element::reference::{ReferenceStat, ReferenceValue};
use stat_viz::{Mark, ReferenceOrientation};

/// Longley with its own reports closed and a scatter plot of GNP against
/// Employed (a bivariate report).
fn scatter() -> (Gui, usize) {
    let mut gui = Gui::with_sample("Longley");
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui.app_mut().add_report(ReportRequest::Bivariate(app_model::BivariateRequest::new(2, 0)));
    gui.run();
    (gui, active)
}

/// The first plot's saved reference lines, if any.
fn saved_reference(gui: &Gui, active: usize) -> stat_viz::plot_style::ReferenceLook {
    let scope = gui.app().documents[active].reports[0].scope();
    scope.plot_styles.first().map(|(_, s)| s.reference.clone()).unwrap_or_default()
}

/// The window whose title starts with "Reference lines".
fn reference_window_open(gui: &Gui) -> bool {
    gui.harness.query_all_by_role(Role::Window).any(|w| w.label().is_some_and(|l| l.starts_with("Reference lines")))
}

/// Click `label` inside the Reference lines window (the plot's own menus may
/// have controls of the same name).
fn click_in_window(gui: &mut Gui, label: &str) {
    let window = gui
        .harness
        .get_all_by_role(Role::Window)
        .find(|w| w.label().is_some_and(|l| l.starts_with("Reference lines")))
        .expect("the Reference lines window")
        .id();
    gui.harness
        .get_by(|n| n.id() == window)
        .get_by(|n| n.role() != Role::Label && n.label().as_deref() == Some(label))
        .simulate_click();
    gui.run();
}

/// Plot settings ▸ Reference lines… opens the window; a line added there
/// is drawn with its label, moves to a statistic of the data (the mean of
/// Employed, drawn over the points), and is saved with the report so a
/// reopened project draws it again.
#[test]
fn report_plot_reference_line_from_plot_settings() {
    let (mut gui, active) = scatter();
    gui.click("Plot settings ▾");
    gui.press(crate::reference_lines_ui::OPEN_LABEL);
    assert!(reference_window_open(&gui));
    click_in_window(&mut gui, "+ Add line");
    let look = saved_reference(&gui, active);
    assert_eq!(look.lines.len(), 1, "the line is kept with the report");
    assert!(gui.painted_text().iter().any(|t| t == "y = 0"), "the line's label is drawn");

    // Place it at the mean of the vertical values.
    // The window's second drop-down list (after the orientation).
    let title = gui
        .harness
        .get_all_by_role(Role::Window)
        .find_map(|w| w.label().filter(|l| l.starts_with("Reference lines")))
        .expect("the window");
    gui.choose(&title, 1, "Mean");
    let look = saved_reference(&gui, active);
    assert_eq!(look.lines[0].value, ReferenceValue::Statistic(ReferenceStat::Mean));
    let employed = &gui.app().documents[active].table;
    let column = employed.find("Employed").unwrap();
    let values = employed.columns()[column].as_numeric().unwrap().to_vec();
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    let label = format!("Mean = {}", stat_viz::view::format_value(mean));
    assert!(gui.painted_text().contains(&label), "{label} drawn; {:?}", gui.painted_text());
    gui.shot("reference-line-report");

    // Saved with the report and read back the same.
    let doc = &gui.app().documents[active];
    let saved = doc.reports[0].saved();
    let json = app_model::project::report_to_json(&saved, &doc.table);
    let back = app_model::project::report_from_json(&json, &doc.table).unwrap();
    assert_eq!(back, saved);
    let (table, rows) = (doc.table.clone(), doc.rows.clone());
    gui.app_mut().documents[active].reports.clear();
    let copy = crate::report_view::Report::new(9_999, back, &table, &rows);
    gui.app_mut().documents[active].reports.push(copy);
    gui.run();
    assert!(gui.painted_text().contains(&label), "the reopened report draws the line");
}

/// Right-clicking the plot offers Reference lines… too; excluding rows
/// moves a statistic-linked line, as statistics leave excluded rows out.
#[test]
fn report_plot_reference_line_from_right_click_follows_row_states() {
    let (mut gui, active) = scatter();
    let ruler = {
        let b = gui.widget("Horizontal axis ruler").raw_bounds().unwrap();
        egui::Rect::from_min_max(egui::pos2(b.x0 as f32, b.y0 as f32), egui::pos2(b.x1 as f32, b.y1 as f32))
    };
    let at = egui::pos2(ruler.center().x, ruler.top() - 60.0);
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
    gui.click(crate::reference_lines_ui::OPEN_LABEL);
    assert!(reference_window_open(&gui));
    // A line at the median, set directly (the window edits the same value).
    gui.app_mut().documents[active].reports[0]
        .plot_state_mut(app_model::report::SectionKind::BivariateScatter)
        .unwrap()
        .reference
        .lines
        .push(stat_viz::ReferenceLine {
            value: ReferenceValue::Statistic(ReferenceStat::Median),
            ..stat_viz::ReferenceLine::default()
        });
    gui.run();
    let median_label = |gui: &Gui| gui.painted_text().into_iter().find(|t| t.starts_with("Median = "));
    let before = median_label(&gui).expect("the median line is labelled");
    // Exclude the eight rows with the largest Employed values.
    let doc = &gui.app().documents[active];
    let column = doc.table.find("Employed").unwrap();
    let values = doc.table.columns()[column].as_numeric().unwrap().to_vec();
    let mut order: Vec<usize> = (0..values.len()).collect();
    order.sort_by(|&a, &b| values[b].total_cmp(&values[a]));
    let rows = &mut gui.app_mut().documents[active].rows;
    rows.select(order[..8].to_vec(), stat_data::SelectionMode::Replace);
    rows.exclude_selected();
    gui.run();
    let after = median_label(&gui).expect("still labelled");
    assert_ne!(before, after, "the median follows the exclusion");
}

/// Longley with a graph canvas of Employed against GNP.
fn graph() -> Gui {
    let mut gui = Gui::with_sample("Longley");
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    let spec = stat_viz::GraphSpec {
        encoding: stat_viz::EncodingPlan { x: vec!["GNP".into()], y: vec!["Employed".into()], ..Default::default() },
        points: Some(stat_viz::PointsElement::default()),
        ..stat_viz::GraphSpec::default()
    };
    gui.app_mut().add_report(ReportRequest::Graph(Box::new(spec)));
    gui.run();
    gui
}

fn saved_spec(gui: &Gui) -> stat_viz::GraphSpec {
    let doc = &gui.app().documents[gui.app().active];
    let app_model::SavedReport::Graph { spec } = doc.reports[0].saved() else { panic!("a graph report") };
    *spec
}

/// The canvas's Chart row opens Reference lines…; a band added there is
/// saved with the graph, drawn behind the points with its edge values, and
/// survives the graph's JSON form.
#[test]
fn canvas_reference_band_from_the_chart_row() {
    let mut gui = graph();
    gui.click(crate::reference_lines_ui::OPEN_LABEL);
    assert!(reference_window_open(&gui));
    click_in_window(&mut gui, "+ Add band");
    let spec = saved_spec(&gui);
    let look = &spec.axes.style.as_ref().expect("a style").reference;
    assert_eq!(look.bands.len(), 1);
    assert_eq!(look.bands[0].orientation, ReferenceOrientation::Horizontal);
    // Drawn first in each panel (behind the points), with its edge labels.
    let doc = &gui.app().documents[gui.app().active];
    let frame = spec.frame(&doc.table, &doc.rows).unwrap();
    let layers = spec.layers(&frame, &doc.rows);
    assert!(matches!(layers[0][0].marks[0], Mark::Span { .. }), "the band is the first layer");
    assert!(gui.painted_text().iter().any(|t| t == "1"), "an edge value is written");
    assert!(gui.shows("y from 0 to 1"), "the band has a key entry");
    // The graph's saved form keeps it.
    let back = app_model::graph_json::graph_from_json(&app_model::graph_json::graph_to_json(&spec)).unwrap();
    assert_eq!(back.axes.style.unwrap().reference, *look);
    gui.shot("reference-band-canvas");
}

/// Plant growth heights by fertilizer with the given layers.
fn heights_by_fertilizer(change: impl FnOnce(&mut stat_viz::GraphSpec)) -> Gui {
    let mut gui = Gui::with_sample("Plant growth");
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    let mut spec = stat_viz::GraphSpec {
        encoding: stat_viz::EncodingPlan {
            x: vec!["Fertilizer".into()],
            y: vec!["Height".into()],
            ..Default::default()
        },
        points: None,
        ..stat_viz::GraphSpec::default()
    };
    change(&mut spec);
    gui.app_mut().add_report(ReportRequest::Graph(Box::new(spec)));
    gui.run();
    gui
}

/// The settings menu ("…") right of the layer toggle `toggle`.
fn open_layer_settings(gui: &mut Gui, toggle: &str) {
    // The layer bar is wider than the default window with these layers on.
    gui.harness.set_size(egui::vec2(1700.0, 1000.0));
    gui.run();
    let b = gui.widget(toggle).raw_bounds().expect("the layer toggle");
    let dots = |n: &Node<'_>| n.role() == Role::Button && n.label().as_deref() == Some("…");
    let menu = gui
        .harness
        .get_all_by(dots)
        .filter_map(|n| n.raw_bounds().map(|r| (r, n)))
        .filter(|(r, _)| r.x0 > b.x0 && (r.y0 - b.y0).abs() < 4.0)
        .min_by(|a, c| a.0.x0.total_cmp(&c.0.x0))
        .expect("the layer's settings")
        .1;
    menu.click();
    gui.run();
}

/// Bracket pairs are picked by ticking them; only those are drawn, and the
/// choice is saved with the graph.
#[test]
fn bracket_pairs_are_ticked() {
    let mut gui = heights_by_fertilizer(|s| s.brackets = Some(stat_viz::BracketElement::default()));
    let doc = &gui.app().documents[gui.app().active];
    let spec = saved_spec(&gui);
    let frame = spec.frame(&doc.table, &doc.rows).unwrap();
    let levels = frame.panels[0].x.levels().to_vec();
    assert!(levels.len() >= 3, "{levels:?}");
    open_layer_settings(&mut gui, "Brackets");
    gui.press("Only the pairs ticked");
    let pair = format!("{} – {}", levels[0], levels[1]);
    gui.press(&pair);
    let spec = saved_spec(&gui);
    let k = spec.brackets.as_ref().unwrap();
    assert_eq!(k.pairs, vec![(levels[0].clone(), levels[1].clone())]);
    let doc = &gui.app().documents[gui.app().active];
    let layers = spec.layers(&frame, &doc.rows);
    let drawn = layers[0]
        .iter()
        .flat_map(|l| &l.marks)
        .filter(|m| matches!(m, Mark::Line { name, .. } if name == "Significance bracket"))
        .count();
    assert_eq!(drawn, 1, "only the ticked pair");
    let back = app_model::graph_json::graph_from_json(&app_model::graph_json::graph_to_json(&spec)).unwrap();
    assert_eq!(back.brackets, spec.brackets);
}

/// Error bars of summary points can reach one way only, in a colour and
/// width of their own; the setting is saved with the graph.
#[test]
fn error_bars_can_reach_one_way() {
    use stat_viz::summary::{ErrorBarDirection, ErrorBars};
    let mut gui = heights_by_fertilizer(|s| {
        s.summary = Some(stat_viz::SummaryElement { error_bars: ErrorBars::StdErr, ..Default::default() })
    });
    open_layer_settings(&mut gui, "Summary");
    gui.press("Upper only");
    let spec = saved_spec(&gui);
    let s = spec.summary.unwrap();
    assert_eq!(s.error_look.direction, ErrorBarDirection::Upper);
    // Each bar starts at its mean: the lowest end of every bar is the mean.
    let doc = &gui.app().documents[gui.app().active];
    let frame = spec.frame(&doc.table, &doc.rows).unwrap();
    let layers = spec.layers(&frame, &doc.rows);
    let means: Vec<f64> = layers[0]
        .iter()
        .flat_map(|l| &l.marks)
        .filter_map(|m| if let Mark::Points { points, .. } = m { Some(points.iter().map(|p| p[1])) } else { None })
        .flatten()
        .collect();
    let bars: Vec<[[f64; 2]; 2]> = layers[0]
        .iter()
        .flat_map(|l| &l.marks)
        .filter_map(|m| match m {
            Mark::Line { points, name, .. } if name.ends_with("error bar") && points[0][0] == points[1][0] => {
                Some([points[0], points[1]])
            }
            _ => None,
        })
        .collect();
    assert_eq!(bars.len(), means.len());
    for (bar, mean) in bars.iter().zip(&means) {
        assert!((bar[0][1] - mean).abs() < 1e-9 && bar[1][1] > *mean, "{bar:?} from {mean}");
    }
    let back = app_model::graph_json::graph_from_json(&app_model::graph_json::graph_to_json(&spec)).unwrap();
    assert_eq!(back.summary.unwrap().error_look, s.error_look);
}
