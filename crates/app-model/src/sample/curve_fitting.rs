//! Samples for the curve-fitting options of nonlinear regression: a global
//! fit with shared parameters, a weighted standard curve with outlier
//! removal and unknowns read off the curve, and a differential-equation
//! model.

use super::{Sample, col, table};
use crate::nonlinear_analysis::{FitMethod, NonlinearAnalysis, NonlinearRequest, Weighting};
use crate::project::SavedReport;
use stat_data::Column;
use stat_linear::CurveForm;

fn saved(request: NonlinearRequest) -> SavedReport {
    SavedReport::Nonlinear { request, profilers: NonlinearAnalysis::default_profilers() }
}

/// **Simulated** responses (% of maximum) of two compounds at eleven log
/// concentrations in triplicate, from
/// `2 + 98 / (1 + 10^(LogEC50 − x))` with LogEC50 −7.0 (A) and −6.4 (B) and
/// normal errors of SD 4, rounded to 0.1. Drawn with NumPy's
/// `default_rng(20261018)`.
pub(super) fn compound_potency_sample() -> Sample {
    let x: Vec<f64> = (0..66).map(|i| -9.0 + 0.5 * f64::from((i % 33) / 3)).collect();
    let compound = (0..66).map(|i| if i < 33 { "A" } else { "B" }.to_owned()).collect();
    let t = table(
        "Compound potency (simulated)",
        vec![
            Column::numeric("Log concentration (M)", x),
            Column::text("Compound", compound),
            Column::numeric(
                "Response (%)",
                vec![
                    9.8, 3.7, 12.9, 7.3, 4.1, 7.3, 10.5, 11.1, 5.0, 31.0, 21.0, 22.7, 58.6, 48.0, 53.6, 76.1, 80.6,
                    74.1, 95.9, 90.4, 95.6, 90.9, 96.0, 98.6, 102.9, 106.7, 104.3, 93.9, 99.5, 96.8, 106.8, 102.7,
                    114.2, 1.0, -3.8, 7.4, 2.3, 2.3, -1.7, 5.8, 0.9, 2.2, 9.5, 12.1, 1.6, 29.8, 21.8, 20.2, 52.0, 38.1,
                    42.8, 76.4, 70.1, 73.8, 84.2, 91.8, 79.8, 96.1, 96.9, 91.6, 98.5, 95.9, 96.6, 101.4, 101.9, 100.1,
                ],
            ),
        ],
    );
    let mut request = NonlinearRequest::library(
        col(&t, "Response (%)"),
        col(&t, "Log concentration (M)"),
        CurveForm::DoseResponseLog,
    );
    request.group = Some(col(&t, "Compound"));
    // Bottom, Top and Hill slope shared; LogEC50 per compound.
    request.parameters[2].shared = false;
    Sample { reports: vec![saved(request)], table: t }
}

/// **Simulated** absorbances of an assay's standards (eight concentrations in
/// duplicate) from `0.05 + 2.15 / (1 + (10/x)^1.3)` with 3% relative
/// errors, one mis-pipetted well (row 10, 45% high), and six unknown samples
/// with an absorbance and no concentration. Drawn with NumPy's
/// `default_rng(20261018)` after the potency data.
pub(super) fn standard_curve_sample() -> Sample {
    let conc = [0.5, 1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0];
    let mut c: Vec<f64> = conc.iter().flat_map(|&v| [v, v]).collect();
    c.extend([f64::NAN; 6]);
    let mut sample: Vec<String> = vec!["Standard".to_owned(); 16];
    sample.extend((1..=6).map(|i| format!("Unknown {i}")));
    let t = table(
        "Assay standard curve (simulated)",
        vec![
            Column::text("Sample", sample),
            Column::numeric("Concentration (ng/ml)", c),
            Column::numeric(
                "Absorbance",
                vec![
                    0.09, 0.093, 0.164, 0.155, 0.296, 0.281, 0.524, 0.534, 0.981, 1.417, 1.497, 1.431, 1.831, 1.814,
                    1.963, 1.925, 0.21, 0.58, 1.02, 1.37, 1.86, 2.05,
                ],
            ),
        ],
    );
    let mut request =
        NonlinearRequest::library(col(&t, "Absorbance"), col(&t, "Concentration (ng/ml)"), CurveForm::DoseResponse);
    request.weighting = Weighting::InverseResponseSquared;
    request.method = FitMethod::RemoveOutliers { q: 0.01 };
    Sample { reports: vec![saved(request)], table: t }
}

/// **Simulated** substrate concentrations (mM) of an enzyme reaction every
/// 2.5 minutes, from the solution of `dS/dt = −2.5·S/(8 + S)`, `S(0) = 50`
/// (integrated Michaelis–Menten kinetics, solved with SciPy at tolerance
/// 10⁻¹²), with normal errors of SD 0.6, rounded to 0.01. Drawn with
/// NumPy's `default_rng(20261018)` after the standard curve.
pub(super) fn substrate_depletion_sample() -> Sample {
    let t = table(
        "Substrate depletion (simulated)",
        vec![
            Column::numeric("Time", (0..17).map(|i| 2.5 * f64::from(i)).collect()),
            Column::numeric(
                "Substrate",
                vec![
                    50.49, 44.95, 37.86, 34.06, 29.76, 23.67, 21.2, 15.38, 12.42, 8.53, 5.49, 2.99, 1.9, 1.65, 0.21,
                    -0.06, -0.27,
                ],
            ),
        ],
    );
    let request = NonlinearRequest::differential(
        col(&t, "Substrate"),
        col(&t, "Time"),
        "-Vmax * Substrate / (Km + Substrate)",
        "S0",
        &[("Vmax", 2.0), ("Km", 5.0), ("S0", 50.0)],
    );
    Sample { reports: vec![saved(request)], table: t }
}
