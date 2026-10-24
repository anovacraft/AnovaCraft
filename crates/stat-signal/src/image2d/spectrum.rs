//! Centred 2-D spectra, radial and angular averages, power-law slopes and
//! dominant spatial frequencies.
//!
//! # Spectra
//! The image (mean removed if asked) is multiplied by a window `w`
//! ([`Taper2`]), padded ([`Padding`]) and transformed ([`fft2`]); the
//! results are centred with [`fft_shift2`]. With `X_{k,l}` the transform:
//!
//! * **magnitude** `|X_{k,l}| / Σw` — a cosine of amplitude `a` exactly on
//!   a frequency bin shows `a/2` at each of its two conjugate cells;
//! * **phase** `arg X_{k,l}` in radians (relative to the top-left pixel);
//! * **power** `|X_{k,l}|² / (N Σw²)`, with `N` the transform size, so the
//!   power sums to `Σ(w x)² / Σw²`: the mean square of the image (its
//!   variance once the mean is removed) when there is no window.
//!
//! Row `i` of a centred spectrum has vertical frequency `f_y[i]` and
//! column `j` horizontal frequency `f_x[j]`, both ascending; the spectrum's
//! grid spacings are the frequency steps `1/(C dx)` and `1/(R dy)`.
//!
//! # Radial and angular averages
//! The radial frequency of a cell is `ρ = √(f_x² + f_y²)`. The **radial
//! spectrum** averages the power over rings `ρ ∈ [(k − ½)Δ, (k + ½)Δ)`,
//! `Δ` the larger of the two frequency steps, for `k = 0` up to the
//! smaller Nyquist frequency (van der Schaaf & van Hateren 1996, "Modelling
//! the power spectra of natural images", *Vision Research* 36(17)).
//! Fitting `log₁₀ P = b + s log₁₀ ρ` by least squares ([`power_law_fit`])
//! estimates the exponent `β = −s` of a `1/ρ^β` spectrum (β ≈ 2 for
//! natural scenes, 0 for white noise).
//!
//! The direction of a cell is the angle of its wave vector measured
//! counter-clockwise from the +x axis *as seen on screen* (y pointing
//! down), `θ = atan2(−f_y, f_x)`, folded into `[0°, 180°)` because a real
//! image's spectrum is symmetric through the origin. The **angular
//! spectrum** averages the power over equal angle sectors (optionally
//! within a band of `ρ`), excluding the zero frequency. A pattern of
//! stripes shows power at the direction *across* the stripes.
//!
//! # Radial profiles
//! [`directional_profile`] reads the power spectrum along one direction:
//! a straight cut through the centre (bilinear interpolation at steps of
//! one column or row), or the mean over a wedge of directions. Unlike the
//! ring averages it keeps directions apart, so it shows anisotropy.
//!
//! # Dominant frequencies
//! Cells whose power is a local maximum of its 3 × 3 neighbourhood (one of
//! each conjugate pair, zero frequency excluded) are ranked by power. Each
//! location is refined by fitting a parabola through the peak and its two
//! neighbours along each axis (Gonzalez & Woods 2018, §4.6, for reading
//! periodic patterns from the spectrum). The period is `1/ρ`, the share is
//! the pair's fraction of the total power away from zero frequency, and the
//! amplitude (`2 ×` magnitude) estimates the cosine's amplitude when the
//! peak is on a bin and no window is used.

use super::Grid;
use super::transform::{Padding, Taper2, fft_shift2, fft2, frequency_axes, pad, taper2};
use crate::{Result, StatError, invalid};

/// Options for [`spectrum2`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpectrumOptions2 {
    /// Subtract the image mean first (removes the zero-frequency spike).
    pub remove_mean: bool,
    /// Window applied before the transform.
    pub taper: Taper2,
    /// Padding applied after the window.
    pub padding: Padding,
}

impl Default for SpectrumOptions2 {
    fn default() -> Self {
        SpectrumOptions2 { remove_mean: true, taper: Taper2::None, padding: Padding::None }
    }
}

/// A centred 2-D spectrum.
#[derive(Debug, Clone, PartialEq)]
pub struct Spectrum2 {
    /// Horizontal frequencies of the columns, ascending.
    pub fx: Vec<f64>,
    /// Vertical frequencies of the rows, ascending.
    pub fy: Vec<f64>,
    /// `|X| / Σw`, centred.
    pub magnitude: Grid,
    /// `arg X` in radians, centred.
    pub phase: Grid,
    /// `|X|² / (N Σw²)`, centred.
    pub power: Grid,
}

impl Spectrum2 {
    /// The radial frequency `√(f_x² + f_y²)` of cell `(row, col)`.
    pub fn radius(&self, row: usize, col: usize) -> f64 {
        self.fx[col].hypot(self.fy[row])
    }

    /// The on-screen direction of cell `(row, col)` in degrees, `[0, 180)`.
    pub fn direction(&self, row: usize, col: usize) -> f64 {
        direction_deg(self.fx[col], self.fy[row])
    }

    /// The row and column of the zero frequency.
    pub fn centre(&self) -> (usize, usize) {
        (self.power.rows / 2, self.power.cols / 2)
    }
}

pub(crate) fn direction_deg(fx: f64, fy: f64) -> f64 {
    let t = (-fy).atan2(fx).to_degrees().rem_euclid(180.0);
    if t >= 180.0 - 1e-9 { 0.0 } else { t }
}

/// The centred magnitude, phase and power spectra of an image.
pub fn spectrum2(image: &Grid, options: &SpectrumOptions2) -> Result<Spectrum2> {
    image.check("image")?;
    let base = if options.remove_mean { image.subtract_mean() } else { image.clone() };
    let w = taper2(options.taper, image.rows, image.cols)?;
    let sum_w: f64 = w.data.iter().sum();
    let sum_w2: f64 = w.data.iter().map(|v| v * v).sum();
    if sum_w2 <= 0.0 || sum_w.abs() <= 0.0 {
        return Err(invalid("the window has no weight"));
    }
    let windowed = base.like(base.data.iter().zip(&w.data).map(|(a, b)| a * b).collect());
    let padded = pad(&windowed, options.padding)?;
    let (rows, cols) = (padded.rows, padded.cols);
    let n = (rows * cols) as f64;
    let f = fft2(&padded.to_complex());
    let centred = fft_shift2(&f.data, rows, cols);
    let (fx, fy) = frequency_axes(rows, cols, image.dx, image.dy);
    let fx = fft_shift2(&fx, 1, cols);
    let fy = fft_shift2(&fy, 1, rows);
    let (sx, sy) = (1.0 / (cols as f64 * image.dx), 1.0 / (rows as f64 * image.dy));
    let make = |data: Vec<f64>| Grid { rows, cols, data, dx: sx, dy: sy };
    Ok(Spectrum2 {
        magnitude: make(centred.iter().map(|z| z.abs() / sum_w.abs()).collect()),
        phase: make(centred.iter().map(|z| z.arg()).collect()),
        power: make(centred.iter().map(|z| z.norm_sqr() / (n * sum_w2)).collect()),
        fx,
        fy,
    })
}

/// Power averaged over rings of radial frequency.
#[derive(Debug, Clone, PartialEq)]
pub struct RadialSpectrum {
    /// Ring centres `kΔ`, starting at 0.
    pub frequency: Vec<f64>,
    /// Mean power in each ring.
    pub power: Vec<f64>,
    /// Number of cells in each ring.
    pub count: Vec<usize>,
}

/// The radially averaged power spectrum (rings of width `Δ`, the larger
/// frequency step, up to the smaller Nyquist frequency).
pub fn radial_spectrum(spectrum: &Spectrum2) -> RadialSpectrum {
    let p = &spectrum.power;
    let step = p.dx.max(p.dy);
    let limit = spectrum
        .fx
        .iter()
        .fold(0.0f64, |m, v| m.max(v.abs()))
        .min(spectrum.fy.iter().fold(0.0f64, |m, v| m.max(v.abs())));
    let bins = (limit / step + 1e-9).floor() as usize + 1;
    let mut sum = vec![0.0; bins];
    let mut count = vec![0usize; bins];
    for r in 0..p.rows {
        for c in 0..p.cols {
            let k = (spectrum.radius(r, c) / step).round() as usize;
            if k < bins {
                sum[k] += p.get(r, c);
                count[k] += 1;
            }
        }
    }
    RadialSpectrum {
        frequency: (0..bins).map(|k| k as f64 * step).collect(),
        power: sum.iter().zip(&count).map(|(s, &n)| if n > 0 { s / n as f64 } else { 0.0 }).collect(),
        count,
    }
}

/// A least-squares power law `P = 10^b ρ^s` fitted on log–log axes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PowerLawFit {
    /// Slope `s` of `log₁₀ P` on `log₁₀ ρ`.
    pub slope: f64,
    /// Standard error of the slope.
    pub slope_se: f64,
    /// Intercept `b`.
    pub intercept: f64,
    /// Spectral exponent `β = −s`.
    pub exponent: f64,
    /// Coefficient of determination on the log scale.
    pub r_squared: f64,
    /// Number of rings used.
    pub points: usize,
}

/// Fit a power law to the rings with `fmin ≤ ρ ≤ fmax` (`None` = no limit),
/// positive frequency and positive power. Needs at least 3 rings.
pub fn power_law_fit(radial: &RadialSpectrum, fmin: Option<f64>, fmax: Option<f64>) -> Result<PowerLawFit> {
    let (lo, hi) = (fmin.unwrap_or(0.0), fmax.unwrap_or(f64::INFINITY));
    let pts: Vec<(f64, f64)> = radial
        .frequency
        .iter()
        .zip(&radial.power)
        .filter(|&(&f, &p)| f > 0.0 && f >= lo && f <= hi && p > 0.0)
        .map(|(&f, &p)| (f.log10(), p.log10()))
        .collect();
    let n = pts.len();
    if n < 3 {
        return Err(StatError::InsufficientData { required: 3, found: n });
    }
    let mx = pts.iter().map(|p| p.0).sum::<f64>() / n as f64;
    let my = pts.iter().map(|p| p.1).sum::<f64>() / n as f64;
    let sxx: f64 = pts.iter().map(|p| (p.0 - mx).powi(2)).sum();
    let sxy: f64 = pts.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum();
    let syy: f64 = pts.iter().map(|p| (p.1 - my).powi(2)).sum();
    let slope = sxy / sxx;
    let intercept = my - slope * mx;
    let sse = (syy - slope * sxy).max(0.0);
    let slope_se = (sse / (n - 2) as f64 / sxx).sqrt();
    let r_squared = if syy > 0.0 { 1.0 - sse / syy } else { 1.0 };
    Ok(PowerLawFit { slope, slope_se, intercept, exponent: -slope, r_squared, points: n })
}

/// Power averaged over sectors of direction.
#[derive(Debug, Clone, PartialEq)]
pub struct AngularSpectrum {
    /// Sector centres in degrees, `0, w, 2w, …` with `w = 180°/sectors`.
    pub direction: Vec<f64>,
    /// Mean power in each sector.
    pub power: Vec<f64>,
    /// Number of cells in each sector.
    pub count: Vec<usize>,
}

/// The angular (orientation) spectrum: mean power in `sectors` equal
/// sectors of direction, from cells with `fmin ≤ ρ ≤ fmax` (zero frequency
/// excluded).
pub fn angular_spectrum(
    spectrum: &Spectrum2,
    sectors: usize,
    fmin: Option<f64>,
    fmax: Option<f64>,
) -> Result<AngularSpectrum> {
    if !(2..=720).contains(&sectors) {
        return Err(invalid("use between 2 and 720 direction sectors"));
    }
    let width = 180.0 / sectors as f64;
    let (lo, hi) = (fmin.unwrap_or(0.0), fmax.unwrap_or(f64::INFINITY));
    let p = &spectrum.power;
    let mut sum = vec![0.0; sectors];
    let mut count = vec![0usize; sectors];
    for r in 0..p.rows {
        for c in 0..p.cols {
            let rho = spectrum.radius(r, c);
            if rho == 0.0 || rho < lo || rho > hi {
                continue;
            }
            let k = ((spectrum.direction(r, c) + width / 2.0) / width).floor() as usize % sectors;
            sum[k] += p.get(r, c);
            count[k] += 1;
        }
    }
    Ok(AngularSpectrum {
        direction: (0..sectors).map(|k| k as f64 * width).collect(),
        power: sum.iter().zip(&count).map(|(s, &n)| if n > 0 { s / n as f64 } else { 0.0 }).collect(),
        count,
    })
}

/// A profile of the power spectrum along one direction.
#[derive(Debug, Clone, PartialEq)]
pub struct DirectionalProfile {
    /// Direction of the profile in degrees, `[0, 180)`, measured as for
    /// [`Spectrum2::direction`].
    pub direction: f64,
    /// Half-width of the wedge averaged, in degrees (0 for a straight cut).
    pub half_width: f64,
    /// Radial frequencies of the samples, ascending from 0.
    pub frequency: Vec<f64>,
    /// Power at each sample: interpolated on the cut, or the wedge mean.
    pub power: Vec<f64>,
    /// Cells averaged for each sample (1 on a straight cut).
    pub count: Vec<usize>,
}

/// The power spectrum along direction `direction` (degrees, as for
/// [`Spectrum2::direction`]), from zero frequency outwards.
///
/// With `half_width = 0` the profile is a straight cut through the
/// centred spectrum: the power is interpolated bilinearly between cells at
/// steps of one column or one row, whichever the direction crosses faster,
/// `Δ = 1 / max(|cos θ| / s_x, sin θ / s_y)` with `s_x`, `s_y` the
/// frequency steps, so a cut at 0° or 90° reads the centre row or column
/// exactly and a cut along a diagonal of cells reads those cells. A real image's spectrum is symmetric
/// through the origin, so each sample is read on whichever side of the
/// centre lies inside the grid, and the cut runs out to the edge of the
/// spectrum (the Nyquist frequency on the axes, further on diagonals of a
/// square grid). With `half_width > 0` each sample is instead the mean
/// power of the cells within `half_width` degrees of the direction whose
/// radial frequency rounds to it (halfway values to the outer sample) (a wedge of the rings of
/// [`radial_spectrum`]); samples with no cell are left out.
///
/// # Errors
/// A direction that is not finite, or a half-width outside `[0, 90]`.
pub fn directional_profile(spectrum: &Spectrum2, direction: f64, half_width: f64) -> Result<DirectionalProfile> {
    if !direction.is_finite() {
        return Err(invalid("the profile direction must be a number of degrees"));
    }
    if !(0.0..=90.0).contains(&half_width) {
        return Err(invalid("the wedge half-width must be between 0 and 90 degrees"));
    }
    let p = &spectrum.power;
    let theta = direction.rem_euclid(180.0);
    let (sin, cos) = theta.to_radians().sin_cos();
    let (sx, sy) = (p.dx, p.dy);
    let step = 1.0 / (cos.abs() / sx).max(sin / sy);
    let (r0, c0) = spectrum.centre();
    let (r0, c0) = (r0 as f64, c0 as f64);
    let (max_r, max_c) = ((p.rows - 1) as f64, (p.cols - 1) as f64);
    let inside = |r: f64, c: f64| r >= -1e-9 && c >= -1e-9 && r <= max_r + 1e-9 && c <= max_c + 1e-9;
    let bilinear = |r: f64, c: f64| {
        let (r, c) = (r.clamp(0.0, max_r), c.clamp(0.0, max_c));
        let (ri, ci) = (r.floor() as usize, c.floor() as usize);
        let (r1, c1) = ((ri + 1).min(p.rows - 1), (ci + 1).min(p.cols - 1));
        let (tr, tc) = (r - ri as f64, c - ci as f64);
        let top = p.get(ri, ci) * (1.0 - tc) + p.get(ri, c1) * tc;
        let bottom = p.get(r1, ci) * (1.0 - tc) + p.get(r1, c1) * tc;
        top * (1.0 - tr) + bottom * tr
    };
    // Cell offsets per unit frequency along the direction (rows grow with f_y,
    // and the on-screen angle is measured with f_y pointing down).
    let (dr, dc) = (-sin / sy, cos / sx);
    let mut samples = 0usize;
    loop {
        let f = samples as f64 * step;
        if !(inside(r0 + f * dr, c0 + f * dc) || inside(r0 - f * dr, c0 - f * dc)) {
            break;
        }
        samples += 1;
    }
    let mut out = DirectionalProfile { direction: theta, half_width, frequency: Vec::new(), power: Vec::new(), count: Vec::new() };
    if half_width == 0.0 {
        for k in 0..samples {
            let f = k as f64 * step;
            let (r, c) = (r0 + f * dr, c0 + f * dc);
            let value = if inside(r, c) { bilinear(r, c) } else { bilinear(r0 - f * dr, c0 - f * dc) };
            out.frequency.push(f);
            out.power.push(value);
            out.count.push(1);
        }
        return Ok(out);
    }
    let mut sum = vec![0.0; samples];
    let mut count = vec![0usize; samples];
    for r in 0..p.rows {
        for c in 0..p.cols {
            let rho = spectrum.radius(r, c);
            // Halfway values go to the outer sample (with a margin, so a tie
            // does not hang on the last bit of the frequency).
            let k = (rho / step + 0.5 + 1e-9).floor() as usize;
            if k >= samples {
                continue;
            }
            let off = (spectrum.direction(r, c) - theta).rem_euclid(180.0);
            if rho == 0.0 || off.min(180.0 - off) <= half_width + 1e-9 {
                sum[k] += p.get(r, c);
                count[k] += 1;
            }
        }
    }
    for k in 0..samples {
        if count[k] > 0 {
            out.frequency.push(k as f64 * step);
            out.power.push(sum[k] / count[k] as f64);
            out.count.push(count[k]);
        }
    }
    Ok(out)
}

/// A dominant spatial frequency (one of a conjugate pair).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrequencyPeak {
    /// Row of the peak cell in the centred spectrum.
    pub row: usize,
    /// Column of the peak cell in the centred spectrum.
    pub col: usize,
    /// Refined horizontal frequency.
    pub fx: f64,
    /// Refined vertical frequency.
    pub fy: f64,
    /// Radial frequency `√(f_x² + f_y²)`.
    pub frequency: f64,
    /// Period (wavelength) `1/ρ`, in units of `dx`.
    pub period: f64,
    /// Direction of the wave vector in degrees, `[0, 180)`, counter-clockwise
    /// from +x on screen.
    pub direction: f64,
    /// Power of the peak cell.
    pub power: f64,
    /// Fraction of the non-zero-frequency power in the pair of peak cells.
    pub share: f64,
    /// Twice the peak cell's magnitude (a cosine's amplitude when on a bin).
    pub amplitude: f64,
}

/// Up to `max_peaks` local maxima of the power spectrum with radial
/// frequency at least `min_frequency`, strongest first.
pub fn dominant_frequencies(spectrum: &Spectrum2, max_peaks: usize, min_frequency: f64) -> Vec<FrequencyPeak> {
    let p = &spectrum.power;
    let (rows, cols) = (p.rows, p.cols);
    let (cr, cc) = spectrum.centre();
    let total: f64 = p.data.iter().sum::<f64>() - p.get(cr, cc);
    let mut found = Vec::new();
    for r in 0..rows {
        for c in 0..cols {
            let (fx, fy) = (spectrum.fx[c], spectrum.fy[r]);
            // One of each conjugate pair: the upper half-plane on screen.
            if !(-fy > 0.0 || (fy == 0.0 && fx > 0.0)) {
                continue;
            }
            let rho = spectrum.radius(r, c);
            if rho < min_frequency {
                continue;
            }
            let v = p.get(r, c);
            if v <= 0.0 {
                continue;
            }
            let mut is_max = true;
            'n: for dr in -1i64..=1 {
                for dc in -1i64..=1 {
                    if dr == 0 && dc == 0 {
                        continue;
                    }
                    let (nr, nc) = (r as i64 + dr, c as i64 + dc);
                    if nr < 0 || nc < 0 || nr >= rows as i64 || nc >= cols as i64 {
                        continue;
                    }
                    let u = p.get(nr as usize, nc as usize);
                    // Ties go to the earlier cell.
                    let earlier = dr < 0 || (dr == 0 && dc < 0);
                    if u > v || (earlier && u == v) {
                        is_max = false;
                        break 'n;
                    }
                }
            }
            if is_max {
                found.push((r, c));
            }
        }
    }
    found.sort_by(|a, b| p.get(b.0, b.1).total_cmp(&p.get(a.0, a.1)));
    found.truncate(max_peaks);
    let vertex = |m: f64, z: f64, q: f64| {
        let d = m - 2.0 * z + q;
        if d < 0.0 { (0.5 * (m - q) / d).clamp(-0.5, 0.5) } else { 0.0 }
    };
    found
        .into_iter()
        .map(|(r, c)| {
            let v = p.get(r, c);
            let dc = if c > 0 && c + 1 < cols { vertex(p.get(r, c - 1), v, p.get(r, c + 1)) } else { 0.0 };
            let dr = if r > 0 && r + 1 < rows { vertex(p.get(r - 1, c), v, p.get(r + 1, c)) } else { 0.0 };
            let fx = spectrum.fx[c] + dc * p.dx;
            let fy = spectrum.fy[r] + dr * p.dy;
            let rho = fx.hypot(fy);
            FrequencyPeak {
                row: r,
                col: c,
                fx,
                fy,
                frequency: rho,
                period: 1.0 / rho,
                direction: direction_deg(fx, fy),
                power: v,
                share: if total > 0.0 { 2.0 * v / total } else { 0.0 },
                amplitude: 2.0 * spectrum.magnitude.get(r, c),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window::Window;
    use std::f64::consts::PI;

    fn stripes(rows: usize, cols: usize, kx: f64, ky: f64, a: f64) -> Grid {
        Grid::from_fn(rows, cols, |r, c| {
            a * (2.0 * PI * (kx * c as f64 / cols as f64 + ky * r as f64 / rows as f64)).cos()
        })
    }

    #[test]
    fn on_bin_cosine_gives_its_amplitude_period_and_direction() {
        // 8 cycles across and −4 cycles down (wave vector up and to the right on screen).
        let g = stripes(32, 64, 8.0, -4.0, 3.0).with_spacing(0.5, 0.5).unwrap();
        let s = spectrum2(&g, &SpectrumOptions2::default()).unwrap();
        let peaks = dominant_frequencies(&s, 3, 0.0);
        let top = peaks[0];
        assert!((top.fx - 8.0 / 32.0).abs() < 1e-12);
        assert!((top.fy + 4.0 / 16.0).abs() < 1e-12);
        assert!((top.amplitude - 3.0).abs() < 1e-10);
        assert!((top.share - 1.0).abs() < 1e-10);
        assert!((top.direction - 45.0).abs() < 1e-9);
        assert!((top.period - 1.0 / (0.25f64.hypot(0.25))).abs() < 1e-9);
        // Power sums to the variance (a²/2).
        let total: f64 = s.power.data.iter().sum();
        assert!((total - 4.5).abs() < 1e-10);
    }

    #[test]
    fn off_bin_peak_is_refined_within_a_bin() {
        let g = stripes(64, 64, 10.4, 0.0, 1.0);
        let opts = SpectrumOptions2 { taper: Taper2::Separable(Window::Hann), ..Default::default() };
        let s = spectrum2(&g, &opts).unwrap();
        let top = dominant_frequencies(&s, 1, 0.0)[0];
        assert!((top.fx * 64.0 - 10.4).abs() < 0.25, "{}", top.fx * 64.0);
        assert!(top.direction.abs() < 1e-9);
    }

    #[test]
    fn angular_spectrum_finds_stripe_direction() {
        // Wave vector straight down the image → direction 90°.
        let g = stripes(48, 48, 0.0, 6.0, 1.0);
        let s = spectrum2(&g, &SpectrumOptions2::default()).unwrap();
        let a = angular_spectrum(&s, 12, None, None).unwrap();
        let best = (0..12).max_by(|&i, &j| a.power[i].total_cmp(&a.power[j])).unwrap();
        assert_eq!(a.direction[best], 90.0);
    }

    #[test]
    fn axis_cuts_read_the_centre_row_and_column() {
        let g = stripes(32, 48, 5.0, 3.0, 1.0).with_spacing(0.5, 2.0).unwrap();
        let s = spectrum2(&g, &SpectrumOptions2::default()).unwrap();
        let (r0, c0) = s.centre();
        let across = directional_profile(&s, 0.0, 0.0).unwrap();
        // Out to the Nyquist frequency 1/(2 dx) = 1, in steps of 1/(C dx).
        assert_eq!(across.frequency.len(), 25);
        assert!((across.frequency[24] - 1.0).abs() < 1e-12);
        for (k, (&f, &v)) in across.frequency.iter().zip(&across.power).enumerate() {
            assert!((f - k as f64 / 24.0).abs() < 1e-12);
            // Past the last positive column the mirror cell is read.
            let want = if c0 + k < 48 { s.power.get(r0, c0 + k) } else { s.power.get(r0, c0 - k) };
            assert!((v - want).abs() < 1e-15);
        }
        // 90° points up the screen: rows above the centre (negative f_y).
        let down = directional_profile(&s, 90.0, 0.0).unwrap();
        assert_eq!(down.frequency.len(), 17);
        for (k, &v) in down.power.iter().enumerate() {
            assert!((v - s.power.get(r0 - k, c0)).abs() < 1e-15);
        }
    }

    #[test]
    fn a_cut_through_a_peak_finds_it_and_a_cut_across_misses_it() {
        // Wave vector at 45° on screen: 8 cycles across, −8 down.
        let g = stripes(64, 64, 8.0, -8.0, 2.0);
        let s = spectrum2(&g, &SpectrumOptions2::default()).unwrap();
        let along = directional_profile(&s, 45.0, 0.0).unwrap();
        let best = (0..along.power.len()).max_by(|&i, &j| along.power[i].total_cmp(&along.power[j])).unwrap();
        // Diagonal steps of one cell: 8 cells is 8 steps, at ρ = 8√2/64.
        assert!((along.frequency[best] - 8.0 * 2f64.sqrt() / 64.0).abs() < 1e-12);
        assert!((along.power[best] - s.power.get(32 - 8, 32 + 8)).abs() < 1e-15);
        // The diagonal of a square grid reaches past the axis Nyquist frequency.
        assert!(*along.frequency.last().unwrap() > 0.5);
        let across = directional_profile(&s, 135.0, 0.0).unwrap();
        assert!(across.power.iter().all(|&v| v < 1e-20));
    }

    #[test]
    fn a_full_wedge_matches_the_radial_spectrum() {
        let g = stripes(40, 40, 3.0, 7.0, 1.0);
        let s = spectrum2(&g, &SpectrumOptions2::default()).unwrap();
        let radial = radial_spectrum(&s);
        let wedge = directional_profile(&s, 0.0, 90.0).unwrap();
        for (k, &f) in radial.frequency.iter().enumerate() {
            let j = wedge.frequency.iter().position(|&w| (w - f).abs() < 1e-12).unwrap();
            assert_eq!(wedge.count[j], radial.count[k]);
            assert!((wedge.power[j] - radial.power[k]).abs() < 1e-15);
        }
        // A narrow wedge around the wave direction holds all the power there.
        let d = dominant_frequencies(&s, 1, 0.0)[0].direction;
        let narrow = directional_profile(&s, d, 2.0).unwrap();
        assert!(narrow.power.iter().any(|&v| v > 0.1));
        assert!(directional_profile(&s, 0.0, 91.0).is_err());
        assert!(directional_profile(&s, f64::NAN, 0.0).is_err());
    }

    #[test]
    fn white_noise_has_a_flat_radial_spectrum() {
        let mut state = 12345u64;
        let noise: Vec<f64> = (0..64 * 64)
            .map(|_| {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                ((state >> 11) as f64 / (1u64 << 53) as f64) - 0.5
            })
            .collect();
        let g = Grid::new(64, 64, noise).unwrap();
        let s = spectrum2(&g, &SpectrumOptions2::default()).unwrap();
        let radial = radial_spectrum(&s);
        assert_eq!(radial.frequency.len(), 33);
        let fit = power_law_fit(&radial, Some(0.05), None).unwrap();
        assert!(fit.exponent.abs() < 0.15, "{}", fit.exponent);
    }
}
