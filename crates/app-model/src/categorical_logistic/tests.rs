use super::*;
use crate::formula::{FormulaValue, evaluate};
use crate::project::{report_from_json, report_to_json};
use crate::sample::{commute_modes, service_ratings};

fn col(t: &DataTable, name: &str) -> usize {
    t.find(name).unwrap()
}

fn ordinal_request(t: &DataTable) -> CategoricalLogisticRequest {
    CategoricalLogisticRequest {
        order: crate::sample::SERVICE_RATING_SCALE.iter().map(|s| (*s).to_owned()).collect(),
        ..CategoricalLogisticRequest::new(
            CategoricalModel::Ordinal,
            col(t, "Rating"),
            vec![col(t, "Wait (min)"), col(t, "Channel")],
        )
    }
}

fn nominal_request(t: &DataTable) -> CategoricalLogisticRequest {
    CategoricalLogisticRequest {
        reference: Some("Car".to_owned()),
        ..CategoricalLogisticRequest::new(
            CategoricalModel::Nominal,
            col(t, "Mode"),
            vec![col(t, "Distance (km)"), col(t, "Age")],
        )
    }
}

fn table_of(section: &ReportSection) -> &TextTable {
    let SectionContent::Table(t) = &section.content else { panic!("{} is not a table", section.title) };
    t
}

fn find(sections: &[ReportSection], kind: SectionKind) -> &ReportSection {
    sections.iter().find(|s| s.kind == kind).unwrap_or_else(|| panic!("no {kind:?} section"))
}

/// Every formula column reproduces the saved values on every row.
fn check_formulas(t: &DataTable, a: &CategoricalLogisticAnalysis) {
    let cols = a.saveable_columns();
    assert_eq!(cols.len(), a.categories().len() + 1);
    for c in &cols {
        let formula = c.formula.as_ref().unwrap_or_else(|| panic!("{} has no formula", c.name));
        let eval = evaluate(t, None, formula, 0..t.n_rows()).unwrap_or_else(|e| panic!("{e}\n{formula}"));
        for (r, v) in eval.values.iter().enumerate() {
            match (&c.text, v) {
                (Some(text), FormulaValue::Text(got)) => assert_eq!(got, &text[r], "row {r} of {}", c.name),
                (None, FormulaValue::Number(got)) => {
                    assert!((got - c.values[r]).abs() < 1e-12, "row {r} of {}: {got} vs {}", c.name, c.values[r]);
                }
                other => panic!("row {r} of {}: {other:?}", c.name),
            }
        }
    }
    // The probabilities sum to one.
    for r in 0..t.n_rows() {
        let total: f64 = cols[..cols.len() - 1].iter().map(|c| c.values[r]).sum();
        assert!((total - 1.0).abs() < 1e-12);
    }
}

#[test]
fn ordinal_report_on_the_ratings_sample() {
    let t = service_ratings();
    let rows = RowStateManager::new(t.n_rows());
    let a = CategoricalLogisticAnalysis::run(&t, &rows, &ordinal_request(&t)).unwrap();
    assert_eq!(a.title(), "Ordinal Logistic Regression: Rating ~ Wait (min) + Channel");
    assert_eq!(a.categories(), ["Poor", "Fair", "Good", "Very good", "Excellent"]);
    let fit = a.ordinal_fit().unwrap();
    assert_eq!(fit.thresholds.len(), 4);
    assert!(fit.slopes[0].estimate < 0.0, "longer waits lower the rating");
    let s = a.sections();
    let kinds: Vec<SectionKind> = s.iter().map(|x| x.kind).collect();
    assert_eq!(
        kinds,
        [
            SectionKind::ModelSummary,
            SectionKind::EffectPareto,
            SectionKind::LikelihoodRatioTests,
            SectionKind::GlmEstimates,
            SectionKind::OddsRatios,
            SectionKind::ParallelLinesTest,
            SectionKind::ConfusionMatrix,
            SectionKind::ClassProbabilityPlot,
            SectionKind::PredictionFormula,
        ]
    );
    let est = table_of(find(&s, SectionKind::GlmEstimates));
    assert_eq!(est.cell(0, "Term"), Some("Threshold Poor | Fair"));
    assert_eq!(est.cell(5, "Term"), Some("Channel: Chat"));
    assert!(find(&s, SectionKind::GlmEstimates).title.contains("reference levels: Channel = Branch"));
    let lr = table_of(find(&s, SectionKind::LikelihoodRatioTests));
    assert_eq!(lr.cell(1, "Effect"), Some("Channel"));
    assert_eq!(lr.cell(1, "DF"), Some("2"));
    // The LR test of Channel equals the drop in 2ℓ when it is left out.
    let without = CategoricalLogisticAnalysis::run(
        &t,
        &rows,
        &CategoricalLogisticRequest { predictors: vec![col(&t, "Wait (min)")], ..ordinal_request(&t) },
    )
    .unwrap();
    let chi = 2.0 * (fit.statistics.log_likelihood - without.ordinal_fit().unwrap().statistics.log_likelihood);
    assert!((a.term_tests()[1].2 - chi).abs() < 1e-9);
    let odds = table_of(find(&s, SectionKind::OddsRatios));
    let or: f64 = odds.cell(0, "Odds ratio").unwrap().parse().unwrap();
    assert!((or - fit.slopes[0].estimate.exp()).abs() < 1e-5);
    let parallel = table_of(find(&s, SectionKind::ParallelLinesTest));
    assert_eq!(parallel.cell(0, "DF"), Some("9"), "3 slopes × (5 − 2)");
    let confusion = table_of(find(&s, SectionKind::ConfusionMatrix));
    assert_eq!(confusion.rows.len(), 5);
    let plot = find(&s, SectionKind::ClassProbabilityPlot);
    assert!(plot.title.starts_with("Predicted Probabilities by Wait (min)"));
    let SectionContent::Plot(p) = &plot.content else { panic!() };
    let groups = p.groups.as_ref().unwrap();
    assert_eq!(groups.lines.len(), 5);
    assert_eq!(p.points.len(), 150);
    // At every wait the five curves sum to one.
    for i in 0..groups.lines[0].len() {
        let total: f64 = groups.lines.iter().map(|l| l[i][1]).sum();
        assert!((total - 1.0).abs() < 1e-12);
    }
    check_formulas(&t, &a);
    let cols = a.saveable_columns();
    assert_eq!(cols[0].key, "probability_1");
    assert_eq!(cols[0].name, "Prob Rating = Poor");
    assert_eq!(cols[5].key, "most_likely");
    assert_eq!(cols[5].level, stat_data::MeasurementLevel::Ordinal);
}

#[test]
fn nominal_report_on_the_commute_sample() {
    let t = commute_modes();
    let rows = RowStateManager::new(t.n_rows());
    let a = CategoricalLogisticAnalysis::run(&t, &rows, &nominal_request(&t)).unwrap();
    assert_eq!(a.title(), "Nominal Logistic Regression: Mode ~ Distance (km) + Age");
    assert_eq!(a.categories(), ["Bicycle", "Bus", "Car", "Walk"], "natural order");
    let fit = a.nominal_fit().unwrap();
    assert_eq!(fit.levels[fit.reference], "Car");
    let s = a.sections();
    assert!(s.iter().all(|x| x.kind != SectionKind::ParallelLinesTest));
    let est = table_of(find(&s, SectionKind::GlmEstimates));
    assert_eq!(est.rows.len(), 9, "three equations of three coefficients");
    assert_eq!(est.cell(0, "Category"), Some("Bicycle"));
    assert_eq!(est.cell(0, "Term"), Some("Intercept"));
    assert_eq!(est.cell(3, "Category"), Some("Bus"));
    let odds = find(&s, SectionKind::OddsRatios);
    assert!(odds.title.contains("against Car"));
    assert_eq!(table_of(odds).rows.len(), 6);
    let lr = table_of(find(&s, SectionKind::LikelihoodRatioTests));
    assert_eq!(lr.cell(0, "DF"), Some("3"), "one column in three equations");
    let walk = fit.equations.iter().find(|e| fit.levels[e.level] == "Walk").unwrap();
    assert!(walk.coefficients[1].estimate < 0.0, "walking falls off with distance");
    check_formulas(&t, &a);
    // The first category is the default reference.
    let first = CategoricalLogisticRequest { reference: None, ..nominal_request(&t) };
    let b = CategoricalLogisticAnalysis::run(&t, &rows, &first).unwrap();
    assert_eq!(b.nominal_fit().unwrap().reference, 0);
    let (la, lb) = (fit.statistics.log_likelihood, b.nominal_fit().unwrap().statistics.log_likelihood);
    assert!((la - lb).abs() < 1e-9 * la.abs());
    let bad = CategoricalLogisticRequest { reference: Some("Tram".into()), ..nominal_request(&t) };
    assert!(CategoricalLogisticAnalysis::run(&t, &rows, &bad).is_err());
}

#[test]
fn interactions_exclusions_and_errors() {
    let t = service_ratings();
    let rows = RowStateManager::new(t.n_rows());
    let req = CategoricalLogisticRequest {
        effects: vec![vec![(0, 1)], vec![(1, 1)], vec![(0, 1), (1, 1)]],
        ..ordinal_request(&t)
    };
    let a = CategoricalLogisticAnalysis::run(&t, &rows, &req).unwrap();
    assert_eq!(a.title(), "Ordinal Logistic Regression: Rating ~ Wait (min) + Channel + Wait (min) × Channel");
    assert_eq!(a.term_tests()[2].1, 2);
    check_formulas(&t, &a);
    // An interaction without its main effects is noted.
    let req = CategoricalLogisticRequest { effects: vec![vec![(0, 1)], vec![(0, 1), (1, 1)]], ..ordinal_request(&t) };
    let a = CategoricalLogisticAnalysis::run(&t, &rows, &req).unwrap();
    assert!(a.sections().iter().any(|s| s.kind == SectionKind::HierarchyNote));
    // Excluding rows changes the fit; natural order sorts the labels as text.
    let mut states = RowStateManager::new(t.n_rows());
    states.set_excluded(0..20, true);
    let natural = CategoricalLogisticRequest { order: Vec::new(), ..ordinal_request(&t) };
    let b = CategoricalLogisticAnalysis::run(&t, &states, &natural).unwrap();
    assert_eq!(b.categories()[0], "Excellent");
    let SectionContent::Table(sum) = &b.sections()[0].content else { panic!() };
    assert_eq!(sum.cell(3, "Value"), Some("130"));
    assert_eq!(sum.cell(4, "Value"), Some("20"));
    let none = CategoricalLogisticRequest { predictors: Vec::new(), ..ordinal_request(&t) };
    assert!(CategoricalLogisticAnalysis::run(&t, &rows, &none).is_err());
    let numeric_response = CategoricalLogisticRequest {
        response: col(&t, "Channel"),
        predictors: vec![col(&t, "Wait (min)")],
        ..ordinal_request(&t)
    };
    // A text column with three categories is a valid response; its order
    // ignores the unknown labels of the request.
    let c = CategoricalLogisticAnalysis::run(&t, &rows, &numeric_response).unwrap();
    assert_eq!(c.categories(), ["Branch", "Chat", "Phone"]);
}

#[test]
fn project_json_round_trip() {
    for (t, req) in [
        (service_ratings(), None),
        (commute_modes(), None),
        (
            service_ratings(),
            Some(CategoricalLogisticRequest {
                effects: vec![vec![(0, 1)], vec![(1, 1)], vec![(0, 1), (1, 1)]],
                ..ordinal_request(&service_ratings())
            }),
        ),
    ] {
        let req =
            req.unwrap_or_else(|| if t.find("Mode").is_some() { nominal_request(&t) } else { ordinal_request(&t) });
        let saved = SavedReport::CategoricalLogistic { request: req.clone() };
        let json = report_to_json(&saved, &t);
        let back = report_from_json(&json, &t).unwrap();
        assert_eq!(back, saved, "{json:?}");
    }
    let t = commute_modes();
    let v = stat_data::parse_json(
        r#"{"analysis": "nominal-logistic", "response": "Mode", "predictors": ["Age"], "reference": "Bus"}"#,
    )
    .unwrap();
    let SavedReport::CategoricalLogistic { request } = report_from_json(&v, &t).unwrap() else { panic!() };
    assert_eq!(request.model, CategoricalModel::Nominal);
    assert_eq!(request.reference.as_deref(), Some("Bus"));
    assert!(request.effects.is_empty() && request.order.is_empty());
}
