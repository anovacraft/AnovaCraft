//! Sample for latent class analysis.

use super::{Sample, col, table};
use crate::LatentClassRequest;
use crate::project::SavedReport;
use stat_core::rng::Rng;
use stat_data::{Column, DataTable, MeasurementLevel};

/// The checklist items of [`symptom_checklist`], in column order.
pub const SYMPTOM_ITEMS: [&str; 7] =
    ["Fatigue", "Poor sleep", "Headache", "Muscle pain", "Low mood", "Worry", "Poor concentration"];

/// Levels of the three-level items (their alphabetical order is their
/// severity order).
const SEVERITY: [&str; 3] = ["Absent", "Mild", "Severe"];
const YES_NO: [&str; 2] = ["No", "Yes"];

/// Simulated profiles: name and share.
const PROFILES: [(&str, f64); 3] = [("Few symptoms", 0.5), ("Fatigue and pain", 0.3), ("Mood and worry", 0.2)];

/// Level probabilities of each item (rows) in each profile (columns of
/// `PROFILES`).
const ITEM_PROBABILITIES: [[&[f64]; 3]; 7] = [
    [&[0.75, 0.20, 0.05], &[0.05, 0.35, 0.60], &[0.30, 0.50, 0.20]],
    [&[0.70, 0.25, 0.05], &[0.15, 0.40, 0.45], &[0.10, 0.40, 0.50]],
    [&[0.85, 0.15], &[0.40, 0.60], &[0.60, 0.40]],
    [&[0.90, 0.10], &[0.25, 0.75], &[0.80, 0.20]],
    [&[0.80, 0.17, 0.03], &[0.55, 0.35, 0.10], &[0.05, 0.35, 0.60]],
    [&[0.85, 0.15], &[0.70, 0.30], &[0.15, 0.85]],
    [&[0.90, 0.10], &[0.50, 0.50], &[0.35, 0.65]],
];

fn draw(rng: &mut Rng, probabilities: &[f64]) -> usize {
    let u = rng.next_f64();
    let mut cum = 0.0;
    for (i, p) in probabilities.iter().enumerate() {
        cum += p;
        if u < cum {
            return i;
        }
    }
    probabilities.len() - 1
}

/// **Simulated** answers of 600 respondents to a seven-item symptom
/// checklist. Each respondent belongs to one of three profiles (few
/// symptoms 50%, fatigue and pain 30%, mood and worry 20%) and, given the
/// profile, answers the items independently with the probabilities in
/// `ITEM_PROBABILITIES`: Fatigue, Poor sleep and Low mood as
/// Absent/Mild/Severe (ordinal), Headache, Muscle pain, Worry and Poor
/// concentration as No/Yes (nominal). Each answer is left blank with
/// probability 0.03 (missing at random). Age is uniform on 18–80, unrelated
/// to the profile. Drawn with the SplitMix64 generator seeded 0x5C1A; the
/// generating profile is kept in a column to compare with the classes found.
#[must_use]
pub fn symptom_checklist() -> DataTable {
    let mut rng = Rng::new(0x5C1A);
    let n = 600;
    let mut profile = Vec::with_capacity(n);
    let mut answers: Vec<Vec<String>> = vec![Vec::with_capacity(n); SYMPTOM_ITEMS.len()];
    let mut age = Vec::with_capacity(n);
    let shares: Vec<f64> = PROFILES.iter().map(|p| p.1).collect();
    for _ in 0..n {
        let k = draw(&mut rng, &shares);
        profile.push(PROFILES[k].0.to_owned());
        for (j, column) in answers.iter_mut().enumerate() {
            let probs = ITEM_PROBABILITIES[j][k];
            let level = draw(&mut rng, probs);
            let missing = rng.next_f64() < 0.03;
            let labels: &[&str] = if probs.len() == 3 { &SEVERITY } else { &YES_NO };
            column.push(if missing { String::new() } else { labels[level].to_owned() });
        }
        age.push((18.0 + 62.0 * rng.next_f64()).floor());
    }
    let mut id = Column::numeric("Respondent", (1..=n).map(|i| i as f64).collect());
    id.level = MeasurementLevel::Nominal;
    let mut columns = vec![id, Column::numeric("Age", age)];
    for (j, values) in answers.into_iter().enumerate() {
        let mut c = Column::text(SYMPTOM_ITEMS[j], values);
        if ITEM_PROBABILITIES[j][0].len() == 3 {
            c.level = MeasurementLevel::Ordinal;
        }
        columns.push(c);
    }
    columns.push(Column::text("Simulated profile", profile));
    table("Symptom checklist (simulated)", columns)
}

pub(crate) fn symptom_checklist_sample() -> Sample {
    let t = symptom_checklist();
    let items = SYMPTOM_ITEMS.iter().map(|n| col(&t, n)).collect();
    Sample { reports: vec![SavedReport::LatentClass { request: LatentClassRequest::new(items) }], table: t }
}
