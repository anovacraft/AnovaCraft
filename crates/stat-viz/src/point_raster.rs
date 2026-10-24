//! Point clouds too large to draw one marker per row, drawn as a picture.
//!
//! A plot of a few hundred thousand points costs tens of milliseconds a frame
//! to tessellate as circles, and a million points overflow the GPU's index
//! buffer. Above [`POINT_BUDGET`] visible points a plot is drawn instead as
//! a bitmap at the screen's resolution: the points of each layer are counted
//! into pixels (a two-dimensional histogram with one bin per pixel), each
//! count is spread over a small square so single points stay visible, and
//! the layers are composited in order with the Porter–Duff "over" operator
//! (Porter & Duff 1984, "Compositing digital images", *SIGGRAPH* 18(3)).
//! Dense layers map counts to opacity on a logarithmic scale, so crowded
//! regions read darker without hiding sparse ones; solid layers (selected
//! and excluded rows) take their colour's own opacity wherever they have a
//! point.
//!
//! Only drawing changes: selection works in data coordinates, and exported
//! vector files still carry every point.

use crate::bitmap::Bitmap;
use crate::layer::Rgba;

/// The most points a plot draws as individual markers; above it, the plot's
/// points are drawn as a picture ([`rasterize_points`]).
pub const POINT_BUDGET: usize = 20_000;

/// The lowest opacity of a pixel holding at least one point of a dense layer.
const MIN_DENSITY_ALPHA: f32 = 0.35;

/// One set of points drawn in one colour.
#[derive(Debug, Clone, Copy)]
pub struct RasterLayer<'a> {
    /// Positions in plot coordinates; non-finite ones are skipped.
    pub points: &'a [[f64; 2]],
    /// Straight (not premultiplied) colour.
    pub color: Rgba,
    /// `true`: opacity grows with the number of points in a pixel;
    /// `false`: every pixel with a point takes the colour's opacity.
    pub density: bool,
    /// Side of the square each point covers, in pixels (at least 1).
    pub splat: u32,
}

/// The visible region of a plot: horizontal and vertical ranges in plot
/// coordinates, mapped onto the bitmap with the top row at `y[1]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RasterView {
    /// Left and right edges.
    pub x: [f64; 2],
    /// Bottom and top edges.
    pub y: [f64; 2],
}

/// The pixel holding `p` in a `width` × `height` picture of `view`, or `None`
/// outside it.
fn pixel_of(p: [f64; 2], view: &RasterView, width: usize, height: usize) -> Option<(usize, usize)> {
    let fx = (p[0] - view.x[0]) / (view.x[1] - view.x[0]);
    let fy = (view.y[1] - p[1]) / (view.y[1] - view.y[0]);
    if !(fx.is_finite() && fy.is_finite()) || !(0.0..=1.0).contains(&fx) || !(0.0..=1.0).contains(&fy) {
        return None;
    }
    // The far edge belongs to the last pixel.
    let col = ((fx * width as f64) as usize).min(width - 1);
    let row = ((fy * height as f64) as usize).min(height - 1);
    Some((col, row))
}

/// Sums of `counts` over the `splat` × `splat` square around each pixel
/// (separable running sums, so the cost does not grow with the square).
fn spread(counts: &[u32], width: usize, height: usize, splat: usize) -> Vec<u32> {
    if splat <= 1 {
        return counts.to_vec();
    }
    // A point covers (splat − 1) / 2 pixels before it and splat / 2 after,
    // so a pixel sums the points from splat / 2 before it to (splat − 1) / 2
    // after.
    let (before, after) = (splat / 2, (splat - 1) / 2);
    let window = |line: &[u32], out: &mut [u32]| {
        let n = line.len();
        let mut sum: u64 = line.iter().take(after.min(n - 1) + 1).map(|&c| u64::from(c)).sum();
        for i in 0..n {
            out[i] = u32::try_from(sum).unwrap_or(u32::MAX);
            if i + after + 1 < n {
                sum += u64::from(line[i + after + 1]);
            }
            if i >= before {
                sum -= u64::from(line[i - before]);
            }
        }
    };
    let mut across = vec![0_u32; counts.len()];
    for r in 0..height {
        let row = r * width..(r + 1) * width;
        window(&counts[row.clone()], &mut across[row]);
    }
    let mut out = vec![0_u32; counts.len()];
    let (mut column, mut summed) = (vec![0_u32; height], vec![0_u32; height]);
    for c in 0..width {
        for r in 0..height {
            column[r] = across[r * width + c];
        }
        window(&column, &mut summed);
        for r in 0..height {
            out[r * width + c] = summed[r];
        }
    }
    out
}

/// Draw `layers` (first at the bottom) as a `width` × `height` picture of
/// `view`, transparent where no layer has a point.
///
/// Each layer's points are counted per pixel, spread over its splat square,
/// and composited over the layers below: a dense layer's pixel opacity is
/// `α · (m + (1 − m) · ln(1 + c) / ln(1 + c_max))` for a pixel count `c > 0`
/// (`α` the colour's opacity, `m` = 0.35, `c_max` the layer's largest
/// count); a solid layer's is `α`.
///
/// Returns an empty bitmap when either size is zero or the view has no
/// extent.
#[must_use]
pub fn rasterize_points(layers: &[RasterLayer<'_>], view: RasterView, width: u32, height: u32) -> Bitmap {
    let (w, h) = (width as usize, height as usize);
    let extent_ok = |r: [f64; 2]| r[0].is_finite() && r[1].is_finite() && r[1] > r[0];
    if w == 0 || h == 0 || !extent_ok(view.x) || !extent_ok(view.y) {
        return Bitmap::default();
    }
    // Premultiplied red, green, blue and alpha, from 0 to 1.
    let mut acc = vec![[0.0_f32; 4]; w * h];
    let mut counts = vec![0_u32; w * h];
    for layer in layers {
        counts.iter_mut().for_each(|c| *c = 0);
        let mut any = false;
        for &p in layer.points {
            if let Some((col, row)) = pixel_of(p, &view, w, h) {
                counts[row * w + col] = counts[row * w + col].saturating_add(1);
                any = true;
            }
        }
        if !any {
            continue;
        }
        let covered = spread(&counts, w, h, layer.splat.max(1) as usize);
        let [r, g, b, a] = layer.color.0.map(|v| f32::from(v) / 255.0);
        let log_max = covered.iter().copied().max().map_or(1.0, |m| (1.0 + m as f32).ln()).max(f32::MIN_POSITIVE);
        for (px, &c) in acc.iter_mut().zip(&covered) {
            if c == 0 {
                continue;
            }
            let alpha = if layer.density {
                a * (MIN_DENSITY_ALPHA + (1.0 - MIN_DENSITY_ALPHA) * ((1.0 + c as f32).ln() / log_max).min(1.0))
            } else {
                a
            };
            let keep = 1.0 - alpha;
            *px = [r * alpha + px[0] * keep, g * alpha + px[1] * keep, b * alpha + px[2] * keep, alpha + px[3] * keep];
        }
    }
    let mut rgba = Vec::with_capacity(4 * w * h);
    for [r, g, b, a] in acc {
        if a <= 0.0 {
            rgba.extend_from_slice(&[0, 0, 0, 0]);
        } else {
            let byte = |v: f32| (v * 255.0).round().clamp(0.0, 255.0) as u8;
            rgba.extend_from_slice(&[byte(r / a), byte(g / a), byte(b / a), byte(a)]);
        }
    }
    Bitmap { width, height, rgba }
}

/// A cheap fingerprint of `points`: their number and a sample of at most
/// 4096 of them, for telling when a cached picture must be redrawn (hashing
/// every point each frame would cost more than the drawing it saves).
#[must_use]
pub fn sample_key(points: &[[f64; 2]]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    points.len().hash(&mut h);
    let stride = (points.len() / 4096).max(1);
    for p in points.iter().step_by(stride) {
        (p[0].to_bits(), p[1].to_bits()).hash(&mut h);
    }
    points.last().map(|p| (p[0].to_bits(), p[1].to_bits())).hash(&mut h);
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIEW: RasterView = RasterView { x: [0.0, 10.0], y: [0.0, 10.0] };

    fn pixel(b: &Bitmap, col: usize, row: usize) -> [u8; 4] {
        let i = 4 * (row * b.width as usize + col);
        [b.rgba[i], b.rgba[i + 1], b.rgba[i + 2], b.rgba[i + 3]]
    }

    #[test]
    fn points_land_in_their_pixels_with_the_top_row_highest() {
        let pts = [[0.5, 9.5], [9.5, 0.5]];
        let layer = RasterLayer { points: &pts, color: Rgba([255, 0, 0, 255]), density: false, splat: 1 };
        let b = rasterize_points(&[layer], VIEW, 10, 10);
        assert!(b.is_valid());
        assert_eq!(pixel(&b, 0, 0), [255, 0, 0, 255]);
        assert_eq!(pixel(&b, 9, 9), [255, 0, 0, 255]);
        assert_eq!(pixel(&b, 9, 0)[3], 0);
        assert_eq!(b.rgba.chunks_exact(4).filter(|p| p[3] > 0).count(), 2);
    }

    #[test]
    fn points_outside_the_view_or_not_finite_are_skipped() {
        let pts = [[-1.0, 5.0], [5.0, 11.0], [f64::NAN, 5.0], [5.0, f64::INFINITY]];
        let layer = RasterLayer { points: &pts, color: Rgba([0, 0, 0, 255]), density: true, splat: 3 };
        let b = rasterize_points(&[layer], VIEW, 10, 10);
        assert!(b.rgba.chunks_exact(4).all(|p| p[3] == 0));
    }

    #[test]
    fn the_splat_square_is_centred_on_the_point() {
        let pts = [[5.5, 4.5]]; // column 5, row 5
        let layer = RasterLayer { points: &pts, color: Rgba([0, 0, 255, 255]), density: false, splat: 3 };
        let b = rasterize_points(&[layer], VIEW, 10, 10);
        let lit: Vec<(usize, usize)> =
            (0..10).flat_map(|r| (0..10).map(move |c| (c, r))).filter(|&(c, r)| pixel(&b, c, r)[3] > 0).collect();
        let expected: Vec<(usize, usize)> = (4..=6).flat_map(|r| (4..=6).map(move |c| (c, r))).collect();
        let mut lit_sorted = lit.clone();
        lit_sorted.sort_by_key(|&(c, r)| (r, c));
        assert_eq!(lit_sorted, expected);
        // An even square reaches one pixel further right and down.
        let even = RasterLayer { splat: 2, ..layer };
        let b = rasterize_points(&[even], VIEW, 10, 10);
        for (c, r) in [(5, 5), (6, 5), (5, 6), (6, 6)] {
            assert_eq!(pixel(&b, c, r)[3], 255, "({c}, {r})");
        }
        assert_eq!(b.rgba.chunks_exact(4).filter(|p| p[3] > 0).count(), 4);
    }

    #[test]
    fn spread_matches_a_direct_window_sum() {
        let (w, h) = (7, 5);
        let counts: Vec<u32> = (0..w * h).map(|i| ((i * 7919) % 5) as u32).collect();
        for splat in 1..=5 {
            let fast = spread(&counts, w, h, splat);
            let (before, after) = (splat / 2, (splat - 1) / 2);
            for r in 0..h {
                for c in 0..w {
                    let mut sum = 0;
                    for rr in r.saturating_sub(before)..=(r + after).min(h - 1) {
                        for cc in c.saturating_sub(before)..=(c + after).min(w - 1) {
                            sum += counts[rr * w + cc];
                        }
                    }
                    assert_eq!(fast[r * w + c], sum, "splat {splat} at ({c}, {r})");
                }
            }
        }
    }

    #[test]
    fn denser_pixels_of_a_density_layer_are_more_opaque() {
        let mut pts = vec![[1.5, 1.5]];
        pts.extend(std::iter::repeat_n([7.5, 7.5], 99));
        let layer = RasterLayer { points: &pts, color: Rgba([0, 128, 0, 255]), density: true, splat: 1 };
        let b = rasterize_points(&[layer], VIEW, 10, 10);
        let (sparse, dense) = (pixel(&b, 1, 8), pixel(&b, 7, 2));
        assert_eq!(dense, [0, 128, 0, 255]);
        // ln 2 / ln 100 of the way from the floor.
        let expected = 255.0 * (MIN_DENSITY_ALPHA + (1.0 - MIN_DENSITY_ALPHA) * 2_f32.ln() / 100_f32.ln());
        assert!((f32::from(sparse[3]) - expected).abs() <= 1.0, "{sparse:?}");
        assert_eq!(&sparse[..3], &[0, 128, 0]);
    }

    #[test]
    fn later_layers_composite_over_earlier_ones() {
        let pts = [[5.5, 5.5]];
        let below = RasterLayer { points: &pts, color: Rgba([0, 0, 255, 255]), density: false, splat: 1 };
        let above = RasterLayer { color: Rgba([255, 0, 0, 128]), ..below };
        let b = rasterize_points(&[below, above], VIEW, 10, 10);
        let [r, g, bl, a] = pixel(&b, 5, 4);
        assert_eq!(a, 255);
        assert_eq!(g, 0);
        // Half red over blue.
        assert!((i32::from(r) - 128).abs() <= 1 && (i32::from(bl) - 127).abs() <= 1, "{r} {bl}");
    }

    #[test]
    fn degenerate_sizes_or_views_give_an_empty_bitmap() {
        let pts = [[1.0, 1.0]];
        let layer = RasterLayer { points: &pts, color: Rgba([0, 0, 0, 255]), density: true, splat: 1 };
        assert!(!rasterize_points(&[layer], VIEW, 0, 10).is_valid());
        let flat = RasterView { x: [1.0, 1.0], y: [0.0, 1.0] };
        assert!(!rasterize_points(&[layer], flat, 10, 10).is_valid());
    }

    #[test]
    fn a_million_points_fit_in_a_screen_sized_picture() {
        let pts: Vec<[f64; 2]> =
            (0..1_000_000).map(|i| [f64::from(i % 1000) / 100.0, f64::from(i / 1000) / 100.0]).collect();
        let layer = RasterLayer { points: &pts, color: Rgba([10, 20, 30, 255]), density: true, splat: 3 };
        let b = rasterize_points(&[layer], VIEW, 800, 400);
        assert_eq!(b.rgba.len(), 4 * 800 * 400);
        assert!(b.rgba.chunks_exact(4).all(|p| p[3] > 0));
    }

    #[test]
    fn sample_key_follows_changes_in_size_and_values() {
        let a: Vec<[f64; 2]> = (0..10_000).map(|i| [f64::from(i), 0.0]).collect();
        let mut b = a.clone();
        assert_eq!(sample_key(&a), sample_key(&b));
        b[0][1] = 1.0;
        assert_ne!(sample_key(&a), sample_key(&b));
        assert_ne!(sample_key(&a), sample_key(&a[..9_999]));
    }
}
