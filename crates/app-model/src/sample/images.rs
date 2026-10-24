//! Samples for Series and Signals ▸ Signals and spectra ▸ Images: a machined
//! surface scan with tool marks hiding a pit, and a scene photographed
//! blurred and shifted.

use std::f64::consts::PI;

use super::{Sample, col, table};
use crate::image_analysis::{
    DeblurMethod, ImageAnalysisRequest, ImageFilterSettings, ImageInput, ImageMethod, ImageRestoreSettings,
    ImageSpectrumSettings, RestoreOperation,
};
use crate::project::SavedReport;
use stat_core::rng::Rng;
use stat_data::{Column, DataTable};
use stat_signal::image2d::{Grid, fourier_shift, gaussian_psf};

/// Pixels across and down the surface scan.
const SCAN: usize = 256;
/// Pixels across and down the scene.
const SCENE: usize = 128;

/// Long-format pixel columns `x`, `y` for a `side × side` image, row by row
/// from the top left.
fn pixel_columns(side: usize, spacing: f64) -> (Vec<f64>, Vec<f64>) {
    let mut x = Vec::with_capacity(side * side);
    let mut y = Vec::with_capacity(side * side);
    for r in 0..side {
        for c in 0..side {
            x.push(c as f64 * spacing);
            y.push(r as f64 * spacing);
        }
    }
    (x, y)
}

/// **Simulated** height map of a machined steel surface, 256 × 256 pixels
/// of 2 µm (a 512 µm square), one row per pixel (x across, y down, both in
/// µm). Height (µm) at pixel centre `(x, y)`:
///
/// `h = 0.6 cos(2π(x cos 20° + y sin 20°)/16) + 0.35 cos(2π(x cos 110° + y sin 110°)/24)
///      + 0.25 sin(2π x/400) sin(2π y/520) − 0.9 exp(−r²/(2·6²)) + 0.12 ε`
///
/// two sets of tool marks (16 µm and 24 µm apart, in directions 20° and
/// 110°), a slow waviness, a pit 0.9 µm deep and 6 µm wide centred at
/// (340, 180) µm (`r` is the distance from it), and sensor noise
/// `ε ~ N(0, 1)` (`stat_core::rng::Rng`, seed `0x5CA7_5E1F`). Heights are
/// rounded to 0.001 µm. The pit is about as deep as the tool marks are
/// high, so it is hard to see until the marks are notched out.
#[must_use]
pub fn machined_surface_scan() -> DataTable {
    let mut rng = Rng::new(0x5CA7_5E1F);
    let spacing = 2.0;
    let (x, y) = pixel_columns(SCAN, spacing);
    let wave = |x: f64, y: f64, deg: f64, period: f64| {
        let t = deg.to_radians();
        (2.0 * PI * (x * t.cos() + y * t.sin()) / period).cos()
    };
    let height: Vec<f64> = x
        .iter()
        .zip(&y)
        .map(|(&x, &y)| {
            let r2 = (x - 340.0).powi(2) + (y - 180.0).powi(2);
            let h = 0.6 * wave(x, y, 20.0, 16.0)
                + 0.35 * wave(x, y, 110.0, 24.0)
                + 0.25 * (2.0 * PI * x / 400.0).sin() * (2.0 * PI * y / 520.0).sin()
                - 0.9 * (-r2 / (2.0 * 36.0)).exp()
                + 0.12 * rng.normal();
            (h * 1e3).round() / 1e3
        })
        .collect();
    table(
        "Machined surface scan (simulated)",
        vec![Column::numeric("x (µm)", x), Column::numeric("y (µm)", y), Column::numeric("Height (µm)", height)],
    )
}

pub(super) fn machined_surface_scan_sample() -> Sample {
    let t = machined_surface_scan();
    let input = ImageInput { value: col(&t, "Height (µm)"), x: col(&t, "x (µm)"), y: col(&t, "y (µm)"), group: None };
    let reports = vec![
        ImageAnalysisRequest { input, method: ImageMethod::Spectrum(ImageSpectrumSettings { peaks: 4, profile_peaks: true, ..ImageSpectrumSettings::default() }) },
        ImageAnalysisRequest { input, method: ImageMethod::Filter(ImageFilterSettings::auto_notch(2)) },
    ];
    Sample { table: t, reports: reports.into_iter().map(|request| SavedReport::ImageAnalysis { request }).collect() }
}

/// The sharp scene: a few bright discs and bars on a mid-grey background
/// with a gentle gradient (values roughly 0.2–1).
fn scene() -> Grid {
    let n = SCENE as f64;
    Grid::from_fn(SCENE, SCENE, |r, c| {
        let (x, y) = (c as f64, r as f64);
        let mut v = 0.3 + 0.1 * x / n;
        for (cx, cy, rad, level) in [(32.0, 40.0, 10.0, 0.55), (90.0, 30.0, 6.0, 0.6), (70.0, 90.0, 14.0, 0.4)] {
            if (x - cx).hypot(y - cy) <= rad {
                v += level;
            }
        }
        if (20.0..=60.0).contains(&x) && (95.0..=101.0).contains(&y) {
            v += 0.5;
        }
        if (100.0..=106.0).contains(&x) && (60.0..=118.0).contains(&y) {
            v += 0.45;
        }
        v
    })
}

/// `image` convolved with `psf` (odd sides, centred), the image extended
/// beyond its border by repeating the edge pixels.
fn blur_replicated(image: &Grid, psf: &Grid) -> Grid {
    let (hr, hc) = ((psf.rows / 2) as isize, (psf.cols / 2) as isize);
    let clamp = |i: isize, n: usize| i.clamp(0, n as isize - 1) as usize;
    Grid::from_fn(image.rows, image.cols, |r, c| {
        let mut sum = 0.0;
        for i in 0..psf.rows {
            for j in 0..psf.cols {
                let rr = clamp(r as isize + hr - i as isize, image.rows);
                let cc = clamp(c as isize + hc - j as isize, image.cols);
                sum += psf.get(i, j) * image.get(rr, cc);
            }
        }
        sum
    })
}

/// **Simulated** photographs of one scene, 128 × 128 pixels, one row per
/// pixel (x across, y down, in pixels):
///
/// * **Sharp** — the scene (three discs and two bars on a shaded
///   background) plus noise `0.01 ε`.
/// * **Blurred** — the scene convolved with a Gaussian blur of standard
///   deviation 2 pixels (edge pixels repeated beyond the border), plus
///   noise `0.005 ε`.
/// * **Shifted** — the scene moved 5.3 pixels right and 3.6 pixels up (a
///   circular Fourier shift), plus noise `0.01 ε`.
///
/// `ε ~ N(0, 1)` independent (`stat_core::rng::Rng`, seed `0x0B5C_E7E5`);
/// values rounded to 0.0001.
#[must_use]
pub fn blurred_shifted_scene() -> DataTable {
    let mut rng = Rng::new(0x0B5C_E7E5);
    let sharp = scene();
    let psf = gaussian_psf(2.0, 2.0).expect("valid blur");
    let blurred = blur_replicated(&sharp, &psf);
    let shifted = fourier_shift(&sharp, -3.6, 5.3).expect("shift");
    let round = |v: f64| (v * 1e4).round() / 1e4;
    let (x, y) = pixel_columns(SCENE, 1.0);
    let mut noisy = |g: &Grid, sd: f64| g.data.iter().map(|&v| round(v + sd * rng.normal())).collect::<Vec<_>>();
    let sharp_v = noisy(&sharp, 0.01);
    let blurred_v = noisy(&blurred, 0.005);
    let shifted_v = noisy(&shifted, 0.01);
    table(
        "Blurred and shifted scene (simulated)",
        vec![
            Column::numeric("x", x),
            Column::numeric("y", y),
            Column::numeric("Sharp", sharp_v),
            Column::numeric("Blurred", blurred_v),
            Column::numeric("Shifted", shifted_v),
        ],
    )
}

pub(super) fn blurred_shifted_scene_sample() -> Sample {
    let t = blurred_shifted_scene();
    let input = |name: &str| ImageInput { value: col(&t, name), x: col(&t, "x"), y: col(&t, "y"), group: None };
    let reports = vec![
        ImageAnalysisRequest {
            input: input("Blurred"),
            method: ImageMethod::Restore(ImageRestoreSettings {
                operation: RestoreOperation::Deblur {
                    method: DeblurMethod::Wiener { lambda: 0.01 },
                    sigma_x: 2.0,
                    sigma_y: 2.0,
                },
            }),
        },
        ImageAnalysisRequest {
            input: input("Sharp"),
            method: ImageMethod::Restore(ImageRestoreSettings {
                operation: RestoreOperation::Register { other: col(&t, "Shifted"), upsample: 20 },
            }),
        },
    ];
    Sample { table: t, reports: reports.into_iter().map(|request| SavedReport::ImageAnalysis { request }).collect() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image_analysis::ImageAnalysis;
    use crate::report::{SectionContent, SectionKind};
    use stat_data::RowStateManager;

    fn table_of(a: &ImageAnalysis, kind: SectionKind) -> crate::report::TextTable {
        a.sections()
            .into_iter()
            .find_map(|s| match s.content {
                SectionContent::Table(t) if s.kind == kind => Some(t),
                _ => None,
            })
            .expect("table")
    }

    #[test]
    fn surface_scan_spectrum_finds_both_tool_marks() {
        let s = machined_surface_scan_sample();
        let rows = RowStateManager::new(s.table.n_rows());
        let SavedReport::ImageAnalysis { request } = &s.reports[0] else { panic!() };
        let a = ImageAnalysis::run(&s.table, &rows, request).unwrap();
        let peaks = table_of(&a, SectionKind::ImageDominantFrequencies);
        let periods: Vec<f64> = peaks.rows.iter().take(2).map(|r| r[4].parse().unwrap()).collect();
        let directions: Vec<f64> = peaks.rows.iter().take(2).map(|r| r[5].parse().unwrap()).collect();
        // 16 µm marks at 20° (y down, so −20° on screen ≡ 160°) and 24 µm marks at 110° (≡ 70°).
        assert!((periods[0] - 16.0).abs() < 0.5, "{periods:?}");
        assert!((periods[1] - 24.0).abs() < 1.0, "{periods:?}");
        assert!((directions[0] - 160.0).abs() < 3.0, "{directions:?}");
        assert!((directions[1] - 70.0).abs() < 3.0, "{directions:?}");
    }

    #[test]
    fn notching_the_tool_marks_reveals_the_pit() {
        let s = machined_surface_scan_sample();
        let rows = RowStateManager::new(s.table.n_rows());
        let SavedReport::ImageAnalysis { request } = &s.reports[1] else { panic!() };
        let a = ImageAnalysis::run(&s.table, &rows, request).unwrap();
        let filtered = a.saveable_columns().into_iter().find(|c| c.key == "filtered").unwrap().values;
        // The lowest point of the filtered surface is in the pit at (340, 180) µm.
        let (k, _) = filtered.iter().enumerate().min_by(|a, b| a.1.total_cmp(b.1)).unwrap();
        let (x, y) = ((k % SCAN) as f64 * 2.0, (k / SCAN) as f64 * 2.0);
        assert!((x - 340.0).hypot(y - 180.0) < 12.0, "lowest point at ({x}, {y})");
    }

    #[test]
    fn scene_registration_recovers_the_shift() {
        let s = blurred_shifted_scene_sample();
        let rows = RowStateManager::new(s.table.n_rows());
        let SavedReport::ImageAnalysis { request } = &s.reports[1] else { panic!() };
        let a = ImageAnalysis::run(&s.table, &rows, request).unwrap();
        let t = table_of(&a, SectionKind::ImageShift);
        let across: f64 = t.rows[0][0].parse().unwrap();
        let down: f64 = t.rows[0][1].parse().unwrap();
        // Shifting "Shifted" back by (−5.3, +3.6) aligns it with "Sharp".
        assert!((across + 5.3).abs() < 0.1, "{across}");
        assert!((down - 3.6).abs() < 0.1, "{down}");
    }

    #[test]
    fn deblurring_brings_the_scene_closer_to_sharp() {
        let s = blurred_shifted_scene_sample();
        let rows = RowStateManager::new(s.table.n_rows());
        let SavedReport::ImageAnalysis { request } = &s.reports[0] else { panic!() };
        let a = ImageAnalysis::run(&s.table, &rows, request).unwrap();
        let restored = a.saveable_columns().into_iter().find(|c| c.key == "restored").unwrap().values;
        let sharp = s.table.column(col(&s.table, "Sharp")).unwrap().as_numeric().unwrap().to_vec();
        let blurred = s.table.column(col(&s.table, "Blurred")).unwrap().as_numeric().unwrap().to_vec();
        // Away from the border (where the circular restoration meets the edges).
        let inner = |k: &usize| (10..SCENE - 10).contains(&(k % SCENE)) && (10..SCENE - 10).contains(&(k / SCENE));
        let rms = |a: &[f64]| {
            let d: Vec<f64> = (0..a.len()).filter(inner).map(|k| a[k] - sharp[k]).collect();
            (d.iter().map(|v| v * v).sum::<f64>() / d.len() as f64).sqrt()
        };
        assert!(rms(&restored) < 0.85 * rms(&blurred), "{} vs {}", rms(&restored), rms(&blurred));
    }
}
