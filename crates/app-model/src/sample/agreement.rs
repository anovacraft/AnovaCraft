//! Simulated samples for the attribute measurement studies: a visual
//! inspection agreement study and a go/no-go gauge study. Each is drawn from
//! a stated model with a fixed seed.

use super::{Sample, col, table, text};
use crate::project::SavedReport;
use crate::{AgreementRequest, AttributeGaugeRequest};
use stat_core::distributions::{ContinuousDistribution, Normal};
use stat_core::rng::Rng;
use stat_data::{Column, DataTable, MeasurementLevel};

/// The lower limit the sample go/no-go gauge checks against (mm).
pub const GO_NO_GO_LIMIT: f64 = 12.0;

/// **Simulated** visual inspection study: three inspectors (A, B, C) each
/// grade 30 parts twice, in the order trial 1 (every inspector, every part)
/// then trial 2. Each part has a true quality `q` uniform on 0.6–5.4; its
/// standard grade is `q` rounded to 1–5 and its standard result is Pass for
/// grades 3 and above. An inspector sees `q + bias + σ·Z` (A: bias 0,
/// σ = 0.3; B: bias +0.25, lenient, σ = 0.5; C: bias −0.1, σ = 0.7), records
/// it rounded to 1–5 as the grade, and passes the part when it is 2.5 or
/// more. Grade and standard grade are ordinal.
#[must_use]
pub fn visual_inspection() -> DataTable {
    const INSPECTORS: [(&str, f64, f64); 3] = [("A", 0.0, 0.3), ("B", 0.25, 0.5), ("C", -0.1, 0.7)];
    let mut rng = Rng::new(0xA77E);
    let quality: Vec<f64> = (0..30).map(|_| 0.6 + 4.8 * rng.next_f64()).collect();
    let grade = |v: f64| v.round().clamp(1.0, 5.0);
    let result = |pass: bool| if pass { "Pass" } else { "Fail" }.to_owned();
    let (mut part, mut inspector, mut trial, mut res, mut gr, mut sres, mut sgr) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for t in 1..=2 {
        for (name, bias, sd) in INSPECTORS {
            for (p, &q) in quality.iter().enumerate() {
                let seen = q + bias + sd * rng.normal();
                part.push(format!("P{:02}", p + 1));
                inspector.push(name.to_owned());
                trial.push(f64::from(t));
                res.push(result(seen >= 2.5));
                gr.push(grade(seen));
                sres.push(result(grade(q) >= 3.0));
                sgr.push(grade(q));
            }
        }
    }
    let mut t = table(
        "Visual inspection (simulated)",
        vec![
            text("Part", part),
            text("Inspector", inspector),
            Column::numeric("Trial", trial),
            text("Result", res),
            Column::numeric("Grade", gr),
            text("Standard result", sres),
            Column::numeric("Standard grade", sgr),
        ],
    );
    for c in ["Grade", "Standard grade"] {
        t.set_level(col(&t, c), MeasurementLevel::Ordinal).expect("numeric columns can be ordinal");
    }
    t
}

pub(super) fn visual_inspection_sample() -> Sample {
    let t = visual_inspection();
    let request = |rating: &str, standard: &str| {
        let mut r = AgreementRequest::new(&t, col(&t, "Part"), col(&t, "Inspector"), col(&t, rating));
        r.trial = Some(col(&t, "Trial"));
        r.standard = Some(col(&t, standard));
        r
    };
    let reports = vec![
        SavedReport::AttributeAgreement { request: request("Result", "Standard result") },
        SavedReport::AttributeAgreement { request: request("Grade", "Standard grade") },
    ];
    Sample { table: t, reports }
}

/// **Simulated** go/no-go plug gauge study at a lower limit of 12.000 mm:
/// 16 reference parts from 11.960 to 12.035 mm (steps of 0.005 mm) each
/// checked 20 times. The gauge accepts a part of size `x` with probability
/// `Φ((x − 12.002)/0.010)`: it is centred 0.002 mm above the limit (a bias
/// of −0.002 mm) with a repeatability standard deviation of 0.010 mm.
#[must_use]
pub fn go_no_go_gauge() -> DataTable {
    let mut rng = Rng::new(0x60_60);
    let normal = Normal::standard();
    let (mut part, mut reference, mut accepted) = (Vec::new(), Vec::new(), Vec::new());
    for i in 0..16 {
        let x = 11.960 + 0.005 * f64::from(i);
        let p = normal.cdf((x - 12.002) / 0.010).expect("finite");
        let a = (0..20).filter(|_| rng.next_f64() < p).count();
        part.push(format!("R{:02}", i + 1));
        reference.push((x * 1000.0).round() / 1000.0);
        accepted.push(a as f64);
    }
    table(
        "Go/no-go gauge (simulated)",
        vec![
            text("Part", part),
            Column::numeric("Reference (mm)", reference),
            Column::numeric("Accepted", accepted),
            Column::numeric("Checks", vec![20.0; 16]),
        ],
    )
}

pub(super) fn go_no_go_gauge_sample() -> Sample {
    let t = go_no_go_gauge();
    let request = AttributeGaugeRequest::counts(
        col(&t, "Reference (mm)"),
        col(&t, "Accepted"),
        col(&t, "Checks"),
        GO_NO_GO_LIMIT,
    );
    Sample { reports: vec![SavedReport::AttributeGauge { request }], table: t }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_have_the_stated_shapes() {
        let v = visual_inspection();
        assert_eq!((v.n_rows(), v.n_cols()), (180, 7));
        assert_eq!(v.column(4).unwrap().level, MeasurementLevel::Ordinal);
        assert_eq!(visual_inspection(), v, "generated identically every time");
        let g = go_no_go_gauge();
        assert_eq!(g.n_rows(), 16);
        let acc = g.column(2).unwrap().as_numeric().unwrap();
        assert_eq!(acc[0], 0.0, "the smallest part is never accepted");
        assert_eq!(acc[15], 20.0, "the largest always");
        assert!(acc.iter().filter(|&&a| a > 0.0 && a < 20.0).count() >= 6, "{acc:?}");
    }
}
