//! Smoothing curves: the cubic smoothing spline and LOESS.

use stat_data::RowStateManager;

use super::{Element, ElementKind, both_numeric, group_color, group_prefix, included_by_group, percent};
use crate::frame::Panel;
use crate::layer::{KeyGlyph, Layer, LinePattern, Mark};
use crate::loess::loess_with_frequencies;
use crate::palette;
use crate::stats::{MAX_BAND_KNOTS, SmoothingSpline};

/// How a smoother is computed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum SmoothMethod {
    /// Penalized cubic smoothing spline (λ).
    #[default]
    Spline,
    /// Locally weighted regression (span).
    Loess,
}

impl SmoothMethod {
    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Spline => "Smoothing spline",
            Self::Loess => "LOESS",
        }
    }

    /// Short name for saving.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Spline => "spline",
            Self::Loess => "loess",
        }
    }

    /// The method with this [`SmoothMethod::key`].
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        [Self::Spline, Self::Loess].into_iter().find(|m| m.key() == key)
    }
}

/// A smooth curve per subgroup, dashed, with an optional shaded confidence
/// band: a cubic smoothing spline ([`SmoothingSpline`]; `lambda` is the
/// relative penalty, larger is smoother) or LOESS ([`loess`]; `span` is the
/// fraction of rows in each local fit and `local_degree` its polynomial
/// degree). A row's frequency counts it as that many observations (in the
/// fit, the span and the band's degrees of freedom); its weight weights it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SmootherElement {
    /// Spline or LOESS.
    pub method: SmoothMethod,
    /// Spline smoothing parameter λ (scaled by the cube of the x range).
    pub lambda: f64,
    /// LOESS span (fraction of rows in each local fit).
    pub span: f64,
    /// LOESS local polynomial degree (1 or 2).
    pub local_degree: usize,
    /// Shade a confidence band.
    pub band: bool,
    /// Confidence level of the band.
    pub level: f64,
}

impl Default for SmootherElement {
    fn default() -> Self {
        Self { method: SmoothMethod::Spline, lambda: 1e-3, span: 0.75, local_degree: 2, band: false, level: 0.95 }
    }
}

/// Points along each smoother.
const SMOOTH_POINTS: usize = 160;

impl Element for SmootherElement {
    fn kind(&self) -> ElementKind {
        ElementKind::Smoother
    }

    fn layer(&self, panel: &Panel, _state: &RowStateManager) -> Layer {
        let mut layer = Layer::default();
        if !both_numeric(panel) {
            layer.notes.push("Smoothers need numbers on both axes.".to_owned());
            return layer;
        }
        let mut band_missing = false;
        for (g, pts) in included_by_group(panel).iter().enumerate() {
            let x: Vec<f64> = pts.iter().map(|p| p.x).collect();
            let y: Vec<f64> = pts.iter().map(|p| p.y).collect();
            let f: Vec<f64> = pts.iter().map(|p| p.freq).collect();
            let w: Vec<f64> = pts.iter().map(|p| p.weight).collect();
            let fit = match self.method {
                SmoothMethod::Spline if self.band => SmoothingSpline::fit_with_band_and_frequencies(
                    &x,
                    &y,
                    &f,
                    &w,
                    self.lambda,
                    self.level,
                    SMOOTH_POINTS,
                ),
                SmoothMethod::Spline => SmoothingSpline::fit_with_frequencies(&x, &y, &f, &w, self.lambda)
                    .map(|s| crate::stats::SmoothFit { curve: s.curve(SMOOTH_POINTS), band: None, equivalent_df: 0.0 }),
                SmoothMethod::Loess => {
                    loess_with_frequencies(&x, &y, &f, &w, self.span, self.local_degree, self.level, SMOOTH_POINTS)
                }
            };
            let Some(fit) = fit else { continue };
            if fit.curve.len() < 2 {
                continue;
            }
            let color = group_color(panel, g, palette::SMOOTH);
            if self.band {
                match fit.band {
                    Some((lower, upper)) => layer.marks.push(Mark::Band {
                        lower,
                        upper,
                        color: color.with_alpha(0.16),
                        name: format!("{}smoother band", group_prefix(panel, g)),
                    }),
                    None => band_missing = true,
                }
            }
            layer.marks.push(Mark::Line {
                points: fit.curve,
                color,
                width: 2.0,
                pattern: LinePattern::Dashed,
                name: format!("{}{}", group_prefix(panel, g), self.method.label().to_lowercase()),
            });
        }
        if band_missing {
            layer.notes.push(format!(
                "A smoother band needs residual degrees of freedom (and at most {MAX_BAND_KNOTS} distinct x for \
                 a spline); some are left out."
            ));
        }
        if !layer.marks.is_empty() {
            layer.key_entry(self.method.label(), palette::SMOOTH, KeyGlyph::Line(LinePattern::Dashed));
            if self.band && layer.marks.iter().any(|m| matches!(m, Mark::Band { .. })) {
                layer.key_entry(
                    format!("{} confidence band for the smoother", percent(self.level)),
                    palette::SMOOTH,
                    KeyGlyph::Area(palette::SMOOTH.with_alpha(0.25)),
                );
            }
        }
        layer
    }
}
