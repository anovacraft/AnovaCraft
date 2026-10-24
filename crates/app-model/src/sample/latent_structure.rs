//! Simulated samples for factor analysis and discriminant analysis. Each is
//! drawn from a stated model with a fixed seed.

use super::{Sample, col, round_to, table, text};
use crate::project::SavedReport;
use crate::{DiscriminantRequest, FactorRequest, PriorChoice, ValidationMethod};
use stat_core::rng::Rng;
use stat_data::{Column, DataTable};
use stat_multivariate::{DiscriminantRule, Extraction, Rotation};

/// Test names of the ability-test sample, with the latent factor each
/// mainly measures (0 verbal, 1 quantitative, 2 spatial) and its loading.
pub const ABILITY_TESTS: [(&str, usize, f64); 9] = [
    ("Vocabulary", 0, 0.85),
    ("Reading comprehension", 0, 0.8),
    ("Sentence completion", 0, 0.75),
    ("Arithmetic", 1, 0.8),
    ("Number series", 1, 0.75),
    ("Word problems", 1, 0.65),
    ("Mental rotation", 2, 0.8),
    ("Paper folding", 2, 0.75),
    ("Block design", 2, 0.7),
];

/// **Simulated** scores of 240 students on nine ability tests. Three
/// latent abilities (verbal, quantitative, spatial) are drawn as
/// correlated standard normals (correlation 0.3 between each pair, from a
/// shared general component); each test is `50 + 10·(λ·ability + 0.25·other
/// ability + e)` with the loading λ of [`ABILITY_TESTS`], a small cross-loading
/// on one other ability ("Word problems" on verbal, "Block design" on
/// quantitative) and unit-variance-completing noise, rounded to whole points.
#[must_use]
pub fn ability_tests() -> DataTable {
    let mut rng = Rng::new(0xFAC7_0125);
    let n = 240;
    let mut scores: Vec<Vec<f64>> = vec![Vec::with_capacity(n); ABILITY_TESTS.len()];
    let mut group = Vec::with_capacity(n);
    for i in 0..n {
        let g = rng.normal();
        let ability: Vec<f64> = (0..3).map(|_| 0.3_f64.sqrt() * g + 0.7_f64.sqrt() * rng.normal()).collect();
        for (j, (_, f, l)) in ABILITY_TESTS.iter().enumerate() {
            let cross = match j {
                5 => 0.25 * ability[0],
                8 => 0.25 * ability[1],
                _ => 0.0,
            };
            let noise = (1.0 - l * l).max(0.1).sqrt();
            scores[j].push(round_to(50.0 + 10.0 * (l * ability[*f] + cross + noise * rng.normal()), 0));
        }
        group.push(if i % 2 == 0 { "Morning" } else { "Afternoon" }.to_owned());
    }
    let mut columns = vec![Column::numeric("Student", (1..=n).map(|i| i as f64).collect())];
    columns[0].level = stat_data::MeasurementLevel::Nominal;
    columns.push(text("Session", group));
    for ((name, _, _), v) in ABILITY_TESTS.iter().zip(scores) {
        columns.push(Column::numeric(*name, v));
    }
    table("Ability tests (simulated)", columns)
}

pub(super) fn ability_tests_sample() -> Sample {
    let t = ability_tests();
    let tests: Vec<usize> = ABILITY_TESTS.iter().map(|(n, _, _)| col(&t, n)).collect();
    let reports = vec![
        SavedReport::FactorAnalysis { request: FactorRequest::new(tests.clone()) },
        SavedReport::FactorAnalysis {
            request: FactorRequest {
                factors: Some(3),
                extraction: Extraction::PrincipalAxis,
                rotation: Rotation::Promax,
                ..FactorRequest::new(tests)
            },
        },
    ];
    Sample { table: t, reports }
}

/// Origins of the honey sample and the mean of each measurement there:
/// moisture (%), fructose (%), glucose (%), electrical conductivity
/// (mS/cm), proline (mg/kg) and pH.
const HONEY_ORIGINS: [(&str, [f64; 6]); 3] = [
    ("Coast", [17.8, 39.5, 31.0, 0.45, 420.0, 3.9]),
    ("Valley", [17.0, 38.2, 32.4, 0.32, 360.0, 3.7]),
    ("Upland", [16.4, 40.8, 30.2, 0.62, 520.0, 4.2]),
];

/// **Simulated** composition of 180 honey samples, 60 from each of three
/// origins. Each measurement is its origin's mean (`HONEY_ORIGINS`) plus
/// noise with standard deviations 0.6, 1.2, 1.1, 0.08, 55 and 0.15; moisture
/// and the two sugars share a common component (correlation about 0.4), and
/// Upland samples vary more in conductivity and proline (1.5 times), so
/// the classes' covariances differ. A "Validation" column assigns 60% of
/// each origin to training, 20% to validation and 20% to test, in turn.
#[must_use]
pub fn honey_origins() -> DataTable {
    let mut rng = Rng::new(0x0040_41E7);
    let sd = [0.6, 1.2, 1.1, 0.08, 55.0, 0.15];
    let decimals = [1, 1, 1, 2, 0, 2];
    let mut values: Vec<Vec<f64>> = vec![Vec::new(); 6];
    let mut origin = Vec::new();
    let mut role = Vec::new();
    for i in 0..180 {
        let k = i % 3;
        let (name, mean) = HONEY_ORIGINS[k];
        let shared = rng.normal();
        for j in 0..6 {
            let spread = if k == 2 && (j == 3 || j == 4) { 1.5 } else { 1.0 };
            let e = if j < 3 { 0.4_f64.sqrt() * shared + 0.6_f64.sqrt() * rng.normal() } else { rng.normal() };
            values[j].push(round_to(mean[j] + spread * sd[j] * e, decimals[j]));
        }
        origin.push(name.to_owned());
        role.push(["Training", "Training", "Training", "Validation", "Test"][(i / 3) % 5].to_owned());
    }
    let names = ["Moisture", "Fructose", "Glucose", "Conductivity", "Proline", "pH"];
    let mut columns = vec![text("Origin", origin)];
    columns.extend(names.iter().zip(values).map(|(n, v)| Column::numeric(*n, v)));
    columns.push(text("Validation", role));
    table("Honey origins (simulated)", columns)
}

pub(super) fn honey_origins_sample() -> Sample {
    let t = honey_origins();
    let measures: Vec<usize> =
        ["Moisture", "Fructose", "Glucose", "Conductivity", "Proline", "pH"].iter().map(|n| col(&t, n)).collect();
    let validation = ValidationMethod::Column(col(&t, "Validation"));
    let reports = vec![
        SavedReport::DiscriminantAnalysis {
            request: DiscriminantRequest {
                validation: validation.clone(),
                ..DiscriminantRequest::new(col(&t, "Origin"), measures.clone())
            },
        },
        SavedReport::DiscriminantAnalysis {
            request: DiscriminantRequest {
                rule: DiscriminantRule::Quadratic,
                priors: PriorChoice::Equal,
                validation,
                ..DiscriminantRequest::new(col(&t, "Origin"), measures)
            },
        },
    ];
    Sample { table: t, reports }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::RowStateManager;

    #[test]
    fn ability_tests_show_three_factors() {
        let s = ability_tests_sample();
        let rows = RowStateManager::new(s.table.n_rows());
        let SavedReport::FactorAnalysis { request } = &s.reports[0] else { panic!() };
        let a = crate::FactorAnalysisReport::run(&s.table, &rows, request).unwrap();
        assert_eq!(a.fit().n_factors(), 3, "three eigenvalues above 1");
        let p = &a.fit().pattern;
        // Each block of three tests loads mainly on one factor.
        for block in 0..3 {
            let main: Vec<usize> = (0..3)
                .map(|j| {
                    (0..3).max_by(|&x, &y| p[3 * block + j][x].abs().total_cmp(&p[3 * block + j][y].abs())).unwrap()
                })
                .collect();
            assert!(main.iter().all(|&m| m == main[0]), "block {block}: {main:?}");
        }
        assert!(a.fit().fit_test.unwrap().p_value.unwrap() > 0.01, "three factors fit");
    }

    #[test]
    fn honey_origins_are_separated() {
        let s = honey_origins_sample();
        let rows = RowStateManager::new(s.table.n_rows());
        for r in &s.reports {
            let SavedReport::DiscriminantAnalysis { request } = r else { panic!() };
            let a = crate::DiscriminantAnalysis::run(&s.table, &rows, request).unwrap();
            let crate::prediction::Predictions::Probabilities(p) = crate::prediction::Predictor::predictions(&a) else {
                panic!()
            };
            let codes = match crate::prediction::Predictor::response(&a) {
                crate::prediction::Response::Categorical { codes, .. } => codes.clone(),
                crate::prediction::Response::Continuous(_) => panic!(),
            };
            let test: Vec<usize> = (0..180).filter(|i| (i / 3) % 5 == 4).collect();
            let wrong =
                test.iter().filter(|&&r| stat_predictive::measures::most_probable(&p[r]) != codes[r].unwrap()).count();
            assert!(wrong <= 6, "{wrong} of {} test rows misclassified", test.len());
        }
    }
}
