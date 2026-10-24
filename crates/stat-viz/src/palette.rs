//! Color assignment. Categorical levels take colors from the color-blind-safe
//! set of Okabe & Ito ("Color Universal Design", 2008), each paired with a
//! marker shape so groups never rely on color alone. Continuous values map
//! to a sequential ramp (dark blue → teal → yellow) whose lightness rises
//! steadily, so order reads in grayscale too.

use crate::layer::{MarkShape, Rgba};

/// The categorical palette (Okabe & Ito, without black).
pub const CATEGORICAL: [Rgba; 7] = [
    Rgba::rgb(0, 114, 178),   // blue
    Rgba::rgb(230, 159, 0),   // orange
    Rgba::rgb(0, 158, 115),   // bluish green
    Rgba::rgb(204, 121, 167), // reddish purple
    Rgba::rgb(86, 180, 233),  // sky blue
    Rgba::rgb(213, 94, 0),    // vermillion
    Rgba::rgb(240, 228, 66),  // yellow
];

/// Bars when nothing is mapped to color.
pub const BAR: Rgba = Rgba::rgb(86, 130, 180);
/// Text and error bars on bars.
pub const BAR_TEXT: Rgba = Rgba::rgb(30, 41, 59);
/// Point labels.
pub const LABEL: Rgba = Rgba::rgb(30, 41, 59);
/// Lines when nothing is mapped to color.
pub const LINE: Rgba = Rgba::rgb(70, 90, 130);

/// The color of points when nothing is mapped to color.
pub const DEFAULT_POINT: Rgba = Rgba::rgb(2, 132, 199);
/// Excluded rows.
pub const EXCLUDED: Rgba = Rgba::rgb(148, 163, 184);
/// Selection rings.
pub const SELECTED: Rgba = Rgba::rgb(234, 88, 12);
/// Fit lines when nothing else colors them.
pub const FIT: Rgba = Rgba::rgb(225, 29, 72);
/// Smoother curves when nothing else colors them.
pub const SMOOTH: Rgba = Rgba::rgb(46, 140, 96);
/// Box outlines when nothing else colors them.
pub const BOX: Rgba = Rgba::rgb(90, 100, 120);
/// Increases in a waterfall chart (the palette's bluish green).
pub const INCREASE: Rgba = CATEGORICAL[2];
/// Decreases in a waterfall chart (the palette's vermillion; also hatched).
pub const DECREASE: Rgba = CATEGORICAL[5];

/// Reference lines added by the user, unless given a color of their own.
pub const REFERENCE: Rgba = Rgba::rgb(109, 40, 217);
/// Reference bands added by the user, unless given a color of their own
/// (drawn translucent).
pub const REFERENCE_BAND: Rgba = Rgba::rgb(100, 116, 139);

/// The colors above as drawn on a dark canvas: each fixed color of the light
/// appearance maps to a lighter counterpart with the same alpha, as does the
/// first categorical color (a blue too dark for the dark canvas); any other
/// color (the other categorical levels, the ramp, user choices) is kept.
#[must_use]
pub fn for_dark(c: Rgba) -> Rgba {
    const MAP: [(Rgba, Rgba); 12] = [
        (CATEGORICAL[0], Rgba::rgb(64, 140, 230)),
        (DEFAULT_POINT, Rgba::rgb(56, 189, 248)),
        (FIT, Rgba::rgb(255, 69, 102)),
        (LABEL, Rgba::rgb(229, 231, 235)),
        (EXCLUDED, Rgba::rgb(107, 114, 128)),
        (SELECTED, Rgba::rgb(251, 146, 60)),
        (LINE, Rgba::rgb(156, 163, 175)),
        (BOX, Rgba::rgb(156, 163, 175)),
        (BAR, Rgba::rgb(56, 189, 248)),
        (SMOOTH, Rgba::rgb(52, 211, 153)),
        (REFERENCE, Rgba::rgb(167, 139, 250)),
        (REFERENCE_BAND, Rgba::rgb(148, 163, 184)),
    ];
    let [r, g, b, a] = c.0;
    MAP.iter()
        .find(|(light, _)| light.0[..3] == [r, g, b])
        .map_or(c, |(_, dark)| Rgba([dark.0[0], dark.0[1], dark.0[2], a]))
}

/// Color and shape of categorical level `i`.
#[must_use]
pub fn level_style(i: usize) -> (Rgba, MarkShape) {
    (CATEGORICAL[i % CATEGORICAL.len()], MarkShape::CYCLE[i % MarkShape::CYCLE.len()])
}

/// The sequential ramp at `t ∈ [0, 1]`.
#[must_use]
pub fn ramp(t: f64) -> Rgba {
    const STOPS: [[f64; 3]; 5] =
        [[46.0, 52.0, 128.0], [38.0, 110.0, 170.0], [36.0, 158.0, 146.0], [124.0, 196.0, 92.0], [244.0, 222.0, 92.0]];
    let t = if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.0 } * (STOPS.len() - 1) as f64;
    let k = (t.floor() as usize).min(STOPS.len() - 2);
    let f = t - k as f64;
    let c = |i: usize| (STOPS[k][i] + f * (STOPS[k + 1][i] - STOPS[k][i])).round() as u8;
    Rgba::rgb(c(0), c(1), c(2))
}

/// Ends and middle of the diverging ramp on a light canvas: blue for −1, a
/// light neutral for 0, vermillion for +1 (Okabe & Ito hues, darkened).
pub const DIVERGING_LIGHT: [Rgba; 3] = [Rgba::rgb(0, 92, 158), Rgba::rgb(246, 246, 244), Rgba::rgb(186, 72, 0)];
/// The diverging ramp on a dark canvas: the same hues around a dark neutral,
/// so magnitude still reads as distance from the background.
pub const DIVERGING_DARK: [Rgba; 3] = [Rgba::rgb(86, 170, 230), Rgba::rgb(40, 44, 52), Rgba::rgb(240, 130, 60)];

/// The diverging ramp at `t ∈ [−1, 1]` (signed values such as
/// correlations) between the `ends` (negative, neutral, positive): the
/// colour moves from the neutral towards an end as |t| grows, so the sign
/// reads from the hue and the size from the contrast with the neutral.
/// Values outside the range are clamped; NaN gives the neutral.
#[must_use]
pub fn diverging(t: f64, ends: [Rgba; 3]) -> Rgba {
    let t = if t.is_finite() { t.clamp(-1.0, 1.0) } else { 0.0 };
    let (from, to) = (ends[1], if t < 0.0 { ends[0] } else { ends[2] });
    let f = t.abs();
    let c = |i: usize| (f64::from(from.0[i]) + f * (f64::from(to.0[i]) - f64::from(from.0[i]))).round() as u8;
    Rgba::rgb(c(0), c(1), c(2))
}

/// A colour scale for continuous values on a filled map (contour plots and
/// their colour bars). Every scale has a light and a dark variant; the dark
/// one is the light one mixed 18% towards the dark canvas, so it glares less
/// without changing the order of the colours.
///
/// Provenance of the stops:
/// * **Sequential**: this palette's own ramp ([`ramp`]), dark blue → teal →
///   yellow with steadily rising lightness, so order reads in grayscale too.
/// * **Heat**: pale yellow → orange → dark red, the single-hue-family
///   progression of Brewer's yellow–orange–red scheme (Harrower & Brewer,
///   "ColorBrewer.org", *The Cartographic Journal* 40(1), 2003; Apache
///   licence), re-sampled at five stops.
/// * **Spectrum**: blue → cyan → green → yellow → red, the hypsometric
///   ("rainbow") order long used for elevation and response maps. Its
///   lightness does not rise steadily and red–green readers confuse its
///   middle, so plots using it keep their level lines and labels.
/// * **Diverging**: blue → light neutral → vermillion ([`DIVERGING_LIGHT`],
///   Okabe & Ito hues), for values on either side of a centre such as a
///   target.
/// * **Grey**: white → near black, for printing in one ink.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorScale {
    /// Dark blue → teal → yellow.
    #[default]
    Sequential,
    /// Pale yellow → orange → dark red.
    Heat,
    /// Blue → green → yellow → red (not colour-blind safe).
    Spectrum,
    /// Blue → light neutral → vermillion around a centre.
    Diverging,
    /// White → near black.
    Grey,
}

impl ColorScale {
    /// Every scale, in menu order.
    pub const ALL: [Self; 5] = [Self::Sequential, Self::Heat, Self::Spectrum, Self::Diverging, Self::Grey];

    /// Name shown in menus.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Sequential => "Sequential (blue to yellow)",
            Self::Heat => "Heat (yellow to red)",
            Self::Spectrum => "Spectrum (blue to red)",
            Self::Diverging => "Diverging (blue, neutral, red)",
            Self::Grey => "Grey (for print)",
        }
    }

    /// Key used in project files and scripts.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Sequential => "sequential",
            Self::Heat => "heat",
            Self::Spectrum => "spectrum",
            Self::Diverging => "diverging",
            Self::Grey => "grey",
        }
    }

    /// The scale named by [`ColorScale::key`] (`"gray"` also reads as Grey).
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "gray" => Some(Self::Grey),
            _ => Self::ALL.into_iter().find(|s| s.key() == key),
        }
    }

    fn stops(self) -> &'static [[f64; 3]] {
        const SEQUENTIAL: [[f64; 3]; 5] = [
            [46.0, 52.0, 128.0],
            [38.0, 110.0, 170.0],
            [36.0, 158.0, 146.0],
            [124.0, 196.0, 92.0],
            [244.0, 222.0, 92.0],
        ];
        const HEAT: [[f64; 3]; 5] = [
            [255.0, 245.0, 190.0],
            [254.0, 204.0, 92.0],
            [247.0, 140.0, 46.0],
            [214.0, 62.0, 30.0],
            [140.0, 12.0, 24.0],
        ];
        const SPECTRUM: [[f64; 3]; 6] = [
            [40.0, 60.0, 200.0],
            [40.0, 160.0, 230.0],
            [70.0, 200.0, 110.0],
            [240.0, 225.0, 60.0],
            [245.0, 140.0, 40.0],
            [205.0, 35.0, 35.0],
        ];
        const GREY: [[f64; 3]; 2] = [[250.0, 250.0, 250.0], [40.0, 40.0, 40.0]];
        match self {
            Self::Sequential => &SEQUENTIAL,
            Self::Heat => &HEAT,
            Self::Spectrum => &SPECTRUM,
            Self::Grey => &GREY,
            Self::Diverging => &[],
        }
    }

    /// The scale's colour at `t ∈ [0, 1]` (clamped; NaN gives the start),
    /// in the dark variant when `dark`.
    #[must_use]
    pub fn color(self, t: f64, dark: bool) -> Rgba {
        let t = if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.0 };
        let light = if self == Self::Diverging {
            diverging(2.0 * t - 1.0, DIVERGING_LIGHT)
        } else {
            let stops = self.stops();
            let u = t * (stops.len() - 1) as f64;
            let k = (u.floor() as usize).min(stops.len() - 2);
            let f = u - k as f64;
            let c = |i: usize| (stops[k][i] + f * (stops[k + 1][i] - stops[k][i])).round() as u8;
            Rgba::rgb(c(0), c(1), c(2))
        };
        if dark { toward_dark_canvas(light) } else { light }
    }
}

/// The dark canvas the dark variants of [`ColorScale`] lean towards.
const DARK_CANVAS: [f64; 3] = [24.0, 26.0, 32.0];

fn toward_dark_canvas(c: Rgba) -> Rgba {
    let mix = |i: usize| (0.82 * f64::from(c.0[i]) + 0.18 * DARK_CANVAS[i]).round() as u8;
    Rgba([mix(0), mix(1), mix(2), c.0[3]])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diverging_ramp_darkens_away_from_zero() {
        let lum = |c: Rgba| 0.2126 * f64::from(c.0[0]) + 0.7152 * f64::from(c.0[1]) + 0.0722 * f64::from(c.0[2]);
        assert_eq!(diverging(0.0, DIVERGING_LIGHT), DIVERGING_LIGHT[1]);
        assert_eq!(diverging(-1.0, DIVERGING_LIGHT), DIVERGING_LIGHT[0]);
        assert_eq!(diverging(2.0, DIVERGING_LIGHT), DIVERGING_LIGHT[2]);
        assert_eq!(diverging(f64::NAN, DIVERGING_DARK), DIVERGING_DARK[1]);
        for sign in [-1.0, 1.0] {
            let steps: Vec<f64> =
                (0..=10).map(|i| lum(diverging(sign * f64::from(i) / 10.0, DIVERGING_LIGHT))).collect();
            assert!(steps.windows(2).all(|w| w[1] < w[0]), "contrast grows with |t|: {steps:?}");
        }
    }

    #[test]
    fn styles_and_ramp() {
        assert_eq!(level_style(0).0, CATEGORICAL[0]);
        assert_ne!(level_style(1).1, level_style(0).1, "shapes differ too");
        assert_eq!(level_style(7).0, level_style(0).0);
        assert_eq!(ramp(0.0), Rgba::rgb(46, 52, 128));
        assert_eq!(ramp(1.0), Rgba::rgb(244, 222, 92));
        assert_eq!(ramp(f64::NAN), ramp(0.0));
        let lum = |c: Rgba| 0.2126 * f64::from(c.0[0]) + 0.7152 * f64::from(c.0[1]) + 0.0722 * f64::from(c.0[2]);
        let steps: Vec<f64> = (0..=10).map(|i| lum(ramp(f64::from(i) / 10.0))).collect();
        assert!(steps.windows(2).all(|w| w[1] > w[0]), "lightness rises: {steps:?}");
        assert_eq!(Rgba::rgb(1, 2, 3).with_alpha(0.5).0[3], 128);
        // Dark counterparts keep alpha; other colors are untouched.
        assert_eq!(for_dark(DEFAULT_POINT), Rgba::rgb(56, 189, 248));
        assert_eq!(for_dark(FIT.with_alpha(0.5)).0, [255, 69, 102, 128]);
        assert_eq!(for_dark(CATEGORICAL[1]), CATEGORICAL[1]);
        assert_eq!(for_dark(CATEGORICAL[0]), Rgba::rgb(64, 140, 230));
    }

    #[test]
    fn colour_scales_at_ends_and_middle() {
        let lum = |c: Rgba| 0.2126 * f64::from(c.0[0]) + 0.7152 * f64::from(c.0[1]) + 0.0722 * f64::from(c.0[2]);
        // Light ends and middles.
        assert_eq!(ColorScale::Sequential.color(0.0, false), ramp(0.0));
        assert_eq!(ColorScale::Sequential.color(0.5, false), ramp(0.5));
        assert_eq!(ColorScale::Sequential.color(1.0, false), ramp(1.0));
        assert_eq!(ColorScale::Heat.color(0.0, false), Rgba::rgb(255, 245, 190));
        assert_eq!(ColorScale::Heat.color(0.5, false), Rgba::rgb(247, 140, 46));
        assert_eq!(ColorScale::Heat.color(1.0, false), Rgba::rgb(140, 12, 24));
        assert_eq!(ColorScale::Spectrum.color(0.0, false), Rgba::rgb(40, 60, 200));
        assert_eq!(ColorScale::Spectrum.color(0.5, false), Rgba::rgb(155, 213, 85));
        assert_eq!(ColorScale::Spectrum.color(1.0, false), Rgba::rgb(205, 35, 35));
        assert_eq!(ColorScale::Diverging.color(0.0, false), DIVERGING_LIGHT[0]);
        assert_eq!(ColorScale::Diverging.color(0.5, false), DIVERGING_LIGHT[1]);
        assert_eq!(ColorScale::Diverging.color(1.0, false), DIVERGING_LIGHT[2]);
        assert_eq!(ColorScale::Grey.color(0.0, false), Rgba::rgb(250, 250, 250));
        assert_eq!(ColorScale::Grey.color(0.5, false), Rgba::rgb(145, 145, 145));
        assert_eq!(ColorScale::Grey.color(1.0, false), Rgba::rgb(40, 40, 40));
        // Dark variants lean towards the canvas, keeping the order.
        assert_eq!(ColorScale::Heat.color(0.0, true), Rgba::rgb(213, 206, 162));
        assert_eq!(ColorScale::Grey.color(0.5, true), Rgba::rgb(123, 124, 125));
        assert_eq!(ColorScale::Sequential.color(1.0, true), Rgba::rgb(204, 187, 81));
        for scale in ColorScale::ALL {
            for dark in [false, true] {
                assert!(lum(scale.color(0.0, dark)) != lum(scale.color(1.0, dark)), "{scale:?}");
                assert_eq!(scale.color(f64::NAN, dark), scale.color(0.0, dark));
                assert_eq!(scale.color(2.0, dark), scale.color(1.0, dark));
            }
            assert_eq!(ColorScale::from_key(scale.key()), Some(scale));
        }
        // Heat and Grey darken steadily, Sequential lightens steadily.
        for (scale, rising) in [(ColorScale::Heat, false), (ColorScale::Grey, false), (ColorScale::Sequential, true)] {
            let steps: Vec<f64> = (0..=10).map(|i| lum(scale.color(f64::from(i) / 10.0, false))).collect();
            assert!(steps.windows(2).all(|w| (w[1] > w[0]) == rising), "{scale:?}: {steps:?}");
        }
    }
}
