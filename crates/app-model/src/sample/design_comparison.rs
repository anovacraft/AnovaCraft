//! Two candidate designs for the same response-surface experiment, stacked
//! in one table for design evaluation before any runs are made.

use super::{Sample, col, table, text};
use crate::DesignEvaluationRequest;
use crate::project::SavedReport;
use stat_data::{Column, DataTable};
use stat_doe::{Factor, ModelFormula, OptimalOptions, box_behnken, i_optimal};

/// Factor names and ranges of the planned experiment.
const FACTORS: [(&str, f64, f64); 3] = [("Temperature", 150.0, 190.0), ("Time", 10.0, 30.0), ("Pressure", 2.0, 6.0)];

/// Two 15-run plans for a quadratic model in temperature (150–190 °C), time
/// (10–30 min) and pressure (2–6 bar), in actual units: a Box–Behnken
/// design with three centre runs, and an I-optimal design
/// built by coordinate exchange for the full quadratic model
/// ([`stat_doe::i_optimal`], seed 1). The `Design` column names each run's
/// plan; `Yield` is empty, waiting for the experiment.
///
/// # Panics
/// Never: the designs are built from fixed, valid settings.
#[must_use]
pub fn design_comparison() -> DataTable {
    let bbd = box_behnken(3, Some(3)).expect("Box–Behnken design");
    let optimal = i_optimal(&ModelFormula::quadratic(3), &OptimalOptions::new(15)).expect("I-optimal design");
    let factors: Vec<Factor> = FACTORS.iter().map(|&(n, lo, hi)| Factor::new(n, lo, hi)).collect();
    let (mut design, mut run) = (Vec::new(), Vec::new());
    let mut settings: Vec<Vec<f64>> = vec![Vec::new(); 3];
    for (label, runs) in [("Box–Behnken", &bbd.runs), ("I-optimal", &optimal.design.runs)] {
        for (i, r) in runs.iter().enumerate() {
            design.push(label.to_owned());
            run.push((i + 1) as f64);
            for (j, f) in factors.iter().enumerate() {
                settings[j].push(f.actual(r[j]));
            }
        }
    }
    let n = design.len();
    let mut columns = vec![text("Design", design), Column::numeric("Run", run)];
    columns.extend(FACTORS.iter().zip(settings).map(|(&(name, _, _), v)| Column::numeric(name, v)));
    columns.push(Column::numeric("Yield", vec![f64::NAN; n]));
    table("Design comparison (planned)", columns)
}

pub(super) fn design_comparison_sample() -> Sample {
    let t = design_comparison();
    let factors: Vec<usize> = FACTORS.iter().map(|f| col(&t, f.0)).collect();
    let names: Vec<String> = FACTORS.iter().map(|f| f.0.to_owned()).collect();
    let request = DesignEvaluationRequest {
        model: ModelFormula::quadratic(3).describe(&names),
        compare_by: Some(col(&t, "Design")),
        ..DesignEvaluationRequest::new(&factors)
    };
    Sample { table: t, reports: vec![SavedReport::DesignEvaluation { request }] }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::RowStateManager;

    /// The I-optimal plan predicts better on average over the cube than the
    /// Box–Behnken one (smaller I), as it was built to; the Box–Behnken
    /// design's I is 0.384722… (exact moments, NumPy:
    /// `design_evaluation_reference.py`).
    #[test]
    fn compares_the_two_plans() {
        let s = design_comparison_sample();
        assert_eq!(s.table.n_rows(), 30);
        let SavedReport::DesignEvaluation { request } = &s.reports[0] else { panic!() };
        let a = crate::DesignEvaluationAnalysis::run(&s.table, &RowStateManager::new(30), request).unwrap();
        let e: Vec<_> = a.evaluations().collect();
        assert_eq!((e[0].runs, e[1].runs, e[0].formula.n_terms()), (15, 15, 10));
        assert!((e[0].average_variance - 0.384_722_222_222_222_24).abs() < 1e-12, "{}", e[0].average_variance);
        assert!(e[1].average_variance < e[0].average_variance, "I-optimal has the smaller I");
        assert!(a.comparison().unwrap().relative_i < 1.0);
        assert!(a.title().contains("Box–Behnken vs I-optimal"), "{}", a.title());
    }
}
