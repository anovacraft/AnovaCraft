//! Least-squares polynomial fits.

use stat_data::RowStateManager;

use super::{Element, ElementKind, both_numeric, group_prefix, included_by_group, key_color, marker_color, percent};
use crate::frame::Panel;
use crate::layer::{KeyGlyph, Layer, LinePattern, Mark, TextAnchor};
use crate::stats::{PolyFit, poly_fit_weighted};
use crate::view::format_value;

/// Which fit statistics are written at the end of each fitted line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FitLabels {
    /// The fitted equation.
    pub equation: bool,
    /// R².
    pub r_squared: bool,
    /// p-value of the overall F test.
    pub p_value: bool,
}

impl Default for FitLabels {
    fn default() -> Self {
        Self { equation: false, r_squared: true, p_value: false }
    }
}

/// Weighted least-squares polynomial (degree 1 to 5) per subgroup, with a
/// shaded pointwise confidence band for the mean, dashed prediction limits
/// for a new observation, and fit statistics labelled at the line's right
/// end. Fits are computed in drawn units (on transformed scales, of the
/// transformed values), so equations read e.g. `log₁₀(y) = …`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FitLineElement {
    /// Polynomial degree.
    pub degree: usize,
    /// Shade the confidence band for the mean.
    pub band: bool,
    /// Draw prediction limits for a new observation.
    pub prediction: bool,
    /// Confidence level of the bands.
    pub level: f64,
    /// Statistics written on the graph.
    pub labels: FitLabels,
}

impl Default for FitLineElement {
    fn default() -> Self {
        Self { degree: 1, band: true, prediction: false, level: 0.95, labels: FitLabels::default() }
    }
}

/// A p-value for a label: `p = 0.0123`, or `p < 0.0001`.
fn p_text(p: f64) -> String {
    if p < 1e-4 { "p < 0.0001".to_owned() } else { format!("p = {}", format_value(p)) }
}

impl FitLineElement {
    /// The fit for each subgroup of `panel` (`None` where it cannot be fitted).
    #[must_use]
    pub fn fits(&self, panel: &Panel) -> Vec<Option<PolyFit>> {
        included_by_group(panel)
            .iter()
            .map(|pts| {
                let x: Vec<f64> = pts.iter().map(|p| p.x).collect();
                let y: Vec<f64> = pts.iter().map(|p| p.y).collect();
                let f: Vec<f64> = pts.iter().map(|p| p.freq).collect();
                let w: Vec<f64> = pts.iter().map(|p| p.weight).collect();
                poly_fit_weighted(&x, &y, &f, &w, self.degree, self.level)
            })
            .collect()
    }

    /// The label written at a fit's end (empty when nothing is chosen).
    #[must_use]
    pub fn label(&self, panel: &Panel, g: usize, fit: &PolyFit) -> String {
        let mut stats = Vec::new();
        if self.labels.r_squared {
            stats.push(format!("R² = {}", fit.r_squared.map_or_else(|| "—".to_owned(), |r| format!("{r:.3}"))));
        }
        if self.labels.p_value {
            stats.push(fit.p_value.map_or_else(|| "p = —".to_owned(), p_text));
        }
        let mut lines = Vec::new();
        if self.labels.equation {
            let (x, y) = (panel.x.scale.wrap_name(&panel.x.name), panel.y.scale.wrap_name(&panel.y.name));
            let y = if y.is_empty() { "y".to_owned() } else { y };
            let x = if x.is_empty() { "x".to_owned() } else { x };
            lines.push(fit.equation(&y, &x));
        }
        if !stats.is_empty() {
            lines.push(stats.join(", "));
        }
        if lines.is_empty() {
            return String::new();
        }
        format!("{}{}", group_prefix(panel, g), lines.join("\n"))
    }
}

impl Element for FitLineElement {
    fn kind(&self) -> ElementKind {
        ElementKind::FitLine
    }

    fn layer(&self, panel: &Panel, _state: &RowStateManager) -> Layer {
        let mut layer = Layer::default();
        if !both_numeric(panel) {
            layer.notes.push("Fit lines need numbers on both axes.".to_owned());
            return layer;
        }
        let mut missing = 0;
        let (mut any_band, mut any_prediction) = (false, false);
        let mut degree_used = self.degree;
        for (g, fit) in self.fits(panel).into_iter().enumerate() {
            let Some(fit) = fit else {
                missing += 1;
                continue;
            };
            degree_used = degree_used.min(fit.degree);
            // Lines, bands and labels in the subgroup's marker color.
            let color = marker_color(panel, g);
            if self.band {
                if let Some((lower, upper)) = fit.band.clone() {
                    any_band = true;
                    layer.marks.push(Mark::Band {
                        lower,
                        upper,
                        color: color.with_alpha(0.18),
                        name: format!("{}{} confidence band", group_prefix(panel, g), percent(self.level)),
                    });
                }
            }
            if self.prediction {
                if let Some((lower, upper)) = fit.prediction_band.clone() {
                    any_prediction = true;
                    for curve in [lower, upper] {
                        layer.marks.push(Mark::Line {
                            points: curve,
                            color,
                            width: 1.2,
                            pattern: LinePattern::Dashed,
                            name: format!("{}{} prediction limits", group_prefix(panel, g), percent(self.level)),
                        });
                    }
                }
            }
            let end = *fit.curve.last().expect("a fit has a curve");
            let text = self.label(panel, g, &fit);
            layer.marks.push(Mark::Line {
                points: fit.curve.clone(),
                color,
                width: 2.0,
                pattern: LinePattern::Solid,
                name: format!("{}fit (degree {})", group_prefix(panel, g), fit.degree),
            });
            if !text.is_empty() {
                layer.marks.push(Mark::Text {
                    at: end,
                    text,
                    color,
                    anchor: TextAnchor::RightOfText,
                    movable: Some(format!("fit/{g}")),
                });
            }
        }
        if missing > 0 {
            layer.notes.push(format!("{missing} subgroup(s) have too few distinct x values for a fit."));
        }
        if layer.marks.is_empty() {
            return layer;
        }
        let what = match degree_used {
            1 => "Least-squares line".to_owned(),
            2 => "Quadratic fit".to_owned(),
            3 => "Cubic fit".to_owned(),
            d => format!("Polynomial fit, degree {d}"),
        };
        let ink = key_color(panel);
        layer.key_entry(what, ink, KeyGlyph::Line(LinePattern::Solid));
        if any_band {
            layer.key_entry(
                format!("{} confidence band for the mean", percent(self.level)),
                ink,
                KeyGlyph::Area(ink.with_alpha(0.25)),
            );
        }
        if any_prediction {
            layer.key_entry(
                format!("{} prediction limits for a new row", percent(self.level)),
                ink,
                KeyGlyph::Line(LinePattern::Dashed),
            );
        }
        layer
    }
}
