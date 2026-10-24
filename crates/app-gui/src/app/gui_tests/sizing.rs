//! Plot sizing: the Response Explorer's panels share one resize grip, and
//! the 3-D plots have the same grip as the other plots.

use super::*;

/// The screen rectangle of an accessibility node.
fn rect(node: &Node<'_>) -> egui::Rect {
    let b = node.raw_bounds().expect("the control has a position");
    egui::Rect::from_min_max(egui::pos2(b.x0 as f32, b.y0 as f32), egui::pos2(b.x1 as f32, b.y1 as f32))
}

/// Rectangles of every control whose label starts with `prefix`, in order.
fn rects_starting(gui: &Gui, prefix: &str) -> Vec<egui::Rect> {
    gui.harness.get_all_by(|n| n.label().is_some_and(|l| l.starts_with(prefix))).map(|n| rect(&n)).collect()
}

/// The panels of the first Response Explorer (the first row of panels).
fn first_explorer(gui: &Gui) -> Vec<egui::Rect> {
    let all = rects_starting(gui, "Profile of ");
    let top = all.first().expect("a Response Explorer").top();
    all.into_iter().filter(|r| (r.top() - top).abs() < 0.5).collect()
}

/// The first control labelled exactly `label` lying below `y`.
fn first_below(gui: &Gui, label: &str, y: f32) -> egui::Rect {
    gui.harness
        .get_all_by(|n| n.label().as_deref() == Some(label))
        .map(|n| rect(&n))
        .filter(|r| r.top() >= y - 1.0)
        .min_by(|a, b| a.top().total_cmp(&b.top()))
        .unwrap_or_else(|| panic!("no {label} below {y}"))
}

/// Double-click with the primary button at `at`.
fn double_click(gui: &mut Gui, at: egui::Pos2) {
    let button = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    gui.harness.input_mut().events.push(egui::Event::PointerMoved(at));
    gui.harness.step();
    // Each step advances the clock a quarter second, so a click per step
    // keeps the two within the double-click delay.
    for _ in 0..2 {
        gui.harness.input_mut().events.extend([button(true), button(false)]);
        gui.harness.step();
    }
    gui.run();
}

/// Whether `a` and `b` agree within half a point.
fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.5
}

/// Dragging the strip under the Response Explorer changes every panel's
/// height together; its corner changes every panel's width together; a
/// double-click restores the default size.
#[test]
fn explorer_panels_resize_together() {
    let mut gui = Gui::with_sample("Longley");
    gui.harness.set_size(egui::vec2(1500.0, 6000.0));
    gui.results();
    let before = first_explorer(&gui);
    assert!(before.len() >= 3, "the explorer's panels: {before:?}");
    let (w0, h0) = (before[0].width(), before[0].height());
    assert!(before.iter().all(|r| near(r.height(), h0) && near(r.width(), w0)), "{before:?}");
    // One row of panels, so the strip moves every panel by the whole drag.
    assert!(before.iter().all(|r| near(r.top(), before[0].top())), "{before:?}");

    let grip = first_below(&gui, "Size of panels", before[0].bottom());
    gui.drag(grip.center(), grip.center() + egui::vec2(0.0, 80.0));
    let taller = first_explorer(&gui);
    assert_eq!(taller.len(), before.len());
    for (a, b) in before.iter().zip(&taller) {
        assert!((b.height() - a.height() - 80.0).abs() < 1.0, "every panel 80 points taller: {a:?} → {b:?}");
        assert!(near(a.width(), b.width()), "the strip leaves the width: {a:?} → {b:?}");
    }
    apply_color_scheme(&gui.harness.ctx.clone(), ColorScheme::Light);
    gui.run();
    gui.shot("explorer-panels-taller");
    // Dark theme, for review of the handles.
    apply_color_scheme(&gui.harness.ctx.clone(), ColorScheme::Dark);
    gui.run();
    gui.shot("explorer-panels-taller-dark");
    apply_color_scheme(&gui.harness.ctx.clone(), ColorScheme::Light);
    gui.run();

    // The corner: 120 points narrower across the row, shared by its panels.
    let n = taller.len() as f32;
    let grip = first_below(&gui, "Size of panels", taller[0].bottom());
    let corner = egui::pos2(grip.right() - 4.0, grip.center().y);
    gui.drag(corner, corner - egui::vec2(120.0, 0.0));
    let narrower = first_explorer(&gui);
    for (a, b) in taller.iter().zip(&narrower) {
        assert!((a.width() - b.width() - 120.0 / n).abs() < 1.5, "each panel narrower by its share: {a:?} → {b:?}");
        assert!(near(a.height(), b.height()), "a sideways drag keeps the height: {a:?} → {b:?}");
    }

    let grip = first_below(&gui, "Size of panels", narrower[0].bottom());
    double_click(&mut gui, grip.center());
    let reset = first_explorer(&gui);
    for (a, b) in before.iter().zip(&reset) {
        assert!(near(a.height(), b.height()) && near(a.width(), b.width()), "double-click restores: {a:?} → {b:?}");
    }
}

/// The 3-D scatter plot opens no wider than the other x–y plots and has
/// their grip: the corner changes both its width and height, and a
/// double-click restores them. The 3-D surface has the grip too.
#[test]
fn three_d_plots_resize_with_their_grip() {
    let mut gui = Gui::with_sample("Hillside survey");
    gui.harness.set_size(egui::vec2(1400.0, 4000.0));
    gui.results();
    let plot = rect(&gui.widget("Scatter (drag to turn)"));
    let cap = crate::plot_size::MAX_DEFAULT_PLOT_WIDTH * 1.5;
    assert!(plot.width() <= cap, "opens at a moderate width, not the whole pane: {plot:?}");

    let grip = first_below(&gui, "Size of plot", plot.bottom());
    assert!(grip.top() - plot.bottom() < 6.0, "the grip sits right under the plot: {plot:?}, {grip:?}");
    let corner = egui::pos2(grip.right() - 4.0, grip.center().y);
    gui.drag(corner, corner + egui::vec2(-150.0, 90.0));
    let resized = rect(&gui.widget("Scatter (drag to turn)"));
    assert!((resized.height() - plot.height() - 90.0).abs() < 1.0, "{plot:?} → {resized:?}");
    assert!((plot.width() - resized.width() - 150.0).abs() < 1.0, "{plot:?} → {resized:?}");
    apply_color_scheme(&gui.harness.ctx.clone(), ColorScheme::Light);
    gui.run();
    gui.shot("scatter-3d-resized");
    // Dark theme, for review of the handles.
    apply_color_scheme(&gui.harness.ctx.clone(), ColorScheme::Dark);
    gui.run();
    gui.shot("scatter-3d-resized-dark");
    apply_color_scheme(&gui.harness.ctx.clone(), ColorScheme::Light);
    gui.run();

    // The width strip beside the plot.
    let edge = first_below(&gui, "Width of plot", resized.top());
    gui.drag(edge.center(), edge.center() + egui::vec2(60.0, 0.0));
    let wider = rect(&gui.widget("Scatter (drag to turn)"));
    assert!((wider.width() - resized.width() - 60.0).abs() < 1.0, "{resized:?} → {wider:?}");
    assert!(near(wider.height(), resized.height()));

    let grip = first_below(&gui, "Size of plot", wider.bottom());
    double_click(&mut gui, grip.center());
    let reset = rect(&gui.widget("Scatter (drag to turn)"));
    assert!(near(reset.width(), plot.width()) && near(reset.height(), plot.height()), "{plot:?} → {reset:?}");

    // Every 3-D surface has its grip directly under it.
    let surfaces = rects_starting(&gui, "Surface (drag to turn)");
    assert!(!surfaces.is_empty());
    for s in surfaces {
        let grip = first_below(&gui, "Size of plot", s.bottom());
        assert!(grip.top() - s.bottom() < 6.0, "{s:?}, {grip:?}");
        assert!(s.width() <= cap, "{s:?}");
    }
}

/// The linked explorer of several responses stacks a row of panels per
/// response; its one grip makes every row taller together, sharing the
/// drag among the rows so the strip follows the pointer.
#[test]
fn linked_explorer_rows_resize_together() {
    let mut gui = Gui::with_sample("Coating formulation");
    gui.harness.set_size(egui::vec2(1500.0, 6000.0));
    gui.results();
    let first = rects_starting(&gui, "Profile of ");
    let top = first.first().expect("the linked explorer").top();
    let grip = first_below(&gui, "Size of panels", top);
    let before: Vec<egui::Rect> = first.into_iter().filter(|r| r.top() < grip.top()).collect();
    let mut tops: Vec<f32> = before.iter().map(|r| r.top()).collect();
    tops.dedup_by(|a, b| (*a - *b).abs() < 0.5);
    let rows = tops.len() as f32;
    assert!(rows >= 2.0, "a row per response: {before:?}");

    gui.drag(grip.center(), grip.center() + egui::vec2(0.0, 90.0));
    let after: Vec<egui::Rect> = rects_starting(&gui, "Profile of ").into_iter().take(before.len()).collect();
    for (a, b) in before.iter().zip(&after) {
        assert!((b.height() - a.height() - 90.0 / rows).abs() < 1.0, "each row shares the drag: {a:?} → {b:?}");
    }
}
