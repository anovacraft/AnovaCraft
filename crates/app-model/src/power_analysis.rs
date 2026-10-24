//! Power and sample-size reports ([`stat_inference::power`]). These reports
//! do not read the data table: they describe a planned study.
//!
//! One of three quantities is solved for, given the others: the power for a
//! sample size, the sample size for a target power, or the smallest effect
//! detectable with a sample size and target power. The report shows the
//! inputs, the answer, power against sample size, and power against the size
//! of the effect.
//!
//! Besides tests of means, proportions, rates, standard deviations and
//! equivalence, the designs include margins of error (the answer is the
//! probability that the interval's half-width is at most a chosen `E`),
//! tolerance intervals (the probability that the interval's content stays
//! below a chosen limit), and two-level, screening and general factorial
//! experiments, where the sample size is the number of replicates.

use crate::compare::{DIGITS, section};
use crate::format::format_number;
use crate::report::{Annotations, PlotSpec, ReportSection, SectionContent, SectionKind, TextTable};
use stat_core::Result;
use stat_inference::Alternative;
use stat_inference::power::PowerDesign;

/// What to solve for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PowerSolve {
    /// Power with `n` units per (first) group.
    Power {
        /// `n`.
        n: usize,
    },
    /// Smallest `n` per (first) group reaching the target power.
    SampleSize {
        /// Target power.
        target: f64,
    },
    /// Smallest effect detectable with `n` units per group and the target power.
    Effect {
        /// `n`.
        n: usize,
        /// Target power.
        target: f64,
    },
}

/// A power or sample-size question.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PowerRequest {
    /// The design and assumed effect.
    pub design: PowerDesign,
    /// Significance level `α`.
    pub alpha: f64,
    /// Alternative hypothesis (ignored by ANOVA and equivalence designs).
    pub alternative: Alternative,
    /// What to solve for.
    pub solve: PowerSolve,
}

impl Default for PowerRequest {
    fn default() -> Self {
        Self {
            design: PowerDesign::TwoSampleT { difference: 0.5, std_dev: 1.0, ratio: 1.0 },
            alpha: 0.05,
            alternative: Alternative::TwoSided,
            solve: PowerSolve::SampleSize { target: 0.8 },
        }
    }
}

/// The answer to a [`PowerRequest`].
#[derive(Debug, Clone, PartialEq)]
pub struct PowerAnalysis {
    request: PowerRequest,
    /// Units per (first) group.
    pub n: usize,
    /// Power achieved.
    pub power: f64,
    /// The design used (with the solved effect when solving for the effect).
    pub design: PowerDesign,
    curve_n: Vec<[f64; 2]>,
    curve_effect: Vec<[f64; 2]>,
}

/// Name of the effect parameter of a design.
#[must_use]
pub fn effect_name(design: &PowerDesign) -> &'static str {
    match design {
        PowerDesign::OneSampleT { .. } => "Difference from the hypothesized mean",
        PowerDesign::TwoSampleT { .. } => "Difference between means",
        PowerDesign::OneWayAnova { .. } => "Largest difference between means",
        PowerDesign::OneProportion { .. } => "True proportion",
        PowerDesign::TwoProportions { .. } => "First proportion",
        PowerDesign::OneStdDev { .. } => "True standard deviation",
        PowerDesign::TwoVariances { .. } => "Variance ratio σ₁²/σ₂²",
        PowerDesign::EquivalenceOneSample { .. } | PowerDesign::EquivalenceTwoSample { .. } => "True difference",
        PowerDesign::OneSampleZ { .. } => "Difference from the hypothesized mean",
        PowerDesign::PairedT { .. } => "Mean of the paired differences",
        PowerDesign::OneRate { .. } => "True rate",
        PowerDesign::TwoRates { .. } => "First rate",
        PowerDesign::MeanMargin { .. }
        | PowerDesign::ProportionMargin { .. }
        | PowerDesign::StdDevMargin { .. }
        | PowerDesign::RateMargin { .. } => "Margin of error E",
        PowerDesign::ToleranceInterval { .. } => "Content limit P′",
        PowerDesign::DistributionFreeTolerance { .. } => "Proportion covered P",
        PowerDesign::TwoLevelFactorial { .. } | PowerDesign::Screening { .. } => "Effect (high − low)",
        PowerDesign::GeneralFactorial { .. } => "Largest difference between level means",
    }
}

/// Name of the probability a design reports: the power of a test, or for
/// estimation designs the probability of the planned precision.
#[must_use]
pub fn power_name(design: &PowerDesign) -> &'static str {
    match design {
        PowerDesign::ToleranceInterval { .. } => "Probability the content is below P′",
        PowerDesign::DistributionFreeTolerance { .. } => "Confidence the interval covers P",
        d if d.is_estimation() => "Probability the margin is at most E",
        _ => "Power",
    }
}

/// Label of the sample size `n`: units, units per group, or replicates.
fn n_name(design: &PowerDesign) -> &'static str {
    match design {
        PowerDesign::GeneralFactorial { .. } => "Replicates per cell",
        d if d.is_factorial() => "Replicates",
        PowerDesign::OneWayAnova { .. } => "Sample size per group",
        d if d.second_group(10) > 0 => "Sample size, first group",
        _ => "Sample size",
    }
}

fn clamp_effect(design: &PowerDesign, e: f64) -> f64 {
    match design {
        PowerDesign::OneProportion { .. } | PowerDesign::TwoProportions { .. } => e.clamp(0.001, 0.999),
        PowerDesign::OneStdDev { .. }
        | PowerDesign::TwoVariances { .. }
        | PowerDesign::OneRate { .. }
        | PowerDesign::TwoRates { .. } => e.max(1e-6),
        PowerDesign::ProportionMargin { .. } => e.clamp(1e-6, 1.0),
        PowerDesign::MeanMargin { .. } | PowerDesign::StdDevMargin { .. } | PowerDesign::RateMargin { .. } => {
            e.max(1e-9)
        }
        PowerDesign::ToleranceInterval { proportion, .. } => e.clamp(*proportion, 0.999_99),
        PowerDesign::DistributionFreeTolerance { .. } => e.clamp(1e-6, 0.999_999),
        PowerDesign::OneWayAnova { .. }
        | PowerDesign::TwoLevelFactorial { .. }
        | PowerDesign::Screening { .. }
        | PowerDesign::GeneralFactorial { .. } => e.max(0.0),
        _ => e,
    }
}

impl PowerAnalysis {
    /// Solve the question.
    ///
    /// # Errors
    /// Invalid design parameters, or a target that cannot be reached.
    pub fn run(request: &PowerRequest) -> Result<Self> {
        let r = *request;
        let (n, design) = match r.solve {
            PowerSolve::Power { n } => (n, r.design),
            PowerSolve::SampleSize { target } => (r.design.sample_size(target, r.alpha, r.alternative)?.n, r.design),
            PowerSolve::Effect { n, target } => {
                (n, r.design.with_effect(r.design.detectable_effect(n, target, r.alpha, r.alternative)?))
            }
        };
        let power = design.power(n, r.alpha, r.alternative)?;
        // Power against n: from the minimum up to about twice the answer.
        let n_max = (2 * n).max(design.min_n() + 10);
        let step = ((n_max - design.min_n()) / 60).max(1);
        let curve_n = (design.min_n()..=n_max)
            .step_by(step)
            .filter_map(|k| design.power(k, r.alpha, r.alternative).ok().map(|p| [k as f64, p]))
            .collect();
        // Power against the effect at this n.
        let (from, to) = match design {
            PowerDesign::EquivalenceOneSample { lower, upper, .. }
            | PowerDesign::EquivalenceTwoSample { lower, upper, .. } => {
                let w = upper - lower;
                (lower - 0.25 * w, upper + 0.25 * w)
            }
            _ => {
                let e0 = design.null_effect();
                let e1 = design.effect();
                let far = if (e1 - e0).abs() > 0.0 { e0 + 2.0 * (e1 - e0) } else { e0 + 1.0 };
                (clamp_effect(&design, e0), clamp_effect(&design, far))
            }
        };
        let curve_effect = (0..=80)
            .filter_map(|i| {
                let e = from + (to - from) * f64::from(i) / 80.0;
                design.with_effect(e).power(n, r.alpha, r.alternative).ok().map(|p| [e, p])
            })
            .collect();
        Ok(Self { request: r, n, power, design, curve_n, curve_effect })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &PowerRequest {
        &self.request
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        let what = match self.request.solve {
            PowerSolve::Power { .. } => "Power",
            PowerSolve::SampleSize { .. } => "Sample Size",
            PowerSolve::Effect { .. } if matches!(self.design, PowerDesign::DistributionFreeTolerance { .. }) => {
                "Largest Proportion Covered"
            }
            PowerSolve::Effect { .. } if self.design.is_estimation() => "Smallest Margin",
            PowerSolve::Effect { .. } if matches!(self.design, PowerDesign::ToleranceInterval { .. }) => {
                "Smallest Content Limit"
            }
            PowerSolve::Effect { .. } => "Detectable Difference",
        };
        format!("{what}: {}", self.design.label())
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let r = &self.request;
        let mut inputs = TextTable::new(["Setting", "Value"]);
        let mut row = |a: &str, b: String| inputs.push([a.to_owned(), b]);
        row("Test", self.design.label().to_owned());
        // The distribution-free interval's confidence is the answer, not an input.
        if !matches!(self.design, PowerDesign::DistributionFreeTolerance { .. }) {
            row("Significance level α", format_number(r.alpha, 4));
        }
        if self.design.uses_alternative() {
            row("Alternative", r.alternative.label().to_owned());
        }
        for (name, value) in design_rows(&r.design) {
            row(&name, value);
        }
        let mut out = vec![section(SectionKind::PowerInputs, "Assumptions", SectionContent::Table(inputs))];
        let mut result = TextTable::new(["Quantity", "Value"]);
        let two_groups = self.design.second_group(self.n) > 0;
        let anova = matches!(self.design, PowerDesign::OneWayAnova { .. });
        let factorial = self.design.is_factorial();
        let n_label = n_name(&self.design);
        result.push([n_label.to_owned(), self.n.to_string()]);
        if two_groups {
            result.push(["Sample size, second group".to_owned(), self.design.second_group(self.n).to_string()]);
        }
        if two_groups || anova {
            result.push(["Total sample size".to_owned(), self.design.total(self.n).to_string()]);
        }
        if factorial {
            result.push(["Total runs".to_owned(), self.design.total(self.n).to_string()]);
        }
        let power_label = power_name(&self.design);
        result.push([power_label.to_owned(), format_number(self.power, 4)]);
        if let PowerSolve::Effect { target, .. } = r.solve {
            result.push([
                format!(
                    "{} {} ({} {})",
                    match self.design {
                        PowerDesign::DistributionFreeTolerance { .. } => "Largest",
                        _ if self.design.uses_alternative() || factorial => "Detectable",
                        _ => "Smallest",
                    },
                    effect_name(&self.design).to_lowercase(),
                    power_label.to_lowercase(),
                    format_number(target, 3)
                ),
                format_number(self.design.effect(), DIGITS),
            ]);
        }
        if let PowerSolve::SampleSize { target } = r.solve {
            result.push([format!("Target: {}", power_label.to_lowercase()), format_number(target, 3)]);
        }
        out.push(section(SectionKind::PowerResult, "Result", SectionContent::Table(result)));
        let target = match r.solve {
            PowerSolve::Power { .. } => None,
            PowerSolve::SampleSize { target } | PowerSolve::Effect { target, .. } => Some(target),
        };
        let marker = |x: f64, label: String| Annotations {
            groups: vec!["This study".to_owned()],
            items: vec![(x, self.power, label, 0)],
            labels: Vec::new(),
        };
        let reference = |c: &[[f64; 2]]| -> Vec<Vec<[f64; 2]>> {
            match (target, c.first(), c.last()) {
                (Some(t), Some(a), Some(b)) => vec![vec![[a[0], t], [b[0], t]]],
                _ => Vec::new(),
            }
        };
        let y_axis = if power_label == "Power" { "Power" } else { "Probability" };
        out.push(section(
            SectionKind::PowerCurve,
            format!("{power_label} by {n_label} (marker: this study; thin line: target)"),
            SectionContent::Plot(Box::new(PlotSpec {
                x_time: None,
                y_time: None,
                x_label: n_label.to_owned(),
                y_label: y_axis.to_owned(),
                points: Vec::new(),
                line: Some(self.curve_n.clone()),
                line_name: Some(format!("{y_axis} curve")),
                band: None,
                groups: None,
                x_ticks: None,
                panes: Vec::new(),
                annotations: Some(marker(self.n as f64, format!("n = {}", self.n))),
                segments: reference(&self.curve_n),
                ..PlotSpec::default()
            })),
        ));
        out.push(section(
            SectionKind::EffectCurve,
            format!("{power_label} by {} (n = {})", effect_name(&self.design), self.n),
            SectionContent::Plot(Box::new(PlotSpec {
                x_time: None,
                y_time: None,
                x_label: effect_name(&self.design).to_owned(),
                y_label: y_axis.to_owned(),
                points: Vec::new(),
                line: Some(self.curve_effect.clone()),
                line_name: Some(format!("{y_axis} curve")),
                band: None,
                groups: None,
                x_ticks: None,
                panes: Vec::new(),
                annotations: Some(marker(self.design.effect(), format_number(self.design.effect(), 4))),
                segments: reference(&self.curve_effect),
                ..PlotSpec::default()
            })),
        ));
        out
    }
}

/// `1st`, `2nd`, `3rd`, `4th`, …, `11th`, `21st`.
fn ordinal(k: usize) -> String {
    let suffix = match (k % 10, k % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{k}{suffix}")
}

/// Labelled parameters of a design.
#[must_use]
pub fn design_rows(design: &PowerDesign) -> Vec<(String, String)> {
    let f = |v: f64| format_number(v, DIGITS);
    match *design {
        PowerDesign::OneSampleT { difference, std_dev } => vec![
            ("Difference to detect δ".into(), f(difference)),
            ("Standard deviation σ (of differences, if paired)".into(), f(std_dev)),
            ("Standardized effect δ/σ".into(), f(difference / std_dev)),
        ],
        PowerDesign::TwoSampleT { difference, std_dev, ratio } => vec![
            ("Difference between means".into(), f(difference)),
            ("Standard deviation σ".into(), f(std_dev)),
            ("Standardized effect (Cohen's d)".into(), f(difference / std_dev)),
            ("Allocation n₂/n₁".into(), f(ratio)),
        ],
        PowerDesign::OneWayAnova { groups, max_difference, std_dev } => vec![
            ("Number of groups".into(), groups.to_string()),
            ("Largest difference between means".into(), f(max_difference)),
            ("Standard deviation σ".into(), f(std_dev)),
            (
                "Cohen's f (least favourable spread)".into(),
                f(max_difference / (std_dev * (2.0 * groups as f64).sqrt())),
            ),
        ],
        PowerDesign::OneProportion { null, actual } => {
            vec![("Hypothesized proportion p₀".into(), f(null)), ("True proportion p₁".into(), f(actual))]
        }
        PowerDesign::TwoProportions { first, second, ratio } => vec![
            ("First proportion p₁".into(), f(first)),
            ("Second proportion p₂".into(), f(second)),
            ("Allocation n₂/n₁".into(), f(ratio)),
        ],
        PowerDesign::OneStdDev { null, actual } => {
            vec![("Hypothesized σ₀".into(), f(null)), ("True σ₁".into(), f(actual))]
        }
        PowerDesign::TwoVariances { variance_ratio, ratio } => {
            vec![("Variance ratio σ₁²/σ₂²".into(), f(variance_ratio)), ("Allocation n₂/n₁".into(), f(ratio))]
        }
        PowerDesign::EquivalenceOneSample { lower, upper, difference, std_dev } => vec![
            ("Equivalence bounds".into(), format!("{} to {}", f(lower), f(upper))),
            ("True difference".into(), f(difference)),
            ("Standard deviation σ".into(), f(std_dev)),
        ],
        PowerDesign::EquivalenceTwoSample { lower, upper, difference, std_dev, ratio } => vec![
            ("Equivalence bounds".into(), format!("{} to {}", f(lower), f(upper))),
            ("True difference".into(), f(difference)),
            ("Standard deviation σ".into(), f(std_dev)),
            ("Allocation n₂/n₁".into(), f(ratio)),
        ],
        PowerDesign::OneSampleZ { difference, std_dev } => vec![
            ("Difference to detect δ".into(), f(difference)),
            ("Known standard deviation σ".into(), f(std_dev)),
            ("Standardized effect δ/σ".into(), f(difference / std_dev)),
        ],
        PowerDesign::PairedT { difference, std_dev, correlation } => vec![
            ("Mean difference δ".into(), f(difference)),
            ("Standard deviation σ (each condition)".into(), f(std_dev)),
            ("Correlation ρ between the pairs".into(), f(correlation)),
            ("SD of the differences σ√(2(1 − ρ))".into(), f(std_dev * (2.0 * (1.0 - correlation)).sqrt())),
        ],
        PowerDesign::OneRate { null, actual, exposure } => vec![
            ("Hypothesized rate λ₀".into(), f(null)),
            ("True rate λ₁".into(), f(actual)),
            ("Exposure per unit".into(), f(exposure)),
        ],
        PowerDesign::TwoRates { first, second, exposure, ratio } => vec![
            ("First rate λ₁".into(), f(first)),
            ("Second rate λ₂".into(), f(second)),
            ("Exposure per unit".into(), f(exposure)),
            ("Allocation n₂/n₁".into(), f(ratio)),
        ],
        PowerDesign::MeanMargin { margin, std_dev } => {
            vec![("Margin of error E".into(), f(margin)), ("Standard deviation σ".into(), f(std_dev))]
        }
        PowerDesign::ProportionMargin { margin, proportion } => vec![
            ("Margin of error E (Wilson interval)".into(), f(margin)),
            ("Expected proportion p".into(), f(proportion)),
        ],
        PowerDesign::StdDevMargin { margin, std_dev } => {
            vec![("Margin of error E (upper limit − s)".into(), f(margin)), ("Standard deviation σ".into(), f(std_dev))]
        }
        PowerDesign::RateMargin { margin, rate, exposure } => vec![
            ("Margin of error E".into(), f(margin)),
            ("Expected rate λ".into(), f(rate)),
            ("Exposure per unit".into(), f(exposure)),
        ],
        PowerDesign::ToleranceInterval { proportion, confidence, max_proportion, two_sided } => vec![
            ("Interval".into(), if two_sided { "two-sided".into() } else { "one-sided".into() }),
            ("Proportion covered P".into(), f(proportion)),
            ("Confidence".into(), f(confidence)),
            ("Content limit P′".into(), f(max_proportion)),
        ],
        PowerDesign::DistributionFreeTolerance { proportion, removed, two_sided } => vec![
            (
                "Interval".into(),
                if two_sided {
                    "two-sided, between order statistics".into()
                } else {
                    "one limit, an order statistic".into()
                },
            ),
            ("Proportion covered P".into(), f(proportion)),
            ("Order statistics outside the interval m".into(), removed.to_string()),
            (
                "Limits".into(),
                match (two_sided, removed) {
                    (true, 2) => "the smallest and largest values".into(),
                    (false, 1) => "the smallest (or largest) value".into(),
                    (true, m) => format!("the {} smallest and {} largest values", ordinal(m / 2), ordinal(m - m / 2)),
                    (false, m) => format!("the {} smallest (or largest) value", ordinal(m)),
                },
            ),
        ],
        PowerDesign::TwoLevelFactorial { factors, fraction, center_points, interactions, effect, std_dev } => vec![
            ("Factors".into(), factors.to_string()),
            (
                "Runs per replicate".into(),
                format!(
                    "{} (2{})",
                    1usize << factors.saturating_sub(fraction),
                    stat_core::text::superscript(&format!("{factors}−{fraction}"))
                ),
            ),
            ("Center points".into(), center_points.to_string()),
            (
                "Model".into(),
                if interactions { "main effects and two-factor interactions".into() } else { "main effects".into() },
            ),
            ("Effect to detect (high − low)".into(), f(effect)),
            ("Standard deviation σ".into(), f(std_dev)),
        ],
        PowerDesign::Screening { runs, factors, center_points, effect, std_dev } => vec![
            ("Runs per replicate (two-level orthogonal array)".into(), runs.to_string()),
            ("Factors".into(), factors.to_string()),
            ("Center points".into(), center_points.to_string()),
            ("Effect to detect (high − low)".into(), f(effect)),
            ("Standard deviation σ".into(), f(std_dev)),
        ],
        PowerDesign::GeneralFactorial { levels, other_cells, max_difference, std_dev } => vec![
            ("Levels of the factor tested".into(), levels.to_string()),
            ("Cells of the other factors".into(), other_cells.to_string()),
            ("Largest difference between level means".into(), f(max_difference)),
            ("Standard deviation σ".into(), f(std_dev)),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_size_report() {
        let a = PowerAnalysis::run(&PowerRequest::default()).unwrap();
        assert_eq!(a.n, 64, "Cohen: d = 0.5 needs 64 per group");
        assert_eq!(a.title(), "Sample Size: Two-sample t");
        let s = a.sections();
        let kinds: Vec<_> = s.iter().map(|x| x.kind).collect();
        assert_eq!(
            kinds,
            [SectionKind::PowerInputs, SectionKind::PowerResult, SectionKind::PowerCurve, SectionKind::EffectCurve]
        );
        let SectionContent::Table(r) = &s[1].content else { panic!() };
        assert_eq!(r.rows[0], ["Sample size, first group", "64"]);
        assert_eq!(r.rows[2], ["Total sample size", "128"]);
        let SectionContent::Plot(p) = &s[2].content else { panic!() };
        assert!(p.line.as_ref().unwrap().len() > 30);
        assert!(p.line.as_ref().unwrap().windows(2).all(|w| w[1][1] >= w[0][1] - 1e-12), "power rises with n");
        assert_eq!(p.line_label(), "Power curve");
        let SectionContent::Plot(e) = &s[3].content else { panic!() };
        assert_eq!(e.line_label(), "Power curve");
    }

    #[test]
    fn power_and_effect_questions() {
        let req = PowerRequest { solve: PowerSolve::Power { n: 64 }, ..PowerRequest::default() };
        let a = PowerAnalysis::run(&req).unwrap();
        assert!((a.power - 0.8015).abs() < 1e-4);
        let req = PowerRequest { solve: PowerSolve::Effect { n: 64, target: 0.8 }, ..PowerRequest::default() };
        let a = PowerAnalysis::run(&req).unwrap();
        assert!((a.design.effect() - 0.4991).abs() < 1e-3, "{}", a.design.effect());
        assert!(a.title().starts_with("Detectable Difference"));
        let eq = PowerRequest {
            design: PowerDesign::EquivalenceTwoSample {
                lower: -0.5,
                upper: 0.5,
                difference: 0.0,
                std_dev: 1.0,
                ratio: 1.0,
            },
            ..PowerRequest::default()
        };
        let a = PowerAnalysis::run(&eq).unwrap();
        assert!(a.power >= 0.8);
        let bad =
            PowerRequest { design: PowerDesign::OneProportion { null: 0.5, actual: 1.5 }, ..PowerRequest::default() };
        assert!(PowerAnalysis::run(&bad).is_err());
    }

    /// The extended designs run every question and label their results
    /// (the engine values are checked in `stat_inference::power`).
    #[test]
    fn extended_designs() {
        let designs = [
            PowerDesign::OneSampleZ { difference: 0.5, std_dev: 1.0 },
            PowerDesign::PairedT { difference: 1.0, std_dev: 2.0, correlation: 0.6 },
            PowerDesign::OneRate { null: 2.0, actual: 2.5, exposure: 1.5 },
            PowerDesign::TwoRates { first: 3.0, second: 2.0, exposure: 2.0, ratio: 1.0 },
            PowerDesign::MeanMargin { margin: 0.5, std_dev: 2.0 },
            PowerDesign::ProportionMargin { margin: 0.05, proportion: 0.3 },
            PowerDesign::StdDevMargin { margin: 0.2, std_dev: 1.0 },
            PowerDesign::RateMargin { margin: 0.5, rate: 4.0, exposure: 1.0 },
            PowerDesign::ToleranceInterval { proportion: 0.9, confidence: 0.95, max_proportion: 0.97, two_sided: true },
            PowerDesign::DistributionFreeTolerance { proportion: 0.9, removed: 2, two_sided: true },
            PowerDesign::TwoLevelFactorial {
                factors: 3,
                fraction: 0,
                center_points: 0,
                interactions: false,
                effect: 1.0,
                std_dev: 1.0,
            },
            PowerDesign::Screening { runs: 12, factors: 7, center_points: 0, effect: 1.2, std_dev: 1.5 },
            PowerDesign::GeneralFactorial { levels: 3, other_cells: 4, max_difference: 2.0, std_dev: 1.5 },
        ];
        for design in designs {
            for solve in [
                PowerSolve::SampleSize { target: 0.8 },
                PowerSolve::Power { n: 20 },
                PowerSolve::Effect { n: 20, target: 0.8 },
            ] {
                let req = PowerRequest { design, solve, ..PowerRequest::default() };
                let a = PowerAnalysis::run(&req).unwrap_or_else(|e| panic!("{design:?} {solve:?}: {e}"));
                let s = a.sections();
                assert_eq!(s.len(), 4);
                let SectionContent::Plot(p) = &s[3].content else { panic!() };
                assert!(p.line.as_ref().unwrap().len() > 10, "{design:?}");
            }
        }
        let a = PowerAnalysis::run(&PowerRequest {
            design: designs[10],
            solve: PowerSolve::Power { n: 2 },
            ..PowerRequest::default()
        })
        .unwrap();
        let SectionContent::Table(r) = &a.sections()[1].content else { panic!() };
        assert_eq!(r.rows[0], ["Replicates", "2"]);
        assert_eq!(r.rows[1], ["Total runs", "16"]);
        let a = PowerAnalysis::run(&PowerRequest { design: designs[4], ..PowerRequest::default() }).unwrap();
        let SectionContent::Table(r) = &a.sections()[1].content else { panic!() };
        assert_eq!(r.rows[1][0], "Probability the margin is at most E");
    }

    /// A distribution-free tolerance interval: 95% content with 95%
    /// confidence needs 93 values for two limits (Wilks 1941).
    #[test]
    fn distribution_free_tolerance_report() {
        let req = PowerRequest {
            design: PowerDesign::DistributionFreeTolerance { proportion: 0.95, removed: 2, two_sided: true },
            solve: PowerSolve::SampleSize { target: 0.95 },
            ..PowerRequest::default()
        };
        let a = PowerAnalysis::run(&req).unwrap();
        assert_eq!(a.n, 93);
        assert_eq!(a.title(), "Sample Size: Tolerance interval, distribution-free");
        let s = a.sections();
        let SectionContent::Table(inputs) = &s[0].content else { panic!() };
        assert!(inputs.rows.iter().all(|r| r[0] != "Significance level α"));
        assert!(inputs.rows.iter().any(|r| r[1] == "the smallest and largest values"));
        let SectionContent::Table(r) = &s[1].content else { panic!() };
        assert_eq!(r.rows[1][0], "Confidence the interval covers P");
        let req = PowerRequest { solve: PowerSolve::Effect { n: 93, target: 0.95 }, ..req };
        let a = PowerAnalysis::run(&req).unwrap();
        assert_eq!(a.title(), "Largest Proportion Covered: Tolerance interval, distribution-free");
        assert!((a.design.effect() - 0.95).abs() < 3e-4, "{}", a.design.effect());
        let SectionContent::Plot(e) = &a.sections()[3].content else { panic!() };
        let line = e.line.as_ref().unwrap();
        assert!(line.len() > 10 && line.windows(2).all(|w| (w[1][0] - w[0][0]) * (w[1][1] - w[0][1]) <= 1e-12));
    }
}
