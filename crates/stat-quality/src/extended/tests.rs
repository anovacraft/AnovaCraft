use super::*;
use crate::charts::{ChartKind, ChartStatistic};
use crate::constants::{ChartConstants, d2, d3};
use crate::phased::{ChartSettings, phased_chart_with};

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-12 * a.abs().max(b.abs()).max(1.0)
}

fn singles(x: &[f64]) -> Vec<Vec<f64>> {
    x.iter().map(|&v| vec![v]).collect()
}

fn settings(span: usize) -> ChartSettings {
    ChartSettings { span, parts: None, ..ChartSettings::default() }
}

/// Moving averages of span 3 of six values, by hand: M̄R = 8/5, σ̂ = M̄R/d₂(2),
/// x̄ = 3.5, averages 1, 2, 2, 3, 4, 5 with limits ±3σ̂/√wᵢ.
#[test]
fn moving_average_of_individuals_by_hand() {
    let x = [1.0, 3.0, 2.0, 4.0, 6.0, 5.0];
    let c = phased_chart_with(ChartKind::MovingAverage, &singles(&x), &[0; 6], &settings(3)).unwrap();
    assert_eq!(c.location.statistic, ChartStatistic::MovingAverage);
    assert_eq!(c.dispersion.statistic, ChartStatistic::MovingRange);
    let sigma = 1.6 / d2(2).unwrap();
    assert!(close(c.phases[0].sigma, sigma));
    let averages: Vec<f64> = c.location.values.iter().map(|v| v.unwrap()).collect();
    for (got, want) in averages.iter().zip([1.0, 2.0, 2.0, 3.0, 4.0, 5.0]) {
        assert!(close(*got, want), "{got} vs {want}");
    }
    for (i, l) in c.location.limits.iter().enumerate() {
        let w = (i + 1).min(3) as f64;
        assert!(close(l.center, 3.5));
        assert!(close(l.upper, 3.5 + 3.0 * sigma / w.sqrt()));
        assert!(close(l.lower, 3.5 - 3.0 * sigma / w.sqrt()));
    }
    // The companion chart is the usual moving range chart.
    assert_eq!(c.dispersion.values[0], None);
    assert!(close(c.dispersion.limits[0].upper, ChartConstants::for_subgroup_size(2).unwrap().range_upper * 1.6));
    assert!(phased_chart_with(ChartKind::MovingAverage, &singles(&x), &[0; 6], &settings(1)).is_err());
}

/// With subgroups of n the limits are x̿ ± 3σ̂/√(n w) (Montgomery §9.3), and
/// the averages restart at a phase boundary.
#[test]
fn moving_average_of_subgroup_means_and_phases() {
    let g: Vec<Vec<f64>> =
        (0..8).map(|i| (0..4).map(|j| 10.0 + f64::from((i * 5 + j * 3) % 7) * 0.4).collect()).collect();
    let c = phased_chart_with(ChartKind::MovingAverage, &g, &[0; 8], &settings(4)).unwrap();
    assert_eq!(c.dispersion.statistic, ChartStatistic::SubgroupRange);
    let sigma = c.phases[0].sigma;
    let l = c.location.limits[5];
    assert!(close(l.upper - l.center, 3.0 * sigma / (4.0_f64 * 4.0).sqrt()));
    let means: Vec<f64> = g.iter().map(|v| v.iter().sum::<f64>() / 4.0).collect();
    assert!(close(c.location.values[5].unwrap(), means[2..6].iter().sum::<f64>() / 4.0));
    let phases = [0, 0, 0, 0, 1, 1, 1, 1];
    let p = phased_chart_with(ChartKind::MovingAverage, &g, &phases, &settings(4)).unwrap();
    assert!(close(p.location.values[4].unwrap(), means[4]), "the average restarts at phase 2");
    // Mixed single values and subgroups are refused.
    let mixed = vec![vec![1.0], vec![2.0, 3.0], vec![4.0]];
    assert!(phased_chart_with(ChartKind::MovingAverage, &mixed, &[0; 3], &settings(2)).is_err());
}

/// Three-way chart by hand: means 11, 14, 12, 16 (ranges 2 each); M̄R of the
/// means = 3, so limits 13.25 ± 3·3/d₂(2); within R chart from R̄ = 2.
#[test]
fn three_way_by_hand() {
    let g = vec![vec![10.0, 11.0, 12.0], vec![13.0, 14.0, 15.0], vec![11.0, 12.0, 13.0], vec![15.0, 16.0, 17.0]];
    let c = phased_chart_with(ChartKind::ThreeWayRange, &g, &[0; 4], &ChartSettings::default()).unwrap();
    assert_eq!(c.location.statistic, ChartStatistic::SubgroupMean);
    assert_eq!(c.dispersion.statistic, ChartStatistic::MeanMovingRange);
    let between = 3.0 / d2(2).unwrap();
    let l = c.location.limits[0];
    assert!(close(l.center, 13.25) && close(l.upper, 13.25 + 3.0 * between) && close(l.lower, 13.25 - 3.0 * between));
    // The half-width is 3/d₂(2) = 2.66 times M̄R (Wheeler's tabled factor).
    assert!((3.0 / d2(2).unwrap() - 2.66).abs() < 2e-3);
    assert!(close(c.phases[0].between_sigma.unwrap(), between));
    let mr: Vec<Option<f64>> = c.dispersion.values.clone();
    assert_eq!(mr, vec![None, Some(3.0), Some(2.0), Some(4.0)]);
    let d4 = 1.0 + 3.0 * d3(2).unwrap() / d2(2).unwrap();
    assert!(close(c.dispersion.limits[0].upper, d4 * 3.0));
    let within = c.within.as_ref().unwrap();
    assert_eq!(within.statistic, ChartStatistic::SubgroupRange);
    let k = ChartConstants::for_subgroup_size(3).unwrap();
    assert!(close(within.limits[0].center, 2.0) && close(within.limits[0].upper, k.range_upper * 2.0));
    assert!(close(c.phases[0].sigma, 2.0 / k.d2));
    // An X̄ chart with these within-subgroup limits would flag every mean;
    // the three-way limits flag none.
    assert!(c.location.violations(&[crate::RunRule::BeyondLimits], &c.phases).is_empty());
    let s = phased_chart_with(ChartKind::ThreeWayStdDev, &g, &[0; 4], &ChartSettings::default()).unwrap();
    assert_eq!(s.within.as_ref().unwrap().statistic, ChartStatistic::SubgroupStdDev);
    assert!(close(s.location.limits[0].upper, l.upper), "the means chart does not depend on R or S");
    let flat = vec![vec![1.0, 2.0], vec![1.0, 2.0], vec![1.0, 2.0]];
    let e = phased_chart_with(ChartKind::ThreeWayRange, &flat, &[0; 3], &ChartSettings::default()).unwrap_err();
    assert!(e.to_string().contains("means"), "{e}");
}

/// Zone scores worked by hand on a chart with center 0 and σ = 1.
#[test]
fn zone_scores_by_hand() {
    let x = [0.5, 1.5, 2.5, -0.5, -2.5, -1.2, 3.5, 0.2, 1.1, 1.2, 0.0, 2.1, 2.2, 1.0];
    let chart = VaryingChart {
        statistic: ChartStatistic::Individual,
        values: x.iter().copied().map(Some).collect(),
        limits: vec![ControlLimits { center: 0.0, lower: -3.0, upper: 3.0 }; x.len()],
        sigma_multiple: 3.0,
    };
    let phases = [PhaseSummary { start: 0, end: x.len(), sigma: 1.0, center: 0.0, sizes: (1, 1), between_sigma: None }];
    let s = zone_scores(&chart, &phases);
    let scores: Vec<u32> = s.iter().map(|z| z.score).collect();
    let totals: Vec<u32> = s.iter().map(|z| z.cumulative).collect();
    let signals: Vec<usize> = s.iter().enumerate().filter(|(_, z)| z.signal).map(|(i, _)| i).collect();
    assert_eq!(scores, [1, 2, 4, 1, 4, 2, 8, 1, 2, 2, 1, 4, 4, 1]);
    assert_eq!(totals, [1, 3, 7, 1, 5, 7, 8, 1, 3, 5, 1, 4, 8, 1]);
    assert_eq!(signals, [6, 12]);
    // Through the chart builder: a zone chart of individual values.
    let c = phased_chart_with(ChartKind::Zone, &singles(&x), &[0; 14], &ChartSettings::default()).unwrap();
    assert_eq!(c.zones.len(), 14);
    assert_eq!(c.location.statistic, ChartStatistic::Individual);
}

/// Short-run chart of two part numbers by hand.
#[test]
fn short_run_by_hand() {
    let x = [10.0, 50.0, 10.4, 51.0, 9.8, 49.5, 50.5, 10.2];
    let parts = vec![0, 1, 0, 1, 0, 1, 1, 0];
    let s = ChartSettings { span: 5, parts: Some(parts.clone()), ..ChartSettings::default() };
    let c = phased_chart_with(ChartKind::ShortRun, &singles(&x), &[0; 8], &s).unwrap();
    let d = d2(2).unwrap();
    let (s0, s1) = (1.4 / 3.0 / d, 3.5 / 3.0 / d);
    assert_eq!(c.parts.len(), 2);
    assert!(close(c.parts[0].mean, 10.1) && close(c.parts[0].sigma, s0) && c.parts[0].n == 4);
    assert!(close(c.parts[1].mean, 50.25) && close(c.parts[1].sigma, s1));
    let z: Vec<f64> = c.location.values.iter().map(|v| v.unwrap()).collect();
    assert!(close(z[0], (10.0 - 10.1) / s0));
    assert!(close(z[1], (50.0 - 50.25) / s1));
    assert!(close(c.dispersion.values[1].unwrap(), (z[1] - z[0]).abs()));
    let l = c.location.limits[3];
    assert_eq!((l.center, l.lower, l.upper), (0.0, -3.0, 3.0));
    let m = c.dispersion.limits[3];
    assert!(close(m.center, d) && close(m.upper, d + 3.0 * d3(2).unwrap()) && m.lower == 0.0);
    // Phases re-estimate each part.
    let more: Vec<f64> = x.iter().chain(x.iter()).map(|v| v + 1.0).collect();
    let parts2: Vec<usize> = parts.iter().chain(parts.iter()).copied().collect();
    let phases: Vec<usize> = (0..16).map(|i| usize::from(i >= 8)).collect();
    let p = phased_chart_with(
        ChartKind::ShortRun,
        &singles(&more),
        &phases,
        &ChartSettings { span: 5, parts: Some(parts2), ..ChartSettings::default() },
    )
    .unwrap();
    assert_eq!(p.parts.len(), 4);
    assert_eq!(p.parts[2].phase, 1);
    // Errors: a part with one value, a part without variation, missing parts.
    let lone = ChartSettings { span: 5, parts: Some(vec![0, 0, 0, 1]), ..ChartSettings::default() };
    let e = phased_chart_with(ChartKind::ShortRun, &singles(&[1.0, 2.0, 3.0, 4.0]), &[0; 4], &lone).unwrap_err();
    assert!(e.to_string().contains("part 2"), "{e}");
    let flat = ChartSettings { span: 5, parts: Some(vec![0, 1, 0, 1]), ..ChartSettings::default() };
    assert!(phased_chart_with(ChartKind::ShortRun, &singles(&[1.0, 2.0, 1.0, 3.0]), &[0; 4], &flat).is_err());
    assert!(phased_chart_with(ChartKind::ShortRun, &singles(&x), &[0; 8], &ChartSettings::default()).is_err());
}
