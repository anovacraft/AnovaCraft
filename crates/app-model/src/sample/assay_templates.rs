//! Samples for the assay curve templates (Model ▸ Curve templates):
//! an immunoassay standard curve, saturation and competition binding,
//! enzyme inhibition, an antagonist shift, microbial growth, clonogenic
//! survival and a relative potency assay. All values are **simulated** with
//! NumPy's `default_rng(20261019)`, drawn in the order of this file; the
//! models and true values are given with each sample.

use super::{Sample, col, table};
use crate::curve_templates::{CurveTemplate, TemplateSettings};
use crate::nonlinear_analysis::{NonlinearAnalysis, NonlinearRequest};
use crate::project::SavedReport;
use stat_data::{Column, DataTable};

fn saved(request: NonlinearRequest) -> SavedReport {
    SavedReport::Nonlinear { request, profilers: NonlinearAnalysis::default_profilers() }
}

/// The template's request on `t`, or a plain sample if it cannot be built.
fn report(t: &DataTable, settings: &TemplateSettings, response: &str, group: Option<&str>) -> Vec<SavedReport> {
    settings.request(t, col(t, response), group.map(|g| col(t, g))).map(saved).into_iter().collect()
}

/// ELISA plate: two blanks, eight standards (2000 pg/ml down in twofold
/// steps) in duplicate from the 4PL curve `A = 0.06, B = 1.15, C = 180,
/// D = 2.85` with 3% relative and 0.003 absolute errors, and twelve diluted
/// samples, one above the curve's top standard and one below its lowest.
pub(super) fn elisa_sample() -> Sample {
    let t = table(
        "ELISA plate (simulated)",
        vec![
            Column::text(
                "Well",
                vec![
                    "Blank".to_owned(),
                    "Blank".to_owned(),
                    "Standard".to_owned(),
                    "Standard".to_owned(),
                    "Standard".to_owned(),
                    "Standard".to_owned(),
                    "Standard".to_owned(),
                    "Standard".to_owned(),
                    "Standard".to_owned(),
                    "Standard".to_owned(),
                    "Standard".to_owned(),
                    "Standard".to_owned(),
                    "Standard".to_owned(),
                    "Standard".to_owned(),
                    "Standard".to_owned(),
                    "Standard".to_owned(),
                    "Standard".to_owned(),
                    "Standard".to_owned(),
                    "Sample".to_owned(),
                    "Sample".to_owned(),
                    "Sample".to_owned(),
                    "Sample".to_owned(),
                    "Sample".to_owned(),
                    "Sample".to_owned(),
                    "Sample".to_owned(),
                    "Sample".to_owned(),
                    "Sample".to_owned(),
                    "Sample".to_owned(),
                    "Sample".to_owned(),
                    "Sample".to_owned(),
                ],
            ),
            Column::text(
                "Sample",
                vec![
                    "Blank".to_owned(),
                    "Blank".to_owned(),
                    "Std 1".to_owned(),
                    "Std 1".to_owned(),
                    "Std 2".to_owned(),
                    "Std 2".to_owned(),
                    "Std 3".to_owned(),
                    "Std 3".to_owned(),
                    "Std 4".to_owned(),
                    "Std 4".to_owned(),
                    "Std 5".to_owned(),
                    "Std 5".to_owned(),
                    "Std 6".to_owned(),
                    "Std 6".to_owned(),
                    "Std 7".to_owned(),
                    "Std 7".to_owned(),
                    "Std 8".to_owned(),
                    "Std 8".to_owned(),
                    "S01".to_owned(),
                    "S02".to_owned(),
                    "S03".to_owned(),
                    "S04".to_owned(),
                    "S05".to_owned(),
                    "S06".to_owned(),
                    "S07".to_owned(),
                    "S08".to_owned(),
                    "S09".to_owned(),
                    "S10".to_owned(),
                    "S11".to_owned(),
                    "S12".to_owned(),
                ],
            ),
            Column::numeric(
                "Concentration (pg/ml)",
                vec![
                    0.0,
                    0.0,
                    2000.0,
                    2000.0,
                    1000.0,
                    1000.0,
                    500.0,
                    500.0,
                    250.0,
                    250.0,
                    125.0,
                    125.0,
                    62.5,
                    62.5,
                    31.25,
                    31.25,
                    15.625,
                    15.625,
                    f64::NAN,
                    f64::NAN,
                    f64::NAN,
                    f64::NAN,
                    f64::NAN,
                    f64::NAN,
                    f64::NAN,
                    f64::NAN,
                    f64::NAN,
                    f64::NAN,
                    f64::NAN,
                    f64::NAN,
                ],
            ),
            Column::numeric(
                "OD 450",
                vec![
                    0.061, 0.056, 2.727, 2.743, 2.472, 2.443, 2.154, 2.225, 1.73, 1.732, 1.229, 1.163, 0.698, 0.717,
                    0.385, 0.4, 0.223, 0.212, 0.543, 1.118, 1.229, 1.873, 1.481, 0.359, 1.833, 0.141, 1.92, 0.815,
                    1.449, 1.673,
                ],
            ),
            Column::numeric(
                "Dilution factor",
                vec![
                    1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0,
                    2.0, 2.0, 5.0, 1.0, 5.0, 1.0, 10.0, 1.0, 2.0, 1.0,
                ],
            ),
        ],
    );
    let mut s = TemplateSettings::new(CurveTemplate::FourPl, col(&t, "Concentration (pg/ml)"));
    s.dilution = Some(col(&t, "Dilution factor"));
    let reports = report(&t, &s, "OD 450", None);
    Sample { reports, table: t }
}

/// Saturation binding: total and nonspecific binding at eight radioligand
/// concentrations in triplicate, from `Bmax = 1500 cpm, Kd = 1.2 nM`,
/// nonspecific `45 cpm/nM`, background 60 cpm, errors SD 35 (total) and 15
/// (nonspecific).
pub(super) fn saturation_sample() -> Sample {
    let conc: Vec<f64> = vec![
        0.1, 0.1, 0.1, 0.25, 0.25, 0.25, 0.5, 0.5, 0.5, 1.0, 1.0, 1.0, 2.0, 2.0, 2.0, 4.0, 4.0, 4.0, 8.0, 8.0, 8.0,
        16.0, 16.0, 16.0,
    ];
    let mut x = conc.clone();
    x.extend(&conc);
    let mut binding: Vec<f64> = vec![
        91.0, 148.0, 155.0, 342.0, 327.0, 294.0, 516.0, 554.0, 491.0, 750.0, 815.0, 699.0, 1205.0, 1047.0, 1134.0,
        1393.0, 1343.0, 1387.0, 1718.0, 1696.0, 1732.0, 2128.0, 2174.0, 2211.0,
    ];
    binding.extend(vec![
        80.0, 71.0, 67.0, 73.0, 64.0, 86.0, 110.0, 94.0, 72.0, 125.0, 115.0, 125.0, 150.0, 157.0, 151.0, 242.0, 239.0,
        216.0, 417.0, 449.0, 428.0, 776.0, 777.0, 788.0,
    ]);
    let measured = (0..x.len()).map(|i| if i < conc.len() { "Total" } else { "Nonspecific" }.to_owned()).collect();
    let t = table(
        "Receptor saturation (simulated)",
        vec![
            Column::numeric("Radioligand (nM)", x),
            Column::text("Measured", measured),
            Column::numeric("Binding (cpm)", binding),
        ],
    );
    let mut s = TemplateSettings::new(CurveTemplate::SaturationTotalNonspecific, col(&t, "Radioligand (nM)"));
    s.second = Some(col(&t, "Measured"));
    let reports = report(&t, &s, "Binding (cpm)", None);
    Sample { reports, table: t }
}

/// Competition binding: a competitor at thirteen log concentrations in
/// triplicate against 1 nM radioligand (Kd 0.5 nM), from `Top = 2400,
/// Bottom = 180, log IC50 = −7.6`, errors SD 60 cpm; Ki = IC50/3 ≈ 8.4 nM.
pub(super) fn competition_sample() -> Sample {
    let t = table(
        "Competition binding (simulated)",
        vec![
            Column::numeric(
                "log Competitor (M)",
                vec![
                    -11.0, -11.0, -11.0, -10.5, -10.5, -10.5, -10.0, -10.0, -10.0, -9.5, -9.5, -9.5, -9.0, -9.0, -9.0,
                    -8.5, -8.5, -8.5, -8.0, -8.0, -8.0, -7.5, -7.5, -7.5, -7.0, -7.0, -7.0, -6.5, -6.5, -6.5, -6.0,
                    -6.0, -6.0, -5.5, -5.5, -5.5, -5.0, -5.0, -5.0,
                ],
            ),
            Column::numeric(
                "Bound (cpm)",
                vec![
                    2390.0, 2438.0, 2441.0, 2324.0, 2359.0, 2428.0, 2485.0, 2304.0, 2364.0, 2309.0, 2288.0, 2287.0,
                    2288.0, 2373.0, 2207.0, 2209.0, 2122.0, 2106.0, 1881.0, 1786.0, 1764.0, 1245.0, 1184.0, 1162.0,
                    649.0, 672.0, 703.0, 292.0, 328.0, 242.0, 196.0, 135.0, 215.0, 214.0, 258.0, 247.0, 108.0, 139.0,
                    74.0,
                ],
            ),
        ],
    );
    let mut s = TemplateSettings::new(CurveTemplate::CompetitionOneSite, col(&t, "log Competitor (M)"));
    s.constants = vec![1e-9, 5e-10];
    let reports = report(&t, &s, "Bound (cpm)", None);
    Sample { reports, table: t }
}

/// Enzyme inhibition: rates at seven substrate and four inhibitor
/// concentrations from mixed inhibition with `Vmax = 120, Km = 6 µM,
/// Ki = 1.5 µM, alpha = 8` (close to competitive), errors SD 2.5.
pub(super) fn inhibition_sample() -> Sample {
    let t = table(
        "Enzyme inhibition (simulated)",
        vec![
            Column::numeric(
                "Substrate (µM)",
                vec![
                    1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0, 1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0, 1.0, 2.0, 4.0, 8.0,
                    16.0, 32.0, 64.0, 1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0,
                ],
            ),
            Column::numeric(
                "Inhibitor (µM)",
                vec![
                    0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 2.0, 2.0, 2.0, 2.0, 2.0, 2.0, 2.0, 5.0, 5.0, 5.0, 5.0, 5.0, 5.0,
                    5.0, 10.0, 10.0, 10.0, 10.0, 10.0, 10.0, 10.0,
                ],
            ),
            Column::numeric(
                "Rate (nmol/min)",
                vec![
                    18.5, 25.8, 49.3, 67.7, 88.9, 102.1, 110.3, 9.8, 15.6, 22.4, 38.8, 57.6, 75.6, 84.5, 1.7, 4.9,
                    21.0, 22.7, 37.2, 59.4, 71.3, 2.7, 6.1, 9.5, 17.6, 29.0, 36.4, 44.3,
                ],
            ),
        ],
    );
    let mut s = TemplateSettings::new(CurveTemplate::Mixed, col(&t, "Substrate (µM)"));
    s.second = Some(col(&t, "Inhibitor (µM)"));
    let reports = report(&t, &s, "Rate (nmol/min)", None);
    Sample { reports, table: t }
}

/// Antagonist shift: agonist curves (fourteen log concentrations) with no
/// antagonist and 3, 30 and 300 nM antagonist, from `Bottom = 2, Top = 98,
/// log EC50 = −8.2, Hill = 1.05, pA2 = 8.9`, Schild slope 1, errors SD 3.
pub(super) fn schild_sample() -> Sample {
    let t = table(
        "Antagonist shift (simulated)",
        vec![
            Column::numeric(
                "log Agonist (M)",
                vec![
                    -10.0, -9.5, -9.0, -8.5, -8.0, -7.5, -7.0, -6.5, -6.0, -5.5, -5.0, -4.5, -4.0, -3.5, -10.0, -9.5,
                    -9.0, -8.5, -8.0, -7.5, -7.0, -6.5, -6.0, -5.5, -5.0, -4.5, -4.0, -3.5, -10.0, -9.5, -9.0, -8.5,
                    -8.0, -7.5, -7.0, -6.5, -6.0, -5.5, -5.0, -4.5, -4.0, -3.5, -10.0, -9.5, -9.0, -8.5, -8.0, -7.5,
                    -7.0, -6.5, -6.0, -5.5, -5.0, -4.5, -4.0, -3.5,
                ],
            ),
            Column::numeric(
                "Antagonist (M)",
                vec![
                    0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 3e-09, 3e-09, 3e-09, 3e-09,
                    3e-09, 3e-09, 3e-09, 3e-09, 3e-09, 3e-09, 3e-09, 3e-09, 3e-09, 3e-09, 3e-08, 3e-08, 3e-08, 3e-08,
                    3e-08, 3e-08, 3e-08, 3e-08, 3e-08, 3e-08, 3e-08, 3e-08, 3e-08, 3e-08, 3e-07, 3e-07, 3e-07, 3e-07,
                    3e-07, 3e-07, 3e-07, 3e-07, 3e-07, 3e-07, 3e-07, 3e-07, 3e-07, 3e-07,
                ],
            ),
            Column::numeric(
                "Response (%)",
                vec![
                    5.5, 10.6, 14.7, 32.6, 62.6, 81.4, 92.5, 99.2, 95.2, 95.5, 100.7, 97.5, 98.2, 102.6, 2.3, 2.3, 5.9,
                    13.7, 31.1, 57.6, 84.7, 90.3, 95.8, 98.1, 95.9, 99.7, 97.4, 98.9, 4.2, 2.5, 0.1, 7.7, 10.6, 15.8,
                    39.3, 71.6, 88.3, 99.4, 100.4, 101.7, 99.3, 104.7, 3.8, 1.1, 2.8, 0.2, 1.7, 4.7, 8.2, 17.1, 32.7,
                    70.1, 83.8, 93.0, 91.9, 100.5,
                ],
            ),
        ],
    );
    let mut s = TemplateSettings::new(CurveTemplate::GaddumSchild, col(&t, "log Agonist (M)"));
    s.second = Some(col(&t, "Antagonist (M)"));
    let reports = report(&t, &s, "Response (%)", None);
    Sample { reports, table: t }
}

/// Microbial growth: ln(N/N0) of two strains every hour for 24 hours from
/// the modified Gompertz curve, strain A `A = 8.2, mu = 0.95/h, lambda = 2 h`
/// and strain B `A = 7.6, mu = 0.7/h, lambda = 3.5 h`, errors SD 0.12.
pub(super) fn growth_sample() -> Sample {
    let time: Vec<f64> = vec![
        0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0, 16.0, 17.0, 18.0, 19.0,
        20.0, 21.0, 22.0, 23.0, 24.0,
    ];
    let mut x = time.clone();
    x.extend(&time);
    let mut y: Vec<f64> = vec![
        0.215, 0.193, 0.486, 1.012, 2.017, 2.682, 3.58, 4.566, 5.571, 6.058, 6.547, 6.957, 7.457, 7.582, 7.758, 7.773,
        8.159, 8.048, 8.004, 7.928, 8.102, 8.3, 8.148, 8.071, 8.15,
    ];
    y.extend(vec![
        -0.019, 0.044, 0.134, 0.393, 0.677, 1.018, 1.724, 2.334, 3.477, 3.908, 4.524, 5.145, 5.658, 6.179, 6.339,
        6.532, 6.792, 6.734, 7.176, 7.254, 7.401, 7.379, 7.312, 7.35, 7.542,
    ]);
    let strain = (0..x.len()).map(|i| if i < time.len() { "A" } else { "B" }.to_owned()).collect();
    let t = table(
        "Bacterial growth (simulated)",
        vec![Column::numeric("Time (h)", x), Column::text("Strain", strain), Column::numeric("ln(N/N0)", y)],
    );
    let s = TemplateSettings::new(CurveTemplate::Gompertz, col(&t, "Time (h)"));
    let mut reports = report(&t, &s, "ln(N/N0)", Some("Strain"));
    // Separate curves for the two strains.
    if let Some(SavedReport::Nonlinear { request, .. }) = reports.first_mut() {
        request.parameters.iter_mut().for_each(|p| p.shared = false);
    }
    Sample { reports, table: t }
}

/// Clonogenic survival: colonies from cells plated at six doses in
/// triplicate, Poisson counts with plating efficiency 0.55 and surviving
/// fraction `exp(−0.22·D − 0.035·D²)` (α/β ≈ 6.3 Gy).
pub(super) fn clonogenic_sample() -> Sample {
    let t = table(
        "Clonogenic survival (simulated)",
        vec![
            Column::numeric(
                "Dose (Gy)",
                vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 2.0, 2.0, 2.0, 4.0, 4.0, 4.0, 6.0, 6.0, 6.0, 8.0, 8.0, 8.0],
            ),
            Column::numeric(
                "Cells plated",
                vec![
                    200.0, 200.0, 200.0, 300.0, 300.0, 300.0, 500.0, 500.0, 500.0, 1000.0, 1000.0, 1000.0, 3000.0,
                    3000.0, 3000.0, 10000.0, 10000.0, 10000.0,
                ],
            ),
            Column::numeric(
                "Colonies",
                vec![
                    103.0, 119.0, 106.0, 121.0, 125.0, 132.0, 157.0, 146.0, 185.0, 157.0, 119.0, 117.0, 126.0, 125.0,
                    155.0, 98.0, 95.0, 94.0,
                ],
            ),
            Column::numeric(
                "Colonies per cell plated",
                vec![
                    0.515,
                    0.595,
                    0.53,
                    0.4033333333333333,
                    0.4166666666666667,
                    0.44,
                    0.314,
                    0.292,
                    0.37,
                    0.157,
                    0.119,
                    0.117,
                    0.042,
                    0.041666666666666664,
                    0.051666666666666666,
                    0.0098,
                    0.0095,
                    0.0094,
                ],
            ),
        ],
    );
    let s = TemplateSettings::new(CurveTemplate::LinearQuadratic, col(&t, "Dose (Gy)"));
    let reports = report(&t, &s, "Colonies per cell plated", None);
    Sample { reports, table: t }
}

/// Potency assay: a reference standard and a test lot at ten log
/// concentrations in duplicate, parallel curves `Bottom = 3, Top = 100,
/// Hill = 1.1` with log EC50 −7.0 (reference) and −6.75 (test lot): the
/// test lot has about 56% of the reference's potency. Errors SD 3.
pub(super) fn potency_sample() -> Sample {
    let preparation = (0..40).map(|i| if i < 20 { "Reference" } else { "Test lot" }.to_owned()).collect();
    let t = table(
        "Potency assay (simulated)",
        vec![
            Column::numeric(
                "log Concentration (M)",
                vec![
                    -9.0, -9.0, -8.5, -8.5, -8.0, -8.0, -7.5, -7.5, -7.0, -7.0, -6.5, -6.5, -6.0, -6.0, -5.5, -5.5,
                    -5.0, -5.0, -4.5, -4.5, -9.0, -9.0, -8.5, -8.5, -8.0, -8.0, -7.5, -7.5, -7.0, -7.0, -6.5, -6.5,
                    -6.0, -6.0, -5.5, -5.5, -5.0, -5.0, -4.5, -4.5,
                ],
            ),
            Column::text("Preparation", preparation),
            Column::numeric(
                "Response (%)",
                vec![
                    5.7, 1.9, 1.4, 5.6, 5.8, 11.6, 22.9, 25.7, 54.7, 51.8, 81.6, 86.0, 99.7, 90.3, 99.8, 101.3, 96.2,
                    101.6, 100.9, 96.3, 6.9, -0.9, 6.1, 1.4, 7.1, 4.4, 18.2, 22.6, 40.4, 40.7, 67.7, 67.4, 87.8, 87.8,
                    94.2, 94.8, 99.5, 102.8, 96.2, 101.6,
                ],
            ),
        ],
    );
    let mut s = TemplateSettings::new(CurveTemplate::RelativePotency, col(&t, "log Concentration (M)"));
    s.standard = Some("Reference".to_owned());
    let reports = report(&t, &s, "Response (%)", Some("Preparation"));
    Sample { reports, table: t }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::{SectionContent, SectionKind};
    use stat_data::RowStateManager;

    fn run(sample: Sample) -> (NonlinearAnalysis, DataTable, NonlinearRequest) {
        let SavedReport::Nonlinear { request, .. } = sample.reports.into_iter().next().expect("a report") else {
            panic!("nonlinear report")
        };
        let n = sample.table.n_rows();
        let a = NonlinearAnalysis::run(&sample.table, &RowStateManager::new(n), &request).unwrap();
        (a, sample.table, request)
    }

    fn table(a: &NonlinearAnalysis, kind: SectionKind) -> Vec<Vec<String>> {
        let sections = a.sections();
        let section = sections.iter().find(|s| s.kind == kind).unwrap_or_else(|| panic!("no {kind:?} section"));
        match &section.content {
            SectionContent::Table(t) => t.rows.clone(),
            other => panic!("{kind:?}: expected a table, got {other:?}"),
        }
    }

    fn value(rows: &[Vec<String>], label: &str) -> String {
        rows.iter().find(|r| r[0].starts_with(label)).unwrap_or_else(|| panic!("no row {label}"))[1].clone()
    }

    #[test]
    fn standard_curve_checks_standards_and_reads_samples() {
        let (a, _, _) = run(elisa_sample());
        let back = table(&a, SectionKind::StandardBackCalculation);
        assert_eq!(back.len(), 16, "sixteen positive standards");
        // Every standard is read off the curve; the flattened top level fails.
        assert!(back.iter().all(|r| r[3].parse::<f64>().is_ok()), "{back:?}");
        assert_eq!(back.iter().filter(|r| r[6] == "no").count(), 2);
        let acc = table(&a, SectionKind::StandardAcceptance);
        assert_eq!(value(&acc, "Curve accepted"), "yes");
        assert_eq!(value(&acc, "Standards within limits"), "14 of 16");
        let lloq: f64 = value(&acc, "Lower limit").parse().unwrap();
        let uloq: f64 = value(&acc, "Upper limit").parse().unwrap();
        assert!(lloq >= 15.625 && lloq < uloq && uloq <= 2000.0, "{lloq} {uloq}");
        let samples = table(&a, SectionKind::SampleConcentrations);
        assert_eq!(samples.len(), 12);
        for r in &samples {
            // Concentration = read-off × dilution, inside its interval.
            let (read, df, conc): (f64, f64, f64) =
                (r[2].parse().unwrap(), r[3].parse().unwrap(), r[4].parse().unwrap());
            let (lo, hi): (f64, f64) = (r[5].parse().unwrap(), r[6].parse().unwrap());
            assert!((conc - read * df).abs() <= 1e-4 * conc, "{r:?}");
            assert!(lo < conc && conc < hi, "{r:?}");
            assert_eq!(r[7] == "yes", read >= lloq && read <= uloq, "{r:?}");
        }
    }

    #[test]
    fn inhibition_mechanisms_prefer_the_simulated_one() {
        let (a, _, _) = run(inhibition_sample());
        let rows = table(&a, SectionKind::InhibitionMechanisms);
        assert_eq!(rows.len(), 4);
        let best = rows.iter().find(|r| r[4] == "0").expect("a best mechanism");
        assert_eq!(best[0], "Mixed inhibition");
        let weights: f64 = rows.iter().map(|r| r[5].parse::<f64>().unwrap()).sum();
        assert!((weights - 100.0).abs() < 1e-3, "Akaike weights sum to 100%: {weights}");
    }

    #[test]
    fn schild_slope_is_tested_against_one() {
        let (a, _, _) = run(schild_sample());
        let sections = a.sections();
        let schild = sections.iter().find(|s| s.kind == SectionKind::SchildPlot).expect("Schild section");
        assert!(matches!(schild.content, SectionContent::Plot(_)), "{:?}", schild.content);
        assert!(schild.title.contains("t for slope = 1"), "{}", schild.title);
        let compare = table(&a, SectionKind::NonlinearComparison);
        assert!(compare.iter().any(|r| r[0].starts_with("Is the Schild slope 1?")));
    }

    #[test]
    fn relative_potency_is_the_ratio_of_ec50s() {
        let (a, _, _) = run(potency_sample());
        let rows = table(&a, SectionKind::RelativePotency);
        let (std, test): (f64, f64) = (rows[0][1].parse().unwrap(), rows[1][1].parse().unwrap());
        let rp: f64 = rows[1][2].parse().unwrap();
        assert!((rp - 10f64.powf(std - test)).abs() < 1e-5 * rp, "{rp}");
        let (lo, hi): (f64, f64) = (rows[1][3].parse().unwrap(), rows[1][4].parse().unwrap());
        assert!(lo < rp && rp < hi);
        let compare = table(&a, SectionKind::NonlinearComparison);
        assert!(compare.iter().any(|r| r[0].starts_with("Are the curves parallel?")));
    }

    #[test]
    fn template_requests_survive_a_project_round_trip() {
        for sample in [
            elisa_sample(),
            saturation_sample(),
            competition_sample(),
            inhibition_sample(),
            schild_sample(),
            growth_sample(),
            clonogenic_sample(),
            potency_sample(),
        ] {
            let SavedReport::Nonlinear { request, profilers } = sample.reports[0].clone() else { panic!() };
            let json = crate::nonlinear_analysis::to_json(&request, profilers, &sample.table);
            let SavedReport::Nonlinear { request: back, .. } =
                crate::nonlinear_analysis::from_json(&json, &sample.table).unwrap()
            else {
                panic!()
            };
            assert_eq!(back.template, request.template);
            assert_eq!(back.weighting, request.weighting);
            assert_eq!(back.group, request.group);
        }
        // A script may give the template alone: the formula, starts and
        // weighting come from the data.
        let sample = elisa_sample();
        let json = stat_data::parse_json(
            r#"{"analysis": "nonlinear-regression", "response": "OD 450",
                "template": {"key": "standard_4pl", "x": "Concentration (pg/ml)", "dilution": "Dilution factor"}}"#,
        )
        .unwrap();
        let SavedReport::Nonlinear { request, .. } =
            crate::nonlinear_analysis::from_json(&json, &sample.table).unwrap()
        else {
            panic!()
        };
        let SavedReport::Nonlinear { request: expected, .. } = &sample.reports[0] else { panic!() };
        assert_eq!(request.weighting, expected.weighting);
        assert_eq!(request.parameters, expected.parameters);
        assert_eq!(request.template, expected.template);
    }
}
