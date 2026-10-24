//! The two user manuals against the running app.
//!
//! * [`quick_guide_names_every_menu_item`] walks every menu of the menu bar
//!   and checks that the Quick Guide (`docs/guide`) describes each item on
//!   its menu's page, so a new, moved or renamed item fails here until the
//!   guide catches up.
//! * [`manual_screenshots`] (ignored; run on demand) renders the pictures
//!   both manuals use, at one fixed window size so none is stretched or
//!   squashed:
//!
//! ```sh
//! scripts/capture-manual-shots.sh
//! ```

use std::collections::BTreeSet;
use std::path::PathBuf;

use super::*;

/// The window size every manual picture is taken at (16:10).
pub(super) const SHOT_SIZE: [f32; 2] = [1440.0, 900.0];

/// Manual pictures taken partway through an ordinary GUI test, where the
/// steps to reach them already live: (picture, the test's own shot name,
/// the test). `scripts/capture-manual-shots.sh` runs these tests with
/// `MANUAL_TEST_SHOTS` set, and [`Gui::shot`] then saves only these shots,
/// at [`SHOT_SIZE`] in light colours, under the picture's name.
pub(super) const TEST_SHOTS: &[(&str, &str, &str)] = &[
    ("distribution-censored-report", "distribution-censored-report", "detection_pk_fatigue::distribution_dialog_takes_non_detects"),
    ("pk-report", "pk-report", "detection_pk_fatigue::pk_dialog_analyses_profiles"),
    ("fatigue-report", "fatigue-report", "detection_pk_fatigue::fatigue_dialog_fits_steel"),
    ("pcoa-report", "pcoa-report", "detection_pk_fatigue::principal_coordinates_dialog_measures_or_reads_distances"),
    ("dialog-value-labels", "dialog-value-labels", "data_table::value_labels_show_in_the_grid_and_rename_levels"),
    ("row-colours-in-plot", "row-colours-in-plot", "data_table::rows_take_colours_and_markers_from_a_column"),
    ("heading-charts", "heading-charts", "data_table::heading_charts_select_the_rows_of_a_bar"),
    ("save-plot-dialog", "save-plot-dialog", "plot_export::save_plot_dialog_writes_the_size_on_paper_and_keeps_it"),
    ("report-filter-panel", "report-filter-panel", "canvas_interaction::report_filter_panel_steps_through_levels"),
    ("graph-drop-zones", "graph-drop-zones", "canvas_interaction::columns_dropped_on_the_panels_go_to_the_zone_under_the_pointer"),
    ("duplicate-on-another-table", "duplicate-on-another-table", "plot_styles::duplicate_maps_columns_and_paste_reaches_the_project"),
    ("project-folders", "project-folders", "folders::folders_take_items_by_menu_and_drag_and_are_saved"),
    ("plot-axis-look", "axis-offset", "axis_look::offset_axis_moves_its_ruler_and_labels"),
    ("plot-axis-breaks", "graph-axis-breaks", "graph_axes::several_breaks_with_segment_scales"),
    ("reference-lines-window", "reference-line-report", "reference_lines::report_plot_reference_line_from_plot_settings"),
    ("graph-annotations", "annotation-selected", "plot_annotations::annotations_drag_and_nudge"),
    ("formula-typeset", "dialog-formula-typeset", "dialogs::d_formula_dialog_typesets_the_formula"),
    ("open-image-large", "open-image-large", "images::large_image_opens_reduced_cropped_or_full_size"),
    ("image-plot-report", "image-plot-report", "images::image_plot_draws_pixels_and_box_selection_selects_their_rows"),
    ("image-plot-selection", "image-plot-selection", "images::image_plot_draws_pixels_and_box_selection_selects_their_rows"),
    ("export-image", "export-image", "images::export_image_writes_the_value_column_back"),
    ("dialog-several-responses-stepwise", "dialog-several-responses-stepwise", "stepwise_several::several_responses_dialog_selects_terms_stepwise"),
    ("dialog-stepwise-several", "dialog-stepwise-several", "stepwise_several::stepwise_dialog_runs_for_several_responses"),
];

/// Print the tests that take [`TEST_SHOTS`] (for the capture script).
#[test]
#[ignore = "lists the tests that take manual pictures; used by scripts/capture-manual-shots.sh"]
fn print_test_shots() {
    let only = std::env::var("MANUAL_SHOTS").unwrap_or_default();
    let only: Vec<&str> = only.split(',').map(str::trim).filter(|n| !n.is_empty()).collect();
    let mut tests: Vec<&str> =
        TEST_SHOTS.iter().filter(|t| only.is_empty() || only.contains(&t.0)).map(|t| t.2).collect();
    tests.dedup();
    println!("TEST_SHOTS {}", tests.join(" "));
}

/// The ids of every button now in the accessibility tree.
fn buttons(gui: &Gui) -> BTreeSet<egui::accesskit::NodeId> {
    gui.harness.query_all_by_role(Role::Button).map(|n| n.id()).collect()
}

/// The label of the button with accessibility id `id`.
fn label_of(gui: &Gui, id: egui::accesskit::NodeId) -> Option<String> {
    gui.harness.query_all_by(|n| n.id() == id).next().and_then(|n| n.label())
}

/// One item of a menu: its label and, for a submenu, its items.
#[derive(Debug)]
struct MenuItem {
    label: String,
    items: Vec<MenuItem>,
}

/// Submenus whose items depend on the user's files rather than the app:
/// their own name is checked, their contents are not.
const DYNAMIC_SUBMENUS: &[&str] = &["Open recent", "Open sample data", "Run saved script"];

/// The items of the menu whose buttons appeared beyond `before`, opening
/// each submenu by hovering it. `menu` is the set of buttons visible with
/// the menu open (submenus closed).
fn walk(gui: &mut Gui, before: &BTreeSet<egui::accesskit::NodeId>) -> Vec<MenuItem> {
    let menu = buttons(gui);
    let mut ids: Vec<_> = menu.difference(before).copied().collect();
    // Top to bottom, as the menu shows them.
    let top = |id| {
        gui.harness.query_all_by(|n| n.id() == id).next().and_then(|n| n.bounding_box()).map_or(f64::MAX, |b| b.y0)
    };
    ids.sort_by(|a, b| top(*a).total_cmp(&top(*b)));
    let mut items = Vec::new();
    for id in ids {
        let Some(label) = label_of(gui, id) else { continue };
        if label.trim().is_empty() {
            continue;
        }
        let mut sub = Vec::new();
        if !DYNAMIC_SUBMENUS.contains(&label.as_str()) {
            let base = buttons(gui);
            let hovered = match gui.harness.query_all_by(|n| n.id() == id).next() {
                Some(node) => {
                    node.hover();
                    true
                }
                None => false,
            };
            if hovered {
                gui.run();
                if buttons(gui).difference(&base).next().is_some() {
                    sub = walk(gui, &base);
                }
            }
        }
        items.push(MenuItem { label, items: sub });
    }
    items
}

/// Every top menu of the menu bar with its items, on a sample dataset (so
/// every item is shown).
fn menu_tree() -> Vec<MenuItem> {
    let mut gui = Gui::with_sample("Longley");
    gui.app_mut().settings.tips_dismissed = true;
    gui.harness.set_size(egui::vec2(1440.0, 1400.0));
    gui.run();
    let tops = [
        "File",
        "Edit",
        "Data",
        "Describe",
        "Compare",
        "Model",
        "Multivariate",
        "Design",
        "Quality",
        "Reliability",
        "Series and Signals",
        "Visualize",
        "Scripts",
        "Help",
    ];
    let mut tree = Vec::new();
    for top in tops {
        let before = buttons(&gui);
        let is_menu = |n: &Node<'_>| n.role() == Role::Button && n.label().as_deref() == Some(top);
        let Some(button) = gui.harness.query_all_by(is_menu).next() else {
            panic!("no {top} menu in the menu bar");
        };
        button.simulate_click();
        gui.run();
        let items = walk(&mut gui, &before);
        tree.push(MenuItem { label: top.to_owned(), items });
        // Close it again.
        gui.harness.query_all_by(is_menu).next().expect("menu").simulate_click();
        gui.run();
    }
    tree
}

/// The folder holding the repository's `docs`.
fn docs_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs")
}

/// The Quick Guide page for top menu `top`.
fn guide_page(top: &str) -> PathBuf {
    let slug = top.to_lowercase().replace(' ', "-");
    docs_dir().join("guide/src/menus").join(format!("{slug}.md"))
}

/// Whether `page` names menu item `label` in bold or as a heading (for a
/// submenu), with or without its
/// trailing "…", a leading symbol (such as ⏺) or a count in brackets (such
/// as "(0)").
fn guide_names(page: &str, label: &str) -> bool {
    let name = label.trim_start_matches(|c: char| !c.is_alphanumeric()).trim_end_matches('…');
    let name = match name.rsplit_once(" (") {
        Some((head, count)) if count.trim_end_matches(')').chars().all(|c| c.is_ascii_digit()) => head,
        _ => name,
    };
    // Bold text can wrap across lines.
    let flat = page.split_whitespace().collect::<Vec<_>>().join(" ");
    flat.contains(&format!("**{name}**"))
        || flat.contains(&format!("**{name}…**"))
        || page.lines().any(|line| line.trim_start_matches('#').trim() == name && line.starts_with('#'))
}

fn print_tree(items: &[MenuItem], depth: usize, out: &mut String) {
    for item in items {
        out.push_str(&format!("{}{}\n", "  ".repeat(depth), item.label));
        print_tree(&item.items, depth + 1, out);
    }
}

#[test]
fn quick_guide_names_every_menu_item() {
    let tree = menu_tree();
    let mut missing = Vec::new();
    for top in &tree {
        assert!(!top.items.is_empty(), "the {} menu showed no items", top.label);
        let path = guide_page(&top.label);
        let page = std::fs::read_to_string(&path).unwrap_or_default();
        fn check(items: &[MenuItem], trail: &str, page: &str, missing: &mut Vec<String>) {
            for item in items {
                let here = format!("{trail} ▸ {}", item.label);
                if !guide_names(page, &item.label) {
                    missing.push(here.clone());
                }
                check(&item.items, &here, page, missing);
            }
        }
        if page.is_empty() {
            missing.push(format!("{} (no page {})", top.label, path.display()));
        } else {
            check(&top.items, &top.label, &page, &mut missing);
        }
    }
    let mut listing = String::new();
    print_tree(&tree, 0, &mut listing);
    assert!(
        missing.is_empty(),
        "The Quick Guide (docs/guide/src/menus) does not describe these menu items; add each, in bold, to its \
         menu's page:\n  {}\n\nThe menus as the app shows them:\n{listing}",
        missing.join("\n  ")
    );
}

/// Print the menus as the app shows them (for writing the guide).
#[test]
#[ignore = "prints the menu tree; run on demand"]
fn print_menu_tree() {
    let mut listing = String::new();
    print_tree(&menu_tree(), 0, &mut listing);
    println!("{listing}");
}

// ---------------------------------------------------------------------------
// Screenshots for the manuals.

/// A fresh app at the manual's window size, light colours, tips hidden,
/// on sample `name` (the start of its catalog name), or with nothing open.
fn shot_app(sample: Option<&str>) -> Gui {
    let mut gui = match sample {
        Some(name) => Gui::with_sample(name),
        None => Gui::start(),
    };
    gui.app_mut().settings.tips_dismissed = true;
    apply_color_scheme(&gui.harness.ctx.clone(), ColorScheme::Light);
    gui.harness.set_size(egui::vec2(SHOT_SIZE[0], SHOT_SIZE[1]));
    gui.run();
    gui
}

fn show(gui: &mut Gui, workspace: Workspace) {
    let active = gui.app().active;
    gui.app_mut().documents[active].workspace = workspace;
    gui.run();
}

/// The results view with every report folded except the first whose title
/// contains `title`, which is opened.
fn open_report(gui: &mut Gui, title: &str) {
    gui.results();
    let active = gui.app().active;
    let full = gui.app().documents[active]
        .reports
        .iter()
        .map(|r| r.title())
        .find(|t| t.contains(title))
        .unwrap_or_else(|| panic!("no report titled …{title}…"));
    gui.press("Collapse all");
    let header = gui
        .harness
        .query_all_by(|node| node.role() != Role::Label && node.label().as_deref() == Some(full.as_str()))
        .next()
        .map(|node| node.id())
        .unwrap_or_else(|| panic!("no header for {full}"));
    gui.harness.get_by(|node| node.id() == header).click();
    gui.run();
}

/// Save a picture of the part of the window around `focus` (in points): the
/// smallest 16:10 rectangle at least 720 points wide that holds it with a
/// margin, kept inside the window, so menus and dialogs show large but at a
/// window's proportions.
fn shot_around(gui: &mut Gui, name: &str, focus: egui::Rect) {
    let Some(dir) = std::env::var_os(SHOTS) else { return };
    let image = gui.harness.render().unwrap_or_else(|e| panic!("could not render {name}: {e}"));
    let scale = image.width() as f32 / SHOT_SIZE[0];
    let focus = focus.expand(24.0);
    let ratio = SHOT_SIZE[0] / SHOT_SIZE[1];
    let w = focus.width().max(focus.height() * ratio).clamp(720.0, SHOT_SIZE[0]);
    let h = w / ratio;
    let x = (focus.center().x - w / 2.0).clamp(0.0, SHOT_SIZE[0] - w);
    // Menus open downwards from the menu bar: keep the bar in the picture.
    let y = (focus.center().y - h / 2.0).clamp(0.0, SHOT_SIZE[1] - h).min(focus.top().max(0.0));
    let crop = image::imageops::crop_imm(
        &image,
        (x * scale) as u32,
        (y * scale) as u32,
        (w * scale) as u32,
        (h * scale) as u32,
    )
    .to_image();
    let dir = PathBuf::from(dir);
    std::fs::create_dir_all(&dir).expect("screenshot folder");
    crop.save(dir.join(format!("{name}.png"))).expect("screenshot written");
}

/// The rectangle (in points) of accessibility node `id`.
fn rect_of(gui: &Gui, id: egui::accesskit::NodeId) -> Option<egui::Rect> {
    let b = gui.harness.query_all_by(|n| n.id() == id).next()?.bounding_box()?;
    Some(egui::Rect::from_min_max(egui::pos2(b.x0 as f32, b.y0 as f32), egui::pos2(b.x1 as f32, b.y1 as f32)))
}

/// The smallest window (a dialog, not the app's own) now open.
fn dialog_rect(gui: &Gui) -> egui::Rect {
    gui.harness
        .query_all_by_role(Role::Window)
        .filter_map(|n| n.bounding_box())
        .map(|b| egui::Rect::from_min_max(egui::pos2(b.x0 as f32, b.y0 as f32), egui::pos2(b.x1 as f32, b.y1 as f32)))
        .filter(|r| r.width() < SHOT_SIZE[0] - 1.0)
        .min_by(|a, b| a.area().total_cmp(&b.area()))
        .expect("an open dialog")
}

/// Scroll the view under the window's centre until the first text starting
/// with `label` is near the top.
fn scroll_to(gui: &mut Gui, label: &str) {
    let centre = egui::pos2(SHOT_SIZE[0] * 0.6, SHOT_SIZE[1] * 0.55);
    let mut last = None;
    for _ in 0..60 {
        let top = gui
            .painted_text_rects()
            .into_iter()
            .find(|(text, _, _)| text.starts_with(label))
            .map(|(_, rect, _)| rect.top());
        // Text far below the view is not drawn: scroll down to find it.
        let step = top.map_or(300.0, |top| top - 130.0);
        // Near the top, or as near as the end of the reports allows.
        if step.abs() < 30.0 || (top.is_some() && top == last) {
            return park(gui);
        }
        last = top;
        let events = &mut gui.harness.input_mut().events;
        events.push(egui::Event::PointerMoved(centre));
        events.push(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -step.clamp(-300.0, 300.0)),
            modifiers: egui::Modifiers::NONE,
        });
        gui.harness.run_steps(12);
    }
    panic!("no text starting {label}");
}

/// Take the pointer away, so no tooltip shows.
fn park(gui: &mut Gui) {
    gui.harness.input_mut().events.push(egui::Event::PointerGone);
    gui.harness.run_steps(12);
}

/// Open the menus along `path` and leave them open; returns the rectangle
/// the open menus (and the menu bar's button) cover.
fn open_menus(gui: &mut Gui, path: &[&str]) -> egui::Rect {
    let before = buttons(gui);
    for (i, menu) in path.iter().enumerate() {
        let is_item = |n: &Node<'_>| n.role() == Role::Button && n.label().as_deref() == Some(*menu);
        let node = if i == 0 { gui.harness.get_all_by(is_item).next() } else { gui.harness.get_all_by(is_item).last() };
        let node = node.unwrap_or_else(|| panic!("no menu {menu}"));
        if i == 0 {
            node.simulate_click()
        } else {
            node.hover()
        }
        gui.run();
    }
    let is_top = |n: &Node<'_>| n.role() == Role::Button && n.label().as_deref() == Some(path[0]);
    let top = gui.harness.query_all_by(is_top).next().map(|n| n.id()).and_then(|id| rect_of(gui, id));
    buttons(gui)
        .difference(&before)
        .filter_map(|id| rect_of(gui, *id))
        .chain(top)
        .fold(egui::Rect::NOTHING, |a, b| a.union(b))
}

/// A report screenshot: the picture's name, the sample, part of the report's
/// title, and the section to scroll to (if not the report's top).
type ReportShot = (&'static str, &'static str, &'static str, Option<&'static str>);

/// Every report picture in the Reference Manual.
const REPORT_SHOTS: &[ReportShot] = &[
    ("linear-model-report", "Longley", "Linear Model", None),
    ("imputation-report", "Air quality", "Multivariate normal", None),
    ("response-explorer", "Catalyst", "Linear Model", Some("Response Explorer")),
    ("distribution-report", "Michelson", "Distribution", None),
    ("distribution-identification", "Seal strength", "Distribution Identification", Some("Histogram with Best Fit")),
    ("capability-non-normal", "Seal strength", "Process Capability", Some("Histogram with Specification Limits")),
    ("pareto-chart", "Solder defects", "Pareto Chart of Cause", None),
    ("cause-effect-diagram", "Solder defects", "Cause and Effect", None),
    ("multi-vari-chart", "Shaft diameters", "Multi-Vari Chart of Diameter (mm) by Batch / Shaft", None),
    ("best-subsets", "Hald", "Best Subsets", Some("BIC by Model Size")),
    ("screening-volcano", "Transcript counts", "Treatment + Batch", Some("Volcano Plot of Treatment")),
    ("dunnett-comparisons", "Blood counts", "Compare Groups", None),
    ("best-comparisons", "Paint drying", "Compare Groups", None),
    ("crossover-equivalence", "Bioequivalence", "Crossover", None),
    ("decision-tree-diagram", "Loan applications", "Classification Tree", None),
    ("forest-oob-error", "Loan applications", "Random forest", Some("Out-of-Bag Error by Number of Trees")),
    ("model-comparison", "Loan applications", "Model Comparison", Some("ROC Curves")),
    ("pls-report", "Fuel spectra", "Partial Least Squares", None),
    ("svm-tuning", "Pump sensors", "radial", Some("Tuning")),
    ("mars-report", "Fuel consumption", "degree 2", None),
    ("several-responses", "Coating formulation", "Several Responses", None),
    ("several-responses-region", "Coating formulation", "Several Responses", Some("Specification Region")),
    ("several-responses-stepwise", "Coating formulation", "terms selected stepwise", Some("Term Selection")),
    ("correlations-heat-map", "Flower", "Correlations", None),
    ("scatterplot-matrix", "Flower", "Scatterplot", None),
    ("outlier-distances", "Flower", "Outlier", None),
    ("manova", "Flower", "MANOVA", None),
    ("item-analysis", "Course evaluation", "Item Analysis", None),
    ("factor-analysis", "Ability tests", "Factor Analysis", None),
    ("discriminant-canonical-plot", "Honey origins", "Linear Discriminant", Some("Canonical Plot")),
    ("gaussian-mixture", "Cell populations", "Gaussian Mixture", None),
    ("latent-class", "Symptom checklist", "Latent Class", None),
    ("dimension-reduction", "Expression profiles", "t-SNE", None),
    ("shepard-diagram", "Town distances", "Metric Scaling", Some("Shepard Diagram")),
    ("signal-to-noise", "Moulding shrinkage", "Signal-to-Noise", None),
    ("design-evaluation", "Design comparison", "Design Evaluation", None),
    ("factorial-analysis", "Filtration rate", "Factorial Analysis", None),
    ("gauge-nested", "Weld pull", "Nested gauge", None),
    ("gauge-expanded", "Bore gauges", "Expanded gauge", None),
    ("gauge-linearity", "Gauge linearity", "Bias and Linearity", None),
    ("gauge-repeatability", "Gauge repeatability", "Gauge Repeatability", None),
    ("intraclass-correlation", "Gauge study", "Intraclass", None),
    ("attribute-agreement", "Visual inspection", "Grade", None),
    ("bayes-estimation", "Response rates by site (simulated)", "Bayesian Estimation: Proportion", None),
    ("bayes-factor-test", "Stroop reaction times (simulated)", "Bayes Factor t Test", None),
    ("bayes-anova", "Stroop reaction times (simulated)", "Bayesian ANOVA", None),
    ("bayes-model", "Eight schools (Rubin 1981)", "Bayesian Hierarchical", None),
    ("bayes-model-trace", "Eight schools (Rubin 1981)", "Bayesian Hierarchical", Some("Trace of")),
    ("attribute-gauge", "Go/no-go", "Attribute Gauge", None),
    ("control-chart-phases", "Fill weights", "by phase", None),
    ("between-within-capability", "Coating batches", "Between/Within", None),
    ("attribute-capability", "Supplier lots", "Binomial Capability", None),
    ("acceptance-sampling", "Supplier lots", "Acceptance Sampling", None),
    ("control-chart-three-way", "Coil thickness", "Three-Way", None),
    ("control-chart-zone", "Coil thickness", "Zone", None),
    ("attribute-laney", "Ward safety", "p′ Chart", None),
    ("capability-summary", "Tablet hardness", "Capability Summary", None),
    ("multivariate-chart", "Bottle filling", "Hotelling", None),
    ("pca-chart", "Reactor sensors", "Principal-Components Control Chart", Some("T² Chart")),
    ("monitoring-limits", "Clean-room monitoring", "Monitoring Limits: Air colonies", Some("Alert and Action Levels")),
    ("analysis-of-means", "Heat treatment", "by Furnace and Shift", None),
    ("categorical-logistic-report", "Service ratings", "Ordinal Logistic", None),
    ("dose-response", "Rotenone", "Probit", None),
    ("treatment-effects", "Cardiac rehabilitation", "Treatment Effect: Fitness gain", Some("Balance Plot")),
    ("method-comparison", "Glucose methods", "Method Comparison", None),
    ("shelf-life", "Tablet stability", "Shelf Life", None),
    ("nonlinear-regression", "Seedling growth", "Nonlinear Regression", None),
    ("nonlinear-global-fit", "Compound potency", "Nonlinear Regression", None),
    ("curve-template-standard", "ELISA plate", "4PL", Some("Standard Curve Acceptance")),
    ("curve-template-mechanisms", "Enzyme inhibition", "Mixed inhibition", Some("Inhibition Mechanism")),
    ("curve-template-schild", "Antagonist shift", "Gaddum", Some("Schild Plot")),
    ("survival-curves", "Leukemia remission", "Survival", None),
    ("life-distributions", "Seal inspections", "Life Distributions", None),
    ("life-stress", "Device-A", "Life–Stress", None),
    ("degradation", "Laser degradation", "Degradation", None),
    ("proportional-hazards", "Leukemia with covariates", "Proportional Hazards", None),
    ("survival-regression", "Capacitor life", "Parametric Survival", None),
    ("reliability-growth", "Prototype failures", "by Phase", None),
    ("repairable-systems", "Pump repairs", "Repairable", None),
    ("competing-risks", "Pump failure modes", "Competing Risks", None),
    ("warranty-returns", "Monthly warranty", "Warranty Returns", None),
    ("time-series-description", "Monthly electricity", "Series Description", None),
    ("time-series-decomposition", "Monthly electricity", "Decomposition", None),
    ("time-series-trend", "Product adoption", "Trend Model", None),
    ("time-series-cross-correlation", "Advertising and sales", "Cross-Correlation", None),
    ("exponential-smoothing", "Hotel occupancy", "Exponential Smoothing", None),
    ("arima", "Reactor temperature", "ARIMA", None),
    ("arima-inputs", "Kiln fuel feed", "ARIMA", Some("Transfer Functions")),
    ("signal-smoothing", "Machine vibration", "Smoothing", None),
    ("curve-calculus", "Machine vibration", "Derivative and Integral", None),
    ("peak-finding", "ECG-like signal", "Spectrum Peaks", None),
    ("peak-fitting", "Raman bands", "Spectrum Peaks", Some("Peak Fit")),
    ("signal-spectrum", "Test bench signals", "Fourier Spectrum", Some("Spectrogram: A")),
    ("signal-filter", "Test bench signals", "Digital Filter", Some("Frequency Response")),
    ("signal-correlation", "Test bench signals", "Cross-Correlation", None),
    ("signal-wavelets", "Test bench signals", "Wavelet Analysis", Some("Multilevel Decomposition: A")),
    ("image-spectrum", "Machined surface scan", "Image Spectrum", Some("Log-Magnitude Spectrum")),
    ("image-radial-profiles", "Machined surface scan", "Image Spectrum", Some("Radial Profiles")),
    ("image-filter", "Machined surface scan", "Image Frequency Filter", Some("Filtered Image")),
    ("image-registration", "Blurred and shifted scene", "Image Registration", Some("Shift Found")),
    ("functional-data", "Exotherm profiles", "Functional", None),
    ("multiway-table", "Multi-centre", "Multi-way Table", None),
    ("stratified-tables", "Multi-centre", "Stratified", None),
    ("bootstrap-report", "Repair times", "Bootstrap", None),
    ("permutation-test-report", "Repair times", "by Crew", None),
    ("scatter-3d", "Hillside", "3-D Scatter", None),
    ("data-surface", "Hillside", "Contour and Surface: Elevation", None),
    ("contour-filled", "Reaction yield", "Response Surface", Some("Contour Plot")),
    ("mixture-ternary", "Yarn elongation", "Mixture Model", Some("Ternary Contour Plot")),
    ("many-tests-report", "Gene screen", "Many Tests", None),
    ("factorial-repeated-measures", "Reaction times", "Repeated Measures", None),
    ("nested-comparison", "Dendrite lengths", "Nested Comparison", None),
    ("adjust-p-values-report", "Trial endpoints", "Adjusted p-values", None),
    ("area-under-curve-report", "Plasma concentration", "Area under the Curve", None),
    ("multiple-response-report", "Visitor survey", "Multiple Response", None),
    ("text-exploration", "Espresso machine reviews", "Text Exploration", None),
    ("topic-model", "Espresso machine reviews", "Topic Model", None),
    ("sentiment-scores", "Espresso machine reviews", "Sentiment Scores", None),
    ("neural-network-report", "Polymer batches", "Neural Network (5 tanh)", None),
    ("model-search", "Polymer batches", "Model Search", None),
    ("gaussian-process-report", "Reaction yield (simulated)", "Gaussian Process", None),
    ("sequential-optimization", "Reaction yield (simulated)", "Sequential Optimization: Yield", None),
    ("graph-estimation", "Reading trial", "Estimation Plot: Score by Programme", None),
];

/// A dialog screenshot as it first opens: name, sample (if any) and menu path.
type DialogShot = (&'static str, Option<&'static str>, &'static [&'static str]);

/// Every first-open dialog picture in the Reference Manual.
const DIALOG_SHOTS: &[DialogShot] = &[
    ("import-database", None, &["File", "Import from database…"]),
    ("import-web", None, &["File", "Import from the web…"]),
    ("distribution-calculator", Some("Longley"), &["Describe", "Distribution calculator…"]),
    ("digital-filter-dialog", Some("Test bench signals"), &["Series and Signals", "Digital filter…"]),
    ("image-filter-dialog", Some("Machined surface scan"), &["Series and Signals", "Image frequency filter…"]),
    ("space-filling-dialog", None, &["Design", "Latin hypercube (maximin)…"]),
    ("design-dialog", None, &["Design", "Central composite…"]),
    ("extreme-vertices-dialog", None, &["Design", "Extreme vertices…"]),
    ("robust-design-dialog", None, &["Design", "Robust design (orthogonal arrays)…"]),
    ("split-plot-dialog", None, &["Design", "Split-plot (hard-to-change factors)…"]),
    (
        "bayes-model-dialog",
        Some("Eight schools (Rubin 1981)"),
        &["Model", "Bayesian", "Bayesian regression or hierarchical model…"],
    ),
    ("design-constraints", None, &["Design", "D-optimal (coordinate exchange)…"]),
    ("design-augment", Some("Catalyst"), &["Design", "Augment design…"]),
    ("design-dsd", None, &["Design", "Definitive screening…"]),
    ("design-categorical-screening", None, &["Design", "Screening with categorical factors…"]),
    ("design-simulate", Some("Catalyst"), &["Design", "Simulate responses…"]),
    ("categorical-logistic-dialog", Some("Service ratings"), &["Model", "Ordinal or nominal logistic regression…"]),
    ("life-distributions-dialog", Some("Seal inspections"), &["Reliability", "Life distributions…"]),
    ("reliability-test-plan", Some("Seal inspections"), &["Reliability", "Test plans…"]),
    ("analysis-dialog", Some("Longley"), &["Model", "Linear model…"]),
    ("peak-fitting-dialog", Some("Raman bands"), &["Series and Signals", "Peak fitting…"]),
];

/// The top menus, as named in the Quick Guide's pictures `guide-menu-<slug>`.
const GUIDE_MENUS: &[&str] = &[
    "File",
    "Edit",
    "Data",
    "Describe",
    "Compare",
    "Model",
    "Multivariate",
    "Design",
    "Quality",
    "Reliability",
    "Series and Signals",
    "Visualize",
    "Scripts",
    "Help",
];

/// One picture's set-up: the app, and the part of it to show (`None`: the
/// whole window).
type ShotStep = Box<dyn Fn() -> (Gui, Option<egui::Rect>)>;

/// The pictures that need more than opening a report, dialog or menu.
/// The clinical trial sample project, showing document `table` (by name).
fn clinical_trial(table: &str) -> Gui {
    let mut gui = shot_app(None);
    let n = app_model::sample::project_samples()
        .iter()
        .position(|s| s.name.starts_with("Clinical trial with dropout"))
        .expect("the sample project");
    gui.perform(Action::OpenProjectSample(n));
    let index = gui.app().documents.iter().position(|d| d.table.name() == table).expect("the table");
    gui.app_mut().set_active(index);
    gui.run();
    gui
}

fn custom_shots() -> Vec<(&'static str, ShotStep)> {
    vec![
        (
            "multiple-imputation-pooled",
            Box::new(|| {
                let mut gui = clinical_trial("20 imputations");
                open_report(&mut gui, "Linear Model");
                (gui, None)
            }),
        ),
        (
            "multiple-imputation-dialog",
            Box::new(|| {
                let mut gui = shot_app(Some("Air quality"));
                show(&mut gui, Workspace::Data);
                gui.menu(&["Data", "Impute missing values…"]);
                for c in app_model::sample::AIR_QUALITY_COLUMNS {
                    gui.harness
                        .get_by(|n| n.role() == Role::CheckBox && n.label().as_deref() == Some(c))
                        .simulate_click();
                    gui.run();
                }
                gui.click("Multiple imputation: chained equations");
                gui.click("More options");
                park(&mut gui);
                let rect = dialog_rect(&gui);
                (gui, Some(rect))
            }),
        ),
        (
            "multiple-imputation-diagnostics",
            Box::new(|| {
                let mut gui = clinical_trial("Clinical trial with dropout (simulated)");
                open_report(&mut gui, "Imputation");
                scroll_to(&mut gui, "Convergence: Mean of imputed values of Week 12");
                (gui, None)
            }),
        ),
        (
            "welcome-page",
            Box::new(|| {
                let mut gui = shot_app(None);
                gui.perform(Action::Close(0));
                (gui, None)
            }),
        ),
        (
            "window-overview",
            Box::new(|| {
                let mut gui = shot_app(Some("Longley"));
                open_report(&mut gui, "Linear Model");
                show(&mut gui, Workspace::SideBySide);
                (gui, None)
            }),
        ),
        (
            "first-analysis-brushing",
            Box::new(|| {
                let mut gui = shot_app(Some("Norris"));
                open_report(&mut gui, "Linear Model");
                let active = gui.app().active;
                gui.app_mut().documents[active].rows.select([25], SelectionMode::Replace);
                show(&mut gui, Workspace::SideBySide);
                (gui, None)
            }),
        ),
        (
            "data-grid",
            Box::new(|| {
                let mut gui = shot_app(Some("Longley"));
                show(&mut gui, Workspace::Data);
                (gui, None)
            }),
        ),
        (
            "report-options-menu",
            Box::new(|| {
                let mut gui = shot_app(Some("Longley"));
                open_report(&mut gui, "Linear Model");
                let options = |n: &Node<'_>| n.role() == Role::Button && n.label().as_deref() == Some("⋯ Options");
                gui.harness.get_all_by(options).next().expect("options").simulate_click();
                gui.run();
                (gui, None)
            }),
        ),
        (
            "graph-canvas",
            Box::new(|| {
                let mut gui = shot_app(Some("Flower"));
                let active = gui.app().active;
                gui.app_mut().documents[active].reports.clear();
                let spec = stat_viz::GraphSpec {
                    encoding: stat_viz::EncodingPlan {
                        x: vec!["Sepal length".into()],
                        y: vec!["Petal length".into()],
                        color: Some("Variety".into()),
                        ..Default::default()
                    },
                    points: Some(stat_viz::PointsElement::default()),
                    smoother: Some(stat_viz::SmootherElement::default()),
                    ..stat_viz::GraphSpec::default()
                };
                gui.app_mut().add_report(ReportRequest::Graph(Box::new(spec)));
                gui.results();
                (gui, None)
            }),
        ),
        (
            "graph-right-axis",
            Box::new(|| {
                let mut gui = shot_app(Some("Longley"));
                let active = gui.app().active;
                gui.app_mut().documents[active].reports.clear();
                let spec = stat_viz::GraphSpec {
                    encoding: stat_viz::EncodingPlan {
                        x: vec!["Year".into()],
                        y: vec!["Employed".into()],
                        y2: vec!["Unemployed".into()],
                        ..Default::default()
                    },
                    points: Some(stat_viz::PointsElement::default()),
                    line: Some(stat_viz::LineElement::default()),
                    axes: stat_viz::GraphAxes {
                        title: Some(r"Workers, 1947 to 1962: N_{employed} and N_{unemployed}".into()),
                        ..Default::default()
                    },
                    ..stat_viz::GraphSpec::default()
                };
                gui.app_mut().add_report(ReportRequest::Graph(Box::new(spec)));
                gui.results();
                (gui, None)
            }),
        ),
        (
            "figure-page",
            Box::new(|| {
                let mut gui = shot_app(Some("Longley"));
                super::figure_page::add_plots(&mut gui, &["Residual Normal Quantile Plot", "Scree Plot"]);
                (gui, None)
            }),
        ),
        (
            "row-subsets",
            Box::new(|| {
                let mut gui = shot_app(Some("Visitor survey"));
                show(&mut gui, Workspace::SideBySide);
                let active = gui.app().active;
                let rows = &mut gui.app_mut().documents[active].rows;
                rows.save_subset("Under 35", 0..60);
                rows.save_subset("55 and over", 115..160);
                rows.set_active_subset(Some(0));
                gui.run();
                gui.menu(&["Data", "Row subsets"]);
                gui.widget("Row subsets").hover();
                gui.run();
                (gui, None)
            }),
        ),
        (
            "text-pane",
            Box::new(|| {
                let mut gui = shot_app(Some("Visitor survey"));
                show(&mut gui, Workspace::Data);
                let active = gui.app().active;
                let table = &gui.app().documents[active].table;
                let comment = table.find("Comment").expect("comments");
                let row = (0..table.n_rows())
                    .find(|&r| table.column(comment).is_some_and(|c| c.cell_text(r).starts_with("The café")))
                    .expect("a long comment");
                let grid = &mut gui.app_mut().documents[active].grid;
                grid.place_cursor(row, comment);
                grid.text_pane = true;
                gui.run();
                (gui, None)
            }),
        ),
        (
            "notebook",
            Box::new(|| {
                let mut gui = shot_app(Some("Norris"));
                gui.menu(&["Scripts", "New notebook"]);
                let project = gui.app().documents[gui.app().active].project;
                let notebook = &mut gui.app_mut().project_mut(project).expect("the project").notebooks[0];
                notebook.cells[0].source = "## Calibration check\n\nThe instrument's readings against the \
                                            reference values."
                    .into();
                notebook.cells[1].source = "let t = current_table();\nprint(`${t.rows()} rows`);\nt".into();
                gui.run();
                gui.click("▶ Run all");
                super::data_and_scripts::settle_notebooks(&mut gui);
                (gui, None)
            }),
        ),
        // The Quick Guide's walk-through, on Plant growth.
        (
            "guide-data-grid",
            Box::new(|| {
                let mut gui = shot_app(Some("Plant growth"));
                show(&mut gui, Workspace::Data);
                (gui, None)
            }),
        ),
        (
            "guide-dialog",
            Box::new(|| {
                let gui = guide_dialog();
                let rect = dialog_rect(&gui);
                (gui, Some(rect))
            }),
        ),
        (
            "guide-report",
            Box::new(|| {
                let mut gui = guide_dialog();
                gui.click("Run");
                gui.results();
                (gui, None)
            }),
        ),
        (
            "guide-window-parts",
            Box::new(|| {
                let mut gui = guide_dialog();
                gui.click("Run");
                show(&mut gui, Workspace::SideBySide);
                (gui, None)
            }),
        ),
        (
            "open-data-workbook",
            Box::new(|| {
                let mut gui = shot_app(None);
                let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../examples/data/shift_weights_report.xlsx");
                gui.perform(Action::OpenPaths(vec![path]));
                let rect = dialog_rect(&gui);
                (gui, Some(rect))
            }),
        ),
        (
            "script-library",
            Box::new(|| {
                let mut gui = shot_app(Some("Plant growth"));
                show(&mut gui, Workspace::Data);
                let dir = temp_dir("manual-script-library");
                for name in ["Daily summary", "Clean column names", "Weekly control charts"] {
                    std::fs::write(dir.join(format!("{name}.rhai")), "current_table().rows()\n").unwrap();
                }
                gui.app_mut().settings.scripts_folder = Some(dir);
                let project = gui.app().documents[gui.app().active].project;
                gui.app_mut().project_mut(project).expect("the project").scripts.push(
                    app_model::project::ProjectScript {
                        name: "Refit growth model".into(),
                        source: "current_table().rows()\n".into(),
                        run_on_open: false,
                    },
                );
                gui.run();
                // The sidebar's section, below the menu bar's Scripts menu.
                let header = gui
                    .harness
                    .get_all_by_label("Scripts")
                    .filter_map(|n| Some((n.raw_bounds()?.y0, n.id())))
                    .max_by(|a, b| a.0.total_cmp(&b.0))
                    .expect("the Scripts section")
                    .1;
                gui.harness.get_by(|n| n.id() == header).simulate_click();
                gui.run();
                let item = node_rect(&gui.widget("Daily summary"));
                let at = item.center();
                let button = |pressed| egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Secondary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                };
                gui.harness.input_mut().events.push(egui::Event::PointerMoved(at));
                gui.harness.step();
                gui.harness.input_mut().events.push(button(true));
                gui.harness.step();
                gui.harness.input_mut().events.push(button(false));
                gui.run();
                gui.widget("Keep in project").hover();
                gui.run();
                let menu = node_rect(&gui.widget("Keep in project"));
                let focus = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), menu.max + egui::vec2(40.0, 120.0));
                (gui, Some(focus))
            }),
        ),
        (
            "batch-run",
            Box::new(|| {
                let mut gui = shot_app(None);
                let dir = temp_dir("manual-batch-run");
                let data = dir.join("daily weights");
                std::fs::create_dir_all(&data).unwrap();
                std::fs::write(data.join("2026-10-12.csv"), "Line,Weight\nA,501.2\nB,498.7\nA,502.9\n").unwrap();
                std::fs::write(data.join("2026-10-13.csv"), "Line,Weight\nA,499.8\nB,n/a\nA,500.4\n").unwrap();
                std::fs::write(data.join("2026-10-14.csv"), "Line,Weight\nA,503.1\nB,497.9\nB,499.6\n").unwrap();
                let script = dir.join("mean weight.rhai");
                std::fs::write(
                    &script,
                    "// Rows and mean of the \"Weight\" column of each file.\nlet t = open_table(batch_file);\n\
                     let total = 0.0;\nfor w in t.column(\"Weight\") { total += w; }\n\
                     print(`${t.rows()} rows`);\n#{ rows: t.rows(), mean: total / t.rows() }\n",
                )
                .unwrap();
                gui.app_mut().batch.set_for_test(script, data, "*.csv");
                gui.run();
                gui.window("Run script over a folder").get_by_label("▶ Run").simulate_click();
                gui.run();
                for _ in 0..600 {
                    if !gui.app().batch.is_running() {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(20));
                    gui.run();
                }
                gui.run();
                park(&mut gui);
                let rect = dialog_rect(&gui);
                (gui, Some(rect))
            }),
        ),
        (
            "export-reports-dialog",
            Box::new(|| {
                let mut gui = shot_app(Some("Plant growth"));
                let active = gui.app().active;
                let first = gui.app().documents[active].reports[0].title();
                open_report(&mut gui, &first);
                gui.harness.get_all_by_label("Export…").next().expect("the report's Export… button").simulate_click();
                gui.run();
                gui.click("Word document (.docx)");
                park(&mut gui);
                let rect = dialog_rect(&gui);
                (gui, Some(rect))
            }),
        ),
        (
            "report-filter-switcher",
            Box::new(|| {
                let mut gui = shot_app(Some("Plant growth"));
                let active = gui.app().active;
                gui.app_mut().documents[active].reports.clear();
                let height = gui.app().documents[active].table.find("Height").unwrap();
                gui.app_mut().add_report(ReportRequest::Distribution(app_model::DistributionRequest {
                    column: height,
                    quantile_method: Default::default(), censoring: None }));
                gui.results();
                gui.click("⋯ Options");
                gui.click("Filter rows…");
                gui.click("+ Add condition");
                let fertilizer = |n: &Node<'_>| n.role() == Role::Button && n.label().as_deref() == Some("Fertilizer");
                gui.harness.get_all_by(fertilizer).last().expect("the column in the menu").simulate_click();
                gui.run();
                gui.click("Filter Fertilizer");
                gui.click("A");
                gui.harness.press_key(egui::Key::Escape);
                gui.run();
                gui.click("⋯ Options");
                gui.click("Column picker");
                gui.harness.press_key(egui::Key::Escape);
                park(&mut gui);
                (gui, None)
            }),
        ),
        ("graph-brackets", Box::new(|| (reading_trial_brackets(), None))),
        (
            "graph-cluster-map",
            Box::new(|| {
                let spec = stat_viz::GraphSpec {
                    encoding: stat_viz::EncodingPlan {
                        x: ["January (°C)", "July (°C)", "Rainfall (mm)", "Sunshine (h)", "Wind (m/s)", "Frost days"]
                            .map(String::from)
                            .to_vec(),
                        label: Some("Town".into()),
                        ..Default::default()
                    },
                    form: stat_viz::GraphForm::ClusterMap(stat_viz::ClusterMapSettings::default()),
                    ..stat_viz::GraphSpec::default()
                };
                (graph_shot("Regional climate", spec), None)
            }),
        ),
        (
            "graph-flows",
            Box::new(|| {
                let spec = stat_viz::GraphSpec {
                    encoding: stat_viz::EncodingPlan {
                        x: vec!["Origin".into(), "Hub".into(), "Destination".into()],
                        y: vec!["Parcels".into()],
                        ..Default::default()
                    },
                    form: stat_viz::GraphForm::Sankey(stat_viz::FlowSettings { size_by_sum: true, labels: true }),
                    ..stat_viz::GraphSpec::default()
                };
                let mut gui = graph_shot("Parcel flows", spec);
                let label = last_text_rect(&gui, "Hub A");
                click_at(&mut gui, egui::pos2(label.min.x - 18.0, label.center().y));
                park(&mut gui);
                (gui, None)
            }),
        ),
        (
            "dashboard-overview",
            Box::new(|| {
                let gui = greenhouse();
                (gui, None)
            }),
        ),
        (
            "dashboard-move-tile",
            Box::new(|| {
                let mut gui = greenhouse();
                let id = gui.app().shown_dashboard().expect("a dashboard");
                // Two columns narrower first, so there is room to its right.
                let narrower = app_model::dashboard::DashboardEdit::ResizeTile {
                    tile: app_model::dashboard::TileId(3),
                    width: 4,
                    height: 6,
                };
                assert!(gui.app_mut().edit_dashboard(id, narrower));
                gui.run();
                let title =
                    gui.app().dashboard(id).unwrap().tile_report(app_model::dashboard::TileId(3)).unwrap().title();
                let strip = node_rect(&gui.widget(&format!("Move tile {title}")));
                let step = (strip.width() + 10.0) / 6.0;
                let from = egui::pos2(strip.left() + 8.0, strip.center().y);
                drag_and_hold(&mut gui, from, from + egui::vec2(2.0 * step, 0.0));
                (gui, None)
            }),
        ),
        (
            "dashboard-filter",
            Box::new(|| {
                let mut gui = greenhouse();
                gui.click("Filter Greenhouse");
                (gui, None)
            }),
        ),
        (
            "dashboard-switcher",
            Box::new(|| {
                let mut gui = greenhouse();
                gui.click("Next Humidity");
                park(&mut gui);
                (gui, None)
            }),
        ),
        (
            "dashboard-web-viewer",
            Box::new(|| {
                let mut builder = Harness::builder().with_size(SHOT_SIZE);
                if std::env::var_os(SHOTS).is_some() {
                    builder = builder.wgpu();
                }
                let file = app_model::sample::greenhouse_project();
                let mut gui = Gui {
                    harness: builder.build_eframe(move |cc| {
                        apply_color_scheme(&cc.egui_ctx, ColorScheme::Light);
                        StatsApp::new_viewer(cc, file.clone(), "Greenhouse overview · Greenhouse trial".to_owned())
                    }),
                };
                apply_color_scheme(&gui.harness.ctx.clone(), ColorScheme::Light);
                gui.run();
                (gui, None)
            }),
        ),
        (
            "publish-build-viewer",
            Box::new(|| {
                let mut gui = greenhouse();
                crate::app::dashboards::test_bundle::set(None);
                file_io::test_save_answer::set(None);
                gui.click("Publish as web app…");
                park(&mut gui);
                let rect = dialog_rect(&gui);
                (gui, Some(rect))
            }),
        ),
        (
            "view-windows",
            Box::new(|| {
                let mut gui = shot_app(Some("Longley"));
                let active = gui.app().active;
                gui.app_mut().documents[active].reports.clear();
                gui.app_mut().add_report(ReportRequest::Bivariate(app_model::BivariateRequest::new(2, 0)));
                gui.run();
                let id = gui.app().documents[active].id;
                let report = gui.app().documents[active].reports[0].id();
                gui.perform(Action::PopOut { doc: id, view: View::Data });
                gui.perform(Action::PopOut { doc: id, view: View::Report(report) });
                let name = gui.app().documents[active].name().to_owned();
                let report_title = format!("{name} · {} · AnovaCraft", gui.app().documents[active].reports[0].title());
                let data_title = format!("{name} · Data · AnovaCraft");
                // Lay the two windows side by side over the main window.
                move_window(&mut gui, &data_title, egui::pos2(260.0, 120.0));
                move_window(&mut gui, &report_title, egui::pos2(760.0, 150.0));
                let (x, y) = {
                    let window = gui.window(&report_title);
                    (
                        node_rect(&window.get_by_label("Horizontal axis ruler")),
                        node_rect(&window.get_by_label("Vertical axis ruler")),
                    )
                };
                let from = egui::pos2(y.right() + 4.0, y.top() + 4.0);
                let to = egui::pos2(x.center().x, x.top() - 4.0);
                gui.drag(from, to);
                park(&mut gui);
                (gui, None)
            }),
        ),
        (
            "view-window-placeholder",
            Box::new(|| {
                let mut gui = shot_app(Some("Longley"));
                show(&mut gui, Workspace::Data);
                gui.click("New window");
                let active = gui.app().active;
                let data_title = format!("{} · Data · AnovaCraft", gui.app().documents[active].name());
                // The window is drawn inside the main one here: shrink it and
                // move it into the bottom right corner, out of the way.
                let r = node_rect(&gui.window(&data_title));
                let corner = r.right_bottom() - egui::vec2(4.0, 4.0);
                gui.drag(corner, corner - egui::vec2(r.width() - 380.0, r.height() - 220.0));
                let r = node_rect(&gui.window(&data_title));
                move_window(
                    &mut gui,
                    &data_title,
                    egui::pos2(SHOT_SIZE[0] - r.width() - 8.0, SHOT_SIZE[1] - r.height() - 34.0),
                );
                park(&mut gui);
                (gui, None)
            }),
        ),
        (
            "keep-in-project",
            Box::new(|| {
                let mut gui = shot_app(Some("Plant growth"));
                show(&mut gui, Workspace::Data);
                gui.menu(&["Data", "Summarize by group…"]);
                tick_check(&mut gui, "Height", 0);
                tick_check(&mut gui, "Fertilizer", 1);
                park(&mut gui);
                let rect = dialog_rect(&gui);
                (gui, Some(rect))
            }),
        ),
        (
            "project-tree",
            Box::new(|| {
                let mut gui = shot_app(Some("Plant growth"));
                show(&mut gui, Workspace::Data);
                let source = gui.app().active;
                gui.menu(&["Data", "Summarize by group…"]);
                tick_check(&mut gui, "Height", 0);
                tick_check(&mut gui, "Fertilizer", 1);
                gui.click("Create");
                // Fertilizer A's plants, as a second table made from the source.
                gui.perform(Action::Activate(source));
                let rows: Vec<usize> = {
                    let t = &gui.app().documents[source].table;
                    let f = t.column(t.find("Fertilizer").unwrap()).unwrap();
                    (0..t.n_rows()).filter(|&r| f.cell_text(r) == "A").collect()
                };
                gui.app_mut().documents[source].rows.select(rows.clone(), SelectionMode::Replace);
                gui.run();
                gui.menu(&["Data", "Rows", "Subset and sample…"]);
                gui.click(&format!("Selected ({})", rows.len()));
                gui.click("Create");
                // Edit the source, then show the summary with its banner.
                gui.perform(Action::Activate(source));
                gui.app_mut().documents[source].rows.select([], SelectionMode::Replace);
                let column = gui.app().documents[source].table.find("Height").unwrap();
                gui.grid(vec![GridRequest::SetCell { row: rows[0], column, text: "1000".into() }]);
                let summary =
                    gui.app().documents.iter().position(|d| d.name() == "Summary by Fertilizer").expect("the summary");
                gui.perform(Action::Activate(summary));
                park(&mut gui);
                (gui, None)
            }),
        ),
        (
            "join-dialog",
            Box::new(|| {
                let mut gui = shot_app(None);
                gui.harness.set_size(egui::vec2(SHOT_SIZE[0], SHOT_SIZE[1]));
                gui.perform(Action::OpenProjectSample(0));
                let plants = gui
                    .app()
                    .documents
                    .iter()
                    .position(|d| d.table.find("Fertilizer").is_some())
                    .expect("the plants table");
                gui.perform(Action::Activate(plants));
                show(&mut gui, Workspace::Data);
                gui.menu(&["Data", "Merge", "Join…"]);
                let title = dialog_title(&gui);
                gui.choose(&title, 1, "Greenhouse");
                gui.click("Add a key pair");
                gui.choose(&title, 2, "Week");
                park(&mut gui);
                let rect = dialog_rect(&gui);
                (gui, Some(rect))
            }),
        ),
    ]
}

/// Tick the `n`th check box labelled `label` (from 0, in drawing order),
/// unless it is ticked already.
fn tick_check(gui: &mut Gui, label: &str, n: usize) {
    let node = gui
        .harness
        .get_all_by(|node| node.role() == Role::CheckBox && node.label().as_deref() == Some(label))
        .nth(n)
        .unwrap_or_else(|| panic!("no check box {label} number {n}"));
    if node.toggled() != Some(egui::accesskit::Toggled::True) {
        node.simulate_click();
        gui.run();
    }
}

/// The title of the smallest window (a dialog) now open.
fn dialog_title(gui: &Gui) -> String {
    gui.harness
        .query_all_by_role(Role::Window)
        .filter_map(|n| Some((n.bounding_box()?, n.label()?)))
        .filter(|(b, _)| b.width() < f64::from(SHOT_SIZE[0]) - 1.0)
        .min_by(|a, b| a.0.area().total_cmp(&b.0.area()))
        .map(|(_, label)| label)
        .expect("an open dialog")
}

/// Where accessibility node `node` is on screen.
fn node_rect(node: &Node<'_>) -> egui::Rect {
    let b = node.raw_bounds().expect("the node has a position");
    egui::Rect::from_min_max(egui::pos2(b.x0 as f32, b.y0 as f32), egui::pos2(b.x1 as f32, b.y1 as f32))
}

/// Drag the window titled `title` (drawn inside the main one in tests) by
/// its title bar so its top-left corner lands near `to`.
fn move_window(gui: &mut Gui, title: &str, to: egui::Pos2) {
    let r = node_rect(&gui.window(title));
    let grip = r.left_top() + egui::vec2(60.0, 12.0);
    gui.drag(grip, to + egui::vec2(60.0, 12.0));
}

/// Press at `from` and move to `to` in steps, leaving the button down (a
/// drag in progress).
fn drag_and_hold(gui: &mut Gui, from: egui::Pos2, to: egui::Pos2) {
    let pressed = egui::Event::PointerButton {
        pos: from,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    };
    gui.harness.input_mut().events.push(egui::Event::PointerMoved(from));
    gui.harness.step();
    gui.harness.input_mut().events.push(pressed);
    gui.harness.step();
    for i in 1..=6 {
        gui.harness.input_mut().events.push(egui::Event::PointerMoved(from.lerp(to, i as f32 / 6.0)));
        gui.harness.step();
    }
}

/// Click once at `at` (screen points).
fn click_at(gui: &mut Gui, at: egui::Pos2) {
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    gui.harness.input_mut().events.push(egui::Event::PointerMoved(at));
    gui.harness.step();
    gui.harness.input_mut().events.push(button(at, true));
    gui.harness.step();
    gui.harness.input_mut().events.push(button(at, false));
    gui.run();
}

/// Where text starting with `label` was last painted.
fn last_text_rect(gui: &Gui, label: &str) -> egui::Rect {
    gui.painted_text_rects()
        .into_iter()
        .rfind(|(t, _, clip)| t.starts_with(label) && clip.width() > 0.0)
        .unwrap_or_else(|| panic!("{label:?} is not drawn"))
        .1
}

/// Sample `name` with its reports replaced by one graph canvas of `spec`.
fn graph_shot(name: &str, spec: stat_viz::GraphSpec) -> Gui {
    let mut gui = shot_app(Some(name));
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui.app_mut().add_report(ReportRequest::Graph(Box::new(spec)));
    gui.results();
    park(&mut gui);
    gui
}

/// The reading trial's scores by programme as a swarm with significance
/// brackets.
fn reading_trial_brackets() -> Gui {
    let spec = stat_viz::GraphSpec {
        encoding: stat_viz::EncodingPlan { x: vec!["Programme".into()], y: vec!["Score".into()], ..Default::default() },
        points: Some(stat_viz::PointsElement::default()),
        ..stat_viz::GraphSpec::default()
    };
    let mut gui = graph_shot("Reading trial", spec);
    // The first settings button of the layer bar is the points'.
    gui.harness.get_all_by_label("…").next().expect("points settings").simulate_click();
    gui.run();
    gui.click("Swarm (no overlap)");
    gui.harness.press_key(egui::Key::Escape);
    gui.run();
    gui.click("Brackets");
    park(&mut gui);
    gui
}

/// The greenhouse sample project on its dashboard, at the manual's size.
fn greenhouse() -> Gui {
    let mut gui = shot_app(None);
    gui.perform(Action::OpenProjectSample(0));
    park(&mut gui);
    gui
}

/// Plant growth without its ready-made reports, Compare ▸ Two or more
/// groups filled in with Height by Fertilizer.
fn guide_dialog() -> Gui {
    let mut gui = shot_app(Some("Plant growth"));
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    show(&mut gui, Workspace::Data);
    gui.menu(&["Compare", "Two or more groups…"]);
    gui.choose("Compare groups", 0, "Height");
    gui.choose("Compare groups", 1, "Fertilizer");
    gui
}

/// Pictures made of several windows' worth, each set-up drawn into one
/// quarter of the picture (left to right, top to bottom).
fn composite_shots() -> Vec<(&'static str, Vec<ShotStep>)> {
    let form = |sample: &'static str,
                form: stat_viz::GraphForm,
                x: &'static [&'static str],
                y: &'static [&'static str],
                color: Option<&'static str>|
     -> ShotStep {
        Box::new(move || {
            let spec = stat_viz::GraphSpec {
                encoding: stat_viz::EncodingPlan {
                    x: x.iter().map(|s| (*s).to_owned()).collect(),
                    y: y.iter().map(|s| (*s).to_owned()).collect(),
                    color: color.map(str::to_owned),
                    ..Default::default()
                },
                form,
                ..stat_viz::GraphSpec::default()
            };
            let mut gui = graph_shot(sample, spec);
            hide_shelves(&mut gui);
            (gui, None)
        })
    };
    let donut = stat_viz::PieSettings { donut: true, ..stat_viz::PieSettings::default() };
    let seal = |layer: fn(&mut stat_viz::GraphSpec)| -> ShotStep {
        Box::new(move || {
            let mut spec = stat_viz::GraphSpec {
                encoding: stat_viz::EncodingPlan { x: vec!["Peel strength (N)".into()], ..Default::default() },
                points: None,
                ..stat_viz::GraphSpec::default()
            };
            layer(&mut spec);
            let mut gui = graph_shot("Seal strength", spec);
            hide_shelves(&mut gui);
            (gui, None)
        })
    };
    vec![
        (
            "graph-distribution-layers",
            vec![
                seal(|spec| spec.histogram = Some(stat_viz::HistogramElement::default())),
                seal(|spec| spec.dot_plot = Some(stat_viz::DotPlotElement::default())),
                seal(|spec| spec.probability_plot = Some(stat_viz::ProbabilityPlotElement::default())),
                seal(|spec| {
                    spec.probability_plot = Some(stat_viz::ProbabilityPlotElement {
                        family: stat_core::distribution_fit::FitFamily::Weibull,
                        ..Default::default()
                    });
                    spec.options.x_scale = stat_viz::AxisScale::Log10;
                }),
            ],
        ),
        (
            "graph-round-forms",
            vec![
                form(
                    "Harbour weather",
                    stat_viz::GraphForm::WindRose(stat_viz::WindRoseSettings::default()),
                    &["Wind direction (°)"],
                    &["Wind speed (m/s)"],
                    None,
                ),
                Box::new(move || {
                    let spec = stat_viz::GraphSpec {
                        encoding: stat_viz::EncodingPlan { x: vec!["Sky".into()], ..Default::default() },
                        form: stat_viz::GraphForm::Pie(donut),
                        ..stat_viz::GraphSpec::default()
                    };
                    let mut gui = graph_shot("Harbour weather", spec);
                    let centre = last_text_rect(&gui, "Total").center();
                    let label = last_text_rect(&gui, "Clear").center();
                    click_at(&mut gui, centre + (label - centre) * (0.78 / 1.3));
                    hide_shelves(&mut gui);
                    (gui, None)
                }),
                form(
                    "Soil texture",
                    stat_viz::GraphForm::Ternary,
                    &["Sand (%)", "Silt (%)"],
                    &["Clay (%)"],
                    Some("Field"),
                ),
                form(
                    "Household budget",
                    stat_viz::GraphForm::Waterfall(stat_viz::WaterfallSettings::default()),
                    &["Item"],
                    &["Amount"],
                    None,
                ),
            ],
        ),
    ]
}

/// Draw each of `parts` and save them as one picture: two one above the
/// other, four in quarters. Each part is cropped to its first plot (from
/// its Reset view button down), drawn in a window narrow enough to give
/// the plot its cell's proportions.
fn shot_tiled(name: &str, parts: &[ShotStep]) {
    let Some(dir) = std::env::var_os(SHOTS) else { return };
    let (cols, rows) = if parts.len() <= 2 { (1, parts.len() as u32) } else { (2, 2) };
    let cell_aspect = (SHOT_SIZE[0] / cols as f32) / (SHOT_SIZE[1] / rows as f32);
    let window_width = if cols == 1 { SHOT_SIZE[0] } else { 1000.0 };
    let mut images = Vec::new();
    for step in parts {
        let (mut gui, _) = step();
        gui.harness.set_size(egui::vec2(window_width, SHOT_SIZE[1]));
        gui.run();
        let image = gui.harness.render().unwrap_or_else(|e| panic!("could not render {name}: {e}"));
        let scale = image.width() as f32 / window_width;
        let reset = gui.harness.query_all_by_label("Reset view").next().and_then(|n| n.bounding_box()).expect("a plot");
        let left = 232.0;
        let width = window_width - left - 14.0;
        let top = reset.y0 as f32 - 6.0;
        let height = (width / cell_aspect).min(SHOT_SIZE[1] - top);
        let crop = image::imageops::crop_imm(
            &image,
            (left * scale) as u32,
            (top * scale) as u32,
            (width * scale) as u32,
            (height * scale) as u32,
        )
        .to_image();
        images.push((crop, scale));
    }
    let scale = images.iter().map(|(_, s)| *s).fold(1.0, f32::max);
    let (w, h) = ((SHOT_SIZE[0] * scale) as u32, (SHOT_SIZE[1] * scale) as u32);
    let (cw, ch) = (w / cols, h / rows);
    let mut out = image::RgbaImage::from_pixel(w, h, image::Rgba([255, 255, 255, 255]));
    for (i, (part, _)) in images.iter().enumerate() {
        let small = image::imageops::resize(part, cw, ch, image::imageops::FilterType::Lanczos3);
        let (x, y) = ((i as u32 % cols) * cw, (i as u32 / cols) * ch);
        image::imageops::overlay(&mut out, &small, i64::from(x), i64::from(y));
    }
    let dir = PathBuf::from(dir);
    std::fs::create_dir_all(&dir).expect("screenshot folder");
    out.save(dir.join(format!("{name}.png"))).expect("screenshot written");
}

/// Hide every graph canvas's shelves, so the plots show large.
fn hide_shelves(gui: &mut Gui) {
    let shown = |n: &Node<'_>| {
        n.label().as_deref() == Some("Hide shelves") && n.toggled() != Some(egui::accesskit::Toggled::True)
    };
    for _ in 0..8 {
        let Some(button) = gui.harness.query_all_by(shown).next() else { break };
        button.simulate_click();
        gui.run();
    }
    park(gui);
}

/// Render every picture both manuals use into `ANOVACRAFT_GUI_SHOTS`
/// (`scripts/capture-manual-shots.sh` points it at the manual's images).
/// Set `MANUAL_SHOTS` to a comma-separated list of names to render only those.
#[test]
#[ignore = "renders the manuals' pictures; run with scripts/capture-manual-shots.sh"]
fn manual_screenshots() {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    assert!(std::env::var_os(SHOTS).is_some(), "set {SHOTS} to the folder for the pictures");
    let only: Option<Vec<String>> = std::env::var("MANUAL_SHOTS")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.split(',').map(|n| n.trim().to_owned()).collect());
    let wanted = |name: &str| only.as_ref().is_none_or(|o| o.iter().any(|n| n == name));
    let mut steps: Vec<(String, ShotStep)> = Vec::new();
    for &(name, sample, title, section) in REPORT_SHOTS {
        steps.push((
            name.to_owned(),
            Box::new(move || {
                let mut gui = shot_app(Some(sample));
                open_report(&mut gui, title);
                if let Some(section) = section {
                    scroll_to(&mut gui, section);
                }
                (gui, None)
            }),
        ));
    }
    for &(name, sample, path) in DIALOG_SHOTS {
        steps.push((
            name.to_owned(),
            Box::new(move || {
                let mut gui = shot_app(sample);
                if sample.is_some() {
                    show(&mut gui, Workspace::Data);
                }
                gui.menu(path);
                let rect = dialog_rect(&gui);
                (gui, Some(rect))
            }),
        ));
    }
    for &top in GUIDE_MENUS {
        steps.push((
            format!("guide-menu-{}", top.to_lowercase().replace(' ', "-")),
            Box::new(move || {
                let mut gui = shot_app(Some("Longley"));
                show(&mut gui, Workspace::Data);
                let rect = open_menus(&mut gui, &[top]);
                (gui, Some(rect))
            }),
        ));
    }
    for (name, step) in custom_shots() {
        steps.push((name.to_owned(), step));
    }
    let mut failed = Vec::new();
    for (name, parts) in composite_shots().into_iter().filter(|(name, _)| wanted(name)) {
        if catch_unwind(AssertUnwindSafe(|| shot_tiled(name, &parts))).is_err() {
            failed.push(name.to_owned());
        }
    }
    for (name, step) in steps.iter().filter(|(name, _)| wanted(name)) {
        let take = || {
            let (mut gui, focus) = step();
            match focus {
                Some(rect) => shot_around(&mut gui, name, rect),
                None => gui.shot(name),
            }
        };
        if catch_unwind(AssertUnwindSafe(take)).is_err() {
            failed.push(name.clone());
        }
    }
    assert!(failed.is_empty(), "could not take: {failed:?}");
}
