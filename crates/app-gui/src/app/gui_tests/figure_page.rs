//! Visualize ▸ Figure page…: plots added from their menus, laid out with
//! panel letters and exported as one file.

use super::*;

/// Right-click the control labelled `label`, then let the UI settle.
pub(super) fn right_click(gui: &mut Gui, label: &str) {
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

/// Scroll the results until the control labelled `label` is in view.
pub(super) fn scroll_into_view(gui: &mut Gui, label: &str) {
    for _ in 0..40 {
        let b = gui.widget(label).raw_bounds().expect("the control has a position");
        let step = b.y0 - 300.0;
        if step.abs() < 200.0 {
            return;
        }
        let events = &mut gui.harness.input_mut().events;
        events.push(egui::Event::PointerMoved(egui::pos2(900.0, 450.0)));
        events.push(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -step.clamp(-300.0, 300.0) as f32),
            modifiers: egui::Modifiers::NONE,
        });
        gui.harness.run_steps(8);
    }
    panic!("could not scroll {label} into view");
}

/// Longley's reports with two of their plots on the figure page.
fn two_panels() -> Gui {
    let mut gui = Gui::with_sample("Longley");
    add_plots(&mut gui, &["Residual Normal Quantile Plot", "Scree Plot"]);
    gui
}

/// Add the report plots headed `plots` to the figure page from their
/// right-click menus, leaving the figure page window open.
pub(super) fn add_plots(gui: &mut Gui, plots: &[&str]) {
    gui.results();
    for &plot in plots {
        scroll_into_view(gui, plot);
        right_click(gui, plot);
        gui.press("Add to figure page");
        assert!(gui.app().figure_page.open, "the window opened");
        // Out of the way of the next plot.
        gui.app_mut().figure_page.open = false;
        gui.run();
    }
    gui.app_mut().figure_page.open = true;
    gui.run();
}

/// The menu item opens an empty page that says how to add plots.
#[test]
fn figure_page_opens_from_the_visualize_menu() {
    let mut gui = Gui::with_sample("Longley");
    gui.menu(&["Visualize", "Figure page…"]);
    assert!(gui.app().figure_page.open);
    assert!(gui.shows("No plots yet. Right-click any plot (or a graph's output menu) and choose Add to figure page."));
    gui.shot("figure-page-empty");
}

/// "Add to figure page" in a plot's right-click menu puts the plot on the
/// page as the next lettered panel and opens the window; the page exports
/// as one file with every plot and its letter, in the chosen format.
#[test]
fn plots_added_from_their_menus_export_as_one_page() {
    let mut gui = two_panels();
    assert_eq!(gui.app().figure_page.panel_names(), ["Residual Normal Quantile Plot", "Scree Plot"]);
    assert!(gui.shows("Added “Scree Plot” as panel B"));
    gui.shot("figure-page-two-panels");

    // SVG: both plots, redrawn for their cells, with their letters.
    gui.choose("Figure page", 2, "SVG");
    let dir = temp_dir("figure-page");
    file_io::test_save_answer::set(Some(dir.join("figure")));
    gui.click("Export page as SVG…");
    assert!(!file_io::test_save_answer::pending(), "the save dialog was asked");
    let svg = std::fs::read_to_string(dir.join("figure.svg")).expect("the page was written");
    for text in ["Residual Normal Quantile Plot", "Scree Plot", ">A</text>", ">B</text>"] {
        assert!(svg.contains(text), "{text} on the page");
    }
    // The default page: two journal columns, 183 mm wide.
    assert!(svg.contains("width=\"518.74pt\""));

    // Lower-case letters in parentheses, without the plots' own titles, on a clear background.
    gui.choose("Figure page", 1, "(a), (b), (c)");
    gui.click("Keep each plot's own title");
    gui.click("Transparent background");
    file_io::test_save_answer::set(Some(dir.join("plain")));
    gui.click("Export page as SVG…");
    let svg = std::fs::read_to_string(dir.join("plain.svg")).unwrap();
    assert!(svg.contains(">(a)</text>") && svg.contains(">(b)</text>"));
    assert!(!svg.contains("Scree Plot"), "no plot titles");
    assert!(!svg.contains("fill=\"#ffffff\""), "no white page");

    // PDF in CMYK.
    gui.choose("Figure page", 2, "PDF");
    gui.click("CMYK colour");
    file_io::test_save_answer::set(Some(dir.join("print")));
    gui.click("Export page as PDF…");
    let pdf = std::fs::read(dir.join("print.pdf")).unwrap();
    let pdf = String::from_utf8_lossy(&pdf);
    assert!(pdf.starts_with("%PDF") && pdf.contains(" k\n") && !pdf.contains(" rg\n"), "CMYK colours only");
    let _ = std::fs::remove_dir_all(dir);
}

/// Panels move, span columns and leave the page from their row of the list.
#[test]
fn panels_reorder_span_and_remove() {
    let mut gui = two_panels();
    gui.click("Move panel 1 later");
    assert_eq!(gui.app().figure_page.panel_names(), ["Scree Plot", "Residual Normal Quantile Plot"]);
    let span = gui.widget("Columns spanned by panel 1").id();
    gui.set_number(span, 2.0);
    // A two-column panel fills its row, so the second moves below it.
    let page = &gui.app().figure_page;
    assert_eq!(page.spans(), [2, 1]);
    let cells = page.layout().cells(&page.spans());
    assert!(cells[1][1] > cells[0][3], "the second panel starts a new row");
    gui.click("Remove panel 2");
    assert_eq!(gui.app().figure_page.panel_names().len(), 1);
    gui.click("Clear page");
    assert!(gui.app().figure_page.panel_names().is_empty());
}

/// A graph canvas joins the page from its options menu and is redrawn at
/// its panel's size.
#[test]
fn graph_canvas_joins_the_page() {
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
    let options = |n: &Node<'_>| n.role() == Role::Button && n.label().as_deref() == Some("⋯ Options");
    gui.harness.get_all_by(options).next().expect("the graph's report").simulate_click();
    gui.run();
    gui.press("Add to figure page");
    let page = &mut gui.app_mut().figure_page;
    assert_eq!(page.panel_names().len(), 1);
    let cell = page.layout().cells(&[1])[0];
    let drawn = page.drawn(0).expect("the graph is drawn");
    assert_eq!((drawn.width, drawn.height), (cell[2] - cell[0], cell[3] - cell[1]), "drawn for its cell");
    assert!(drawn.texts().contains(&"Employed"));
}
