//! The analysis menus' grouping: Quality, Multivariate and Reliability list
//! their items under headings in order, and the cross-listed items (Describe
//! ▸ Summarize by group…, Visualize ▸ Scatterplot matrix…, Edit ▸ Add column
//! ▸ Random values…) open the same dialogs as their home menus.

use super::*;

/// Open the top-level menu `name` and check that `expected` (headings and
/// items) is painted in that order, top to bottom.
fn check_order(gui: &mut Gui, name: &str, expected: &[&str]) {
    let is_menu = |n: &Node<'_>| n.role() == Role::Button && n.label().as_deref() == Some(name);
    gui.harness.get_all_by(is_menu).next().expect("no such menu").simulate_click();
    gui.run();
    let painted = gui.painted_text();
    let mut from = 0;
    for entry in expected {
        let at = painted[from..]
            .iter()
            .position(|t| t == entry)
            .unwrap_or_else(|| panic!("{name}: {entry} missing or out of order in {painted:?}"));
        from += at + 1;
    }
    // Close the menu again.
    gui.harness.get_all_by(is_menu).next().expect("no such menu").simulate_click();
    gui.run();
}

#[test]
fn analysis_menus_are_grouped_under_headings() {
    let mut gui = Gui::with_sample("Device-A");
    check_order(
        &mut gui,
        "Quality",
        &[
            "Control charts",
            "Control chart…",
            "Multivariate control chart…",
            "Process capability",
            "Process capability…",
            "Attribute capability…",
            "Measurement systems",
            "Gauge R&R…",
            "Attribute gauge bias and repeatability…",
            "Acceptance sampling",
            "Acceptance sampling plan…",
            "Problem-solving tools",
            "Analysis of means…",
            "Multi-vari chart…",
        ],
    );
    check_order(
        &mut gui,
        "Multivariate",
        &[
            "Relationships",
            "Correlations…",
            "Item analysis…",
            "Reduce dimensions",
            "Principal components…",
            "Factor analysis…",
            "Correspondence analysis…",
            "Dimension reduction (MDS, t-SNE, UMAP)…",
            "Clusters",
            "Clustering…",
            "Gaussian mixture clustering…",
            "Latent class analysis…",
            "Groups",
            "Multivariate analysis of variance…",
            "Discriminant analysis…",
        ],
    );
    check_order(
        &mut gui,
        "Reliability",
        &[
            "Life data",
            "Survival curves…",
            "Life distributions…",
            "Competing risks…",
            "Life data with predictors",
            "Proportional hazards…",
            "Degradation…",
            "Field and repair data",
            "Warranty returns…",
            "Reliability growth…",
            "Planning",
            "Test plans…",
        ],
    );
}

#[test]
fn cross_listed_items_open_their_dialogs() {
    let mut gui = Gui::with_sample("Device-A");
    gui.menu(&["Describe", "Summarize by group…"]);
    assert!(gui.app().table_op_dialog.is_some(), "Describe ▸ Summarize by group… opened nothing");
    gui.app_mut().table_op_dialog = None;
    gui.run();

    gui.menu(&["Visualize", "Scatterplot matrix…"]);
    assert!(gui.app().method_dialog.is_some(), "Visualize ▸ Scatterplot matrix… opened nothing");
    gui.app_mut().method_dialog = None;
    gui.run();

    gui.menu(&["Edit", "Add column", "Random values…"]);
    assert!(gui.app().compare_dialog.is_some(), "Edit ▸ Add column ▸ Random values… opened nothing");
}
