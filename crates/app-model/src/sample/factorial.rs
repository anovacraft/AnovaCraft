//! Samples for the factorial analysis: a published unreplicated 2⁴
//! (Montgomery's filtration rate experiment) and a simulated replicated 2³
//! with a dispersion effect.

use super::{Sample, col, table};
use crate::project::SavedReport;
use crate::{FactorialAnalysisRequest, MeansSource};
use stat_data::{Column, DataTable};

/// Coded settings (−1, +1) of factor `j` in a `2^k` in standard order (first
/// factor fastest), each run repeated `reps` times.
fn coded(j: usize, k: usize, reps: usize) -> Vec<f64> {
    (0..(1usize << k) * reps).map(|i| if ((i / reps) >> j) & 1 == 1 { 1.0 } else { -1.0 }).collect()
}

/// Montgomery, *Design and Analysis of Experiments*, Example 6.2: the
/// filtration rate (gallons per hour) of a chemical product in an
/// unreplicated 2⁴ factorial in temperature (A), pressure (B), formaldehyde
/// concentration (C) and stirring rate (D), in standard order, factors in
/// coded units. Published data.
#[must_use]
pub fn filtration_rate() -> DataTable {
    let y = vec![45.0, 71.0, 48.0, 65.0, 68.0, 60.0, 80.0, 65.0, 43.0, 100.0, 45.0, 104.0, 75.0, 86.0, 70.0, 96.0];
    let mut columns: Vec<Column> = ["Temperature", "Pressure", "Concentration", "Stirring rate"]
        .iter()
        .enumerate()
        .map(|(j, n)| Column::numeric(*n, coded(j, 4, 1)))
        .collect();
    columns.push(Column::numeric("Filtration rate", y));
    table("Filtration rate (Montgomery Ex. 6.2)", columns)
}

pub(super) fn filtration_rate_sample() -> Sample {
    let t = filtration_rate();
    let factors: Vec<usize> =
        ["Temperature", "Pressure", "Concentration", "Stirring rate"].iter().map(|n| col(&t, n)).collect();
    let reports = vec![SavedReport::FactorialAnalysis {
        request: FactorialAnalysisRequest::new(&factors, col(&t, "Filtration rate")),
    }];
    Sample { table: t, reports }
}

/// **Simulated** coating thickness from a 2³ factorial in line speed (A),
/// viscosity (B) and oven temperature (C), three replicates of each run:
/// thickness = 50 + 5A − 3B + 2AB + ε with ε ~ N(0, σ²), σ = exp(0.4 + 0.6C)
/// (so the oven temperature changes the variability, not the mean), drawn
/// once with seed 20261016 and rounded to 0.1 (the same numbers as
/// `crates/stat-doe/tests/data/factorial_reference.py`).
#[must_use]
pub fn coating_thickness() -> DataTable {
    let y = vec![
        48.9, 50.8, 50.0, 54.4, 55.0, 55.9, 39.3, 39.1, 39.3, 52.9, 53.2, 55.8, 50.5, 49.0, 47.5, 52.0, 48.2, 55.2,
        38.5, 46.0, 40.1, 51.3, 51.6, 59.2,
    ];
    let mut columns: Vec<Column> = ["Line speed", "Viscosity", "Oven temperature"]
        .iter()
        .enumerate()
        .map(|(j, n)| Column::numeric(*n, coded(j, 3, 3)))
        .collect();
    columns.push(Column::numeric("Thickness", y));
    table("Coating thickness (simulated)", columns)
}

pub(super) fn coating_thickness_sample() -> Sample {
    let t = coating_thickness();
    let factors: Vec<usize> = ["Line speed", "Viscosity", "Oven temperature"].iter().map(|n| col(&t, n)).collect();
    let mut request = FactorialAnalysisRequest::new(&factors, col(&t, "Thickness"));
    request.means = MeansSource::Model;
    Sample { table: t, reports: vec![SavedReport::FactorialAnalysis { request }] }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::SectionContent;
    use stat_data::RowStateManager;

    /// The sample reproduces Montgomery's published effects.
    #[test]
    fn filtration_sample_effects() {
        let s = filtration_rate_sample();
        let SavedReport::FactorialAnalysis { request } = &s.reports[0] else { panic!() };
        let a = crate::FactorialAnalysis::run(&s.table, &RowStateManager::new(16), request).unwrap();
        let e: Vec<f64> = a.effects().values();
        for (got, want) in e.iter().zip([21.625, 3.125, 9.875, 14.625, 0.125, -18.125, 16.625]) {
            assert!((got - want).abs() < 1e-10, "{got} vs {want}");
        }
    }

    /// The replicated sample has t tests, and its dispersion table finds the
    /// oven temperature's effect on ln s as the largest.
    #[test]
    fn coating_sample_dispersion() {
        let s = coating_thickness_sample();
        let SavedReport::FactorialAnalysis { request } = &s.reports[0] else { panic!() };
        let a = crate::FactorialAnalysis::run(&s.table, &RowStateManager::new(24), request).unwrap();
        assert!(a.effects().test.is_some());
        let sections = a.sections();
        let d = sections.iter().find(|s| s.title == "Dispersion Effects (ln s)").unwrap();
        let SectionContent::Table(t) = &d.content else { panic!() };
        assert_eq!(t.rows[2][0], "Oven temperature");
        assert_eq!(t.rows[2][1], "1.6381");
    }
}
