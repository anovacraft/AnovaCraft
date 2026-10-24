//! Sample for treatment effects from observational data.

use super::{Sample, col};
use crate::project::SavedReport;
use crate::{EffectMethod, Estimand, MatchingOptions, TreatmentEffectsRequest, TreatmentOutcome};
use stat_data::{DataTable, MeasurementLevel};

/// **Invented** observational study of a cardiac rehabilitation programme:
/// 400 patients, 170 of whom enrolled. Enrolment was more likely for
/// younger patients, women, milder disease, non-smokers and the South
/// region, and those also predict the outcomes (fitness gain, readmission
/// within a year, clinic visits), so the unadjusted comparison is
/// confounded. The programme's true effect on fitness gain is 4 points for
/// patients under 65 and 1.5 for older ones. Written by
/// `tests/data/treatment_study.py` (NumPy, seed 2519).
#[must_use]
pub fn treatment_study() -> DataTable {
    let mut t = stat_data::read_csv("Cardiac rehabilitation", include_str!("../../tests/data/treatment_study.csv"))
        .expect("the bundled study parses");
    for (name, level) in [("Patient", MeasurementLevel::Ordinal), ("Readmitted", MeasurementLevel::Nominal)] {
        let i = t.find(name).expect("column present");
        t.set_level(i, level).expect("a numeric column takes any level");
    }
    t
}

/// The study with a doubly robust estimate of the programme's effect on
/// fitness gain by age group and sex, and 1:1 matching for its effect on
/// readmission among those enrolled.
#[must_use]
pub fn treatment_study_sample() -> Sample {
    let t = treatment_study();
    let covariates: Vec<usize> = ["Age", "Sex", "Severity", "Smoker", "Region"].iter().map(|c| col(&t, c)).collect();
    let mut fitness = TreatmentEffectsRequest::new(
        col(&t, "Programme"),
        col(&t, "Fitness gain"),
        TreatmentOutcome::Continuous,
        covariates.clone(),
    );
    fitness.treated_level = Some("Enrolled".to_owned());
    fitness.method = EffectMethod::DoublyRobust;
    fitness.subgroups = vec![col(&t, "Age group"), col(&t, "Sex")];
    let mut readmission =
        TreatmentEffectsRequest::new(col(&t, "Programme"), col(&t, "Readmitted"), TreatmentOutcome::Binary, covariates);
    readmission.treated_level = Some("Enrolled".to_owned());
    readmission.method = EffectMethod::Matching(MatchingOptions::default());
    readmission.estimand = Estimand::Att;
    Sample {
        reports: vec![
            SavedReport::TreatmentEffects { request: fitness },
            SavedReport::TreatmentEffects { request: readmission },
        ],
        table: t,
    }
}
