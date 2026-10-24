//! A plot's figure output from its right-click menu: Copy plot, the Save
//! plot dialog (size on paper, font, text size), Transparent background and
//! Plot font, kept with the plot.

use super::figure_page::{right_click, scroll_into_view};
use super::*;
use crate::plot_clipboard::{Entry, test_clipboard};
use app_model::plot_style::{ExportLook, PlotFont};

const PLOT: &str = "Residual Normal Quantile Plot";

/// Longley's reports in view with `PLOT`'s menu open.
fn plot_menu() -> Gui {
    let mut gui = Gui::with_sample("Longley");
    gui.results();
    scroll_into_view(&mut gui, PLOT);
    right_click(&mut gui, PLOT);
    gui
}

/// The figure looks kept by the active document's reports.
fn kept_looks(gui: &Gui) -> Vec<ExportLook> {
    let doc = &gui.app().documents[gui.app().active];
    doc.reports.iter().flat_map(|r| r.scope().plot_styles.iter().map(|(_, s)| s.export.clone())).collect()
}

/// The bytes copied under the format whose name contains `part`.
fn copied<'a>(entries: &'a [Entry], part: &str) -> &'a [u8] {
    entries
        .iter()
        .find_map(|e| match e {
            Entry::Named(name, bytes) if name.contains(part) => Some(bytes.as_slice()),
            Entry::Bitmap(bytes) if part == "bitmap" => Some(bytes.as_slice()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("nothing copied as {part}"))
}

/// Copy plot puts the plot on the clipboard as PDF, PNG and SVG at once
/// (in this platform's format names).
#[test]
fn copy_plot_offers_pdf_png_and_svg() {
    let mut gui = plot_menu();
    gui.press("Copy plot");
    let entries = test_clipboard::take();
    let formats: Vec<&str> = entries.iter().map(Entry::format).collect();
    let expected = crate::plot_clipboard::payload(crate::plot_clipboard::Platform::current(), vec![], vec![], vec![]);
    assert_eq!(formats, expected.iter().map(Entry::format).collect::<Vec<_>>());
    let svg = copied(&entries, "svg");
    assert!(svg.starts_with(b"<?xml") && String::from_utf8_lossy(svg).contains(PLOT));
    if crate::plot_clipboard::Platform::current() != crate::plot_clipboard::Platform::Windows {
        assert!(copied(&entries, "pdf").starts_with(b"%PDF"));
        assert!(copied(&entries, "png").starts_with(b"\x89PNG"));
    }
    // The outcome, shown under the report's options button.
    let doc = &gui.app().documents[gui.app().active];
    let said: Vec<String> = doc
        .reports
        .iter()
        .filter_map(|r| {
            let id = crate::report_view::figure_status_id(r.id());
            gui.harness.ctx.data(|d| d.get_temp::<(bool, String)>(id)).map(|s| s.1)
        })
        .collect();
    assert!(said.iter().any(|t| t.starts_with("Copied the plot as") && t.ends_with("SVG")), "{said:?}");
}

/// Save plot as… opens the dialog; a size on paper with a serif font is
/// written at that size, and the plot keeps the settings (so its next copy
/// and its saved report carry them).
#[test]
fn save_plot_dialog_writes_the_size_on_paper_and_keeps_it() {
    let mut gui = plot_menu();
    gui.press("Save plot as…");
    assert!(crate::plot_export::dialog_open(&gui.harness.ctx));
    gui.click("Set the size on paper");
    let width = gui.widget("Figure width").id();
    gui.set_number(width, 89.0);
    gui.click("Serif");
    let size = gui.widget("Text size").id();
    gui.set_number(size, 125.0);
    gui.shot("save-plot-dialog");
    let dir = temp_dir("save-plot");
    file_io::test_save_answer::set(Some(dir.join("plot")));
    gui.click("Save as PDF…");
    assert!(!file_io::test_save_answer::pending(), "the save dialog was asked");
    let pdf = String::from_utf8_lossy(&std::fs::read(dir.join("plot.pdf")).expect("written")).into_owned();
    assert!(pdf.contains("/MediaBox [0 0 252.28 "), "89 mm wide");
    assert!(pdf.contains("/BaseFont /Times-Roman"));
    assert!(!crate::plot_export::dialog_open(&gui.harness.ctx), "closed after saving");
    assert!(gui.shows(&format!("Saved {}", dir.join("plot.pdf").display())));

    let looks = kept_looks(&gui);
    assert_eq!(looks.len(), 1, "the plot keeps its look");
    let look = &looks[0];
    assert_eq!(look.font, PlotFont::Serif);
    assert_eq!(look.text_size, 1.25);
    let size = look.size.expect("a size on paper");
    assert_eq!(size.width, 89.0);
    assert!(size.height > 0.0 && size.height < 89.0, "the height followed the width");

    // Saved with the report and read back the same.
    let doc = &gui.app().documents[gui.app().active];
    let report = doc.reports.iter().find(|r| !r.scope().plot_styles.is_empty()).unwrap();
    let saved = report.saved();
    let json = app_model::project::report_to_json(&saved, &doc.table);
    let back = app_model::project::report_from_json(&json, &doc.table).unwrap();
    assert_eq!(back, saved);

    // The quick items use the kept look.
    right_click(&mut gui, PLOT);
    file_io::test_save_answer::set(Some(dir.join("again")));
    gui.press("Save plot as SVG…");
    let svg = std::fs::read_to_string(dir.join("again.svg")).unwrap();
    assert!(svg.contains("width=\"252.28pt\"") && svg.contains("Times New Roman"));
    let _ = std::fs::remove_dir_all(dir);
}

/// Transparent background and Plot font in the menu change the plot's
/// look: figures leave the background clear, and the plot's titles are
/// drawn in the serif font on screen.
#[test]
fn transparent_background_and_plot_font_items() {
    let mut gui = plot_menu();
    gui.press("Transparent background");
    assert!(kept_looks(&gui).first().is_some_and(|l| l.transparent));

    right_click(&mut gui, PLOT);
    gui.widget("Plot font").hover();
    gui.run();
    gui.click("Serif");
    assert_eq!(kept_looks(&gui)[0].font, PlotFont::Serif);
    // On screen: an axis title of the plot is set in the serif family.
    let serif = crate::theme::plot_serif();
    let fonts = painted_fonts(&gui);
    let titles: Vec<_> = fonts.iter().filter(|(_, f)| *f == serif).collect();
    assert!(!titles.is_empty(), "{:?}", fonts.iter().filter(|(t, _)| t.contains("Quantile")).collect::<Vec<_>>());

    right_click(&mut gui, PLOT);
    let dir = temp_dir("transparent-plot");
    file_io::test_save_answer::set(Some(dir.join("clear")));
    gui.press("Save plot as SVG…");
    let svg = std::fs::read_to_string(dir.join("clear.svg")).unwrap();
    assert!(!svg.contains("fill=\"#ffffff\"/>"), "no white page");
    assert!(svg.contains("Times New Roman"));
    let _ = std::fs::remove_dir_all(dir);
}

/// The font family of every piece of text painted in the last frame.
fn painted_fonts(gui: &Gui) -> Vec<(String, egui::FontFamily)> {
    fn collect(shape: &egui::Shape, out: &mut Vec<(String, egui::FontFamily)>) {
        match shape {
            egui::Shape::Text(t) => {
                for section in &t.galley.job.sections {
                    let text = t.galley.job.text[section.byte_range.clone()].to_owned();
                    out.push((text, section.format.font_id.family.clone()));
                }
            }
            egui::Shape::Vec(v) => v.iter().for_each(|s| collect(s, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for clipped in &gui.harness.output().shapes {
        collect(&clipped.shape, &mut out);
    }
    out
}

/// A graph canvas offers the same items in its options menu and keeps its
/// look in the graph (saved with its pipeline).
#[test]
fn graph_canvas_copies_and_keeps_its_look() {
    let mut gui = Gui::with_sample("Longley");
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    let spec = stat_viz::GraphSpec {
        encoding: stat_viz::EncodingPlan { x: vec!["GNP".into()], y: vec!["Employed".into()], ..Default::default() },
        points: Some(stat_viz::PointsElement::default()),
        ..stat_viz::GraphSpec::default()
    };
    gui.app_mut().add_report(ReportRequest::Graph(Box::new(spec)));
    gui.run();
    let open_options = |gui: &mut Gui| {
        let options = |n: &Node<'_>| n.role() == Role::Button && n.label().as_deref() == Some("⋯ Options");
        gui.harness.get_all_by(options).next().expect("the graph's report").simulate_click();
        gui.run();
    };
    open_options(&mut gui);
    gui.press("Copy plot");
    let entries = test_clipboard::take();
    assert!(String::from_utf8_lossy(copied(&entries, "svg")).contains("Employed"));

    open_options(&mut gui);
    gui.press("Transparent background");
    let app_model::SavedReport::Graph { spec } = gui.app().documents[active].reports[0].saved() else {
        panic!("a graph report")
    };
    let spec = *spec;
    let style = spec.axes.style.clone().expect("the graph keeps a style");
    assert!(style.export.transparent);
    let text = app_model::graph_json::pipeline_text(&spec);
    assert_eq!(app_model::graph_json::pipeline_from_text(&text).unwrap(), spec);
}
