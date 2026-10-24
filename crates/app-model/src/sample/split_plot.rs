//! Sample for restricted randomization: a split-plot experiment with a
//! hard-to-change furnace temperature.

use super::Sample;
use crate::doe::{DesignKind, DesignRequest, design_project};
use stat_data::DataTable;
use stat_doe::{ChangeLevel, Factor, Rng, Units};

/// The design request of the furnace experiment: a D-optimal split-plot
/// design (seed 1, 20 starts) for main effects and two-factor interactions
/// of the furnace temperature (hard to change: one setting per heat), the
/// coating thickness and the rack position (both easy), 24 runs in 8 heats
/// of 3, whole-plot variance ratio 1, run order randomized within heats.
#[must_use]
pub fn furnace_request() -> DesignRequest {
    let factors = vec![
        Factor::new("Temperature (°C)", 360.0, 380.0),
        Factor::new("Coating (µm)", 2.0, 6.0),
        Factor::new("Position (cm)", 10.0, 50.0),
    ];
    let names: Vec<String> = factors.iter().map(|f| f.name.clone()).collect();
    DesignRequest {
        kind: DesignKind::SplitPlot {
            formula: stat_doe::ModelFormula::interactions(3).describe(&names),
            runs: 24,
            whole_plots: 8,
            subplots: 0,
            changes: vec![ChangeLevel::Hard, ChangeLevel::Easy, ChangeLevel::Easy],
            whole_plot_ratio: 1.0,
            subplot_ratio: 1.0,
            starts: 20,
            seed: 1,
        },
        factors,
        center_points: None,
        randomize: Some(1),
        units: Units::Actual,
        response: "Corrosion resistance".to_owned(),
    }
}

/// The furnace experiment with simulated results: corrosion resistance
/// `70 + 6t + 4c − 3p + 2.5tc + 1.5cp + γ + ε` in coded units (t
/// temperature, c coating, p position), with a heat-to-heat effect
/// `γ ~ N(0, 3²)` shared by the runs of a heat and run error `ε ~ N(0, 1.5²)`
/// (seed 7), rounded to 0.1. Opens with the design declaration (whole plots
/// and the model) and the split-plot mixed model fitted by REML.
pub(super) fn furnace_sample() -> Sample {
    let (mut project, _) = design_project(&furnace_request()).expect("a valid split-plot design");
    let t = &mut project.table;
    let values = |name: &str| -> Vec<f64> {
        t.column(t.find(name).expect("column")).and_then(|c| c.as_numeric()).expect("numeric").to_vec()
    };
    let (temp, coat, pos, heat) =
        (values("Temperature (°C)"), values("Coating (µm)"), values("Position (cm)"), values("Whole Plot"));
    let mut rng = Rng::new(7);
    let heat_effect: Vec<f64> = (0..8).map(|_| 3.0 * rng.normal()).collect();
    let y: Vec<f64> = (0..t.n_rows())
        .map(|i| {
            let (tc, cc, pc) = ((temp[i] - 370.0) / 10.0, (coat[i] - 4.0) / 2.0, (pos[i] - 30.0) / 20.0);
            let mean = 70.0 + 6.0 * tc + 4.0 * cc - 3.0 * pc + 2.5 * tc * cc + 1.5 * cc * pc;
            let v = mean + heat_effect[heat[i] as usize - 1] + 1.5 * rng.normal();
            (v * 10.0).round() / 10.0
        })
        .collect();
    let r = t.find("Corrosion resistance").expect("response column");
    let mut columns: Vec<stat_data::Column> = t.columns().to_vec();
    columns[r].data = stat_data::ColumnData::Numeric(y);
    *t = DataTable::new("Furnace coatings (split-plot)", columns).expect("valid table");
    let declared = match &project.reports[0] {
        crate::SavedReport::DeclaredDesign { request } => request.clone(),
        _ => unreachable!("a split-plot design opens declared"),
    };
    let analysis = crate::split_plot_analysis(&project.table, &declared).expect("a split-plot analysis");
    project.reports.push(analysis);
    Sample { table: project.table, reports: project.reports }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MixedAnalysis, SavedReport};
    use stat_data::RowStateManager;

    /// The split-plot fit against R (`lme4` 1.1 / `lmerTest` with REML and
    /// Satterthwaite degrees of freedom) on the same data
    /// (`tests/data/split_plot_furnace.csv`, script
    /// `tests/data/split_plot_reference.R`).
    #[test]
    fn furnace_split_plot_matches_lmer() {
        let s = furnace_sample();
        let csv = include_str!("../../tests/data/split_plot_furnace.csv");
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines.len(), s.table.n_rows() + 1, "the CSV holds the sample");
        let header: Vec<&str> = lines[0].split(',').collect();
        for (i, line) in lines[1..].iter().enumerate() {
            for (name, cell) in header.iter().zip(line.split(',')) {
                let c = s.table.column(s.table.find(name.trim_matches('"')).unwrap()).unwrap();
                let v: f64 = cell.parse().unwrap();
                assert!((c.as_numeric().unwrap()[i] - v).abs() < 1e-9, "{name} row {i}");
            }
        }
        let SavedReport::Mixed { request, .. } = &s.reports[1] else { panic!("{:?}", s.reports[1]) };
        let fit = MixedAnalysis::run(&s.table, &RowStateManager::new(s.table.n_rows()), request).unwrap();
        let f = fit.fit();
        // lmerTest: estimate, std error, Satterthwaite df.
        let fixed = [
            (-242.852_130_540_830, 45.173_161_878_130_49, 5.902_591),
            (0.829_166_666_673_416, 0.122_021_240_522_767, 5.898_006),
            (2.407_880_011_291_65, 0.178_489_670_715_717, 11.205_024),
            (-0.145_146_316_894_530, 0.017_848_967_071_571_7, 11.205_024),
            (0.122_035_707_733_154, 0.017_848_967_071_571_7, 11.205_024),
            (0.001_611_056_329_346_46, 0.001_784_896_707_157_17, 11.205_024),
            (0.031_858_430_435_183_4, 0.008_924_483_535_785_85, 11.205_024),
        ];
        assert_eq!(f.fixed.len(), fixed.len());
        let close = |a: f64, b: f64, rel: f64| (a - b).abs() <= rel * b.abs().max(1e-12);
        for (c, (est, se, df)) in f.fixed.iter().zip(fixed) {
            assert!(close(c.estimate, est, 1e-6), "{}: {} vs {est}", c.name, c.estimate);
            assert!(close(c.std_error, se, 1e-5), "{}: {} vs {se}", c.name, c.std_error);
            // lmerTest differentiates numerically; agreement to 10⁻³ df.
            assert!((c.df.unwrap() - df).abs() < 1e-3, "{}: {:?} vs {df}", c.name, c.df);
        }
        // Temperature, the whole-plot factor, is tested against whole-plot
        // error (8 whole plots − 2 whole-plot terms ≈ 6 df).
        assert!((f.fixed[1].df.unwrap() - 6.0).abs() < 0.2);
        assert!(close(f.variance_components[0].variance, 10.996_446_857_439_49, 1e-5));
        assert!(close(f.residual.as_ref().unwrap().variance, 2.744_698_960_303_09, 1e-5));
        assert!((f.minus_two_log_likelihood - 138.460_943_478_446).abs() < 1e-6);
        let tests = [
            (46.175_63, 0.000_534_62),
            (181.988_32, 2.8221e-8),
            (66.128_07, 4.9645e-6),
            (46.746_35, 2.5608e-5),
            (0.814_70, 0.385_728_04),
            (12.743_32, 0.004_274_35),
        ];
        assert_eq!(fit.effect_tests().len(), tests.len());
        for ((name, t), (stat, p)) in fit.effect_tests().iter().zip(tests) {
            assert!(close(t.statistic, stat, 1e-4), "{name}: F {} vs {stat}", t.statistic);
            assert!(close(t.p_value, p, 2e-3), "{name}: p {} vs {p}", t.p_value);
        }
    }
}
