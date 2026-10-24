//! Images as tables: opening an image file (the Open image dialog's Reduce,
//! Crop and Keep full size for large images), Visualize ▸ Image plot… with
//! box selection of pixel rows, and File ▸ Export data as ▸ Image….

use super::*;
use app_model::image_table::{DecodedImage, ImageFileFormat, PixelGrid, encode_image};

/// Write a grey PNG of `width` × `height` pixels with `level(x, y)` as its
/// grey levels (0–255; the corners are 0 and 255 so the data range is the
/// full scale).
fn grey_png(path: &std::path::Path, width: usize, height: usize, level: impl Fn(usize, usize) -> f64) {
    let mut values: Vec<f64> = (0..width * height).map(|i| level(i % width, i / width)).collect();
    values[0] = 0.0;
    values[width * height - 1] = 255.0;
    let grid = PixelGrid {
        cols: width,
        rows: height,
        x: (0.0, 1.0),
        y: (0.0, 1.0),
        values,
        row_index: vec![stat_viz::image_plot::NO_ROW; width * height],
    };
    std::fs::write(path, encode_image(&grid, (0.0, 255.0), ImageFileFormat::Png8).unwrap()).unwrap();
}

/// The numbers in column `name` of the active table.
fn column(gui: &Gui, name: &str) -> Vec<f64> {
    let table = &gui.app().documents[gui.app().active].table;
    let i = table.find(name).unwrap_or_else(|| panic!("no column {name}"));
    table.column(i).and_then(|c| c.as_numeric()).expect("numbers").to_vec()
}

/// The project notes of the active table.
fn project_notes(gui: &Gui) -> String {
    let project = gui.app().documents[gui.app().active].project;
    gui.app().project(project).expect("the table's project").notes.clone()
}

const LARGE: &str = "Open image large.png";

/// A 1300 × 700 image is larger than 1024 pixels across, so the dialog
/// offers Reduce (blocks of 2 × 2, x and y in original pixel units), Crop
/// (full resolution, original coordinates) and Keep full size; each choice
/// opens the table it describes and the note records it.
#[test]
fn large_image_opens_reduced_cropped_or_full_size() {
    let dir = temp_dir("open-large-image");
    let path = dir.join("large.png");
    grey_png(&path, 1300, 700, |x, y| ((x + 2 * y) % 251) as f64);
    let open = |choice: Option<&str>| {
        let mut gui = Gui::start();
        gui.app_mut().settings.open_plain_directly = true;
        gui.perform(Action::OpenPaths(vec![path.clone()]));
        assert!(gui.app().documents.is_empty(), "a large image always asks");
        assert!(gui.harness.query_by_role_and_label(Role::Window, LARGE).is_some());
        assert!(gui.shows("Reduce: average blocks of 2 × 2 pixels (650 × 350)"));
        if let Some(choice) = choice {
            gui.click(choice);
        }
        gui
    };

    // Reduce, the default.
    let mut gui = open(None);
    gui.shot("open-image-large");
    gui.click("Open");
    assert!(gui.harness.query_by_role_and_label(Role::Window, LARGE).is_none(), "the dialog closes");
    let x = column(&gui, "x");
    let y = column(&gui, "y");
    assert_eq!(x.len(), 650 * 350);
    assert_eq!((x[0], y[0], x[1], y[650]), (0.5, 0.5, 2.5, 2.5), "block centres in original pixel units");
    let notes = project_notes(&gui);
    assert!(notes.contains("reduced by averaging blocks of 2 × 2 pixels to 650 × 350"), "{notes}");

    // Crop: the centred 1024 × 700 region at full resolution.
    let mut gui = open(Some("Crop: a region at full resolution (drag it in the preview)"));
    gui.click("Open");
    let x = column(&gui, "x");
    assert_eq!(x.len(), 1024 * 700);
    assert_eq!(x[0], 138.0, "original column numbers");
    let notes = project_notes(&gui);
    assert!(notes.contains("cropped to columns 138–1161 and rows 0–699"), "{notes}");

    // Keep full size.
    let mut gui = open(Some("Keep full size"));
    gui.click("Open");
    assert_eq!(column(&gui, "intensity").len(), 1300 * 700);
    assert!(project_notes(&gui).contains("kept at full size"));
}

/// A small grey image opens at once (Settings' "open plain files
/// directly"); Visualize ▸ Image plot… guesses x, y and intensity and draws
/// the picture; a box dragged over its left part selects those pixels' rows.
#[test]
fn image_plot_draws_pixels_and_box_selection_selects_their_rows() {
    let dir = temp_dir("image-plot");
    let path = dir.join("ramp.png");
    grey_png(&path, 20, 10, |x, y| (x * 12 + y) as f64);
    let mut gui = Gui::start();
    gui.app_mut().settings.open_plain_directly = true;
    gui.perform(Action::OpenPaths(vec![path]));
    assert_eq!(gui.app().documents.len(), 1, "a small grey image opens without the dialog");
    assert_eq!(column(&gui, "intensity").len(), 200);
    assert!(project_notes(&gui).contains("Image ramp.png: 20 × 10 pixels"));

    const TITLE: &str = "Image plot";
    gui.menu(&["Visualize", "Image plot…"]);
    assert!(gui.harness.query_by_role_and_label(Role::Window, TITLE).is_some());
    gui.click("Run");
    let active = gui.app().active;
    let reports = &gui.app().documents[active].reports;
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].title(), "Image Plot: intensity");
    let app_model::SavedReport::ImagePlot { request } = reports[0].saved() else { panic!("not an image plot") };
    assert_eq!((request.x, request.y, request.value), (0, 1, 2));
    assert!(request.y_down);

    gui.harness.set_size(egui::vec2(1400.0, 1400.0));
    gui.results();
    assert!(gui.shows("intensity over x and y"));
    gui.shot("image-plot-report");
    let b = gui.widget("Horizontal axis ruler").raw_bounds().expect("the ruler");
    let v = gui.widget("Vertical axis ruler").raw_bounds().expect("the ruler");
    let (left, right, top, bottom) = (b.x0 as f32, b.x1 as f32, v.y0 as f32, v.y1 as f32);
    gui.drag(
        egui::pos2(left + 0.02 * (right - left), top + 0.02 * (bottom - top)),
        egui::pos2(left + 0.4 * (right - left), bottom - 0.02 * (bottom - top)),
    );
    let selected = gui.app().documents[active].rows.selected_rows();
    let x = column(&gui, "x");
    assert!(selected.len() >= 10, "the box covers pixels: {selected:?}");
    assert!(selected.iter().all(|&r| x[r] < 10.0), "only pixels of the left part: {selected:?}");
    // Every row of the image is in the box, so each selected column is whole.
    for &r in &selected {
        let whole = (0..200).filter(|&s| x[s] == x[r]).all(|s| selected.contains(&s));
        assert!(whole, "column x = {} is selected from top to bottom", x[r]);
    }
    gui.shot("image-plot-selection");
}

/// File ▸ Export data as ▸ Image… writes the intensity column back as a
/// grey PNG that decodes to the same levels, and as a 16-bit TIFF.
#[test]
fn export_image_writes_the_value_column_back() {
    let dir = temp_dir("export-image");
    let source = dir.join("ramp.png");
    grey_png(&source, 12, 7, |x, y| (x * 20 + y * 3) as f64);
    let mut gui = Gui::start();
    gui.app_mut().settings.open_plain_directly = true;
    gui.perform(Action::OpenPaths(vec![source.clone()]));
    let original = column(&gui, "intensity");
    let active = gui.app().active;
    gui.perform(Action::ExportImage(active));
    assert!(gui.harness.query_by_role_and_label(Role::Window, "Export image").is_some());
    assert!(gui.shows("Write a 12 × 7 image of intensity"));
    gui.shot("export-image");
    let target = dir.join("written.png");
    crate::file_io::test_save_answer::set(Some(target.clone()));
    gui.click("Export…");
    assert!(gui.harness.query_by_role_and_label(Role::Window, "Export image").is_none(), "the dialog closes");
    let image = DecodedImage::open(&target).expect("a PNG");
    assert_eq!(image.size(), (12, 7));
    let back = app_model::image_table::image_table(&image, "back", "written.png", &Default::default()).unwrap().0;
    let i = back.find("intensity").unwrap();
    assert_eq!(back.column(i).unwrap().as_numeric().unwrap(), original.as_slice());

    gui.perform(Action::ExportImage(active));
    gui.choose("Export image", 3, ImageFileFormat::Tiff16.label());
    let tiff = dir.join("written.tif");
    crate::file_io::test_save_answer::set(Some(tiff.clone()));
    gui.click("Export…");
    let image = DecodedImage::open(&tiff).expect("a TIFF");
    assert!(image.describe().contains("16"), "{}", image.describe());
}
