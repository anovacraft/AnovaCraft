use super::*;

/// A grey 16-bit image whose pixel `(x, y)` is `f(x, y)`.
fn grey16(w: u32, h: u32, f: impl Fn(u32, u32) -> u16) -> DecodedImage {
    let buffer = image::ImageBuffer::from_fn(w, h, |x, y| image::Luma([f(x, y)]));
    DecodedImage { image: DynamicImage::ImageLuma16(buffer), format: "PNG".into() }
}

fn rgb8(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 3]) -> DecodedImage {
    let buffer = image::ImageBuffer::from_fn(w, h, |x, y| image::Rgb(f(x, y)));
    DecodedImage { image: DynamicImage::ImageRgb8(buffer), format: "PNG".into() }
}

fn column(t: &DataTable, name: &str) -> Vec<f64> {
    t.column(t.find(name).unwrap()).unwrap().as_numeric().unwrap().to_vec()
}

#[test]
fn small_images_open_pixel_for_pixel_row_by_row_from_the_top() {
    let img = grey16(3, 2, |x, y| (100 * y + x) as u16);
    let (t, info) = image_table(&img, "scan", "scan.png", &ImageOpenOptions::default()).unwrap();
    assert_eq!(t.n_rows(), 6);
    assert_eq!(column(&t, "x"), [0.0, 1.0, 2.0, 0.0, 1.0, 2.0]);
    assert_eq!(column(&t, "y"), [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
    assert_eq!(column(&t, "intensity"), [0.0, 1.0, 2.0, 100.0, 101.0, 102.0]);
    assert_eq!((info.grid, info.reduction, info.crop), ((3, 2), 1, None));
    let note = info.note();
    assert!(note.starts_with("Image scan.png: 3 × 2 pixels, 16-bit grey."), "{note}");
    assert!(!note.contains("reduced"));
    // The pixel size scales the coordinates.
    let options = ImageOpenOptions { pixel_size: 0.5, ..ImageOpenOptions::default() };
    let (t, info) = image_table(&img, "scan", "scan.png", &options).unwrap();
    assert_eq!(column(&t, "x")[2], 1.0);
    assert!(info.note().contains("pixel size 0.5"), "{}", info.note());
    let bad = ImageOpenOptions { pixel_size: 0.0, ..ImageOpenOptions::default() };
    assert!(image_table(&img, "scan", "scan.png", &bad).is_err());
}

#[test]
fn colour_images_give_channels_and_rec709_luminance_or_grey() {
    let img = rgb8(2, 1, |x, _| if x == 0 { [255, 0, 0] } else { [10, 20, 30] });
    let rgb = ImageOpenOptions { channels: ChannelChoice::Rgb, ..ImageOpenOptions::default() };
    let (t, info) = image_table(&img, "c", "c.png", &rgb).unwrap();
    let names: Vec<&str> = (0..t.n_cols()).map(|i| t.column(i).unwrap().name.as_str()).collect();
    assert_eq!(names, ["x", "y", "red", "green", "blue", "luminance"]);
    assert_eq!(column(&t, "red"), [255.0, 10.0]);
    let lum = column(&t, "luminance");
    assert!((lum[0] - 0.2126 * 255.0).abs() < 1e-12);
    assert!((lum[1] - (0.2126 * 10.0 + 0.7152 * 20.0 + 0.0722 * 30.0)).abs() < 1e-12);
    assert_eq!(info.pixels, "8-bit RGB");
    // Read as grey: one intensity column holding the luminance.
    let (g, _) = image_table(&img, "c", "c.png", &ImageOpenOptions::default()).unwrap();
    assert_eq!(g.n_cols(), 3);
    assert_eq!(column(&g, "intensity"), lum);
}

#[test]
fn large_images_reduce_by_block_means_keeping_original_units() {
    // 2050 × 1100: k = ⌈2050 / 1024⌉ = 3, so 683 × 366 cells, 1 column and
    // 2 rows left out.
    let f = |x: u32, y: u32| ((x * 7 + y * 3) % 1000) as u16;
    let img = grey16(2050, 1100, f);
    assert_eq!(reduction_factor(2050, 1100), 3);
    let plan = img.plan(SizeChoice::Reduce);
    assert_eq!((plan.grid, plan.block, plan.left_out), ((683, 366), 3, (1, 2)));
    let options = ImageOpenOptions { pixel_size: 2.0, ..ImageOpenOptions::default() };
    let (t, info) = image_table(&img, "big", "big.png", &options).unwrap();
    assert_eq!(t.n_rows(), 683 * 366);
    let (x, y, v) = (column(&t, "x"), column(&t, "y"), column(&t, "intensity"));
    for (r, c) in [(0usize, 0usize), (5, 17), (365, 682), (200, 400)] {
        let k = r * 683 + c;
        let mean = (0..3)
            .flat_map(|dy| (0..3).map(move |dx| (dx, dy)))
            .map(|(dx, dy)| f64::from(f((3 * c + dx) as u32, (3 * r + dy) as u32)))
            .sum::<f64>()
            / 9.0;
        assert!((v[k] - mean).abs() < 1e-9, "cell {r},{c}");
        // Block centres in original pixel units, times the pixel size.
        assert_eq!(x[k], (3 * c + 1) as f64 * 2.0);
        assert_eq!(y[k], (3 * r + 1) as f64 * 2.0);
    }
    let note = info.note();
    assert!(note.contains("reduced by averaging blocks of 3 × 3 pixels to 683 × 366"), "{note}");
    assert!(note.contains("1 columns at the right edge and 2 rows at the bottom left out"), "{note}");
}

#[test]
fn crops_keep_full_resolution_and_original_coordinates_and_full_size_is_capped() {
    let img = grey16(1500, 1200, |x, y| (x + y) as u16);
    let crop = SizeChoice::Crop { left: 100, top: 1190, width: 50, height: 40 };
    // The crop is cut back to the image: rows 1190–1199 only.
    let (t, info) = image_table(&img, "c", "c.png", &ImageOpenOptions { size: crop, ..ImageOpenOptions::default() }).unwrap();
    assert_eq!(t.n_rows(), 50 * 10);
    assert_eq!(column(&t, "x")[0], 100.0);
    assert_eq!(column(&t, "y")[0], 1190.0);
    assert_eq!(column(&t, "intensity")[0], 1290.0);
    assert_eq!(info.crop, Some([100, 1190, 50, 10]));
    assert!(info.note().contains("cropped to columns 100–149 and rows 1190–1199 (50 × 10 pixels)"), "{}", info.note());
    assert_eq!(centred_crop(1500, 1200), SizeChoice::Crop { left: 238, top: 88, width: 1024, height: 1024 });
    let full = ImageOpenOptions { size: SizeChoice::Full, ..ImageOpenOptions::default() };
    let (t, info) = image_table(&img, "c", "c.png", &full).unwrap();
    assert_eq!(t.n_rows(), 1500 * 1200);
    assert!(info.note().contains("kept at full size"));
    assert!(full_size_allowed(4096, 4096) && !full_size_allowed(4097, 10));
    let huge = grey16(4100, 2, |_, _| 0);
    assert!(image_table(&huge, "h", "h.png", &full).is_err());
    assert_eq!(table_bytes(1_000_000, 3), 32_000_000);
}

#[test]
fn grids_rebuild_from_long_tables_with_blanks_and_reject_bad_layouts() {
    // A 3 × 2 grid with spacing 0.5, rows shuffled and one cell missing.
    let x = [1.0, 0.0, 0.5, 1.0, 0.0];
    let y = [2.0, 2.0, 2.0, 3.0, 3.0];
    let v = [3.0, 1.0, 2.0, 6.0, 4.0];
    let g = PixelGrid::from_columns(&x, &y, &v, &|_| true).unwrap();
    assert_eq!((g.cols, g.rows, g.x, g.y), (3, 2, (0.0, 0.5), (2.0, 1.0)));
    assert_eq!(g.values[..3], [1.0, 2.0, 3.0]);
    assert!(g.values[4].is_nan());
    assert_eq!(g.row_index[..3], [1, 2, 0]);
    assert_eq!(g.filled(), 5);
    // Rows left out by the filter are blank.
    let g2 = PixelGrid::from_columns(&x, &y, &v, &|r| r != 0).unwrap();
    assert!(g2.values[2].is_nan());
    // As an image plot either way up.
    let down = g.raster(true);
    assert_eq!((down.x_range, down.y_range), ((-0.25, 1.25), (1.5, 3.5)));
    assert_eq!(down.values[0], 1.0);
    let up = g.raster(false);
    assert_eq!(up.values[0], 4.0, "greatest y on top");
    assert_eq!(up.row_index.as_ref().unwrap()[0], 4);
    // Off the grid, and two rows in one cell.
    assert!(PixelGrid::from_columns(&[0.0, 1.0, 1.5], &[0.0; 3], &[1.0; 3], &|_| true).is_ok());
    assert!(PixelGrid::from_columns(&[0.0, 1.0, 1.7], &[0.0; 3], &[1.0; 3], &|_| true).is_err());
    let same = PixelGrid::from_columns(&[0.0, 0.0], &[1.0, 1.0], &[1.0, 2.0], &|_| true).unwrap_err();
    assert!(same.to_string().contains("rows 1 and 2"), "{same}");
    assert!(PixelGrid::from_columns(&[f64::NAN], &[0.0], &[1.0], &|_| true).is_err());
}

#[test]
fn images_written_from_grids_read_back_with_linear_levels() {
    let x: Vec<f64> = (0..12).map(|k| f64::from(k % 4)).collect();
    let y: Vec<f64> = (0..12).map(|k| f64::from(k / 4)).collect();
    let v: Vec<f64> = (0..12).map(|k| f64::from(k) * 10.0).collect();
    let mut v_gap = v.clone();
    v_gap[5] = f64::NAN;
    let g = PixelGrid::from_columns(&x, &y, &v_gap, &|_| true).unwrap();
    for format in ImageFileFormat::ALL {
        let bytes = encode_image(&g, (0.0, 110.0), format).unwrap();
        let back = DecodedImage::decode(&bytes).unwrap();
        assert_eq!(back.size(), (4, 3));
        let (t, _) = image_table(&back, "b", "b", &ImageOpenOptions::default()).unwrap();
        let levels = format.levels();
        let got = column(&t, "intensity");
        for k in 0..12 {
            let want = if k == 5 { 0.0 } else { (v[k] / 110.0 * levels).round() };
            assert_eq!(got[k], want, "{format:?} pixel {k}");
        }
        assert_eq!(ImageFileFormat::from_key(format.key()), Some(format));
    }
    assert_eq!(grey_level(-5.0, 0.0, 10.0, 255.0), 0.0);
    assert_eq!(grey_level(15.0, 0.0, 10.0, 255.0), 255.0);
    assert_eq!(grey_level(5.0, 0.0, 10.0, 255.0), 128.0);
    assert_eq!(grey_level(5.0, 5.0, 5.0, 255.0), 255.0);
    assert_eq!(value_range(&[f64::NAN, 3.0, -1.0]), (-1.0, 3.0));
}

#[test]
fn columns_are_guessed_by_name() {
    let t = DataTable::new(
        "t",
        vec![
            Column::numeric("y", vec![0.0]),
            Column::numeric("red", vec![0.0]),
            Column::numeric("x", vec![0.0]),
            Column::numeric("luminance", vec![0.0]),
        ],
    )
    .unwrap();
    assert_eq!(guess_columns(&t), (Some(2), Some(0), Some(3)));
    assert!(is_image_path(std::path::Path::new("a/b.TIFF")));
    assert!(!is_image_path(std::path::Path::new("a/b.csv")));
}

#[test]
fn files_that_are_not_images_are_refused() {
    assert!(DecodedImage::decode(b"x,y\n1,2\n").is_err());
    assert!(DecodedImage::decode(&[0x89, b'P', b'N', b'G', 0, 0]).is_err());
}
