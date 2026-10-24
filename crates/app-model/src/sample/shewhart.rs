//! Simulated samples for the moving-average, three-way, zone and short-run
//! charts, the Laney p′/u′ and rare-event g/t charts, and the capability
//! summary page. Each is drawn from a stated model with a fixed seed.

use super::{Sample, col, round_to, table, text};
use crate::project::SavedReport;
use crate::{
    AttributeChartKind, AttributeChartRequest, BootstrapSettings, CapabilityModel, CapabilityRequest,
    ControlChartOptions, ControlChartRequest, FitFamily, RuleSet, SpecLimits, SubgroupSize, WithinSigma,
};
use stat_core::rng::Rng;
use stat_data::{Column, DataTable, MeasurementLevel};
use stat_quality::ChartKind;

/// **Simulated** strip thickness (mm) across 30 coils, 4 readings per coil:
/// 2.000 + a coil effect N(0, 0.015²) + a within-coil error N(0, 0.005²),
/// rounded to 0.001 mm. Alongside, 120 line-speed readings (m/min),
/// N(100, 2²) with a sustained shift of +4 from reading 101 on.
#[must_use]
pub fn coil_thickness() -> DataTable {
    let mut rng = Rng::new(0xC011);
    let (mut coil, mut thickness, mut speed) = (Vec::new(), Vec::new(), Vec::new());
    for c in 1..=30 {
        let effect = 0.015 * rng.normal();
        for _ in 0..4 {
            coil.push(f64::from(c));
            thickness.push(round_to(2.0 + effect + 0.005 * rng.normal(), 3));
        }
    }
    for i in 0..120 {
        let shift = if i >= 100 { 4.0 } else { 0.0 };
        speed.push(round_to(100.0 + shift + 2.0 * rng.normal(), 1));
    }
    let mut coil = Column::numeric("Coil", coil);
    coil.level = MeasurementLevel::Ordinal;
    table(
        "Coil thickness (simulated)",
        vec![coil, Column::numeric("Thickness (mm)", thickness), Column::numeric("Line speed (m/min)", speed)],
    )
}

pub(super) fn coil_thickness_sample() -> Sample {
    let t = coil_thickness();
    let (coil, thickness, speed) = (col(&t, "Coil"), col(&t, "Thickness (mm)"), col(&t, "Line speed (m/min)"));
    let subgrouped = |kind| SavedReport::ControlChart {
        request: ControlChartRequest { subgroup: Some(coil), ..ControlChartRequest::new(thickness, kind, 4) },
        options: ControlChartOptions::default(),
    };
    let individuals = |kind| SavedReport::ControlChart {
        request: ControlChartRequest::new(speed, kind, 1),
        options: ControlChartOptions::default(),
    };
    Sample {
        reports: vec![
            subgrouped(ChartKind::MeanAndRange),
            subgrouped(ChartKind::ThreeWayRange),
            individuals(ChartKind::MovingAverage),
            individuals(ChartKind::Zone),
        ],
        table: t,
    }
}

/// **Simulated** bore diameters (mm) of three part numbers machined in
/// batches of 12 on one lathe: Bracket N(12, 0.02²), Flange N(30, 0.05²) and
/// Hub N(55, 0.10²), batches in the order Bracket, Flange, Hub, Bracket,
/// Flange, Hub. The last six Flange parts are 0.30 mm (6σ) oversize after a
/// tool slipped.
#[must_use]
pub fn short_run_machining() -> DataTable {
    let mut rng = Rng::new(0x5407);
    let parts = [("Bracket", 12.0, 0.02), ("Flange", 30.0, 0.05), ("Hub", 55.0, 0.10)];
    let (mut part, mut bore, mut batch) = (Vec::new(), Vec::new(), Vec::new());
    for b in 0..6 {
        let (name, mean, sd) = parts[b % 3];
        for i in 0..12 {
            let slip = if b == 4 && i >= 6 { 0.30 } else { 0.0 };
            part.push(name.to_owned());
            bore.push(round_to(mean + slip + sd * rng.normal(), 3));
            batch.push(f64::from(b as u32 + 1));
        }
    }
    let mut batch = Column::numeric("Batch", batch);
    batch.level = MeasurementLevel::Ordinal;
    table("Short-run machining (simulated)", vec![batch, text("Part number", part), Column::numeric("Bore (mm)", bore)])
}

pub(super) fn short_run_machining_sample() -> Sample {
    let t = short_run_machining();
    let request = ControlChartRequest {
        part: Some(col(&t, "Part number")),
        ..ControlChartRequest::new(col(&t, "Bore (mm)"), ChartKind::ShortRun, 1)
    };
    Sample { reports: vec![SavedReport::ControlChart { request, options: ControlChartOptions::default() }], table: t }
}

/// **Simulated** weekly safety counts on a hospital ward over 30 weeks.
/// Discharges are uniform on 2000–3000; readmissions are binomial with a
/// weekly rate 0.08·exp(0.15 Z) that varies from week to week
/// (overdispersion); patient days are uniform on 900–1100 and medication
/// errors Poisson with a weekly rate 0.05·exp(0.25 Z) per day (both counts
/// drawn by their normal approximations). Procedures between infections are
/// geometric with mean 150, with one run of 900 in week 26; days between
/// pressure injuries are exponential with mean 12, falling to mean 2 from
/// week 21.
#[must_use]
pub fn ward_safety() -> DataTable {
    let mut rng = Rng::new(7);
    let mut cols: [Vec<f64>; 7] = Default::default();
    for w in 1..=30 {
        let n = (2000.0 + 1000.0 * rng.next_f64()).round();
        let p = 0.08 * (0.15 * rng.normal()).exp();
        let readmitted = (n * p + (n * p * (1.0 - p)).sqrt() * rng.normal()).round().max(0.0);
        let days = (900.0 + 200.0 * rng.next_f64()).round();
        let rate = 0.05 * (0.25 * rng.normal()).exp() * days;
        let errors = (rate + rate.sqrt() * rng.normal()).round().max(0.0);
        let u = 1.0 - rng.next_f64();
        let gap = if w == 26 { 900.0 } else { (u.ln() / (1.0 - 1.0 / 150.0_f64).ln()).floor() };
        let mean_days = if w >= 21 { 2.0 } else { 12.0 };
        let days_between = round_to(-mean_days * (1.0 - rng.next_f64()).ln(), 2).max(0.01);
        for (c, v) in cols.iter_mut().zip([f64::from(w), n, readmitted, days, errors, gap, days_between]) {
            c.push(v);
        }
    }
    let [week, n, readmitted, days, errors, gap, days_between] = cols;
    let mut week = Column::numeric("Week", week);
    week.level = MeasurementLevel::Ordinal;
    table(
        "Ward safety (simulated)",
        vec![
            week,
            Column::numeric("Discharges", n),
            Column::numeric("Readmissions", readmitted),
            Column::numeric("Patient days", days),
            Column::numeric("Medication errors", errors),
            Column::numeric("Procedures between infections", gap),
            Column::numeric("Days between pressure injuries", days_between),
        ],
    )
}

pub(super) fn ward_safety_sample() -> Sample {
    let t = ward_safety();
    let chart = |kind, count: &str, size: Option<&str>, rules| SavedReport::Attribute {
        request: AttributeChartRequest {
            rules,
            ..AttributeChartRequest::new(
                kind,
                col(&t, count),
                size.map_or(SubgroupSize::None, |s| SubgroupSize::Column(col(&t, s))),
            )
        },
    };
    let limits = RuleSet::LIMITS_ONLY;
    Sample {
        reports: vec![
            chart(AttributeChartKind::P, "Readmissions", Some("Discharges"), limits),
            chart(AttributeChartKind::LaneyP, "Readmissions", Some("Discharges"), limits),
            chart(AttributeChartKind::LaneyU, "Medication errors", Some("Patient days"), limits),
            chart(AttributeChartKind::G, "Procedures between infections", None, limits),
            chart(AttributeChartKind::T, "Days between pressure injuries", None, RuleSet::ALL),
        ],
        table: t,
    }
}

/// **Simulated** crushing strength (kp) of 100 tablets, 4 from each of 25
/// consecutive press samples: lognormal with median 8 kp and log-scale
/// standard deviation 0.12, rounded to 0.01 kp. Specification 5–13 kp.
#[must_use]
pub fn tablet_hardness() -> DataTable {
    let mut rng = Rng::new(0x7AB1);
    let (mut sample, mut hardness) = (Vec::new(), Vec::new());
    for s in 1..=25 {
        for _ in 0..4 {
            sample.push(f64::from(s));
            hardness.push(round_to(8.0 * (0.12 * rng.normal()).exp(), 2));
        }
    }
    let mut sample = Column::numeric("Press sample", sample);
    sample.level = MeasurementLevel::Ordinal;
    table("Tablet hardness (simulated)", vec![sample, Column::numeric("Hardness (kp)", hardness)])
}

pub(super) fn tablet_hardness_sample() -> Sample {
    let t = tablet_hardness();
    let request = CapabilityRequest {
        within: WithinSigma::AverageRange { subgroup_size: 4 },
        model: CapabilityModel::Distribution(FitFamily::Lognormal),
        summary: true,
        bootstrap: Some(BootstrapSettings::default()),
        ..CapabilityRequest::new(
            col(&t, "Hardness (kp)"),
            SpecLimits { lower: Some(5.0), upper: Some(13.0), target: None },
        )
    };
    // The tablets by press sample on the graph canvas, with the
    // specification as a shaded band and a line at the overall mean.
    let reference = stat_viz::ReferenceLook {
        lines: vec![stat_viz::ReferenceLine {
            value: stat_viz::ReferenceValue::Statistic(stat_viz::ReferenceStat::Mean),
            scope: stat_viz::ReferenceScope::Whole,
            ..stat_viz::ReferenceLine::default()
        }],
        bands: vec![stat_viz::ReferenceBand {
            from: stat_viz::ReferenceValue::Fixed(5.0),
            to: stat_viz::ReferenceValue::Fixed(13.0),
            label: "Specification".into(),
            ..stat_viz::ReferenceBand::default()
        }],
    };
    let graph = stat_viz::GraphSpec {
        encoding: stat_viz::EncodingPlan {
            x: vec!["Press sample".into()],
            y: vec!["Hardness (kp)".into()],
            ..Default::default()
        },
        axes: stat_viz::GraphAxes {
            style: Some(stat_viz::plot_style::PlotStyle { reference, ..Default::default() }),
            ..Default::default()
        },
        ..stat_viz::GraphSpec::default()
    };
    Sample {
        reports: vec![SavedReport::Capability { request }, SavedReport::Graph { spec: Box::new(graph) }],
        table: t,
    }
}

#[cfg(test)]
mod tests {
    use crate::project::SavedReport;
    use crate::{AttributeChartAnalysis, CapabilityAnalysis, ControlChartAnalysis, SectionKind};
    use stat_data::RowStateManager;
    use stat_quality::RunRule;

    fn chart(s: &super::Sample, k: usize) -> (ControlChartAnalysis, crate::ControlChartOptions) {
        let SavedReport::ControlChart { request, options } = &s.reports[k] else { panic!() };
        let rows = RowStateManager::new(s.table.n_rows());
        (ControlChartAnalysis::run(&s.table, &rows, request).unwrap(), *options)
    }

    fn rule_one(a: &ControlChartAnalysis, options: &crate::ControlChartOptions) -> Vec<usize> {
        a.location_violations(options).iter().filter(|v| v.rule == RunRule::BeyondLimits).map(|v| v.index).collect()
    }

    /// Coil-to-coil variation swamps the X̄ limits; the three-way chart judges
    /// the coil means against their own variation and finds them stable.
    /// The moving-average and zone charts catch the line-speed shift.
    #[test]
    fn coil_thickness_three_way_moving_average_and_zone() {
        let s = super::coil_thickness_sample();
        let (xbar, options) = chart(&s, 0);
        let (three, _) = chart(&s, 1);
        assert!(rule_one(&xbar, &options).len() >= 10, "X̄ flags most coils: {:?}", rule_one(&xbar, &options));
        assert!(rule_one(&three, &options).len() <= 1, "{:?}", rule_one(&three, &options));
        assert!(three.within_violations(&options).is_empty());
        let (ma, options) = chart(&s, 2);
        let flagged = rule_one(&ma, &options);
        assert!(!flagged.is_empty() && flagged.iter().all(|&i| i >= 100), "{flagged:?}");
        let (zone, _) = chart(&s, 3);
        let signals = zone.zone_signals();
        // Scoring 1, 2, 4, 8 also signals by chance every few dozen points, so
        // compare the signal rates before and after the shift.
        let after = signals.iter().filter(|&&i| i >= 100).count();
        assert!(after >= 3 && after * 100 > 4 * 20 * (signals.len() - after), "{signals:?}");
    }

    /// Each part's own mean and σ are recovered, and only the parts made
    /// after the tool slipped stand out.
    #[test]
    fn short_run_finds_the_slipped_tool() {
        let s = super::short_run_machining_sample();
        let (a, options) = chart(&s, 0);
        assert_eq!(a.part_names(), ["Bracket", "Flange", "Hub"]);
        let parts = &a.charts().parts;
        for (p, sd) in parts.iter().zip([0.02, 0.05, 0.10]) {
            if p.part != 1 {
                assert!((p.sigma / sd - 1.0).abs() < 0.4, "{p:?}");
            }
        }
        let flagged = rule_one(&a, &options);
        assert!(flagged.len() >= 3 && flagged.iter().all(|i| (54..60).contains(i)), "{flagged:?}");
    }

    /// The p chart flags many weeks of ordinary rate variation; p′ widens its
    /// limits by the between-week variation and flags none. The u′ chart
    /// does the same for medication errors; the g chart flags the long run
    /// without infection; the t chart's run rules catch pressure injuries
    /// becoming more frequent.
    #[test]
    fn ward_safety_laney_and_rare_event_charts() {
        let s = super::ward_safety_sample();
        let rows = RowStateManager::new(s.table.n_rows());
        let run = |k: usize| {
            let SavedReport::Attribute { request } = &s.reports[k] else { panic!() };
            AttributeChartAnalysis::run(&s.table, &rows, request).unwrap()
        };
        let (p, laney) = (run(0), run(1));
        assert!(p.signal_rows().len() >= 6, "{:?}", p.signal_rows());
        assert!(laney.signal_rows().len() <= 1, "{:?}", laney.signal_rows());
        assert!(laney.charts()[0].sigma_z.unwrap() > 1.5);
        let u = run(2);
        assert!(u.charts()[0].sigma_z.unwrap() > 1.5 && u.signal_rows().len() <= 1, "{:?}", u.signal_rows());
        assert_eq!(run(3).signal_rows(), [25]);
        let t = run(4);
        let v = t.violations();
        assert!(v.iter().any(|v| v.index >= 20) && v.iter().all(|v| v.index >= 20), "{v:?}");
    }

    /// The summary page holds every part, and the bootstrap intervals bracket
    /// the lognormal percentile indices.
    #[test]
    fn tablet_hardness_summary_page() {
        let s = super::tablet_hardness_sample();
        let rows = RowStateManager::new(s.table.n_rows());
        let SavedReport::Capability { request } = &s.reports[0] else { panic!() };
        let a = CapabilityAnalysis::run(&s.table, &rows, request).unwrap();
        let kinds: Vec<_> = a.sections().iter().map(|x| x.kind).collect();
        for k in [
            SectionKind::CapabilityLocationChart,
            SectionKind::CapabilityDispersionChart,
            SectionKind::CapabilityLastSubgroups,
            SectionKind::CapabilityProbabilityPlot,
        ] {
            assert!(kinds.contains(&k), "{k:?} in {kinds:?}");
        }
        let ppk = a.percentile().unwrap().ppk;
        let b = a.bootstrap().unwrap().as_ref().unwrap();
        assert_eq!(b.used, 1000);
        assert!(b.ppk.0 < ppk && ppk < b.ppk.1, "{ppk} in {:?}", b.ppk);
        assert!(ppk > 1.0, "a capable process: {ppk}");
        // The graph shows the specification band behind the tablets and a
        // line at their mean.
        let SavedReport::Graph { spec } = &s.reports[1] else { panic!() };
        let frame = spec.frame(&s.table, &rows).unwrap();
        let layers = spec.layers(&frame, &rows);
        let marks: Vec<&stat_viz::Mark> = layers[0].iter().flat_map(|l| &l.marks).collect();
        assert!(matches!(marks[0], stat_viz::Mark::Span { from, to, .. } if *from == 5.0 && *to == 13.0));
        let hardness = s.table.column(1).and_then(stat_data::Column::as_numeric).unwrap();
        let mean = hardness.iter().sum::<f64>() / hardness.len() as f64;
        assert!(
            marks.iter().any(|m| matches!(m, stat_viz::Mark::Rule { at, .. } if (at - mean).abs() < 1e-9)),
            "a line at the mean {mean}"
        );
    }
}
