//! Sample for the multi-way table and the stratified contingency analysis.

use super::{Sample, col, table, text};
use crate::project::SavedReport;
use crate::{CellFrequency, CellSummary, MultiwayTableRequest, StratifiedRequest};
use stat_core::rng::Rng;
use stat_data::{Column, DataTable, MeasurementLevel};

/// Patients per centre: (drug improved, drug not improved, placebo
/// improved, placebo not improved).
const COUNTS: [(&str, [usize; 4]); 5] =
    [("A", [18, 7, 10, 15]), ("B", [12, 13, 6, 19]), ("C", [25, 5, 18, 12]), ("D", [4, 6, 2, 8]), ("E", [8, 2, 5, 5])];

/// An **invented** multi-centre trial of a drug against placebo: 200
/// patients in five centres (A–E) with the outcome (improved or not), a
/// baseline severity (mild, moderate, severe), age and the change in a
/// symptom score. The outcome counts per centre and arm are fixed (see
/// `COUNTS`): improvement rates differ between centres, the drug's odds
/// ratio is similar in each, and two centres are small. Severity, age and
/// score change are drawn with the workspace's integer-based generator
/// (seed 156), so the table is the same on every platform.
#[must_use]
pub fn multicentre_trial() -> DataTable {
    let mut rng = Rng::new(156);
    let (mut centre, mut arm, mut outcome, mut severity, mut age, mut change) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (c, counts) in COUNTS {
        let mut patients: Vec<(bool, bool)> = Vec::new();
        for (k, &n) in counts.iter().enumerate() {
            patients.extend(std::iter::repeat_n((k < 2, k % 2 == 0), n));
        }
        rng.shuffle(&mut patients);
        for (drug, improved) in patients {
            centre.push(c.to_owned());
            arm.push(if drug { "Drug" } else { "Placebo" }.to_owned());
            outcome.push(if improved { "Improved" } else { "Not improved" }.to_owned());
            severity.push(
                ["Mild", "Mild", "Mild", "Mild", "Moderate", "Moderate", "Moderate", "Moderate", "Severe", "Severe"]
                    [rng.below(10)]
                .to_owned(),
            );
            age.push((25 + rng.below(46)) as f64);
            let noise = (rng.below(17) + rng.below(17)) as f64 / 4.0 - 4.0;
            let base = if improved { 8.0 } else { 2.0 } + if drug { 1.0 } else { 0.0 };
            change.push(base + noise);
        }
    }
    let mut severity = text("Severity", severity);
    severity.level = MeasurementLevel::Ordinal;
    table(
        "Multi-centre trial (invented)",
        vec![
            text("Centre", centre),
            text("Treatment", arm),
            severity,
            text("Outcome", outcome),
            Column::numeric("Age", age),
            Column::numeric("Score change", change),
        ],
    )
}

pub(super) fn multicentre_trial_sample() -> Sample {
    let t = multicentre_trial();
    let multiway = MultiwayTableRequest {
        frequencies: vec![CellFrequency::Count, CellFrequency::RowPercent],
        measures: vec![col(&t, "Score change")],
        statistics: vec![CellSummary::Mean],
        subtotals: true,
        ..MultiwayTableRequest::new(vec![col(&t, "Centre"), col(&t, "Treatment")], vec![col(&t, "Outcome")])
    };
    let stratified = StratifiedRequest::new(col(&t, "Treatment"), col(&t, "Outcome"), vec![col(&t, "Centre")]);
    Sample {
        reports: vec![
            SavedReport::MultiwayTable { request: multiway },
            SavedReport::StratifiedTables { request: stratified },
        ],
        table: t,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::RowStateManager;

    #[test]
    fn trial_counts_and_reports() {
        let t = multicentre_trial();
        assert_eq!(t.n_rows(), 200);
        let s = multicentre_trial_sample();
        let rows = RowStateManager::new(200);
        let SavedReport::StratifiedTables { request } = &s.reports[1] else { panic!() };
        let a = crate::StratifiedAnalysis::run(&t, &rows, request).unwrap();
        let mh = a.mantel_haenszel().unwrap();
        // Computed by hand from COUNTS: Σad/n over Σbc/n.
        let (mut r, mut q) = (0.0, 0.0);
        for (_, [a, b, c, d]) in COUNTS {
            let n = (a + b + c + d) as f64;
            r += (a * d) as f64 / n;
            q += (b * c) as f64 / n;
        }
        assert!((mh.odds_ratio - r / q).abs() < 1e-12);
        assert!(mh.cmh.p_value < 0.01, "the drug works in this invented trial");
        assert!(mh.breslow_day.unwrap().p_value > 0.5, "with similar odds ratios in every centre");
        assert!(a.pooled_exact().is_some());
    }
}
