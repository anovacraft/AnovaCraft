//! Reusing plots and reports: Copy and Paste plot style, Duplicate on
//! another table, and project templates.

use super::figure_page::{right_click, scroll_into_view};
use super::*;
use app_model::axis_settings::GridLines;
use app_model::plot_style::{PlotFont, PlotStyle};
use app_model::style_paste::CopiedStyle;

const QUANTILES: &str = "Residual Normal Quantile Plot";
const SCREE: &str = "Scree Plot";

/// A style with no horizontal grid and the serif font.
fn custom() -> CopiedStyle {
    let mut style = PlotStyle::default();
    style.axes.y.grid = GridLines::None;
    style.export.font = PlotFont::Serif;
    CopiedStyle { from: "Test".into(), style, markers: None, legend: None }
}

/// Every plot style kept by document `doc`'s reports.
fn kept(gui: &Gui, doc: usize) -> Vec<PlotStyle> {
    gui.app().documents[doc].reports.iter().flat_map(|r| r.scope().plot_styles.iter().map(|(_, s)| s.clone())).collect()
}

/// Open `plot`'s right-click menu and its Paste plot style submenu.
fn paste_menu(gui: &mut Gui, plot: &str) {
    scroll_into_view(gui, plot);
    right_click(gui, plot);
    gui.widget("Paste plot style").hover();
    gui.run();
}

/// Paste onto one plot with a part unticked, then Copy plot style from it
/// and paste onto another plot of another report.
#[test]
fn plot_style_copies_and_pastes_between_report_plots() {
    let mut gui = Gui::with_sample("Longley");
    gui.results();
    let active = gui.app().active;
    let ctx = gui.harness.ctx.clone();
    crate::plot_style_menu::set_copied(&ctx, custom());

    paste_menu(&mut gui, QUANTILES);
    assert!(gui.shows("From: Test"));
    gui.press("Font and size");
    gui.press("Onto this plot");
    let styles = kept(&gui, active);
    assert_eq!(styles.len(), 1, "one plot took the style");
    assert_eq!(styles[0].axes.y.grid, GridLines::None);
    assert_eq!(styles[0].export.font, PlotFont::Sans, "the unticked part stays");

    // Copy it back from that plot, with every part ticked again.
    scroll_into_view(&mut gui, QUANTILES);
    right_click(&mut gui, QUANTILES);
    gui.press("Copy plot style");
    let copied = crate::plot_style_menu::copied(&ctx).expect("a copied style");
    assert_eq!(copied.from, QUANTILES);
    assert_eq!(copied.style.axes.y.grid, GridLines::None);
    paste_menu(&mut gui, SCREE);
    assert!(gui.shows(&format!("From: {QUANTILES}")));
    gui.press("Font and size");
    gui.press("Onto this plot");
    assert_eq!(kept(&gui, active).len(), 2);
    gui.shot("plot-style-paste");
}

/// "Onto every plot in this report" styles each plot of the report, the
/// collapsed ones too, and nothing in other reports.
#[test]
fn plot_style_pastes_onto_every_plot_of_a_report() {
    let mut gui = Gui::with_sample("Longley");
    gui.results();
    let active = gui.app().active;
    crate::plot_style_menu::set_copied(&gui.harness.ctx.clone(), custom());
    paste_menu(&mut gui, QUANTILES);
    gui.press("Onto every plot in this report");
    let doc = &gui.app().documents[active];
    let styled: Vec<usize> = doc.reports.iter().map(|r| r.scope().plot_styles.len()).collect();
    let owner = styled.iter().position(|&n| n > 0).expect("a report took the style");
    assert!(styled[owner] >= 2, "several plots: {styled:?}");
    assert_eq!(styled[owner], doc.reports[owner].plot_keys().len());
    assert_eq!(styled.iter().filter(|&&n| n > 0).count(), 1, "only that report");
}

/// The tensile tests sample project, showing batch A's results.
fn tensile() -> Gui {
    let mut gui = Gui::start();
    let n = app_model::sample::project_samples().iter().position(|s| s.name.starts_with("Tensile")).expect("sample");
    gui.perform(Action::OpenProjectSample(n));
    gui.results();
    gui
}

/// [`Gui::choose`] for entries named like a table, which the sidebar and
/// tabs show too: the list, drawn last, holds the last one. Entries may
/// add " · project".
fn choose_last(gui: &mut Gui, window: &str, n: usize, option: &str) {
    gui.window(window).get_all_by_role(Role::ComboBox).nth(n).expect("no such drop-down list").simulate_click();
    gui.run();
    let entry = |n: &Node<'_>| {
        !matches!(n.role(), Role::Label | Role::CheckBox)
            && !(n.role() == Role::Button && n.toggled().is_none())
            && n.label().is_some_and(|l| l == option || l.starts_with(&format!("{option} · ")))
    };
    gui.harness.get_all_by(entry).last().expect("the list's entry").simulate_click();
    gui.run();
}

fn doc_named(gui: &Gui, name: &str) -> usize {
    gui.app().documents.iter().position(|d| d.name() == name).unwrap_or_else(|| panic!("no table {name}"))
}

/// Open the graph's (the first report's) ⋯ Options menu.
fn graph_options(gui: &mut Gui) {
    let options = |n: &Node<'_>| n.role() == Role::Button && n.label().as_deref() == Some("⋯ Options");
    gui.harness.get_all_by(options).next().expect("a report").simulate_click();
    gui.run();
}

/// Duplicate on another table copies batch A's graph onto batch C after the
/// two columns named differently are chosen; its style comes along. Then a
/// copied style pasted onto every plot of the project reaches that copy.
#[test]
fn duplicate_maps_columns_and_paste_reaches_the_project() {
    let mut gui = tensile();
    graph_options(&mut gui);
    gui.press("Copy plot style");
    let copied = crate::plot_style_menu::copied(&gui.harness.ctx.clone()).expect("copied");
    assert!(copied.markers.is_some(), "a canvas copies its markers too");

    graph_options(&mut gui);
    gui.press("Duplicate on another table…");
    const DIALOG: &str = "Duplicate on another table";
    assert!(gui.shows(DIALOG));
    choose_last(&mut gui, DIALOG, 0, "Tensile batch C");
    // Specimen and Strain (%) have no column of that name; stress (MPa) matches.
    assert!(gui.shows("Choose the column that stands in for each column whose name differs:"));
    gui.choose(DIALOG, 1, "Test piece");
    gui.choose(DIALOG, 2, "Elongation (%)");
    gui.shot("duplicate-on-another-table");
    gui.click("Duplicate");
    assert!(!gui.shows(DIALOG));
    let c = doc_named(&gui, "Tensile batch C");
    let reports = &gui.app().documents[c].reports;
    assert_eq!(reports.len(), 1);
    let app_model::SavedReport::Graph { spec } = reports[0].saved() else { panic!("a graph") };
    assert_eq!(spec.encoding.x, vec!["Elongation (%)".to_owned()]);
    assert_eq!(spec.encoding.y, vec!["stress (MPa)".to_owned()]);
    let style = spec.axes.style.clone().expect("the style came along");
    assert_eq!(style.axes.y.grid, GridLines::MajorAndMinor);

    // Paste onto every plot of the project: batch C's copy takes it too.
    crate::plot_style_menu::set_copied(&gui.harness.ctx.clone(), custom());
    let a = doc_named(&gui, "Tensile batch A");
    let ctx = gui.harness.ctx.clone();
    let report = gui.app().documents[a].reports[0].id();
    gui.app_mut().carry_out_style(
        &ctx,
        a,
        report,
        crate::plot_style_menu::StyleRequest::Paste(crate::plot_style_menu::PasteTarget::Project),
    );
    gui.run();
    let app_model::SavedReport::Graph { spec } = gui.app().documents[c].reports[0].saved() else { panic!("a graph") };
    assert_eq!(spec.axes.style.expect("styled").axes.y.grid, GridLines::None);
}

/// Save project as template writes the reports without data; New project
/// from template builds them on batch C after the column choices.
#[test]
fn templates_save_and_build_on_another_table() {
    let mut gui = tensile();
    let dir = temp_dir("template");
    let path = dir.join("Tensile.acrafttemplate");
    crate::file_io::test_save_answer::set(Some(path.clone()));
    let a = gui.app().active;
    gui.perform(Action::SaveTemplate(a));
    let text = std::fs::read_to_string(&path).expect("the template was written");
    let template = app_model::project_template::ProjectTemplate::parse(&text).expect("it reads back");
    assert_eq!(template.tables.len(), 3);
    assert!(!text.contains("\"values\": [\n"), "no data in the template");

    crate::file_io::test_save_answer::set(Some(path));
    gui.perform(Action::NewFromTemplate);
    const DIALOG: &str = "New project from template";
    assert!(gui.shows(DIALOG));
    // Three template tables: A onto C, B and C onto themselves. Choosing
    // A's data adds its three column lists after its own list.
    choose_last(&mut gui, DIALOG, 0, "Tensile batch C");
    gui.choose(DIALOG, 1, "Test piece");
    gui.choose(DIALOG, 2, "Elongation (%)");
    choose_last(&mut gui, DIALOG, 4, "Tensile batch B");
    choose_last(&mut gui, DIALOG, 5, "Tensile batch C");
    gui.shot("new-project-from-template");
    let projects = gui.app().projects.len();
    gui.click("Create");
    assert!(!gui.shows(DIALOG));
    assert_eq!(gui.app().projects.len(), projects + 1);
    let new = gui.app().documents[gui.app().active].project;
    let tables: Vec<&Document> = gui.app().documents.iter().filter(|d| d.project == new).collect();
    assert_eq!(tables.len(), 3);
    assert_eq!(tables[0].name(), "Tensile batch C");
    assert_eq!(tables[0].reports.len(), 2, "batch A's graph and model");
    let app_model::SavedReport::Graph { spec } = tables[0].reports[0].saved() else { panic!("a graph") };
    assert_eq!(spec.encoding.x, vec!["Elongation (%)".to_owned()]);
}
