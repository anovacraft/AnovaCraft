//! The sample for Gaussian processes and sequential optimization.

use super::{Sample, col, round_to, table, text};
use crate::project::SavedReport;
use crate::sequential_optimization::ResponseGoal;
use crate::{GaussianProcessRequest, ProfilerOptions, SequentialAnalysis, SequentialRequest};
use stat_data::{Column, DataTable, RowStateManager};
use stat_doe::Goal;

/// The simulated reaction's yield (%), as a formula column: a smooth peak
/// near 98 °C and 55 min, saturating in catalyst, scaled by solvent, with a
/// small ripple standing in for run-to-run variation.
pub const YIELD_FORMULA: &str = "let s = switch text(Solvent) { \"Toluene\" => 1.0, \"Ethanol\" => 0.85, \
\"Acetonitrile\" => 0.93, _ => missing() };\n\
round(92.0 * exp(-(((Temperature - 98.0) / 22.0) ** 2) - (((Time - 55.0) / 35.0) ** 2)) \
* (1.0 - exp(-1.6 * Catalyst)) * s + 0.6 * sin(0.7 * Temperature + 0.3 * Time), 2)";

/// The simulated impurity (%), as a formula column: rising with
/// temperature, catalyst and time, higher in ethanol.
pub const IMPURITY_FORMULA: &str = "let e = if text(Solvent) == \"Ethanol\" { 0.5 } else { 0.0 };\n\
round(0.4 + 0.035 * (Temperature - 60.0) + 0.45 * Catalyst + 0.004 * Time + e, 3)";

/// The constraint of the sample: temperature and time together stay
/// within the reactor's heat budget.
pub const HEAT_BUDGET: &str = "Temperature + 0.5*Time <= 125";

/// **Simulated** reaction study: round 1 is a 12-run Latin hypercube over
/// temperature 60–120 °C, time 10–90 min and catalyst 0.5–3% (within the
/// heat budget), the three solvents in turn; Yield and Impurity are formula
/// columns ([`YIELD_FORMULA`], [`IMPURITY_FORMULA`]).
#[must_use]
pub fn reaction_round_one() -> DataTable {
    let mut rng = stat_core::rng::Rng::new(0x5E9_0A7);
    let solvents = ["Toluene", "Ethanol", "Acetonitrile"];
    let (mut temp, mut time, mut cat, mut solvent) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut k = 0;
    while temp.len() < 12 {
        let u = stat_core::optimize::latin_hypercube(12, 3, &mut rng);
        for p in u {
            let (t, m) = (60.0 + 60.0 * p[0], 10.0 + 80.0 * p[1]);
            if temp.len() < 12 && t + 0.5 * m <= 125.0 {
                temp.push(round_to(t, 0));
                time.push(round_to(m, 0));
                cat.push(round_to(0.5 + 2.5 * p[2], 2));
                solvent.push(solvents[k % 3].to_owned());
                k += 1;
            }
        }
    }
    let n = temp.len();
    let mut t = table(
        "Reaction yield (simulated)",
        vec![
            Column::numeric("Round", vec![1.0; n]),
            Column::numeric("Temperature", temp),
            Column::numeric("Time", time),
            Column::numeric("Catalyst", cat),
            text("Solvent", solvent),
        ],
    );
    for (name, f) in [("Yield", YIELD_FORMULA), ("Impurity", IMPURITY_FORMULA)] {
        let column = Column { formula: Some(f.to_owned()), ..Column::numeric(name, vec![f64::NAN; n]) };
        t.insert_column(t.n_cols(), column).expect("a new column name");
    }
    crate::formula::refresh_all(&mut t);
    t
}

/// Round 1, then round 2 as the four runs the yield report proposed after
/// it; the report then proposes round 3. Also the trade-off of yield
/// against impurity, and a Gaussian-process model of yield.
pub(super) fn reaction_yield_sample() -> Sample {
    let mut t = reaction_round_one();
    let c = |t: &DataTable, n: &str| col(t, n);
    let factors = vec![c(&t, "Temperature"), c(&t, "Time"), c(&t, "Catalyst"), c(&t, "Solvent")];
    let request = SequentialRequest {
        round: Some(c(&t, "Round")),
        constraints: HEAT_BUDGET.to_owned(),
        stop_value: Some(90.0),
        ..SequentialRequest::new(vec![c(&t, "Yield")], factors.clone())
    };
    let mut rows = RowStateManager::new(t.n_rows());
    if let Ok(a) = SequentialAnalysis::run(&t, &rows, &request)
        && let Ok(edit) = a.new_runs_edit(&t)
        && edit.apply(&mut t, &mut rows).is_ok()
    {
        crate::formula::refresh_all(&mut t);
    }
    let mut both = SequentialRequest {
        batch: 2,
        stop_value: None,
        ..SequentialRequest::new(vec![c(&t, "Yield"), c(&t, "Impurity")], factors.clone())
    };
    both.round = request.round;
    both.constraints = request.constraints.clone();
    both.goals[1] = ResponseGoal::new(Goal::Minimize);
    let model = GaussianProcessRequest::new(c(&t, "Yield"), factors);
    let reports = vec![
        SavedReport::SequentialOptimization { request: Box::new(request) },
        SavedReport::SequentialOptimization { request: Box::new(both) },
        SavedReport::GaussianProcess { request: model, profilers: ProfilerOptions::default() },
    ];
    Sample { reports, table: t }
}

#[cfg(test)]
mod tests {
    use crate::project::SavedReport;
    use crate::report::{SectionContent, SectionKind};
    use stat_data::RowStateManager;

    /// Round 2 followed the proposals; a few more rounds by the report's own
    /// proposals reach the heat-budget optimum.
    #[test]
    fn reaction_rounds_approach_the_optimum() {
        let s = super::reaction_yield_sample();
        assert_eq!(s.table.n_rows(), 16);
        // Proposed settings are written as short decimals.
        for name in ["Temperature", "Time", "Catalyst"] {
            let c = s.table.column(s.table.find(name).unwrap()).unwrap();
            for r in 12..16 {
                let text = c.cell_text(r);
                assert!(text.split('.').nth(1).is_none_or(|d| d.len() <= 4), "{name}: {text}");
            }
        }
        let rows = RowStateManager::new(s.table.n_rows());
        let SavedReport::SequentialOptimization { request } = &s.reports[0] else { panic!() };
        let mut t = s.table.clone();
        let mut r = rows.clone();
        let y = |t: &stat_data::DataTable| t.column(t.find("Yield").unwrap()).unwrap().as_numeric().unwrap().to_vec();
        let start = y(&t).iter().copied().fold(f64::NEG_INFINITY, f64::max);
        for _ in 0..4 {
            let a = crate::SequentialAnalysis::run(&t, &r, request).unwrap();
            a.new_runs_edit(&t).unwrap().apply(&mut t, &mut r).unwrap();
            crate::formula::refresh_all(&mut t);
        }
        let best = y(&t).iter().copied().fold(f64::NEG_INFINITY, f64::max);
        assert!(best > start && best > 85.0 && best < 92.0, "best yield {best} (start {start})");
        // Every run keeps to the heat budget.
        let col = |n: &str| t.column(t.find(n).unwrap()).unwrap().as_numeric().unwrap().to_vec();
        for (a, b) in col("Temperature").iter().zip(col("Time")) {
            assert!(a + 0.5 * b <= 125.0 + 0.05, "{a} + 0.5·{b}");
        }
        for report in &s.reports {
            let sections = match report {
                SavedReport::SequentialOptimization { request } => {
                    crate::SequentialAnalysis::run(&s.table, &rows, request).unwrap().sections()
                }
                SavedReport::GaussianProcess { request, .. } => {
                    crate::GaussianProcessAnalysis::run(&s.table, &rows, request).unwrap().sections()
                }
                _ => panic!(),
            };
            let progress = sections.iter().find(|x| x.kind == SectionKind::SequentialProgress);
            if let Some(p) = progress {
                let SectionContent::Plot(p) = &p.content else { panic!() };
                assert_eq!(p.points.len(), 16);
            }
        }
    }
}
