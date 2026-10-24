//! Annotations: text, arrows, lines and rectangles the user places on a
//! plot, kept in the plot's style ([`crate::plot_style::AnnotationLook`]).
//!
//! An annotation is anchored either to the data (its positions are data
//! values, so it moves with the axes and follows a change of scale) or to
//! the plot frame (its positions are fractions of the visible view, 0–1
//! across from the left and up from the bottom, so it stays put while the
//! axes move). Text is read as math markup ([`crate::math_text`]).

use crate::axes::Gap;
use crate::frame::AxisSpec;
use crate::layer::{Layer, LinePattern, Mark, Rgba, TextAnchor};

/// What an annotation draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AnnotationShape {
    /// Text at a point.
    Text,
    /// An arrow from its first point to its second, with optional text at
    /// its tail.
    Arrow,
    /// A straight line between two points.
    Line,
    /// A rectangle with two opposite corners at its points.
    Rectangle,
}

impl AnnotationShape {
    /// Every shape, in menu order.
    pub const ALL: [AnnotationShape; 4] = [Self::Text, Self::Arrow, Self::Line, Self::Rectangle];

    /// Name used in saved files.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Arrow => "arrow",
            Self::Line => "line",
            Self::Rectangle => "rectangle",
        }
    }

    /// The shape with this [`AnnotationShape::key`].
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.key() == key)
    }

    /// Display name (lower case, for "Add …" and "Edit …").
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Arrow => "arrow",
            Self::Line => "line",
            Self::Rectangle => "rectangle",
        }
    }

    /// `true` for the shapes drawn between two points.
    #[must_use]
    pub fn has_end(self) -> bool {
        self != Self::Text
    }
}

/// What an annotation's positions are measured in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AnnotationAnchor {
    /// Data values: the annotation moves with the axes.
    #[default]
    Data,
    /// Fractions of the visible view: the annotation stays in place on the
    /// plot while the axes move.
    Frame,
}

impl AnnotationAnchor {
    /// Both anchors.
    pub const ALL: [AnnotationAnchor; 2] = [Self::Data, Self::Frame];

    /// Name used in saved files.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Data => "data",
            Self::Frame => "frame",
        }
    }

    /// The anchor with this [`AnnotationAnchor::key`].
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.key() == key)
    }

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Data => "Data values (moves with the axes)",
            Self::Frame => "Plot frame (stays in place)",
        }
    }
}

/// One annotation.
#[derive(Debug, Clone, PartialEq)]
pub struct Annotation {
    /// What it draws.
    pub shape: AnnotationShape,
    /// What its positions are measured in.
    pub anchor: AnnotationAnchor,
    /// The panel it sits on (index in the frame; 0 for a single plot).
    pub panel: usize,
    /// Its point: where text starts, an arrow's tail, a line's or a
    /// rectangle's first corner.
    pub at: [f64; 2],
    /// Its second point (an arrow's head, a line's end, a rectangle's
    /// opposite corner); unused by text.
    pub to: [f64; 2],
    /// Text (math markup); for an arrow, written at its tail (may be empty).
    pub text: String,
}

/// Width of annotation lines, in screen points.
pub const ANNOTATION_WIDTH: f32 = 1.5;

/// The color annotations are drawn in (dark ink, light in the dark
/// appearance: see [`crate::palette::for_dark`]).
pub const ANNOTATION_INK: Rgba = crate::palette::LABEL;

/// How a panel's positions map to plot coordinates: its axes (for the scale
/// of data values), axis breaks in drawn units and the visible view in drawn
/// units (after breaks).
#[derive(Debug, Clone)]
pub struct PanelPlace<'a> {
    /// Horizontal axis.
    pub x: &'a AxisSpec,
    /// Vertical axis.
    pub y: &'a AxisSpec,
    /// Break of the horizontal axis.
    pub x_break: Option<Gap>,
    /// Break of the vertical axis.
    pub y_break: Option<Gap>,
    /// The view `([x0, x1], [y0, y1])` in plot coordinates.
    pub view: ([f64; 2], [f64; 2]),
}

/// A data value's drawn position on `axis` (categorical positions and
/// linear values unchanged; outside a scale's domain: NaN).
fn drawn(axis: &AxisSpec, v: f64) -> f64 {
    if axis.is_categorical() { v } else { axis.scale.forward(v).unwrap_or(f64::NAN) }
}

/// The data value drawn at position `t` on `axis`.
fn value(axis: &AxisSpec, t: f64) -> f64 {
    if axis.is_categorical() { t } else { axis.scale.inverse(t) }
}

impl PanelPlace<'_> {
    /// Plot coordinates of the position `p` measured as `anchor` says.
    #[must_use]
    pub fn to_plot(&self, anchor: AnnotationAnchor, p: [f64; 2]) -> [f64; 2] {
        let ([x0, x1], [y0, y1]) = self.view;
        match anchor {
            AnnotationAnchor::Frame => [x0 + p[0] * (x1 - x0), y0 + p[1] * (y1 - y0)],
            AnnotationAnchor::Data => {
                let brk = |t: f64, gap: &Option<Gap>| gap.as_ref().map_or(t, |g| g.clamp(t));
                [brk(drawn(self.x, p[0]), &self.x_break), brk(drawn(self.y, p[1]), &self.y_break)]
            }
        }
    }

    /// The position, measured as `anchor` says, drawn at plot coordinates
    /// `q` (the inverse of [`PanelPlace::to_plot`]).
    #[must_use]
    pub fn from_plot(&self, anchor: AnnotationAnchor, q: [f64; 2]) -> [f64; 2] {
        match anchor {
            AnnotationAnchor::Frame => crate::view::view_fraction(q, self.view),
            AnnotationAnchor::Data => {
                let unbreak = |t: f64, gap: &Option<Gap>| gap.as_ref().map_or(t, |g| g.unmap(t));
                [value(self.x, unbreak(q[0], &self.x_break)), value(self.y, unbreak(q[1], &self.y_break))]
            }
        }
    }
}

impl Annotation {
    /// A new annotation of `shape` on panel `panel` from plot coordinates
    /// `at` to `to`, anchored to the data, with no text yet.
    #[must_use]
    pub fn new(shape: AnnotationShape, panel: usize, at: [f64; 2], to: [f64; 2], place: &PanelPlace<'_>) -> Self {
        let anchor = AnnotationAnchor::Data;
        Self {
            shape,
            anchor,
            panel,
            at: place.from_plot(anchor, at),
            to: place.from_plot(anchor, to),
            text: String::new(),
        }
    }

    /// Its two points in plot coordinates.
    #[must_use]
    pub fn plot_points(&self, place: &PanelPlace<'_>) -> [[f64; 2]; 2] {
        [place.to_plot(self.anchor, self.at), place.to_plot(self.anchor, self.to)]
    }

    /// Put its points at plot coordinates `at` and `to`.
    pub fn set_plot_points(&mut self, [at, to]: [[f64; 2]; 2], place: &PanelPlace<'_>) {
        self.at = place.from_plot(self.anchor, at);
        self.to = place.from_plot(self.anchor, to);
    }

    /// Anchor it as `anchor` says, keeping where it is drawn.
    pub fn set_anchor(&mut self, anchor: AnnotationAnchor, place: &PanelPlace<'_>) {
        let points = self.plot_points(place);
        self.anchor = anchor;
        self.set_plot_points(points, place);
    }

    /// Where its text is placed relative to its point: text starts at the
    /// point, except at an arrow's tail, where it sits on the side away from
    /// the head.
    #[must_use]
    pub fn text_anchor(&self, points: [[f64; 2]; 2]) -> TextAnchor {
        match self.shape {
            AnnotationShape::Arrow if points[1][0] > points[0][0] => TextAnchor::RightOfText,
            _ => TextAnchor::LeftOfText,
        }
    }

    /// The marks it draws, in plot coordinates (nothing when a position is
    /// outside its axis's scale).
    #[must_use]
    pub fn marks(&self, place: &PanelPlace<'_>) -> Vec<Mark> {
        let [a, b] = self.plot_points(place);
        let finite = |p: [f64; 2]| p[0].is_finite() && p[1].is_finite();
        if !finite(a) || (self.shape.has_end() && !finite(b)) {
            return Vec::new();
        }
        let line = |points: Vec<[f64; 2]>| Mark::Line {
            points,
            color: ANNOTATION_INK,
            width: ANNOTATION_WIDTH,
            pattern: LinePattern::Solid,
            name: String::new(),
        };
        let mut out = match self.shape {
            AnnotationShape::Text => Vec::new(),
            AnnotationShape::Arrow => {
                vec![Mark::Arrow { from: a, to: b, color: ANNOTATION_INK, width: ANNOTATION_WIDTH }]
            }
            AnnotationShape::Line => vec![line(vec![a, b])],
            AnnotationShape::Rectangle => vec![line(vec![a, [b[0], a[1]], b, [a[0], b[1]], a])],
        };
        let text = self.text.trim_end();
        if !text.is_empty() && matches!(self.shape, AnnotationShape::Text | AnnotationShape::Arrow) {
            out.push(Mark::Text {
                at: a,
                text: text.to_owned(),
                color: ANNOTATION_INK,
                anchor: self.text_anchor([a, b]),
                movable: None,
            });
        }
        out
    }
}

/// The layer drawing the annotations of panel `panel` from `annotations`,
/// in their order (later ones on top).
#[must_use]
pub fn annotation_layer(annotations: &[Annotation], panel: usize, place: &PanelPlace<'_>) -> Layer {
    Layer {
        marks: annotations.iter().filter(|a| a.panel == panel).flat_map(|a| a.marks(place)).collect(),
        ..Layer::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::AxisKind;
    use crate::scale::AxisScale;

    fn axis(scale: AxisScale) -> AxisSpec {
        AxisSpec { name: "x".into(), kind: AxisKind::Continuous, scale, time: None }
    }

    #[test]
    fn data_anchors_follow_the_scale_and_frame_anchors_the_view() {
        let (x, y) = (axis(AxisScale::Log10), axis(AxisScale::Linear));
        let place = PanelPlace { x: &x, y: &y, x_break: None, y_break: None, view: ([0.0, 3.0], [10.0, 20.0]) };
        // 100 on a log axis is drawn at 2.
        let p = place.to_plot(AnnotationAnchor::Data, [100.0, 15.0]);
        assert!((p[0] - 2.0).abs() < 1e-12 && (p[1] - 15.0).abs() < 1e-12);
        let back = place.from_plot(AnnotationAnchor::Data, p);
        assert!((back[0] - 100.0).abs() < 1e-9);
        // The middle of the view.
        assert_eq!(place.to_plot(AnnotationAnchor::Frame, [0.5, 0.5]), [1.5, 15.0]);
        assert_eq!(place.from_plot(AnnotationAnchor::Frame, [1.5, 15.0]), [0.5, 0.5]);
        // Outside the scale: nothing is drawn.
        let bad = Annotation {
            shape: AnnotationShape::Text,
            anchor: AnnotationAnchor::Data,
            panel: 0,
            at: [-1.0, 12.0],
            to: [0.0, 0.0],
            text: "t".into(),
        };
        assert!(bad.marks(&place).is_empty());
    }

    #[test]
    fn breaks_are_taken_out_and_put_back() {
        let (x, y) = (axis(AxisScale::Linear), axis(AxisScale::Linear));
        let place =
            PanelPlace { x: &x, y: &y, x_break: None, y_break: Some(Gap::gap(AxisScale::Linear, (10.0, 90.0))), view: ([0.0, 1.0], [0.0, 30.0]) };
        let p = place.to_plot(AnnotationAnchor::Data, [0.5, 100.0]);
        assert!((p[1] - 20.0).abs() < 1e-12, "100 drawn 80 lower: {p:?}");
        assert!((place.from_plot(AnnotationAnchor::Data, p)[1] - 100.0).abs() < 1e-12);
    }

    #[test]
    fn shapes_draw_their_marks_and_keep_their_place_when_reanchored() {
        let (x, y) = (axis(AxisScale::Linear), axis(AxisScale::Linear));
        let place = PanelPlace { x: &x, y: &y, x_break: None, y_break: None, view: ([0.0, 10.0], [0.0, 10.0]) };
        let mut arrow = Annotation::new(AnnotationShape::Arrow, 0, [1.0, 1.0], [4.0, 5.0], &place);
        arrow.text = "peak".into();
        let marks = arrow.marks(&place);
        assert!(matches!(marks[0], Mark::Arrow { from: [1.0, 1.0], to: [4.0, 5.0], .. }));
        assert!(matches!(&marks[1], Mark::Text { text, anchor: TextAnchor::RightOfText, .. } if text == "peak"));
        let rect = Annotation::new(AnnotationShape::Rectangle, 0, [1.0, 1.0], [2.0, 3.0], &place);
        let Mark::Line { points, .. } = &rect.marks(&place)[0] else { panic!("a closed outline") };
        assert_eq!(points.len(), 5);
        assert_eq!(points[0], points[4]);
        let text = Annotation::new(AnnotationShape::Text, 1, [2.0, 2.0], [2.0, 2.0], &place);
        assert!(text.text.is_empty());
        assert!(annotation_layer(std::slice::from_ref(&text), 0, &place).marks.is_empty(), "other panels' are left out");
        let mut moved = arrow.clone();
        moved.set_anchor(AnnotationAnchor::Frame, &place);
        assert_eq!(moved.at, [0.1, 0.1]);
        assert_eq!(moved.plot_points(&place), arrow.plot_points(&place));
    }
}
