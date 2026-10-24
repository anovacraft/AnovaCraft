//! Drawable geometry produced by chart elements, independent of any GUI
//! toolkit: point series, lines, shaded bands and text, in data coordinates
//! (already on the axes' scales). The `egui` feature draws them with
//! `egui_plot`; other front ends can draw them however they like.

/// An sRGB color with alpha (not premultiplied).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgba(pub [u8; 4]);

impl Rgba {
    /// Opaque color.
    #[must_use]
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self([r, g, b, 255])
    }

    /// The same color with alpha `a` (0–1).
    #[must_use]
    pub fn with_alpha(self, a: f64) -> Self {
        let [r, g, b, _] = self.0;
        Self([r, g, b, (a.clamp(0.0, 1.0) * 255.0).round() as u8])
    }

    /// `#rrggbb` (opaque) or `#rrggbbaa`.
    #[must_use]
    pub fn hex(self) -> String {
        let [r, g, b, a] = self.0;
        if a == 255 { format!("#{r:02x}{g:02x}{b:02x}") } else { format!("#{r:02x}{g:02x}{b:02x}{a:02x}") }
    }

    /// Read [`Rgba::hex`] (`#rrggbb` or `#rrggbbaa`, case-insensitive).
    #[must_use]
    pub fn from_hex(text: &str) -> Option<Self> {
        let digits = text.strip_prefix('#')?;
        if !(digits.len() == 6 || digits.len() == 8) || !digits.is_ascii() {
            return None;
        }
        let byte = |i: usize| u8::from_str_radix(&digits[i..i + 2], 16).ok();
        let a = if digits.len() == 8 { byte(6)? } else { 255 };
        Some(Self([byte(0)?, byte(2)?, byte(4)?, a]))
    }
}

/// Marker shapes, so groups never rely on color alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MarkShape {
    /// Filled circle.
    Circle,
    /// Square.
    Square,
    /// Diamond.
    Diamond,
    /// Upward triangle.
    Up,
    /// Downward triangle.
    Down,
    /// Plus sign.
    Plus,
    /// Diagonal cross.
    Cross,
    /// Asterisk.
    Asterisk,
}

impl MarkShape {
    /// The shapes in assignment order (filled, easily told apart shapes first).
    pub const CYCLE: [MarkShape; 7] = [
        MarkShape::Circle,
        MarkShape::Square,
        MarkShape::Up,
        MarkShape::Diamond,
        MarkShape::Down,
        MarkShape::Plus,
        MarkShape::Asterisk,
    ];

    /// Every shape, in menu order.
    pub const ALL: [MarkShape; 8] = [
        MarkShape::Circle,
        MarkShape::Square,
        MarkShape::Diamond,
        MarkShape::Up,
        MarkShape::Down,
        MarkShape::Plus,
        MarkShape::Cross,
        MarkShape::Asterisk,
    ];

    /// Name used in saved files.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Circle => "circle",
            Self::Square => "square",
            Self::Diamond => "diamond",
            Self::Up => "triangle_up",
            Self::Down => "triangle_down",
            Self::Plus => "plus",
            Self::Cross => "cross",
            Self::Asterisk => "asterisk",
        }
    }

    /// The shape with this [`MarkShape::key`].
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.key() == key)
    }

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Circle => "Circle",
            Self::Square => "Square",
            Self::Diamond => "Diamond",
            Self::Up => "Triangle (up)",
            Self::Down => "Triangle (down)",
            Self::Plus => "Plus",
            Self::Cross => "Cross",
            Self::Asterisk => "Asterisk",
        }
    }
}

/// Line pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinePattern {
    /// Solid.
    Solid,
    /// Dashed.
    Dashed,
    /// Dotted.
    Dotted,
    /// Alternating dashes and dots.
    DashDot,
}

impl LinePattern {
    /// Every pattern, in menu order.
    pub const ALL: [LinePattern; 4] =
        [LinePattern::Solid, LinePattern::Dashed, LinePattern::Dotted, LinePattern::DashDot];

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Solid => "Solid",
            Self::Dashed => "Dashed",
            Self::Dotted => "Dotted",
            Self::DashDot => "Dash-dot",
        }
    }

    /// Short name for saving.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Solid => "solid",
            Self::Dashed => "dashed",
            Self::Dotted => "dotted",
            Self::DashDot => "dash-dot",
        }
    }

    /// The pattern with this [`LinePattern::key`].
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.key() == key)
    }

    /// Dash and gap lengths in points (empty for solid), for exporters and
    /// painters that draw patterns themselves.
    #[must_use]
    pub fn dashes(self) -> &'static [f64] {
        match self {
            Self::Solid => &[],
            Self::Dashed => &[5.0, 3.0],
            Self::Dotted => &[1.0, 2.5],
            Self::DashDot => &[6.0, 2.5, 1.0, 2.5],
        }
    }
}

/// Where a text label sits relative to its anchor point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextAnchor {
    /// Text ending at the point, just above it (labels at a line's end).
    RightOfText,
    /// Text to the right of the point, vertically centred.
    LeftOfText,
    /// Text centred on the point.
    Centre,
    /// Text centred just above the point (labels on bars).
    Above,
}

/// One drawable item.
#[derive(Debug, Clone, PartialEq)]
pub enum Mark {
    /// A series of markers sharing one style.
    Points {
        /// Positions.
        points: Vec<[f64; 2]>,
        /// Fill (or outline) color.
        color: Rgba,
        /// Shape.
        shape: MarkShape,
        /// Radius in screen points.
        radius: f32,
        /// Filled (otherwise outline only).
        filled: bool,
        /// Name for keys and hover.
        name: String,
    },
    /// A polyline.
    Line {
        /// Vertices.
        points: Vec<[f64; 2]>,
        /// Color.
        color: Rgba,
        /// Width in screen points.
        width: f32,
        /// Pattern.
        pattern: LinePattern,
        /// Name for keys and hover.
        name: String,
    },
    /// A shaded region between two curves over the same x.
    Band {
        /// Lower curve.
        lower: Vec<[f64; 2]>,
        /// Upper curve.
        upper: Vec<[f64; 2]>,
        /// Fill color (usually translucent).
        color: Rgba,
        /// Name for keys and hover.
        name: String,
    },
    /// A filled polygon with an outline (box bodies).
    Polygon {
        /// Vertices.
        points: Vec<[f64; 2]>,
        /// Fill.
        fill: Rgba,
        /// Outline.
        stroke: Rgba,
        /// Name for keys and hover.
        name: String,
    },
    /// A filled area of any shape, with holes and several parts (a band of
    /// a filled contour map), without an outline.
    Area {
        /// The outline: closed rings filled with the even–odd rule (drawn in
        /// vector files and pictures).
        rings: Vec<Vec<[f64; 2]>>,
        /// Convex pieces tiling the same area (drawn on screen, where only
        /// convex shapes fill).
        pieces: Vec<Vec<[f64; 2]>>,
        /// Fill.
        color: Rgba,
        /// Name for keys and hover.
        name: String,
    },
    /// Separate straight segments sharing one style (contour lines).
    Segments {
        /// `[start, end]` of each segment.
        segments: Vec<[[f64; 2]; 2]>,
        /// Color.
        color: Rgba,
        /// Width in screen points.
        width: f32,
        /// Name for keys and hover.
        name: String,
    },
    /// A straight arrow with its head at `to`, the head a fixed size on
    /// screen (annotations).
    Arrow {
        /// Tail.
        from: [f64; 2],
        /// Head.
        to: [f64; 2],
        /// Color.
        color: Rgba,
        /// Width of the shaft in screen points.
        width: f32,
    },
    /// A straight line right across the panel at one value of an axis (a
    /// reference line): horizontal at `at` on the vertical axis, or
    /// vertical at `at` on the horizontal axis when `vertical`. Its label
    /// is written along it inside the plot frame, at the right end of a
    /// horizontal line or the top of a vertical one. When fitting the view
    /// only `at` counts, on its own axis.
    Rule {
        /// Position on the axis the line crosses.
        at: f64,
        /// A vertical line (at a horizontal-axis value).
        vertical: bool,
        /// Color.
        color: Rgba,
        /// Width in screen points (0: the label only).
        width: f32,
        /// Pattern.
        pattern: LinePattern,
        /// Text written along the line (empty for none).
        label: String,
        /// Name for keys and hover.
        name: String,
    },
    /// A shaded stretch right across the panel between two values of an
    /// axis (a reference band): between `from` and `to` on the vertical
    /// axis, or on the horizontal axis when `vertical`. Both ends count
    /// when fitting the view.
    Span {
        /// One end.
        from: f64,
        /// The other end.
        to: f64,
        /// Between horizontal-axis values (a vertical strip).
        vertical: bool,
        /// Fill (usually translucent).
        color: Rgba,
        /// Name for keys and hover.
        name: String,
    },
    /// A text label (may span several lines).
    Text {
        /// Anchor position.
        at: [f64; 2],
        /// Text.
        text: String,
        /// Color.
        color: Rgba,
        /// Placement relative to the anchor.
        anchor: TextAnchor,
        /// Identifier of a label the user can drag elsewhere (e.g. `fit/0`);
        /// `None` for fixed text.
        movable: Option<String>,
    },
}

impl Mark {
    /// Every finite coordinate the mark draws (for fitting the view).
    pub fn coordinates(&self) -> Box<dyn Iterator<Item = [f64; 2]> + '_> {
        let finite = |p: &&[f64; 2]| p[0].is_finite() && p[1].is_finite();
        match self {
            Mark::Points { points, .. } | Mark::Line { points, .. } | Mark::Polygon { points, .. } => {
                Box::new(points.iter().filter(finite).copied())
            }
            Mark::Band { lower, upper, .. } => Box::new(lower.iter().chain(upper).filter(finite).copied()),
            Mark::Area { rings, .. } => Box::new(rings.iter().flatten().filter(finite).copied()),
            Mark::Segments { segments, .. } => Box::new(segments.iter().flatten().filter(finite).copied()),
            Mark::Arrow { from, to, .. } => Box::new([from, to].into_iter().filter(finite).copied()),
            Mark::Text { at, .. } => Box::new(std::iter::once(*at).filter(|p| p[0].is_finite() && p[1].is_finite())),
            // Only the crossed axis's value: the other coordinate is NaN,
            // which range fitting skips.
            Mark::Rule { at, vertical, .. } => Box::new(
                std::iter::once(if *vertical { [*at, f64::NAN] } else { [f64::NAN, *at] }).filter(|p| {
                    p.iter().any(|v| v.is_finite())
                }),
            ),
            Mark::Span { from, to, vertical, .. } => Box::new(
                [*from, *to]
                    .into_iter()
                    .filter(|v| v.is_finite())
                    .map(move |v| if *vertical { [v, f64::NAN] } else { [f64::NAN, v] }),
            ),
        }
    }
}

/// How a key entry is drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum KeyGlyph {
    /// A line in a pattern.
    Line(LinePattern),
    /// A filled area with an outline in the entry's color.
    Area(Rgba),
    /// A marker, filled or hollow.
    Marker(MarkShape, bool),
}

/// One entry of a graph's key contributed by an element.
#[derive(Debug, Clone, PartialEq)]
pub struct KeyEntry {
    /// Text.
    pub label: String,
    /// Color of the line, outline or marker.
    pub color: Rgba,
    /// Glyph.
    pub glyph: KeyGlyph,
}

/// A color ramp an element maps a quantity to (e.g. counts per bin).
#[derive(Debug, Clone, PartialEq)]
pub struct Ramp {
    /// What the colors show.
    pub title: String,
    /// Value at the first end.
    pub min: f64,
    /// Value at the last end.
    pub max: f64,
    /// Colors run from `ramp(1)` at `min` to `ramp(0)` at `max` (light to
    /// dark) instead of `ramp(0)` to `ramp(1)`.
    pub reversed: bool,
}

/// The marks one element draws in one panel, in drawing order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Layer {
    /// Marks, back to front.
    pub marks: Vec<Mark>,
    /// Notes about what could not be drawn (e.g. "a fit needs two distinct x").
    pub notes: Vec<String>,
    /// Entries for the graph's key.
    pub key: Vec<KeyEntry>,
    /// A color ramp the element uses.
    pub ramp: Option<Ramp>,
    /// Areas that stand for groups of rows (bars, cells, tiles): clicking one
    /// selects its rows.
    pub regions: Vec<Region>,
}

/// An area of a layer standing for a set of rows.
#[derive(Debug, Clone, PartialEq)]
pub struct Region {
    /// Closed outline (the last vertex joins the first).
    pub outline: Vec<[f64; 2]>,
    /// Rows the area stands for, ascending.
    pub rows: Vec<usize>,
}

impl Layer {
    /// Add a key entry.
    pub fn key_entry(&mut self, label: impl Into<String>, color: Rgba, glyph: KeyGlyph) {
        self.key.push(KeyEntry { label: label.into(), color, glyph });
    }

    /// `true` if every coordinate of every mark is finite (no NaN reached the
    /// drawing).
    #[must_use]
    pub fn all_finite(&self) -> bool {
        self.marks.iter().all(|m| match m {
            Mark::Points { points, .. } | Mark::Line { points, .. } | Mark::Polygon { points, .. } => {
                points.iter().all(|p| p[0].is_finite() && p[1].is_finite())
            }
            Mark::Band { lower, upper, .. } => lower.iter().chain(upper).all(|p| p[0].is_finite() && p[1].is_finite()),
            Mark::Area { rings, pieces, .. } => {
                rings.iter().chain(pieces).flatten().all(|p| p[0].is_finite() && p[1].is_finite())
            }
            Mark::Segments { segments, .. } => segments.iter().flatten().all(|p| p[0].is_finite() && p[1].is_finite()),
            Mark::Arrow { from, to, .. } => from.iter().chain(to).all(|v| v.is_finite()),
            Mark::Text { at, .. } => at[0].is_finite() && at[1].is_finite(),
            Mark::Rule { at, .. } => at.is_finite(),
            Mark::Span { from, to, .. } => from.is_finite() && to.is_finite(),
        })
    }
}

impl Layer {
    /// Move every coordinate of every mark and region: `point` maps the
    /// positions of markers, text and line vertices (a NaN result is dropped:
    /// the marker or label is not drawn, a line joins across it, as the
    /// exporters draw it), `area`
    /// the vertices of bands, polygons and regions (which must stay closed,
    /// so it should not give NaN). Used for axis breaks and for drawing
    /// right-axis layers in the left axis's coordinates.
    pub fn map_coordinates(&mut self, point: &dyn Fn([f64; 2]) -> [f64; 2], area: &dyn Fn([f64; 2]) -> [f64; 2]) {
        let each = |pts: &mut Vec<[f64; 2]>, f: &dyn Fn([f64; 2]) -> [f64; 2]| {
            for p in pts.iter_mut() {
                *p = f(*p);
            }
        };
        for mark in &mut self.marks {
            match mark {
                Mark::Points { points, .. } => {
                    each(points, point);
                    points.retain(|p| p[0].is_finite() && p[1].is_finite());
                }
                Mark::Line { points, .. } => {
                    each(points, point);
                    points.retain(|p| p[0].is_finite() && p[1].is_finite());
                }
                Mark::Polygon { points, .. } => each(points, area),
                Mark::Area { rings, pieces, .. } => {
                    rings.iter_mut().chain(pieces.iter_mut()).for_each(|r| each(r, area));
                }
                Mark::Band { lower, upper, .. } => {
                    each(lower, area);
                    each(upper, area);
                }
                Mark::Segments { segments, .. } => {
                    for [a, b] in segments.iter_mut() {
                        (*a, *b) = (point(*a), point(*b));
                    }
                    segments.retain(|[a, b]| a.iter().chain(b).all(|v| v.is_finite()));
                }
                Mark::Arrow { from, to, .. } => (*from, *to) = (point(*from), point(*to)),
                Mark::Text { at, .. } => *at = point(*at),
                Mark::Rule { at, vertical, .. } => {
                    *at = if *vertical { point([*at, 0.0])[0] } else { point([0.0, *at])[1] };
                }
                Mark::Span { from, to, vertical, .. } => {
                    for v in [from, to] {
                        *v = if *vertical { area([*v, 0.0])[0] } else { area([0.0, *v])[1] };
                    }
                }
            }
        }
        self.marks.retain(|m| match m {
            Mark::Text { at, .. } => at[0].is_finite() && at[1].is_finite(),
            Mark::Arrow { from, to, .. } => from.iter().chain(to).all(|v| v.is_finite()),
            Mark::Rule { at, .. } => at.is_finite(),
            _ => true,
        });
        for r in &mut self.regions {
            each(&mut r.outline, area);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapping_moves_marks_and_drops_hidden_points() {
        let mut layer = Layer {
            marks: vec![
                Mark::Points {
                    points: vec![[1.0, 1.0], [2.0, 5.0]],
                    color: Rgba::rgb(0, 0, 0),
                    shape: MarkShape::Circle,
                    radius: 3.0,
                    filled: true,
                    name: String::new(),
                },
                Mark::Text {
                    at: [0.0, 5.0],
                    text: "t".into(),
                    color: Rgba::rgb(0, 0, 0),
                    anchor: TextAnchor::Centre,
                    movable: None,
                },
                Mark::Polygon {
                    points: vec![[0.0, 0.0], [0.0, 5.0], [1.0, 5.0]],
                    fill: Rgba::rgb(0, 0, 0),
                    stroke: Rgba::rgb(0, 0, 0),
                    name: String::new(),
                },
            ],
            regions: vec![Region { outline: vec![[0.0, 5.0]], rows: vec![0] }],
            ..Layer::default()
        };
        // y = 5 falls in a gap for points; areas clamp it to 4.
        let point = |p: [f64; 2]| [p[0] * 2.0, if p[1] > 3.0 { f64::NAN } else { p[1] }];
        let area = |p: [f64; 2]| [p[0] * 2.0, p[1].min(4.0)];
        layer.map_coordinates(&point, &area);
        assert_eq!(layer.marks.len(), 2, "the hidden label is dropped");
        assert!(matches!(&layer.marks[0], Mark::Points { points, .. } if points == &vec![[2.0, 1.0]]));
        assert!(matches!(&layer.marks[1], Mark::Polygon { points, .. } if points[1] == [0.0, 4.0]));
        assert_eq!(layer.regions[0].outline, vec![[0.0, 4.0]]);
    }

    #[test]
    fn colors_and_shapes_round_trip() {
        for c in [Rgba::rgb(0, 128, 255), Rgba([1, 2, 3, 4])] {
            assert_eq!(Rgba::from_hex(&c.hex()), Some(c));
        }
        assert_eq!(Rgba::from_hex("#FF0000"), Some(Rgba::rgb(255, 0, 0)));
        for bad in ["", "ff0000", "#ff00", "#gg0000", "#ff00000", "#ééé"] {
            assert_eq!(Rgba::from_hex(bad), None, "{bad}");
        }
        for s in MarkShape::ALL {
            assert_eq!(MarkShape::from_key(s.key()), Some(s));
            assert!(!s.label().is_empty());
        }
        assert!(MarkShape::CYCLE.iter().all(|s| MarkShape::ALL.contains(s)));
        assert_eq!(MarkShape::from_key("star"), None);
    }

    /// A line mapped through an axis break used to keep NaN vertices, which
    /// the screen drew differently from the exporters (they skip them).
    #[test]
    fn mapped_lines_drop_points_that_fall_in_a_gap() {
        let mut layer = Layer::default();
        layer.marks.push(Mark::Line {
            points: vec![[0.0, 0.0], [1.0, 5.0], [2.0, 10.0]],
            color: crate::palette::DEFAULT_POINT,
            width: 1.0,
            pattern: LinePattern::Solid,
            name: String::new(),
        });
        let b = (3.0, 7.0);
        let point = |p: [f64; 2]| [p[0], crate::axes::break_map(p[1], b)];
        let area = |p: [f64; 2]| [p[0], crate::axes::break_clamp(p[1], b)];
        layer.map_coordinates(&point, &area);
        let Mark::Line { points, .. } = &layer.marks[0] else { panic!() };
        assert!(points.iter().all(|p| p[0].is_finite() && p[1].is_finite()), "{points:?}");
        assert_eq!(points.len(), 2, "{points:?}");
        assert!((points[1][1] - 6.0).abs() < 1e-12, "10 drawn 4 units lower: {points:?}");
    }
}
