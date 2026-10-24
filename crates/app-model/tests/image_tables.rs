//! Image files read as long tables, checked against Pillow's decoding of
//! the same files (`tests/data/images/make_image_fixtures.py` writes the
//! fixtures and `image_reference.json`).

use std::path::PathBuf;

use app_model::image_table::{ChannelChoice, DecodedImage, ImageOpenOptions, SizeChoice, image_table};

fn images() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/images")
}

#[test]
fn every_fixture_matches_the_reference_decoding() {
    let text = std::fs::read_to_string(images().join("image_reference.json")).unwrap();
    let reference = stat_data::parse_json(&text).unwrap();
    let files = reference.as_object().unwrap();
    assert_eq!(files.len(), 10);
    for (file, expected) in files {
        let image = DecodedImage::open(&images().join(file)).unwrap_or_else(|e| panic!("{file}: {e}"));
        let field = |k: &str| expected.get(k).unwrap();
        let (w, h) = (field("width").as_usize().unwrap(), field("height").as_usize().unwrap());
        assert_eq!(image.size(), (w, h), "{file}");
        let lossy = field("lossy").as_bool().unwrap();
        // Baseline JPEG decoders may differ by a level or two in the inverse
        // DCT and colour conversion; lossless formats must agree exactly.
        let tolerance = if lossy { 2.0 } else { 1e-9 };
        let channels = field("channels").as_object().unwrap();
        let options = ImageOpenOptions {
            channels: if channels.iter().any(|(k, _)| k == "red") { ChannelChoice::Rgb } else { ChannelChoice::Grey },
            size: SizeChoice::Reduce,
            pixel_size: 1.0,
        };
        let (table, info) = image_table(&image, "t", file, &options).unwrap();
        assert_eq!(table.n_rows(), w * h, "{file}");
        assert_eq!(info.reduction, 1);
        let column = |name: &str| table.find(name).and_then(|i| table.column(i)).and_then(|c| c.as_numeric()).unwrap().to_vec();
        let (x, y) = (column("x"), column("y"));
        for (i, (&xi, &yi)) in x.iter().zip(&y).enumerate() {
            assert_eq!((xi, yi), ((i % w) as f64, (i / w) as f64), "{file}: pixel order is row by row from the top");
        }
        for (name, values) in channels {
            let want: Vec<f64> = values.as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
            let got = column(name);
            let tol = if name == "luminance" && !lossy { 1e-9 } else { tolerance };
            for (i, (g, e)) in got.iter().zip(&want).enumerate() {
                assert!((g - e).abs() <= tol, "{file} {name}[{i}]: {g} vs {e}");
            }
        }
    }
}

#[test]
fn colour_images_open_grey_as_luminance() {
    let image = DecodedImage::open(&images().join("rgb8.png")).unwrap();
    let grey = image_table(&image, "t", "rgb8.png", &ImageOpenOptions::default()).unwrap().0;
    let rgb = ImageOpenOptions { channels: ChannelChoice::Rgb, ..ImageOpenOptions::default() };
    let colour = image_table(&image, "t", "rgb8.png", &rgb).unwrap().0;
    let column = |t: &stat_data::DataTable, name: &str| t.find(name).and_then(|i| t.column(i)).and_then(|c| c.as_numeric()).unwrap().to_vec();
    assert_eq!(grey.n_cols(), 3);
    assert_eq!(column(&grey, "intensity"), column(&colour, "luminance"));
}

#[test]
fn files_open_images_with_the_default_choices() {
    let opened = app_model::files::open_path(&images().join("gray16.tif")).unwrap();
    let [app_model::file_formats::Opened::Table(table, notes)] = opened.as_slice() else { panic!("one table") };
    assert_eq!((table.n_rows(), table.n_cols()), (13 * 9, 3));
    assert!(notes[0].contains("gray16.tif"), "{notes:?}");
}
