//! Distributions of one continuous column across: histograms, Wilkinson dot
//! plots and probability plots against a fitted distribution.
//!
//! All three use the horizontal variable only; the vertical axis shows
//! counts (or density), stacked dots, or standard quantiles, so they need no
//! vertical column.

use stat_core::discrete::beta_quantile;
use stat_core::distribution_fit::{FitFamily, FittedDistribution, fit_distribution};
use stat_data::RowStateManager;

use super::{Element, ElementKind, group_color, group_prefix, included_by_group, split_key_color};
use crate::frame::{Panel, PanelPoint};
use crate::layer::{KeyGlyph, Layer, LinePattern, Mark, MarkShape, Region, Rgba, TextAnchor};
use crate::palette;
use crate::scale::AxisScale;

/// How histogram bins are chosen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HistogramBins {
    /// Sturges' rule (⌈log₂ n⌉ + 1 bins), widened to a round width.
    Auto,
    /// About this many bins, widened to a round width.
    Count(usize),
    /// This width, in the horizontal axis's units (widened, with a note,
    /// when it would need more than 10 000 bins).
    Width(f64),
}

/// A histogram of the horizontal variable: rows counted (with their
/// frequencies) in equal-width bins on the horizontal axis's scale, per
/// subgroup overlaid (translucent) or stacked. Optionally scaled to density,
/// so each subgroup's bars have area 1. The selected share of each bar is
/// shaded and clicking a bar selects its rows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HistogramElement {
    /// Bin choice.
    pub bins: HistogramBins,
    /// Stack subgroups instead of overlaying them.
    pub stacked: bool,
    /// Heights as density (count / (n × width)) instead of counts.
    pub density: bool,
}

impl Default for HistogramElement {
    fn default() -> Self {
        Self { bins: HistogramBins::Auto, stacked: false, density: false }
    }
}

/// Most bins a histogram draws; a narrower width is widened to fit.
const MAX_BINS: usize = 10_000;

/// A round bin width at least `raw`: 1, 2, 2.5 or 5 × 10ⁿ.
fn nice_width(raw: f64) -> f64 {
    if !(raw > 0.0 && raw.is_finite()) {
        return 1.0;
    }
    let base = 10f64.powf(raw.log10().floor());
    [1.0, 2.0, 2.5, 5.0, 10.0].iter().map(|m| m * base).find(|w| *w >= raw * (1.0 - 1e-12)).unwrap_or(10.0 * base)
}

fn rect(x0: f64, x1: f64, y0: f64, y1: f64) -> Vec<[f64; 2]> {
    vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1], [x0, y0]]
}

/// `true` when the panel suits a one-variable layer: numbers across and no
/// vertical column.
fn one_variable(panel: &Panel) -> Result<(), String> {
    if panel.x.is_categorical() || panel.x.name.is_empty() {
        Err("needs a continuous column across".to_owned())
    } else if !panel.y.name.is_empty() {
        Err("uses the horizontal column only; clear the vertical column".to_owned())
    } else {
        Ok(())
    }
}

fn selected_count(pts: &[&PanelPoint], state: &RowStateManager) -> f64 {
    pts.iter().filter(|p| state.is_selected(p.row)).map(|p| p.freq).sum()
}

impl Element for HistogramElement {
    fn kind(&self) -> ElementKind {
        ElementKind::Histogram
    }

    fn layer(&self, panel: &Panel, state: &RowStateManager) -> Layer {
        let mut layer = Layer::default();
        if let Err(why) = one_variable(panel) {
            layer.notes.push(format!("The histogram {why}."));
            return layer;
        }
        let groups = included_by_group(panel);
        let all: Vec<&PanelPoint> = groups.iter().flatten().copied().collect();
        let (lo, hi) = all.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(p.x), b.max(p.x)));
        if !(lo.is_finite() && hi.is_finite()) {
            return layer;
        }
        let n_total: f64 = all.iter().map(|p| p.freq).sum();
        // A single value gets bins scaled to its size; a span too wide for
        // binary64 is capped so bin edges stay finite.
        let span = if hi > lo { (hi - lo).min(f64::MAX / 4.0) } else { lo.abs().max(1.0) };
        let mut width = match self.bins {
            HistogramBins::Width(w) if w > 0.0 && w.is_finite() => w,
            HistogramBins::Count(k) if k > 0 => nice_width(span / k as f64),
            _ => nice_width(span / (n_total.max(1.0).log2().ceil() + 1.0)),
        };
        // A width that would need more than MAX_BINS bins is widened to a
        // round width that fits, rather than piling the rest into the last bin.
        let needed = ((hi - (lo / width).floor() * width) / width).floor() + 1.0;
        if needed.is_nan() || needed > MAX_BINS as f64 {
            let requested = width;
            width = nice_width(span / (MAX_BINS - 2) as f64);
            layer.notes.push(format!(
                "Bins of width {} would number more than {MAX_BINS}; the width is widened to {}.",
                crate::view::format_value(requested),
                crate::view::format_value(width)
            ));
        }
        let start = (lo / width).floor() * width;
        let n_bins = (((hi - start) / width).floor() as usize + 1).clamp(1, MAX_BINS);
        let bin_of = |x: f64| (((x - start) / width).floor().max(0.0) as usize).min(n_bins - 1);
        let stacked = self.stacked && groups.len() > 1;
        let mut base = vec![0.0; n_bins];
        for (g, pts) in groups.iter().enumerate() {
            let mut bins: Vec<Vec<&PanelPoint>> = vec![Vec::new(); n_bins];
            for p in pts {
                bins[bin_of(p.x)].push(p);
            }
            let n_group: f64 = pts.iter().map(|p| p.freq).sum();
            if n_group <= 0.0 {
                continue;
            }
            let divisor = if !self.density {
                1.0
            } else if stacked {
                n_total * width
            } else {
                n_group * width
            };
            let color = group_color(panel, g, palette::BAR);
            let name = format!("{}histogram", group_prefix(panel, g));
            let alpha = if groups.len() > 1 && !stacked { 0.35 } else { 0.55 };
            for (i, bin) in bins.iter().enumerate() {
                let count: f64 = bin.iter().map(|p| p.freq).sum();
                if count <= 0.0 {
                    continue;
                }
                let x0 = start + i as f64 * width;
                if !(x0 + width).is_finite() {
                    continue;
                }
                let y0 = if stacked { base[i] } else { 0.0 };
                let y1 = y0 + count / divisor;
                if stacked {
                    base[i] = y1;
                }
                let outline = rect(x0, x0 + width, y0, y1);
                layer.marks.push(Mark::Polygon {
                    points: outline.clone(),
                    fill: color.with_alpha(alpha),
                    stroke: color,
                    name: name.clone(),
                });
                let selected = selected_count(bin, state);
                if selected > 0.0 {
                    let top = y0 + selected / divisor;
                    layer.marks.push(Mark::Polygon {
                        points: rect(x0, x0 + width, y0, top),
                        fill: Rgba::rgb(20, 20, 30).with_alpha(0.4),
                        stroke: palette::SELECTED,
                        name: "Selected rows".to_owned(),
                    });
                }
                let mut rows: Vec<usize> = bin.iter().map(|p| p.row).collect();
                rows.sort_unstable();
                rows.dedup();
                layer.regions.push(Region { outline, rows });
            }
        }
        if !layer.marks.is_empty() {
            let what = if self.density { "density" } else { "count" };
            let color = split_key_color(panel, palette::BAR);
            layer.key_entry(
                format!("Histogram: {what} per bin of width {}", crate::view::format_value(width)),
                color,
                KeyGlyph::Area(color.with_alpha(0.55)),
            );
            if layer.marks.iter().any(|m| matches!(m, Mark::Polygon { name, .. } if name == "Selected rows")) {
                layer.key_entry(
                    "Share of rows selected",
                    palette::SELECTED,
                    KeyGlyph::Area(palette::SELECTED.with_alpha(0.65)),
                );
            }
        }
        layer
    }
}

/// A Wilkinson dot plot (Wilkinson 1999, "Dot plots", *American
/// Statistician* 53(3)): one dot per row (frequencies repeat it), stacked
/// where values fall within a bin width of each other. The dot-density
/// algorithm scans the sorted values from the left: a stack takes every value
/// within `width` of its first, and sits at the midpoint of its smallest and
/// largest value. Subgroups get their own stacks, in their colors. Each dot
/// is a row: selected rows are ringed, and clicking a dot selects its row.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DotPlotElement {
    /// Stack width in the horizontal axis's units (`None`: 1/30 of the range).
    pub width: Option<f64>,
}

/// Stacks of the dot-density algorithm: `(position, members)` with members
/// in ascending order.
fn dot_stacks<'a>(sorted: &[&'a PanelPoint], width: f64) -> Vec<(f64, Vec<&'a PanelPoint>)> {
    let mut out: Vec<(f64, Vec<&PanelPoint>)> = Vec::new();
    let mut i = 0;
    while i < sorted.len() {
        let first = sorted[i].x;
        let mut j = i;
        while j < sorted.len() && sorted[j].x - first < width * (1.0 - 1e-12) {
            j += 1;
        }
        let members = sorted[i..j].to_vec();
        let last = members.last().map_or(first, |p| p.x);
        out.push((0.5 * (first + last), members));
        i = j;
    }
    out
}

impl Element for DotPlotElement {
    fn kind(&self) -> ElementKind {
        ElementKind::DotPlot
    }

    fn layer(&self, panel: &Panel, state: &RowStateManager) -> Layer {
        let mut layer = Layer::default();
        if let Err(why) = one_variable(panel) {
            layer.notes.push(format!("The dot plot {why}."));
            return layer;
        }
        let groups = included_by_group(panel);
        let (lo, hi) =
            groups.iter().flatten().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(p.x), b.max(p.x)));
        if !(lo.is_finite() && hi.is_finite()) {
            return layer;
        }
        let width = match self.width {
            Some(w) if w > 0.0 && w.is_finite() => w,
            _ => ((hi - lo) / 30.0).max(f64::MIN_POSITIVE),
        };
        let mut heights: std::collections::BTreeMap<i64, f64> = std::collections::BTreeMap::new();
        let mut tallest = 0.0_f64;
        for (g, pts) in groups.iter().enumerate() {
            let mut sorted = pts.clone();
            sorted.sort_by(|a, b| a.x.total_cmp(&b.x));
            let color = group_color(panel, g, palette::DEFAULT_POINT);
            let shape = panel.groups.get(g).map_or(MarkShape::Circle, |s| s.shape);
            let (mut dots, mut selected) = (Vec::new(), Vec::new());
            for (x, members) in dot_stacks(&sorted, width) {
                // Stacks of different subgroups at the same place continue upwards.
                let level = heights.entry(crate::element::position_key(x / width, 1e6)).or_insert(0.0);
                for p in members {
                    // At most a thousand dots per row, so a huge frequency cannot exhaust memory.
                    let copies = p.freq.round().clamp(1.0, 1000.0) as usize;
                    for _ in 0..copies {
                        let y = *level + 0.5;
                        *level += 1.0;
                        dots.push([x, y]);
                        if state.is_selected(p.row) {
                            selected.push([x, y]);
                        }
                        let (hw, hh) = (0.5 * width, 0.5);
                        layer.regions.push(Region { outline: rect(x - hw, x + hw, y - hh, y + hh), rows: vec![p.row] });
                    }
                }
                tallest = tallest.max(*level);
            }
            if !dots.is_empty() {
                layer.marks.push(Mark::Points {
                    points: dots,
                    color,
                    shape,
                    radius: 4.0,
                    filled: true,
                    name: format!("{}dots", group_prefix(panel, g)),
                });
            }
            if !selected.is_empty() {
                layer.marks.push(Mark::Points {
                    points: selected,
                    color: palette::SELECTED,
                    shape: MarkShape::Circle,
                    radius: 6.0,
                    filled: false,
                    name: "Selected rows".to_owned(),
                });
            }
        }
        if !layer.marks.is_empty() {
            layer.key_entry(
                format!("Dot plot: one dot per row, stacks {} wide", crate::view::format_value(width)),
                palette::DEFAULT_POINT,
                KeyGlyph::Marker(MarkShape::Circle, true),
            );
            // Keep the stacks' tops in view with a little headroom.
            layer.marks.push(Mark::Points {
                points: vec![[lo, 0.0], [hi, tallest + 0.5]],
                color: Rgba([0, 0, 0, 0]),
                shape: MarkShape::Circle,
                radius: 0.0,
                filled: false,
                name: String::new(),
            });
        }
        layer
    }
}

/// A probability plot of the horizontal variable against a distribution
/// fitted to it by maximum likelihood ([`stat_core::distribution_fit`]),
/// one per subgroup: each ordered value `x₍ᵢ₎` at the standard quantile of
/// its median-rank plotting position `(i − 0.3)/(n + 0.4)` (Benard &
/// Bos-Levenbach 1953), with the fitted relationship drawn as a line. On the
/// family's natural horizontal scale (logarithmic for the lognormal, Weibull
/// and log-logistic) the line is straight, and points along it show a good
/// fit.
///
/// The band holds, for each plotting position, the central `level` range of
/// the `i`-th order statistic when the fitted distribution is true:
/// `F̂⁻¹(B⁻¹(α/2; i, n − i + 1))` to `F̂⁻¹(B⁻¹(1 − α/2; i, n − i + 1))`
/// (pointwise; David & Nagaraja, *Order Statistics*, §2.1). Dotted guides
/// mark cumulative percentages on the vertical axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProbabilityPlotElement {
    /// Distribution fitted to each subgroup.
    pub family: FitFamily,
    /// Draw the order-statistic band.
    pub band: bool,
    /// Coverage of the band.
    pub level: f64,
}

impl Default for ProbabilityPlotElement {
    fn default() -> Self {
        Self { family: FitFamily::Normal, band: true, level: 0.95 }
    }
}

/// Cumulative percentages marked by guides.
const GUIDES: [f64; 9] = [0.01, 0.05, 0.1, 0.25, 0.5, 0.75, 0.9, 0.95, 0.99];

impl ProbabilityPlotElement {
    fn group_marks(
        &self,
        panel: &Panel,
        g: usize,
        pts: &[&PanelPoint],
        layer: &mut Layer,
    ) -> Option<FittedDistribution> {
        let sx = panel.x.scale;
        let mut data: Vec<(f64, &PanelPoint)> = pts.iter().map(|p| (sx.inverse(p.x), *p)).collect();
        data.sort_by(|a, b| a.0.total_cmp(&b.0));
        let values: Vec<f64> = data.iter().map(|d| d.0).collect();
        let fit = match fit_distribution(&values, self.family) {
            Ok(f) => f,
            Err(e) => {
                layer.notes.push(format!("{}{}: {e}", group_prefix(panel, g), self.family.label()));
                return None;
            }
        };
        let n = values.len() as f64;
        let color = group_color(panel, g, palette::DEFAULT_POINT);
        let shape = panel.groups.get(g).map_or(MarkShape::Circle, |s| s.shape);
        let mut points = Vec::with_capacity(data.len());
        let (mut lower, mut upper) = (Vec::new(), Vec::new());
        let alpha = 1.0 - self.level;
        for (i, (_, p)) in data.iter().enumerate() {
            let w = fit.standard_quantile((i as f64 + 0.7) / (n + 0.4));
            if !w.is_finite() {
                continue;
            }
            points.push([p.x, w]);
            if self.band {
                let a = (i + 1) as f64;
                let b = n - i as f64;
                let (Ok(ql), Ok(qu)) = (beta_quantile(a, b, alpha / 2.0), beta_quantile(a, b, 1.0 - alpha / 2.0))
                else {
                    continue;
                };
                let (xl, xu) = (sx.forward(fit.quantile(ql)), sx.forward(fit.quantile(qu)));
                if let (Some(xl), Some(xu)) = (xl, xu) {
                    if xl.is_finite() && xu.is_finite() {
                        lower.push([xl, w]);
                        upper.push([xu, w]);
                    }
                }
            }
        }
        let prefix = group_prefix(panel, g);
        if lower.len() > 1 {
            layer.marks.push(Mark::Band {
                lower,
                upper,
                color: color.with_alpha(0.12),
                name: format!("{prefix}{} band", super::percent(self.level)),
            });
        }
        // The fitted line, traced across the data on the axis scale.
        let (lo, hi) = (points.first()?[0], points.last()?[0]);
        let line: Vec<[f64; 2]> = (0..=120)
            .filter_map(|k| {
                let t = lo + (hi - lo) * f64::from(k) / 120.0;
                let w = fit.standardize(sx.inverse(t));
                w.is_finite().then_some([t, w])
            })
            .collect();
        if line.len() > 1 {
            layer.marks.push(Mark::Line {
                points: line,
                color: group_color(panel, g, palette::FIT),
                width: 2.0,
                pattern: LinePattern::Solid,
                name: format!("{prefix}{} fit", self.family.label()),
            });
        }
        layer.marks.push(Mark::Points {
            points,
            color,
            shape,
            radius: 4.0,
            filled: true,
            name: format!("{prefix}ordered values"),
        });
        Some(fit)
    }
}

impl Element for ProbabilityPlotElement {
    fn kind(&self) -> ElementKind {
        ElementKind::ProbabilityPlot
    }

    fn layer(&self, panel: &Panel, _state: &RowStateManager) -> Layer {
        let mut layer = Layer::default();
        if let Err(why) = one_variable(panel) {
            layer.notes.push(format!("The probability plot {why}."));
            return layer;
        }
        let groups = included_by_group(panel);
        let mut fits = Vec::new();
        for (g, pts) in groups.iter().enumerate() {
            if let Some(f) = self.group_marks(panel, g, pts, &mut layer) {
                fits.push((g, f));
            }
        }
        if groups.iter().flatten().any(|p| p.freq != 1.0) {
            layer.notes.push("The probability plot counts each row once (frequencies are not used).".to_owned());
        }
        let Some((_, first)) = fits.first() else { return layer };
        // Percentage guides across the plotted range.
        let (lo, hi) = layer
            .marks
            .iter()
            .flat_map(|m| m.coordinates().map(|p| p[0]).collect::<Vec<_>>())
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), x| (a.min(x), b.max(x)));
        if lo.is_finite() && hi > lo {
            let mut guides = Vec::new();
            for p in GUIDES {
                let w = first.standard_quantile(p);
                if !w.is_finite() {
                    continue;
                }
                guides.push([[lo, w], [hi, w]]);
                layer.marks.push(Mark::Text {
                    at: [hi, w],
                    text: super::percent(p),
                    color: palette::LABEL,
                    anchor: TextAnchor::RightOfText,
                    movable: None,
                });
            }
            layer.marks.insert(
                0,
                Mark::Segments {
                    segments: guides,
                    color: palette::BOX.with_alpha(0.35),
                    width: 0.8,
                    name: "cumulative percentage".to_owned(),
                },
            );
        }
        let family = self.family.label();
        layer.key_entry(
            format!("Probability plot: {family} fitted by maximum likelihood; up: standard quantile"),
            palette::FIT,
            KeyGlyph::Line(LinePattern::Solid),
        );
        if self.band {
            layer.key_entry(
                format!("{} band for each ordered value under the fit", super::percent(self.level)),
                palette::DEFAULT_POINT,
                KeyGlyph::Area(palette::DEFAULT_POINT.with_alpha(0.12)),
            );
        }
        for (g, f) in &fits {
            let params: Vec<String> =
                f.parameters().iter().map(|p| format!("{} = {}", p.name, crate::view::format_value(p.value))).collect();
            layer.notes.push(format!("{}{family}: {}", group_prefix(panel, *g), params.join(", ")));
        }
        if self.family.needs_positive() && !matches!(panel.x.scale, AxisScale::Log10 | AxisScale::Ln) {
            layer
                .notes
                .push(format!("The {family} line is straight on a logarithmic horizontal scale (Axes settings)."));
        }
        layer
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoding::EncodingPlan;
    use crate::frame::{FrameOptions, build_frame};
    use stat_core::ContinuousDistribution;
    use stat_data::{Column, DataTable};

    fn panel_of(values: Vec<f64>, group: Option<Vec<String>>) -> (Panel, RowStateManager) {
        let mut cols = vec![Column::numeric("v", values)];
        let mut plan = EncodingPlan { x: vec!["v".into()], ..EncodingPlan::default() };
        if let Some(g) = group {
            cols.push(Column::text("g", g));
            plan.color = Some("g".into());
        }
        let t = DataTable::new("t", cols).unwrap();
        let state = RowStateManager::new(t.n_rows());
        let frame = build_frame(&t, &state, &plan, &FrameOptions::default()).unwrap();
        (frame.panels.into_iter().next().unwrap(), state)
    }

    fn polygons(layer: &Layer) -> Vec<&Vec<[f64; 2]>> {
        layer
            .marks
            .iter()
            .filter_map(|m| match m {
                Mark::Polygon { points, name, .. } if name != "Selected rows" => Some(points),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn histogram_counts_and_density() {
        let values: Vec<f64> = (0..20).map(|i| f64::from(i) * 0.5).collect(); // 0 … 9.5
        let (panel, mut state) = panel_of(values, None);
        let h = HistogramElement { bins: HistogramBins::Width(2.0), ..Default::default() };
        let layer = h.layer(&panel, &state);
        let bars = polygons(&layer);
        assert_eq!(bars.len(), 5);
        // Each bin [0, 2), [2, 4), … holds four values.
        assert!(bars.iter().all(|b| (b[2][1] - 4.0).abs() < 1e-12), "{bars:?}");
        assert_eq!(layer.regions[0].rows, vec![0, 1, 2, 3]);
        // Density: total area 1.
        let d = HistogramElement { density: true, ..h };
        let layer = d.layer(&panel, &state);
        let area: f64 = polygons(&layer).iter().map(|b| (b[1][0] - b[0][0]) * (b[2][1] - b[0][1])).sum();
        assert!((area - 1.0).abs() < 1e-12);
        // The selected share is shaded.
        state.select([0, 1], stat_data::SelectionMode::Replace);
        let layer = h.layer(&panel, &state);
        assert!(layer.marks.iter().any(|m| matches!(m, Mark::Polygon { name, points, .. }
            if name == "Selected rows" && (points[2][1] - 2.0).abs() < 1e-12)));
        // Automatic bins: Sturges for 20 rows is 6 bins over 9.5, widened to a round width.
        let auto = HistogramElement::default().layer(&panel, &state);
        assert!(!polygons(&auto).is_empty());
        assert_eq!(nice_width(1.58), 2.0);
        assert_eq!(nice_width(0.023), 0.025);
    }

    #[test]
    fn too_narrow_bins_are_widened_not_piled_into_the_last() {
        // 20 000 values one apart: width 1 would need 20 000 bins. Every row
        // must still be counted in the bin that holds it.
        let values: Vec<f64> = (0..20_000).map(f64::from).collect();
        let (panel, state) = panel_of(values, None);
        let layer = HistogramElement { bins: HistogramBins::Width(1.0), ..Default::default() }.layer(&panel, &state);
        let bars = polygons(&layer);
        assert!(bars.len() <= MAX_BINS);
        let width = bars[0][1][0] - bars[0][0][0];
        // 19 999 / 9 998 rounds up to the round width 2.5: two or three values per bin.
        assert!((width - 2.5).abs() < 1e-12, "widened to a round width, got {width}");
        assert!(bars.iter().all(|b| b[2][1] <= 3.0 + 1e-12), "no bin collects the overflow");
        let total: f64 = bars.iter().map(|b| b[2][1]).sum();
        assert!((total - 20_000.0).abs() < 1e-9);
        assert!(layer.notes.iter().any(|n| n.contains("widened")), "{:?}", layer.notes);
    }

    #[test]
    fn histogram_stacks_groups() {
        let values = vec![1.0, 1.5, 2.5, 1.2, 2.2, 2.8];
        let g: Vec<String> = ["a", "a", "a", "b", "b", "b"].map(String::from).to_vec();
        let (panel, state) = panel_of(values, Some(g));
        let h = HistogramElement { bins: HistogramBins::Width(1.0), stacked: true, density: false };
        let layer = h.layer(&panel, &state);
        let tops: Vec<f64> = polygons(&layer).iter().map(|b| b[2][1]).collect();
        // Group a: [1,2) 2, [2,3) 1; group b stacks on top: [1,2) 1 → 3, [2,3) 2 → 3.
        assert_eq!(tops, vec![2.0, 1.0, 3.0, 3.0]);
    }

    #[test]
    fn dot_plot_stacks_nearby_values() {
        let values = vec![1.0, 1.05, 1.08, 2.0, 3.0, 3.01];
        let (panel, state) = panel_of(values, None);
        let layer = DotPlotElement { width: Some(0.1) }.layer(&panel, &state);
        let Mark::Points { points, .. } = &layer.marks[0] else { panic!() };
        // Stacks: {1, 1.05, 1.08} at 1.04, {2}, {3, 3.01} at 3.005.
        assert_eq!(points.len(), 6);
        assert!((points[0][0] - 1.04).abs() < 1e-12 && (points[2][1] - 2.5).abs() < 1e-12);
        assert!((points[5][0] - 3.005).abs() < 1e-12 && (points[5][1] - 1.5).abs() < 1e-12);
        assert_eq!(layer.regions.len(), 6);
        assert_eq!(layer.regions[3].rows, vec![3]);
    }

    #[test]
    fn probability_plot_is_straight_for_its_own_family() {
        // Normal plotting positions mapped through a normal quantile: exactly on the line.
        let n = 40;
        let values: Vec<f64> = (0..n)
            .map(|i| {
                10.0 + 2.0
                    * stat_core::Normal::standard().quantile((f64::from(i) + 0.7) / (f64::from(n) + 0.4)).unwrap()
            })
            .collect();
        let (panel, state) = panel_of(values, None);
        let e = ProbabilityPlotElement::default();
        let layer = e.layer(&panel, &state);
        let pts = layer
            .marks
            .iter()
            .find_map(|m| match m {
                Mark::Points { points, name, .. } if name == "ordered values" => Some(points),
                _ => None,
            })
            .unwrap();
        assert_eq!(pts.len(), 40);
        let fit = fit_distribution(&pts.iter().map(|p| p[0]).collect::<Vec<_>>(), FitFamily::Normal).unwrap();
        // The points are exactly collinear, and the fitted line (σ̂ = s) runs close to them.
        let slope = (pts[39][1] - pts[0][1]) / (pts[39][0] - pts[0][0]);
        for p in pts {
            assert!((pts[0][1] + slope * (p[0] - pts[0][0]) - p[1]).abs() < 1e-9, "{p:?}");
            assert!((fit.standardize(p[0]) - p[1]).abs() < 0.15, "{p:?}");
        }
        // The band contains every point and is horizontal (lower and upper share y).
        let Some(Mark::Band { lower, upper, .. }) = layer.marks.iter().find(|m| matches!(m, Mark::Band { .. })) else {
            panic!()
        };
        assert_eq!(lower.len(), 40);
        for ((l, u), p) in lower.iter().zip(upper).zip(pts) {
            assert_eq!(l[1], u[1]);
            assert!(l[0] < p[0] && p[0] < u[0]);
        }
        // Guides and parameter notes.
        assert!(layer.marks.iter().any(|m| matches!(m, Mark::Text { text, .. } if text == "50%")));
        assert!(layer.notes.iter().any(|n| n.starts_with("Normal: Location μ = 10")));
        assert!(layer.all_finite());
    }

    #[test]
    fn one_variable_layers_explain_unsuitable_panels() {
        let t = DataTable::new(
            "t",
            vec![Column::numeric("a", vec![1.0, 2.0, 3.0]), Column::numeric("b", vec![2.0, 1.0, 3.0])],
        )
        .unwrap();
        let plan = EncodingPlan { x: vec!["a".into()], y: vec!["b".into()], ..EncodingPlan::default() };
        let state = RowStateManager::new(3);
        let frame = build_frame(&t, &state, &plan, &FrameOptions::default()).unwrap();
        let p = &frame.panels[0];
        for layer in [
            HistogramElement::default().layer(p, &state),
            DotPlotElement::default().layer(p, &state),
            ProbabilityPlotElement::default().layer(p, &state),
        ] {
            assert!(layer.marks.is_empty());
            assert!(layer.notes[0].contains("clear the vertical column"), "{:?}", layer.notes);
        }
        // With no vertical column, the vertical axis is titled by the layer.
        let mut spec = crate::GraphSpec { histogram: Some(HistogramElement::default()), ..Default::default() };
        let (panel, _) = panel_of(vec![1.0, 2.0], None);
        assert_eq!(spec.y_title(&panel.y), "Count");
        spec.probability_plot = Some(ProbabilityPlotElement::default());
        assert_eq!(spec.y_title(&panel.y), "Standard quantile (Normal fit)");
        assert_eq!(spec.y_title(&p.y), "b", "a vertical column keeps its own title");
        // A family that needs positive values says so.
        let (panel, state) = panel_of(vec![-1.0, 2.0, 3.0, 4.0], None);
        let layer = ProbabilityPlotElement { family: FitFamily::Weibull, ..Default::default() }.layer(&panel, &state);
        assert!(layer.notes.iter().any(|n| n.contains("positive")), "{:?}", layer.notes);
    }

    /// Subgroup stacks used to share one height counter once x/width passed
    /// about 9·10¹² (the key saturated), drawing a staircase.
    #[test]
    fn dot_plot_stacks_at_large_positions_start_from_zero() {
        let x = vec![1e12, 1e12 + 1.0, 1e12 + 2.0, 1e12, 1e12 + 1.0, 1e12 + 2.0];
        let g: Vec<String> = ["a", "a", "a", "b", "b", "b"].map(String::from).to_vec();
        let (panel, state) = panel_of(x, Some(g));
        let layer = DotPlotElement::default().layer(&panel, &state);
        let dots: Vec<&Vec<[f64; 2]>> = layer
            .marks
            .iter()
            .filter_map(|m| match m {
                Mark::Points { points, filled: true, .. } => Some(points),
                _ => None,
            })
            .collect();
        assert_eq!(dots.len(), 2, "one set of dots per subgroup");
        assert_eq!(dots[0].len(), 3);
        assert_eq!(dots[1].len(), 3);
        assert!(dots[0].iter().all(|p| (p[1] - 0.5).abs() < 1e-12), "{:?}", dots[0]);
        assert!(dots[1].iter().all(|p| (p[1] - 1.5).abs() < 1e-12), "{:?}", dots[1]);
    }
}
