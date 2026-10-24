//! The sample for multivariate adaptive regression splines.

use super::{Sample, col, round_to, table, text};
use crate::project::SavedReport;
use crate::{BoostRequest, MarsRequest, ModelComparisonRequest, ProfilerOptions, ValidationMethod};
use stat_data::{Column, DataTable, RowStateManager};

/// **Simulated** fuel consumption (L/100 km) of 500 test drives: speed
/// uniform on 20–130 km/h, load on 0–100% of the rated payload, ambient
/// temperature on −10–35 °C, tyre pressure on 1.8–2.8 bar (no effect) and
/// fuel type petrol, diesel or hybrid. Consumption is
/// 6 + 0.07·max(0, S − 80) + 0.05·max(0, 50 − S) + 0.025·L +
/// 0.0008·max(0, S − 80)·L + 0.06·max(0, 5 − T), less 0.8 for diesel
/// and, for hybrids, `1.2 + 0.04·max(0, 70 − S)`, plus normal noise (sd 0.3).
/// Roles: 60% training, 25% validation, 15% test, at random.
#[must_use]
pub fn fuel_consumption() -> DataTable {
    let mut rng = stat_core::rng::Rng::new(0xF0E1_5EED);
    let n = 500;
    let mut cols: [Vec<f64>; 5] = Default::default();
    let (mut fuel, mut role) = (Vec::new(), Vec::new());
    for _ in 0..n {
        let s = round_to(20.0 + 110.0 * rng.next_f64(), 1);
        let l = round_to(100.0 * rng.next_f64(), 0);
        let t = round_to(-10.0 + 45.0 * rng.next_f64(), 1);
        let p = round_to(1.8 + rng.next_f64(), 2);
        let f = ["Petrol", "Diesel", "Hybrid"][rng.below(3)];
        let mut y = 6.0
            + 0.07 * (s - 80.0).max(0.0)
            + 0.05 * (50.0 - s).max(0.0)
            + 0.025 * l
            + 0.0008 * (s - 80.0).max(0.0) * l
            + 0.06 * (5.0 - t).max(0.0);
        y -= match f {
            "Diesel" => 0.8,
            "Hybrid" => 1.2 + 0.04 * (70.0 - s).max(0.0),
            _ => 0.0,
        };
        y += 0.3 * rng.normal();
        let u = rng.next_f64();
        for (c, v) in cols.iter_mut().zip([s, l, t, p, round_to(y, 2)]) {
            c.push(v);
        }
        fuel.push(f.to_owned());
        role.push(
            if u < 0.6 {
                "Training"
            } else if u < 0.85 {
                "Validation"
            } else {
                "Test"
            }
            .to_owned(),
        );
    }
    let [speed, load, temp, pressure, consumption] = cols;
    table(
        "Fuel consumption (simulated)",
        vec![
            Column::numeric("Speed (km/h)", speed),
            Column::numeric("Load (%)", load),
            Column::numeric("Ambient temperature (°C)", temp),
            Column::numeric("Tyre pressure (bar)", pressure),
            text("Fuel", fuel),
            Column::numeric("Consumption (L/100 km)", consumption),
            text("Validation", role),
        ],
    )
}

/// Additive and degree-2 MARS models sized on the validation rows, boosted
/// trees for comparison, and the saved predictions compared on the test rows.
pub(super) fn fuel_consumption_sample() -> Sample {
    let mut t = fuel_consumption();
    let c = |n: &str| col(&t, n);
    let predictors =
        vec![c("Speed (km/h)"), c("Load (%)"), c("Ambient temperature (°C)"), c("Tyre pressure (bar)"), c("Fuel")];
    let response = c("Consumption (L/100 km)");
    let roles = ValidationMethod::Column(c("Validation"));
    let interactions =
        MarsRequest { max_degree: 2, validation: roles.clone(), ..MarsRequest::new(response, predictors.clone()) };
    let additive = MarsRequest { max_degree: 1, ..interactions.clone() };
    let boosted = BoostRequest { validation: roles, ..BoostRequest::new(response, predictors) };
    let rows = RowStateManager::new(t.n_rows());
    let predicted = |cols: stat_core::Result<Vec<crate::ReportColumn>>| {
        cols.ok().and_then(|cols| cols.into_iter().find(|k| k.key == "predicted")).map(|k| k.values)
    };
    let saved = [
        (
            "Predicted (adaptive splines, degree 2)",
            predicted(crate::MarsAnalysis::run(&t, &rows, &interactions).map(|a| a.saveable_columns())),
        ),
        (
            "Predicted (adaptive splines, additive)",
            predicted(crate::MarsAnalysis::run(&t, &rows, &additive).map(|a| a.saveable_columns())),
        ),
        (
            "Predicted (boosted trees)",
            predicted(crate::BoostAnalysis::run(&t, &rows, &boosted).map(|a| a.saveable_columns())),
        ),
    ];
    let profilers = ProfilerOptions::default();
    let mut reports = vec![
        SavedReport::Mars { request: interactions, profilers },
        SavedReport::Mars { request: additive, profilers },
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
                response: col(&t, "Consumption (L/100 km)"),
                models,
                event: None,
                validation: Some(col(&t, "Validation")),
            },
        });
    }
    Sample { reports, table: t }
}

#[cfg(test)]
mod tests {
    use crate::project::SavedReport;
    use crate::report::{SectionContent, SectionKind};
    use stat_data::RowStateManager;

    /// The degree-2 model finds the speed knots and the speed × load
    /// interaction, ignores tyre pressure, and beats the additive model on
    /// the test rows; the formula column reproduces the predictions.
    #[test]
    fn fuel_consumption_mars() {
        let s = super::fuel_consumption_sample();
        assert_eq!(s.reports.len(), 4);
        let rows = RowStateManager::new(s.table.n_rows());
        let SavedReport::Mars { request, .. } = &s.reports[0] else { panic!() };
        let a = crate::MarsAnalysis::run(&s.table, &rows, request).unwrap();
        let sections = a.sections();
        let kinds: Vec<SectionKind> = sections.iter().map(|s| s.kind).collect();
        assert_eq!(
            kinds,
            [
                SectionKind::MarsSummary,
                SectionKind::FitByRole,
                SectionKind::MarsBasis,
                SectionKind::VariableImportance,
                SectionKind::MarsSelection,
                SectionKind::MarsForward,
            ]
        );
        let SectionContent::Table(fit) = &sections[1].content else { panic!() };
        assert_eq!(fit.rows.iter().map(|r| r[0].as_str()).collect::<Vec<_>>(), ["Training", "Validation", "Test"]);
        let test_rmse: f64 = fit.rows[2][2].parse().unwrap();
        assert!(test_rmse < 0.45, "test RMSE {test_rmse}");
        // Speed has knots near 50 and 80 km/h.
        let m = a.model();
        let knots: Vec<f64> = m
            .selected
            .iter()
            .flat_map(|&t| m.basis[t].factors.clone())
            .filter_map(|f| match f {
                stat_predictive::mars::Factor::Hinge { feature: 0, knot, .. } => Some(knot),
                _ => None,
            })
            .collect();
        for k in [50.0, 80.0] {
            assert!(knots.iter().any(|t| (t - k).abs() < 6.0), "no knot near {k}: {knots:?}");
        }
        assert!(m.selected.iter().any(|&t| m.basis[t].degree() == 2), "an interaction term");
        let SectionContent::Table(imp) = &sections[3].content else { panic!() };
        assert_eq!(imp.rows[0][0], "Speed (km/h)");
        let pressure = imp.rows.iter().find(|r| r[0] == "Tyre pressure (bar)").unwrap();
        assert!(pressure[1].parse::<usize>().unwrap() <= 1, "{pressure:?}");
        // Saved columns: values, formula, roles; the formula computes the same values.
        let cols = a.saveable_columns();
        assert_eq!(
            cols.iter().map(|c| c.key.as_str()).collect::<Vec<_>>(),
            ["predicted", "prediction_formula", "validation"]
        );
        let formula = cols[1].formula.clone().unwrap();
        assert!(formula.contains("max(col(\"Speed (km/h)\") - "), "{formula}");
        let computed = crate::formula::evaluate(&s.table, None, &formula, 0..s.table.n_rows()).unwrap().values;
        for (r, (x, y)) in computed.iter().zip(&cols[0].values).enumerate() {
            let crate::formula::FormulaValue::Number(x) = x else { panic!("row {r}: {x:?}") };
            assert!((x - y).abs() < 1e-9 * y.abs().max(1.0), "row {r}: {x} vs {y}");
        }
        // The additive model fits worse on the test rows.
        let SavedReport::Mars { request, .. } = &s.reports[1] else { panic!() };
        let b = crate::MarsAnalysis::run(&s.table, &rows, request).unwrap();
        let SectionContent::Table(fit_b) = &b.sections()[1].content else { panic!() };
        assert!(fit_b.rows[2][2].parse::<f64>().unwrap() > test_rmse);
        let SavedReport::ModelComparison { request } = &s.reports[3] else { panic!() };
        assert_eq!(request.models.len(), 3);
    }
}
