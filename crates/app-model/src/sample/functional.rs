//! The sample for functional data analysis.

use super::{Sample, col, round_to, table, text};
use crate::project::SavedReport;
use crate::{FunctionalLandmark, FunctionalRequest, ProfilerOptions};
use stat_data::{Column, DataTable, MeasurementLevel};

/// Reading interval (minutes) and number of readings per batch.
const STEP: f64 = 2.0;
const READINGS: usize = 61;

/// **Simulated** temperature profiles of 24 exothermic batches: a 3 × 2 × 2
/// factorial in catalyst (0.5, 1.0, 1.5%), feed rate (2 or 4 L/min) and
/// agitator (pitched blade or Rushton turbine), run twice, with the jacket
/// temperature read every 2 minutes for 2 hours. Each profile is
/// `25 + 0.03t` plus an exotherm peaking at
/// `τ = 50 − 12(c − 1) − 4(f − 3) + N(0, 3²)` minutes with height
/// `A = 30 + 15(c − 1) + 4(f − 3) + 5[Rushton] + N(0, 2²)` °C, a rise of
/// width `0.7w` and a fall of width `1.3w` (`w = 14`, 11 with the Rushton
/// turbine), plus reading noise N(0, 0.4²). Three readings are missing.
#[must_use]
pub fn exotherm_profiles() -> DataTable {
    let mut rng = stat_core::rng::Rng::new(0x00E0_7AE5);
    let mut batch = Vec::new();
    let (mut catalyst, mut feed, mut time, mut temperature) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut agitator = Vec::new();
    let mut b = 0;
    for _replicate in 0..2 {
        for &c in &[0.5, 1.0, 1.5] {
            for &f in &[2.0, 4.0] {
                for rushton in [false, true] {
                    b += 1;
                    let tau = 50.0 - 12.0 * (c - 1.0) - 4.0 * (f - 3.0) + 3.0 * rng.normal();
                    let height = 30.0
                        + 15.0 * (c - 1.0)
                        + 4.0 * (f - 3.0)
                        + if rushton { 5.0 } else { 0.0 }
                        + 2.0 * rng.normal();
                    let w = if rushton { 11.0 } else { 14.0 };
                    for j in 0..READINGS {
                        let t = STEP * j as f64;
                        let width = if t < tau { 0.7 * w } else { 1.3 * w };
                        let mut y = 25.0 + 0.03 * t + height * (-(t - tau).powi(2) / (2.0 * width * width)).exp();
                        y += 0.4 * rng.normal();
                        batch.push(format!("B{b:02}"));
                        catalyst.push(c);
                        feed.push(f);
                        agitator.push(if rushton { "Rushton turbine" } else { "Pitched blade" }.to_owned());
                        time.push(t);
                        temperature.push(round_to(y, 2));
                    }
                }
            }
        }
    }
    // A few readings were not recorded.
    for r in [40, 703, 1240] {
        temperature[r] = f64::NAN;
    }
    let mut feed_col = Column::numeric("Feed rate (L/min)", feed);
    feed_col.level = MeasurementLevel::Continuous;
    table(
        "Exotherm profiles (simulated)",
        vec![
            text("Batch", batch),
            Column::numeric("Catalyst (%)", catalyst),
            feed_col,
            text("Agitator", agitator),
            Column::numeric("Time (min)", time),
            Column::numeric("Temperature (°C)", temperature),
        ],
    )
}

/// The profiles as they are, with the scores modelled on the three
/// factors (so the profiler shows the exotherm moving earlier and growing
/// with catalyst), and the same profiles aligned on their peaks by landmark
/// and continuous registration.
pub(super) fn exotherm_profiles_sample() -> Sample {
    let t = exotherm_profiles();
    let c = |n: &str| col(&t, n);
    let factors = vec![c("Catalyst (%)"), c("Feed rate (L/min)"), c("Agitator")];
    let raw = FunctionalRequest {
        factors: factors.clone(),
        ..FunctionalRequest::new(c("Batch"), c("Time (min)"), c("Temperature (°C)"))
    };
    let registered = FunctionalRequest {
        landmarks: vec![FunctionalLandmark { maximum: true, window: Some((16.0, 100.0)) }],
        continuous: true,
        ..raw.clone()
    };
    let profilers = ProfilerOptions::default();
    Sample {
        reports: vec![
            SavedReport::FunctionalData { request: raw, profilers },
            SavedReport::FunctionalData { request: registered, profilers },
        ],
        table: t,
    }
}

#[cfg(test)]
mod tests {
    use crate::project::SavedReport;
    use stat_data::RowStateManager;

    /// The raw analysis models the scores on all three factors (catalyst
    /// strongly significant); registration moves every peak to the mean
    /// peak time.
    #[test]
    fn exotherm_profiles_analyses() {
        let s = super::exotherm_profiles_sample();
        let rows = RowStateManager::new(s.table.n_rows());
        let SavedReport::FunctionalData { request, .. } = &s.reports[0] else { panic!() };
        let a = crate::FunctionalAnalysis::run(&s.table, &rows, request).unwrap();
        assert_eq!(a.labels().len(), 24);
        assert!(a.has_model());
        let sections = a.sections();
        let model = sections.iter().find(|s| s.kind == crate::report::SectionKind::FunctionalScoreModel).unwrap();
        let crate::report::SectionContent::Table(t) = &model.content else { panic!() };
        let p: f64 = t.cell(0, "p-value").unwrap().parse().unwrap_or(0.0);
        assert!(p < 1e-4, "catalyst p = {p}");
        let SavedReport::FunctionalData { request, .. } = &s.reports[1] else { panic!() };
        let b = crate::FunctionalAnalysis::run(&s.table, &rows, request).unwrap();
        let peaks = stat_functional::find_landmarks(
            b.curves(),
            &[stat_functional::LandmarkSpec { kind: stat_functional::LandmarkKind::Maximum, window: (16.0, 100.0) }],
        )
        .unwrap();
        let lo = peaks.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min);
        let hi = peaks.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max);
        assert!(hi - lo < 3.0, "registered peaks span {lo}–{hi} min");
    }
}
