//! Reliability test plans ([`stat_reliability::test_plans`]). These reports
//! do not read the data table: they plan a test.
//!
//! * **Demonstration:** units (for a test time) or test time (for a number
//!   of units) to demonstrate a reliability at a time with a confidence,
//!   allowing a number of failures, for a Weibull life of known shape; with
//!   the plans for other failure counts and each plan's operating
//!   characteristic (probability of passing against the true reliability).
//! * **Estimation:** units needed to estimate a percentile of a Weibull or
//!   lognormal life within a precision factor, under type I censoring.
//! * **Accelerated:** the optimum two-level and the 4:2:1 compromise plans
//!   for a simple accelerated life test (one stress, linear, Arrhenius or
//!   inverse-power relationship), and optionally a plan of your own, each
//!   with its levels, shares, expected failures and the units needed for a
//!   precision factor at use conditions.

use crate::format::format_number;
use crate::report::{PlotSpec, PointGroups, ReportSection, SectionContent, SectionKind, TextTable};
use stat_core::{Result, StatError};
use stat_reliability::LifeDistribution;
use stat_reliability::test_plans::{
    AcceleratedSetup, EstimationSetup, StressLevel, demonstration_pass_probability, demonstration_time_ratio,
    demonstration_units, units_for_precision_factor,
};

const DIGITS: usize = 5;

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

fn section(kind: SectionKind, title: impl Into<String>, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.into(), content }
}

fn pct(v: f64) -> String {
    format!("{}%", format_number(v * 100.0, 4))
}

/// What a demonstration plan solves for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DemonstrationSolve {
    /// Units needed when each is tested for this long.
    Units {
        /// Test time per unit.
        test_time: f64,
    },
    /// Test time per unit when this many units are tested.
    TestTime {
        /// Units on test.
        units: usize,
    },
}

/// A demonstration (zero- or few-failure) test plan.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DemonstrationRequest {
    /// Reliability to demonstrate at `time`.
    pub reliability: f64,
    /// The time the reliability refers to (e.g. the mission time).
    pub time: f64,
    /// Confidence level.
    pub confidence: f64,
    /// Assumed Weibull shape β.
    pub shape: f64,
    /// Failures allowed.
    pub failures: usize,
    /// What to solve for.
    pub solve: DemonstrationSolve,
}

impl Default for DemonstrationRequest {
    fn default() -> Self {
        Self {
            reliability: 0.9,
            time: 1000.0,
            confidence: 0.9,
            shape: 1.5,
            failures: 0,
            solve: DemonstrationSolve::Units { test_time: 1000.0 },
        }
    }
}

/// An estimation (sample-size) plan for a percentile.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EstimationRequest {
    /// Life distribution.
    pub distribution: LifeDistribution,
    /// Weibull shape β, or lognormal σ.
    pub spread: f64,
    /// Fraction expected to fail by the end of the test (1: run to failure).
    pub fraction_failing: f64,
    /// Percentile to estimate, as a probability (0.1 for B10).
    pub percentile: f64,
    /// Confidence level of the interval.
    pub confidence: f64,
    /// Target precision factor `R` (the interval runs from t̂/R to t̂·R).
    pub precision: f64,
}

impl Default for EstimationRequest {
    fn default() -> Self {
        Self {
            distribution: LifeDistribution::Weibull,
            spread: 2.0,
            fraction_failing: 0.3,
            percentile: 0.1,
            confidence: 0.95,
            precision: 1.5,
        }
    }
}

/// How stress acts on life in an accelerated test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StressRelationship {
    /// Log life linear in the stress.
    Linear,
    /// Arrhenius: log life linear in `1/(°C + 273.15)` (stress in °C).
    Arrhenius,
    /// Inverse power: log life linear in `ln(stress)`.
    InversePower,
}

impl StressRelationship {
    /// Every relationship, in menu order.
    pub const ALL: [Self; 3] = [Self::Arrhenius, Self::InversePower, Self::Linear];

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Linear => "Linear",
            Self::Arrhenius => "Arrhenius (temperature, °C)",
            Self::InversePower => "Inverse power (e.g. voltage)",
        }
    }

    /// Stable key used in project files and scripts.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Linear => "linear",
            Self::Arrhenius => "arrhenius",
            Self::InversePower => "inverse-power",
        }
    }

    /// The relationship with this key.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.key() == key)
    }

    fn transform(self, x: f64) -> f64 {
        match self {
            Self::Linear => x,
            Self::Arrhenius => 11_604.518 / (x + 273.15),
            Self::InversePower => x.ln(),
        }
    }

    fn inverse(self, s: f64) -> f64 {
        match self {
            Self::Linear => s,
            Self::Arrhenius => 11_604.518 / s - 273.15,
            Self::InversePower => s.exp(),
        }
    }
}

/// A simple accelerated life test plan.
#[derive(Debug, Clone, PartialEq)]
pub struct AcceleratedRequest {
    /// Life distribution.
    pub distribution: LifeDistribution,
    /// Weibull shape β, or lognormal σ.
    pub spread: f64,
    /// Stress–life relationship.
    pub relationship: StressRelationship,
    /// Stress at use conditions.
    pub use_stress: f64,
    /// Highest stress allowed in the test.
    pub high_stress: f64,
    /// Planning probability of failing by the end of the test at use.
    pub p_use: f64,
    /// Planning probability of failing by the end of the test at the
    /// highest stress.
    pub p_high: f64,
    /// Percentile at use conditions to estimate.
    pub percentile: f64,
    /// Confidence level.
    pub confidence: f64,
    /// Target precision factor.
    pub precision: f64,
    /// An own plan to evaluate: `(stress, share of units)`; empty for none.
    pub levels: Vec<(f64, f64)>,
}

impl Default for AcceleratedRequest {
    fn default() -> Self {
        Self {
            distribution: LifeDistribution::Weibull,
            spread: 2.0,
            relationship: StressRelationship::Arrhenius,
            use_stress: 50.0,
            high_stress: 120.0,
            p_use: 0.001,
            p_high: 0.9,
            percentile: 0.1,
            confidence: 0.95,
            precision: 2.0,
            levels: Vec::new(),
        }
    }
}

impl AcceleratedRequest {
    fn xi(&self, x: f64) -> f64 {
        let r = self.relationship;
        (r.transform(x) - r.transform(self.high_stress))
            / (r.transform(self.use_stress) - r.transform(self.high_stress))
    }

    fn stress(&self, xi: f64) -> f64 {
        let r = self.relationship;
        let (su, sh) = (r.transform(self.use_stress), r.transform(self.high_stress));
        r.inverse(sh + xi * (su - sh))
    }
}

/// Which plan to make.
#[derive(Debug, Clone, PartialEq)]
pub enum TestPlanRequest {
    /// Demonstration test.
    Demonstration(DemonstrationRequest),
    /// Estimation (sample size for a percentile).
    Estimation(EstimationRequest),
    /// Accelerated life test.
    Accelerated(AcceleratedRequest),
}

impl TestPlanRequest {
    /// Stable key of the plan kind.
    #[must_use]
    pub fn key(&self) -> &'static str {
        match self {
            Self::Demonstration(_) => "demonstration",
            Self::Estimation(_) => "estimation",
            Self::Accelerated(_) => "accelerated",
        }
    }
}

fn sigma(distribution: LifeDistribution, spread: f64) -> f64 {
    match distribution {
        LifeDistribution::Weibull => 1.0 / spread,
        LifeDistribution::Lognormal => spread,
    }
}

fn spread_name(distribution: LifeDistribution) -> &'static str {
    match distribution {
        LifeDistribution::Weibull => "Weibull shape β",
        LifeDistribution::Lognormal => "Lognormal σ (of log life)",
    }
}

/// One accelerated plan's evaluation.
#[derive(Debug, Clone, PartialEq)]
pub struct EvaluatedPlan {
    /// Plan name.
    pub name: String,
    /// Levels, low stress first.
    pub levels: Vec<StressLevel>,
    /// `V` (see [`AcceleratedSetup::variance_factor`]).
    pub variance_factor: f64,
    /// Units needed for the target precision.
    pub units: usize,
}

/// A computed test plan.
#[derive(Debug, Clone, PartialEq)]
pub struct TestPlanAnalysis {
    request: TestPlanRequest,
    sections: Vec<ReportSection>,
    /// The answer: units (demonstration by units, estimation, and the
    /// optimum accelerated plan) or test time.
    pub answer: f64,
}

impl TestPlanAnalysis {
    /// Make the plan.
    ///
    /// # Errors
    /// Inputs out of range, or plans that cannot be met (more than a million
    /// units).
    pub fn run(request: &TestPlanRequest) -> Result<Self> {
        let (sections, answer) = match request {
            TestPlanRequest::Demonstration(d) => demonstration(d)?,
            TestPlanRequest::Estimation(e) => estimation(e)?,
            TestPlanRequest::Accelerated(a) => accelerated(a)?,
        };
        Ok(Self { request: request.clone(), sections, answer })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &TestPlanRequest {
        &self.request
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        match &self.request {
            TestPlanRequest::Demonstration(_) => "Reliability Demonstration Test Plan".to_owned(),
            TestPlanRequest::Estimation(_) => "Life Test Plan for Estimating a Percentile".to_owned(),
            TestPlanRequest::Accelerated(_) => "Accelerated Life Test Plan".to_owned(),
        }
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        self.sections.clone()
    }
}

fn inputs(rows: Vec<(&str, String)>) -> ReportSection {
    let mut t = TextTable::new(["Input", "Value"]);
    for (k, v) in rows {
        t.push([k.to_owned(), v]);
    }
    section(SectionKind::TestPlanInputs, "Inputs", SectionContent::Table(t))
}

fn demonstration(d: &DemonstrationRequest) -> Result<(Vec<ReportSection>, f64)> {
    if !(d.time > 0.0 && d.time.is_finite()) {
        return Err(invalid("the time the reliability refers to must be positive"));
    }
    let num = |v: f64| format_number(v, DIGITS);
    let mut input = vec![
        ("Reliability to demonstrate", format!("{} at time {}", num(d.reliability), num(d.time))),
        ("Confidence", pct(d.confidence)),
        ("Assumed Weibull shape β", num(d.shape)),
        ("Failures allowed", d.failures.to_string()),
    ];
    // Plans for 0 … max(failures, 3) failures allowed: (c, units, time).
    let counts: Vec<usize> = (0..=d.failures.max(3)).collect();
    let plan = |c: usize| -> Result<(usize, f64)> {
        match d.solve {
            DemonstrationSolve::Units { test_time } => {
                if !(test_time > 0.0 && test_time.is_finite()) {
                    return Err(invalid("the test time must be positive"));
                }
                Ok((demonstration_units(d.reliability, d.confidence, d.shape, test_time / d.time, c)?, test_time))
            }
            DemonstrationSolve::TestTime { units } => {
                Ok((units, d.time * demonstration_time_ratio(d.reliability, d.confidence, d.shape, units, c)?))
            }
        }
    };
    let (units, test_time) = plan(d.failures)?;
    let answer = match d.solve {
        DemonstrationSolve::Units { test_time } => {
            input.push(("Test time per unit", num(test_time)));
            units as f64
        }
        DemonstrationSolve::TestTime { units } => {
            input.push(("Units on test", units.to_string()));
            test_time
        }
    };
    let mut result = TextTable::new(["Plan", "Value"]);
    result.push(["Units on test".to_owned(), units.to_string()]);
    result.push(["Test time per unit".to_owned(), num(test_time)]);
    result.push(["Test time ÷ target time".to_owned(), num(test_time / d.time)]);
    result.push(["Failures allowed".to_owned(), d.failures.to_string()]);
    result.push(["Total unit time".to_owned(), num(units as f64 * test_time)]);
    result.push([
        "Pass probability if the reliability is exactly the target".to_owned(),
        format_number(
            demonstration_pass_probability(d.reliability, d.shape, test_time / d.time, units, d.failures)?,
            4,
        ),
    ]);
    let mut alternatives =
        TextTable::new(["Failures allowed", "Units on test", "Test time per unit", "Total unit time"]);
    let mut plans = Vec::new();
    for &c in &counts {
        if let Ok((n, t)) = plan(c) {
            alternatives.push([c.to_string(), n.to_string(), num(t), num(n as f64 * t)]);
            plans.push((c, n, t));
        }
    }
    // Operating characteristic of each plan: P(pass) against the true
    // reliability at the target time.
    let lo = (1.0 - 4.0 * (1.0 - d.reliability)).max(0.0);
    let grid: Vec<f64> = (0..=120).map(|i| lo + (1.0 - lo) * i as f64 / 120.0).collect();
    let lines = plans
        .iter()
        .map(|&(c, n, t)| {
            grid.iter()
                .map(|&r| [r, demonstration_pass_probability(r, d.shape, t / d.time, n, c).unwrap_or(f64::NAN)])
                .filter(|p| p[1].is_finite())
                .collect()
        })
        .collect();
    let curve = PlotSpec {
        x_label: format!("True reliability at time {}", num(d.time)),
        y_label: "Probability of passing".to_owned(),
        groups: Some(PointGroups {
            title: "Failures allowed:".to_owned(),
            labels: plans.iter().map(|p| p.0.to_string()).collect(),
            membership: Vec::new(),
            lines,
        }),
        segments: vec![vec![[d.reliability, 0.0], [d.reliability, 1.0]]],
        ..PlotSpec::default()
    };
    let sections = vec![
        inputs(input),
        section(SectionKind::TestPlanResult, "Demonstration Plan", SectionContent::Table(result)),
        section(
            SectionKind::TestPlanAlternatives,
            "Plans Allowing Other Numbers of Failures (more failures allowed: more units or time, but a fairer chance of passing)",
            SectionContent::Table(alternatives),
        ),
        section(
            SectionKind::TestPlanCurve,
            format!(
                "Operating Characteristic (probability of passing; thin line: the target reliability, where it is at most {})",
                format_number(1.0 - d.confidence, 4)
            ),
            SectionContent::Plot(Box::new(curve)),
        ),
    ];
    Ok((sections, answer))
}

fn estimation(e: &EstimationRequest) -> Result<(Vec<ReportSection>, f64)> {
    let setup = EstimationSetup {
        distribution: e.distribution,
        sigma: if e.spread > 0.0 { sigma(e.distribution, e.spread) } else { f64::NAN },
        fraction_failing: e.fraction_failing,
        percentile: e.percentile,
        confidence: e.confidence,
    };
    let plan = setup.plan(e.precision)?;
    let num = |v: f64| format_number(v, DIGITS);
    let input = vec![
        ("Life distribution", e.distribution.label().to_owned()),
        (spread_name(e.distribution), num(e.spread)),
        ("Fraction failing by the end of the test", num(e.fraction_failing)),
        ("Percentile to estimate", format!("{} (B{} life)", num(e.percentile), format_number(e.percentile * 100.0, 4))),
        ("Confidence", pct(e.confidence)),
        ("Target precision factor R", num(e.precision)),
    ];
    let mut result = TextTable::new(["Plan", "Value"]);
    result.push(["Units on test".to_owned(), plan.units.to_string()]);
    result.push(["Expected failures".to_owned(), num(plan.expected_failures)]);
    result.push(["Precision factor reached".to_owned(), num(plan.precision_factor)]);
    result.push(["Variance factor V (Var ln t̂ₚ = σ²V/n)".to_owned(), num(plan.variance_factor)]);
    let mut alternatives = TextTable::new(["Precision factor R", "Units on test", "Expected failures"]);
    for r in [1.1, 1.2, 1.3, 1.5, 2.0, 3.0] {
        if let Ok(p) = setup.plan(r) {
            alternatives.push([num(r), p.units.to_string(), num(p.expected_failures)]);
        }
    }
    let n_max = (plan.units * 4).max(20);
    let line: Vec<[f64; 2]> = (2..=n_max)
        .step_by((n_max / 200).max(1))
        .filter_map(|n| setup.precision_factor(n).ok().map(|r| [n as f64, r]))
        .collect();
    let curve = PlotSpec {
        x_label: "Units on test".to_owned(),
        y_label: "Precision factor R".to_owned(),
        line: Some(line),
        line_name: Some("Precision factor R".to_owned()),
        segments: vec![vec![[plan.units as f64, 1.0], [plan.units as f64, plan.precision_factor]]],
        ..PlotSpec::default()
    };
    let sections = vec![
        inputs(input),
        section(
            SectionKind::TestPlanResult,
            format!(
                "Sample Size (large-sample; the {} interval for the percentile runs from t̂/R to t̂·R)",
                pct(e.confidence)
            ),
            SectionContent::Table(result),
        ),
        section(SectionKind::TestPlanAlternatives, "Other Precision Factors", SectionContent::Table(alternatives)),
        section(
            SectionKind::TestPlanCurve,
            "Precision Factor Against Sample Size (thin line: this plan)",
            SectionContent::Plot(Box::new(curve)),
        ),
    ];
    Ok((sections, plan.units as f64))
}

fn accelerated(a: &AcceleratedRequest) -> Result<(Vec<ReportSection>, f64)> {
    let valid_stress = |x: f64| match a.relationship {
        StressRelationship::Arrhenius => x > -273.15,
        StressRelationship::InversePower => x > 0.0,
        StressRelationship::Linear => x.is_finite(),
    };
    if !(valid_stress(a.use_stress) && valid_stress(a.high_stress)) || a.use_stress == a.high_stress {
        return Err(invalid(
            "use and highest stress must differ and suit the relationship (°C above −273.15, positive for inverse power)",
        ));
    }
    if a.spread.is_nan() || a.spread <= 0.0 {
        return Err(invalid("the spread (shape or σ) must be positive"));
    }
    let setup = AcceleratedSetup {
        distribution: a.distribution,
        sigma: sigma(a.distribution, a.spread),
        p_use: a.p_use,
        p_high: a.p_high,
        percentile: a.percentile,
    };
    let units = |v: f64| units_for_precision_factor(setup.sigma, v, a.confidence, a.precision);
    let mut plans = Vec::new();
    let (opt, v) = setup.optimum_plan()?;
    plans.push(EvaluatedPlan {
        name: "Optimum (two levels)".into(),
        levels: opt,
        variance_factor: v,
        units: units(v)?,
    });
    let (comp, v) = setup.compromise_plan()?;
    plans.push(EvaluatedPlan { name: "Compromise (4:2:1)".into(), levels: comp, variance_factor: v, units: units(v)? });
    if !a.levels.is_empty() {
        if a.levels.iter().any(|&(x, _)| !valid_stress(x)) {
            return Err(invalid("a test level's stress does not suit the relationship"));
        }
        let mut levels: Vec<StressLevel> =
            a.levels.iter().map(|&(x, f)| StressLevel { xi: a.xi(x), fraction: f }).collect();
        levels.sort_by(|p, q| q.xi.total_cmp(&p.xi));
        let total: f64 = levels.iter().map(|l| l.fraction).sum();
        levels.iter_mut().for_each(|l| l.fraction /= total);
        let v = setup.variance_factor(&levels)?;
        plans.push(EvaluatedPlan { name: "Your plan".into(), levels, variance_factor: v, units: units(v)? });
    }
    let num = |v: f64| format_number(v, DIGITS);
    let input = vec![
        ("Life distribution", a.distribution.label().to_owned()),
        (spread_name(a.distribution), num(a.spread)),
        ("Stress relationship", a.relationship.label().to_owned()),
        ("Use stress", num(a.use_stress)),
        ("Highest test stress", num(a.high_stress)),
        ("Probability of failing by the end of the test at use", num(a.p_use)),
        ("Probability of failing by the end of the test at the highest stress", num(a.p_high)),
        ("Percentile at use to estimate", num(a.percentile)),
        ("Confidence", pct(a.confidence)),
        ("Target precision factor R", num(a.precision)),
    ];
    let mut levels = TextTable::new([
        "Plan",
        "Stress",
        "Standardized stress ξ",
        "Share of units",
        "Units",
        "Probability of failing",
        "Expected failures",
    ]);
    for p in &plans {
        for l in &p.levels {
            let n = (l.fraction * p.units as f64).round();
            let pf = setup.failure_probability(l.xi)?;
            levels.push([
                p.name.clone(),
                num(a.stress(l.xi)),
                format_number(l.xi, 4),
                format_number(l.fraction, 4),
                format!("{n}"),
                format_number(pf, 4),
                format_number(n * pf, 4),
            ]);
        }
    }
    let best = plans[0].variance_factor;
    let mut summary = TextTable::new(["Plan", "Variance factor V", "Units for the precision", "Relative efficiency"]);
    for p in &plans {
        summary.push([
            p.name.clone(),
            num(p.variance_factor),
            p.units.to_string(),
            format_number(best / p.variance_factor, 4),
        ]);
    }
    let precision: Vec<f64> = (0..=60).map(|i| 1.2 + 2.8 * f64::from(i) / 60.0).collect();
    let lines = plans
        .iter()
        .map(|p| {
            precision
                .iter()
                .filter_map(|&r| {
                    units_for_precision_factor(setup.sigma, p.variance_factor, a.confidence, r)
                        .ok()
                        .map(|n| [r, n as f64])
                })
                .collect()
        })
        .collect();
    let curve = PlotSpec {
        x_label: "Precision factor R".to_owned(),
        y_label: "Units on test".to_owned(),
        groups: Some(PointGroups {
            title: "Plan:".to_owned(),
            labels: plans.iter().map(|p| p.name.clone()).collect(),
            membership: Vec::new(),
            lines,
        }),
        ..PlotSpec::default()
    };
    let sections = vec![
        inputs(input),
        section(
            SectionKind::TestPlanResult,
            "Test Levels (ξ = 0 at the highest stress, 1 at use; units rounded)",
            SectionContent::Table(levels),
        ),
        section(
            SectionKind::TestPlanAlternatives,
            "Plans Compared (large-sample; Var ln t̂ₚ at use = σ²V/n; the optimum plan has no middle level to check the model, the compromise plan does)",
            SectionContent::Table(summary),
        ),
        section(
            SectionKind::TestPlanCurve,
            "Units Needed Against the Precision Factor",
            SectionContent::Plot(Box::new(curve)),
        ),
    ];
    Ok((sections, plans[0].units as f64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demonstration_by_units_and_by_time() {
        let d = DemonstrationRequest {
            reliability: 0.95,
            time: 100.0,
            confidence: 0.9,
            shape: 1.0,
            failures: 0,
            solve: DemonstrationSolve::Units { test_time: 100.0 },
        };
        let a = TestPlanAnalysis::run(&TestPlanRequest::Demonstration(d)).unwrap();
        // The success-run plan: 45 units.
        assert_eq!(a.answer, 45.0);
        let s = a.sections();
        assert_eq!(s.len(), 4);
        let SectionContent::Table(alt) = &s[2].content else { panic!() };
        assert_eq!(alt.rows.len(), 4);
        let b = TestPlanAnalysis::run(&TestPlanRequest::Demonstration(DemonstrationRequest {
            solve: DemonstrationSolve::TestTime { units: 45 },
            ..d
        }))
        .unwrap();
        assert!(b.answer <= 100.0 && b.answer > 95.0, "{}", b.answer);
        let SectionContent::Plot(p) = &s[3].content else { panic!() };
        assert_eq!(p.groups.as_ref().unwrap().lines.len(), 4);
    }

    #[test]
    fn estimation_and_accelerated() {
        let e = TestPlanAnalysis::run(&TestPlanRequest::Estimation(EstimationRequest::default())).unwrap();
        let direct = EstimationSetup {
            distribution: LifeDistribution::Weibull,
            sigma: 0.5,
            fraction_failing: 0.3,
            percentile: 0.1,
            confidence: 0.95,
        }
        .plan(1.5)
        .unwrap();
        assert_eq!(e.answer, direct.units as f64);
        let request = AcceleratedRequest { levels: vec![(80.0, 0.5), (120.0, 0.5)], ..AcceleratedRequest::default() };
        let a = TestPlanAnalysis::run(&TestPlanRequest::Accelerated(request.clone())).unwrap();
        let s = a.sections();
        let SectionContent::Table(summary) = &s[2].content else { panic!() };
        assert_eq!(summary.rows.len(), 3);
        // The optimum is the most efficient.
        let eff: Vec<f64> = summary.rows.iter().map(|r| r[3].parse().unwrap()).collect();
        assert!((eff[0] - 1.0).abs() < 1e-12 && eff[1] <= 1.0 && eff[2] <= 1.0);
        // Standardized stress round trip.
        assert!((request.xi(120.0)).abs() < 1e-12 && (request.xi(50.0) - 1.0).abs() < 1e-12);
        assert!((request.stress(request.xi(80.0)) - 80.0).abs() < 1e-9);
        let bad = AcceleratedRequest { use_stress: 120.0, ..AcceleratedRequest::default() };
        assert!(TestPlanAnalysis::run(&TestPlanRequest::Accelerated(bad)).is_err());
    }
}
