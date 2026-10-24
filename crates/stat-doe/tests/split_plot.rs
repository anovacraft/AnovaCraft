//! D-optimal split-plot designs against exhaustive search
//! (`tests/data/split_plot_reference.py`, NumPy).

use stat_doe::split_plot::{ChangeLevel, PlotStructure, SplitPlotOptions, split_plot_design, split_plot_log_det};
use stat_doe::{ModelFormula, default_factor_names};

use ChangeLevel::{Easy, Hard, VeryHard};

fn formula(text: &str, k: usize) -> ModelFormula {
    ModelFormula::parse(text, &default_factor_names(k)).unwrap()
}

fn check(f: &ModelFormula, changes: &[ChangeLevel], o: &SplitPlotOptions, reference: f64) {
    let d = split_plot_design(f, changes, o).unwrap();
    assert!((d.log_det - reference).abs() < 1e-9, "{changes:?} {o:?}: {} vs exhaustive {reference}", d.log_det);
    // The reported criterion is the determinant of the returned runs.
    let direct = split_plot_log_det(f, &d.design.runs, &d.structure, d.whole_plot_ratio, d.subplot_ratio);
    assert!((direct - d.log_det).abs() < 1e-9);
}

#[test]
fn exhaustive_split_plot_two_factors() {
    let f = formula("A + B + A*B", 2);
    let mut o = SplitPlotOptions::new(8, 4);
    check(&f, &[Hard, Easy], &o, 6.120_541_589_383_125);
    o.whole_plot_ratio = 10.0;
    check(&f, &[Hard, Easy], &o, 2.228_721_291_272_498);
    // Unequal whole plots: 10 runs in plots of 3, 3, 2, 2.
    let o = SplitPlotOptions::new(10, 4);
    let d = split_plot_design(&f, &[Hard, Easy], &o).unwrap();
    let mut sizes = vec![0; 4];
    d.structure.whole_plot.iter().for_each(|&w| sizes[w] += 1);
    assert_eq!(sizes, [3, 3, 2, 2]);
    check(&f, &[Hard, Easy], &o, 6.566_828_692_011_544);
}

#[test]
fn exhaustive_split_plot_quadratic() {
    let f = ModelFormula::quadratic(2);
    check(&f, &[Hard, Easy], &SplitPlotOptions::new(12, 4), 5.981_414_211_254_481);
}

#[test]
fn exhaustive_split_plot_two_hard_factors() {
    let f = ModelFormula::interactions(3);
    check(&f, &[Hard, Hard, Easy], &SplitPlotOptions::new(12, 4), 11.495_819_997_067_29);
}

#[test]
fn exhaustive_split_split_plot() {
    let f = ModelFormula::interactions(3);
    let mut o = SplitPlotOptions { subplots: 4, ..SplitPlotOptions::new(8, 2) };
    check(&f, &[VeryHard, Hard, Easy], &o, 8.467_045_916_312_005);
    o.whole_plot_ratio = 2.0;
    o.subplot_ratio = 0.5;
    check(&f, &[VeryHard, Hard, Easy], &o, 8.564_626_244_650_869);
}

/// With η = 0 the criterion is det(XᵀX): putting each run in its own whole
/// plot gives the ordinary D-optimal design (the 2³ factorial, 100%).
#[test]
fn reduces_to_ordinary_d_optimality() {
    let f = ModelFormula::interactions(3);
    let o = SplitPlotOptions { whole_plot_ratio: 0.0, ..SplitPlotOptions::new(8, 8) };
    let d = split_plot_design(&f, &[Hard, Easy, Easy], &o).unwrap();
    assert!((d.d_efficiency - 100.0).abs() < 1e-9, "{}", d.d_efficiency);
    let ols = stat_doe::log_det_information(&f, &d.design.runs);
    assert!((ols - d.log_det).abs() < 1e-9);
    // A completely randomized design scaled to the same total variance has
    // the same efficiency for any η when every whole plot has one run.
    let o = SplitPlotOptions { whole_plot_ratio: 3.0, ..o };
    let d = split_plot_design(&f, &[Hard, Easy, Easy], &o).unwrap();
    assert!((d.d_efficiency - 100.0).abs() < 1e-9, "{}", d.d_efficiency);
    // Average prediction variance of the 2³ factorial for this model over
    // the cube: tr(M (XᵀX)⁻¹) = (1 + 3/3 + 3/9)/8 = 7/24.
    assert!((d.average_variance - 7.0 / 24.0).abs() < 1e-12, "{}", d.average_variance);
    assert!((d.max_variance - 7.0 / 8.0).abs() < 1e-12, "{}", d.max_variance);
}

/// Larger whole plots: the classical 2⁴ with two hard factors in 4 whole
/// plots of 4 (the hard factors' 2² factorial across the whole plots, the
/// easy ones' 2² within each) is orthogonal and D-optimal for the main
/// effects and two-factor interactions; coordinate exchange finds a design
/// at least as good.
#[test]
fn matches_the_classical_split_plot_factorial() {
    let f = ModelFormula::interactions(4);
    let structure = PlotStructure::split_plot(16, 4).unwrap();
    let mut classical = Vec::new();
    for (a, b) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
        for (c, d) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
            classical.push(vec![a, b, c, d]);
        }
    }
    let reference = split_plot_log_det(&f, &classical, &structure, 1.0, 0.0);
    let d = split_plot_design(&f, &[Hard, Hard, Easy, Easy], &SplitPlotOptions::new(16, 4)).unwrap();
    assert!(d.log_det >= reference - 1e-9, "{} vs classical {reference}", d.log_det);
    assert_eq!(d.whole_plot_error_df, 0, "intercept, A, B, A×B use all four whole plots");
    assert!(d.design.notes.iter().any(|n| n.contains("Add whole plots")));
}
