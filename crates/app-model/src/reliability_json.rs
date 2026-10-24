//! Project-file (JSON) form of the reliability growth, repairable-systems
//! and test-plan reports (see [`crate::project`]). Columns are stored by
//! name; a missing setting keeps its default, so older files stay readable
//! when settings are added.
//!
//! ```text
//! {"analysis": "reliability-growth", "time": "Hours", "event": "Failure",
//!  "phase": "Phase", "end_time": 4000, "beta": "unbiased", "confidence": 0.95}
//! {"analysis": "repairable-systems", "system": "Unit", "time": "Age",
//!  "event": "Repair", "group": "Design", "variance": "robust",
//!  "limits": "normal", "confidence": 0.95}
//! {"analysis": "life-distributions", "layout": "intervals", "start": "From",
//!  "end": "To", "frequency": "Units", "group": "Supplier",
//!  "families": ["weibull", "lognormal"], "detail": "weibull",
//!  "confidence": 0.95, "probabilities": [0.1, 0.5], "times": [1000],
//!  "known_shape": 2}            (or "layout": "times", "time": "Hours",
//!                                "censor": "Censored", "censor_coding": "nonzero-censored")
//! {"analysis": "proportional-hazards", "time": "Weeks", "censor": "Relapse",
//!  "censor_coding": "nonzero-event", "predictors": ["Treatment", "log WBC"],
//!  "strata": "Sex", "ties": "efron", "ph_transform": "km", …}
//!      (see crate::proportional_hazards_analysis::to_json)
//! {"analysis": "survival-regression", "layout": "intervals", "start": "From",
//!  "end": "To", "frequency": "Units", "predictors": ["Temperature"],
//!  "distribution": "weibull", …}      (see crate::survival_regression_analysis::to_json)
//! {"analysis": "reliability-test-plan", "plan": "demonstration",
//!  "reliability": 0.9, "time": 1000, "confidence": 0.9, "shape": 1.5,
//!  "failures": 0, "test_time": 1000}            (or "units": 20)
//! {"analysis": "reliability-test-plan", "plan": "estimation",
//!  "distribution": "weibull", "spread": 2, "fraction_failing": 0.3,
//!  "percentile": 0.1, "confidence": 0.95, "precision": 1.5}
//! {"analysis": "reliability-test-plan", "plan": "accelerated",
//!  "distribution": "weibull", "spread": 2, "relationship": "arrhenius",
//!  "use_stress": 50, "high_stress": 120, "p_use": 0.001, "p_high": 0.9,
//!  "percentile": 0.1, "confidence": 0.95, "precision": 2,
//!  "levels": [[80, 0.5], [120, 0.5]]}
//! ```

use crate::life_distribution::{LifeDataLayout, LifeDistributionRequest};
use crate::project::{SavedReport, invalid, member, s};
use crate::recurrence::RecurrenceRequest;
use crate::reliability_growth::GrowthRequest;
use crate::survival::StatusCoding;
use crate::test_plan::{
    AcceleratedRequest, DemonstrationRequest, DemonstrationSolve, EstimationRequest, StressRelationship,
    TestPlanRequest,
};
use stat_core::Result;
use stat_core::distribution_fit::FitFamily;
use stat_data::{DataTable, JsonValue};
use stat_reliability::LifeDistribution;
use stat_reliability::growth::BetaEstimator;
use stat_reliability::repairable::{McfLimits, McfVariance};

fn object(members: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(members.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

fn distribution_key(d: LifeDistribution) -> &'static str {
    match d {
        LifeDistribution::Weibull => "weibull",
        LifeDistribution::Lognormal => "lognormal",
    }
}

/// The JSON form of a growth, repairable-systems or test-plan report
/// (`None` for any other report).
pub(crate) fn to_json(report: &SavedReport, table: &DataTable) -> Option<JsonValue> {
    let col = |i: usize| s(table.column(i).map_or("", |c| c.name.as_str()));
    let num = JsonValue::number;
    Some(match report {
        SavedReport::Growth { request: r } => {
            let mut m = vec![("analysis", s("reliability-growth")), ("time", col(r.time))];
            if let Some(c) = r.event {
                m.push(("event", col(c)));
            }
            if let Some(c) = r.phase {
                m.push(("phase", col(c)));
            }
            if let Some(t) = r.end_time {
                m.push(("end_time", num(t)));
            }
            let beta = match r.estimator {
                BetaEstimator::Unbiased => "unbiased",
                BetaEstimator::MaximumLikelihood => "maximum-likelihood",
            };
            m.push(("beta", s(beta)));
            m.push(("confidence", num(r.confidence)));
            object(m)
        }
        SavedReport::Recurrence { request: r } => {
            let mut m = vec![("analysis", s("repairable-systems")), ("system", col(r.system)), ("time", col(r.time))];
            if let Some(c) = r.event {
                m.push(("event", col(c)));
            }
            if let Some(c) = r.group {
                m.push(("group", col(c)));
            }
            let variance = match r.variance {
                McfVariance::Robust => "robust",
                McfVariance::Poisson => "poisson",
            };
            let limits = match r.limits {
                McfLimits::Normal => "normal",
                McfLimits::Log => "log",
            };
            m.extend([("variance", s(variance)), ("limits", s(limits)), ("confidence", num(r.confidence))]);
            object(m)
        }
        SavedReport::LifeDistribution { request: r } => {
            let mut m = vec![("analysis", s("life-distributions"))];
            match r.layout {
                LifeDataLayout::Times { time, censor, coding } => {
                    m.extend([("layout", s("times")), ("time", col(time))]);
                    if let Some(c) = censor {
                        m.push(("censor", col(c)));
                    }
                    let coding = match coding {
                        StatusCoding::NonzeroIsCensored => "nonzero-censored",
                        StatusCoding::NonzeroIsEvent => "nonzero-event",
                    };
                    m.push(("censor_coding", s(coding)));
                }
                LifeDataLayout::Intervals { start, end } => {
                    m.extend([("layout", s("intervals")), ("start", col(start)), ("end", col(end))]);
                }
            }
            if let Some(c) = r.frequency {
                m.push(("frequency", col(c)));
            }
            if let Some(c) = r.group {
                m.push(("group", col(c)));
            }
            m.push(("families", JsonValue::Array(r.families.iter().map(|f| s(f.key())).collect())));
            if let Some(f) = r.detail {
                m.push(("detail", s(f.key())));
            }
            m.push(("confidence", num(r.confidence)));
            m.push(("probabilities", JsonValue::Array(r.probabilities.iter().map(|&p| num(p)).collect())));
            m.push(("times", JsonValue::Array(r.times.iter().map(|&t| num(t)).collect())));
            if let Some(b) = r.known_shape {
                m.push(("known_shape", num(b)));
            }
            object(m)
        }
        SavedReport::ProportionalHazards { request, profilers } => {
            crate::proportional_hazards_analysis::to_json(request, profilers, table)
        }
        SavedReport::SurvivalRegression { request, profilers } => {
            crate::survival_regression_analysis::to_json(request, profilers, table)
        }
        SavedReport::TestPlan { request } => {
            let mut m = vec![("analysis", s("reliability-test-plan")), ("plan", s(request.key()))];
            match request {
                TestPlanRequest::Demonstration(d) => {
                    m.extend([
                        ("reliability", num(d.reliability)),
                        ("time", num(d.time)),
                        ("confidence", num(d.confidence)),
                        ("shape", num(d.shape)),
                        ("failures", num(d.failures as f64)),
                    ]);
                    match d.solve {
                        DemonstrationSolve::Units { test_time } => m.push(("test_time", num(test_time))),
                        DemonstrationSolve::TestTime { units } => m.push(("units", num(units as f64))),
                    }
                }
                TestPlanRequest::Estimation(e) => m.extend([
                    ("distribution", s(distribution_key(e.distribution))),
                    ("spread", num(e.spread)),
                    ("fraction_failing", num(e.fraction_failing)),
                    ("percentile", num(e.percentile)),
                    ("confidence", num(e.confidence)),
                    ("precision", num(e.precision)),
                ]),
                TestPlanRequest::Accelerated(a) => m.extend([
                    ("distribution", s(distribution_key(a.distribution))),
                    ("spread", num(a.spread)),
                    ("relationship", s(a.relationship.key())),
                    ("use_stress", num(a.use_stress)),
                    ("high_stress", num(a.high_stress)),
                    ("p_use", num(a.p_use)),
                    ("p_high", num(a.p_high)),
                    ("percentile", num(a.percentile)),
                    ("confidence", num(a.confidence)),
                    ("precision", num(a.precision)),
                    (
                        "levels",
                        JsonValue::Array(
                            a.levels.iter().map(|&(x, f)| JsonValue::Array(vec![num(x), num(f)])).collect(),
                        ),
                    ),
                ]),
            }
            object(m)
        }
        SavedReport::CompetingRisks { request } => request.to_json(table),
        SavedReport::Warranty { request } => request.to_json(table),
        SavedReport::LifeStress { request } => crate::life_stress::to_json(request, table),
        SavedReport::Degradation { request } => crate::degradation::to_json(request, table),
        _ => return None,
    })
}

/// Read a growth, repairable-systems or test-plan report from its JSON
/// form (`None` when `analysis` names another kind of report).
pub(crate) fn from_json(analysis: &str, v: &JsonValue, table: &DataTable) -> Option<Result<SavedReport>> {
    let col = |key: &str| -> Result<usize> {
        let name = member(v, key)?.as_str().ok_or_else(|| invalid(format!("report {key}: expected a column name")))?;
        table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")))
    };
    let optional_col = |key: &str| -> Result<Option<usize>> {
        match v.get(key) {
            None | Some(JsonValue::Null) => Ok(None),
            Some(_) => col(key).map(Some),
        }
    };
    let num = |key: &str| v.get(key).and_then(JsonValue::as_f64);
    let text = |key: &str| v.get(key).and_then(JsonValue::as_str);
    let distribution = || -> Result<LifeDistribution> {
        match text("distribution") {
            None | Some("weibull") => Ok(LifeDistribution::Weibull),
            Some("lognormal") => Ok(LifeDistribution::Lognormal),
            Some(other) => Err(invalid(format!("test plan: unknown distribution {other:?}"))),
        }
    };
    let result = match analysis {
        "reliability-growth" => (|| {
            let estimator = match text("beta") {
                None | Some("unbiased") => BetaEstimator::Unbiased,
                Some("maximum-likelihood") => BetaEstimator::MaximumLikelihood,
                Some(other) => return Err(invalid(format!("reliability growth: unknown β estimate {other:?}"))),
            };
            Ok(SavedReport::Growth {
                request: GrowthRequest {
                    time: col("time")?,
                    event: optional_col("event")?,
                    phase: optional_col("phase")?,
                    end_time: num("end_time"),
                    estimator,
                    confidence: num("confidence").unwrap_or(GrowthRequest::DEFAULT_CONFIDENCE),
                },
            })
        })(),
        "repairable-systems" => (|| {
            let variance = match text("variance") {
                None | Some("robust") => McfVariance::Robust,
                Some("poisson") => McfVariance::Poisson,
                Some(other) => return Err(invalid(format!("repairable systems: unknown variance {other:?}"))),
            };
            let limits = match text("limits") {
                Some("normal") => McfLimits::Normal,
                None | Some("log") => McfLimits::Log,
                Some(other) => return Err(invalid(format!("repairable systems: unknown limits {other:?}"))),
            };
            Ok(SavedReport::Recurrence {
                request: RecurrenceRequest {
                    system: col("system")?,
                    time: col("time")?,
                    event: optional_col("event")?,
                    group: optional_col("group")?,
                    variance,
                    limits,
                    confidence: num("confidence").unwrap_or(RecurrenceRequest::DEFAULT_CONFIDENCE),
                },
            })
        })(),
        "life-distributions" => (|| {
            let layout = match text("layout") {
                Some("intervals") => LifeDataLayout::Intervals { start: col("start")?, end: col("end")? },
                None | Some("times") => {
                    let coding = match text("censor_coding") {
                        None | Some("nonzero-censored") => StatusCoding::NonzeroIsCensored,
                        Some("nonzero-event") => StatusCoding::NonzeroIsEvent,
                        Some(other) => return Err(invalid(format!("life distributions: unknown coding {other:?}"))),
                    };
                    LifeDataLayout::Times { time: col("time")?, censor: optional_col("censor")?, coding }
                }
                Some(other) => return Err(invalid(format!("life distributions: unknown layout {other:?}"))),
            };
            let family = |k: &str| {
                FitFamily::from_key(k).ok_or_else(|| invalid(format!("life distributions: unknown distribution {k:?}")))
            };
            let numbers = |key: &str| -> Option<Vec<f64>> {
                v.get(key).and_then(JsonValue::as_array).map(|a| a.iter().filter_map(JsonValue::as_f64).collect())
            };
            let d = LifeDistributionRequest::new(layout);
            let families = match v.get("families").and_then(JsonValue::as_array) {
                Some(items) => {
                    items.iter().map(|x| family(x.as_str().unwrap_or_default())).collect::<Result<Vec<_>>>()?
                }
                None => d.families.clone(),
            };
            Ok(SavedReport::LifeDistribution {
                request: LifeDistributionRequest {
                    frequency: optional_col("frequency")?,
                    group: optional_col("group")?,
                    families,
                    detail: text("detail").map(family).transpose()?,
                    confidence: num("confidence").unwrap_or(d.confidence),
                    probabilities: numbers("probabilities").unwrap_or_else(|| d.probabilities.clone()),
                    times: numbers("times").unwrap_or_default(),
                    known_shape: num("known_shape"),
                    layout,
                },
            })
        })(),
        "proportional-hazards" => crate::proportional_hazards_analysis::from_json(v, table),
        "survival-regression" => crate::survival_regression_analysis::from_json(v, table),
        "reliability-test-plan" => (|| {
            let request = match text("plan") {
                Some("demonstration") => {
                    let d = DemonstrationRequest::default();
                    let solve = match (num("units"), num("test_time")) {
                        (Some(n), _) => DemonstrationSolve::TestTime { units: n.max(1.0) as usize },
                        (None, Some(t)) => DemonstrationSolve::Units { test_time: t },
                        (None, None) => d.solve,
                    };
                    TestPlanRequest::Demonstration(DemonstrationRequest {
                        reliability: num("reliability").unwrap_or(d.reliability),
                        time: num("time").unwrap_or(d.time),
                        confidence: num("confidence").unwrap_or(d.confidence),
                        shape: num("shape").unwrap_or(d.shape),
                        failures: v.get("failures").and_then(JsonValue::as_usize).unwrap_or(d.failures),
                        solve,
                    })
                }
                Some("estimation") => {
                    let d = EstimationRequest::default();
                    TestPlanRequest::Estimation(EstimationRequest {
                        distribution: distribution()?,
                        spread: num("spread").unwrap_or(d.spread),
                        fraction_failing: num("fraction_failing").unwrap_or(d.fraction_failing),
                        percentile: num("percentile").unwrap_or(d.percentile),
                        confidence: num("confidence").unwrap_or(d.confidence),
                        precision: num("precision").unwrap_or(d.precision),
                    })
                }
                Some("accelerated") => {
                    let d = AcceleratedRequest::default();
                    let relationship = match text("relationship") {
                        None => d.relationship,
                        Some(k) => StressRelationship::from_key(k)
                            .ok_or_else(|| invalid(format!("test plan: unknown relationship {k:?}")))?,
                    };
                    let levels = match v.get("levels").and_then(JsonValue::as_array) {
                        None => Vec::new(),
                        Some(items) => items
                            .iter()
                            .map(|l| match l.as_array() {
                                Some([x, f]) => x
                                    .as_f64()
                                    .zip(f.as_f64())
                                    .ok_or_else(|| invalid("test plan: each level is [stress, share]")),
                                _ => Err(invalid("test plan: each level is [stress, share]")),
                            })
                            .collect::<Result<_>>()?,
                    };
                    TestPlanRequest::Accelerated(AcceleratedRequest {
                        distribution: distribution()?,
                        spread: num("spread").unwrap_or(d.spread),
                        relationship,
                        use_stress: num("use_stress").unwrap_or(d.use_stress),
                        high_stress: num("high_stress").unwrap_or(d.high_stress),
                        p_use: num("p_use").unwrap_or(d.p_use),
                        p_high: num("p_high").unwrap_or(d.p_high),
                        percentile: num("percentile").unwrap_or(d.percentile),
                        confidence: num("confidence").unwrap_or(d.confidence),
                        precision: num("precision").unwrap_or(d.precision),
                        levels,
                    })
                }
                other => return Err(invalid(format!("test plan: unknown plan {other:?}"))),
            };
            Ok(SavedReport::TestPlan { request })
        })(),
        "competing-risks" => {
            crate::CompetingRisksRequest::from_json(v, table).map(|request| SavedReport::CompetingRisks { request })
        }
        "warranty-returns" => {
            crate::WarrantyRequest::from_json(v, table).map(|request| SavedReport::Warranty { request })
        }
        "life-stress" => crate::life_stress::from_json(v, table),
        "degradation" => crate::degradation::from_json(v, table),
        _ => return None,
    };
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{report_from_json, report_to_json};
    use stat_data::Column;

    fn table() -> DataTable {
        DataTable::new(
            "t",
            vec![
                Column::numeric("Hours", vec![1.0, 2.0]),
                Column::numeric("Failure", vec![1.0, 0.0]),
                Column::text("Phase", vec!["A".into(), "B".into()]),
                Column::text("Unit", vec!["U1".into(), "U2".into()]),
            ],
        )
        .unwrap()
    }

    #[test]
    fn reports_round_trip() {
        let t = table();
        let reports = vec![
            SavedReport::Growth { request: GrowthRequest::new(0) },
            SavedReport::Growth {
                request: GrowthRequest {
                    event: Some(1),
                    phase: Some(2),
                    end_time: Some(40.0),
                    estimator: BetaEstimator::MaximumLikelihood,
                    confidence: 0.9,
                    ..GrowthRequest::new(0)
                },
            },
            SavedReport::Recurrence { request: RecurrenceRequest::new(3, 0) },
            SavedReport::Recurrence {
                request: RecurrenceRequest {
                    event: Some(1),
                    group: Some(2),
                    variance: McfVariance::Poisson,
                    limits: McfLimits::Log,
                    confidence: 0.9,
                    ..RecurrenceRequest::new(3, 0)
                },
            },
            SavedReport::LifeDistribution {
                request: LifeDistributionRequest::new(LifeDataLayout::Times {
                    time: 0,
                    censor: None,
                    coding: StatusCoding::NonzeroIsCensored,
                }),
            },
            SavedReport::LifeDistribution {
                request: LifeDistributionRequest {
                    frequency: Some(1),
                    group: Some(2),
                    families: vec![FitFamily::Weibull3, FitFamily::Normal],
                    detail: Some(FitFamily::Normal),
                    confidence: 0.9,
                    probabilities: vec![0.2],
                    times: vec![1.5, 2.0],
                    known_shape: Some(1.5),
                    ..LifeDistributionRequest::new(LifeDataLayout::Intervals { start: 0, end: 1 })
                },
            },
            SavedReport::LifeDistribution {
                request: LifeDistributionRequest::new(LifeDataLayout::Times {
                    time: 0,
                    censor: Some(1),
                    coding: StatusCoding::NonzeroIsEvent,
                }),
            },
            SavedReport::ProportionalHazards {
                request: crate::ProportionalHazardsRequest::new(0, None, vec![2]),
                profilers: crate::surface_profiler::ProfilerOptions::default(),
            },
            SavedReport::ProportionalHazards {
                request: crate::ProportionalHazardsRequest {
                    coding: StatusCoding::NonzeroIsEvent,
                    effects: vec![vec![(0, 1)], vec![(1, 1)], vec![(0, 1), (1, 1)]],
                    strata: Some(3),
                    ties: crate::TieMethod::Breslow,
                    confidence: 0.9,
                    transform: crate::TimeTransform::Rank,
                    curve_by: Some(2),
                    times: vec![1.0, 1.5],
                    profile: crate::HazardProfile::Survival,
                    profile_time: Some(1.5),
                    ..crate::ProportionalHazardsRequest::new(0, Some(1), vec![2, 3])
                },
                profilers: crate::surface_profiler::ProfilerOptions {
                    show_profiler: false,
                    show_surface_profiler: true,
                    show_observed: true,
                },
            },
            SavedReport::SurvivalRegression {
                request: crate::SurvivalRegressionRequest::new(
                    LifeDataLayout::Times { time: 0, censor: None, coding: StatusCoding::NonzeroIsCensored },
                    vec![2],
                ),
                profilers: crate::surface_profiler::ProfilerOptions::default(),
            },
            SavedReport::SurvivalRegression {
                request: crate::SurvivalRegressionRequest {
                    frequency: Some(1),
                    effects: vec![vec![(0, 1)], vec![(1, 1)]],
                    distribution: crate::AftDistribution::LogLogistic,
                    confidence: 0.9,
                    curve_by: Some(3),
                    probabilities: vec![0.05],
                    times: vec![2.0],
                    profile_probability: 0.1,
                    ..crate::SurvivalRegressionRequest::new(LifeDataLayout::Intervals { start: 0, end: 1 }, vec![2, 3])
                },
                profilers: crate::surface_profiler::ProfilerOptions::default(),
            },
            SavedReport::TestPlan { request: TestPlanRequest::Demonstration(DemonstrationRequest::default()) },
            SavedReport::TestPlan {
                request: TestPlanRequest::Demonstration(DemonstrationRequest {
                    solve: DemonstrationSolve::TestTime { units: 12 },
                    failures: 2,
                    ..DemonstrationRequest::default()
                }),
            },
            SavedReport::TestPlan {
                request: TestPlanRequest::Estimation(EstimationRequest {
                    distribution: LifeDistribution::Lognormal,
                    ..EstimationRequest::default()
                }),
            },
            SavedReport::TestPlan {
                request: TestPlanRequest::Accelerated(AcceleratedRequest {
                    relationship: StressRelationship::InversePower,
                    use_stress: 10.0,
                    high_stress: 40.0,
                    levels: vec![(20.0, 0.6), (40.0, 0.4)],
                    ..AcceleratedRequest::default()
                }),
            },
        ];
        for r in reports {
            let json = report_to_json(&r, &t);
            assert_eq!(report_from_json(&json, &t).unwrap(), r, "{}", json.to_pretty());
        }
    }

    #[test]
    fn defaults_and_errors() {
        let t = table();
        let v = stat_data::parse_json(r#"{"analysis": "reliability-growth", "time": "Hours"}"#).unwrap();
        assert_eq!(report_from_json(&v, &t).unwrap(), SavedReport::Growth { request: GrowthRequest::new(0) });
        let v = stat_data::parse_json(r#"{"analysis": "reliability-test-plan", "plan": "x"}"#).unwrap();
        assert!(report_from_json(&v, &t).is_err());
        let v =
            stat_data::parse_json(r#"{"analysis": "repairable-systems", "system": "Unit", "time": "Nope"}"#).unwrap();
        assert!(report_from_json(&v, &t).is_err());
    }
}
