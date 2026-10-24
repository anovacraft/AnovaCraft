//! Summary points: one statistic per category (or per distinct x) with
//! optional error bars.

use stat_data::RowStateManager;

use super::{Element, ElementKind, data_obs, group_color, group_prefix, included_by_slot};
use crate::frame::Panel;
use crate::layer::{KeyGlyph, Layer, LinePattern, Mark, MarkShape};
use crate::palette;
use crate::summary::{ErrorBarDirection, ErrorBarLook, ErrorBars, Obs, SummaryStat, error_interval, summarize};

/// A statistic of the vertical variable for each horizontal position (each
/// category slot, or each distinct x) and subgroup, drawn as a large diamond
/// in the subgroup's color, with optional error bars around the mean
/// (standard error, confidence interval or standard deviation). Statistics
/// are of the data values (on a transformed axis they are computed before
/// the transformation); frequencies and weights are used as in
/// [`crate::summary`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SummaryElement {
    /// The statistic.
    pub stat: SummaryStat,
    /// Error bars (drawn for the mean only).
    pub error_bars: ErrorBars,
    /// Confidence level for confidence-interval bars.
    pub level: f64,
    /// How the error bars are drawn.
    pub error_look: ErrorBarLook,
}

impl Default for SummaryElement {
    fn default() -> Self {
        Self { stat: SummaryStat::Mean, error_bars: ErrorBars::None, level: 0.95, error_look: ErrorBarLook::default() }
    }
}

/// Draw error bars `lo..hi` around `mid` at `x` with caps of half-width
/// `cap` (scaled by the look), in `color` unless the look sets one, reaching
/// the sides the look asks for.
pub(crate) fn error_bar_marks(
    x: f64,
    [mid, lo, hi]: [f64; 3],
    cap: f64,
    color: crate::layer::Rgba,
    look: &ErrorBarLook,
    name: &str,
) -> Vec<Mark> {
    let line = |points: Vec<[f64; 2]>| Mark::Line {
        points,
        color: look.color.unwrap_or(color),
        width: look.width.max(0.25),
        pattern: LinePattern::Solid,
        name: name.to_owned(),
    };
    let (lo, hi) = match look.direction {
        ErrorBarDirection::Both => (lo, hi),
        ErrorBarDirection::Upper => (mid, hi),
        ErrorBarDirection::Lower => (lo, mid),
    };
    let cap = cap * look.cap.max(0.0);
    let mut out = vec![line(vec![[x, lo], [x, hi]])];
    if cap > 0.0 {
        if look.direction != ErrorBarDirection::Upper {
            out.push(line(vec![[x - cap, lo], [x + cap, lo]]));
        }
        if look.direction != ErrorBarDirection::Lower {
            out.push(line(vec![[x - cap, hi], [x + cap, hi]]));
        }
    }
    out
}

impl Element for SummaryElement {
    fn kind(&self) -> ElementKind {
        ElementKind::Summary
    }

    fn layer(&self, panel: &Panel, _state: &RowStateManager) -> Layer {
        let mut layer = Layer::default();
        if panel.y.is_categorical() {
            layer.notes.push("Summary points need numbers up.".to_owned());
            return layer;
        }
        let bars = if self.stat == SummaryStat::Mean { self.error_bars } else { ErrorBars::None };
        if self.error_bars != ErrorBars::None && bars == ErrorBars::None {
            layer.notes.push("Error bars are drawn around means only.".to_owned());
        }
        // Caps: a fraction of the slot on a category axis, else of the x spacing.
        let slots = included_by_slot(panel);
        let cap = if panel.x.is_categorical() {
            panel.slot_half_width * 0.35
        } else {
            let xs: Vec<f64> = slots.iter().map(|s| s.0.0).collect();
            let gap = xs.windows(2).map(|w| w[1] - w[0]).filter(|d| *d > 0.0).fold(f64::INFINITY, f64::min);
            let span = xs.last().copied().unwrap_or(0.0) - xs.first().copied().unwrap_or(0.0);
            if gap.is_finite() { (gap * 0.25).min(span * 0.02).max(span * 0.005) } else { 0.05 }
        };
        let mut by_group: std::collections::BTreeMap<usize, Vec<[f64; 2]>> = std::collections::BTreeMap::new();
        for ((x, g), pts) in slots {
            let obs: Vec<Obs> = data_obs(panel, &pts);
            let sy = panel.y.scale;
            let Some(v) = summarize(&obs, self.stat).and_then(|v| sy.forward(v)) else { continue };
            by_group.entry(g).or_default().push([x, v]);
            let interval =
                error_interval(&obs, bars, self.level).and_then(|(lo, hi)| Some((sy.forward(lo)?, sy.forward(hi)?)));
            if let Some((lo, hi)) = interval {
                let color = group_color(panel, g, palette::FIT);
                layer.marks.extend(error_bar_marks(
                    x,
                    [v, lo, hi],
                    cap,
                    color,
                    &self.error_look,
                    &format!("{}error bar", group_prefix(panel, g)),
                ));
            }
        }
        for (g, points) in by_group {
            layer.marks.push(Mark::Points {
                points,
                color: group_color(panel, g, palette::FIT),
                shape: MarkShape::Diamond,
                radius: 6.0,
                filled: true,
                name: format!("{}{}", group_prefix(panel, g), self.stat.label().to_lowercase()),
            });
        }
        if !layer.marks.is_empty() {
            let of = if panel.y.name.is_empty() { String::new() } else { format!(" of {}", panel.y.name) };
            layer.key_entry(
                format!("{}{of}", self.stat.label()),
                palette::FIT,
                KeyGlyph::Marker(MarkShape::Diamond, true),
            );
            if bars != ErrorBars::None {
                let what = match bars {
                    ErrorBars::Confidence => format!("{} confidence interval for the mean", super::percent(self.level)),
                    other => format!("Mean {}", other.label()),
                };
                layer.key_entry(what, self.error_look.color.unwrap_or(palette::FIT), KeyGlyph::Line(LinePattern::Solid));
            }
        }
        layer
    }
}
