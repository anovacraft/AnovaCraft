//! Augmenting a design held in a data table: an [`AugmentRequest`] names the
//! factor columns (with the settings coded −1 and +1, as for a declared
//! design), the response columns and the method, and [`augment_table`]
//! returns a **new** table with every original row and column, followed by
//! the added runs (factor settings in actual units, every other column
//! empty), and a nominal block column (1 for the original runs, 2 for the
//! added ones). The methods are those of [`stat_doe::augment`]: fold-over,
//! center runs, axial runs, and D-optimal augmentation for a model over the
//! factor names, optionally within constraints ([`stat_doe::Region::parse`]).
//!
//! A numeric continuous factor is coded from its low and high settings
//! (by default the smallest and largest values among the rows). A
//! categorical factor (text, or numbers marked nominal or ordinal) must have
//! exactly two levels, coded −1 and +1 in sorted order; a fold-over swaps
//! them and the other methods keep it at one of them (center and axial runs
//! need continuous factors).

use crate::declared_design::DeclaredFactor;
use crate::project::invalid;
use stat_core::Result;
use stat_data::{CellValue, Column, DataTable, MeasurementLevel, table_ops::natural_order};
use stat_doe::{
    Augmented, ChangeLevel, Factor, ModelFormula, OptimalOptions, PointType, Region, Rng, SplitPlotAugmentOptions,
    add_axial_runs, add_center_runs, augment_optimal, augment_split_plot, fold_over,
};

/// How to add runs.
#[derive(Debug, Clone, PartialEq)]
pub enum AugmentMethod {
    /// Repeat the two-level runs with signs reversed: every factor
    /// (`factor: None`) or one factor (an index into
    /// [`AugmentRequest::factors`]).
    FoldOver {
        /// The factor to fold, or `None` for all.
        factor: Option<usize>,
    },
    /// Add center runs.
    CenterRuns {
        /// How many.
        count: usize,
    },
    /// Add axial runs at ±α on each factor axis, and center runs.
    Axial {
        /// Axial distance in coded units (1 = face-centerd).
        alpha: f64,
        /// Center runs added with them.
        centers: usize,
    },
    /// Add D-optimal runs for a model, keeping the existing runs.
    Optimal {
        /// Model over the factor column names (see [`ModelFormula::parse`]).
        formula: String,
        /// Number of runs to add.
        runs: usize,
        /// Random starts of the coordinate exchange.
        starts: usize,
        /// Seed of the starts.
        seed: u64,
        /// Constraints in actual units, one per line; empty for none.
        constraints: String,
    },
    /// Add whole plots to a split-plot design (see
    /// [`stat_doe::augment_split_plot`]): the factors that are constant
    /// within every existing whole plot are the hard-to-change ones and stay
    /// fixed within each new whole plot. The new whole plots are numbered
    /// after the existing ones and run in random order, with the runs in
    /// random order within each.
    WholePlots {
        /// The column naming each run's whole plot.
        whole_plot: usize,
        /// Model over the factor column names (see [`ModelFormula::parse`]).
        formula: String,
        /// Whole plots to add.
        count: usize,
        /// Runs in each new whole plot.
        runs: usize,
        /// Prior ratio η of the whole-plot to the run variance.
        ratio: f64,
        /// Random starts of the coordinate exchange.
        starts: usize,
        /// Seed of the starts.
        seed: u64,
    },
}

/// A request to augment the design in a table.
#[derive(Debug, Clone, PartialEq)]
pub struct AugmentRequest {
    /// Factor columns and their coding.
    pub factors: Vec<DeclaredFactor>,
    /// Response columns (left empty in the new rows).
    pub responses: Vec<usize>,
    /// The method.
    pub method: AugmentMethod,
    /// Randomize the order of the added runs with this seed.
    pub randomize: Option<u64>,
}

/// The augmented table and what was done.
#[derive(Debug, Clone)]
pub struct AugmentOutcome {
    /// The new table.
    pub table: DataTable,
    /// The augmented design in coded units.
    pub design: Augmented,
    /// Number of rows added.
    pub added: usize,
    /// Name of the block column.
    pub block_column: String,
    /// Notes for the preview and the user.
    pub notes: Vec<String>,
}

/// How one factor column is coded.
enum Coding {
    /// Numeric: actual = factor.actual(coded).
    Numeric(Factor),
    /// Two-level text column: the labels coded −1 and +1.
    Text(String, String),
    /// Two-level numeric categorical column: the values coded −1 and +1.
    Codes(f64, f64),
}

/// Augment the design held in `table` as `request` asks.
///
/// # Errors
/// No factors, a column used twice or missing, a categorical factor without
/// exactly two levels, a non-numeric response, a factor whose low and high
/// settings coincide, fewer than one complete run, center or axial runs with
/// a categorical factor, and any error of the method (a fold-over of a
/// design that is not two-level, an invalid model or constraint, too few
/// runs for the model…).
pub fn augment_table(table: &DataTable, request: &AugmentRequest) -> Result<AugmentOutcome> {
    if request.factors.is_empty() {
        return Err(invalid("choose at least one factor column"));
    }
    let plot_column = match &request.method {
        AugmentMethod::WholePlots { whole_plot, .. } => Some(*whole_plot),
        _ => None,
    };
    let all: Vec<usize> =
        request.factors.iter().map(|f| f.column).chain(request.responses.iter().copied()).chain(plot_column).collect();
    if all.iter().enumerate().any(|(i, c)| all[..i].contains(c)) {
        return Err(invalid("each column can have only one role"));
    }
    for &r in &request.responses {
        let c = table.column(r).ok_or_else(|| invalid(format!("no column {r}")))?;
        if c.as_numeric().is_none() {
            return Err(invalid(format!("the response {} must be numeric", c.name)));
        }
    }
    let n = table.n_rows();
    // Code every factor column.
    let mut codings = Vec::new();
    let mut coded_columns: Vec<Vec<f64>> = Vec::new();
    let mut names = Vec::new();
    for f in &request.factors {
        let c = table.column(f.column).ok_or_else(|| invalid(format!("no column {}", f.column)))?;
        names.push(c.name.clone());
        match c.as_numeric().filter(|_| c.level == MeasurementLevel::Continuous) {
            Some(v) => {
                let (lo, hi) = v
                    .iter()
                    .filter(|x| x.is_finite())
                    .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &x| (a.min(x), b.max(x)));
                let (low, high) = (f.low.unwrap_or(lo), f.high.unwrap_or(hi));
                if !(low.is_finite() && high.is_finite()) || low == high {
                    return Err(invalid(format!("{} needs different low and high settings (it may not vary)", c.name)));
                }
                coded_columns.push(v.iter().map(|x| (2.0 * x - (low + high)) / (high - low)).collect());
                codings.push(Coding::Numeric(Factor::new(c.name.clone(), low, high)));
            }
            None => {
                let text: Vec<String> = (0..n).map(|r| c.cell_text(r)).collect();
                let mut distinct: Vec<&String> = text.iter().filter(|t| !t.is_empty()).collect();
                distinct.sort_by(|a, b| natural_order(a, b));
                distinct.dedup();
                if distinct.len() != 2 {
                    return Err(invalid(format!(
                        "{} is categorical with {} levels: a categorical factor needs exactly two",
                        c.name,
                        distinct.len()
                    )));
                }
                let (lo, hi) = (distinct[0].clone(), distinct[1].clone());
                coded_columns.push(
                    text.iter()
                        .map(|t| {
                            if t.is_empty() {
                                f64::NAN
                            } else if *t == lo {
                                -1.0
                            } else {
                                1.0
                            }
                        })
                        .collect(),
                );
                codings.push(match c.as_numeric() {
                    Some(v) => {
                        let value = |label: &str| (0..n).find(|&r| text[r] == label).map_or(f64::NAN, |r| v[r]);
                        Coding::Codes(value(&lo), value(&hi))
                    }
                    None => Coding::Text(lo, hi),
                });
            }
        }
    }
    let plot_labels: Option<Vec<String>> = match plot_column {
        Some(c) => {
            let col = table.column(c).ok_or_else(|| invalid(format!("no column {c}")))?;
            Some((0..n).map(|r| col.cell_text(r)).collect())
        }
        None => None,
    };
    let rows: Vec<usize> = (0..n)
        .filter(|&r| coded_columns.iter().all(|v| v[r].is_finite()))
        .filter(|&r| plot_labels.as_ref().is_none_or(|l| !l[r].is_empty()))
        .collect();
    if rows.is_empty() {
        return Err(invalid("no row has a setting for every factor"));
    }
    let runs: Vec<Vec<f64>> = rows.iter().map(|&r| coded_columns.iter().map(|v| v[r]).collect()).collect();
    let categorical: Vec<bool> = codings.iter().map(|c| !matches!(c, Coding::Numeric(_))).collect();
    let need_continuous = |what: &str| -> Result<()> {
        match categorical.iter().position(|&c| c) {
            Some(j) => Err(invalid(format!("{what} need continuous factors ({} is categorical)", names[j]))),
            None => Ok(()),
        }
    };
    // New whole plots: (existing whole plots, new whole plot of each added run).
    let mut new_plots: Option<(usize, Vec<usize>)> = None;
    let design = match &request.method {
        AugmentMethod::FoldOver { factor } => {
            if factor.is_some_and(|j| j >= names.len()) {
                return Err(invalid("the factor to fold is not one of the chosen factors"));
            }
            fold_over(&runs, *factor, &names)?
        }
        AugmentMethod::CenterRuns { count } => {
            need_continuous("center runs")?;
            add_center_runs(&runs, *count)?
        }
        AugmentMethod::Axial { alpha, centers } => {
            need_continuous("axial runs")?;
            add_axial_runs(&runs, *alpha, *centers)?
        }
        AugmentMethod::Optimal { formula, runs: m, starts, seed, constraints } => {
            let model = ModelFormula::parse(formula, &names)?;
            let factors: Vec<Factor> = codings
                .iter()
                .zip(&names)
                .map(|(c, name)| match c {
                    Coding::Numeric(f) => f.clone(),
                    _ => Factor::coded(name.clone()),
                })
                .collect();
            let mut region = Region::parse(constraints, &factors)?;
            for (j, _) in categorical.iter().enumerate().filter(|(_, c)| **c) {
                region.restrict_to_two_levels(j);
            }
            let options = OptimalOptions { runs: *m, starts: *starts, seed: *seed, max_passes: 100 };
            augment_optimal(&runs, &model, &region, &options, &names)?
        }
        AugmentMethod::WholePlots { formula, count, runs: per_plot, ratio, starts, seed, .. } => {
            let labels = plot_labels.as_ref().map_or(&[][..], Vec::as_slice);
            let mut ids: Vec<&str> = Vec::new();
            let plots: Vec<usize> = rows
                .iter()
                .map(|&r| {
                    let l = labels[r].as_str();
                    ids.iter().position(|v| *v == l).unwrap_or_else(|| {
                        ids.push(l);
                        ids.len() - 1
                    })
                })
                .collect();
            // Hard to change: constant within every existing whole plot.
            let changes: Vec<ChangeLevel> = (0..names.len())
                .map(|j| {
                    let constant =
                        (0..runs.len()).all(|a| (0..a).all(|b| plots[a] != plots[b] || runs[a][j] == runs[b][j]));
                    if constant { ChangeLevel::Hard } else { ChangeLevel::Easy }
                })
                .collect();
            if changes.iter().all(|c| *c == ChangeLevel::Hard) {
                return Err(invalid(
                    "every factor is constant within the whole plots, so none is easy to change; check the whole \
                     plot column",
                ));
            }
            let model = ModelFormula::parse(formula, &names)?;
            let options = SplitPlotAugmentOptions {
                new_whole_plots: *count,
                runs_per_whole_plot: *per_plot,
                whole_plot_ratio: *ratio,
                starts: *starts,
                seed: *seed,
                max_passes: 100,
            };
            let a = augment_split_plot(&runs, &plots, &model, &changes, &options)?;
            let hard: Vec<&str> =
                names.iter().zip(&changes).filter(|(_, c)| **c == ChangeLevel::Hard).map(|(n, _)| n.as_str()).collect();
            let mut notes =
                vec![format!("Hard to change (constant within every existing whole plot): {}.", hard.join(", "))];
            notes.extend(a.notes.iter().map(|n| rename_factors(n, &names)));
            new_plots =
                Some((ids.len(), a.whole_plot[a.n_existing..].iter().map(|w| w - a.existing_whole_plots).collect()));
            let added = a.runs.len() - a.n_existing;
            Augmented {
                description: format!("{count} new whole plots ({added} runs)"),
                blocks: std::iter::repeat_n(1, a.n_existing).chain(std::iter::repeat_n(2, added)).collect(),
                added_types: vec![PointType::Other; added],
                runs: a.runs,
                notes,
            }
        }
    };
    // The added runs, in random order if asked.
    let existing = design.n_existing();
    let mut order: Vec<usize> = (existing..design.runs.len()).collect();
    if let Some(seed) = request.randomize {
        let mut rng = Rng::new(seed);
        match &new_plots {
            // Whole plots in random order, runs in random order within each.
            Some((_, plots)) => {
                let count = plots.iter().max().map_or(0, |m| m + 1);
                let mut sequence: Vec<usize> = (0..count).collect();
                rng.shuffle(&mut sequence);
                order.clear();
                for w in sequence {
                    let mut members: Vec<usize> =
                        (0..plots.len()).filter(|&i| plots[i] == w).map(|i| existing + i).collect();
                    rng.shuffle(&mut members);
                    order.extend(members);
                }
            }
            None => rng.shuffle(&mut order),
        }
    }
    // Labels of the new whole plots, after the existing ones, numbered in run order.
    let plot_label = |run: usize| -> Option<CellValue> {
        let (w0, plots) = new_plots.as_ref()?;
        let column = table.column(plot_column?)?;
        let mut seen: Vec<usize> = Vec::new();
        for &r in &order {
            let w = plots[r - existing];
            if !seen.contains(&w) {
                seen.push(w);
            }
            if r == run {
                break;
            }
        }
        let numbers: Vec<f64> =
            column.as_numeric().map(|v| v.iter().copied().filter(|x| x.is_finite()).collect()).unwrap_or_default();
        let top = numbers.iter().copied().fold(*w0 as f64, f64::max);
        let id = top + seen.len() as f64;
        Some(if column.as_numeric().is_some() { CellValue::Number(id) } else { CellValue::Text(format!("{id}")) })
    };
    let added = order.len();
    let mut out = table.clone();
    let point_type = table.find("Point type").filter(|&c| table.column(c).is_some_and(|c| c.as_numeric().is_none()));
    let new_rows: Vec<(usize, Vec<CellValue>)> = order
        .iter()
        .enumerate()
        .map(|(i, &run)| {
            let mut values = table.empty_row();
            for ((f, coding), &x) in request.factors.iter().zip(&codings).zip(&design.runs[run]) {
                values[f.column] = match coding {
                    Coding::Numeric(factor) => CellValue::Number(clean(factor.actual(x))),
                    Coding::Codes(lo, hi) => CellValue::Number(if x < 0.0 { *lo } else { *hi }),
                    Coding::Text(lo, hi) => CellValue::Text(if x < 0.0 { lo.clone() } else { hi.clone() }),
                };
            }
            if let Some(c) = point_type {
                values[c] = CellValue::Text(design.added_types[run - existing].label().to_owned());
            }
            if let (Some(c), Some(label)) = (plot_column, plot_label(run)) {
                values[c] = label;
            }
            (n + i, values)
        })
        .collect();
    out.insert_rows(new_rows)?;
    let block_column =
        if table.find("Block").is_none() { "Block".to_owned() } else { table.unused_column_name("Block") };
    // Rows with a missing factor setting were not part of the design; they
    // stay in block 1 with the rest of the original table.
    let mut block = Column::text(
        block_column.clone(),
        (0..n + added).map(|r| if r < n { "1".to_owned() } else { "2".to_owned() }).collect(),
    );
    block.level = MeasurementLevel::Nominal;
    let mut columns = out.columns().to_vec();
    columns.push(block);
    let table_out = DataTable::new(format!("{} (augmented)", table.name()), columns)?;
    let mut notes = vec![format!("{}: {} runs → {} runs.", design.description, rows.len(), rows.len() + added)];
    notes.extend(design.notes.iter().cloned());
    if rows.len() < n {
        notes.push(format!(
            "{} row(s) without every factor setting{} were not part of the design.",
            n - rows.len(),
            if plot_column.is_some() { " or a whole plot" } else { "" }
        ));
    }
    if !request.responses.is_empty() {
        let list: Vec<&str> =
            request.responses.iter().filter_map(|&r| table.column(r).map(|c| c.name.as_str())).collect();
        notes.push(format!("Responses to measure in the new runs: {}.", list.join(", ")));
    }
    notes.push(format!("{block_column} is 1 for the original runs and 2 for the added runs."));
    Ok(AugmentOutcome { table: table_out, design, added, block_column, notes })
}

/// A whole-plot augmentation from script settings (JSON): `factors` (column
/// names, or `#{ column, low, high }`), `whole_plot` (the column naming each
/// run's whole plot), `responses` (optional), `model` (formula over the
/// factor names, or `"main effects"`, `"interactions"` (default),
/// `"quadratic"`), `whole_plots` (how many to add), `runs` (per new whole
/// plot), `ratio` (η, 1), `starts` (20), `seed` (1), `randomize` (a seed,
/// default 1, or `false`).
///
/// # Errors
/// Missing columns or malformed settings.
pub fn whole_plots_request_from_json(v: &stat_data::JsonValue, table: &DataTable) -> Result<AugmentRequest> {
    use stat_data::JsonValue;
    let declared = crate::DesignDeclarationRequest::from_json(v, table)?;
    let whole_plot = declared.whole_plot.ok_or_else(|| invalid("give the whole_plot column"))?;
    let count = |key: &str, default: Option<usize>| -> Result<usize> {
        match v.get(key) {
            None | Some(JsonValue::Null) => default.ok_or_else(|| invalid(format!("give {key}"))),
            Some(j) => j.as_usize().ok_or_else(|| invalid(format!("{key}: expected a whole number"))),
        }
    };
    let seed = |key: &str, default: Option<u64>| -> Result<Option<u64>> {
        match v.get(key) {
            None | Some(JsonValue::Null) => Ok(default),
            Some(JsonValue::Bool(false)) => Ok(None),
            Some(JsonValue::Bool(true)) => Ok(Some(1)),
            Some(j) => crate::seed::seed_from_json(j).map(Some).ok_or_else(|| invalid(format!("{key}: a seed"))),
        }
    };
    Ok(AugmentRequest {
        factors: declared.factors,
        responses: declared.responses,
        method: AugmentMethod::WholePlots {
            whole_plot,
            formula: v.get("model").and_then(JsonValue::as_str).unwrap_or("interactions").to_owned(),
            count: count("whole_plots", None)?,
            runs: count("runs", None)?,
            ratio: v.get("ratio").and_then(JsonValue::as_f64).unwrap_or(1.0),
            starts: count("starts", Some(20))?,
            seed: seed("seed", Some(1))?.unwrap_or(1),
        },
        randomize: seed("randomize", Some(1))?,
    })
}

/// `note` with the default factor names `A, B, …` of a note's factor list
/// replaced by `names` (the list after "; " up to " fixed within").
fn rename_factors(note: &str, names: &[String]) -> String {
    let Some((head, rest)) = note.split_once("); ") else { return note.to_owned() };
    let Some((list, tail)) = rest.split_once(" fixed within") else { return note.to_owned() };
    let defaults = stat_doe::default_factor_names(names.len());
    let renamed: Vec<String> = list
        .split(", ")
        .map(|n| defaults.iter().position(|d| d == n).map_or_else(|| n.to_owned(), |j| names[j].clone()))
        .collect();
    format!("{head}); {} fixed within{tail}", renamed.join(", "))
}

/// Round to 12 significant digits, so coded steps such as 0.25 read cleanly
/// in actual units.
fn clean(x: f64) -> f64 {
    if x == 0.0 || !x.is_finite() {
        return x;
    }
    let digits = 12 - x.abs().log10().ceil() as i32;
    let f = 10f64.powi(digits.clamp(-300, 300));
    let r = (x * f).round() / f;
    if r.is_finite() { r } else { x }
}

/// The role of a new run, by its point type: for tests and callers.
#[must_use]
pub fn added_type_labels(design: &Augmented) -> Vec<&'static str> {
    design.added_types.iter().map(|t: &PointType| t.label()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The 2^(3−1) half fraction (I = ABC) in actual units with a text
    /// factor and a response.
    fn half_fraction() -> DataTable {
        DataTable::new(
            "Screening",
            vec![
                Column::numeric("Run", vec![1.0, 2.0, 3.0, 4.0]),
                Column::numeric("Temp", vec![150.0, 190.0, 150.0, 190.0]),
                Column::numeric("Time", vec![10.0, 10.0, 30.0, 30.0]),
                Column::text("Catalyst", vec!["Y".into(), "X".into(), "X".into(), "Y".into()]),
                Column::text("Point type", vec!["Factorial".into(); 4]),
                Column::numeric("Yield", vec![61.0, 74.0, 66.0, 80.0]),
            ],
        )
        .unwrap()
    }

    fn factors() -> Vec<DeclaredFactor> {
        (1..=3).map(|column| DeclaredFactor { column, low: None, high: None }).collect()
    }

    /// A split-plot design gets two new whole plots: the hard factor is
    /// found from the existing plots, the existing rows are kept, the new
    /// plots are numbered after the old ones, run consecutively, and hold the
    /// hard factor fixed.
    #[test]
    fn new_whole_plots_for_a_split_plot_design() {
        let json = stat_data::parse_json(
            r#"{"factors": [{"name": "Oven", "low": 150, "high": 190, "changes": "hard"},
                            {"name": "Time", "low": 10, "high": 30}, {"name": "Coat", "low": 1, "high": 3}],
                "runs": 12, "whole_plots": 4, "starts": 5}"#,
        )
        .unwrap();
        let (project, _) =
            crate::doe::design_project(&crate::doe::split_plot_request_from_json(&json).unwrap()).unwrap();
        let t = project.table;
        let wp = t.find("Whole Plot").unwrap();
        let factors: Vec<DeclaredFactor> = ["Oven", "Time", "Coat"]
            .iter()
            .map(|n| DeclaredFactor { column: t.find(n).unwrap(), low: None, high: None })
            .collect();
        let request = AugmentRequest {
            factors,
            responses: vec![t.find("Y").unwrap()],
            method: AugmentMethod::WholePlots {
                whole_plot: wp,
                formula: "interactions".into(),
                count: 2,
                runs: 3,
                ratio: 1.0,
                starts: 5,
                seed: 1,
            },
            randomize: Some(3),
        };
        let out = augment_table(&t, &request).unwrap();
        assert_eq!(out.added, 6);
        assert_eq!(out.table.n_rows(), 18);
        assert!(out.notes.iter().any(|n| n == "Hard to change (constant within every existing whole plot): Oven."));
        assert!(out.notes.iter().any(|n| n.contains("; Oven fixed within each new whole plot")), "{:?}", out.notes);
        let plots = out.table.column(wp).unwrap().as_numeric().unwrap();
        assert_eq!(&plots[12..].iter().map(|&v| v as usize).collect::<Vec<_>>()[..], &[5, 5, 5, 6, 6, 6]);
        let oven = out.table.column(t.find("Oven").unwrap()).unwrap().as_numeric().unwrap();
        assert!(oven[12..15].iter().all(|v| *v == oven[12]) && oven[15..].iter().all(|v| *v == oven[15]));
        assert!(oven[12..].iter().all(|v| *v == 150.0 || *v == 190.0));
        for c in 0..t.n_cols() {
            for r in 0..12 {
                assert_eq!(out.table.column(c).unwrap().cell_text(r), t.column(c).unwrap().cell_text(r));
            }
        }
        // One run per whole plot makes every factor look hard to change.
        let mut bad = request.clone();
        bad.method = AugmentMethod::WholePlots {
            whole_plot: t.find("Run").unwrap(),
            formula: "interactions".into(),
            count: 2,
            runs: 3,
            ratio: 1.0,
            starts: 5,
            seed: 1,
        };
        assert!(augment_table(&t, &bad).is_err(), "every factor is constant within one-run plots");
    }

    #[test]
    fn fold_over_appends_mirror_runs_and_a_block_column() {
        let t = half_fraction();
        let req = AugmentRequest {
            factors: factors(),
            responses: vec![5],
            method: AugmentMethod::FoldOver { factor: None },
            randomize: None,
        };
        let out = augment_table(&t, &req).unwrap();
        assert_eq!(out.added, 4);
        let a = &out.table;
        assert_eq!(a.name(), "Screening (augmented)");
        assert_eq!(a.n_rows(), 8);
        assert_eq!(a.n_cols(), 7);
        assert_eq!(a.column(1).unwrap().as_numeric().unwrap()[4..], [190.0, 150.0, 190.0, 150.0]);
        assert_eq!(a.column(2).unwrap().as_numeric().unwrap()[4..], [30.0, 30.0, 10.0, 10.0]);
        let cat: Vec<String> = (4..8).map(|r| a.column(3).unwrap().cell_text(r)).collect();
        assert_eq!(cat, ["X", "Y", "Y", "X"], "text levels swap");
        assert_eq!(a.column(4).unwrap().cell_text(5), "Fold-over");
        assert!(a.column(5).unwrap().as_numeric().unwrap()[4..].iter().all(|v| v.is_nan()), "responses empty");
        assert!(a.column(0).unwrap().as_numeric().unwrap()[4].is_nan(), "other columns empty");
        let block = a.column(6).unwrap();
        assert_eq!((block.name.as_str(), block.level), ("Block", MeasurementLevel::Nominal));
        assert_eq!((block.cell_text(3), block.cell_text(4)), ("1".to_owned(), "2".to_owned()));
        assert!(out.notes[0].starts_with("Full fold-over (4 runs, every sign reversed): 4 runs → 8 runs."));
        assert!(out.notes.iter().any(|n| n.contains("Temp with Time×Catalyst")), "{:?}", out.notes);
        assert!(out.notes.iter().any(|n| n == "Responses to measure in the new runs: Yield."));
        // The original table is untouched; a taken name gets a number.
        assert_eq!(t.n_rows(), 4);
        let again = augment_table(&out.table, &req).unwrap();
        assert_eq!(again.block_column, "Block 1");
    }

    #[test]
    fn optimal_augmentation_completes_the_factorial_in_actual_units() {
        let t = half_fraction();
        let req = AugmentRequest {
            factors: factors(),
            responses: vec![5],
            method: AugmentMethod::Optimal {
                formula: "Temp + Time + Catalyst + Temp*Time + Temp*Catalyst + Time*Catalyst".into(),
                runs: 4,
                starts: 10,
                seed: 1,
                constraints: String::new(),
            },
            randomize: Some(7),
        };
        let out = augment_table(&t, &req).unwrap();
        let a = &out.table;
        let mut combos: Vec<String> = (0..8)
            .map(|r| {
                format!(
                    "{}/{}/{}",
                    a.column(1).unwrap().cell_text(r),
                    a.column(2).unwrap().cell_text(r),
                    a.column(3).unwrap().cell_text(r)
                )
            })
            .collect();
        combos.sort();
        combos.dedup();
        assert_eq!(combos.len(), 8, "the full 2³ factorial: {combos:?}");
        assert!(out.notes.iter().any(|n| n.contains("all 8 runs can (D-efficiency 100.00%)")), "{:?}", out.notes);
        // Constraints in actual units bound the new runs.
        let req = AugmentRequest {
            method: AugmentMethod::Optimal {
                formula: "Temp + Time + Temp*Time".into(),
                runs: 3,
                starts: 5,
                seed: 1,
                constraints: "Temp + 2*Time <= 230".into(),
            },
            factors: factors()[..2].to_vec(),
            ..req
        };
        let out = augment_table(&t, &req).unwrap();
        for r in 4..7 {
            let (temp, time) = (
                out.table.column(1).unwrap().as_numeric().unwrap()[r],
                out.table.column(2).unwrap().as_numeric().unwrap()[r],
            );
            assert!(temp + 2.0 * time <= 230.0 + 1e-6, "({temp}, {time})");
        }
        assert!(out.notes.iter().any(|n| n.contains("existing run(s) lie outside")));
    }

    #[test]
    fn center_and_axial_runs_with_explicit_settings() {
        let t = half_fraction();
        let numeric = vec![
            DeclaredFactor { column: 1, low: Some(140.0), high: Some(200.0) },
            DeclaredFactor { column: 2, low: None, high: None },
        ];
        let req = AugmentRequest {
            factors: numeric.clone(),
            responses: vec![],
            method: AugmentMethod::Axial { alpha: 1.0, centers: 2 },
            randomize: None,
        };
        let out = augment_table(&t, &req).unwrap();
        assert_eq!(out.added, 6);
        let temp = out.table.column(1).unwrap().as_numeric().unwrap();
        assert_eq!(temp[4..], [140.0, 200.0, 170.0, 170.0, 170.0, 170.0]);
        assert_eq!(added_type_labels(&out.design), ["Axial", "Axial", "Axial", "Axial", "Center", "Center"]);
        let req = AugmentRequest { method: AugmentMethod::CenterRuns { count: 3 }, ..req };
        let out = augment_table(&t, &req).unwrap();
        assert_eq!(out.table.column(2).unwrap().as_numeric().unwrap()[4..], [20.0, 20.0, 20.0]);
        // Errors.
        let bad = |method: AugmentMethod, factors: Vec<DeclaredFactor>| {
            augment_table(&t, &AugmentRequest { factors, responses: vec![], method, randomize: None })
                .unwrap_err()
                .to_string()
        };
        assert!(bad(AugmentMethod::CenterRuns { count: 1 }, factors()).contains("Catalyst is categorical"));
        assert!(bad(AugmentMethod::FoldOver { factor: Some(5) }, factors()).contains("not one of"));
        assert!(bad(AugmentMethod::CenterRuns { count: 1 }, vec![]).contains("at least one factor"));
        let err = bad(
            AugmentMethod::FoldOver { factor: None },
            vec![DeclaredFactor { column: 1, low: Some(140.0), high: Some(200.0) }],
        );
        assert!(err.contains("two-level"), "{err}");
        let err =
            bad(AugmentMethod::FoldOver { factor: None }, vec![DeclaredFactor { column: 4, low: None, high: None }]);
        assert!(err.contains("1 levels"), "{err}");
        assert!(
            augment_table(
                &t,
                &AugmentRequest {
                    factors: numeric,
                    responses: vec![3],
                    method: AugmentMethod::CenterRuns { count: 1 },
                    randomize: None
                }
            )
            .is_err(),
            "a text response"
        );
        assert_eq!(clean(172.500_000_000_000_03), 172.5);
    }
}
