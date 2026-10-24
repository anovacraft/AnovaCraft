//! Sample for screening many responses.

use super::{Sample, col, table};
use crate::project::SavedReport;
use crate::{ScreeningModel, ScreeningRequest};
use stat_data::{Column, DataTable};

/// Number of genes in [`transcript_counts`].
pub const COUNT_GENES: usize = 40;

/// A Poisson draw by inversion (means here stay below a few hundred).
fn poisson(rng: &mut stat_core::rng::Rng, mean: f64) -> f64 {
    let (mut k, mut p) = (0.0, (-mean).exp());
    let mut cdf = p;
    let u = rng.next_f64();
    while u > cdf && k < 10_000.0 {
        k += 1.0;
        p *= mean / k;
        cdf += p;
    }
    k
}

/// **Simulated** read counts of 40 genes in 24 tissue samples: 12 control
/// and 12 treated, half liver and half lung, processed in three batches.
/// Each gene has its own mean (exp(3 + N(0, 0.8²))) and negative binomial
/// overdispersion (shape 4: gamma-mixed Poisson counts); batches shift every
/// gene by ×0.85, ×1 and ×1.2. Treatment multiplies genes T01–T05 by 2.5,
/// 2.2, 2.0, 1.8 and 0.5 in both tissues, and T06–T08 by 2.5 in liver only;
/// the other 32 do not respond. Drawn with the workspace's seeded generator
/// (seed `0xC0DE5`).
#[must_use]
pub fn transcript_counts() -> DataTable {
    let mut rng = stat_core::rng::Rng::new(0xC0DE5);
    let n = 24;
    let treated = |i: usize| i >= 12;
    let liver = |i: usize| i % 2 == 0;
    let batch = |i: usize| i % 3;
    let mut columns = vec![
        Column::text("Sample", (1..=n).map(|i| format!("S{i:02}")).collect()),
        Column::text("Treatment", (0..n).map(|i| if treated(i) { "Treated" } else { "Control" }.to_owned()).collect()),
        Column::text("Tissue", (0..n).map(|i| if liver(i) { "Liver" } else { "Lung" }.to_owned()).collect()),
        Column::text("Batch", (0..n).map(|i| format!("B{}", batch(i) + 1)).collect()),
    ];
    let both = [2.5, 2.2, 2.0, 1.8, 0.5];
    let shape = 4.0;
    for g in 0..COUNT_GENES {
        let base = (3.0 + 0.8 * rng.normal()).exp();
        let values = (0..n)
            .map(|i| {
                let effect = match g {
                    0..=4 if treated(i) => both[g],
                    5..=7 if treated(i) && liver(i) => 2.5,
                    _ => 1.0,
                };
                let mean = base * effect * [0.85, 1.0, 1.2][batch(i)];
                // Gamma-mixed Poisson: negative binomial with shape `shape`.
                let lambda = mean * rng.gamma(shape) / shape;
                poisson(&mut rng, lambda)
            })
            .collect();
        columns.push(Column::numeric(format!("T{:02}", g + 1), values));
    }
    table("Transcript counts (simulated)", columns)
}

pub(super) fn transcript_counts_sample() -> Sample {
    let t = transcript_counts();
    let genes: Vec<usize> = (0..COUNT_GENES).map(|g| col(&t, &format!("T{:02}", g + 1))).collect();
    let reports = vec![
        SavedReport::Screening {
            request: ScreeningRequest {
                model: ScreeningModel::NegativeBinomial,
                ..ScreeningRequest::new(genes.clone(), vec![col(&t, "Treatment"), col(&t, "Batch")])
            },
        },
        SavedReport::Screening {
            request: ScreeningRequest {
                model: ScreeningModel::NegativeBinomial,
                subgroup: Some(col(&t, "Tissue")),
                ..ScreeningRequest::new(genes, vec![col(&t, "Treatment")])
            },
        },
    ];
    Sample { table: t, reports }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::RowStateManager;

    /// The treatment screen finds four of the five genes planted in both
    /// tissues (T03's 2-fold change is missed at q ≤ 0.05) and no others.
    #[test]
    fn screening_finds_the_planted_genes() {
        let s = transcript_counts_sample();
        let rows = RowStateManager::new(s.table.n_rows());
        let SavedReport::Screening { request } = &s.reports[0] else { panic!() };
        let a = crate::ScreeningAnalysis::run(&s.table, &rows, request).unwrap();
        let found: Vec<&str> = a.screens()[0]
            .responses
            .iter()
            .filter(|r| r.effects[0].as_ref().is_some_and(|e| e.q_value <= 0.05))
            .map(|r| r.name.as_str())
            .collect();
        // Four of the five genes that respond in both tissues, and nothing else.
        assert_eq!(found, ["T01", "T02", "T04", "T05"]);
        let SavedReport::Screening { request } = &s.reports[1] else { panic!() };
        let b = crate::ScreeningAnalysis::run(&s.table, &rows, request).unwrap();
        assert_eq!(b.screens().len(), 2);
        assert_eq!(b.screens()[0].level, "Liver");
    }
}
