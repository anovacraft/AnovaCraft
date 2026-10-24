//! Annotations on the graph canvas: text, arrows, lines and rectangles the
//! user places from a panel's right-click menu. Dragging one moves it,
//! double-clicking opens its editor, and a click selects it (or a moved
//! statistics label) so the arrow keys can nudge it: one point per press,
//! ten with Shift. Escape clears the selection; Delete removes a selected
//! annotation.

use eframe::egui::{self, PointerButton, RichText};
use egui_plot::{PlotPoint, PlotResponse, PlotTransform};
use stat_viz::render::text_align;
use stat_viz::view::view_fraction;
use stat_viz::{Annotation, AnnotationAnchor, AnnotationShape, Frame, GraphSpec, Layer, PanelPlace};

use crate::theme;

/// Something selected on the canvas for nudging with the arrow keys.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Selected {
    /// The annotation at this index of the graph's annotations.
    Note(usize),
    /// A movable statistics label (fit equations, correlations).
    Label {
        /// Panel index.
        panel: usize,
        /// The label's identifier.
        id: String,
    },
}

/// An annotation being dragged.
#[derive(Debug, Clone)]
struct NoteDrag {
    index: usize,
    /// Where the pointer was pressed (screen).
    origin: egui::Pos2,
    /// The annotation's points when the drag began (plot coordinates).
    start: [[f64; 2]; 2],
    /// Where they are now.
    now: [[f64; 2]; 2],
}

/// Annotation state of a graph canvas.
#[derive(Default)]
pub(super) struct Notes {
    /// What the arrow keys move.
    pub selected: Option<Selected>,
    drag: Option<NoteDrag>,
    /// The annotation whose editor is open.
    pub editing: Option<usize>,
    /// Where a panel was last right-clicked: panel, plot position, and the
    /// annotation under the pointer.
    menu_at: Option<(usize, [f64; 2], Option<usize>)>,
    /// The graph's annotations as changed this frame (put into the graph
    /// once the canvas is drawn).
    pub changed: Option<Vec<Annotation>>,
    /// The view of each panel as last drawn (for the editor's conversions).
    pub views: Vec<([f64; 2], [f64; 2])>,
}

/// Points of the pointer within which a line or arrow is hit.
const REACH: f32 = 5.0;

/// How an annotation sits on screen: its strokes and its text box.
struct OnScreen {
    segments: Vec<[egui::Pos2; 2]>,
    text: Option<egui::Rect>,
}

impl OnScreen {
    fn hit(&self, p: egui::Pos2) -> bool {
        self.text.is_some_and(|r| r.expand(3.0).contains(p))
            || self.segments.iter().any(|[a, b]| distance_to_segment(p, *a, *b) <= REACH)
    }

    fn bounds(&self) -> Option<egui::Rect> {
        let mut out: Option<egui::Rect> = self.text;
        for [a, b] in &self.segments {
            let r = egui::Rect::from_two_pos(*a, *b);
            out = Some(out.map_or(r, |o| o.union(r)));
        }
        out
    }
}

fn distance_to_segment(p: egui::Pos2, a: egui::Pos2, b: egui::Pos2) -> f32 {
    let ab = b - a;
    let t = if ab.length_sq() > 0.0 { ((p - a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0) } else { 0.0 };
    (a + ab * t).distance(p)
}

fn screen(transform: &PlotTransform, p: [f64; 2]) -> egui::Pos2 {
    transform.position_from_point(&PlotPoint::new(p[0], p[1]))
}

fn plot(transform: &PlotTransform, p: egui::Pos2) -> [f64; 2] {
    let v = transform.value_from_position(p);
    [v.x, v.y]
}

/// Where `note`, drawn at plot points `points`, sits on screen.
fn on_screen(ui: &egui::Ui, transform: &PlotTransform, note: &Annotation, points: [[f64; 2]; 2]) -> OnScreen {
    let [a, b] = points.map(|p| screen(transform, p));
    let segments = match note.shape {
        AnnotationShape::Text => Vec::new(),
        AnnotationShape::Arrow | AnnotationShape::Line => vec![[a, b]],
        AnnotationShape::Rectangle => {
            let (c, d) = (egui::pos2(b.x, a.y), egui::pos2(a.x, b.y));
            vec![[a, c], [c, b], [b, d], [d, a]]
        }
    };
    let text = note.text.trim_end();
    let text = (!text.is_empty() && matches!(note.shape, AnnotationShape::Text | AnnotationShape::Arrow)).then(|| {
        // egui_plot writes text in the small text style.
        let font = egui::TextStyle::Small.resolve(ui.style());
        let size = ui.fonts(|f| f.layout_no_wrap(text.to_owned(), font, egui::Color32::WHITE)).size();
        text_align(note.text_anchor(points)).anchor_size(a, size)
    });
    OnScreen { segments, text }
}

impl Notes {
    /// The graph's annotations as they stand this frame.
    pub fn items(&self, spec: &GraphSpec) -> Vec<Annotation> {
        self.changed.clone().unwrap_or_else(|| spec.annotations().to_vec())
    }

    /// The annotations of panel `index` as drawn now (one being dragged
    /// where the pointer has it).
    pub fn layer(&self, spec: &GraphSpec, index: usize, place: &PanelPlace<'_>) -> Layer {
        let mut items = self.items(spec);
        if let Some(d) = &self.drag {
            if let Some(n) = items.get_mut(d.index) {
                n.set_plot_points(d.now, place);
            }
        }
        stat_viz::annotation_layer(&items, index, place)
    }

    /// Forget the selection, drag and editor (the annotations changed under
    /// them).
    fn forget(&mut self) {
        self.selected = None;
        self.drag = None;
        self.editing = None;
    }

    /// Pointer and keyboard handling for panel `index`: dragging, clicking
    /// and double-clicking annotations, selecting movable labels (`labels`:
    /// the panel's layers with labels placed) and nudging the selection.
    /// Returns `true` when the pointer acted on an annotation or label, so
    /// it does not also select rows or reset the view.
    #[allow(clippy::too_many_arguments)]
    pub fn gesture(
        &mut self,
        ui: &egui::Ui,
        response: &PlotResponse<()>,
        spec: &GraphSpec,
        index: usize,
        place: &PanelPlace<'_>,
        labels: &[Layer],
        label_dropped: &mut Option<(usize, String, [f64; 2])>,
    ) -> bool {
        if self.views.len() <= index {
            self.views.resize(index + 1, ([0.0, 1.0], [0.0, 1.0]));
        }
        self.views[index] = place.view;
        let r = &response.response;
        let transform = &response.transform;
        let mut items = self.items(spec);
        let shapes: Vec<(usize, OnScreen)> = items
            .iter()
            .enumerate()
            .filter(|(_, n)| n.panel == index)
            .map(|(i, n)| (i, on_screen(ui, transform, n, n.plot_points(place))))
            .collect();
        let under = |p: egui::Pos2| shapes.iter().rev().find(|(_, s)| s.hit(p)).map(|(i, _)| *i);
        let label_under = |p: egui::Pos2| {
            super::label_boxes(ui, transform, labels)
                .into_iter()
                .rev()
                .find(|(_, rect, _)| rect.expand(3.0).contains(p))
        };
        let mut owned = false;

        // Dragging an annotation.
        if r.drag_started_by(PointerButton::Primary) {
            if let Some((origin, i)) = ui.input(|inp| inp.pointer.press_origin()).and_then(|o| under(o).map(|i| (o, i)))
            {
                let start = items[i].plot_points(place);
                self.drag = Some(NoteDrag { index: i, origin, start, now: start });
                self.selected = Some(Selected::Note(i));
            }
        }
        if let Some(d) = self.drag.as_mut().filter(|d| items.get(d.index).is_some_and(|n| n.panel == index)) {
            if let Some(p) = r.interact_pointer_pos() {
                let delta = p - d.origin;
                d.now = d.start.map(|q| plot(transform, screen(transform, q) + delta));
            }
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            if r.drag_stopped() || !ui.input(|i| i.pointer.primary_down()) {
                let d = self.drag.take().expect("a drag");
                items[d.index].set_plot_points(d.now, place);
                self.changed = Some(items.clone());
            } else {
                ui.ctx().request_repaint();
            }
            owned = true;
        }
        let pointer = r.interact_pointer_pos().or_else(|| r.hover_pos());
        if !owned && r.double_clicked() {
            if let Some(i) = pointer.and_then(under) {
                self.selected = Some(Selected::Note(i));
                self.editing = Some(i);
                owned = true;
            }
        } else if !owned && r.clicked_by(PointerButton::Primary) {
            if let Some(i) = pointer.and_then(under) {
                self.selected = Some(Selected::Note(i));
                owned = true;
            } else if let Some((id, ..)) = pointer.and_then(label_under) {
                self.selected = Some(Selected::Label { panel: index, id });
                owned = true;
            } else {
                self.selected = None;
            }
        }
        if r.secondary_clicked() {
            self.menu_at = pointer.map(|p| (index, plot(transform, p), under(p)));
        }
        if !owned && r.hover_pos().is_some_and(|p| under(p).is_some()) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }

        // Keys move or remove the selection on this panel.
        let here = match &self.selected {
            Some(Selected::Note(i)) => items.get(*i).is_some_and(|n| n.panel == index),
            Some(Selected::Label { panel, .. }) => *panel == index,
            None => false,
        };
        if here && ui.memory(|m| m.focused().is_none()) {
            let step = |inp: &mut egui::InputState| -> Option<egui::Vec2> {
                let mut v = egui::Vec2::ZERO;
                for (key, d) in [
                    (egui::Key::ArrowLeft, egui::vec2(-1.0, 0.0)),
                    (egui::Key::ArrowRight, egui::vec2(1.0, 0.0)),
                    (egui::Key::ArrowUp, egui::vec2(0.0, -1.0)),
                    (egui::Key::ArrowDown, egui::vec2(0.0, 1.0)),
                ] {
                    // Shift first: a plain pattern also matches a shifted key.
                    for m in [egui::Modifiers::SHIFT, egui::Modifiers::NONE] {
                        if inp.consume_key(m, key) {
                            v += d * if m.shift { 10.0 } else { 1.0 };
                        }
                    }
                }
                (v != egui::Vec2::ZERO).then_some(v)
            };
            if let Some(delta) = ui.ctx().input_mut(step) {
                match self.selected.clone() {
                    Some(Selected::Note(i)) => {
                        let moved = items[i].plot_points(place).map(|q| plot(transform, screen(transform, q) + delta));
                        items[i].set_plot_points(moved, place);
                        self.changed = Some(items.clone());
                    }
                    Some(Selected::Label { id, .. }) => {
                        let at = super::label_boxes(ui, transform, labels)
                            .into_iter()
                            .find(|(l, ..)| *l == id)
                            .map(|(.., at)| at);
                        if let Some(at) = at {
                            let moved = plot(transform, screen(transform, at) + delta);
                            *label_dropped = Some((index, id, view_fraction(moved, place.view)));
                        }
                    }
                    None => {}
                }
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.selected = None;
            }
            if let Some(Selected::Note(i)) = self.selected {
                if ui.input(|inp| inp.key_pressed(egui::Key::Delete) || inp.key_pressed(egui::Key::Backspace)) {
                    items.remove(i);
                    self.changed = Some(items);
                    self.forget();
                }
            }
        }
        owned
    }

    /// Outline the selection on panel `index`.
    pub fn paint_selection(
        &self,
        ui: &egui::Ui,
        transform: &PlotTransform,
        spec: &GraphSpec,
        index: usize,
        place: &PanelPlace<'_>,
        labels: &[Layer],
    ) {
        let rect = match &self.selected {
            Some(Selected::Note(i)) => self.items(spec).get(*i).filter(|n| n.panel == index).and_then(|n| {
                let points = match &self.drag {
                    Some(d) if d.index == *i => d.now,
                    _ => n.plot_points(place),
                };
                on_screen(ui, transform, n, points).bounds()
            }),
            Some(Selected::Label { panel, id }) if *panel == index => {
                super::label_boxes(ui, transform, labels).into_iter().find(|(l, ..)| l == id).map(|(_, r, _)| r)
            }
            _ => None,
        };
        let Some(rect) = rect else { return };
        let rect = rect.expand(4.0);
        let stroke = egui::Stroke::new(1.0_f32, theme::plot_palette(ui).selected);
        let painter = ui.painter_at(transform.frame().expand(8.0));
        let corners = [rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom(), rect.left_top()];
        painter.extend(egui::Shape::dashed_line(&corners, stroke, 4.0, 3.0));
    }

    /// The annotation entries of panel `index`'s right-click menu: add one
    /// where the menu was opened, and edit, delete or reorder the one under
    /// the pointer.
    pub fn menu(&mut self, ui: &mut egui::Ui, spec: &GraphSpec, index: usize, place: &PanelPlace<'_>) {
        let Some((panel, at, under)) = self.menu_at.filter(|m| m.0 == index) else { return };
        let mut items = self.items(spec);
        if let Some(i) = under.filter(|i| *i < items.len()) {
            let what = items[i].shape.label();
            if ui.button(format!("Edit {what}…")).clicked() {
                self.selected = Some(Selected::Note(i));
                self.editing = Some(i);
                ui.close_menu();
            }
            if ui.add_enabled(i + 1 < items.len(), egui::Button::new("Bring forward")).clicked() {
                items.swap(i, i + 1);
                self.changed = Some(items.clone());
                self.forget();
                ui.close_menu();
            }
            if ui.add_enabled(i > 0, egui::Button::new("Send backward")).clicked() {
                items.swap(i, i - 1);
                self.changed = Some(items.clone());
                self.forget();
                ui.close_menu();
            }
            if ui.button(format!("Delete {what}")).clicked() {
                items.remove(i);
                self.changed = Some(items.clone());
                self.forget();
                ui.close_menu();
            }
            ui.separator();
        }
        let ([x0, x1], [y0, y1]) = place.view;
        let (dx, dy) = ((x1 - x0) * 0.15, (y1 - y0) * 0.15);
        for shape in AnnotationShape::ALL {
            if ui.button(format!("Add {}", shape.label())).clicked() {
                let to = match shape {
                    AnnotationShape::Text => at,
                    AnnotationShape::Arrow => [at[0] + dx, at[1] + dy],
                    AnnotationShape::Line | AnnotationShape::Rectangle => [at[0] + dx, at[1] - dy],
                };
                items.push(Annotation::new(shape, panel, at, to, place));
                let i = items.len() - 1;
                self.changed = Some(items.clone());
                self.forget();
                self.selected = Some(Selected::Note(i));
                // New text opens its editor so it can be typed at once.
                if shape == AnnotationShape::Text {
                    self.editing = Some(i);
                }
                ui.close_menu();
            }
        }
        ui.separator();
    }

    /// The editor of the annotation being edited: its text, what its
    /// position is measured in, and Delete.
    pub fn editor(&mut self, ctx: &egui::Context, spec: &GraphSpec, frame: &Frame, id: u64) {
        let Some(i) = self.editing else { return };
        let mut items = self.items(spec);
        let Some(note) = items.get(i).cloned() else {
            self.editing = None;
            return;
        };
        let mut open = true;
        let mut edited = note.clone();
        let mut delete = false;
        let mut done = false;
        egui::Window::new(format!("Edit {}", note.shape.label()))
            .id(egui::Id::new(("graph_annotation_editor", id)))
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                if matches!(note.shape, AnnotationShape::Text | AnnotationShape::Arrow) {
                    ui.label(if note.shape == AnnotationShape::Arrow { "Text at the tail:" } else { "Text:" });
                    let text = ui.add(
                        egui::TextEdit::multiline(&mut edited.text)
                            .desired_rows(2)
                            .desired_width(260.0)
                            .hint_text("Math markup: x_{1}, x^{2}, \\alpha"),
                    );
                    text.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Annotation text"));
                    if ui.memory(|m| m.focused().is_none()) && note.text.is_empty() {
                        text.request_focus();
                    }
                }
                ui.label("Position measured in:");
                let place = frame.panels.get(note.panel).map(|p| {
                    let view = self.views.get(note.panel).copied().unwrap_or(([0.0, 1.0], [0.0, 1.0]));
                    spec.panel_place(p, view)
                });
                for anchor in AnnotationAnchor::ALL {
                    let chosen = edited.anchor == anchor;
                    if ui.add_enabled(place.is_some(), egui::RadioButton::new(chosen, anchor.label())).clicked()
                        && !chosen
                    {
                        if let Some(place) = &place {
                            edited.set_anchor(anchor, place);
                        }
                    }
                }
                ui.label(RichText::new("Drag it to move it; select it and use the arrow keys to nudge it.").weak());
                ui.horizontal(|ui| {
                    delete = ui.button("Delete").clicked();
                    done = ui.button("Done").clicked();
                });
            });
        if delete {
            items.remove(i);
            self.changed = Some(items);
            self.forget();
            return;
        }
        if edited != note {
            items[i] = edited;
            self.changed = Some(items.clone());
        }
        if done || !open {
            self.editing = None;
            // Text left empty is dropped.
            if items[i].shape == AnnotationShape::Text && items[i].text.trim().is_empty() {
                items.remove(i);
                self.changed = Some(items);
                self.forget();
            }
        }
    }
}
