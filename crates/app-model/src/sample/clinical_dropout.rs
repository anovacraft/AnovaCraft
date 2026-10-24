//! Sample project for multiple imputation: a two-arm trial with dropout,
//! its multiple imputation, and the stacked table with pooled analyses.

use super::{col, round_to, table};
use crate::compare::{GroupComparisonRequest, GroupLayout};
use crate::multiple_imputation::{IMPUTATION_COLUMN, MiSettings};
use crate::project::{ProjectFile, ProjectLayout, ProjectTable, SavedReport, TableId, TableLineage};
use crate::report_scope::ReportScope;
use crate::table_ops::Source;
use crate::{ImputationMethod, ImputationRequest, LinearModelSpec, ReportOptions};
use stat_core::rng::Rng;
use stat_data::{Column, DataTable, MeasurementLevel, RowStateManager};

/// Columns of the trial table imputed (with Arm and Sex as predictors).
pub const TRIAL_COLUMNS: [&str; 7] = ["Arm", "Sex", "Age", "Baseline", "Week 4", "Week 8", "Week 12"];

/// **Simulated** symptom scores (lower is better) of 120 patients in a
/// two-arm trial, 60 per arm, at baseline and weeks 4, 8 and 12. Baseline
/// `B ~ N(50, 8²)`; at visit `k` (1–3) the score is
/// `10 + 0.8 B − d k + u + e` with `d = 1` on placebo and `2.5` on the active
/// treatment, a patient effect `u ~ N(0, 4²)` and `e ~ N(0, 3²)`. Patients
/// drop out after visit 1 or 2 with probability `1/(1 + exp(3 − 0.12(y − 45)))`
/// given the score `y` they just had, so the missing values are missing at
/// random given the observed scores (worse scores drop out more, and more
/// often on placebo). About 5 % of ages are lost at random. Seed 2112, so the
/// table is the same on every platform.
#[must_use]
pub fn clinical_trial() -> DataTable {
    let mut rng = Rng::new(2112);
    let n = 120;
    let mut cols: Vec<Vec<f64>> = vec![Vec::new(); 5];
    let (mut arm, mut sex, mut id) = (Vec::new(), Vec::new(), Vec::new());
    for i in 0..n {
        let active = i % 2 == 1;
        let baseline = 50.0 + 8.0 * rng.normal();
        let u = 4.0 * rng.normal();
        let age = (55.0 + 10.0 * rng.normal()).round().clamp(18.0, 85.0);
        let mut row = vec![if rng.next_f64() < 0.05 { f64::NAN } else { age }, round_to(baseline, 1)];
        let mut dropped = false;
        for k in 1..=3 {
            let d = if active { 2.5 } else { 1.0 };
            let y = 10.0 + 0.8 * baseline - d * f64::from(k) + u + 3.0 * rng.normal();
            row.push(if dropped { f64::NAN } else { round_to(y, 1) });
            if !dropped && k < 3 {
                dropped = rng.next_f64() < 1.0 / (1.0 + (3.0 - 0.12 * (y - 45.0)).exp());
            }
        }
        for (c, v) in cols.iter_mut().zip(row) {
            c.push(v);
        }
        arm.push(if active { "Active" } else { "Placebo" }.to_owned());
        sex.push(if rng.next_f64() < 0.5 { "F" } else { "M" }.to_owned());
        id.push((i + 1) as f64);
    }
    let mut patient = Column::numeric("Patient", id);
    patient.level = MeasurementLevel::Ordinal;
    let mut columns = vec![patient, Column::text("Arm", arm), Column::text("Sex", sex)];
    for (name, v) in ["Age", "Baseline", "Week 4", "Week 8", "Week 12"].into_iter().zip(cols) {
        columns.push(Column::numeric(name, v));
    }
    table("Clinical trial with dropout (simulated)", columns)
}

/// The trial project: the trial table with its multiple imputation report
/// (chained equations, 20 imputations, seed 1), and the stacked table of
/// those imputations (linked to the trial table) with two pooled analyses:
/// week-12 score on arm and baseline (analysis of covariance) and the
/// two-group comparison of week-12 scores by arm. Each pooled report is
/// followed by its complete-case analysis.
#[must_use]
pub fn clinical_dropout_project() -> ProjectFile {
    let trial = clinical_trial();
    let rows = RowStateManager::new(trial.n_rows());
    let columns: Vec<usize> = TRIAL_COLUMNS.iter().map(|c| col(&trial, c)).collect();
    let request = ImputationRequest {
        method: ImputationMethod::ChainedEquations,
        multiple: MiSettings::default(),
        ..ImputationRequest::new(columns)
    };
    let op = request.stacked_operation(&trial).expect("a multiple imputation");
    let derived = op.run(Source { table: &trial, rows: &rows }, &[], "20 imputations").expect("the trial imputes");
    let trial_id = TableId::from("table-1");
    let lineage =
        TableLineage::record(derived.origin.clone(), &[Some((&trial_id, Source { table: &trial, rows: &rows }))])
            .expect("one source");
    let stacked = derived.table;
    let pooled = ReportScope { pool: Some(IMPUTATION_COLUMN.to_owned()), ..ReportScope::default() };
    let ancova = SavedReport::Linear {
        spec: LinearModelSpec {
            response: col(&stacked, "Week 12"),
            predictors: vec![col(&stacked, "Arm"), col(&stacked, "Baseline")],
            effects: Vec::new(),
            splines: Vec::new(),
        },
        options: ReportOptions::default(),
    }
    .with_scope(pooled.clone());
    let by_arm = SavedReport::Groups {
        request: GroupComparisonRequest::new(GroupLayout::Stacked {
            response: col(&stacked, "Week 12"),
            group: col(&stacked, "Arm"),
        }),
    }
    .with_scope(pooled);
    ProjectFile {
        tables: vec![
            ProjectTable {
                id: trial_id,
                table: trial,
                rows,
                reports: vec![SavedReport::Imputation { request }],
                external: None,
                lineage: None,
            },
            ProjectTable {
                id: TableId::from("table-2"),
                table: stacked,
                rows: derived.rows,
                reports: vec![ancova, by_arm],
                external: None,
                lineage: Some(lineage),
            },
        ],
        scripts: Vec::new(),
        layout: ProjectLayout::default(),
        dashboards: Vec::new(),
        notebooks: Vec::new(),
        name: None,
        folders: Vec::new(),
        notes: "Simulated two-arm trial: symptom scores at baseline and weeks 4, 8 and 12, with patients dropping \
                out after a bad visit.\nThe second table stacks 20 imputations of the trial (Imputation 0 is the \
                original data); its reports pool their results across the imputations, then show the \
                complete-case analysis for comparison."
            .to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{load_project_file, save_project_file};

    #[test]
    fn dropout_is_monotone_and_more_common_on_placebo() {
        let t = clinical_trial();
        let arm = t.column(col(&t, "Arm")).unwrap();
        let weeks: Vec<&[f64]> = ["Week 4", "Week 8", "Week 12"]
            .iter()
            .map(|c| t.column(col(&t, c)).unwrap().as_numeric().unwrap())
            .collect();
        let mut missing = [0usize; 2];
        for r in 0..t.n_rows() {
            assert!(weeks[0][r].is_finite(), "everyone has week 4");
            assert!(!(weeks[1][r].is_nan() && weeks[2][r].is_finite()), "dropout is monotone");
            if weeks[2][r].is_nan() {
                missing[usize::from(arm.cell_text(r) == "Active")] += 1;
            }
        }
        assert!(missing[0] > missing[1], "{missing:?}");
        assert!((15..60).contains(&(missing[0] + missing[1])), "{missing:?}");
    }

    #[test]
    fn the_project_round_trips_and_its_reports_pool() {
        let file = clinical_dropout_project();
        let back = load_project_file(&save_project_file(&file)).expect("a valid project file");
        assert_eq!(back.tables.len(), 2);
        let stacked = &back.tables[1];
        assert_eq!(stacked.table.n_rows(), 21 * 120);
        assert!(stacked.lineage.is_some());
        for saved in &stacked.reports {
            let r = crate::run_report(saved, &stacked.table, &stacked.rows).unwrap();
            assert_eq!(r.sections[0].kind, crate::report::SectionKind::PooledNote, "{}", r.title);
            assert!(!matches!(&r.sections[0].content, crate::report::SectionContent::Note(t) if t.contains('⚠')));
        }
        let trial = &back.tables[0];
        let r = crate::run_report(&trial.reports[0], &trial.table, &trial.rows).unwrap();
        assert!(r.sections.iter().any(|s| s.kind == crate::report::SectionKind::ImputationTrace));
    }
}
