//! Graph canvas formatting: annotations placed from a panel's right-click
//! menu (dragged, nudged with the arrow keys, edited with a double-click),
//! the layers' drawing order, and the key editor.

use super::*;
use stat_viz::{Annotation, AnnotationAnchor, AnnotationShape, ElementKind, GraphSpec};

/// Sample `name` with its reports replaced by one graph canvas of `spec`,
/// on the results view.
fn canvas(name: &str, spec: GraphSpec) -> Gui {
    let mut gui = Gui::with_sample(name);
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui.app_mut().add_report(ReportRequest::Graph(Box::new(spec)));
    gui.harness.set_size(egui::vec2(1400.0, 1100.0));
    gui.results();
    gui
}

/// Employed against GNP (Longley), changed by `change`.
fn longley(change: impl FnOnce(&mut GraphSpec)) -> Gui {
    let mut spec = GraphSpec {
        encoding: stat_viz::EncodingPlan { x: vec!["GNP".into()], y: vec!["Employed".into()], ..Default::default() },
        points: Some(stat_viz::PointsElement::default()),
        ..GraphSpec::default()
    };
    change(&mut spec);
    canvas("Longley", spec)
}

/// The graph's settings as its report saves them.
fn spec(gui: &Gui) -> GraphSpec {
    let app = gui.app();
    let app_model::SavedReport::Graph { spec } = app.documents[app.active].reports[0].saved() else {
        panic!("a graph")
    };
    *spec
}

fn rect(node: &Node<'_>) -> egui::Rect {
    let b = node.raw_bounds().expect("the control has a position");
    egui::Rect::from_min_max(egui::pos2(b.x0 as f32, b.y0 as f32), egui::pos2(b.x1 as f32, b.y1 as f32))
}

/// The plot frame of the only panel (between its two axis rulers).
fn plot_frame(gui: &Gui) -> egui::Rect {
    let x = rect(&gui.harness.get_by_label("Horizontal axis ruler"));
    let y = rect(&gui.harness.get_by_label("Vertical axis ruler"));
    egui::Rect::from_min_max(egui::pos2(x.left(), y.top()), egui::pos2(x.right(), x.top()))
}

/// Press `button` at `at` (`clicks` times in a row), then let the UI settle.
fn click_at(gui: &mut Gui, at: egui::Pos2, button: egui::PointerButton, clicks: usize) {
    let event = |pressed| egui::Event::PointerButton { pos: at, button, pressed, modifiers: egui::Modifiers::NONE };
    gui.harness.input_mut().events.push(egui::Event::PointerMoved(at));
    gui.harness.step();
    for _ in 0..clicks {
        gui.harness.input_mut().events.push(event(true));
        gui.harness.input_mut().events.push(event(false));
        gui.harness.step();
    }
    gui.run();
}

fn key(gui: &mut Gui, key: egui::Key, modifiers: egui::Modifiers) {
    gui.harness.input_mut().events.push(egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    });
    gui.run();
}

/// Where text `label` was last painted inside the plot.
fn text_rect(gui: &Gui, label: &str) -> egui::Rect {
    gui.painted_text_rects()
        .into_iter()
        .rfind(|(t, ..)| t == label)
        .unwrap_or_else(|| panic!("{label:?} is drawn: {:?}", gui.painted_text()))
        .1
}

fn note(shape: AnnotationShape, at: [f64; 2], to: [f64; 2], text: &str) -> Annotation {
    Annotation { shape, anchor: AnnotationAnchor::Data, panel: 0, at, to, text: text.into() }
}

/// Text added from the right-click menu opens its editor; what is typed is
/// drawn at the point clicked, anchored to the data, and kept with the graph.
#[test]
fn text_is_added_where_the_plot_was_right_clicked() {
    let mut gui = longley(|_| {});
    let frame = plot_frame(&gui);
    let at = frame.center();
    click_at(&mut gui, at, egui::PointerButton::Secondary, 1);
    gui.click("Add text");
    assert!(gui.shows("Edit text"), "the editor opens");
    gui.harness.get_by_label("Annotation text").type_text("Recession");
    gui.run();
    gui.click("Done");
    let saved = spec(&gui);
    let notes = saved.annotations();
    assert_eq!(notes.len(), 1);
    let n = &notes[0];
    assert_eq!((n.shape, n.anchor, n.text.as_str()), (AnnotationShape::Text, AnnotationAnchor::Data, "Recession"));
    // GNP runs about 234 000–555 000 and Employed 60 000–71 000: the middle of the view.
    assert!(n.at[0] > 300e3 && n.at[0] < 500e3 && n.at[1] > 62e3 && n.at[1] < 70e3, "{:?}", n.at);
    let drawn = text_rect(&gui, "Recession");
    assert!((drawn.left() - at.x).abs() < 6.0 && (drawn.center().y - at.y).abs() < 8.0, "{drawn:?} at {at:?}");
    gui.shot("annotation-text");

    // Text left empty is not kept.
    click_at(&mut gui, frame.left_top() + egui::vec2(60.0, 60.0), egui::PointerButton::Secondary, 1);
    gui.click("Add text");
    gui.click("Done");
    assert_eq!(spec(&gui).annotations().len(), 1);
}

/// Arrows, lines and rectangles are added from the same menu, drawn on the
/// plot and removed from their own menu.
#[test]
fn shapes_are_added_and_deleted_from_the_menu() {
    let mut gui = longley(|_| {});
    let at = plot_frame(&gui).center();
    for (item, shape) in [
        ("Add arrow", AnnotationShape::Arrow),
        ("Add line", AnnotationShape::Line),
        ("Add rectangle", AnnotationShape::Rectangle),
    ] {
        click_at(&mut gui, at, egui::PointerButton::Secondary, 1);
        gui.click(item);
        assert_eq!(spec(&gui).annotations().last().map(|n| n.shape), Some(shape));
    }
    assert_eq!(spec(&gui).annotations().len(), 3);
    // Right-clicking on the shapes' common point offers the top one's entries.
    click_at(&mut gui, at, egui::PointerButton::Secondary, 1);
    gui.click("Delete rectangle");
    let left: Vec<AnnotationShape> = spec(&gui).annotations().iter().map(|n| n.shape).collect();
    assert_eq!(left, [AnnotationShape::Arrow, AnnotationShape::Line]);
}

/// Dragging an annotation moves it; a click selects it and the arrow keys
/// nudge it one point (ten with Shift); Escape clears the selection and
/// Delete removes a selected annotation.
#[test]
fn annotations_drag_and_nudge() {
    let mut gui = longley(|s| {
        s.edit_style(|st| {
            st.annotations.items.push(note(AnnotationShape::Text, [400_000.0, 66_000.0], [0.0, 0.0], "Peak"))
        })
    });
    let before = text_rect(&gui, "Peak");
    let start = spec(&gui).annotations()[0].at;
    gui.drag(before.center(), before.center() + egui::vec2(80.0, 0.0));
    let moved = spec(&gui).annotations()[0].at;
    assert!(moved[0] > start[0] + 10_000.0 && (moved[1] - start[1]).abs() < 100.0, "{start:?} → {moved:?}");
    assert!((text_rect(&gui, "Peak").left() - before.left() - 80.0).abs() < 3.0);
    assert!(gui.app().documents[gui.app().active].rows.counts().selected == 0, "the drag selects no rows");

    // Select, then nudge.
    let here = text_rect(&gui, "Peak");
    click_at(&mut gui, here.center(), egui::PointerButton::Primary, 1);
    key(&mut gui, egui::Key::ArrowRight, egui::Modifiers::NONE);
    key(&mut gui, egui::Key::ArrowUp, egui::Modifiers::SHIFT);
    let nudged = text_rect(&gui, "Peak");
    assert!((nudged.left() - here.left() - 1.0).abs() < 0.6, "{here:?} → {nudged:?}");
    assert!((here.top() - nudged.top() - 10.0).abs() < 0.6, "{here:?} → {nudged:?}");
    gui.shot("annotation-selected");
    key(&mut gui, egui::Key::Escape, egui::Modifiers::NONE);
    key(&mut gui, egui::Key::ArrowRight, egui::Modifiers::NONE);
    assert_eq!(text_rect(&gui, "Peak"), nudged, "nothing moves once the selection is cleared");

    // Selected again, Delete removes it.
    click_at(&mut gui, nudged.center(), egui::PointerButton::Primary, 1);
    key(&mut gui, egui::Key::Delete, egui::Modifiers::NONE);
    assert!(spec(&gui).annotations().is_empty());
}

/// Double-clicking an annotation opens its editor (rather than re-fitting
/// the view); the editor re-anchors it to the plot frame without moving it.
#[test]
fn double_click_edits_an_annotation() {
    let mut gui = longley(|s| {
        s.edit_style(|st| {
            st.annotations.items.push(note(
                AnnotationShape::Arrow,
                [300_000.0, 62_000.0],
                [450_000.0, 68_000.0],
                "Rising",
            ));
        });
    });
    let before = text_rect(&gui, "Rising");
    click_at(&mut gui, before.center(), egui::PointerButton::Primary, 2);
    assert!(gui.shows("Edit arrow"), "the editor opens");
    gui.click("Plot frame (stays in place)");
    gui.click("Done");
    let saved = spec(&gui);
    let n = &saved.annotations()[0];
    assert_eq!(n.anchor, AnnotationAnchor::Frame);
    assert!(n.at.iter().chain(&n.to).all(|v| (0.0..=1.0).contains(v)), "{n:?}");
    assert!((text_rect(&gui, "Rising").center() - before.center()).length() < 1.0, "it stays where it was");
}

/// A statistics label can be selected and nudged like an annotation.
#[test]
fn fit_labels_are_nudged_with_the_arrow_keys() {
    let mut gui = longley(|s| s.fit = Some(stat_viz::FitLineElement::default()));
    let (label, before) = gui
        .painted_text_rects()
        .into_iter()
        .rfind(|(t, ..)| t.contains("R²"))
        .map(|(t, r, _)| (t, r))
        .expect("the fit's label");
    click_at(&mut gui, before.center(), egui::PointerButton::Primary, 1);
    key(&mut gui, egui::Key::ArrowDown, egui::Modifiers::SHIFT);
    let after = text_rect(&gui, &label);
    assert!((after.top() - before.top() - 10.0).abs() < 0.6, "{before:?} → {after:?}");
    assert_eq!(spec(&gui).label_positions.len(), 1, "the place is kept");
}

/// The drawing order menu moves layers forward and back; right-clicking a
/// layer's toggle does the same; the order is kept with the graph.
#[test]
fn layers_move_in_the_drawing_order() {
    let mut gui = canvas(
        "Reading trial",
        GraphSpec {
            encoding: stat_viz::EncodingPlan {
                x: vec!["Programme".into()],
                y: vec!["Score".into()],
                ..Default::default()
            },
            points: Some(stat_viz::PointsElement::default()),
            box_plot: Some(stat_viz::BoxPlotElement::default()),
            ..GraphSpec::default()
        },
    );
    assert_eq!(spec(&gui).layer_kinds(), [ElementKind::BoxPlot, ElementKind::Points]);
    gui.click("Drawing order");
    gui.click("Bring Box plot forward");
    assert_eq!(spec(&gui).layer_kinds(), [ElementKind::Points, ElementKind::BoxPlot]);
    gui.shot("drawing-order");
    let points = gui.harness.get_by(|n| n.role() == Role::Button && n.label().as_deref() == Some("Points"));
    let at = rect(&points).center();
    click_at(&mut gui, at, egui::PointerButton::Secondary, 1);
    gui.click("Bring forward");
    assert_eq!(spec(&gui).layer_kinds(), [ElementKind::BoxPlot, ElementKind::Points]);
    assert_eq!(spec(&gui).layer_order, [ElementKind::BoxPlot, ElementKind::Points]);
}

/// The key editor renames, hides and reorders entries, which the key then
/// shows; Reset key undoes every change.
#[test]
fn key_editor_renames_hides_and_reorders() {
    let mut gui = canvas(
        "Reading trial",
        GraphSpec {
            encoding: stat_viz::EncodingPlan {
                x: vec!["Programme".into()],
                y: vec!["Score".into()],
                color: Some("Tutored".into()),
                ..Default::default()
            },
            points: Some(stat_viz::PointsElement::default()),
            ..GraphSpec::default()
        },
    );
    assert!(gui.painted_text().iter().any(|t| t == "Tutored No"));
    gui.click("Edit key…");
    assert!(gui.shows("Edit key"));
    gui.harness.get_by_label("Text of Tutored Yes").type_text(" (weekly)");
    gui.run();
    gui.click("Move Tutored Yes up");
    gui.click("Show Tutored No");
    let look = spec(&gui).key_look();
    assert_eq!(look.renamed("color/Tutored/Yes"), Some("Tutored Yes (weekly)"));
    assert!(look.is_hidden("color/Tutored/No"));
    assert_eq!(look.order, ["color/Tutored/Yes", "color/Tutored/No"]);
    gui.shot("key-editor");
    gui.click("Done");
    let texts = gui.painted_text();
    assert!(texts.iter().any(|t| t == "Tutored Yes (weekly)"), "{texts:?}");
    assert!(!texts.iter().any(|t| t == "Tutored No"), "the hidden entry is gone");

    gui.click("Edit key…");
    gui.click("Reset key");
    gui.click("Done");
    assert!(spec(&gui).key_look().is_default());
    assert!(gui.painted_text().iter().any(|t| t == "Tutored No"));
}
