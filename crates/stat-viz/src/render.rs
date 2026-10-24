//! Drawing [`Layer`]s with `egui_plot` (feature `egui`).

use crate::layer::{Layer, LinePattern, Mark, MarkShape, Rgba, TextAnchor};
use std::ops::RangeInclusive;

use egui::{Align2, Color32, Id, Mesh, RichText, Shape, Stroke, Ui};
use egui_plot::{
    Line, LineStyle, MarkerShape, PlotBounds, PlotGeometry, PlotItem, PlotPoint, PlotPoints, PlotTransform, PlotUi,
    Points, Polygon, Text,
};

/// Opacity of the halo behind points in the dark appearance.
const DARK_GLOW: f32 = 0.25;

/// The egui color of `c`.
#[must_use]
pub fn color32(c: Rgba) -> Color32 {
    let [r, g, b, a] = c.0;
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

/// The egui_plot marker of `s`.
#[must_use]
pub fn marker(s: MarkShape) -> MarkerShape {
    match s {
        MarkShape::Circle => MarkerShape::Circle,
        MarkShape::Square => MarkerShape::Square,
        MarkShape::Diamond => MarkerShape::Diamond,
        MarkShape::Up => MarkerShape::Up,
        MarkShape::Down => MarkerShape::Down,
        MarkShape::Plus => MarkerShape::Plus,
        MarkShape::Cross => MarkerShape::Cross,
        MarkShape::Asterisk => MarkerShape::Asterisk,
    }
}

/// Draw every mark of `layer` into `plot` in the light appearance.
pub fn draw_layer(plot: &mut PlotUi, layer: &Layer) {
    draw_layer_in(plot, layer, false);
}

/// Draw every mark of `layer` into `plot`, in the dark appearance when `dark`:
/// fixed palette colors take their dark counterparts
/// ([`crate::palette::for_dark`]) and filled points get a soft halo.
pub fn draw_layer_in(plot: &mut PlotUi, layer: &Layer, dark: bool) {
    draw_layer_with(plot, layer, dark, false);
}

/// [`draw_layer_in`], reading one-line text labels as math markup when
/// `markup` is set ([`crate::math_text`]).
///
/// A layer with more points than [`crate::point_raster::POINT_BUDGET`] draws
/// all its point marks as one picture ([`PointRaster`]), in place of the
/// first, without halos.
pub fn draw_layer_with(plot: &mut PlotUi, layer: &Layer, dark: bool, markup: bool) {
    let color32 = |c: Rgba| color32(if dark { crate::palette::for_dark(c) } else { c });
    let point_count: usize = layer
        .marks
        .iter()
        .map(|m| if let Mark::Points { points, .. } = m { points.len() } else { 0 })
        .sum();
    let mut raster = (point_count > crate::point_raster::POINT_BUDGET).then(|| {
        let layers = layer
            .marks
            .iter()
            .filter_map(|m| match m {
                Mark::Points { points, color, radius, filled, .. } => Some(RasterPoints {
                    points: points.clone(),
                    color: color32(*color),
                    density: *filled,
                    splat: raster_splat(*radius) + if *filled { 0.0 } else { 1.0 },
                }),
                _ => None,
            })
            .collect();
        PointRaster::new(layers, u64::from(dark))
    });
    for mark in &layer.marks {
        match mark {
            Mark::Points { .. } if point_count > crate::point_raster::POINT_BUDGET => {
                if let Some(item) = raster.take() {
                    plot.add(item);
                }
            }
            Mark::Points { points, color, shape, radius, filled, name } => {
                if dark && *filled {
                    plot.points(
                        Points::new(PlotPoints::from(points.clone()))
                            .shape(marker(*shape))
                            .radius(*radius + 2.5)
                            .color(color32(*color).gamma_multiply(DARK_GLOW)),
                    );
                }
                plot.points(
                    Points::new(PlotPoints::from(points.clone()))
                        .shape(marker(*shape))
                        .radius(*radius)
                        .filled(*filled)
                        .color(color32(*color))
                        .name(name),
                );
            }
            Mark::Line { points, color, width, pattern, name } => {
                let style = match pattern {
                    LinePattern::Solid => LineStyle::Solid,
                    // egui_plot has no dash-dot style; dashes are closest.
                    LinePattern::Dashed | LinePattern::DashDot => LineStyle::dashed_loose(),
                    LinePattern::Dotted => LineStyle::dotted_dense(),
                };
                plot.line(
                    Line::new(PlotPoints::from(points.clone()))
                        .color(color32(*color))
                        .width(*width)
                        .style(style)
                        .name(name),
                );
            }
            Mark::Band { lower, upper, color, name } => {
                plot.add(BandItem {
                    lower: lower.clone(),
                    upper: upper.clone(),
                    color: color32(*color),
                    name: name.clone(),
                });
            }
            Mark::Polygon { points, fill, stroke, name } => {
                plot.polygon(
                    Polygon::new(PlotPoints::from(points.clone()))
                        .fill_color(color32(*fill))
                        .stroke(Stroke::new(1.5_f32, color32(*stroke)))
                        .name(name),
                );
            }
            Mark::Area { pieces, color, name, .. } => {
                let mesh = FillMesh::new([(pieces.as_slice(), color32(*color))]);
                plot.add(fill_item(std::sync::Arc::new(mesh), name));
            }
            Mark::Segments { segments, color, width, name } => {
                plot.add(SegmentsItem {
                    segments: segments.clone(),
                    color: color32(*color),
                    width: *width,
                    name: name.clone(),
                });
            }
            Mark::Arrow { from, to, color, width } => {
                plot.add(ArrowItem { from: *from, to: *to, color: color32(*color), width: *width });
            }
            Mark::Rule { at, vertical, color, width, pattern, label, name } => {
                plot.add(RuleItem {
                    at: *at,
                    vertical: *vertical,
                    color: color32(*color),
                    width: *width,
                    pattern: *pattern,
                    label: label.clone(),
                    name: name.clone(),
                });
            }
            Mark::Span { from, to, vertical, color, name } => {
                plot.add(SpanItem {
                    from: from.min(*to),
                    to: from.max(*to),
                    vertical: *vertical,
                    color: color32(*color),
                    name: name.clone(),
                });
            }
            Mark::Text { at, text, color, anchor, .. } => {
                let math = (markup && !text.contains('\n')).then(|| crate::math_text::MathText::parse(text));
                let label: egui::WidgetText = match math.filter(|m| m.as_plain().is_none()) {
                    Some(m) => {
                        let font = egui::TextStyle::Small.resolve(&plot.ctx().style());
                        let galley = crate::math_text::egui_text::galley(plot.ctx(), &m, &font, color32(*color));
                        egui::WidgetText::Galley(galley)
                    }
                    None => RichText::new(text.as_str()).color(color32(*color)).into(),
                };
                plot.text(Text::new(PlotPoint::new(at[0], at[1]), label).anchor(text_align(*anchor)));
            }
        }
    }
}

/// How a text mark's anchor point maps onto its text box.
#[must_use]
pub fn text_align(anchor: TextAnchor) -> Align2 {
    match anchor {
        TextAnchor::LeftOfText => Align2::LEFT_CENTER,
        TextAnchor::RightOfText => Align2::RIGHT_BOTTOM,
        TextAnchor::Centre => Align2::CENTER_CENTER,
        TextAnchor::Above => Align2::CENTER_BOTTOM,
    }
}

/// A filled band between two curves sampled at the same horizontal positions,
/// drawn as one triangle strip (egui_plot's polygons fill convex shapes only,
/// and a curved band is not convex).
struct BandItem {
    lower: Vec<[f64; 2]>,
    upper: Vec<[f64; 2]>,
    color: Color32,
    name: String,
}

impl BandItem {
    /// Matching pairs of finite lower and upper points.
    fn pairs(&self) -> impl Iterator<Item = (&[f64; 2], &[f64; 2])> {
        let finite = |p: &[f64; 2]| p[0].is_finite() && p[1].is_finite();
        self.lower.iter().zip(&self.upper).filter(move |(a, b)| finite(a) && finite(b))
    }
}

impl PlotItem for BandItem {
    fn shapes(&self, _ui: &Ui, transform: &PlotTransform, shapes: &mut Vec<Shape>) {
        let mut mesh = Mesh::default();
        for (a, b) in self.pairs() {
            mesh.colored_vertex(transform.position_from_point(&PlotPoint::new(a[0], a[1])), self.color);
            mesh.colored_vertex(transform.position_from_point(&PlotPoint::new(b[0], b[1])), self.color);
        }
        let n = (mesh.vertices.len() / 2) as u32;
        for i in 0..n.saturating_sub(1) {
            let (l0, u0, l1, u1) = (2 * i, 2 * i + 1, 2 * i + 2, 2 * i + 3);
            mesh.add_triangle(l0, u0, l1);
            mesh.add_triangle(u0, u1, l1);
        }
        if !mesh.indices.is_empty() {
            shapes.push(Shape::mesh(mesh));
        }
    }

    fn initialize(&mut self, _x_range: RangeInclusive<f64>) {}

    fn name(&self) -> &str {
        &self.name
    }

    fn color(&self) -> Color32 {
        self.color
    }

    fn highlight(&mut self) {}

    fn highlighted(&self) -> bool {
        false
    }

    fn allow_hover(&self) -> bool {
        false
    }

    fn geometry(&self) -> PlotGeometry<'_> {
        PlotGeometry::None
    }

    fn bounds(&self) -> PlotBounds {
        let mut b = PlotBounds::NOTHING;
        for (a, u) in self.pairs() {
            b.extend_with(&PlotPoint::new(a[0], a[1]));
            b.extend_with(&PlotPoint::new(u[0], u[1]));
        }
        b
    }

    fn id(&self) -> Option<Id> {
        None
    }
}

/// Convex polygons in their colours as one triangle mesh in plot
/// coordinates (the bands of a filled contour map), built once and drawn
/// with [`fill_item`].
#[derive(Debug, Clone, Default)]
pub struct FillMesh {
    vertices: Vec<[f64; 2]>,
    colors: Vec<Color32>,
    indices: Vec<u32>,
}

impl FillMesh {
    /// A mesh of `groups`: each a set of convex polygons and their colour.
    #[must_use]
    pub fn new<'a>(groups: impl IntoIterator<Item = (&'a [Vec<[f64; 2]>], Color32)>) -> Self {
        let mut m = Self::default();
        for (polygons, color) in groups {
            for poly in polygons.iter().filter(|p| p.len() >= 3) {
                let first = m.vertices.len() as u32;
                m.vertices.extend_from_slice(poly);
                m.colors.extend(std::iter::repeat_n(color, poly.len()));
                // Convex: a fan from the first vertex.
                for i in 1..poly.len() as u32 - 1 {
                    m.indices.extend([first, first + i, first + i + 1]);
                }
            }
        }
        m
    }

    /// Whether the mesh has nothing to draw.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }
}

/// A plot item drawing `mesh`. A mesh has no antialiased edges, so pieces
/// that share an edge meet without a hairline seam.
#[must_use]
pub fn fill_item(mesh: std::sync::Arc<FillMesh>, name: &str) -> FillItem {
    FillItem { mesh, name: name.to_owned() }
}

/// The plot item [`fill_item`] makes.
#[derive(Debug)]
pub struct FillItem {
    mesh: std::sync::Arc<FillMesh>,
    name: String,
}

impl PlotItem for FillItem {
    fn shapes(&self, _ui: &Ui, transform: &PlotTransform, shapes: &mut Vec<Shape>) {
        if self.mesh.is_empty() {
            return;
        }
        let mut mesh = Mesh::default();
        for (p, &color) in self.mesh.vertices.iter().zip(&self.mesh.colors) {
            mesh.colored_vertex(transform.position_from_point(&PlotPoint::new(p[0], p[1])), color);
        }
        mesh.indices.clone_from(&self.mesh.indices);
        shapes.push(Shape::mesh(mesh));
    }

    fn initialize(&mut self, _x_range: RangeInclusive<f64>) {}

    fn name(&self) -> &str {
        &self.name
    }

    fn color(&self) -> Color32 {
        self.mesh.colors.first().copied().unwrap_or(Color32::TRANSPARENT)
    }

    fn highlight(&mut self) {}

    fn highlighted(&self) -> bool {
        false
    }

    fn allow_hover(&self) -> bool {
        false
    }

    fn geometry(&self) -> PlotGeometry<'_> {
        PlotGeometry::None
    }

    fn bounds(&self) -> PlotBounds {
        let mut b = PlotBounds::NOTHING;
        for p in &self.mesh.vertices {
            if p[0].is_finite() && p[1].is_finite() {
                b.extend_with(&PlotPoint::new(p[0], p[1]));
            }
        }
        b
    }

    fn id(&self) -> Option<Id> {
        None
    }
}

/// Length of an arrow's head along the shaft, in screen points.
pub const ARROW_HEAD: f32 = 9.0;

/// The head of an arrow from `from` to `to` (screen points): a triangle with
/// its point at `to`, [`ARROW_HEAD`] long, and where the shaft ends.
#[must_use]
pub fn arrow_head(from: egui::Pos2, to: egui::Pos2) -> ([egui::Pos2; 3], egui::Pos2) {
    let d = to - from;
    let len = d.length();
    if len < 1e-3 {
        return ([to; 3], to);
    }
    let u = d / len;
    let n = egui::vec2(-u.y, u.x);
    let head = ARROW_HEAD.min(len);
    let base = to - u * head;
    ([to, base + n * head * 0.45, base - n * head * 0.45], base)
}

/// An arrow whose head keeps its size on screen.
struct ArrowItem {
    from: [f64; 2],
    to: [f64; 2],
    color: Color32,
    width: f32,
}

impl PlotItem for ArrowItem {
    fn shapes(&self, _ui: &Ui, transform: &PlotTransform, shapes: &mut Vec<Shape>) {
        let a = transform.position_from_point(&PlotPoint::new(self.from[0], self.from[1]));
        let b = transform.position_from_point(&PlotPoint::new(self.to[0], self.to[1]));
        if !(a.is_finite() && b.is_finite()) {
            return;
        }
        let (head, base) = arrow_head(a, b);
        shapes.push(Shape::line_segment([a, base], Stroke::new(self.width, self.color)));
        shapes.push(Shape::convex_polygon(head.to_vec(), self.color, Stroke::NONE));
    }

    fn initialize(&mut self, _x_range: RangeInclusive<f64>) {}

    fn name(&self) -> &str {
        ""
    }

    fn color(&self) -> Color32 {
        self.color
    }

    fn highlight(&mut self) {}

    fn highlighted(&self) -> bool {
        false
    }

    fn allow_hover(&self) -> bool {
        false
    }

    fn geometry(&self) -> PlotGeometry<'_> {
        PlotGeometry::None
    }

    fn bounds(&self) -> PlotBounds {
        let mut b = PlotBounds::NOTHING;
        for p in [self.from, self.to] {
            if p[0].is_finite() && p[1].is_finite() {
                b.extend_with(&PlotPoint::new(p[0], p[1]));
            }
        }
        b
    }

    fn id(&self) -> Option<Id> {
        None
    }
}

/// Straight segments in one style, drawn as one plot item (contour lines can
/// have thousands).
struct SegmentsItem {
    segments: Vec<[[f64; 2]; 2]>,
    color: Color32,
    width: f32,
    name: String,
}

impl PlotItem for SegmentsItem {
    fn shapes(&self, _ui: &Ui, transform: &PlotTransform, shapes: &mut Vec<Shape>) {
        let stroke = Stroke::new(self.width, self.color);
        for [a, b] in &self.segments {
            let (p, q) = (
                transform.position_from_point(&PlotPoint::new(a[0], a[1])),
                transform.position_from_point(&PlotPoint::new(b[0], b[1])),
            );
            if p.is_finite() && q.is_finite() {
                shapes.push(Shape::line_segment([p, q], stroke));
            }
        }
    }

    fn initialize(&mut self, _x_range: RangeInclusive<f64>) {}

    fn name(&self) -> &str {
        &self.name
    }

    fn color(&self) -> Color32 {
        self.color
    }

    fn highlight(&mut self) {}

    fn highlighted(&self) -> bool {
        false
    }

    fn allow_hover(&self) -> bool {
        false
    }

    fn geometry(&self) -> PlotGeometry<'_> {
        PlotGeometry::None
    }

    fn bounds(&self) -> PlotBounds {
        let mut b = PlotBounds::NOTHING;
        for p in self.segments.iter().flatten() {
            if p[0].is_finite() && p[1].is_finite() {
                b.extend_with(&PlotPoint::new(p[0], p[1]));
            }
        }
        b
    }

    fn id(&self) -> Option<Id> {
        None
    }
}

/// The shapes of a polyline `points` (screen positions) drawn in `pattern`.
#[must_use]
pub fn pattern_shapes(points: &[egui::Pos2], stroke: Stroke, pattern: LinePattern) -> Vec<Shape> {
    let dashes = pattern.dashes();
    if dashes.is_empty() {
        return vec![Shape::line(points.to_vec(), stroke)];
    }
    let dash: Vec<f32> = dashes.iter().step_by(2).map(|d| *d as f32 * 1.3).collect();
    let gap: Vec<f32> = dashes.iter().skip(1).step_by(2).map(|d| *d as f32 * 1.3).collect();
    let mut out = Vec::new();
    out.extend(Shape::dashed_line_with_offset(points, stroke, &dash, &gap, 0.0));
    out
}

/// Text written beside a reference line, inside the plot frame: at the
/// right end just above a horizontal line, at the top just right of a
/// vertical one.
fn rule_label(ui: &Ui, frame: egui::Rect, at: egui::Pos2, vertical: bool, text: &str, color: Color32) -> Shape {
    let font = egui::TextStyle::Small.resolve(ui.style());
    let galley = ui.fonts(|f| f.layout_no_wrap(text.to_owned(), font, color));
    let size = galley.size();
    let pos = if vertical {
        egui::pos2((at.x + 4.0).min(frame.right() - size.x - 2.0), frame.top() + 3.0)
    } else {
        egui::pos2(frame.right() - size.x - 4.0, (at.y - size.y - 1.0).max(frame.top() + 1.0))
    };
    Shape::galley(pos, galley, color)
}

/// A reference line across the whole plot ([`Mark::Rule`]).
struct RuleItem {
    at: f64,
    vertical: bool,
    color: Color32,
    width: f32,
    pattern: LinePattern,
    label: String,
    name: String,
}

impl PlotItem for RuleItem {
    fn shapes(&self, ui: &Ui, transform: &PlotTransform, shapes: &mut Vec<Shape>) {
        let frame = *transform.frame();
        let (a, b) = if self.vertical {
            let x = transform.position_from_point(&PlotPoint::new(self.at, 0.0)).x;
            (egui::pos2(x, frame.top()), egui::pos2(x, frame.bottom()))
        } else {
            let y = transform.position_from_point(&PlotPoint::new(0.0, self.at)).y;
            (egui::pos2(frame.left(), y), egui::pos2(frame.right(), y))
        };
        if !(a.is_finite() && b.is_finite()) {
            return;
        }
        if self.width > 0.0 {
            shapes.extend(pattern_shapes(&[a, b], Stroke::new(self.width, self.color), self.pattern));
        }
        if !self.label.is_empty() {
            shapes.push(rule_label(ui, frame, a, self.vertical, &self.label, self.color));
        }
    }

    fn initialize(&mut self, _x_range: RangeInclusive<f64>) {}

    fn name(&self) -> &str {
        &self.name
    }

    fn color(&self) -> Color32 {
        self.color
    }

    fn highlight(&mut self) {}

    fn highlighted(&self) -> bool {
        false
    }

    fn allow_hover(&self) -> bool {
        false
    }

    fn geometry(&self) -> PlotGeometry<'_> {
        PlotGeometry::None
    }

    fn bounds(&self) -> PlotBounds {
        let mut b = PlotBounds::NOTHING;
        if self.at.is_finite() {
            if self.vertical { b.extend_with_x(self.at) } else { b.extend_with_y(self.at) }
        }
        b
    }

    fn id(&self) -> Option<Id> {
        None
    }
}

/// A reference band across the whole plot ([`Mark::Span`]).
struct SpanItem {
    from: f64,
    to: f64,
    vertical: bool,
    color: Color32,
    name: String,
}

impl PlotItem for SpanItem {
    fn shapes(&self, _ui: &Ui, transform: &PlotTransform, shapes: &mut Vec<Shape>) {
        let frame = *transform.frame();
        let rect = if self.vertical {
            let x0 = transform.position_from_point(&PlotPoint::new(self.from, 0.0)).x;
            let x1 = transform.position_from_point(&PlotPoint::new(self.to, 0.0)).x;
            egui::Rect::from_x_y_ranges(x0.min(x1)..=x0.max(x1), frame.y_range())
        } else {
            let y0 = transform.position_from_point(&PlotPoint::new(0.0, self.from)).y;
            let y1 = transform.position_from_point(&PlotPoint::new(0.0, self.to)).y;
            egui::Rect::from_x_y_ranges(frame.x_range(), y0.min(y1)..=y0.max(y1))
        };
        let rect = rect.intersect(frame);
        if rect.is_finite() && rect.is_positive() {
            shapes.push(Shape::rect_filled(rect, 0.0, self.color));
        }
    }

    fn initialize(&mut self, _x_range: RangeInclusive<f64>) {}

    fn name(&self) -> &str {
        &self.name
    }

    fn color(&self) -> Color32 {
        self.color
    }

    fn highlight(&mut self) {}

    fn highlighted(&self) -> bool {
        false
    }

    fn allow_hover(&self) -> bool {
        false
    }

    fn geometry(&self) -> PlotGeometry<'_> {
        PlotGeometry::None
    }

    fn bounds(&self) -> PlotBounds {
        let mut b = PlotBounds::NOTHING;
        for v in [self.from, self.to].into_iter().filter(|v| v.is_finite()) {
            if self.vertical { b.extend_with_x(v) } else { b.extend_with_y(v) }
        }
        b
    }

    fn id(&self) -> Option<Id> {
        None
    }
}

/// One colour of points in a [`PointRaster`].
#[derive(Debug, Clone)]
pub struct RasterPoints {
    /// Positions in plot coordinates.
    pub points: Vec<[f64; 2]>,
    /// The points' colour.
    pub color: Color32,
    /// Opacity grows with the number of points in a pixel (`false`: solid).
    pub density: bool,
    /// Side of the square each point covers, in screen points.
    pub splat: f32,
}

/// Points drawn as one picture of the plot area at the screen's resolution
/// ([`crate::point_raster`]), for plots with more points than
/// [`crate::point_raster::POINT_BUDGET`]. The picture is redrawn only when
/// the points, the view or the plot's size change.
#[derive(Debug)]
pub struct PointRaster {
    layers: Vec<RasterPoints>,
    key: u64,
    name: String,
}

impl PointRaster {
    /// `layers` from the bottom up; `key` adds whatever else the caller knows
    /// changes the picture (such as the row states' revision) to the
    /// fingerprint of the points.
    #[must_use]
    pub fn new(layers: Vec<RasterPoints>, key: u64) -> Self {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        key.hash(&mut h);
        for l in &layers {
            (crate::point_raster::sample_key(&l.points), l.color.to_array(), l.density, l.splat.to_bits()).hash(&mut h);
        }
        Self { layers, key: h.finish(), name: String::new() }
    }

    /// Name the item (shown in egui_plot's own legend, when one is used).
    #[must_use]
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// The number of points drawn.
    #[must_use]
    pub fn len(&self) -> usize {
        self.layers.iter().map(|l| l.points.len()).sum()
    }

    /// `true` when there are no points.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Textures of the point pictures drawn lately, shared by every plot and
/// kept in the context's memory: (picture key, texture, viewport, pass when
/// last drawn).
#[derive(Clone, Default)]
struct RasterTextures(Vec<(u64, egui::TextureHandle, egui::ViewportId, u64)>);

/// Textures kept at most, across all plots and windows.
const MAX_RASTER_TEXTURES: usize = 48;

impl PlotItem for PointRaster {
    fn shapes(&self, ui: &Ui, transform: &PlotTransform, shapes: &mut Vec<Shape>) {
        use std::hash::{Hash, Hasher};
        let frame = *transform.frame();
        let ctx = ui.ctx();
        let ppp = ctx.pixels_per_point();
        let size = |v: f32| (v * ppp).round().clamp(0.0, 8192.0) as u32;
        let (width, height) = (size(frame.width()), size(frame.height()));
        if width == 0 || height == 0 || self.is_empty() {
            return;
        }
        let bounds = transform.bounds();
        let (min, max) = (bounds.min(), bounds.max());
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (self.key, min.map(f64::to_bits), max.map(f64::to_bits), width, height).hash(&mut h);
        let key = h.finish();
        let viewport = ctx.viewport_id();
        let pass = ctx.cumulative_pass_nr();
        let id = Id::new("stat_viz_point_raster_textures");
        let cached = ctx.data_mut(|d| {
            let cache = d.get_temp_mut_or_default::<RasterTextures>(id);
            // Pictures this window has not drawn for a couple of frames are
            // out of date (the view moved, or the plot closed).
            cache.0.retain(|e| e.2 != viewport || e.3 + 2 >= pass);
            cache.0.iter_mut().find(|e| e.0 == key).map(|e| {
                e.3 = pass;
                e.1.clone()
            })
        });
        let texture = cached.unwrap_or_else(|| {
            let layers: Vec<crate::point_raster::RasterLayer<'_>> = self
                .layers
                .iter()
                .map(|l| crate::point_raster::RasterLayer {
                    points: &l.points,
                    color: Rgba(l.color.to_srgba_unmultiplied()),
                    density: l.density,
                    splat: (l.splat * ppp).round().max(1.0) as u32,
                })
                .collect();
            let view = crate::point_raster::RasterView { x: [min[0], max[0]], y: [min[1], max[1]] };
            let b = crate::point_raster::rasterize_points(&layers, view, width, height);
            let image = egui::ColorImage::from_rgba_unmultiplied([b.width as usize, b.height as usize], &b.rgba);
            let texture = ctx.load_texture("point-raster", image, egui::TextureOptions::NEAREST);
            ctx.data_mut(|d| {
                let cache = d.get_temp_mut_or_default::<RasterTextures>(id);
                cache.0.push((key, texture.clone(), viewport, pass));
                if cache.0.len() > MAX_RASTER_TEXTURES {
                    cache.0.sort_by_key(|e| std::cmp::Reverse(e.3));
                    cache.0.truncate(MAX_RASTER_TEXTURES);
                }
            });
            texture
        });
        let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        shapes.push(Shape::image(texture.id(), frame, uv, Color32::WHITE));
    }

    fn initialize(&mut self, _x_range: RangeInclusive<f64>) {}

    fn name(&self) -> &str {
        &self.name
    }

    fn color(&self) -> Color32 {
        self.layers.first().map_or(Color32::TRANSPARENT, |l| l.color)
    }

    fn highlight(&mut self) {}

    fn highlighted(&self) -> bool {
        false
    }

    fn allow_hover(&self) -> bool {
        false
    }

    fn geometry(&self) -> PlotGeometry<'_> {
        PlotGeometry::None
    }

    fn bounds(&self) -> PlotBounds {
        let mut b = PlotBounds::NOTHING;
        for p in self.layers.iter().flat_map(|l| &l.points) {
            if p[0].is_finite() && p[1].is_finite() {
                b.extend_with(&PlotPoint::new(p[0], p[1]));
            }
        }
        b
    }

    fn id(&self) -> Option<Id> {
        None
    }
}

/// The square a marker of `radius` covers in a point picture, in screen
/// points: a little smaller than the marker, so dense clouds keep their shape.
#[must_use]
pub fn raster_splat(radius: f32) -> f32 {
    (radius * 0.75).clamp(1.5, 4.0)
}
