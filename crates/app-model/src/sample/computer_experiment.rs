//! Sample for space-filling designs: a deterministic computer experiment.

use super::{Sample, col, linear, table};
use crate::ReportOptions;
use crate::project::SavedReport;
use stat_data::Column;
use stat_doe::{Factor, space_filling::maximin_latin_hypercube};

/// The borehole inputs: column name, lower and upper limit (the ranges of
/// Morris, Mitchell & Ylvisaker 1993, *Technometrics* 35, Table 1).
pub const BOREHOLE_INPUTS: [(&str, f64, f64); 8] = [
    ("rw (m)", 0.05, 0.15),
    ("r (m)", 100.0, 50_000.0),
    ("Tu (m²/yr)", 63_070.0, 115_600.0),
    ("Hu (m)", 990.0, 1110.0),
    ("Tl (m²/yr)", 63.1, 116.0),
    ("Hl (m)", 700.0, 820.0),
    ("L (m)", 1120.0, 1680.0),
    ("Kw (m/yr)", 9855.0, 12_045.0),
];

/// The borehole function (Worley 1987, ORNL/TM-10655): water flow
/// (m³/yr) through a borehole between two aquifers,
/// `Q = 2π Tu (Hu − Hl) / (ln(r/rw) [1 + 2 L Tu / (ln(r/rw) rw² Kw) + Tu/Tl])`,
/// with inputs in the order of [`BOREHOLE_INPUTS`].
#[must_use]
pub fn borehole_flow(x: &[f64; 8]) -> f64 {
    let [rw, r, tu, hu, tl, hl, l, kw] = *x;
    let log_ratio = (r / rw).ln();
    std::f64::consts::TAU * tu * (hu - hl) / (log_ratio * (1.0 + 2.0 * l * tu / (log_ratio * rw * rw * kw) + tu / tl))
}

/// A computer experiment on the borehole function: a 40-run maximin Latin
/// hypercube (seed 1) over the eight inputs' ranges, with the water flow
/// computed exactly from the formula (deterministic: no noise). Inputs are
/// rounded to 6 significant digits and the flow computed from the rounded
/// values, to 0.001 m³/yr.
#[must_use]
pub fn borehole() -> stat_data::DataTable {
    let design = maximin_latin_hypercube(40, 8, 1)
        .and_then(|d| d.with_factors(BOREHOLE_INPUTS.iter().map(|(n, lo, hi)| Factor::new(*n, *lo, *hi)).collect()))
        .expect("a valid space-filling design");
    let round6 = |v: f64| {
        let scale = 10f64.powi(5 - v.abs().log10().floor() as i32);
        (v * scale).round() / scale
    };
    let inputs: Vec<[f64; 8]> =
        design.runs.iter().map(|run| std::array::from_fn(|j| round6(design.factors[j].actual(run[j])))).collect();
    let mut columns = vec![Column::numeric("Run", (1..=40).map(f64::from).collect())];
    for (j, (name, _, _)) in BOREHOLE_INPUTS.iter().enumerate() {
        columns.push(Column::numeric(*name, inputs.iter().map(|x| x[j]).collect()));
    }
    columns.push(Column::numeric(
        "Flow (m³/yr)",
        inputs.iter().map(|x| (borehole_flow(x) * 1000.0).round() / 1000.0).collect(),
    ));
    table("Borehole computer experiment", columns)
}

/// The borehole experiment with a surrogate model: the flow on the eight
/// inputs plus the square of the borehole radius, with the Response Explorer.
pub(super) fn borehole_sample() -> Sample {
    let t = borehole();
    let names: Vec<&str> = BOREHOLE_INPUTS.iter().map(|(n, _, _)| *n).collect();
    let options = ReportOptions { show_surface_profiler: true, show_residual_plot: true, ..ReportOptions::default() };
    let SavedReport::Linear { mut spec, options } = linear(&t, "Flow (m³/yr)", &names, options) else {
        unreachable!()
    };
    spec.effects = (0..8).map(|j| vec![(j, 1)]).chain([vec![(0, 2)]]).collect();
    debug_assert_eq!(spec.response, col(&t, "Flow (m³/yr)"));
    Sample { reports: vec![SavedReport::Linear { spec, options }], table: t }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LinearModelAnalysis;
    use stat_data::RowStateManager;

    /// The flow at the centre of the input ranges, and at the published
    /// extremes (Morris, Mitchell & Ylvisaker 1993 report flows between
    /// about 2 and 310 m³/yr over the region).
    #[test]
    fn borehole_function_values() {
        let centre: [f64; 8] = std::array::from_fn(|j| (BOREHOLE_INPUTS[j].1 + BOREHOLE_INPUTS[j].2) / 2.0);
        // Hand calculation (also 70.87291 in Python): ln(r/rw) = ln(25050/0.1).
        let lr = (25_050.0_f64 / 0.1).ln();
        let q = std::f64::consts::TAU * 89_335.0 * (1050.0 - 760.0)
            / (lr * (1.0 + 2.0 * 1400.0 * 89_335.0 / (lr * 0.01 * 10_950.0) + 89_335.0 / 89.55));
        assert!((borehole_flow(&centre) - q).abs() < 1e-9);
        assert!((borehole_flow(&centre) - 70.8729).abs() < 1e-4, "{}", borehole_flow(&centre));
        let t = borehole();
        assert_eq!(t.n_rows(), 40);
        let flow = t.column(col(&t, "Flow (m³/yr)")).unwrap().as_numeric().unwrap();
        assert!(flow.iter().all(|&q| (2.0..=310.0).contains(&q)));
        // Every input range is sliced into 40 equal strata, one run in each.
        for (j, (name, lo, hi)) in BOREHOLE_INPUTS.iter().enumerate() {
            let x = t.column(col(&t, name)).unwrap().as_numeric().unwrap();
            let mut strata: Vec<usize> = x.iter().map(|v| ((v - lo) / (hi - lo) * 40.0) as usize).collect();
            strata.sort_unstable();
            assert_eq!(strata, (0..40).collect::<Vec<_>>(), "input {j}");
        }
    }

    #[test]
    fn surrogate_model_fits_closely() {
        let s = borehole_sample();
        let SavedReport::Linear { spec, .. } = &s.reports[0] else { panic!() };
        let fit = LinearModelAnalysis::run(&s.table, &RowStateManager::new(40), spec).unwrap();
        let r2 = fit.fit().summary().r_squared;
        assert!(r2 > 0.95, "R² {r2}");
    }
}
