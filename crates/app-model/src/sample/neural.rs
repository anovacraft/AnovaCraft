//! The sample for neural networks.

use super::{Sample, col, round_to, table, text};
use crate::project::SavedReport;
use crate::{
    BoostRequest, ModelComparisonRequest, ModelSearchRequest, NeuralRequest, ProfilerOptions, ValidationMethod,
};
use stat_data::{Column, DataTable, RowStateManager};

/// The latent strength of a polymer batch (MPa) before noise.
fn strength(temp: f64, pressure: f64, catalyst: f64, time: f64, supplier: &str) -> f64 {
    let t = (temp - 180.0) / 12.0;
    let base = 42.0 + 6.0 * t.tanh() - 0.012 * (pressure - 55.0).powi(2) + 3.0 * (1.0 - (-time / 20.0).exp());
    // Catalyst helps most at low temperature (a smooth interaction).
    let catalyst_gain = 4.0 * catalyst * (1.0 - t.tanh()) / 2.0;
    base + catalyst_gain
        + match supplier {
            "B" => -1.5,
            "C" => 1.0,
            _ => 0.0,
        }
}

/// **Simulated** tensile strength (MPa) of 600 polymer batches: cure
/// temperature uniform on 150–210 °C, pressure on 30–80 bar, catalyst on
/// 0–2%, mixing time on 5–60 min and resin supplier A, B or C. With
/// `t = (T − 180) / 12`, strength is
/// `42 + 6·tanh t − 0.012·(P − 55)² + 3·(1 − e^(−M/20)) + 2·C·(1 − tanh t)`,
/// less 1.5 for supplier B and plus 1 for C, with normal noise (sd 0.8).
/// The grade is Reject below 40 MPa, Premium at 47 MPa or more and Standard
/// between, judged on the strength plus a further measurement error (sd 1).
/// Roles: 60% training, 20% validation, 20% test, at random.
#[must_use]
pub fn polymer_batches() -> DataTable {
    let mut rng = stat_core::rng::Rng::new(0x9E7_B47C);
    let n = 600;
    let mut cols: [Vec<f64>; 5] = Default::default();
    let (mut supplier, mut grade, mut role) = (Vec::new(), Vec::new(), Vec::new());
    for _ in 0..n {
        let temp = round_to(150.0 + 60.0 * rng.next_f64(), 1);
        let pressure = round_to(30.0 + 50.0 * rng.next_f64(), 1);
        let catalyst = round_to(2.0 * rng.next_f64(), 2);
        let time = round_to(5.0 + 55.0 * rng.next_f64(), 0);
        let s = ["A", "B", "C"][rng.below(3)];
        let y = strength(temp, pressure, catalyst, time, s) + 0.8 * rng.normal();
        let judged = y + rng.normal();
        let u = rng.next_f64();
        for (c, v) in cols.iter_mut().zip([temp, pressure, catalyst, time, round_to(y, 2)]) {
            c.push(v);
        }
        supplier.push(s.to_owned());
        grade.push(
            if judged < 40.0 {
                "Reject"
            } else if judged < 47.0 {
                "Standard"
            } else {
                "Premium"
            }
            .to_owned(),
        );
        role.push(
            if u < 0.6 {
                "Training"
            } else if u < 0.8 {
                "Validation"
            } else {
                "Test"
            }
            .to_owned(),
        );
    }
    let [temp, pressure, catalyst, time, strength] = cols;
    table(
        "Polymer batches (simulated)",
        vec![
            Column::numeric("Cure temperature (°C)", temp),
            Column::numeric("Pressure (bar)", pressure),
            Column::numeric("Catalyst (%)", catalyst),
            Column::numeric("Mixing time (min)", time),
            text("Supplier", supplier),
            Column::numeric("Tensile strength (MPa)", strength),
            text("Grade", grade),
            text("Validation", role),
        ],
    )
}

/// A neural network of strength and one of grade, both with the penalty
/// tuned on the validation rows, boosted trees of strength, and the saved
/// strength predictions compared on the test rows, and a search over every
/// applicable model of strength.
pub(super) fn polymer_batches_sample() -> Sample {
    let mut t = polymer_batches();
    let c = |n: &str| col(&t, n);
    let predictors =
        vec![c("Cure temperature (°C)"), c("Pressure (bar)"), c("Catalyst (%)"), c("Mixing time (min)"), c("Supplier")];
    let roles = ValidationMethod::Column(c("Validation"));
    let strength = NeuralRequest {
        hidden: vec![5],
        validation: roles.clone(),
        ..NeuralRequest::new(c("Tensile strength (MPa)"), predictors.clone())
    };
    let grade = NeuralRequest { hidden: vec![6, 3], ..NeuralRequest { response: c("Grade"), ..strength.clone() } };
    let search = ModelSearchRequest {
        validation: roles.clone(),
        ..ModelSearchRequest::new(c("Tensile strength (MPa)"), predictors.clone())
    };
    let boosted = BoostRequest { validation: roles, ..BoostRequest::new(c("Tensile strength (MPa)"), predictors) };
    let rows = RowStateManager::new(t.n_rows());
    let predicted = |cols: stat_core::Result<Vec<crate::ReportColumn>>| {
        cols.ok().and_then(|cols| cols.into_iter().find(|k| k.key == "predicted")).map(|k| k.values)
    };
    let saved = [
        (
            "Predicted (neural network)",
            predicted(crate::NeuralAnalysis::run(&t, &rows, &strength).map(|a| a.saveable_columns())),
        ),
        (
            "Predicted (boosted trees)",
            predicted(crate::BoostAnalysis::run(&t, &rows, &boosted).map(|a| a.saveable_columns())),
        ),
    ];
    let profilers = ProfilerOptions::default();
    let mut reports = vec![
        SavedReport::NeuralNetwork { request: strength, profilers },
        SavedReport::NeuralNetwork { request: grade, profilers },
        SavedReport::BoostedTrees { request: boosted, profilers },
    ];
    let mut models = Vec::new();
    for (name, values) in saved {
        let at = t.n_cols();
        if let Some(v) = values {
            if t.insert_column(at, Column::numeric(name, v)).is_ok() {
                models.push(at);
            }
        }
    }
    if !models.is_empty() {
        reports.push(SavedReport::ModelComparison {
            request: ModelComparisonRequest {
                response: col(&t, "Tensile strength (MPa)"),
                models,
                event: None,
                validation: Some(col(&t, "Validation")),
            },
        });
    }
    reports.push(SavedReport::ModelSearch { request: search });
    Sample { reports, table: t }
}

#[cfg(test)]
mod tests {
    use crate::project::SavedReport;
    use crate::report::{SectionContent, SectionKind};
    use stat_data::RowStateManager;

    /// The strength network fits the smooth surface closely and beats the
    /// boosted trees on the test rows; the grade network classifies well.
    #[test]
    fn polymer_networks() {
        let s = super::polymer_batches_sample();
        assert_eq!(s.reports.len(), 5);
        let rows = RowStateManager::new(s.table.n_rows());
        let SavedReport::NeuralNetwork { request, .. } = &s.reports[0] else { panic!() };
        let a = crate::NeuralAnalysis::run(&s.table, &rows, request).unwrap();
        let sections = a.sections();
        let fit = sections.iter().find(|x| x.kind == SectionKind::FitByRole).unwrap();
        let SectionContent::Table(fit) = &fit.content else { panic!() };
        let test_r2 = fit.rows[2][4].parse::<f64>().unwrap();
        assert!(test_r2 > 0.9, "test R² {test_r2}");
        let SavedReport::NeuralNetwork { request, .. } = &s.reports[1] else { panic!() };
        let g = crate::NeuralAnalysis::run(&s.table, &rows, request).unwrap();
        let sections = g.sections();
        let fit = sections.iter().find(|x| x.kind == SectionKind::FitByRole).unwrap();
        let SectionContent::Table(fit) = &fit.content else { panic!() };
        let misclassified = fit.rows[2][2].parse::<f64>().unwrap();
        assert!(misclassified < 0.25, "test misclassification {misclassified}");
        let SavedReport::ModelComparison { request } = &s.reports[3] else { panic!() };
        let m = crate::ModelComparison::run(&s.table, &rows, request).unwrap();
        let SectionContent::Table(table) = &m.sections()[0].content else { panic!() };
        // Rows: a role label on its first model's row, then the other models.
        let test = table.rows.iter().position(|r| r[0] == "Test").unwrap();
        let rmse = |name: &str| -> f64 {
            let row = table.rows[test..].iter().find(|r| r[1].contains(name)).unwrap();
            row[3].parse().unwrap()
        };
        assert!(rmse("neural") < rmse("boosted"), "{table:?}");
        // The search ranks the smooth network above the trees.
        let SavedReport::ModelSearch { request } = &s.reports[4] else { panic!() };
        let search = crate::ModelSearch::run(&s.table, &rows, request).unwrap();
        let ranked = search.ranked();
        let at = |k: crate::ModelKind| ranked.iter().position(|&r| r == k).unwrap();
        assert!(at(crate::ModelKind::NeuralNetwork) < at(crate::ModelKind::DecisionTree), "{ranked:?}");
    }
}
