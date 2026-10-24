//! Graph canvas chart forms: pie and donut, radar, polar, wind rose, ternary,
//! waterfall, ridgeline and vector-field charts, and lollipop bars.

use super::*;
use stat_viz::{EncodingPlan, GraphForm, GraphSpec};

/// Sample `sample` with its own reports closed and one graph canvas.
fn canvas(sample: &str, spec: GraphSpec) -> Gui {
    let mut gui = Gui::with_sample(sample);
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui.app_mut().add_report(ReportRequest::Graph(Box::new(spec)));
    gui.run();
    // The results alone, so the plot's labels are not confused with cells.
    gui.results();
    gui
}

fn spec(form: GraphForm, x: &[&str], y: &[&str], color: Option<&str>) -> GraphSpec {
    GraphSpec {
        encoding: EncodingPlan {
            x: x.iter().map(|s| (*s).to_owned()).collect(),
            y: y.iter().map(|s| (*s).to_owned()).collect(),
            color: color.map(str::to_owned),
            ..Default::default()
        },
        form,
        ..GraphSpec::default()
    }
}

/// The graph's settings as the app holds them.
fn graph_spec(gui: &Gui) -> GraphSpec {
    let saved = gui.app().documents[gui.app().active].reports[0].saved();
    let app_model::SavedReport::Graph { spec } = saved else { panic!("a graph") };
    *spec
}

/// Choose `label` in the canvas's chart-form list (its first drop-down).
fn choose_form(gui: &mut Gui, label: &str) {
    // The list's entry (selectable, so toggled), pressed through the
    // accessibility tree in case the list scrolled it out of sight.
    let entry = |n: &Node<'_>| n.role() != Role::Label && n.toggled().is_some() && n.label().as_deref() == Some(label);
    gui.harness.get_all_by_role(Role::ComboBox).next().expect("the chart chooser").simulate_click();
    gui.run();
    gui.harness.get_by(entry).click();
    gui.run();
    // Pressing an entry this way leaves the list open: close it.
    if gui.harness.query_by(entry).is_some() {
        gui.harness.press_key(egui::Key::Escape);
        gui.run();
    }
}

/// A click at `at` (screen points).
fn click_at(gui: &mut Gui, at: egui::Pos2) {
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    gui.harness.input_mut().events.push(egui::Event::PointerMoved(at));
    gui.harness.step();
    gui.harness.input_mut().events.push(button(at, true));
    gui.harness.step();
    gui.harness.input_mut().events.push(button(at, false));
    gui.run();
}

/// The centre of the last-painted text whose first line is `first` (the
/// plot is drawn after the sidebar and the graph's shelves).
fn text_centre(gui: &Gui, first: &str) -> egui::Pos2 {
    gui.painted_text_rects()
        .into_iter()
        .rfind(|(t, ..)| t.lines().next() == Some(first))
        .unwrap_or_else(|| panic!("no text {first:?} in {:?}", gui.painted_text()))
        .1
        .center()
}

/// Every new form can be chosen from the canvas, shows its own settings and
/// draws its own labels.
#[test]
fn chart_chooser_offers_the_new_forms() {
    let start = spec(GraphForm::Panels, &["Wind direction (°)"], &["Wind speed (m/s)"], None);
    let mut gui = canvas("Harbour weather", start);
    for (label, setting, drawn) in [
        ("Wind rose", "rings in %", "NE"),
        ("Polar chart", "join in row order", "90°"),
        ("Pie chart", "donut", "Total"),
    ] {
        choose_form(&mut gui, label);
        assert_eq!(graph_spec(&gui).form.label(), label);
        assert!(gui.shows(setting), "{label}: its settings are shown");
        if label == "Pie chart" {
            gui.click("donut");
        }
        let texts = gui.painted_text();
        assert!(texts.iter().any(|t| t.lines().next() == Some(drawn)), "{label}: {texts:?}");
    }
    gui.shot("graph-pie-chart");
    // Forms whose shelves do not fit say what they need.
    choose_form(&mut gui, "Ternary plot");
    assert!(gui.painted_text().iter().any(|t| t.contains("three continuous component columns")));
    for label in ["Radar chart", "Waterfall chart", "Ridgeline plot", "Vector field"] {
        choose_form(&mut gui, label);
        assert_eq!(graph_spec(&gui).form.label(), label);
    }
}

/// Clicking a donut slice selects its rows, and the slice is ringed and
/// says how many are selected.
#[test]
fn clicking_a_slice_selects_its_rows() {
    let donut = stat_viz::PieSettings { donut: true, ..stat_viz::PieSettings::default() };
    let mut gui = canvas("Harbour weather", spec(GraphForm::Pie(donut), &["Sky"], &[], None));
    let centre = text_centre(&gui, "Total");
    let label = text_centre(&gui, "Clear");
    // Slice labels sit at 1.3 rim radii; the ring runs from 0.55 to 1.
    let at = centre + (label - centre) * (0.78 / 1.3);
    click_at(&mut gui, at);
    let doc = &gui.app().documents[gui.app().active];
    let sky = doc.table.column(doc.table.find("Sky").unwrap()).unwrap();
    let clear: Vec<usize> = (0..doc.table.n_rows()).filter(|&r| sky.cell_text(r) == "Clear").collect();
    assert!(!clear.is_empty());
    assert_eq!(doc.rows.selected_rows(), clear);
    assert!(gui.painted_text().iter().any(|t| t.ends_with(&format!("{} selected", clear.len()))));
    gui.shot("graph-donut-selected");
}

/// The ternary plot is drawn true to shape: its corner labels are as far
/// apart across as an equilateral triangle needs.
#[test]
fn ternary_plot_keeps_its_triangle() {
    let ternary = spec(GraphForm::Ternary, &["Sand (%)", "Silt (%)"], &["Clay (%)"], Some("Field"));
    let mut gui = canvas("Soil texture", ternary);
    // Corner labels at (−0.06, −0.08), (1.06, −0.08) and (½, √3/2 + 0.07).
    let (a, b, c) = (text_centre(&gui, "Sand (%)"), text_centre(&gui, "Silt (%)"), text_centre(&gui, "Clay (%)"));
    let across = b.x - a.x;
    let up = a.y - c.y;
    let expected = (3f32.sqrt() / 2.0 + 0.15) / 1.12;
    assert!((up / across - expected).abs() < 0.05, "height/width {} (expected {expected})", up / across);
    gui.shot("graph-ternary");
}

/// A wind rose's sectors select their rows when clicked, and a waterfall's
/// bars do too; the waterfall labels its steps with their signs.
#[test]
fn wind_rose_and_waterfall_select_by_clicking() {
    let rose = GraphForm::WindRose(stat_viz::WindRoseSettings::default());
    let mut gui = canvas("Harbour weather", spec(rose, &["Wind direction (°)"], &["Wind speed (m/s)"], None));
    // The pieces cluster around the south-west; click halfway out towards SW.
    let (n, s) = (text_centre(&gui, "N"), text_centre(&gui, "S"));
    let centre = n.lerp(s, 0.5);
    let sw = text_centre(&gui, "SW");
    click_at(&mut gui, centre + (sw - centre) * (0.15 / 1.1));
    let selected = gui.app().documents[gui.app().active].rows.counts().selected;
    assert!(selected > 0, "a south-westerly piece was clicked");
    gui.shot("graph-wind-rose");

    let bridge = GraphForm::Waterfall(stat_viz::WaterfallSettings::default());
    let mut gui = canvas("Household budget", spec(bridge, &["Item"], &["Amount"], None));
    let texts = gui.painted_text();
    assert!(texts.iter().any(|t| t == "+9750"), "salary over the quarter: {texts:?}");
    assert!(texts.iter().any(|t| t == "−3750"), "rent");
    // Click the middle of the rent bar, just under its label.
    let rent = text_centre(&gui, "−3750");
    click_at(&mut gui, rent + egui::vec2(0.0, 18.0));
    let doc = &gui.app().documents[gui.app().active];
    let item = doc.table.column(doc.table.find("Item").unwrap()).unwrap();
    let rows: Vec<usize> = (0..doc.table.n_rows()).filter(|&r| item.cell_text(r) == "Rent").collect();
    assert_eq!(doc.rows.selected_rows(), rows);
    gui.shot("graph-waterfall");
}

/// Ridgelines label their groups on the vertical axis, a radar chart its
/// spokes, and lollipops come from the bar layer's layout list.
#[test]
fn ridgeline_radar_and_lollipops_draw() {
    let ridges = GraphForm::Ridgeline(stat_viz::RidgelineSettings::default());
    let gui = canvas("Harbour weather", spec(ridges, &["Temperature (°C)"], &["Time of day"], Some("Time of day")));
    let texts = gui.painted_text();
    for level in ["Night", "Morning", "Afternoon", "Evening"] {
        assert!(texts.iter().any(|t| t == level), "{level}: {texts:?}");
    }
    let radar = GraphForm::Radar(stat_viz::RadarSettings { group_means: true, fill: true });
    let gui = canvas(
        "Harbour weather",
        spec(radar, &["Wind speed (m/s)", "Temperature (°C)"], &["Humidity (%)"], Some("Time of day")),
    );
    let texts = gui.painted_text();
    assert!(texts.iter().any(|t| t.starts_with("Humidity (%)\n")), "spoke names with ranges: {texts:?}");
    let bars = GraphSpec {
        bars: Some(stat_viz::BarElement::default()),
        points: None,
        ..spec(GraphForm::Panels, &["Item"], &["Amount"], None)
    };
    let mut gui = canvas("Household budget", bars);
    // The bar layer's layout list is in its settings menu (the … right of
    // the Bars toggle).
    let bars_at = gui.widget("Bars").raw_bounds().expect("the Bars toggle").x0;
    let dots = |n: &Node<'_>| n.role() == Role::Button && n.label().as_deref() == Some("…");
    let menu = gui
        .harness
        .get_all_by(dots)
        .filter_map(|n| n.raw_bounds().map(|b| (b.x0, n)))
        .filter(|(x, _)| *x > bars_at)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .expect("the bars' settings")
        .1;
    menu.simulate_click();
    gui.run();
    gui.press("Lollipops (stem and dot)");
    let layout = graph_spec(&gui).bars.map(|b| b.layout);
    assert_eq!(layout, Some(stat_viz::BarLayout::Lollipop));
}
