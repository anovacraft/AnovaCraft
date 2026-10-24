//! Samples for the Bayesian analyses: conjugate estimation and a
//! hierarchical logistic model (response rates by site), Bayes factor tests
//! and a Bayesian ANOVA (Stroop reaction times), and hierarchical shrinkage
//! (the eight schools).

use super::{Sample, col, round_to, table};
use crate::RandomEffectSpec;
use crate::bayes_analysis::{
    BayesEstimationRequest, BayesFactorRequest, BayesRequest, BayesTestKind, EstimatedParameter,
};
use crate::bayes_models::{BayesLinearRequest, BayesModelRequest};
use crate::project::SavedReport;
use stat_bayes::bayes_factor::Sampling;
use stat_bayes::glm::{GlmFamily, Priors, ScalePrior};
use stat_bayes::univariate::Contrast;
use stat_core::rng::Rng;
use stat_data::{Column, DataTable};

/// A Poisson draw with mean `mu` (inversion; `mu` is small here).
fn poisson(rng: &mut Rng, mu: f64) -> f64 {
    let u = rng.next_f64();
    let (mut k, mut p) = (0.0, (-mu).exp());
    let mut cdf = p;
    while u > cdf && k < 1000.0 {
        k += 1.0;
        p *= mu / k;
        cdf += p;
    }
    k
}

/// **Simulated** trial at six sites: 20–38 patients per site, randomized
/// 1:1 to Active or Placebo. Response (Yes/No) has log-odds −0.4 on Placebo,
/// +0.8 more on Active, plus a site effect N(0, 0.5²). Adverse events are
/// Poisson with 0.8 per year on Placebo and 1.1 on Active over a follow-up of
/// 0.5–2 years (to 0.01). Seeded generator `0xBA7E`.
#[must_use]
pub fn response_rates() -> DataTable {
    let mut rng = Rng::new(0xBA7E);
    let (mut patient, mut site, mut arm, mut response, mut events, mut years) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut id = 0;
    for s in ["North", "East", "South", "West", "Central", "Coastal"] {
        let effect = 0.5 * rng.normal();
        let n = 20 + 2 * rng.below(10);
        for k in 0..n {
            id += 1;
            let active = k % 2 == 0;
            let eta = -0.4 + if active { 0.8 } else { 0.0 } + effect;
            let p = 1.0 / (1.0 + (-eta).exp());
            let follow = round_to(0.5 + 1.5 * rng.next_f64(), 2);
            patient.push(format!("P{id:03}"));
            site.push(s.to_owned());
            arm.push(if active { "Active" } else { "Placebo" }.to_owned());
            response.push(if rng.next_f64() < p { "Yes" } else { "No" }.to_owned());
            events.push(poisson(&mut rng, follow * if active { 1.1 } else { 0.8 }));
            years.push(follow);
        }
    }
    table(
        "Response rates by site (simulated)",
        vec![
            Column::text("Patient", patient),
            Column::text("Site", site),
            Column::text("Arm", arm),
            Column::text("Response", response),
            Column::numeric("Adverse events", events),
            Column::numeric("Follow-up years", years),
        ],
    )
}

pub(super) fn response_rates_sample() -> Sample {
    let t = response_rates();
    let two_proportions = BayesEstimationRequest {
        group: Some(col(&t, "Arm")),
        event: Some("Yes".to_owned()),
        rope: Some((-0.05, 0.05)),
        ..BayesEstimationRequest::new(EstimatedParameter::Proportion, col(&t, "Response"))
    };
    let rates = BayesEstimationRequest {
        group: Some(col(&t, "Arm")),
        denominator: Some(col(&t, "Follow-up years")),
        contrast: Contrast::Ratio,
        ..BayesEstimationRequest::new(EstimatedParameter::Rate, col(&t, "Adverse events"))
    };
    let mut model = BayesModelRequest::new(col(&t, "Response"), vec![col(&t, "Arm")], GlmFamily::Binomial);
    model.random = vec![RandomEffectSpec { correlated: false, ..RandomEffectSpec::intercept(col(&t, "Site")) }];
    model.target_accept = 0.9;
    let reports = vec![
        SavedReport::Bayesian { request: BayesRequest::Estimation(two_proportions) },
        SavedReport::Bayesian { request: BayesRequest::Estimation(rates) },
        SavedReport::Bayesian { request: BayesRequest::Model(Box::new(model)) },
    ];
    Sample { table: t, reports }
}

/// **Simulated** Stroop task: 40 participants (20 younger, aged 19–35, and
/// 20 older, aged 60–79), mean naming time (ms) for congruent and
/// incongruent colour words. Congruent = 620 + 4·(age − 20) + person
/// N(0, 60²); interference = 70 + 1.2·(age − 20) + N(0, 35²); to the nearest
/// ms. Strategy (Read or Name first) is "Name first" with probability 0.3
/// for younger and 0.7 for older participants. Seeded generator `0x5700`.
#[must_use]
pub fn stroop() -> DataTable {
    let mut rng = Rng::new(0x5700);
    let (mut id, mut group, mut age, mut congruent, mut incongruent, mut interference, mut strategy) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for k in 0..40 {
        let older = k >= 20;
        let a = if older { 60.0 + rng.below(20) as f64 } else { 19.0 + rng.below(17) as f64 };
        let c = (620.0 + 4.0 * (a - 20.0) + 60.0 * rng.normal()).round();
        let d = (70.0 + 1.2 * (a - 20.0) + 35.0 * rng.normal()).round();
        id.push(format!("S{:02}", k + 1));
        group.push(if older { "Older" } else { "Younger" }.to_owned());
        age.push(a);
        congruent.push(c);
        incongruent.push(c + d);
        interference.push(d);
        let p = if older { 0.7 } else { 0.3 };
        strategy.push(if rng.next_f64() < p { "Name first" } else { "Read first" }.to_owned());
    }
    table(
        "Stroop reaction times (simulated)",
        vec![
            Column::text("Participant", id),
            Column::text("Age group", group),
            Column::numeric("Age", age),
            Column::numeric("Congruent", congruent),
            Column::numeric("Incongruent", incongruent),
            Column::numeric("Interference", interference),
            Column::text("Strategy", strategy),
        ],
    )
}

pub(super) fn stroop_sample() -> Sample {
    let t = stroop();
    let factor = |test| SavedReport::Bayesian { request: BayesRequest::Factor(BayesFactorRequest::new(test)) };
    let reports = vec![
        factor(BayesTestKind::Paired { first: col(&t, "Incongruent"), second: col(&t, "Congruent") }),
        factor(BayesTestKind::TwoSample { response: col(&t, "Interference"), group: col(&t, "Age group") }),
        factor(BayesTestKind::Correlation { x: col(&t, "Age"), y: col(&t, "Interference") }),
        factor(BayesTestKind::Contingency {
            rows: col(&t, "Age group"),
            columns: col(&t, "Strategy"),
            frequency: None,
            sampling: Sampling::FixedRows,
        }),
        SavedReport::Bayesian {
            request: BayesRequest::Linear(BayesLinearRequest {
                effects: vec![vec![(0, 1)], vec![(1, 1)], vec![(0, 1), (1, 1)]],
                ..BayesLinearRequest::new(col(&t, "Interference"), vec![col(&t, "Age group"), col(&t, "Strategy")])
            }),
        },
    ];
    Sample { table: t, reports }
}

/// Estimated effects of coaching on SAT verbal scores in eight schools and
/// their standard errors, from Rubin (1981), *J. Educ. Stat.* 6, table 2, as
/// reprinted in Gelman et al., *Bayesian Data Analysis* (3rd ed., 2013),
/// table 5.2.
#[must_use]
pub fn eight_schools() -> DataTable {
    table(
        "Eight schools (Rubin 1981)",
        vec![
            Column::text("School", ["A", "B", "C", "D", "E", "F", "G", "H"].map(str::to_owned).to_vec()),
            Column::numeric("Estimated effect", vec![28.0, 8.0, -3.0, 7.0, -1.0, 1.0, 18.0, 12.0]),
            Column::numeric("Standard error", vec![15.0, 10.0, 16.0, 11.0, 9.0, 11.0, 10.0, 18.0]),
        ],
    )
}

pub(super) fn eight_schools_sample() -> Sample {
    let t = eight_schools();
    let mut model = BayesModelRequest::new(col(&t, "Estimated effect"), Vec::new(), GlmFamily::Normal);
    model.known_sd = Some(col(&t, "Standard error"));
    model.random = vec![RandomEffectSpec { correlated: false, ..RandomEffectSpec::intercept(col(&t, "School")) }];
    // The priors of Gelman et al. (2013) §5.5 style analyses: N(0, 5²) on the
    // mean effect and half-Cauchy(5) on the between-school SD.
    model.priors = Priors {
        intercept_location: Some(0.0),
        intercept_scale: 5.0,
        sd: ScalePrior::HalfCauchy,
        sd_scale: 5.0,
        autoscale: false,
        ..Priors::default()
    };
    model.target_accept = 0.95;
    model.plot_parameter = Some("SD(Intercept | School)".to_owned());
    Sample { table: t, reports: vec![SavedReport::Bayesian { request: BayesRequest::Model(Box::new(model)) }] }
}
