//! Run sheets for gauge studies.
//!
//! A gauge study measures parts with one or more operators (appraisers), each
//! part one or more times. The run sheet lists every measurement to take, in the order
//! to take it:
//!
//! * **Crossed** — every operator measures every one of the `p` parts `r`
//!   times: `p · o · r` runs. This is the layout the ANOVA gauge R&R
//!   ([`crate::gauge`]) analyses.
//! * **Nested** — each operator measures their own `p` parts `r` times (the
//!   layout for destructive tests, where a part cannot be measured by more
//!   than one operator): `o · p · r` runs on `o · p` different parts.
//!
//! Randomizing the order keeps drift in the gauge, the environment or the
//! operators from masquerading as part or operator effects (Montgomery,
//! *Introduction to Statistical Quality Control*, §8.7; Burdick, Borror &
//! Montgomery, *Design and Analysis of Gauge R&R Studies*, 2005, ch. 1). The
//! order can be fully random, or random within each replicate ("trial"), so
//! that every part is measured once by every operator before any is measured
//! again — the usual practice, which also hides earlier readings from the
//! operator.

use stat_core::rng::Rng;
use stat_core::{Result, StatError};

/// Whether every operator measures the same parts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GaugeLayout {
    /// Every operator measures every part.
    Crossed,
    /// Each operator measures their own parts.
    Nested,
}

/// The order of the runs on the sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RunOrder {
    /// Replicate by replicate, operator by operator, parts in order.
    Standard,
    /// One random permutation of all runs.
    Random,
    /// Replicates in order; within each, the runs in random order.
    RandomWithinReplicate,
}

/// The size, layout and order of a gauge study.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorksheetPlan {
    /// Parts: in all (crossed) or per operator (nested).
    pub parts: usize,
    /// Operators.
    pub operators: usize,
    /// Measurements of each part by each of its operators.
    pub replicates: usize,
    /// Crossed or nested.
    pub layout: GaugeLayout,
    /// Run order.
    pub order: RunOrder,
    /// Seed of the random order (ignored for [`RunOrder::Standard`]).
    pub seed: u64,
}

/// One measurement to take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorksheetRun {
    /// Operator index, `0..operators`.
    pub operator: usize,
    /// Part index: `0..parts` when crossed, `0..operators·parts` when nested
    /// (operator `j` has parts `j·parts .. (j+1)·parts`).
    pub part: usize,
    /// Replicate (trial) index, `0..replicates`.
    pub replicate: usize,
}

/// Most runs on one sheet.
pub const MAX_WORKSHEET_RUNS: usize = 100_000;

/// The runs of a gauge study in the order to take them.
///
/// # Errors
/// [`StatError::InvalidArgument`] for fewer than two parts, no operators, no
/// replicates, a crossed layout with one operator measuring each part only
/// once (nothing to estimate repeatability from), or more than
/// [`MAX_WORKSHEET_RUNS`] runs.
pub fn gauge_worksheet(plan: &WorksheetPlan) -> Result<Vec<WorksheetRun>> {
    let WorksheetPlan { parts, operators, replicates, layout, order, seed } = *plan;
    if parts < 2 || operators < 1 || replicates < 1 {
        return Err(invalid("a gauge study needs at least two parts, one operator and one measurement of each"));
    }
    if layout == GaugeLayout::Crossed && operators < 2 && replicates < 2 {
        return Err(invalid("with one operator, measure each part at least twice"));
    }
    let n = parts.checked_mul(operators).and_then(|x| x.checked_mul(replicates)).filter(|&n| n <= MAX_WORKSHEET_RUNS);
    let Some(n) = n else {
        return Err(invalid(format!("the sheet would have more than {MAX_WORKSHEET_RUNS} runs")));
    };
    let mut runs = Vec::with_capacity(n);
    for replicate in 0..replicates {
        for operator in 0..operators {
            for p in 0..parts {
                let part = match layout {
                    GaugeLayout::Crossed => p,
                    GaugeLayout::Nested => operator * parts + p,
                };
                runs.push(WorksheetRun { operator, part, replicate });
            }
        }
    }
    let mut rng = Rng::new(seed);
    match order {
        RunOrder::Standard => {}
        RunOrder::Random => rng.shuffle(&mut runs),
        RunOrder::RandomWithinReplicate => {
            for block in runs.chunks_mut(parts * operators) {
                rng.shuffle(block);
            }
        }
    }
    Ok(runs)
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(layout: GaugeLayout, order: RunOrder) -> WorksheetPlan {
        WorksheetPlan { parts: 10, operators: 3, replicates: 2, layout, order, seed: 7 }
    }

    /// Count of each (operator, part, replicate) cell.
    fn cells(runs: &[WorksheetRun]) -> std::collections::HashMap<(usize, usize, usize), usize> {
        let mut m = std::collections::HashMap::new();
        for r in runs {
            *m.entry((r.operator, r.part, r.replicate)).or_insert(0) += 1;
        }
        m
    }

    #[test]
    fn crossed_sheet_has_every_cell_once() {
        for order in [RunOrder::Standard, RunOrder::Random, RunOrder::RandomWithinReplicate] {
            let runs = gauge_worksheet(&plan(GaugeLayout::Crossed, order)).unwrap();
            assert_eq!(runs.len(), 60);
            let c = cells(&runs);
            assert_eq!(c.len(), 60, "{order:?}: each operator × part × replicate exactly once");
            assert!(runs.iter().all(|r| r.part < 10 && r.operator < 3 && r.replicate < 2));
        }
        let std = gauge_worksheet(&plan(GaugeLayout::Crossed, RunOrder::Standard)).unwrap();
        assert_eq!(std[0], WorksheetRun { operator: 0, part: 0, replicate: 0 });
        assert_eq!(std[11], WorksheetRun { operator: 1, part: 1, replicate: 0 });
        assert_eq!(std[30], WorksheetRun { operator: 0, part: 0, replicate: 1 });
    }

    #[test]
    fn nested_sheet_gives_each_operator_their_own_parts() {
        let runs = gauge_worksheet(&plan(GaugeLayout::Nested, RunOrder::Random)).unwrap();
        assert_eq!(runs.len(), 60);
        assert_eq!(cells(&runs).len(), 60);
        assert!(runs.iter().all(|r| r.part / 10 == r.operator), "operator j measures parts 10j..10j+9");
        let parts: std::collections::HashSet<usize> = runs.iter().map(|r| r.part).collect();
        assert_eq!(parts.len(), 30);
    }

    #[test]
    fn randomization_is_seeded_and_respects_replicate_blocks() {
        let a = gauge_worksheet(&plan(GaugeLayout::Crossed, RunOrder::RandomWithinReplicate)).unwrap();
        let b = gauge_worksheet(&plan(GaugeLayout::Crossed, RunOrder::RandomWithinReplicate)).unwrap();
        assert_eq!(a, b, "same seed, same sheet");
        assert!(a[..30].iter().all(|r| r.replicate == 0) && a[30..].iter().all(|r| r.replicate == 1));
        let std = gauge_worksheet(&plan(GaugeLayout::Crossed, RunOrder::Standard)).unwrap();
        assert_ne!(a, std, "the order was shuffled");
        let full = gauge_worksheet(&plan(GaugeLayout::Crossed, RunOrder::Random)).unwrap();
        assert!(full[..30].iter().any(|r| r.replicate == 1), "a full shuffle mixes the replicates");
        let other =
            gauge_worksheet(&WorksheetPlan { seed: 8, ..plan(GaugeLayout::Crossed, RunOrder::Random) }).unwrap();
        assert_ne!(full, other);
    }

    #[test]
    fn rejects_degenerate_plans() {
        let ok = plan(GaugeLayout::Crossed, RunOrder::Standard);
        assert!(gauge_worksheet(&WorksheetPlan { parts: 1, ..ok }).is_err());
        assert!(gauge_worksheet(&WorksheetPlan { replicates: 0, ..ok }).is_err());
        assert!(gauge_worksheet(&WorksheetPlan { operators: 1, replicates: 1, ..ok }).is_err());
        assert!(gauge_worksheet(&WorksheetPlan { operators: 1, ..ok }).is_ok());
        assert!(gauge_worksheet(&WorksheetPlan { parts: 1000, operators: 100, replicates: 2, ..ok }).is_err());
    }
}
