//! Curves `y = f(x)` typed as formulas, drawn over the data.

use stat_data::RowStateManager;

use super::{Element, ElementKind};
use crate::expr::Expr;
use crate::frame::Panel;
use crate::layer::{KeyGlyph, Layer, LinePattern, Mark};

/// One dotted curve per formula in `x` (see [`crate::expr`] for the syntax),
/// in data units: x positions across the panel's data are taken back to data
/// values through the horizontal scale, the formula is evaluated, and the
/// result drawn through the vertical scale. Undefined stretches (e.g. `ln`
/// of a negative) break the curve.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FunctionElement {
    /// Formulas, one curve each (blank ones are skipped).
    pub formulas: Vec<String>,
}

/// Points along each curve.
const FUNCTION_POINTS: usize = 240;

/// Colors of successive formula curves (dark, distinct from the data palette).
const CURVE_COLORS: [crate::layer::Rgba; 3] =
    [crate::layer::Rgba::rgb(60, 60, 70), crate::layer::Rgba::rgb(150, 70, 30), crate::layer::Rgba::rgb(40, 100, 140)];

impl Element for FunctionElement {
    fn kind(&self) -> ElementKind {
        ElementKind::Function
    }

    fn layer(&self, panel: &Panel, _state: &RowStateManager) -> Layer {
        let mut layer = Layer::default();
        if panel.x.is_categorical() || panel.y.is_categorical() {
            layer.notes.push("Formula curves need numbers on both axes.".to_owned());
            return layer;
        }
        let (lo, hi) =
            panel.points.iter().map(|p| p.x).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), x| (a.min(x), b.max(x)));
        if !(lo.is_finite() && hi > lo) {
            return layer;
        }
        let (sx, sy) = (panel.x.scale, panel.y.scale);
        for (k, text) in self.formulas.iter().filter(|f| !f.trim().is_empty()).enumerate() {
            let expr = match Expr::parse(text) {
                Ok(e) => e,
                Err(e) => {
                    layer.notes.push(format!("Formula “{text}”: {e}."));
                    continue;
                }
            };
            let color = CURVE_COLORS[k % CURVE_COLORS.len()];
            // Split the curve where it is undefined.
            let mut piece: Vec<[f64; 2]> = Vec::new();
            let mut pieces: Vec<Vec<[f64; 2]>> = Vec::new();
            for i in 0..FUNCTION_POINTS {
                let t = lo + (hi - lo) * i as f64 / (FUNCTION_POINTS - 1) as f64;
                let y = expr.eval(sx.inverse(t));
                match sy.forward(y) {
                    Some(v) => piece.push([t, v]),
                    None => {
                        if piece.len() > 1 {
                            pieces.push(std::mem::take(&mut piece));
                        }
                        piece.clear();
                    }
                }
            }
            if piece.len() > 1 {
                pieces.push(piece);
            }
            if pieces.is_empty() {
                layer.notes.push(format!("Formula “{text}” has no values to draw over this range."));
                continue;
            }
            for points in pieces {
                layer.marks.push(Mark::Line {
                    points,
                    color,
                    width: 2.0,
                    pattern: LinePattern::Dotted,
                    name: format!("y = {text}"),
                });
            }
            layer.key_entry(format!("y = {}", text.trim()), color, KeyGlyph::Line(LinePattern::Dotted));
        }
        layer
    }
}
