//! Plots: navigation and the Plot settings menu.

use super::*;

/// Longley with its own reports closed and a graph canvas of Employed against GNP.
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

/// The Plot settings menu's range fields: (id, value) of x min, x max, y min, y max.
fn range_fields(gui: &Gui) -> Vec<(egui::accesskit::NodeId, f64)> {
    let is_range = |n: &Node<'_>| {
        n.role() == Role::SpinButton && n.value().is_some_and(|v| v.starts_with("min ") || v.starts_with("max "))
    };
    gui.harness.query_all_by(is_range).map(|n| (n.id(), n.numeric_value().unwrap_or(f64::NAN))).collect()
}

fn close_menus(gui: &mut Gui) {
    gui.harness.press_key(egui::Key::Escape);
    gui.run();
}

/// On the graph canvas, ranges typed into Plot settings apply and stay
/// shown as fixed; "Fit view to data" makes them automatic again.
#[test]
fn graph_ranges_can_be_typed_and_fitted_again() {
    let mut gui = graph();
    gui.click("Plot settings ▾");
    let fields = range_fields(&gui);
    assert_eq!(fields.len(), 4, "x and y ranges, each min and max");
    let (x_min, auto_min) = fields[0];
    assert!(auto_min < 234_289.0, "the automatic range covers the smallest GNP");
    assert!(!gui.shows("fixed"));
    gui.set_number(x_min, 300_000.0);
    assert!((range_fields(&gui)[0].1 - 300_000.0).abs() < 1e-6, "the typed minimum is kept");
    assert!(gui.shows("fixed"));
    gui.shot("graph-range-typed");
    // The button sits below the menu's scrolled part.
    gui.press("Fit view to data");
    if range_fields(&gui).is_empty() {
        gui.click("Plot settings ▾");
    }
    let fitted = range_fields(&gui)[0].1;
    assert!((fitted - auto_min).abs() < 1e-6 * auto_min, "back to the automatic range");
    assert!(!gui.shows("fixed"));
}

/// Ranges are in drawn units, so a new axis scale drops a typed range
/// rather than applying it to the logarithms.
#[test]
fn graph_scale_change_forgets_typed_ranges() {
    let mut gui = graph();
    gui.click("Plot settings ▾");
    let x_min = range_fields(&gui)[0].0;
    gui.set_number(x_min, 300_000.0);
    assert!(gui.shows("fixed"));
    close_menus(&mut gui);
    // The canvas's drop-down lists: chart form options first, then the two scales.
    let combos = gui.harness.query_all_by_role(Role::ComboBox).count();
    assert!(combos >= 2);
    gui.harness.get_all_by_role(Role::ComboBox).nth(1).unwrap().simulate_click();
    gui.run();
    gui.click("Log (base 10)");
    gui.click("Plot settings ▾");
    assert!(!gui.shows("fixed"), "the typed range was dropped");
    let x = range_fields(&gui);
    // log10 of GNP (about 234 000 to 554 000).
    assert!(x[0].1 > 5.0 && x[0].1 < 5.4 && x[1].1 > 5.7 && x[1].1 < 5.8, "{x:?}");
}

/// The screen rectangle of the control labelled `label`.
fn rect_of(gui: &Gui, label: &str) -> egui::Rect {
    let b = gui.widget(label).raw_bounds().expect("the control has a position");
    egui::Rect::from_min_max(egui::pos2(b.x0 as f32, b.y0 as f32), egui::pos2(b.x1 as f32, b.y1 as f32))
}

/// The plot's limits as Plot settings shows them (the ones asked for once
/// set, which equal scale can widen when drawing): x min, x max, y min, y max.
fn set_ranges(gui: &mut Gui) -> [f64; 4] {
    gui.click("Plot settings ▾");
    let fields = range_fields(gui);
    close_menus(gui);
    assert_eq!(fields.len(), 4);
    [fields[0].1, fields[1].1, fields[2].1, fields[3].1]
}

/// With equal scale an axis drag rescales the other axis too, and a drag
/// starts from what is drawn, so the grabbed value stays under the pointer;
/// it used to start from the narrower limits asked for, and the axis moved
/// slower than the pointer.
#[test]
fn equal_scale_axis_drag_starts_from_the_drawn_view() {
    let mut gui = Gui::with_sample("Longley");
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    // GNP (about 234 000 to 554 000) against Employed (about 60 000 to 71 000).
    gui.app_mut().add_report(ReportRequest::Bivariate(app_model::BivariateRequest::new(2, 0)));
    gui.run();
    gui.click("Plot settings ▾");
    gui.press("Equal scale (one unit is the same length on both axes)");
    close_menus(&mut gui);
    // Stretch the x axis by dragging its upper end: the y range is rescaled
    // with it, so the limits asked for already keep one unit the same length.
    let x = rect_of(&gui, "Horizontal axis ruler");
    let y_mid = x.center().y;
    gui.drag(egui::pos2(x.right() - 0.05 * x.width(), y_mid), egui::pos2(x.right() - 0.3 * x.width(), y_mid));
    let asked = set_ranges(&mut gui);
    let y = rect_of(&gui, "Vertical axis ruler");
    // One unit is the same length on both axes, so the drawn y range follows from the x range.
    let drawn_y_span = (asked[1] - asked[0]) * f64::from(y.height()) / f64::from(x.width());
    let asked_y_span = asked[3] - asked[2];
    assert!((drawn_y_span - asked_y_span).abs() < 0.02 * drawn_y_span, "equal scale in the asked limits: {asked:?}");
    gui.shot("equal-scale-after-first-drag");
    // Shift the y axis a few points from its middle.
    gui.drag(y.center(), y.center() + egui::vec2(0.0, 5.0));
    let shifted = set_ranges(&mut gui);
    let span = shifted[3] - shifted[2];
    assert!((span - drawn_y_span).abs() < 0.01 * drawn_y_span, "the shift starts from the drawn y range: {shifted:?}");
}

/// Pairs of painted texts drawn in the same clip area (one plot, say) that
/// cover each other by more than a fifth of the smaller one.
fn overlapping_text(gui: &Gui) -> Vec<(String, String)> {
    let texts: Vec<(String, egui::Rect, egui::Rect)> = gui
        .painted_text_rects()
        .into_iter()
        .filter(|(t, r, clip)| !t.trim().is_empty() && clip.contains_rect(*r))
        .collect();
    let mut out = Vec::new();
    for (i, a) in texts.iter().enumerate() {
        for b in &texts[i + 1..] {
            // The same text drawn twice (bold by a hair's offset) is not a clash.
            if a.2 != b.2 || a.0 == b.0 {
                continue;
            }
            let both = a.1.intersect(b.1);
            let smaller = a.1.area().min(b.1.area());
            if both.is_positive() && both.area() > 0.2 * smaller {
                out.push((format!("{} at {:?}", a.0, a.1), format!("{} at {:?}", b.0, b.1)));
            }
        }
    }
    out
}

/// Every report of a sample opened in a tall window, light or dark.
fn sample_results(name: &str) -> Gui {
    let mut gui = Gui::with_sample(name);
    gui.harness.set_size(egui::vec2(1500.0, 6000.0));
    gui.results();
    gui
}

/// Survival curve end labels, run-rule numbers, Pareto percentages and
/// correspondence-map names were drawn where they fell, on top of each other
/// (and the Kaplan–Meier end label on the axis). They now move apart or are
/// left out.
#[test]
fn plot_labels_do_not_overlap() {
    for sample in ["Leukemia", "Solder defects", "Tablet hardness", "Customer survey", "Coil thickness"] {
        let mut gui = sample_results(sample);
        let clashes = overlapping_text(&gui);
        assert!(clashes.is_empty(), "{sample}: {clashes:?}");
        gui.shot(&format!("labels-{}", sample.to_lowercase().replace(' ', "-")));
    }
}

/// The correlation heat map's key caption ran past the end of the drawing
/// and was cut off.
#[test]
fn heat_map_key_caption_is_whole() {
    let gui = sample_results("Flower measurements");
    let caption = gui
        .painted_text_rects()
        .into_iter()
        .find(|(t, ..)| t.ends_with("(value printed in each cell)"))
        .expect("the heat map key");
    assert!(caption.2.contains_rect(caption.1), "caption {:?} clipped by {:?}", caption.1, caption.2);
}

/// Response Explorer panels showed no horizontal tick labels: egui_plot
/// leaves out a label wider than the spacing between ticks, and five ticks
/// across a narrow panel left too little room for any of them.
#[test]
fn response_explorer_panels_label_their_ticks() {
    let mut gui = Gui::with_sample("Longley");
    gui.harness.set_size(egui::vec2(1500.0, 6000.0));
    gui.results();
    let texts = gui.painted_text();
    // Year runs 1947–1962 and GNP about 234 000–555 000.
    assert!(texts.iter().any(|t| t == "1950" || t == "1955" || t == "1960"), "Year ticks");
    assert!(texts.iter().any(|t| t == "300000" || t == "400000" || t == "500000"), "GNP ticks");
    gui.shot("response-explorer-ticks");
}

/// The ternary contour plot was squashed into the plot's shape and carried
/// numeric axes that mean nothing on a simplex; it is now drawn true to
/// shape with no ticks.
#[test]
fn ternary_plot_is_drawn_true_to_shape() {
    let mut gui = sample_results("Yarn elongation");
    // Corner labels sit at (0, −0.04), (1, −0.04) and (½, √3/2 + 0.04).
    let corners: Vec<egui::Pos2> = gui
        .painted_text_rects()
        .into_iter()
        .filter(|(t, ..)| t.ends_with(" = 1"))
        .map(|(_, r, _)| r.center())
        .collect();
    assert_eq!(corners.len(), 3, "{corners:?}");
    let across = (corners[1].x - corners[0].x).abs();
    let up = (corners[0].y - corners[2].y).abs();
    let expected = 3f32.sqrt() / 2.0 + 0.08;
    assert!((up / across - expected).abs() < 0.05, "height/width {} (expected {expected})", up / across);
    gui.shot("ternary-plot");
}

/// Open the Plot settings menu of the report plot that has contour options.
fn open_contour_settings(gui: &mut Gui) {
    let menus = gui.harness.query_all_by_label("Plot settings ▾").count();
    for i in 0..menus {
        let settings = |n: &Node<'_>| n.role() == Role::Button && n.label().as_deref() == Some("Plot settings ▾");
        gui.harness.get_all_by(settings).nth(i).expect("a Plot settings button").simulate_click();
        gui.run();
        if gui.shows("Fill between levels") {
            return;
        }
        close_menus(gui);
    }
    panic!("no plot has contour settings");
}

/// The ternary contour plot is filled with a colour bar; its Plot settings
/// switch the colour scale (kept with the report, so saved in projects) and
/// the plain "Lines only" look, and a saved figure carries each band as one
/// even–odd filled outline in the chosen colours.
#[test]
fn contour_settings_switch_the_colours_and_exports_carry_the_bands() {
    let mut gui = sample_results("Yarn elongation");
    // The colour bar's level values are painted beside the plot.
    let contour = |gui: &Gui| {
        let report = &gui.app().documents[gui.app().active].reports[0];
        let sections = report.for_export().sections.expect("sections");
        sections.into_iter().find(|s| s.has_contours()).expect("a contour plot")
    };
    let map_levels = |s: &app_model::report::ReportSection| match &s.content {
        app_model::report::SectionContent::Plot(p) => p.contour.as_ref().unwrap().drawing(false),
        _ => unreachable!(),
    };
    let drawing = map_levels(&contour(&gui));
    let bar = drawing.bar.as_ref().expect("filled by default");
    let texts = gui.painted_text();
    assert!(bar.labels.iter().all(|l| texts.contains(l)), "colour bar labels {:?}", bar.labels);
    gui.shot("contour-ternary-default");

    open_contour_settings(&mut gui);
    let scale_list = |n: &Node<'_>| n.role() == Role::ComboBox;
    gui.harness.get_all_by(scale_list).last().expect("the colour list").simulate_click();
    gui.run();
    gui.harness.get_by_label("Heat (yellow to red)").simulate_click();
    gui.run();
    let saved = gui.app().documents[gui.app().active].reports[0].saved();
    let app_model::SavedReport::Scoped { scope, .. } = &saved else { panic!("the style is kept with the report") };
    let style = scope.contour.expect("a contour style");
    assert_eq!(style.scale, stat_viz::palette::ColorScale::Heat);
    gui.shot("contour-ternary-heat");

    // The saved figure draws each band as one filled outline in its colour.
    let section = contour(&gui);
    let drawing = map_levels(&section);
    let svg = String::from_utf8(
        app_model::report_figure::section_figure(&section, None, stat_viz::export::VectorFormat::Svg).unwrap(),
    )
    .unwrap();
    assert_eq!(svg.matches("fill-rule=\"evenodd\"").count(), drawing.bands.len());
    for (color, _) in &drawing.bands {
        assert!(svg.contains(&color.with_alpha(1.0).hex()), "band colour {}", color.hex());
    }
    let heat_low = stat_viz::palette::ColorScale::Heat.color(0.0, false);
    let bar = drawing.bar.as_ref().unwrap();
    assert!(bar.colors.iter().all(|c| *c != stat_viz::palette::ColorScale::Sequential.color(0.0, false)));
    assert!(bar.colors[0].0[0] >= heat_low.0[0] - 10, "the low band is pale yellow");

    // "Lines only" leaves no fill, no colour bar and no bands in the file.
    if !gui.shows("Lines only") {
        open_contour_settings(&mut gui);
    }
    gui.press("Lines only");
    let section = contour(&gui);
    let drawing = map_levels(&section);
    assert!(drawing.bands.is_empty() && drawing.bar.is_none() && !drawing.lines.is_empty());
    let svg = String::from_utf8(
        app_model::report_figure::section_figure(&section, None, stat_viz::export::VectorFormat::Svg).unwrap(),
    )
    .unwrap();
    assert!(!svg.contains("evenodd"));
}
