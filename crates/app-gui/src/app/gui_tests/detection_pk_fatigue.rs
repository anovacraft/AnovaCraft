//! Censored values in Describe ▸ Distribution…, Describe ▸ Pharmacokinetic
//! parameters…, Reliability ▸ Fatigue (stress–life) curves…, Multivariate ▸
//! Principal coordinates…, distances in Multivariate ▸ Clustering… and
//! Data ▸ Reshape ▸ Distance matrix…: each dialog fills its request, says
//! what is missing, and the report draws its plots.

use super::*;
use app_model::SectionKind;

fn empty_sample(name: &str) -> Gui {
    let mut gui = Gui::with_sample(name);
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui
}

fn is_open(gui: &Gui, title: &str) -> bool {
    gui.harness.query_by_role_and_label(Role::Window, title).is_some()
}

fn shows_part(gui: &Gui, part: &str) -> bool {
    gui.harness.query_all_by_label_contains(part).next().is_some()
}

fn tick(gui: &mut Gui, label: &str) {
    gui.harness.get_by(|n| n.role() == Role::CheckBox && n.label().as_deref() == Some(label)).simulate_click();
    gui.run();
}

fn first_saved(gui: &Gui) -> app_model::SavedReport {
    gui.app().documents[gui.app().active].reports[0].saved()
}

fn drawn_plots(gui: &mut Gui) -> Vec<SectionKind> {
    gui.harness.set_size(egui::vec2(1500.0, 4200.0));
    gui.results();
    gui.app().documents[gui.app().active].reports[0].plot_keys().into_iter().map(|(k, _)| k).collect()
}

fn assert_drawn(gui: &mut Gui, kinds: &[SectionKind]) {
    let plots = drawn_plots(gui);
    for kind in kinds {
        assert!(plots.contains(kind), "{kind:?} not drawn: {plots:?}");
    }
}

/// Reopen the first report with Edit analysis… and check its summary.
fn edit_first_report(gui: &mut Gui, title: &str, summary: &str) {
    gui.results();
    let options = |n: &Node<'_>| n.role() == Role::Button && n.label().as_deref() == Some("⋯ Options");
    gui.harness.get_all_by(options).next().expect("a report").simulate_click();
    gui.run();
    gui.click("Edit analysis…");
    assert!(is_open(gui, title), "Edit analysis… reopens {title}");
    assert!(shows_part(gui, summary), "filled from the report: {:?}", gui.painted_text());
    gui.click("Update");
    assert_eq!(gui.app().documents[gui.app().active].reports.len(), 1, "Update keeps one report");
}

/// Non-detects: a flag column makes the values left-censored; the flag
/// cannot also be analysed.
#[test]
fn distribution_dialog_takes_non_detects() {
    const TITLE: &str = "Distribution";
    let mut gui = empty_sample("Well water arsenic");
    gui.menu(&["Describe", "Distribution…"]);
    tick(&mut gui, "Arsenic (µg/L)");
    gui.choose(TITLE, 0, "A column flags censored values");
    assert!(gui.shows("⚠ Choose the column that marks censored values"));
    gui.choose(TITLE, 1, "Non-detect");
    gui.shot("dialog-distribution-censored");
    assert!(!gui.widget("Run").is_disabled());
    gui.click("Run");
    let app_model::SavedReport::Distribution { request, .. } = first_saved(&gui) else { panic!("not a distribution") };
    let col = |n: &str| gui.app().documents[gui.app().active].table.find(n).unwrap();
    assert_eq!(
        request.censoring,
        Some(app_model::Censoring::Indicator {
            column: col("Non-detect"),
            direction: app_model::CensorDirection::Left
        })
    );
    assert_drawn(&mut gui, &[SectionKind::CensoredPlot]);
    let text = gui.painted_text();
    assert!(text.iter().any(|t| t.contains("Kaplan–Meier")), "{text:?}");
    gui.shot("distribution-censored-report");
}

/// Pharmacokinetics: subject, time, concentration, formulation and a dose
/// column; the report draws linear and semi-log profiles.
#[test]
fn pk_dialog_analyses_profiles() {
    const TITLE: &str = "Pharmacokinetic parameters";
    let mut gui = empty_sample("PK profiles");
    gui.menu(&["Describe", "Pharmacokinetic parameters…"]);
    gui.shot("dialog-pk-first-open");
    assert!(gui.shows("⚠ Choose a subject column"));
    gui.choose(TITLE, 0, "Subject");
    gui.choose(TITLE, 1, "Time (h)");
    gui.choose(TITLE, 2, "Time (h)");
    assert!(gui.shows("⚠ A column is chosen twice"));
    gui.choose(TITLE, 2, "Concentration (mg/L)");
    gui.choose(TITLE, 3, "Formulation");
    gui.click("From a column");
    assert!(gui.shows("⚠ Choose a dose column"));
    gui.choose(TITLE, 4, "Dose (mg)");
    assert!(!gui.widget("Run").is_disabled());
    gui.click("Run");
    assert!(!is_open(&gui, TITLE));
    let app_model::SavedReport::Pharmacokinetics { request } = first_saved(&gui) else { panic!("not a PK report") };
    assert!(matches!(request.dose, app_model::PkDose::Column(_)));
    assert!(request.treatment.is_some());
    assert_drawn(&mut gui, &[SectionKind::PkProfilePlot, SectionKind::PkSemiLogPlot]);
    gui.shot("pk-report");
    edit_first_report(&mut gui, TITLE, "Non-compartmental parameters of Concentration (mg/L) over Time (h)");
}

/// Fatigue: cycles, run-outs and stress; the report draws the S–N plot and
/// the probability plot of the residuals.
#[test]
fn fatigue_dialog_fits_steel() {
    const TITLE: &str = "Fatigue curves";
    let mut gui = empty_sample("Steel fatigue");
    gui.menu(&["Reliability", "Fatigue (stress–life) curves…"]);
    gui.shot("dialog-fatigue-first-open");
    assert!(gui.shows("⚠ Choose a cycles column"));
    gui.choose(TITLE, 0, "Cycles");
    gui.choose(TITLE, 1, "Run-out");
    gui.choose(TITLE, 2, "Cycles");
    assert!(gui.shows("⚠ A column is chosen twice"));
    gui.choose(TITLE, 2, "Stress (MPa)");
    gui.choose(TITLE, 5, "Weibull");
    gui.click("Run");
    assert!(!is_open(&gui, TITLE));
    let app_model::SavedReport::Fatigue { request } = first_saved(&gui) else { panic!("not a fatigue report") };
    assert_eq!(request.family, app_model::RegressionFamily::Weibull);
    assert!(request.runout.is_some());
    assert_drawn(
        &mut gui,
        &[SectionKind::FatigueSnPlot, SectionKind::FatigueResiduals, SectionKind::FatigueProbabilityPlot],
    );
    gui.shot("fatigue-report");
    edit_first_report(&mut gui, TITLE, "S–N curve of Cycles by Stress (MPa), weibull scatter");
}

/// Principal coordinates of Gower distances on mixed columns, coloured by a
/// column; then of a stored distance matrix.
#[test]
fn principal_coordinates_dialog_measures_or_reads_distances() {
    let mut gui = empty_sample("Shrub traits");
    gui.menu(&["Multivariate", "Principal coordinates…"]);
    gui.shot("dialog-pcoa-first-open");
    assert!(gui.shows("⚠ Choose the columns to compare"));
    for c in ["Leaf length (cm)", "Seed mass (g)", "Shade tolerance", "Growth form", "Thorns"] {
        tick(&mut gui, c);
    }
    gui.click("Run");
    let app_model::SavedReport::PrincipalCoordinates { request } = first_saved(&gui) else { panic!() };
    assert!(matches!(request.input, app_model::PcoaInput::Variables(ref s) if s.columns.len() == 5));
    assert_drawn(&mut gui, &[SectionKind::PcoaPlot]);
    gui.shot("pcoa-report");

    // The town sample's own report reads its distance matrix; Edit
    // analysis… reopens it in matrix mode with all 12 columns.
    let mut gui = Gui::with_sample("Town distances");
    let pcoa = gui.app().documents[gui.app().active]
        .reports
        .iter()
        .position(|r| matches!(r.saved(), app_model::SavedReport::PrincipalCoordinates { .. }))
        .expect("the sample has principal coordinates");
    let active = gui.app().active;
    let keep = gui.app_mut().documents[active].reports.remove(pcoa);
    gui.app_mut().documents[active].reports = vec![keep];
    edit_first_report(&mut gui, "Principal coordinates", "Principal coordinates of a 12-object distance matrix");
    let app_model::SavedReport::PrincipalCoordinates { request } = first_saved(&gui) else { panic!() };
    assert!(matches!(request.input, app_model::PcoaInput::Matrix(ref c) if c.len() == 12), "{:?}", request.input);
    assert_drawn(&mut gui, &[SectionKind::PcoaPlot]);
}

/// Hierarchical clustering on Gower distances accepts text columns.
#[test]
fn clustering_dialog_uses_gower_distances() {
    const TITLE: &str = "Clustering";
    let mut gui = empty_sample("Shrub traits");
    gui.menu(&["Multivariate", "Clustering…"]);
    gui.click("Hierarchical");
    let offered =
        |gui: &Gui, c: &str| gui.window(TITLE).get_all_by_role(Role::CheckBox).any(|n| n.label().as_deref() == Some(c));
    assert!(!offered(&gui, "Growth form"), "text columns are not offered for Euclidean distances");
    gui.choose(TITLE, 0, "Gower (mixed types)");
    for c in ["Leaf length (cm)", "Seed mass (g)", "Growth form", "Thorns"] {
        tick(&mut gui, c);
    }
    assert!(shows_part(&gui, "Gower (mixed types) distances"), "{:?}", gui.painted_text());
    gui.click("Run");
    let app_model::SavedReport::Cluster { request } = first_saved(&gui) else { panic!("not a clustering report") };
    assert_eq!(request.distance, app_model::ClusterDistance::Metric(app_model::DistanceMetric::Gower));
    assert_eq!(request.columns.len(), 4);
}

/// Data ▸ Reshape ▸ Distance matrix… makes a table with a label column and
/// one column per row.
#[test]
fn distance_matrix_table_operation() {
    let mut gui = Gui::with_sample("Shrub traits");
    let docs = gui.app().documents.len();
    gui.menu(&["Data", "Reshape", "Distance matrix…"]);
    gui.shot("dialog-distance-matrix-first-open");
    assert!(gui.shows("⚠ Choose at least one column to compare"));
    gui.choose("Distance matrix", 0, "Gower (mixed types)");
    for c in ["Leaf length (cm)", "Seed mass (g)", "Shade tolerance", "Growth form", "Thorns"] {
        tick(&mut gui, c);
    }
    gui.choose("Distance matrix", 1, "Shrub");
    gui.click("Create");
    assert_eq!(gui.app().documents.len(), docs + 1, "the matrix opens as a new table");
    let t = &gui.app().documents[gui.app().active].table;
    assert_eq!((t.n_rows(), t.n_cols()), (16, 17));
    assert_eq!(t.column(1).unwrap().name, "Shrub 01");
}
