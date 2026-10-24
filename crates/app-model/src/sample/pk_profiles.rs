//! Plasma concentration profiles for the non-compartmental
//! pharmacokinetics report.

use super::{Sample, col, table};
use crate::pk_analysis::{PkDose, PkRequest};
use crate::project::SavedReport;
use stat_data::{Column, DataTable};

/// Sampling times (h) after the dose.
const TIMES: [f64; 12] = [0.0, 0.25, 0.5, 1.0, 1.5, 2.0, 3.0, 4.0, 6.0, 8.0, 12.0, 24.0];

/// Subject, formulation and concentrations (mg/L) at [`TIMES`].
#[rustfmt::skip]
const PROFILES: [(u32, &str, [f64; 12]); 24] = [
    (1, "R", [0.0, 1.288, 1.987, 2.693, 3.083, 2.88, 2.52, 2.392, 1.442, 1.042, 0.568, 0.064]),
    (1, "T", [0.0, 1.166, 2.05, 2.524, 3.566, 3.187, 2.316, 2.016, 1.402, 0.83, 0.556, 0.064]),
    (2, "R", [0.0, 0.641, 1.01, 1.266, 1.386, 1.498, 1.186, 1.126, 0.755, 0.515, 0.239, 0.0]),
    (2, "T", [0.0, 0.636, 0.845, 1.45, 1.125, 1.241, 1.1, 1.016, 0.699, 0.461, 0.248, 0.0]),
    (3, "R", [0.0, 1.394, 1.522, 2.407, 2.24, 1.935, 1.907, 1.674, 1.218, 1.01, 0.52, 0.115]),
    (3, "T", [0.0, 1.238, 1.976, 2.71, 2.784, 3.113, 3.039, 2.454, 1.67, 1.501, 0.833, 0.174]),
    (4, "R", [0.0, 0.863, 1.234, 1.879, 2.373, 2.07, 1.713, 1.463, 1.109, 0.75, 0.407, 0.054]),
    (4, "T", [0.0, 0.439, 0.846, 1.049, 1.357, 1.386, 1.336, 1.214, 0.987, 0.8, 0.327, 0.054]),
    (5, "R", [0.0, 1.675, 2.74, 2.96, 2.81, 2.732, 2.434, 2.183, 1.36, 1.056, 0.516, 0.092]),
    (5, "T", [0.0, 0.502, 0.957, 1.528, 1.582, 1.808, 1.713, 1.951, 1.302, 1.174, 0.606, 0.108]),
    (6, "R", [0.0, 0.973, 1.562, 2.066, 2.236, 1.928, 1.603, 1.302, 1.198, 0.618, 0.297, 0.0]),
    (6, "T", [0.0, 1.147, 1.694, 2.161, 2.264, 1.87, 1.452, 1.393, 1.015, 0.625, 0.282, 0.0]),
    (7, "R", [0.0, 1.023, 1.82, 2.132, 2.221, 2.396, 1.856, 1.57, 1.2, 0.896, 0.481, 0.078]),
    (7, "T", [0.0, 0.791, 1.341, 1.741, 2.076, 1.939, 1.823, 1.645, 1.169, 0.808, 0.531, 0.088]),
    (8, "R", [0.0, 1.587, 2.259, 2.759, 2.541, 2.768, 2.095, 1.677, 1.288, 0.883, 0.493, 0.082]),
    (8, "T", [0.0, 0.585, 1.262, 1.595, 1.841, 1.687, 1.909, 1.733, 1.147, 0.817, 0.425, 0.077]),
    (9, "R", [0.0, 1.165, 1.955, 2.913, 3.768, 3.627, 3.482, 2.931, 2.827, 1.973, 1.146, 0.253]),
    (9, "T", [0.0, 1.046, 1.699, 2.688, 3.099, 3.301, 3.191, 3.03, 2.33, 1.671, 1.11, 0.253]),
    (10, "R", [0.0, 1.109, 1.842, 2.015, 1.841, 1.685, 1.064, 0.934, 0.466, 0.247, 0.06, 0.0]),
    (10, "T", [0.0, 0.507, 0.813, 1.189, 1.394, 1.387, 1.28, 0.834, 0.589, 0.253, 0.071, 0.0]),
    (11, "R", [0.0, 0.517, 0.759, 1.274, 1.431, 1.585, 1.121, 1.171, 0.787, 0.483, 0.213, 0.0]),
    (11, "T", [0.0, 0.687, 0.982, 1.39, 1.478, 1.252, 1.109, 1.042, 0.597, 0.409, 0.195, 0.0]),
    (12, "R", [0.0, 0.62, 0.909, 1.5, 1.517, 1.623, 1.51, 1.178, 0.868, 0.665, 0.304, 0.0]),
    (12, "T", [0.0, 0.713, 1.356, 1.83, 1.741, 2.069, 1.473, 1.454, 0.879, 0.647, 0.279, 0.0]),
];

/// **Invented** plasma concentrations (mg/L) of 12 subjects who each took a
/// 100 mg oral dose of a reference (R) and a test (T) formulation, sampled
/// 12 times over 24 h (one-compartment absorption with 8% noise; values
/// below the quantification limit of 0.05 mg/L entered as 0). The same data
/// are the reference profiles of `stat-inference`'s NCA tests.
#[must_use]
pub fn pk_profiles() -> DataTable {
    let n = PROFILES.len() * TIMES.len();
    let (mut subject, mut formulation, mut time, mut conc) =
        (Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n));
    for (s, f, c) in PROFILES {
        for (t, v) in TIMES.iter().zip(c) {
            subject.push(format!("S{s:02}"));
            formulation.push(f.to_owned());
            time.push(*t);
            conc.push(v);
        }
    }
    table(
        "PK profiles",
        vec![
            Column::text("Subject", subject),
            Column::text("Formulation", formulation),
            Column::numeric("Time (h)", time),
            Column::numeric("Concentration (mg/L)", conc),
            Column::numeric("Dose (mg)", vec![100.0; n]),
        ],
    )
}

/// The PK sample: parameters by subject and formulation with the dose column.
pub(super) fn pk_profiles_sample() -> Sample {
    let t = pk_profiles();
    let request = PkRequest {
        treatment: Some(col(&t, "Formulation")),
        dose: PkDose::Column(col(&t, "Dose (mg)")),
        ..PkRequest::new(col(&t, "Subject"), col(&t, "Time (h)"), col(&t, "Concentration (mg/L)"))
    };
    Sample { table: t, reports: vec![SavedReport::Pharmacokinetics { request }] }
}
