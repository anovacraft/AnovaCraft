use super::*;
use stat_data::Column;

/// A `cols × rows` image of `f(x, y)` in long format with columns x, y, v.
fn image(cols: usize, rows: usize, f: impl Fn(f64, f64) -> f64) -> DataTable {
    let (mut x, mut y, mut v) = (Vec::new(), Vec::new(), Vec::new());
    for r in 0..rows {
        for c in 0..cols {
            x.push(c as f64 * 0.5);
            y.push(r as f64 * 0.5);
            v.push(f(c as f64 * 0.5, r as f64 * 0.5));
        }
    }
    from_columns(x, y, v)
}

fn from_columns(x: Vec<f64>, y: Vec<f64>, v: Vec<f64>) -> DataTable {
    DataTable::new("img", vec![Column::numeric("x", x), Column::numeric("y", y), Column::numeric("v", v)]).unwrap()
}

/// The x, y and v columns of `t`.
fn parts(t: &DataTable) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
    let c = |n: &str| t.column(t.find(n).unwrap()).unwrap().as_numeric().unwrap().to_vec();
    (c("x"), c("y"), c("v"))
}

fn input(t: &DataTable) -> ImageInput {
    ImageInput::guess(t, t.find("v").unwrap()).unwrap()
}

#[test]
fn pixels_are_read_onto_their_lattice_in_any_row_order() {
    // The rows of the table reversed: the image must not change.
    let (mut x, mut y, mut v) = parts(&image(6, 4, |x, y| x + 10.0 * y));
    x.reverse();
    y.reverse();
    v.reverse();
    let t = from_columns(x, y, v);
    let n = t.n_rows();
    let im = Images::extract(&t, &RowStateManager::new(n), &input(&t), 2).unwrap();
    let g = &im.images[0].grid;
    assert_eq!((g.rows, g.cols, g.dx, g.dy), (4, 6, 0.5, 0.5));
    assert_eq!(g.get(1, 2), 1.0 + 5.0);
    assert_eq!(im.images[0].source_row[0], Some(n - 1));
}

#[test]
fn excluded_pixels_are_filled_with_the_mean_and_counted() {
    let t = image(4, 4, |x, _| x);
    let mut rows = RowStateManager::new(t.n_rows());
    rows.set_excluded([0], true);
    let im = Images::extract(&t, &rows, &input(&t), 2).unwrap();
    assert_eq!(im.images[0].holes, 1);
    assert_eq!(im.images[0].source_row[0], None);
    let mean = (0.0 * 3.0 + 0.5 * 4.0 + 1.0 * 4.0 + 1.5 * 4.0) / 15.0;
    assert!((im.images[0].grid.get(0, 0) - mean).abs() < 1e-12);
}

#[test]
fn uneven_or_repeated_pixels_are_errors() {
    let (mut x, y, v) = parts(&image(4, 4, |x, _| x));
    x[1] = 0.7;
    let t = from_columns(x.clone(), y.clone(), v.clone());
    assert!(Images::extract(&t, &RowStateManager::new(16), &input(&t), 2).is_err());
    x[1] = 0.0;
    let t = from_columns(x, y, v);
    let e = Images::extract(&t, &RowStateManager::new(16), &input(&t), 2).unwrap_err().to_string();
    assert!(e.contains("same pixel"), "{e}");
}

#[test]
fn requests_round_trip_through_json() {
    let t = image(8, 8, |x, y| (x * y).sin());
    let v = t.find("v").unwrap();
    let base = input(&t);
    let requests = [
        ImageAnalysisRequest {
            input: base,
            method: ImageMethod::Spectrum(ImageSpectrumSettings { phase: true, taper: TaperChoice::RadialHann, ..Default::default() }),
        },
        ImageAnalysisRequest {
            input: base,
            method: ImageMethod::Spectrum(ImageSpectrumSettings {
                profile_directions: vec![30.0, 120.5],
                profile_half_width: 5.0,
                profile_peaks: true,
                ..Default::default()
            }),
        },
        ImageAnalysisRequest {
            input: base,
            method: ImageMethod::Filter(ImageFilterSettings {
                band: BandChoice::Bandreject,
                low: Some(0.2),
                high: Some(0.4),
                notches: vec![NotchPoint { fx: 0.1, fy: -0.2 }],
                notch_radius: Some(0.05),
                ..Default::default()
            }),
        },
        ImageAnalysisRequest {
            input: base,
            method: ImageMethod::Restore(ImageRestoreSettings {
                operation: RestoreOperation::Register { other: v, upsample: 10 },
            }),
        },
        ImageAnalysisRequest {
            input: base,
            method: ImageMethod::Restore(ImageRestoreSettings {
                operation: RestoreOperation::Deblur { method: DeblurMethod::RichardsonLucy { iterations: 12 }, sigma_x: 1.5, sigma_y: 1.0 },
            }),
        },
    ];
    for r in requests {
        let json = r.to_json(&t);
        let key = json.get("analysis").and_then(JsonValue::as_str).unwrap().to_owned();
        assert_eq!(ImageAnalysisRequest::from_json(&key, &json, &t).unwrap(), r);
    }
}

#[test]
fn every_analysis_runs_and_filters_save_one_value_per_used_row() {
    let t = image(40, 36, |x, y| 1.0 + (2.0 * std::f64::consts::PI * x / 2.0).cos() + 0.1 * y);
    let rows = RowStateManager::new(t.n_rows());
    let methods = [
        ImageMethod::Spectrum(ImageSpectrumSettings::default()),
        ImageMethod::Filter(ImageFilterSettings::default()),
        ImageMethod::Filter(ImageFilterSettings::auto_notch(1)),
        ImageMethod::Restore(ImageRestoreSettings::default()),
        ImageMethod::Restore(ImageRestoreSettings { operation: RestoreOperation::Autocorrelation { remove_mean: true } }),
    ];
    for method in methods {
        let a = ImageAnalysis::run(&t, &rows, &ImageAnalysisRequest { input: input(&t), method }).unwrap();
        assert!(!a.sections().is_empty());
        for c in a.saveable_columns() {
            assert_eq!(c.values.len(), t.n_rows());
            assert!(c.values.iter().all(|v| v.is_finite()));
        }
    }
}

#[test]
fn notching_a_pattern_removes_it() {
    // A cosine of period 2 (x units) across: frequency 0.5 per unit, on a bin.
    let t = image(32, 32, |x, _| 5.0 + (2.0 * std::f64::consts::PI * x / 2.0).cos());
    let rows = RowStateManager::new(t.n_rows());
    let settings = ImageFilterSettings {
        band: BandChoice::None,
        notches: vec![NotchPoint { fx: 0.5, fy: 0.0 }],
        notch_radius: Some(0.05),
        padding: PaddingChoice::None,
        ..Default::default()
    };
    let a = ImageAnalysis::run(&t, &rows, &ImageAnalysisRequest { input: input(&t), method: ImageMethod::Filter(settings) }).unwrap();
    let filtered = &a.saveable_columns()[0].values;
    assert!(filtered.iter().all(|v| (v - 5.0).abs() < 1e-6), "{:?}", &filtered[..4]);
}

#[test]
fn radial_profiles_cut_through_the_pattern_along_its_direction_only() {
    // Stripes varying across x with period 2: frequency 0.5 per unit along 0°.
    let t = image(32, 32, |x, _| (2.0 * std::f64::consts::PI * x / 2.0).cos());
    let rows = RowStateManager::new(t.n_rows());
    let settings = ImageSpectrumSettings { taper: TaperChoice::None, ..Default::default() };
    let a = ImageAnalysis::run(&t, &rows, &ImageAnalysisRequest { input: input(&t), method: ImageMethod::Spectrum(settings) })
        .unwrap();
    let sections = a.sections();
    assert!(sections.iter().any(|s| s.kind == SectionKind::ImageRadialProfile));
    let table = sections
        .iter()
        .find_map(|s| match (&s.kind, &s.content) {
            (SectionKind::ImageRadialProfileValues, SectionContent::Table(t)) => Some(t),
            _ => None,
        })
        .unwrap();
    assert_eq!(table.columns, ["Direction (°)", "Frequency", "Period", "Power"]);
    let value = |row: &Vec<String>, i: usize| row[i].parse::<f64>().unwrap();
    // Each direction runs from 0 to the Nyquist frequency 1 in 17 steps of 1/16.
    assert_eq!(table.rows.len(), 34);
    let strongest = |dir: f64| {
        table
            .rows
            .iter()
            .filter(|r| value(r, 0) == dir)
            .max_by(|a, b| value(a, 3).total_cmp(&value(b, 3)))
            .unwrap()
            .clone()
    };
    let across = strongest(0.0);
    assert!((value(&across, 1) - 0.5).abs() < 1e-9 && (value(&across, 2) - 2.0).abs() < 1e-9);
    // Power a²/4 = 0.25 at each of the pair's cells, none along 90°.
    assert!((value(&across, 3) - 0.25).abs() < 1e-6);
    assert!(value(&strongest(90.0), 3) < 1e-12);
}

#[test]
fn radial_profiles_need_a_direction() {
    let t = image(16, 16, |x, y| x * y);
    let rows = RowStateManager::new(t.n_rows());
    let settings = ImageSpectrumSettings { profile_directions: Vec::new(), ..Default::default() };
    let request = ImageAnalysisRequest { input: input(&t), method: ImageMethod::Spectrum(settings) };
    assert!(ImageAnalysis::run(&t, &rows, &request).is_err());
}
