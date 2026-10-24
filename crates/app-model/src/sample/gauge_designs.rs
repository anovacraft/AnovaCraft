//! Simulated samples for the nested (destructive) and expanded gauge R&R
//! studies. Each is drawn from a stated model with a fixed seed.

use super::{Sample, col, round_to, table, text};
use crate::project::SavedReport;
use crate::{
    GaugeExtraFactor, GaugeFactorNesting, GaugeReportOptions, GaugeStudyDesign, GaugeStudyKind, GaugeStudyRequest,
};
use stat_core::rng::Rng;
use stat_data::{Column, DataTable};

/// **Simulated** destructive weld pull test: three operators (A, B, C) each
/// test five batches of their own (numbered 1–5 within each operator), three
/// coupons cut from each batch, 45 pulls in all, in the order the column
/// `Run` gives. Pull strength (N) is
/// `480 + O + B(O) + ε` with operator effects O ~ N(0, 4²), batch effects
/// B(O) ~ N(0, 10²) and coupon-to-coupon (repeatability) error ε ~ N(0, 5²);
/// values rounded to 0.1 N. Specification 440–520 N.
#[must_use]
pub fn weld_pull_strength() -> DataTable {
    let mut rng = Rng::new(0x3E1D);
    let (mut op, mut batch, mut coupon, mut run, mut strength) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for o in ["A", "B", "C"] {
        let oe = 4.0 * rng.normal();
        for b in 1..=5 {
            let be = 10.0 * rng.normal();
            for c in 1..=3 {
                op.push(o.to_owned());
                batch.push(b.to_string());
                coupon.push(f64::from(c));
                run.push((run.len() + 1) as f64);
                strength.push(round_to(480.0 + oe + be + 5.0 * rng.normal(), 1));
            }
        }
    }
    table(
        "Weld pull test (simulated)",
        vec![
            Column::numeric("Run", run),
            text("Operator", op),
            text("Part", batch),
            Column::numeric("Coupon", coupon),
            Column::numeric("Strength", strength),
        ],
    )
}

pub(super) fn weld_pull_sample() -> Sample {
    let t = weld_pull_strength();
    let request = GaugeStudyRequest {
        tolerance: Some(80.0),
        design: GaugeStudyDesign { run_order: Some(col(&t, "Run")), ..GaugeStudyDesign::nested() },
        ..GaugeStudyRequest::new(col(&t, "Part"), col(&t, "Operator"), col(&t, "Strength"))
    };
    let options =
        GaugeReportOptions { show_run_chart: true, show_operator_plot: true, ..GaugeReportOptions::default() };
    Sample { reports: vec![SavedReport::Gauge { request, options }], table: t }
}

/// **Simulated** bore diameter study with two gauges: eight parts measured
/// twice by each of three operators on each of two bore gauges (G1, G2),
/// 96 planned readings of which seven were lost, leaving 89 (unbalanced).
/// Diameter (mm) is `25 + P + O + G + PO + OG + ε` with part effects
/// N(0, 0.020²), operator N(0, 0.004²), gauge N(0, 0.005²),
/// part × operator N(0, 0.002²), operator × gauge N(0, 0.002²) and
/// repeatability ε ~ N(0, 0.003²); readings rounded to 0.001 mm.
/// Tolerance 0.1 mm.
#[must_use]
pub fn bore_diameter() -> DataTable {
    let mut rng = Rng::new(0xB0E2);
    let pe: Vec<f64> = (0..8).map(|_| 0.020 * rng.normal()).collect();
    let oe: Vec<f64> = (0..3).map(|_| 0.004 * rng.normal()).collect();
    let ge: Vec<f64> = (0..2).map(|_| 0.005 * rng.normal()).collect();
    let po: Vec<f64> = (0..24).map(|_| 0.002 * rng.normal()).collect();
    let og: Vec<f64> = (0..6).map(|_| 0.002 * rng.normal()).collect();
    // Planned runs lost (part, operator, gauge, repeat), numbered from 0.
    let lost = [(1, 0, 1, 1), (2, 2, 0, 0), (3, 1, 1, 0), (4, 0, 0, 1), (5, 2, 1, 1), (6, 1, 0, 0), (7, 0, 1, 0)];
    let (mut run, mut part, mut op, mut gauge, mut value) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut n = 0.0;
    for o in 0..3 {
        for g in 0..2 {
            for r in 0..2 {
                for p in 0..8 {
                    n += 1.0;
                    let y = 25.0 + pe[p] + oe[o] + ge[g] + po[p * 3 + o] + og[o * 2 + g] + 0.003 * rng.normal();
                    if lost.contains(&(p, o, g, r)) {
                        continue;
                    }
                    run.push(n);
                    part.push(format!("P{}", p + 1));
                    op.push(["Dana", "Eli", "Fay"][o].to_owned());
                    gauge.push(format!("G{}", g + 1));
                    value.push(round_to(y, 3));
                }
            }
        }
    }
    table(
        "Bore gauges (simulated)",
        vec![
            Column::numeric("Run", run),
            text("Part", part),
            text("Operator", op),
            text("Gauge", gauge),
            Column::numeric("Diameter", value),
        ],
    )
}

pub(super) fn bore_diameter_sample() -> Sample {
    let t = bore_diameter();
    let request = GaugeStudyRequest {
        tolerance: Some(0.1),
        design: GaugeStudyDesign {
            kind: GaugeStudyKind::Expanded,
            factors: vec![GaugeExtraFactor {
                column: col(&t, "Gauge"),
                random: true,
                nesting: GaugeFactorNesting::Crossed,
            }],
            run_order: Some(col(&t, "Run")),
            ..GaugeStudyDesign::default()
        },
        ..GaugeStudyRequest::new(col(&t, "Part"), col(&t, "Operator"), col(&t, "Diameter"))
    };
    let options = GaugeReportOptions { show_run_chart: true, ..GaugeReportOptions::default() };
    Sample { reports: vec![SavedReport::Gauge { request, options }], table: t }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GaugeAnalysis;
    use crate::report::SectionKind;
    use stat_data::RowStateManager;

    #[test]
    fn weld_sample_runs_as_a_nested_study() {
        let s = weld_pull_sample();
        assert_eq!(s.table.n_rows(), 45);
        assert_eq!(weld_pull_strength(), s.table, "generated identically every time");
        let SavedReport::Gauge { request, options } = &s.reports[0] else { panic!() };
        let a = GaugeAnalysis::run(&s.table, &RowStateManager::new(45), request).unwrap();
        let st = a.study().unwrap();
        assert_eq!((st.parts, st.operators, st.replicates), (15, 3, 3));
        // Batch-to-batch variation dominates, as simulated (σ_B = 10 vs σ = 5).
        assert!(st.components.part.variance > st.components.repeatability.variance);
        let kinds: Vec<_> = a.sections(options).iter().map(|x| x.kind).collect();
        assert!(kinds.contains(&SectionKind::GaugeRunChartByPart));
        assert!(kinds.contains(&SectionKind::MeasurementsByOperator));
    }

    #[test]
    fn bore_sample_runs_as_an_expanded_study() {
        let s = bore_diameter_sample();
        assert_eq!(s.table.n_rows(), 89);
        let SavedReport::Gauge { request, options } = &s.reports[0] else { panic!() };
        let a = GaugeAnalysis::run(&s.table, &RowStateManager::new(89), request).unwrap();
        let e = a.expanded().unwrap();
        let sources: Vec<&str> = e.components.iter().map(|c| c.source.as_str()).collect();
        assert_eq!(
            sources,
            ["Part", "Operator", "Gauge", "Part × Operator", "Part × Gauge", "Operator × Gauge", "Repeatability"]
        );
        // Parts dominate (σ_P = 0.020 against a gauge R&R of about 0.0075).
        assert!(e.part.summary.variance > e.gauge_rr.summary.variance);
        let kinds: Vec<_> = a.sections(options).iter().map(|x| x.kind).collect();
        assert_eq!(kinds[..2], [SectionKind::VarianceComponents, SectionKind::GaugeComponentIntervals]);
    }
}
