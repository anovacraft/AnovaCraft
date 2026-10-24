//! Peak finding in a sampled signal.
//!
//! The definitions follow the usual topographic ones (as in SciPy's
//! `find_peaks`, `peak_prominences` and `peak_widths`):
//!
//! * a **peak** is a local maximum: a point (or the middle of a flat run of
//!   equal points, rounded down) higher than both neighbours; the first and
//!   last points are never peaks;
//! * its **prominence** is its height above the higher of the two lowest
//!   points reached on each side before the signal climbs above the peak
//!   (or the signal ends) — those two points are the peak's **bases**;
//! * its **width** is measured at half the prominence below the top
//!   (`rel_height = 0.5`, the full width at half maximum for a peak on a flat
//!   baseline), between the crossings found by linear interpolation, not
//!   looking beyond the bases.
//!
//! Peaks are kept when their height and prominence reach the minimums; then,
//! taking peaks from the highest down, any lower peak closer than the
//! minimum separation (in points) to a kept one is dropped.
//!
//! [`noise_level`] estimates the noise standard deviation robustly from the
//! first differences, `σ̂ = 1.4826 · MAD(Δy) / √2` (for white noise the
//! differences have variance `2σ²`; the median absolute deviation ignores
//! the few large differences on the flanks of peaks).

use crate::{Result, invalid};

/// What counts as a peak in [`find_peaks`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PeakSearch {
    /// Smallest height (signal value at the peak).
    pub min_height: f64,
    /// Smallest prominence.
    pub min_prominence: f64,
    /// Smallest separation between kept peaks, in points (1: no limit).
    pub min_distance: usize,
}

impl Default for PeakSearch {
    fn default() -> Self {
        Self { min_height: f64::NEG_INFINITY, min_prominence: 0.0, min_distance: 1 }
    }
}

/// A peak found by [`find_peaks`] (positions are indices into the signal).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Peak {
    /// Index of the top.
    pub index: usize,
    /// Signal value at the top.
    pub height: f64,
    /// Prominence.
    pub prominence: f64,
    /// Index of the left base.
    pub left_base: usize,
    /// Index of the right base.
    pub right_base: usize,
    /// Left half-prominence crossing (fractional index).
    pub left_crossing: f64,
    /// Right half-prominence crossing (fractional index).
    pub right_crossing: f64,
}

impl Peak {
    /// Width at half prominence in points.
    #[must_use]
    pub fn width_points(&self) -> f64 {
        self.right_crossing - self.left_crossing
    }

    /// Signal level at which the width is measured.
    #[must_use]
    pub fn width_height(&self) -> f64 {
        self.height - 0.5 * self.prominence
    }
}

fn local_maxima(y: &[f64]) -> Vec<usize> {
    let n = y.len();
    let mut peaks = Vec::new();
    if n < 3 {
        return peaks;
    }
    let mut i = 1;
    while i < n - 1 {
        if y[i - 1] < y[i] {
            let mut ahead = i + 1;
            while ahead < n - 1 && y[ahead] == y[i] {
                ahead += 1;
            }
            if y[ahead] < y[i] {
                peaks.push((i + ahead - 1) / 2);
                i = ahead;
                continue;
            }
        }
        i += 1;
    }
    peaks
}

fn prominence(y: &[f64], peak: usize) -> (f64, usize, usize) {
    let top = y[peak];
    let (mut left_min, mut left_base) = (top, peak);
    let mut i = peak as isize;
    while i >= 0 && y[i as usize] <= top {
        if y[i as usize] < left_min {
            left_min = y[i as usize];
            left_base = i as usize;
        }
        i -= 1;
    }
    let (mut right_min, mut right_base) = (top, peak);
    let mut j = peak;
    while j < y.len() && y[j] <= top {
        if y[j] < right_min {
            right_min = y[j];
            right_base = j;
        }
        j += 1;
    }
    (top - left_min.max(right_min), left_base, right_base)
}

fn crossings(y: &[f64], peak: usize, prom: f64, left_base: usize, right_base: usize) -> (f64, f64) {
    let level = y[peak] - 0.5 * prom;
    let mut i = peak;
    while left_base < i && level < y[i] {
        i -= 1;
    }
    let mut left = i as f64;
    if y[i] < level {
        left += (level - y[i]) / (y[i + 1] - y[i]);
    }
    let mut j = peak;
    while j < right_base && level < y[j] {
        j += 1;
    }
    let mut right = j as f64;
    if y[j] < level {
        right -= (level - y[j]) / (y[j - 1] - y[j]);
    }
    (left, right)
}

/// Peaks of `y` meeting `search`, in order of position.
///
/// # Errors
/// [`crate::StatError::InvalidArgument`] for missing values or a zero
/// separation.
pub fn find_peaks(y: &[f64], search: &PeakSearch) -> Result<Vec<Peak>> {
    if y.iter().any(|v| !v.is_finite()) {
        return Err(invalid("the signal must not contain missing or infinite values"));
    }
    if search.min_distance == 0 {
        return Err(invalid("the minimum separation must be at least one point"));
    }
    let mut peaks: Vec<usize> = local_maxima(y).into_iter().filter(|&p| y[p] >= search.min_height).collect();
    if search.min_distance > 1 {
        let mut order: Vec<usize> = (0..peaks.len()).collect();
        order.sort_by(|&a, &b| y[peaks[a]].total_cmp(&y[peaks[b]]));
        let mut keep = vec![true; peaks.len()];
        for &j in order.iter().rev() {
            if !keep[j] {
                continue;
            }
            let mut k = j;
            while k > 0 && peaks[j] - peaks[k - 1] < search.min_distance {
                keep[k - 1] = false;
                k -= 1;
            }
            let mut k = j + 1;
            while k < peaks.len() && peaks[k] - peaks[j] < search.min_distance {
                keep[k] = false;
                k += 1;
            }
        }
        peaks = peaks.into_iter().zip(keep).filter_map(|(p, k)| k.then_some(p)).collect();
    }
    Ok(peaks
        .into_iter()
        .filter_map(|p| {
            let (prom, left_base, right_base) = prominence(y, p);
            (prom >= search.min_prominence).then(|| {
                let (left_crossing, right_crossing) = crossings(y, p, prom, left_base, right_base);
                Peak { index: p, height: y[p], prominence: prom, left_base, right_base, left_crossing, right_crossing }
            })
        })
        .collect())
}

/// Robust noise standard deviation of `y` from its first differences (see
/// the module documentation); 0 for fewer than three points.
#[must_use]
pub fn noise_level(y: &[f64]) -> f64 {
    if y.len() < 3 {
        return 0.0;
    }
    let mut d: Vec<f64> = y.windows(2).map(|w| w[1] - w[0]).filter(|v| v.is_finite()).collect();
    let median = |v: &mut Vec<f64>| {
        v.sort_by(f64::total_cmp);
        let n = v.len();
        if n % 2 == 1 { v[n / 2] } else { 0.5 * (v[n / 2 - 1] + v[n / 2]) }
    };
    let m = median(&mut d);
    let mut dev: Vec<f64> = d.iter().map(|v| (v - m).abs()).collect();
    1.482_602_218_505_602 * median(&mut dev) / std::f64::consts::SQRT_2
}

/// Position in x units of a fractional index `t` into `x` (linear
/// interpolation between neighbouring positions).
#[must_use]
pub fn position_at(x: &[f64], t: f64) -> f64 {
    let n = x.len();
    if n == 0 {
        return f64::NAN;
    }
    let t = t.clamp(0.0, (n - 1) as f64);
    let i = (t.floor() as usize).min(n.saturating_sub(2));
    if n == 1 {
        return x[0];
    }
    x[i] + (t - i as f64) * (x[i + 1] - x[i])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plateaus_prominence_and_widths() {
        let y = [0.0, 1.0, 3.0, 3.0, 3.0, 1.0, 0.0, 2.0, 0.5, 0.0];
        let peaks = find_peaks(&y, &PeakSearch::default()).unwrap();
        assert_eq!(peaks.iter().map(|p| p.index).collect::<Vec<_>>(), vec![3, 7]);
        assert_eq!(peaks[0].prominence, 3.0);
        assert_eq!(peaks[1].prominence, 2.0);
        // Half prominence 1.5 crosses between indices 1–2 and 4–5.
        assert!((peaks[0].left_crossing - 1.25).abs() < 1e-12 && (peaks[0].right_crossing - 4.75).abs() < 1e-12);
        let few = find_peaks(&y, &PeakSearch { min_distance: 5, ..PeakSearch::default() }).unwrap();
        assert_eq!(few.len(), 1);
        let tall = find_peaks(&y, &PeakSearch { min_prominence: 2.5, ..PeakSearch::default() }).unwrap();
        assert_eq!(tall.len(), 1);
    }

    #[test]
    fn noise_of_a_known_sequence() {
        // Alternating ±1: differences ±2, MAD 2 → σ̂ = 1.4826·2/√2.
        let y: Vec<f64> = (0..101).map(|i| if i % 2 == 0 { 1.0 } else { -1.0 }).collect();
        assert!((noise_level(&y) - 1.482_602_218_505_602 * 2.0_f64.sqrt()).abs() < 1e-12);
    }
}
