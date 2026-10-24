//! Significance brackets between compared categories.

use stat_data::RowStateManager;
use stat_inference::Alternative;
use stat_inference::control::dunnett_test;
use stat_inference::multiple::{ComparisonMethod, compare_all_pairs};

use super::{Element, ElementKind, categories_across};
use crate::frame::Panel;
use crate::layer::{KeyGlyph, Layer, LinePattern, Mark, TextAnchor};
use crate::palette;
use crate::view::format_value;

/// The multiple-comparison procedure behind the brackets (all from
/// `stat_inference`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BracketTest {
    /// Tukey–Kramer comparisons of every pair (pooled variance).
    #[default]
    TukeyKramer,
    /// Games–Howell comparisons of every pair (unequal variances).
    GamesHowell,
    /// Pooled-variance t tests of every pair, Holm-adjusted.
    HolmT,
    /// Dunnett's comparisons of every category with the first (the control).
    Dunnett,
}

impl BracketTest {
    /// Every procedure, in menu order.
    pub const ALL: [BracketTest; 4] =
        [BracketTest::TukeyKramer, BracketTest::GamesHowell, BracketTest::HolmT, BracketTest::Dunnett];

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::TukeyKramer => "Tukey–Kramer (every pair)",
            Self::GamesHowell => "Games–Howell (every pair, unequal variances)",
            Self::HolmT => "Holm-adjusted t tests (every pair)",
            Self::Dunnett => "Dunnett (each against the first category)",
        }
    }

    /// Short name in notes.
    #[must_use]
    pub fn short(self) -> &'static str {
        match self {
            Self::TukeyKramer => "Tukey–Kramer",
            Self::GamesHowell => "Games–Howell",
            Self::HolmT => "Holm-adjusted t",
            Self::Dunnett => "Dunnett",
        }
    }

    /// Short name for saving.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::TukeyKramer => "tukey-kramer",
            Self::GamesHowell => "games-howell",
            Self::HolmT => "holm-t",
            Self::Dunnett => "dunnett",
        }
    }

    /// The procedure with this [`BracketTest::key`].
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.key() == key)
    }
}

/// What each bracket says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BracketLabel {
    /// The adjusted p-value (`p = 0.012`).
    PValue,
    /// Stars ([`stars`]).
    Stars,
    /// Both (`p = 0.012 *`).
    #[default]
    Both,
}

impl BracketLabel {
    /// Every choice, in menu order.
    pub const ALL: [BracketLabel; 3] = [BracketLabel::PValue, BracketLabel::Stars, BracketLabel::Both];

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::PValue => "p-value",
            Self::Stars => "Stars",
            Self::Both => "p-value and stars",
        }
    }

    /// Short name for saving.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::PValue => "p-value",
            Self::Stars => "stars",
            Self::Both => "both",
        }
    }

    /// The choice with this [`BracketLabel::key`].
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.key() == key)
    }
}

/// The usual star thresholds: `*` below 0.05, `**` below 0.01, `***` below
/// 0.001, `****` below 0.0001.
pub const STAR_THRESHOLDS: [f64; 4] = [0.05, 0.01, 0.001, 0.0001];

/// Stars for an adjusted p-value: `****` below 0.0001, `***` below 0.001,
/// `**` below 0.01, `*` below 0.05, otherwise `ns` (not significant).
#[must_use]
pub fn stars(p: f64) -> &'static str {
    stars_with(p, STAR_THRESHOLDS)
}

/// Stars for `p` against `thresholds` (one star below the first, two below
/// the second, …), `ns` when `p` is at or above the first.
#[must_use]
pub fn stars_with(p: f64, thresholds: [f64; 4]) -> &'static str {
    const STARS: [&str; 5] = ["ns", "*", "**", "***", "****"];
    STARS[thresholds.iter().take_while(|t| p < **t).count()]
}

/// The key to the stars, as written under a graph with brackets.
pub const STAR_KEY: &str = "* p < 0.05, ** p < 0.01, *** p < 0.001, **** p < 0.0001, ns not significant";

/// The key to the stars for `thresholds`.
#[must_use]
pub fn star_key(thresholds: [f64; 4]) -> String {
    let mut parts: Vec<String> =
        thresholds.iter().enumerate().map(|(i, t)| format!("{} p < {}", "*".repeat(i + 1), format_value(*t))).collect();
    parts.push("ns not significant".to_owned());
    parts.join(", ")
}

/// One comparison of two categories.
#[derive(Debug, Clone, PartialEq)]
pub struct BracketComparison {
    /// Position of the first category on the horizontal axis (1, 2, …).
    pub first: f64,
    /// Position of the second category.
    pub second: f64,
    /// Mean of the second minus mean of the first (data units).
    pub difference: f64,
    /// Adjusted p-value.
    pub p_value: f64,
}

/// The tier (0 = lowest) of each interval `(lo, hi)` so that intervals in
/// one tier never overlap or touch: intervals are taken shortest first (ties
/// left to right), each into the lowest tier where it meets no interval
/// already there. Short brackets thus sit low, close to their data, and wide
/// ones arch over them.
#[must_use]
pub fn bracket_tiers(intervals: &[(f64, f64)]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..intervals.len()).collect();
    let span = |i: usize| (intervals[i].1 - intervals[i].0).abs();
    order.sort_by(|&a, &b| span(a).total_cmp(&span(b)).then(intervals[a].0.total_cmp(&intervals[b].0)));
    let mut tiers: Vec<Vec<(f64, f64)>> = Vec::new();
    let mut out = vec![0; intervals.len()];
    for i in order {
        let (lo, hi) = (intervals[i].0.min(intervals[i].1), intervals[i].0.max(intervals[i].1));
        let tier = tiers.iter().position(|t| t.iter().all(|&(a, b)| hi < a || lo > b)).unwrap_or(tiers.len());
        if tier == tiers.len() {
            tiers.push(Vec::new());
        }
        tiers[tier].push((lo, hi));
        out[i] = tier;
    }
    out
}

/// Significance brackets: for categories across and a continuous column up,
/// compares the categories' means with a multiple-comparison procedure
/// ([`BracketTest`]: Tukey–Kramer by default, Games–Howell, Holm-adjusted t,
/// or Dunnett against the first category) and draws a bracket above the
/// data between each compared pair (or only the pairs the user picked),
/// labelled with the adjusted p-value, stars ([`stars_with`], thresholds of
/// the user's choice) or both; pairs that are not significant can read
/// `ns`. Optionally only pairs with `p < alpha` are drawn.
///
/// Each bracket sits a gap above the tallest point of the categories it
/// spans (from the first to the second); a bracket that would meet one
/// already placed over an overlapping span is raised above it, shortest
/// spans first, so short brackets sit close to their data and wide ones arch
/// over them ([`place_brackets`]).
///
/// The comparisons use the rows that take part in analyses (not excluded,
/// inside the graph's filter), each counted as many times as its frequency
/// (rounded); subgroups within a category are pooled, and weights are not
/// used. Categories with fewer than two values are left out. The star key and
/// the procedure are written under the graph.
#[derive(Debug, Clone, PartialEq)]
pub struct BracketElement {
    /// The multiple-comparison procedure.
    pub test: BracketTest,
    /// What each bracket says.
    pub label: BracketLabel,
    /// Draw only brackets with `p < alpha`.
    pub only_significant: bool,
    /// Significance level for [`BracketElement::only_significant`].
    pub alpha: f64,
    /// The pairs to draw, by category label (empty: every pair the
    /// procedure compares). A pair the procedure does not compare (Dunnett
    /// compares each category with the first only) is left out with a note.
    pub pairs: Vec<(String, String)>,
    /// p-values below which one, two, three and four stars are written.
    pub thresholds: [f64; 4],
    /// Write `ns` on pairs that are not significant (p at or above the
    /// first threshold), whatever [`BracketElement::label`] says.
    pub ns_label: bool,
    /// Gap between the tallest point under a bracket and the bracket, as a
    /// fraction of the panel's vertical spread.
    pub gap: f64,
}

impl Default for BracketElement {
    fn default() -> Self {
        Self {
            test: BracketTest::TukeyKramer,
            label: BracketLabel::Both,
            only_significant: false,
            alpha: 0.05,
            pairs: Vec::new(),
            thresholds: STAR_THRESHOLDS,
            ns_label: false,
            gap: 0.07,
        }
    }
}

/// Heights of brackets over spans `(first, second)` whose data reach
/// `tops` (the tallest point under each span): each sits `gap` above its
/// own top, and is raised to `step` above every bracket already placed over
/// a span it overlaps or touches, so it arches over them. Spans are placed
/// shortest first (ties left to right).
#[must_use]
pub fn place_brackets(spans: &[(f64, f64)], tops: &[f64], gap: f64, step: f64) -> Vec<f64> {
    let mut order: Vec<usize> = (0..spans.len()).collect();
    let width = |i: usize| (spans[i].1 - spans[i].0).abs();
    order.sort_by(|&a, &b| width(a).total_cmp(&width(b)).then(spans[a].0.total_cmp(&spans[b].0)));
    let mut placed: Vec<(f64, f64, f64)> = Vec::new();
    let mut out = vec![0.0; spans.len()];
    for i in order {
        let (lo, hi) = (spans[i].0.min(spans[i].1), spans[i].0.max(spans[i].1));
        let above = placed.iter().filter(|&&(a, b, _)| !(hi < a || lo > b)).map(|p| p.2 + step);
        let y = above.fold(tops[i] + gap, f64::max);
        placed.push((lo, hi, y));
        out[i] = y;
    }
    out
}

impl BracketElement {
    /// The comparisons the brackets show for `panel` (before
    /// [`BracketElement::only_significant`] is applied), with notes about
    /// categories left out.
    ///
    /// # Errors
    /// A message when the panel has no categories across, fewer than two
    /// categories with two or more values, or the procedure fails (e.g. no
    /// variation within the categories).
    pub fn comparisons(&self, panel: &Panel) -> Result<(Vec<BracketComparison>, Vec<String>), String> {
        if !categories_across(panel) || panel.y.name.is_empty() {
            return Err("Significance brackets need categories across and a continuous column up.".to_owned());
        }
        let levels = panel.x.levels();
        let mut values: Vec<Vec<f64>> = vec![Vec::new(); levels.len()];
        for p in panel.points.iter().filter(|p| p.in_analysis()) {
            let k = p.x.round();
            let y = panel.y.scale.inverse(p.y);
            if k >= 1.0 && (k as usize) <= levels.len() && y.is_finite() {
                let copies = p.freq.round().max(0.0) as usize;
                values[k as usize - 1].extend(std::iter::repeat_n(y, copies));
            }
        }
        let mut notes = Vec::new();
        let kept: Vec<usize> = (0..levels.len()).filter(|&k| values[k].len() >= 2).collect();
        let dropped: Vec<&str> = (0..levels.len()).filter(|k| !kept.contains(k)).map(|k| levels[k].as_str()).collect();
        if !dropped.is_empty() {
            notes.push(format!("Brackets leave out categories with fewer than two values: {}.", dropped.join(", ")));
        }
        if kept.len() < 2 {
            return Err("Significance brackets need two or more categories with two or more values each.".to_owned());
        }
        let groups: Vec<&[f64]> = kept.iter().map(|&k| values[k].as_slice()).collect();
        let position = |i: usize| (kept[i] + 1) as f64;
        let out = match self.test {
            BracketTest::Dunnett => {
                let d =
                    dunnett_test(groups[0], &groups[1..], 0.95, Alternative::TwoSided).map_err(|e| e.to_string())?;
                d.comparisons
                    .iter()
                    .map(|c| BracketComparison {
                        first: position(0),
                        second: position(c.treatment + 1),
                        difference: c.difference,
                        p_value: c.p_value,
                    })
                    .collect()
            }
            test => {
                let method = match test {
                    BracketTest::GamesHowell => ComparisonMethod::GamesHowell,
                    BracketTest::HolmT => ComparisonMethod::HolmT,
                    _ => ComparisonMethod::TukeyKramer,
                };
                compare_all_pairs(&groups, method, 0.95)
                    .map_err(|e| e.to_string())?
                    .iter()
                    .map(|c| BracketComparison {
                        first: position(c.first),
                        second: position(c.second),
                        difference: -c.difference,
                        p_value: c.p_value,
                    })
                    .collect()
            }
        };
        if panel.groups.len() > 1 {
            notes.push("Brackets compare the horizontal categories, pooling the subgroups within each.".to_owned());
        }
        Ok((out, notes))
    }

    /// The text of a bracket with adjusted p-value `p`.
    #[must_use]
    pub fn text(&self, p: f64) -> String {
        let marks = stars_with(p, self.thresholds);
        if self.ns_label && marks == "ns" {
            return "ns".to_owned();
        }
        let value = if p < 1e-4 { "p < 0.0001".to_owned() } else { format!("p = {}", format_value(p)) };
        match self.label {
            BracketLabel::PValue => value,
            BracketLabel::Stars => marks.to_owned(),
            BracketLabel::Both => format!("{value} {marks}"),
        }
    }

    /// `true` when the pair of categories at positions `first` and
    /// `second` (labels `levels`) is to be drawn.
    fn picked(&self, levels: &[String], first: f64, second: f64) -> bool {
        if self.pairs.is_empty() {
            return true;
        }
        let name = |x: f64| levels.get((x.round() as usize).wrapping_sub(1)).map(String::as_str);
        let (Some(a), Some(b)) = (name(first), name(second)) else { return false };
        self.pairs.iter().any(|(p, q)| (p == a && q == b) || (p == b && q == a))
    }
}

impl Element for BracketElement {
    fn kind(&self) -> ElementKind {
        ElementKind::Brackets
    }

    fn layer(&self, panel: &Panel, _state: &RowStateManager) -> Layer {
        let mut layer = Layer::default();
        let (comparisons, notes) = match self.comparisons(panel) {
            Ok(c) => c,
            Err(message) => {
                layer.notes.push(message);
                return layer;
            }
        };
        layer.notes.extend(notes);
        let levels = panel.x.levels();
        let picked: Vec<&BracketComparison> =
            comparisons.iter().filter(|c| self.picked(levels, c.first, c.second)).collect();
        if picked.len() < self.pairs.len() {
            layer.notes.push(format!(
                "{} chosen pair(s) are not compared by {} and are not drawn.",
                self.pairs.len() - picked.len(),
                self.test.short()
            ));
        }
        let shown: Vec<&BracketComparison> =
            picked.iter().copied().filter(|c| !self.only_significant || c.p_value < self.alpha).collect();
        let (lo, hi) = panel
            .points
            .iter()
            .map(|p| p.y)
            .filter(|y| y.is_finite())
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), y| (a.min(y), b.max(y)));
        if !lo.is_finite() {
            return layer;
        }
        let span = if hi > lo { hi - lo } else { hi.abs().max(1.0) };
        let (step, tick) = (0.1 * span, 0.025 * span);
        // The tallest point of the categories each bracket spans.
        let tops: Vec<f64> = shown
            .iter()
            .map(|c| {
                let (a, b) = (c.first.min(c.second) - 0.5, c.first.max(c.second) + 0.5);
                panel
                    .points
                    .iter()
                    .filter(|p| p.y.is_finite() && p.x > a && p.x < b)
                    .map(|p| p.y)
                    .fold(f64::NEG_INFINITY, f64::max)
            })
            .map(|t| if t.is_finite() { t } else { hi })
            .collect();
        let spans: Vec<(f64, f64)> = shown.iter().map(|c| (c.first, c.second)).collect();
        let heights = place_brackets(&spans, &tops, self.gap.max(0.0) * span, step);
        for (c, y) in shown.iter().zip(heights) {
            let (a, b) = (c.first.min(c.second) + 0.04, c.first.max(c.second) - 0.04);
            layer.marks.push(Mark::Line {
                points: vec![[a, y - tick], [a, y], [b, y], [b, y - tick]],
                color: palette::LABEL,
                width: 1.1,
                pattern: LinePattern::Solid,
                name: "Significance bracket".to_owned(),
            });
            layer.marks.push(Mark::Text {
                at: [0.5 * (a + b), y],
                text: self.text(c.p_value),
                color: palette::LABEL,
                anchor: TextAnchor::Above,
                movable: None,
            });
        }
        if !shown.is_empty() {
            layer.key_entry(
                format!("Brackets: {} adjusted p-values", self.test.short()),
                palette::LABEL,
                KeyGlyph::Line(LinePattern::Solid),
            );
        }
        let hidden = picked.len() - shown.len();
        let mut note = format!("Significance brackets: {} comparisons (95% family)", self.test.short());
        if self.label != BracketLabel::PValue || self.ns_label {
            note.push_str(&format!("; {}", star_key(self.thresholds)));
        }
        if hidden > 0 {
            note.push_str(&format!("; {hidden} pair(s) with p ≥ {} not drawn", format_value(self.alpha)));
        }
        note.push('.');
        layer.notes.push(note);
        layer
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoding::EncodingPlan;
    use crate::frame::{FrameOptions, build_frame};
    use stat_data::{Column, DataTable};

    #[test]
    fn tiers_never_overlap() {
        let pairs = [(1.0, 2.0), (2.0, 3.0), (1.0, 3.0), (3.0, 4.0), (1.0, 4.0), (2.0, 4.0)];
        let t = bracket_tiers(&pairs);
        for i in 0..pairs.len() {
            for j in i + 1..pairs.len() {
                if t[i] == t[j] {
                    let (a, b) = (pairs[i], pairs[j]);
                    assert!(a.1 < b.0 || b.1 < a.0, "{a:?} and {b:?} share tier {}", t[i]);
                }
            }
        }
        // Neighbors first: 1–2 and 3–4 share the lowest tier.
        assert_eq!((t[0], t[3]), (0, 0));
        assert!(t[4] > t[2] && t[2] > t[0]);
        assert_eq!(stars(0.00001), "****");
        assert_eq!(stars(0.0499), "*");
        assert_eq!(stars(0.05), "ns");
    }

    /// Each bracket clears the tallest point it spans by the gap, and
    /// brackets over overlapping spans are a step apart, wider over narrower.
    #[test]
    fn brackets_sit_above_their_own_data() {
        let spans = [(1.0, 2.0), (2.0, 3.0), (1.0, 3.0), (3.0, 4.0)];
        let tops = [5.0, 9.0, 9.0, 2.0];
        let h = place_brackets(&spans, &tops, 1.0, 0.5);
        assert_eq!(h[0], 6.0, "own top 5 + gap");
        assert_eq!(h[1], 10.0, "own top 9 + gap");
        // 3–4 is narrow but touches 2–3: one step above it.
        assert_eq!(h[3], 10.5);
        // 1–3 overlaps every other: above the highest it meets.
        assert_eq!(h[2], 11.0);
        assert_eq!(stars_with(0.03, [0.1, 0.05, 0.01, 0.001]), "**");
        assert_eq!(stars_with(0.2, [0.1, 0.05, 0.01, 0.001]), "ns");
        assert_eq!(star_key(STAR_THRESHOLDS), STAR_KEY);
    }

    /// Picked pairs, ns labels and per-pair heights on the SciPy data.
    #[test]
    fn picked_pairs_and_ns_labels() {
        let p = panel();
        let k = BracketElement {
            pairs: vec![("A".into(), "C".into()), ("B".into(), "A".into())],
            ns_label: true,
            ..BracketElement::default()
        };
        let layer = k.layer(&p, &RowStateManager::new(15));
        let texts: Vec<&str> = layer
            .marks
            .iter()
            .filter_map(|m| if let Mark::Text { text, .. } = m { Some(text.as_str()) } else { None })
            .collect();
        assert_eq!(texts.len(), 2);
        assert!(texts.contains(&"ns"), "{texts:?}");
        assert!(texts.iter().any(|t| t.ends_with(" *")), "{texts:?}");
        // A–B spans A (max 29.9) and B (max 34.2): it clears 34.2.
        let lines: Vec<f64> = layer
            .marks
            .iter()
            .filter_map(|m| if let Mark::Line { points, .. } = m { Some(points[1][1]) } else { None })
            .collect();
        assert!(lines.iter().all(|y| *y > 34.2), "{lines:?}");
        // Dunnett compares nothing with B–C: the pair is noted, not drawn.
        let d = BracketElement {
            test: BracketTest::Dunnett,
            pairs: vec![("B".into(), "C".into())],
            ..BracketElement::default()
        };
        let layer = d.layer(&p, &RowStateManager::new(15));
        assert!(layer.notes.iter().any(|n| n.contains("not compared")), "{:?}", layer.notes);
        assert!(!layer.marks.iter().any(|m| matches!(m, Mark::Line { .. })));
    }

    /// SciPy's documentation data for `tukey_hsd`: three groups of five.
    fn panel() -> Panel {
        let a = [24.5, 23.5, 26.4, 27.1, 29.9];
        let b = [28.4, 34.2, 29.5, 32.2, 30.1];
        let c = [26.1, 28.3, 24.3, 26.2, 27.8];
        let mut g = Vec::new();
        let mut y = Vec::new();
        for (name, vals) in [("A", a), ("B", b), ("C", c)] {
            for v in vals {
                g.push(name.to_owned());
                y.push(v);
            }
        }
        let t = DataTable::new("t", vec![Column::text("g", g), Column::numeric("y", y)]).unwrap();
        let state = RowStateManager::new(15);
        let plan = EncodingPlan { x: vec!["g".into()], y: vec!["y".into()], ..Default::default() };
        build_frame(&t, &state, &plan, &FrameOptions::default()).unwrap().panels.remove(0)
    }

    /// Adjusted p-values against independent references: R 4.3 `TukeyHSD`,
    /// `pairwise.t.test` and the Games–Howell formula with `ptukey`, and
    /// SciPy 1.18 `stats.dunnett` (see docs/validation.md).
    #[test]
    fn bracket_p_values_match_references() {
        let p = panel();
        let check = |test: BracketTest, want: &[(f64, f64, f64)], tol: f64| {
            let (got, _) = BracketElement { test, ..BracketElement::default() }.comparisons(&p).unwrap();
            assert_eq!(got.len(), want.len());
            for (g, w) in got.iter().zip(want) {
                assert_eq!((g.first, g.second), (w.0, w.1));
                assert!((g.p_value - w.2).abs() < tol * w.2, "{test:?} {g:?} vs {w:?}");
            }
        };
        // R 4.3 `TukeyHSD(aov(y ~ g))`.
        let tukey = [
            (1.0, 2.0, 0.014_448_326_736_504_42),
            (1.0, 3.0, 0.980_310_724_094_107_8),
            (2.0, 3.0, 0.020_331_136_739_916_49),
        ];
        check(BracketTest::TukeyKramer, &tukey, 1e-6);
        check(BracketTest::HolmT, &HOLM, 1e-8);
        check(BracketTest::GamesHowell, &GAMES_HOWELL, 1e-6);
        check(BracketTest::Dunnett, &DUNNETT, 2e-3);
        let (diffs, _) = BracketElement::default().comparisons(&p).unwrap();
        assert!((diffs[0].difference - 4.6).abs() < 1e-9, "B − A");
        let layer = BracketElement::default().layer(&p, &RowStateManager::new(15));
        assert_eq!(layer.marks.iter().filter(|m| matches!(m, Mark::Line { .. })).count(), 3);
        assert!(layer.notes.iter().any(|n| n.contains(STAR_KEY)));
        let only =
            BracketElement { only_significant: true, ..BracketElement::default() }.layer(&p, &RowStateManager::new(15));
        assert_eq!(only.marks.iter().filter(|m| matches!(m, Mark::Line { .. })).count(), 2);
        assert!(only.notes.iter().any(|n| n.contains("1 pair(s)")));
    }

    /// R 4.3 `pairwise.t.test(y, g, pool.sd = TRUE, p.adjust.method = "holm")`.
    const HOLM: [(f64, f64, f64); 3] = [
        (1.0, 2.0, 0.016_927_936_077_426_858),
        (1.0, 3.0, 0.852_421_355_355_717_8),
        (2.0, 3.0, 0.016_927_936_077_426_858),
    ];
    /// R 4.3: `ptukey(√2·|ȳᵢ − ȳⱼ|/SE, 3, Welch df, lower.tail = FALSE)`.
    const GAMES_HOWELL: [(f64, f64, f64); 3] =
        [(1.0, 2.0, 0.039_058_048_144_920_2), (1.0, 3.0, 0.978_816_392_337_122), (2.0, 3.0, 0.024_702_450_398_346_84)];
    /// SciPy 1.18 `stats.dunnett(B, C, control=A)` (quasi-Monte Carlo
    /// integration, so compared to 0.2%).
    const DUNNETT: [(f64, f64, f64); 2] = [(1.0, 2.0, 0.010_470_262_355_756_43), (1.0, 3.0, 0.973_922_672_566_471_2)];
}
