//! Sample for value labels and row colours: coded columns whose codes have
//! names.

use super::{Sample, bivariate, linear, table};
use crate::ReportOptions;
use stat_core::rng::Rng;
use stat_data::{Column, DataTable, MeasurementLevel, TemporalDisplay, TemporalFormat};

/// Seconds in a day.
const DAY: f64 = 86_400.0;
/// 2026-01-05, the first visit day, in seconds since 1970-01-01.
const FIRST_VISIT: f64 = 20_458.0 * DAY;

/// An **invented** blood-pressure trial: 72 patients, randomized to placebo
/// (coded 0) or the active drug (coded 1), with sex coded 1 (female) and 2
/// (male) — the way such data often arrive from a case-report database.
/// Both coded columns carry value labels, so analyses, plots and keys read
/// "Placebo" / "Active" and "Female" / "Male" while the data keep the
/// codes. Systolic pressure at baseline and at week 8: the active arm falls
/// by about 9 mmHg more than placebo. Drawn with the workspace's
/// integer-based generator (seed `0xC0DE`), so the table is the same on
/// every platform.
#[must_use]
pub fn clinical_visits() -> DataTable {
    let mut rng = Rng::new(0xC0DE);
    let n = 72;
    let (mut id, mut arm, mut sex, mut visit, mut base, mut week8) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for i in 0..n {
        id.push(format!("P{:03}", i + 1));
        let a = (i % 2) as f64;
        arm.push(a);
        sex.push(1.0 + rng.below(2) as f64);
        visit.push(FIRST_VISIT + DAY * (i / 3) as f64);
        let b = 150.0 + 9.0 * rng.normal();
        base.push((b * 10.0f64).round() / 10.0);
        let change = -3.0 - 9.0 * a + 6.0 * rng.normal();
        week8.push(((b + change) * 10.0).round() / 10.0);
    }
    let labelled = |name: &str, values: Vec<f64>, labels: [(&str, &str); 2]| {
        let mut c = Column::numeric(name, values);
        c.level = MeasurementLevel::Nominal;
        c.value_labels = labels.iter().map(|(v, l)| ((*v).to_owned(), (*l).to_owned())).collect();
        c
    };
    let change: Vec<f64> = week8.iter().zip(&base).map(|(w, b)| ((w - b) * 10.0f64).round() / 10.0).collect();
    let mut patient = Column::text("Patient", id);
    patient.hover = true;
    table(
        "Clinical visits (invented)",
        vec![
            patient,
            labelled("Arm", arm, [("0", "Placebo"), ("1", "Active")]),
            labelled("Sex", sex, [("1", "Female"), ("2", "Male")]),
            Column::date_time("First visit", visit, TemporalDisplay::new(TemporalFormat::Date)),
            Column::numeric("Baseline SBP", base),
            Column::numeric("Week 8 SBP", week8),
            Column::numeric("Change", change),
        ],
    )
}

pub(super) fn clinical_visits_sample() -> Sample {
    let t = clinical_visits();
    let options = ReportOptions { show_residual_plot: true, ..ReportOptions::default() };
    let reports = vec![bivariate(&t, "Baseline SBP", "Week 8 SBP"), linear(&t, "Change", &["Arm"], options)];
    Sample { table: t, reports }
}

#[cfg(test)]
mod tests {
    use super::super::col;
    use super::*;
    use crate::project::SavedReport;
    use stat_data::RowStateManager;

    /// The model names the arms by their labels, the active arm lowers
    /// pressure, and colouring rows by the arm keys the plot by label.
    #[test]
    fn the_labels_reach_the_model_and_the_key() {
        let s = clinical_visits_sample();
        let t = &s.table;
        let rows = RowStateManager::new(t.n_rows());
        let SavedReport::Linear { spec, .. } = &s.reports[1] else { panic!("linear model expected") };
        let fit = crate::LinearModelAnalysis::run(t, &rows, spec).unwrap();
        assert_eq!(fit.one_way_levels().unwrap(), ["Active", "Placebo"]);
        let arm = t.column(col(t, "Arm")).unwrap();
        let change = t.column(col(t, "Change")).unwrap().as_numeric().unwrap();
        let mean = |code: f64| {
            let v: Vec<f64> =
                (0..t.n_rows()).filter(|&r| arm.as_numeric().unwrap()[r] == code).map(|r| change[r]).collect();
            v.iter().sum::<f64>() / v.len() as f64
        };
        assert!(mean(1.0) < mean(0.0) - 5.0, "the active arm falls further");
        let mut rows = rows;
        crate::action::color_by_column(t, &mut rows, "Arm").unwrap();
        let all: Vec<usize> = (0..t.n_rows()).collect();
        let Some(stat_viz::ColorKey::Levels { entries, .. }) = stat_viz::row_style_key(t, &rows, &all) else {
            panic!("a key by level")
        };
        assert_eq!(entries.iter().map(|e| e.0.as_str()).collect::<Vec<_>>(), ["Active", "Placebo"]);
    }
}
