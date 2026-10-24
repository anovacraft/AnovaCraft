//! Broken axes: several stretches left out of one continuous axis, each
//! remaining segment with a scale and a share of the axis of its own.
//!
//! **Settings.** [`AxisBreaks`] holds what the user sets on one axis: the
//! stretches left out (data units, [`AxisBreak`]), one [`AxisSegment`] per
//! remaining piece (its scale, or the axis's own; its share of the axis
//! length, or automatic) and the style of the marks drawn where the axis is
//! cut ([`BreakMark`]).
//!
//! **The axis map.** [`BrokenAxis`] is the piecewise map those settings give
//! from an axis's drawn units (its own scale applied, as layers are built)
//! to the broken axis's drawn units. Piece `k` of the axis lies between the
//! end of break `k − 1` and the start of break `k`; inside it a value `v` is
//! drawn at `u = c_k + s_k · g_k(v)`, where `g_k` is the piece's scale. The
//! offsets `c_k` make the pieces meet: the end of one piece and the start of
//! the next are drawn at the same place, where the break marks go
//! (Cleveland, *The Elements of Graphing Data*, rev. ed., 1994, §2.4, on
//! showing scale breaks; a full-scale break between two panels is the
//! alternative he prefers when the pieces differ much).
//!
//! * Without segment scales or shares every slope `s_k` is 1, so the axis is
//!   the unbroken axis with each removed stretch closed up: values below the
//!   first break are drawn where they were, and nothing depends on the data.
//! * With shares, or a segment drawn on another scale (a log segment below a
//!   linear one, say), piece `k` is given `share_k / Σ share` of the data's
//!   extent on the axis: its slope is chosen so that the part of the data's
//!   range inside it takes that fraction of the length. Pieces without a
//!   share split the rest equally. The first piece that holds data keeps
//!   slope 1, so its values are drawn in its own units (the default range's
//!   "never below zero" rule still sees them). Without a data extent (no
//!   points on the axis) the slopes stay 1.
//!
//! Points inside a removed stretch are not drawn; areas are cut at its
//! edge. Statistics are computed before the map, so they do not change.
//! Straight lines between two vertices stay straight in the broken drawing,
//! so a fit drawn across a segment of another scale bends only where its
//! vertices are (fits are sampled finely, so this does not show).

use crate::axes::AxisBreak;
use crate::frame::AxisSpec;
use crate::scale::AxisScale;

/// How the place where an axis is cut is marked on the spine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum BreakMark {
    /// A pair of short parallel slashes across the spine.
    #[default]
    Slashes,
    /// A short zigzag across the spine.
    Zigzag,
}

impl BreakMark {
    /// Every style, in menu order.
    pub const ALL: [BreakMark; 2] = [BreakMark::Slashes, BreakMark::Zigzag];

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Slashes => "Slashes",
            Self::Zigzag => "Zigzag",
        }
    }

    /// Short name for saving.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Slashes => "slashes",
            Self::Zigzag => "zigzag",
        }
    }

    /// The style with this [`BreakMark::key`].
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.key() == key)
    }

    /// The strokes of the mark centred at `at` across a spine running up
    /// (`vertical`) or across, `half` long either side, as polylines in a
    /// coordinate system whose second axis points down (screen and page).
    /// Slashes are two parallel strokes `gap` either side of the centre; a
    /// zigzag is one stroke in the spine, out to either side and back.
    #[must_use]
    pub fn strokes(self, at: [f64; 2], vertical: bool, half: f64, gap: f64, slant: f64) -> Vec<Vec<[f64; 2]>> {
        // Build across a horizontal spine, then turn for a vertical one.
        let place = |dx: f64, dy: f64| if vertical { [at[0] + dy, at[1] + dx] } else { [at[0] + dx, at[1] + dy] };
        match self {
            Self::Slashes => {
                [-gap, gap].into_iter().map(|d| vec![place(d - slant, half), place(d + slant, -half)]).collect()
            }
            Self::Zigzag => {
                // A zigzag in the spine itself: from the spine out to either
                // side and back, over twice the slash spacing.
                let n = 4;
                let pts = (0..=n)
                    .map(|i| {
                        let along = -2.0 * gap + 4.0 * gap * f64::from(i) / f64::from(n);
                        let across = match i {
                            0 => 0.0,
                            i if i == n => 0.0,
                            i if i % 2 == 1 => half / 2.0,
                            _ => -half / 2.0,
                        };
                        place(along, across)
                    })
                    .collect();
                vec![pts]
            }
        }
    }

    /// The outline covering the spine under the mark (painted in the
    /// background colour so the axis reads as cut), as for
    /// [`BreakMark::strokes`].
    #[must_use]
    pub fn cover(self, at: [f64; 2], vertical: bool, half: f64, gap: f64, slant: f64) -> Vec<[f64; 2]> {
        let place = |dx: f64, dy: f64| if vertical { [at[0] + dy, at[1] + dx] } else { [at[0] + dx, at[1] + dy] };
        match self {
            Self::Slashes => vec![
                place(-gap - slant, half),
                place(-gap + slant, -half),
                place(gap + slant, -half),
                place(gap - slant, half),
            ],
            Self::Zigzag => {
                let (a, w) = (2.0 * gap, 1.5);
                vec![place(-a, w), place(-a, -w), place(a, -w), place(a, w)]
            }
        }
    }
}

/// One piece of a broken axis, between two breaks (or a break and an end).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct AxisSegment {
    /// The scale of this piece; `None` keeps the axis's own.
    pub scale: Option<AxisScale>,
    /// This piece's share of the axis length, in percent; `None` is
    /// automatic (see the module documentation).
    pub share: Option<f64>,
}

impl AxisSegment {
    /// `true` when nothing is set.
    #[must_use]
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// Everything the user sets about the breaks of one axis.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AxisBreaks {
    /// The stretches left out, in data units.
    pub breaks: Vec<AxisBreak>,
    /// One entry per piece (`breaks.len() + 1`, lowest first); missing
    /// entries are default pieces, extra ones are ignored.
    pub segments: Vec<AxisSegment>,
    /// How the cuts are marked.
    pub mark: BreakMark,
}

impl AxisBreaks {
    /// One stretch `[from, to]` left out, nothing else set.
    #[must_use]
    pub fn single(from: f64, to: f64) -> Self {
        Self { breaks: vec![AxisBreak { from: from.min(to), to: from.max(to) }], ..Self::default() }
    }

    /// `true` when nothing is set.
    #[must_use]
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// `true` when the axis has no break (segment settings alone draw
    /// nothing).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.breaks.is_empty()
    }

    /// The setting of piece `k` (lowest first).
    #[must_use]
    pub fn segment(&self, k: usize) -> AxisSegment {
        self.segments.get(k).copied().unwrap_or_default()
    }

    /// The breaks sorted, with overlapping ones merged and empty or
    /// non-finite ones dropped; segment settings follow the breaks they lie
    /// between (a merge keeps the lower piece's setting).
    #[must_use]
    pub fn tidied(&self) -> Self {
        let mut order: Vec<usize> = (0..self.breaks.len())
            .filter(|&i| {
                let b = self.breaks[i];
                b.from.is_finite() && b.to.is_finite() && b.from != b.to
            })
            .collect();
        let lo = |b: AxisBreak| b.from.min(b.to);
        let hi = |b: AxisBreak| b.from.max(b.to);
        order.sort_by(|&a, &b| lo(self.breaks[a]).total_cmp(&lo(self.breaks[b])));
        let mut breaks: Vec<AxisBreak> = Vec::new();
        let mut segments = vec![self.segment(0)];
        for i in order {
            let (a, b) = (lo(self.breaks[i]), hi(self.breaks[i]));
            match breaks.last_mut() {
                Some(last) if a <= last.to => last.to = last.to.max(b),
                _ => {
                    breaks.push(AxisBreak { from: a, to: b });
                    // The piece above break i is segment i + 1 of the original order.
                    segments.push(self.segment(i + 1));
                }
            }
        }
        while segments.last().is_some_and(AxisSegment::is_default) {
            segments.pop();
        }
        Self { breaks, segments, mark: self.mark }
    }

    /// Whether the axis map needs the data's extent: some piece has a
    /// share or its own scale.
    #[must_use]
    pub fn uses_shares(&self, axis_scale: AxisScale) -> bool {
        (0..=self.breaks.len()).any(|k| {
            let s = self.segment(k);
            s.share.is_some() || s.scale.is_some_and(|sc| sc != axis_scale)
        })
    }

    /// A sentence saying which scale each piece uses, when they differ
    /// (`None` when every piece uses the axis's scale). `axis` names the
    /// axis ("vertical", "horizontal").
    #[must_use]
    pub fn scales_note(&self, axis: &str, axis_scale: AxisScale) -> Option<String> {
        let t = self.tidied();
        let scales: Vec<AxisScale> = (0..=t.breaks.len()).map(|k| t.segment(k).scale.unwrap_or(axis_scale)).collect();
        if scales.iter().all(|s| *s == scales[0]) {
            return None;
        }
        let name = |s: AxisScale| match s {
            AxisScale::Linear => "linear".to_owned(),
            other => other.label().to_lowercase(),
        };
        let mut parts = Vec::new();
        for (k, s) in scales.iter().enumerate() {
            let place = match (k, k == t.breaks.len()) {
                (0, _) => format!("below {}", short(t.breaks[0].from)),
                (_, true) => format!("above {}", short(t.breaks[k - 1].to)),
                _ => format!("from {} to {}", short(t.breaks[k - 1].to), short(t.breaks[k].from)),
            };
            parts.push(format!("{} {place}", name(*s)));
        }
        Some(format!("The {axis} axis is broken into segments on different scales: {}.", parts.join(", ")))
    }

    /// The axis map for an axis drawn on `scale`, whose data span `extent`
    /// (drawn units of `scale`; needed only when [`AxisBreaks::uses_shares`]).
    /// `None` when no break lies inside the scale's domain.
    #[must_use]
    pub fn map(&self, scale: AxisScale, extent: Option<(f64, f64)>) -> Option<BrokenAxis> {
        BrokenAxis::new(self, scale, extent)
    }
}

/// A number written briefly for a note.
fn short(v: f64) -> String {
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-0" { "0".to_owned() } else { s.to_owned() }
}

/// One piece of a [`BrokenAxis`].
#[derive(Debug, Clone, Copy, PartialEq)]
struct Piece {
    /// Lower end in the axis's drawn units (`-∞` for the first piece).
    lo: f64,
    /// Upper end in the axis's drawn units (`+∞` for the last piece).
    hi: f64,
    /// The piece's own scale.
    scale: AxisScale,
    /// Drawn units per unit of the piece's scale.
    slope: f64,
    /// Drawn position of the piece scale's zero.
    offset: f64,
}

/// The piecewise map of a broken axis (see the module documentation): from
/// the axis's drawn units to the broken drawing's, and back.
#[derive(Debug, Clone, PartialEq)]
pub struct BrokenAxis {
    /// The axis's own scale.
    axis_scale: AxisScale,
    /// Pieces, lowest first; consecutive pieces meet at a break.
    pieces: Vec<Piece>,
    /// How the cuts are marked.
    pub mark: BreakMark,
}

impl BrokenAxis {
    /// One stretch `(lo, hi)` (drawn units) closed up on an axis of
    /// `scale`: the map of a single break with nothing else set.
    #[must_use]
    pub fn gap(scale: AxisScale, (lo, hi): (f64, f64)) -> Self {
        let first = Piece { lo: f64::NEG_INFINITY, hi: lo, scale, slope: 1.0, offset: 0.0 };
        let second = Piece { lo: hi, hi: f64::INFINITY, scale, slope: 1.0, offset: lo - hi };
        Self { axis_scale: scale, pieces: vec![first, second], mark: BreakMark::Slashes }
    }

    fn new(settings: &AxisBreaks, scale: AxisScale, extent: Option<(f64, f64)>) -> Option<Self> {
        let t = settings.tidied();
        // Breaks in drawn units; those outside the scale's domain are dropped
        // with the setting of the piece above them.
        let mut gaps: Vec<(f64, f64)> = Vec::new();
        let mut segs = vec![t.segment(0)];
        for (k, b) in t.breaks.iter().enumerate() {
            if let Some(g) = b.drawn(scale) {
                if gaps.last().is_none_or(|last| g.0 >= last.1) {
                    gaps.push(g);
                    segs.push(t.segment(k + 1));
                }
            }
        }
        if gaps.is_empty() {
            return None;
        }
        let n = gaps.len() + 1;
        let mut pieces: Vec<Piece> = (0..n)
            .map(|k| {
                let lo = if k == 0 { f64::NEG_INFINITY } else { gaps[k - 1].1 };
                let hi = if k == n - 1 { f64::INFINITY } else { gaps[k].0 };
                Piece { lo, hi, scale: segs[k].scale.unwrap_or(scale), slope: 1.0, offset: 0.0 }
            })
            .collect();
        // A piece whose scale cannot take its finite ends keeps the axis's.
        for p in &mut pieces {
            let own = |w: f64| !w.is_finite() || p.scale.forward(scale.inverse(w)).is_some();
            if p.scale != scale && !(own(p.lo) && own(p.hi)) {
                p.scale = scale;
            }
        }
        let mut axis = Self { axis_scale: scale, pieces, mark: t.mark };
        let shares = settings.uses_shares(scale);
        let anchor = match (shares, extent.filter(|(a, b)| a.is_finite() && b.is_finite() && b > a)) {
            (true, Some(extent)) => axis.set_slopes(&segs, extent),
            _ => 0,
        };
        axis.chain(anchor);
        Some(axis)
    }

    /// The value of the piece's scale at the axis's drawn position `w`.
    fn own(&self, p: &Piece, w: f64) -> f64 {
        if p.scale == self.axis_scale {
            w
        } else if w.is_finite() {
            p.scale.forward(self.axis_scale.inverse(w)).unwrap_or(f64::NAN)
        } else {
            w
        }
    }

    /// Slopes from the shares over the data `extent`; returns the anchor
    /// piece (the first holding data, which keeps slope 1).
    fn set_slopes(&mut self, segs: &[AxisSegment], (e_lo, e_hi): (f64, f64)) -> usize {
        let lengths: Vec<f64> = self
            .pieces
            .iter()
            .map(|p| {
                let (a, b) = (p.lo.max(e_lo), p.hi.min(e_hi));
                if b > a {
                    let len = self.own(p, b) - self.own(p, a);
                    if len.is_finite() && len > 0.0 { len } else { 0.0 }
                } else {
                    0.0
                }
            })
            .collect();
        let with_data: Vec<usize> = (0..lengths.len()).filter(|&k| lengths[k] > 0.0).collect();
        let Some(&anchor) = with_data.first() else { return 0 };
        // Shares of the pieces holding data; unset ones split what is left.
        let given: f64 = with_data.iter().filter_map(|&k| segs[k].share).filter(|s| *s > 0.0).sum();
        let unset = with_data.iter().filter(|&&k| segs[k].share.is_none_or(|s| s <= 0.0)).count();
        let rest = if unset > 0 {
            let left = (100.0 - given).max(0.0);
            if left > 0.0 { left / unset as f64 } else { given.max(100.0) / with_data.len() as f64 }
        } else {
            0.0
        };
        let share = |k: usize| segs[k].share.filter(|s| *s > 0.0).unwrap_or(rest);
        let total = lengths[anchor] / share(anchor);
        for &k in &with_data {
            self.pieces[k].slope = total * share(k) / lengths[k];
        }
        // Pieces without data take the slope of the nearest piece with data.
        for k in 0..self.pieces.len() {
            if lengths[k] <= 0.0 {
                let near = with_data.iter().min_by_key(|&&j| j.abs_diff(k)).copied().unwrap_or(anchor);
                self.pieces[k].slope = self.pieces[near].slope;
            }
        }
        anchor
    }

    /// Offsets so that consecutive pieces meet, piece `anchor` having
    /// offset 0.
    fn chain(&mut self, anchor: usize) {
        self.pieces[anchor].offset = 0.0;
        for k in anchor + 1..self.pieces.len() {
            let prev = self.pieces[k - 1];
            let end = prev.offset + prev.slope * self.own(&prev, prev.hi);
            let p = self.pieces[k];
            self.pieces[k].offset = end - p.slope * self.own(&p, p.lo);
        }
        for k in (0..anchor).rev() {
            let next = self.pieces[k + 1];
            let start = next.offset + next.slope * self.own(&next, next.lo);
            let p = self.pieces[k];
            self.pieces[k].offset = start - p.slope * self.own(&p, p.hi);
        }
    }

    fn draw(&self, p: &Piece, w: f64) -> f64 {
        if p.scale == self.axis_scale && p.slope == 1.0 { w + p.offset } else { p.offset + p.slope * self.own(p, w) }
    }

    /// Drawn position of `w` (axis drawn units); NaN inside a break.
    #[must_use]
    pub fn map(&self, w: f64) -> f64 {
        if w.is_nan() {
            return w;
        }
        self.pieces.iter().find(|p| w >= p.lo && w <= p.hi).map_or(f64::NAN, |p| self.draw(p, w))
    }

    /// [`BrokenAxis::map`] for areas: values inside a break go to its lower
    /// edge.
    #[must_use]
    pub fn clamp(&self, w: f64) -> f64 {
        if w.is_nan() {
            return w;
        }
        match self.pieces.iter().find(|p| w >= p.lo && w <= p.hi) {
            Some(p) => self.draw(p, w),
            None => {
                let below = self.pieces.iter().rev().find(|p| p.hi < w).unwrap_or(&self.pieces[0]);
                self.draw(below, below.hi)
            }
        }
    }

    /// Where piece `k` starts and ends in the drawing.
    fn span(&self, k: usize) -> (f64, f64) {
        let p = &self.pieces[k];
        let at = |w: f64| if w.is_finite() { self.draw(p, w) } else { w };
        (at(p.lo), at(p.hi))
    }

    /// The piece drawn at `u` and its scale's value there.
    fn locate(&self, u: f64) -> (usize, f64) {
        let k = (0..self.pieces.len()).find(|&k| u <= self.span(k).1).unwrap_or(self.pieces.len() - 1);
        let p = &self.pieces[k];
        (k, (u - p.offset) / p.slope)
    }

    /// The axis's drawn units at drawn position `u` of the broken axis (the
    /// inverse of [`BrokenAxis::map`]; a break point reads as its lower end).
    #[must_use]
    pub fn unmap(&self, u: f64) -> f64 {
        let (k, g) = self.locate(u);
        let p = &self.pieces[k];
        if p.scale == self.axis_scale {
            if p.slope == 1.0 { u - p.offset } else { g }
        } else {
            self.axis_scale.forward(p.scale.inverse(g)).unwrap_or(f64::NAN)
        }
    }

    /// The data value drawn at `u`.
    #[must_use]
    pub fn value_at(&self, u: f64) -> f64 {
        let (k, g) = self.locate(u);
        self.pieces[k].scale.inverse(g)
    }

    /// Drawn positions of the cuts (where the break marks go).
    #[must_use]
    pub fn cuts(&self) -> Vec<f64> {
        (0..self.pieces.len() - 1).map(|k| self.span(k).1).collect()
    }

    /// `true` when some piece is drawn on another scale than the axis's.
    #[must_use]
    pub fn mixed_scales(&self) -> bool {
        self.pieces.iter().any(|p| p.scale != self.axis_scale)
    }

    /// The tick label at drawn position `u`, written as `axis` writes its
    /// labels on the scale of the piece there.
    #[must_use]
    pub fn label_at(&self, axis: &AxisSpec, u: f64) -> String {
        let (k, g) = self.locate(u);
        let piece_axis = AxisSpec { scale: self.pieces[k].scale, ..axis.clone() };
        crate::view::tick_label(&piece_axis, g)
    }

    /// Ticks of `axis` within the drawn range `lo..=hi`: each piece's own
    /// nice values (on its scale, about `target` over the whole axis, shared
    /// by length), placed through the map. Ticks crowding a cut from above
    /// are dropped when the piece below has ticks.
    #[must_use]
    pub fn ticks(&self, axis: &AxisSpec, lo: f64, hi: f64, target: usize) -> Vec<(f64, String)> {
        if !(lo.is_finite() && hi.is_finite()) || hi <= lo {
            return Vec::new();
        }
        let span = hi - lo;
        let crowd = 0.05 * span;
        let mut out: Vec<(f64, String)> = Vec::new();
        for (k, p) in self.pieces.iter().enumerate() {
            let (s0, s1) = self.span(k);
            let (a, b) = (lo.max(s0), hi.min(s1));
            if b <= a {
                continue;
            }
            let n = ((target as f64 * (b - a) / span).round() as usize).max(2);
            let piece_axis = AxisSpec { scale: p.scale, ..axis.clone() };
            let (ga, gb) = ((a - p.offset) / p.slope, (b - p.offset) / p.slope);
            let lower_side = !out.is_empty();
            for (g, label) in crate::export::axis_tick_labels(&piece_axis, ga, gb, n) {
                let at = p.offset + p.slope * g;
                if at < a - 1e-9 * span || at > b + 1e-9 * span {
                    continue;
                }
                if lower_side && k > 0 && at - s0 < crowd {
                    continue;
                }
                out.push((at, label));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::AxisKind;

    fn linear() -> AxisSpec {
        AxisSpec { name: "y".into(), kind: AxisKind::Continuous, scale: AxisScale::Linear, time: None }
    }

    /// One break with nothing else set is today's single gap: values below
    /// it stay, values above move down by its length, inside is NaN.
    #[test]
    fn a_single_break_closes_up_the_stretch() {
        let m = AxisBreaks::single(10.0, 90.0).map(AxisScale::Linear, None).unwrap();
        assert_eq!(m, BrokenAxis::gap(AxisScale::Linear, (10.0, 90.0)));
        assert_eq!(m.map(5.0), 5.0);
        assert_eq!(m.map(100.0), 20.0);
        assert!(m.map(50.0).is_nan());
        assert_eq!(m.clamp(50.0), 10.0);
        assert_eq!(m.unmap(20.0), 100.0);
        assert_eq!(m.unmap(m.map(95.0)), 95.0);
        assert_eq!(m.cuts(), vec![10.0]);
    }

    /// Several breaks close up one after another, in any order given, and
    /// overlapping ones merge.
    #[test]
    fn several_breaks_close_up_in_turn() {
        let b = AxisBreaks {
            breaks: vec![AxisBreak { from: 200.0, to: 900.0 }, AxisBreak { from: 10.0, to: 90.0 }],
            ..AxisBreaks::default()
        };
        let m = b.map(AxisScale::Linear, None).unwrap();
        assert_eq!(m.map(5.0), 5.0);
        assert_eq!(m.map(100.0), 20.0);
        assert_eq!(m.map(1000.0), 1000.0 - 80.0 - 700.0);
        assert!(m.map(500.0).is_nan());
        assert_eq!(m.cuts(), vec![10.0, 120.0]);
        let merged = AxisBreaks {
            breaks: vec![AxisBreak { from: 10.0, to: 50.0 }, AxisBreak { from: 40.0, to: 90.0 }],
            ..AxisBreaks::default()
        };
        assert_eq!(merged.tidied().breaks, vec![AxisBreak { from: 10.0, to: 90.0 }]);
        for w in [-3.0, 0.0, 9.0, 100.0, 150.0, 950.0, 4000.0] {
            assert!((m.unmap(m.map(w)) - w).abs() < 1e-9, "{w}");
        }
    }

    /// Shares give each piece its fraction of the data's extent; the first
    /// piece keeps its own units.
    #[test]
    fn shares_set_each_piece_length() {
        let b = AxisBreaks {
            breaks: vec![AxisBreak { from: 10.0, to: 100.0 }],
            segments: vec![
                AxisSegment { share: Some(75.0), ..AxisSegment::default() },
                AxisSegment { share: Some(25.0), ..AxisSegment::default() },
            ],
            mark: BreakMark::Zigzag,
        };
        let m = b.map(AxisScale::Linear, Some((0.0, 1000.0))).unwrap();
        assert_eq!(m.mark, BreakMark::Zigzag);
        // First piece 0..10 drawn 0..10 (three quarters), second 100..1000
        // drawn 10..13.33 (one quarter).
        assert!((m.map(10.0) - 10.0).abs() < 1e-12);
        let top = m.map(1000.0);
        assert!(((top - 10.0) / top - 0.25).abs() < 1e-12, "{top}");
        assert!((m.unmap(m.map(550.0)) - 550.0).abs() < 1e-9);
        // Only one share: the other piece takes the rest.
        let one =
            AxisBreaks { segments: vec![AxisSegment { share: Some(40.0), ..AxisSegment::default() }], ..b.clone() };
        let m = one.map(AxisScale::Linear, Some((0.0, 1000.0))).unwrap();
        assert!((m.map(10.0) / m.map(1000.0) - 0.4).abs() < 1e-12);
        // No extent: slopes stay 1.
        let m = b.map(AxisScale::Linear, None).unwrap();
        assert_eq!(m.map(1000.0), 1000.0 - 90.0);
    }

    /// A log segment below a linear one: equal shares by default, values in
    /// the log piece drawn on log₁₀, ticks and labels from each piece's own
    /// scale, and a note naming the scales.
    #[test]
    fn a_segment_on_its_own_scale() {
        let b = AxisBreaks {
            breaks: vec![AxisBreak { from: 1000.0, to: 1000.0 + 1e-9 }],
            segments: vec![AxisSegment { scale: Some(AxisScale::Log10), share: None }],
            ..AxisBreaks::default()
        };
        let m = b.map(AxisScale::Linear, Some((1.0, 5000.0))).unwrap();
        assert!(m.mixed_scales());
        // Log piece 1..1000 is 3 decades drawn 0..3; the linear piece takes
        // the same length.
        assert!((m.map(10.0) - 1.0).abs() < 1e-12);
        assert!((m.map(100.0) - 2.0).abs() < 1e-12);
        assert!((m.map(5000.0) - 6.0).abs() < 1e-6);
        assert!((m.value_at(m.map(3000.0)) - 3000.0).abs() < 1e-6);
        assert!((m.unmap(m.map(30.0)) - 30.0).abs() < 1e-9);
        let ticks = m.ticks(&linear(), 0.0, 6.0, 8);
        let labels: Vec<&str> = ticks.iter().map(|t| t.1.as_str()).collect();
        assert!(labels.contains(&"10") && labels.contains(&"100"), "{labels:?}");
        assert!(ticks.windows(2).all(|w| w[0].0 < w[1].0), "{ticks:?}");
        assert_eq!(m.label_at(&linear(), 2.0), "100");
        let note = b.scales_note("vertical", AxisScale::Linear).unwrap();
        assert!(note.contains("log (base 10) below 1000") && note.contains("linear above 1000"), "{note}");
        assert!(AxisBreaks::single(1.0, 2.0).scales_note("vertical", AxisScale::Linear).is_none());
        // Values the log piece cannot take are not drawn.
        assert!(m.map(-5.0).is_nan());
    }

    /// Ticks on each side of a break come from that side's values; none
    /// crowd the cut from above.
    #[test]
    fn ticks_follow_each_piece() {
        let m = AxisBreaks::single(12.0, 950.0).map(AxisScale::Linear, None).unwrap();
        let ticks = m.ticks(&linear(), 0.0, 62.0, 6);
        assert!(ticks.iter().any(|t| t.1 == "10"));
        assert!(ticks.iter().any(|t| t.1 == "1000"), "{ticks:?}");
        assert!(ticks.iter().all(|t| t.0 <= 12.0 || t.0 - 12.0 >= 0.05 * 62.0));
    }

    #[test]
    fn marks_have_strokes_and_a_cover() {
        for mark in BreakMark::ALL {
            assert_eq!(BreakMark::from_key(mark.key()), Some(mark));
            let s = mark.strokes([10.0, 10.0], true, 6.0, 2.0, 2.5);
            assert!(!s.is_empty() && s.iter().all(|l| l.len() >= 2));
            assert_eq!(mark.cover([10.0, 10.0], false, 6.0, 2.0, 2.5).len(), 4);
        }
    }
}
