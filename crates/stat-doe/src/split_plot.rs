//! D-optimal split-plot and split-split-plot designs by coordinate exchange.
//!
//! # Restricted randomization
//! When some factors are hard to change (a furnace temperature, a batch of raw
//! material), an experiment groups the runs into **whole plots** within
//! which those factors stay fixed: the hard-to-change factors are reset once
//! per whole plot, the easy-to-change factors on every run. Very hard-to-change
//! factors add a third stratum: they stay fixed within whole plots, hard ones
//! within **subplots** nested in the whole plots, and easy ones vary from run
//! to run (a split-split-plot design).
//!
//! Runs in the same whole plot share a random whole-plot effect, so the
//! responses follow the mixed model `y = Xβ + Z_w γ + Z_s δ + ε` with
//! `γ ~ N(0, σ²_w I)`, `δ ~ N(0, σ²_s I)` and `ε ~ N(0, σ²_ε I)`, and
//! `Var(y) = σ²_ε V`, `V = I + η_w Z_w Z_wᵀ + η_s Z_s Z_sᵀ` with the variance
//! ratios `η_w = σ²_w / σ²_ε` and `η_s = σ²_s / σ²_ε` (`Z_w`, `Z_s` the
//! whole-plot and subplot indicator matrices). The generalized least-squares
//! estimator has covariance `σ²_ε (XᵀV⁻¹X)⁻¹`, so a **D-optimal** split-plot
//! design maximizes `det(XᵀV⁻¹X)` (Goos & Vandebroek 2003, "D-optimal
//! split-plot designs with given numbers and sizes of whole plots",
//! *Technometrics* 45(3); Jones & Goos 2009, "D-optimal design of
//! split-split-plot experiments", *Biometrika* 96(1)). `V` is block-diagonal
//! with one block per whole plot, so `XᵀV⁻¹X = Σᵢ XᵢᵀVᵢ⁻¹Xᵢ`.
//!
//! # Coordinate exchange without a candidate set
//! Jones & Goos (2007), "A candidate-set-free algorithm for generating
//! D-optimal split-plot designs", *Applied Statistics* 56(3): starting from a
//! random design that respects the restrictions, each coordinate is replaced
//! by the candidate level that most increases `det(XᵀV⁻¹X)`. The coordinate
//! of a whole-plot factor is its setting in one whole plot (all its runs move
//! together), of a subplot factor its setting in one subplot, and of an easy
//! factor its setting in one run. A change touches one whole plot `i` only, so
//! the information is updated by replacing that plot's contribution
//! `XᵢᵀVᵢ⁻¹Xᵢ`. Passes repeat until no coordinate improves; the best of
//! several random starts is kept. Candidate levels are those of
//! [`crate::optimal`] (−1, +1 for linear terms; −1, 0, +1 for a squared
//! factor). A singular start is improved for `det(XᵀV⁻¹X + δI)` with a small
//! `δ` until it becomes non-singular.
//!
//! # Plot sizes
//! The `n` runs are divided as evenly as possible among the whole plots (for
//! a split-split-plot design: among the subplots, which are themselves
//! divided as evenly as possible among the whole plots), the larger plots
//! first.
//!
//! # Efficiency and prediction variance
//! With `σ²_ε = 1` the reported **D-criterion** is `ln det(XᵀV⁻¹X)`. The
//! **D-efficiency** `100 · det(XᵀV*⁻¹X)^{1/p} / n` uses `V* = V / (1 + η_w + η_s)`,
//! the covariance in units of the total variance of a run; it is 100% for an
//! orthogonal completely randomized design with that variance and can exceed
//! 100% because comparisons within whole plots are free of the whole-plot
//! variance. Prediction variances `f(x)ᵀ(XᵀV*⁻¹X)⁻¹f(x)` are in the same
//! units (the variance of the estimated mean response relative to a run's
//! total variance).

use crate::design::{Design, Factor, PointType, Rng, TableOptions, Units, default_factor_names};
use crate::optimal::{
    ModelFormula, candidate_levels, cholesky, invert, log_det_from_cholesky, region_moments, trace_product,
};
use stat_core::{Result, StatError};
use stat_data::{Column, DataTable, MeasurementLevel};

/// How hard a factor is to change, which decides the stratum where it varies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum ChangeLevel {
    /// Reset on every run.
    #[default]
    Easy,
    /// Held fixed within a whole plot (within a subplot when some factor is
    /// very hard to change).
    Hard,
    /// Held fixed within a whole plot of a split-split-plot design.
    VeryHard,
}

impl ChangeLevel {
    /// Every level, easiest first.
    pub const ALL: [Self; 3] = [Self::Easy, Self::Hard, Self::VeryHard];

    /// Name for menus: "Easy", "Hard", "Very hard".
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Easy => "Easy",
            Self::Hard => "Hard",
            Self::VeryHard => "Very hard",
        }
    }

    /// Key in project and script settings: `easy`, `hard`, `very-hard`.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Easy => "easy",
            Self::Hard => "hard",
            Self::VeryHard => "very-hard",
        }
    }

    /// The level named by a key or label (case-insensitive; `_` or a space
    /// may replace the hyphen).
    #[must_use]
    pub fn from_key(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().replace(['_', ' '], "-").as_str() {
            "easy" => Some(Self::Easy),
            "hard" => Some(Self::Hard),
            "very-hard" | "veryhard" => Some(Self::VeryHard),
            _ => None,
        }
    }
}

/// Options for [`split_plot_design`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SplitPlotOptions {
    /// Number of runs.
    pub runs: usize,
    /// Number of whole plots (1 to `runs`).
    pub whole_plots: usize,
    /// Number of subplots of a split-split-plot design (from `whole_plots` to
    /// `runs`); ignored unless some factors are very hard and others hard to
    /// change.
    pub subplots: usize,
    /// `η_w = σ²_w / σ²_ε`, the whole-plot variance relative to the run
    /// variance (≥ 0; 1 is the usual prior guess).
    pub whole_plot_ratio: f64,
    /// `η_s = σ²_s / σ²_ε`, the subplot variance relative to the run
    /// variance (split-split-plot designs).
    pub subplot_ratio: f64,
    /// Random starts; the best design is kept.
    pub starts: usize,
    /// Seed of the starts.
    pub seed: u64,
    /// Most passes over all coordinates per start.
    pub max_passes: usize,
}

impl SplitPlotOptions {
    /// `runs` runs in `whole_plots` whole plots, no subplots, variance
    /// ratios 1, 20 starts, seed 1, at most 100 passes.
    #[must_use]
    pub fn new(runs: usize, whole_plots: usize) -> Self {
        Self {
            runs,
            whole_plots,
            subplots: 0,
            whole_plot_ratio: 1.0,
            subplot_ratio: 1.0,
            starts: 20,
            seed: 1,
            max_passes: 100,
        }
    }
}

/// Which plot each run belongs to: runs are numbered in standard order,
/// whole plot by whole plot (and subplot by subplot within them).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlotStructure {
    /// Whole plot of each run (0-based).
    pub whole_plot: Vec<usize>,
    /// Subplot of each run (0-based, numbered across the design), for a
    /// split-split-plot design.
    pub subplot: Option<Vec<usize>>,
}

impl PlotStructure {
    /// `runs` runs in `whole_plots` whole plots of sizes as equal as
    /// possible (the larger first).
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] unless `1 ≤ whole_plots ≤ runs`.
    pub fn split_plot(runs: usize, whole_plots: usize) -> Result<Self> {
        if whole_plots == 0 || whole_plots > runs {
            return Err(invalid(format!("use 1 to {runs} whole plots for {runs} runs (got {whole_plots})")));
        }
        Ok(Self { whole_plot: expand(&even_sizes(runs, whole_plots)), subplot: None })
    }

    /// `runs` runs in `subplots` subplots of sizes as equal as possible,
    /// and the subplots in `whole_plots` whole plots likewise.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] unless `1 ≤ whole_plots ≤ subplots ≤ runs`.
    pub fn split_split_plot(runs: usize, whole_plots: usize, subplots: usize) -> Result<Self> {
        if whole_plots == 0 || whole_plots > subplots || subplots > runs {
            return Err(invalid(format!(
                "a split-split-plot design needs 1 ≤ whole plots ≤ subplots ≤ runs (got {whole_plots} whole plots, \
                 {subplots} subplots, {runs} runs)"
            )));
        }
        let subplot = expand(&even_sizes(runs, subplots));
        let wp_of_sp = expand(&even_sizes(subplots, whole_plots));
        Ok(Self { whole_plot: subplot.iter().map(|&s| wp_of_sp[s]).collect(), subplot: Some(subplot) })
    }

    /// Number of runs.
    #[must_use]
    pub fn n_runs(&self) -> usize {
        self.whole_plot.len()
    }

    /// Number of whole plots.
    #[must_use]
    pub fn n_whole_plots(&self) -> usize {
        self.whole_plot.iter().max().map_or(0, |m| m + 1)
    }

    /// Number of subplots (0 without a subplot stratum).
    #[must_use]
    pub fn n_subplots(&self) -> usize {
        self.subplot.as_ref().and_then(|s| s.iter().max()).map_or(0, |m| m + 1)
    }

    /// Runs of each whole plot.
    fn whole_plot_runs(&self) -> Vec<Vec<usize>> {
        groups(&self.whole_plot, self.n_whole_plots())
    }

    /// The block `Vᵢ = I + η_w J + η_s Σ J_s` of whole plot `runs`.
    fn block(&self, runs: &[usize], eta_w: f64, eta_s: f64) -> Vec<Vec<f64>> {
        runs.iter()
            .map(|&a| {
                runs.iter()
                    .map(|&b| {
                        let same_sp = self.subplot.as_ref().is_some_and(|s| s[a] == s[b]);
                        f64::from(u8::from(a == b)) + eta_w + if same_sp { eta_s } else { 0.0 }
                    })
                    .collect()
            })
            .collect()
    }
}

/// Sizes of `parts` groups sharing `n` items as evenly as possible.
fn even_sizes(n: usize, parts: usize) -> Vec<usize> {
    (0..parts).map(|i| n / parts + usize::from(i < n % parts)).collect()
}

/// Group label of each item for consecutive groups of the given sizes.
fn expand(sizes: &[usize]) -> Vec<usize> {
    sizes.iter().enumerate().flat_map(|(g, &m)| std::iter::repeat_n(g, m)).collect()
}

fn groups(labels: &[usize], count: usize) -> Vec<Vec<usize>> {
    let mut out = vec![Vec::new(); count];
    for (i, &g) in labels.iter().enumerate() {
        out[g].push(i);
    }
    out
}

/// A D-optimal split-plot or split-split-plot design and its quality.
#[derive(Debug, Clone, PartialEq)]
pub struct SplitPlotDesign {
    /// The design in standard order, whole plot by whole plot (point type
    /// [`PointType::Other`]).
    pub design: Design,
    /// The model it was built for.
    pub formula: ModelFormula,
    /// How hard each factor is to change.
    pub changes: Vec<ChangeLevel>,
    /// The whole plots (and subplots) of the runs.
    pub structure: PlotStructure,
    /// `η_w` used.
    pub whole_plot_ratio: f64,
    /// `η_s` used (0 without a subplot stratum).
    pub subplot_ratio: f64,
    /// `ln det(XᵀV⁻¹X)` with `σ²_ε = 1`.
    pub log_det: f64,
    /// `100 · det(XᵀV*⁻¹X)^{1/p} / n` (see the module notes).
    pub d_efficiency: f64,
    /// Average prediction variance over `[−1, 1]^k`, `tr(M (XᵀV*⁻¹X)⁻¹)`.
    pub average_variance: f64,
    /// Largest prediction variance over the grid of candidate levels (NaN
    /// for more than 12 factors).
    pub max_variance: f64,
    /// Degrees of freedom for whole-plot error: whole plots minus the terms
    /// in whole-plot factors only (with the intercept); may be 0 or negative.
    pub whole_plot_error_df: i64,
}

impl SplitPlotDesign {
    /// Export as a data table: `Run`, `Std order` (when randomized),
    /// `Whole Plot` and (split-split-plot) `Subplot` as nominal numbered
    /// columns, one numeric column per factor and empty response columns.
    /// Randomization respects the restrictions: the whole plots are run in
    /// random order, the subplots in random order within each whole plot and
    /// the runs in random order within each subplot. Whole plots and subplots
    /// are numbered in run order.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] if a response clashes with another
    /// column name.
    pub fn to_table(&self, name: &str, options: &TableOptions) -> Result<DataTable> {
        let order = self.run_order(options.randomize);
        let n = order.len();
        let mut columns = vec![Column::numeric("Run", (1..=n).map(|i| i as f64).collect())];
        if options.randomize.is_some() {
            let mut c = Column::numeric("Std order", order.iter().map(|&i| (i + 1) as f64).collect());
            c.level = MeasurementLevel::Ordinal;
            columns.push(c);
        }
        let renumber = |labels: &[usize]| -> Vec<f64> {
            let mut seen: Vec<usize> = Vec::new();
            order
                .iter()
                .map(|&i| {
                    let g = labels[i];
                    let id = seen.iter().position(|&s| s == g).unwrap_or_else(|| {
                        seen.push(g);
                        seen.len() - 1
                    });
                    (id + 1) as f64
                })
                .collect()
        };
        let mut wp = Column::numeric(WHOLE_PLOT_COLUMN, renumber(&self.structure.whole_plot));
        wp.level = MeasurementLevel::Nominal;
        columns.push(wp);
        if let Some(sp) = &self.structure.subplot {
            let mut c = Column::numeric(SUBPLOT_COLUMN, renumber(sp));
            c.level = MeasurementLevel::Nominal;
            columns.push(c);
        }
        for (j, f) in self.design.factors.iter().enumerate() {
            let values = order
                .iter()
                .map(|&i| {
                    let c = self.design.runs[i][j];
                    match options.units {
                        Units::Coded => c,
                        Units::Actual => f.actual(c),
                    }
                })
                .collect();
            columns.push(Column::numeric(f.name.clone(), values));
        }
        for r in &options.responses {
            columns.push(Column::numeric(r.clone(), vec![f64::NAN; n]));
        }
        DataTable::new(name, columns)
    }

    /// Standard-order run indices in the order to run them.
    #[must_use]
    pub fn run_order(&self, seed: Option<u64>) -> Vec<usize> {
        let s = &self.structure;
        let n = s.n_runs();
        // Units: subplots when there are any, otherwise single runs.
        let sp: Vec<usize> = s.subplot.clone().unwrap_or_else(|| (0..n).collect());
        let n_sp = sp.iter().max().map_or(0, |m| m + 1);
        let sp_runs = groups(&sp, n_sp);
        let mut wp_sps: Vec<Vec<usize>> = vec![Vec::new(); s.n_whole_plots()];
        for (u, runs) in sp_runs.iter().enumerate() {
            if let Some(&r) = runs.first() {
                wp_sps[s.whole_plot[r]].push(u);
            }
        }
        let mut wps: Vec<usize> = (0..wp_sps.len()).collect();
        let mut rng = seed.map(Rng::new);
        if let Some(rng) = &mut rng {
            rng.shuffle(&mut wps);
        }
        let mut order = Vec::with_capacity(n);
        for w in wps {
            let mut units = wp_sps[w].clone();
            if let Some(rng) = &mut rng {
                rng.shuffle(&mut units);
            }
            for u in units {
                let mut runs = sp_runs[u].clone();
                if let Some(rng) = &mut rng {
                    rng.shuffle(&mut runs);
                }
                order.extend(runs);
            }
        }
        order
    }
}

/// Name of the whole-plot column of an exported design.
pub const WHOLE_PLOT_COLUMN: &str = "Whole Plot";
/// Name of the subplot column of an exported split-split-plot design.
pub const SUBPLOT_COLUMN: &str = "Subplot";

/// `ln det(XᵀV⁻¹X)` (with `σ²_ε = 1`) of coded `runs` for `formula` with the
/// given plots and variance ratios; −∞ if singular.
#[must_use]
pub fn split_plot_log_det(
    formula: &ModelFormula,
    runs: &[Vec<f64>],
    structure: &PlotStructure,
    whole_plot_ratio: f64,
    subplot_ratio: f64,
) -> f64 {
    let rows: Vec<Vec<f64>> = runs.iter().map(|r| formula.expand(r)).collect();
    let info = gls_information(&rows, structure, whole_plot_ratio, subplot_ratio);
    info.and_then(|m| cholesky(&m)).map_or(f64::NEG_INFINITY, |l| log_det_from_cholesky(&l))
}

/// `XᵀV⁻¹X` for model rows `rows`.
fn gls_information(rows: &[Vec<f64>], s: &PlotStructure, eta_w: f64, eta_s: f64) -> Option<Vec<Vec<f64>>> {
    let p = rows.first().map_or(0, Vec::len);
    let mut m = vec![vec![0.0; p]; p];
    for runs in s.whole_plot_runs() {
        let v_inv = invert(&s.block(&runs, eta_w, eta_s))?;
        add_contribution(&mut m, &v_inv, runs.iter().map(|&r| &rows[r]).collect::<Vec<_>>().as_slice(), 1.0);
    }
    Some(m)
}

/// `m += sign · XᵢᵀVᵢ⁻¹Xᵢ` for the rows `x` of one whole plot.
fn add_contribution(m: &mut [Vec<f64>], v_inv: &[Vec<f64>], x: &[&Vec<f64>], sign: f64) {
    let p = m.len();
    // W = Vᵢ⁻¹ Xᵢ.
    let w: Vec<Vec<f64>> =
        v_inv.iter().map(|vrow| (0..p).map(|c| vrow.iter().zip(x).map(|(v, r)| v * r[c]).sum()).collect()).collect();
    for a in 0..p {
        for b in 0..=a {
            let s: f64 = x.iter().zip(&w).map(|(r, wr)| r[a] * wr[b]).sum();
            m[a][b] += sign * s;
            if a != b {
                m[b][a] += sign * s;
            }
        }
    }
}

/// The stratum where each factor varies: 0 whole plot, 1 subplot, 2 run.
fn strata(changes: &[ChangeLevel]) -> (Vec<u8>, bool) {
    let has_very_hard = changes.contains(&ChangeLevel::VeryHard);
    let has_hard = changes.contains(&ChangeLevel::Hard);
    let split_split = has_very_hard && has_hard;
    let s = changes
        .iter()
        .map(|c| match c {
            ChangeLevel::Easy => 2,
            ChangeLevel::Hard if split_split => 1,
            ChangeLevel::Hard | ChangeLevel::VeryHard => 0,
        })
        .collect();
    (s, split_split)
}

/// Build a D-optimal split-plot design for `formula` (factors `A, B, …` in
/// coded units) by coordinate exchange. A factor marked
/// [`ChangeLevel::Hard`] is held fixed within each whole plot; when some
/// factors are [`ChangeLevel::VeryHard`] as well, those are held fixed within
/// whole plots and the hard ones within subplots (a split-split-plot design
/// with `options.subplots` subplots). If only very-hard factors are given,
/// they are the whole-plot factors of a split-plot design.
///
/// # Errors
/// [`StatError::InvalidArgument`] if `changes` does not give one level per
/// factor, no factor is hard or very hard to change, the plot counts are out
/// of range (see [`PlotStructure`]), a variance ratio is negative or not
/// finite, there are fewer runs than model terms, no starts, or more than
/// 10 000 runs; [`StatError::RankDeficient`] if no start reached a
/// non-singular design (for example too few whole plots for the whole-plot
/// terms with `η_w` = 0).
pub fn split_plot_design(
    formula: &ModelFormula,
    changes: &[ChangeLevel],
    options: &SplitPlotOptions,
) -> Result<SplitPlotDesign> {
    let (k, p, n) = (formula.n_factors(), formula.n_terms(), options.runs);
    if k == 0 {
        return Err(invalid("a design needs at least one factor"));
    }
    if changes.len() != k {
        return Err(invalid(format!("give a change level for each of the {k} factors")));
    }
    if changes.iter().all(|c| *c == ChangeLevel::Easy) {
        return Err(invalid("mark at least one factor as hard or very hard to change"));
    }
    if n < p {
        return Err(invalid(format!("the model has {p} terms, so at least {p} runs are needed (got {n})")));
    }
    if options.starts == 0 || n > 10_000 {
        return Err(invalid("use at least one start and at most 10 000 runs"));
    }
    let ratio_ok = |r: f64| r.is_finite() && r >= 0.0;
    if !ratio_ok(options.whole_plot_ratio) || !ratio_ok(options.subplot_ratio) {
        return Err(invalid("variance ratios must be finite and not negative"));
    }
    let (stratum, split_split) = strata(changes);
    let structure = if split_split {
        PlotStructure::split_split_plot(n, options.whole_plots, options.subplots)?
    } else {
        PlotStructure::split_plot(n, options.whole_plots)?
    };
    let eta_w = options.whole_plot_ratio;
    let eta_s = if split_split { options.subplot_ratio } else { 0.0 };
    let search = Search::new(formula, &structure, stratum.clone(), eta_w, eta_s)?;
    let mut rng = Rng::new(options.seed);
    let mut best: Option<(f64, Vec<Vec<f64>>)> = None;
    for _ in 0..options.starts {
        let mut runs = search.random_runs(&mut rng);
        search.exchange(&mut runs, options.max_passes);
        let ld = split_plot_log_det(formula, &runs, &structure, eta_w, eta_s);
        if ld.is_finite() && best.as_ref().is_none_or(|(b, _)| ld > *b + 1e-12 * b.abs().max(1.0)) {
            best = Some((ld, runs));
        }
    }
    let (log_det, runs) = best.ok_or(StatError::RankDeficient { rank: 0, columns: p })?;
    let total = 1.0 + eta_w + eta_s;
    let d_efficiency = 100.0 * total * (log_det / p as f64).exp() / n as f64;
    let rows: Vec<Vec<f64>> = runs.iter().map(|r| formula.expand(r)).collect();
    let info =
        gls_information(&rows, &structure, eta_w, eta_s).ok_or(StatError::RankDeficient { rank: 0, columns: p })?;
    let inverse = invert(&info).ok_or(StatError::RankDeficient { rank: 0, columns: p })?;
    let average_variance = trace_product(&region_moments(formula), &inverse) / total;
    let max_variance = max_grid_variance(formula, &inverse) / total;
    let whole_plot_terms =
        formula.terms().iter().filter(|t| t.powers.iter().zip(&stratum).all(|(&pw, &s)| pw == 0 || s == 0)).count();
    let whole_plot_error_df = structure.n_whole_plots() as i64 - whole_plot_terms as i64;

    let names = default_factor_names(k);
    let factors: Vec<Factor> = names.iter().cloned().map(Factor::coded).collect();
    let (w, s) = (structure.n_whole_plots(), structure.n_subplots());
    let what = if split_split { "Split-split-plot" } else { "Split-plot" };
    let plots = if split_split {
        format!("{n} runs in {w} whole plots and {s} subplots")
    } else {
        format!("{n} runs in {w} whole plots")
    };
    let mut design = Design::new(
        format!("{what} D-optimal, {k} factors, {p}-term model ({plots})"),
        factors,
        runs,
        vec![PointType::Other; n],
    )?;
    design.notes.push(format!("Model: {}", formula.describe(&names)));
    design.notes.extend(change_notes(changes, &names, split_split));
    let ratios = if split_split {
        format!("η_w = σ²_w/σ²_ε = {}, η_s = σ²_s/σ²_ε = {}", fmt(eta_w), fmt(eta_s))
    } else {
        format!("η = σ²_w/σ²_ε = {}", fmt(eta_w))
    };
    design.notes.push(format!(
        "D-efficiency {d_efficiency:.2}%; ln det(XᵀV⁻¹X) = {log_det:.6} with variance ratio {ratios}; best of {} \
         coordinate-exchange starts (seed {}).",
        options.starts, options.seed
    ));
    let max = if max_variance.is_finite() {
        format!("; largest on the grid of levels {max_variance:.6}")
    } else {
        String::new()
    };
    design.notes.push(format!(
        "Prediction variance (relative to a run's total variance): average {average_variance:.6} over the cube{max}."
    ));
    let error = format!(
        "{w} whole plots and {whole_plot_terms} whole-plot terms (intercept and terms in whole-plot factors only): \
         {whole_plot_error_df} degrees of freedom for whole-plot error."
    );
    design.notes.push(if whole_plot_error_df > 0 {
        error
    } else {
        format!("{error} Add whole plots to test whole-plot effects.")
    });
    Ok(SplitPlotDesign {
        design,
        formula: formula.clone(),
        changes: changes.to_vec(),
        structure,
        whole_plot_ratio: eta_w,
        subplot_ratio: eta_s,
        log_det,
        d_efficiency,
        average_variance,
        max_variance,
        whole_plot_error_df,
    })
}

fn fmt(v: f64) -> String {
    let s = format!("{v:.4}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// "Hard to change (… ): A, B." lines for the design notes.
fn change_notes(changes: &[ChangeLevel], names: &[String], split_split: bool) -> Vec<String> {
    let list = |level: ChangeLevel| -> String {
        names.iter().zip(changes).filter(|(_, c)| **c == level).map(|(n, _)| n.clone()).collect::<Vec<_>>().join(", ")
    };
    let mut out = Vec::new();
    let (vh, h, e) = (list(ChangeLevel::VeryHard), list(ChangeLevel::Hard), list(ChangeLevel::Easy));
    if split_split {
        out.push(format!("Very hard to change (fixed within each whole plot): {vh}."));
        out.push(format!("Hard to change (fixed within each subplot): {h}."));
    } else {
        let whole: Vec<&str> = [vh.as_str(), h.as_str()].into_iter().filter(|s| !s.is_empty()).collect();
        out.push(format!("Hard to change (fixed within each whole plot): {}.", whole.join(", ")));
    }
    if !e.is_empty() {
        out.push(format!("Easy to change (reset on every run): {e}."));
    }
    out
}

/// Largest `f(x)ᵀ A⁻¹ f(x)` over the grid of candidate levels (NaN beyond
/// 12 factors).
fn max_grid_variance(formula: &ModelFormula, inverse: &[Vec<f64>]) -> f64 {
    let levels = candidate_levels(formula);
    if levels.len() > 12 {
        return f64::NAN;
    }
    let mut index = vec![0usize; levels.len()];
    let mut best = f64::NEG_INFINITY;
    loop {
        let x: Vec<f64> = index.iter().zip(&levels).map(|(&i, l)| l[i]).collect();
        let f = formula.expand(&x);
        let v: f64 =
            inverse.iter().zip(&f).map(|(row, a)| a * row.iter().zip(&f).map(|(r, b)| r * b).sum::<f64>()).sum();
        best = best.max(v);
        // Next grid point.
        let Some(j) = (0..index.len()).find(|&j| index[j] + 1 < levels[j].len()) else { break };
        index[j] += 1;
        index[..j].iter_mut().for_each(|i| *i = 0);
    }
    best
}

/// The coordinate-exchange state shared by all starts.
struct Search<'a> {
    formula: &'a ModelFormula,
    levels: Vec<Vec<f64>>,
    stratum: Vec<u8>,
    /// Runs of each whole plot.
    wp_runs: Vec<Vec<usize>>,
    /// `Vᵢ⁻¹` of each whole plot.
    v_inv: Vec<Vec<Vec<f64>>>,
    /// Units of change within each whole plot: (stratum, runs).
    units: Vec<Vec<(u8, Vec<usize>)>>,
    /// Whole plots before this one are fixed (an augmentation's existing runs).
    first_free: usize,
}

impl<'a> Search<'a> {
    fn new(formula: &'a ModelFormula, s: &PlotStructure, stratum: Vec<u8>, eta_w: f64, eta_s: f64) -> Result<Self> {
        let wp_runs = s.whole_plot_runs();
        let v_inv = wp_runs
            .iter()
            .map(|runs| invert(&s.block(runs, eta_w, eta_s)))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| invalid("the whole-plot covariance is not positive definite"))?;
        let units = wp_runs
            .iter()
            .map(|runs| {
                let mut u = vec![(0u8, runs.clone())];
                if let Some(sp) = &s.subplot {
                    let mut seen: Vec<usize> = Vec::new();
                    for &r in runs {
                        if !seen.contains(&sp[r]) {
                            seen.push(sp[r]);
                            u.push((1, runs.iter().copied().filter(|&q| sp[q] == sp[r]).collect()));
                        }
                    }
                }
                u.extend(runs.iter().map(|&r| (2, vec![r])));
                u
            })
            .collect();
        Ok(Self { formula, levels: candidate_levels(formula), stratum, wp_runs, v_inv, units, first_free: 0 })
    }

    fn random_runs(&self, rng: &mut Rng) -> Vec<Vec<f64>> {
        let k = self.levels.len();
        let n: usize = self.wp_runs.iter().map(Vec::len).sum();
        let mut runs = vec![vec![0.0; k]; n];
        for units in &self.units {
            for (stratum, members) in units {
                for j in (0..k).filter(|&j| self.stratum[j] == *stratum) {
                    let v = self.levels[j][rng.below(self.levels[j].len())];
                    members.iter().for_each(|&r| runs[r][j] = v);
                }
            }
        }
        runs
    }

    fn contribution(&self, wp: usize, rows: &[Vec<f64>]) -> Vec<Vec<f64>> {
        let p = self.formula.n_terms();
        let mut c = vec![vec![0.0; p]; p];
        let x: Vec<&Vec<f64>> = self.wp_runs[wp].iter().map(|&r| &rows[r]).collect();
        add_contribution(&mut c, &self.v_inv[wp], &x, 1.0);
        c
    }

    /// One coordinate-exchange search from `runs`, improving it in place.
    fn exchange(&self, runs: &mut [Vec<f64>], max_passes: usize) {
        let p = self.formula.n_terms();
        let mut rows: Vec<Vec<f64>> = runs.iter().map(|r| self.formula.expand(r)).collect();
        let mut contrib: Vec<Vec<Vec<f64>>> = (0..self.wp_runs.len()).map(|w| self.contribution(w, &rows)).collect();
        let total = |contrib: &[Vec<Vec<f64>>]| -> Vec<Vec<f64>> {
            let mut m = vec![vec![0.0; p]; p];
            for c in contrib {
                for (mr, cr) in m.iter_mut().zip(c) {
                    mr.iter_mut().zip(cr).for_each(|(a, b)| *a += b);
                }
            }
            m
        };
        let log_det = |m: &[Vec<f64>], delta: f64| -> f64 {
            let mut a = m.to_vec();
            (0..p).for_each(|i| a[i][i] += delta);
            cholesky(&a).map_or(f64::NEG_INFINITY, |l| log_det_from_cholesky(&l))
        };
        let mut m = total(&contrib);
        let mut delta = 0.0;
        let mut current = log_det(&m, delta);
        if !current.is_finite() {
            delta = 1e-6 * runs.len() as f64;
            current = log_det(&m, delta);
            if !current.is_finite() {
                return;
            }
        }
        for _ in 0..max_passes {
            let mut improved = false;
            for wp in self.first_free..self.wp_runs.len() {
                for (stratum, members) in &self.units[wp] {
                    for j in (0..self.levels.len()).filter(|&j| self.stratum[j] == *stratum) {
                        let now = runs[members[0]][j];
                        let mut best: Option<(f64, f64, Vec<Vec<f64>>)> = None;
                        for &v in &self.levels[j] {
                            if v == now {
                                continue;
                            }
                            let mut trial = rows.clone();
                            for &r in members {
                                let mut point = runs[r].clone();
                                point[j] = v;
                                trial[r] = self.formula.expand(&point);
                            }
                            let c = self.contribution(wp, &trial);
                            let mut cand = m.clone();
                            for ((mr, old), new) in cand.iter_mut().zip(&contrib[wp]).zip(&c) {
                                mr.iter_mut().zip(old).zip(new).for_each(|((a, o), n)| *a += n - o);
                            }
                            let ld = log_det(&cand, delta);
                            let bar = best.as_ref().map_or(current + 1e-9, |b| b.0);
                            if ld > bar {
                                best = Some((ld, v, c));
                            }
                        }
                        if let Some((_, v, c)) = best {
                            for &r in members {
                                runs[r][j] = v;
                                rows[r] = self.formula.expand(&runs[r]);
                            }
                            contrib[wp] = c;
                            // Recompute the sum so rounding does not build up.
                            m = total(&contrib);
                            current = log_det(&m, delta);
                            improved = true;
                        }
                    }
                }
            }
            if !improved {
                if delta > 0.0 {
                    let ld = log_det(&m, 0.0);
                    if ld.is_finite() {
                        delta = 0.0;
                        current = ld;
                        continue;
                    }
                }
                break;
            }
        }
    }
}

/// Options for [`augment_split_plot`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SplitPlotAugmentOptions {
    /// Whole plots to add (at least 1).
    pub new_whole_plots: usize,
    /// Runs in each new whole plot (at least 1).
    pub runs_per_whole_plot: usize,
    /// Prior ratio `η` of the whole-plot to the run variance.
    pub whole_plot_ratio: f64,
    /// Random starts.
    pub starts: usize,
    /// Seed of the starts.
    pub seed: u64,
    /// Most passes per start.
    pub max_passes: usize,
}

impl SplitPlotAugmentOptions {
    /// `new_whole_plots` whole plots of `runs_per_whole_plot` runs, `η` = 1,
    /// 20 starts, seed 1.
    #[must_use]
    pub fn new(new_whole_plots: usize, runs_per_whole_plot: usize) -> Self {
        Self { new_whole_plots, runs_per_whole_plot, whole_plot_ratio: 1.0, starts: 20, seed: 1, max_passes: 100 }
    }
}

/// A split-plot design with new whole plots added.
#[derive(Debug, Clone, PartialEq)]
pub struct AugmentedSplitPlot {
    /// Every run in coded units: the existing runs (in the order given),
    /// then the new whole plots, whole plot by whole plot.
    pub runs: Vec<Vec<f64>>,
    /// Whole plot of each run (0-based; existing whole plots numbered in
    /// order of first appearance, new ones after them).
    pub whole_plot: Vec<usize>,
    /// Number of existing runs.
    pub n_existing: usize,
    /// Number of existing whole plots.
    pub existing_whole_plots: usize,
    /// `ln det(XᵀV⁻¹X)` of the existing runs (−∞ if singular).
    pub log_det_before: f64,
    /// `ln det(XᵀV⁻¹X)` of all runs.
    pub log_det_after: f64,
    /// D-efficiency of all runs (as for [`SplitPlotDesign::d_efficiency`]).
    pub d_efficiency: f64,
    /// Whole-plot error degrees of freedom after augmenting (before: the
    /// same count with the existing whole plots).
    pub whole_plot_error_df: (i64, i64),
    /// Notes for the user.
    pub notes: Vec<String>,
}

/// Add whole plots to an existing split-plot design, keeping its runs:
/// D-optimal by coordinate exchange over the new whole plots only (Goos &
/// Jones 2011, *Optimal Design of Experiments: A Case Study Approach*,
/// ch. 10), maximizing `det(XᵀV⁻¹X)` of all runs. Hard-to-change factors
/// ([`ChangeLevel::Hard`] or [`ChangeLevel::VeryHard`]) stay fixed within each
/// new whole plot; easy ones vary from run to run. `whole_plot` labels the
/// existing runs' whole plots (any numbers).
///
/// # Errors
/// [`StatError::InvalidArgument`] for no existing runs, labels or runs of
/// the wrong length, a hard factor that varies within an existing whole
/// plot, no hard factor, both very hard and hard factors (a split-split-plot
/// design), zero new whole plots or runs, a negative or non-finite ratio,
/// no starts or more than 10 000 runs; [`StatError::RankDeficient`] if the
/// augmented design is singular for every start.
pub fn augment_split_plot(
    existing: &[Vec<f64>],
    whole_plot: &[usize],
    formula: &ModelFormula,
    changes: &[ChangeLevel],
    options: &SplitPlotAugmentOptions,
) -> Result<AugmentedSplitPlot> {
    let (k, p) = (formula.n_factors(), formula.n_terms());
    if existing.is_empty() || whole_plot.len() != existing.len() {
        return Err(invalid("give the existing runs and the whole plot of each"));
    }
    if existing.iter().any(|r| r.len() != k || r.iter().any(|v| !v.is_finite())) {
        return Err(invalid(format!("every run needs a setting for each of the {k} factors")));
    }
    if changes.len() != k {
        return Err(invalid(format!("give a change level for each of the {k} factors")));
    }
    if changes.iter().all(|c| *c == ChangeLevel::Easy) {
        return Err(invalid("no factor is hard to change, so the design has no whole-plot factors"));
    }
    if changes.contains(&ChangeLevel::VeryHard) && changes.contains(&ChangeLevel::Hard) {
        return Err(invalid("adding whole plots to a split-split-plot design is not supported"));
    }
    if options.new_whole_plots == 0 || options.runs_per_whole_plot == 0 {
        return Err(invalid("add at least one whole plot of at least one run"));
    }
    if !(options.whole_plot_ratio.is_finite() && options.whole_plot_ratio >= 0.0) {
        return Err(invalid("the variance ratio must be finite and not negative"));
    }
    let added = options.new_whole_plots * options.runs_per_whole_plot;
    let n = existing.len() + added;
    if options.starts == 0 || n > 10_000 {
        return Err(invalid("use at least one start and at most 10 000 runs"));
    }
    // Existing whole plots numbered in order of first appearance.
    let mut ids: Vec<usize> = Vec::new();
    let labels: Vec<usize> = whole_plot
        .iter()
        .map(|w| {
            ids.iter().position(|v| v == w).unwrap_or_else(|| {
                ids.push(*w);
                ids.len() - 1
            })
        })
        .collect();
    let w0 = ids.len();
    let (stratum, _) = strata(changes);
    let names = default_factor_names(k);
    for j in (0..k).filter(|&j| stratum[j] == 0) {
        for a in 0..existing.len() {
            if let Some(b) = (0..a).find(|&b| labels[b] == labels[a] && existing[b][j] != existing[a][j]) {
                return Err(invalid(format!(
                    "{} is hard to change but varies within an existing whole plot (rows {} and {})",
                    names[j],
                    b + 1,
                    a + 1
                )));
            }
        }
    }
    let mut all_labels = labels.clone();
    for w in 0..options.new_whole_plots {
        all_labels.extend(std::iter::repeat_n(w0 + w, options.runs_per_whole_plot));
    }
    let structure = PlotStructure { whole_plot: all_labels, subplot: None };
    let eta = options.whole_plot_ratio;
    let mut search = Search::new(formula, &structure, stratum.clone(), eta, 0.0)?;
    search.first_free = w0;
    let mut rng = Rng::new(options.seed);
    let mut best: Option<(f64, Vec<Vec<f64>>)> = None;
    for _ in 0..options.starts {
        let mut runs = search.random_runs(&mut rng);
        runs[..existing.len()].clone_from_slice(existing);
        search.exchange(&mut runs, options.max_passes);
        let ld = split_plot_log_det(formula, &runs, &structure, eta, 0.0);
        if ld.is_finite() && best.as_ref().is_none_or(|(b, _)| ld > *b + 1e-12 * b.abs().max(1.0)) {
            best = Some((ld, runs));
        }
    }
    let (log_det_after, runs) = best.ok_or(StatError::RankDeficient { rank: 0, columns: p })?;
    let before = PlotStructure { whole_plot: labels.clone(), subplot: None };
    let log_det_before = split_plot_log_det(formula, existing, &before, eta, 0.0);
    let d_efficiency = 100.0 * (1.0 + eta) * (log_det_after / p as f64).exp() / n as f64;
    let whole_plot_terms =
        formula.terms().iter().filter(|t| t.powers.iter().zip(&stratum).all(|(&pw, &s)| pw == 0 || s == 0)).count()
            as i64;
    let df = (w0 as i64 - whole_plot_terms, (w0 + options.new_whole_plots) as i64 - whole_plot_terms);
    let hard: Vec<&str> = names.iter().zip(&stratum).filter(|(_, s)| **s == 0).map(|(n, _)| n.as_str()).collect();
    let mut notes = vec![format!(
        "Added {} whole plots of {} runs ({added} runs) to {w0} whole plots ({} runs); {} fixed within each new \
         whole plot.",
        options.new_whole_plots,
        options.runs_per_whole_plot,
        existing.len(),
        hard.join(", ")
    )];
    notes.push(format!(
        "ln det(XᵀV⁻¹X) with η = {}: {} before, {log_det_after:.6} after; D-efficiency after {d_efficiency:.2}%; \
         best of {} coordinate-exchange starts (seed {}).",
        fmt(eta),
        if log_det_before.is_finite() { format!("{log_det_before:.6}") } else { "singular".to_owned() },
        options.starts,
        options.seed
    ));
    notes.push(format!(
        "Whole-plot error degrees of freedom: {} before, {} after ({whole_plot_terms} whole-plot terms).",
        df.0, df.1
    ));
    Ok(AugmentedSplitPlot {
        runs,
        whole_plot: structure.whole_plot,
        n_existing: existing.len(),
        existing_whole_plots: w0,
        log_det_before,
        log_det_after,
        d_efficiency,
        whole_plot_error_df: df,
        notes,
    })
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plot_structures() {
        let s = PlotStructure::split_plot(10, 4).unwrap();
        assert_eq!(s.whole_plot, [0, 0, 0, 1, 1, 1, 2, 2, 3, 3]);
        assert_eq!((s.n_whole_plots(), s.n_subplots()), (4, 0));
        let s = PlotStructure::split_split_plot(8, 2, 4).unwrap();
        assert_eq!(s.whole_plot, [0, 0, 0, 0, 1, 1, 1, 1]);
        assert_eq!(s.subplot.as_deref(), Some(&[0, 0, 1, 1, 2, 2, 3, 3][..]));
        assert!(PlotStructure::split_plot(4, 5).is_err());
        assert!(PlotStructure::split_split_plot(8, 3, 2).is_err());
        // V block of a split-split whole plot.
        let v = s.block(&[0, 1, 2], 2.0, 0.5);
        assert_eq!(v, vec![vec![3.5, 2.5, 2.0], vec![2.5, 3.5, 2.0], vec![2.0, 2.0, 3.5]]);
    }

    #[test]
    fn change_level_keys() {
        for c in ChangeLevel::ALL {
            assert_eq!(ChangeLevel::from_key(c.key()), Some(c));
            assert_eq!(ChangeLevel::from_key(c.label()), Some(c));
        }
        assert_eq!(ChangeLevel::from_key("very_hard"), Some(ChangeLevel::VeryHard));
        assert_eq!(ChangeLevel::from_key("soft"), None);
    }

    /// Whole-plot factors are constant within whole plots, subplot factors
    /// within subplots; the run counts are as asked.
    #[test]
    fn restrictions_hold() {
        use ChangeLevel::{Easy, Hard, VeryHard};
        let f = ModelFormula::interactions(4);
        let d = split_plot_design(&f, &[Hard, Easy, Hard, Easy], &SplitPlotOptions::new(16, 4)).unwrap();
        assert_eq!(d.design.n_runs(), 16);
        let wp = &d.structure.whole_plot;
        for (a, b) in (0..16).flat_map(|a| (0..16).map(move |b| (a, b))) {
            if wp[a] == wp[b] {
                assert_eq!(d.design.runs[a][0], d.design.runs[b][0]);
                assert_eq!(d.design.runs[a][2], d.design.runs[b][2]);
            }
        }
        assert!(d.design.description.starts_with("Split-plot D-optimal"));
        assert_eq!(d.whole_plot_error_df, 4 - 4, "intercept, A, C, A×C");
        assert!(d.design.notes.iter().any(|n| n.contains("Hard to change (fixed within each whole plot): A, C.")));

        let f = ModelFormula::main_effects(3);
        let mut o = SplitPlotOptions::new(16, 4);
        o.subplots = 8;
        let d = split_plot_design(&f, &[VeryHard, Hard, Easy], &o).unwrap();
        let sp = d.structure.subplot.as_ref().unwrap();
        assert_eq!((d.structure.n_whole_plots(), d.structure.n_subplots()), (4, 8));
        for a in 0..16 {
            for b in 0..16 {
                if d.structure.whole_plot[a] == d.structure.whole_plot[b] {
                    assert_eq!(d.design.runs[a][0], d.design.runs[b][0]);
                }
                if sp[a] == sp[b] {
                    assert_eq!(d.design.runs[a][1], d.design.runs[b][1]);
                }
            }
        }
        assert!(d.design.description.starts_with("Split-split-plot"));
        // Only very-hard factors: an ordinary split plot.
        let d = split_plot_design(&f, &[VeryHard, Easy, Easy], &o).unwrap();
        assert!(d.structure.subplot.is_none());
    }

    #[test]
    fn validation() {
        use ChangeLevel::{Easy, Hard};
        let f = ModelFormula::main_effects(2);
        assert!(split_plot_design(&f, &[Easy, Easy], &SplitPlotOptions::new(8, 4)).is_err(), "no hard factor");
        assert!(split_plot_design(&f, &[Hard], &SplitPlotOptions::new(8, 4)).is_err());
        assert!(split_plot_design(&f, &[Hard, Easy], &SplitPlotOptions::new(2, 2)).is_err(), "too few runs");
        assert!(split_plot_design(&f, &[Hard, Easy], &SplitPlotOptions::new(8, 9)).is_err());
        let mut o = SplitPlotOptions::new(8, 4);
        o.whole_plot_ratio = -1.0;
        assert!(split_plot_design(&f, &[Hard, Easy], &o).is_err());
    }

    /// The log-determinant of the 2² design with A in two whole plots of two
    /// runs: Vᵢ⁻¹ = I − J/3, so XᵀV⁻¹X = diag(4/3, 4/3, 4, 4).
    #[test]
    fn hand_computed_information() {
        let f = ModelFormula::interactions(2);
        let runs = vec![vec![-1.0, -1.0], vec![-1.0, 1.0], vec![1.0, -1.0], vec![1.0, 1.0]];
        let s = PlotStructure::split_plot(4, 2).unwrap();
        let ld = split_plot_log_det(&f, &runs, &s, 1.0, 0.0);
        assert!((ld - (16.0f64 / 9.0 * 16.0).ln()).abs() < 1e-12, "{ld}");
        // η = 0 is ordinary least squares.
        assert!((split_plot_log_det(&f, &runs, &s, 0.0, 0.0) - 256f64.ln()).abs() < 1e-12);
    }

    /// New whole plots keep the existing runs, hold the hard factors fixed,
    /// add whole-plot error degrees of freedom and increase the information.
    #[test]
    fn augmenting_with_whole_plots() {
        use ChangeLevel::{Easy, Hard, VeryHard};
        let f = ModelFormula::interactions(3);
        let d = split_plot_design(&f, &[Hard, Easy, Easy], &SplitPlotOptions::new(12, 4)).unwrap();
        let labels: Vec<usize> = d.structure.whole_plot.iter().map(|w| 10 + 3 * w).collect();
        let a =
            augment_split_plot(&d.design.runs, &labels, &f, &[Hard, Easy, Easy], &SplitPlotAugmentOptions::new(2, 3))
                .unwrap();
        assert_eq!(a.runs.len(), 18);
        assert_eq!(&a.runs[..12], &d.design.runs[..]);
        assert_eq!((a.n_existing, a.existing_whole_plots), (12, 4));
        assert_eq!(&a.whole_plot[12..], &[4, 4, 4, 5, 5, 5]);
        for w in [4, 5] {
            let rows: Vec<&Vec<f64>> =
                a.runs.iter().zip(&a.whole_plot).filter(|(_, g)| **g == w).map(|(r, _)| r).collect();
            assert!(rows.iter().all(|r| r[0] == rows[0][0]), "A fixed within the new whole plot");
        }
        assert!((a.log_det_before - d.log_det).abs() < 1e-9);
        assert!(a.log_det_after > a.log_det_before);
        assert_eq!(a.whole_plot_error_df, (4 - 2, 6 - 2), "intercept and A are whole-plot terms");
        // A local optimum: no single change of an easy factor in a new run
        // improves the result.
        let s = PlotStructure { whole_plot: a.whole_plot.clone(), subplot: None };
        for r in 12..18 {
            for j in 1..3 {
                let mut runs = a.runs.clone();
                runs[r][j] = -runs[r][j];
                assert!(split_plot_log_det(&f, &runs, &s, 1.0, 0.0) <= a.log_det_after + 1e-9);
            }
        }
        let mut bad = d.design.runs.clone();
        bad[1][0] = -bad[0][0];
        assert!(
            augment_split_plot(&bad, &labels, &f, &[Hard, Easy, Easy], &SplitPlotAugmentOptions::new(2, 3)).is_err()
        );
        assert!(
            augment_split_plot(&d.design.runs, &labels, &f, &[Easy; 3], &SplitPlotAugmentOptions::new(2, 3)).is_err()
        );
        assert!(
            augment_split_plot(
                &d.design.runs,
                &labels,
                &f,
                &[VeryHard, Hard, Easy],
                &SplitPlotAugmentOptions::new(2, 3)
            )
            .is_err()
        );
        assert!(
            augment_split_plot(&d.design.runs, &labels, &f, &[Hard, Easy, Easy], &SplitPlotAugmentOptions::new(0, 3))
                .is_err()
        );
    }

    /// Exhaustive search (`tests/data/split_plot_augment_reference.py`):
    /// A hard, B easy, 4 runs in 2 whole plots; the best new whole plots.
    #[test]
    fn augmenting_matches_exhaustive_search() {
        use ChangeLevel::{Easy, Hard};
        let existing = vec![vec![-1.0, -1.0], vec![-1.0, 1.0], vec![1.0, -1.0], vec![1.0, 1.0]];
        let labels = [0, 0, 1, 1];
        let interactions = ModelFormula::interactions(2);
        let quadratic = ModelFormula::parse("A + B + A*B + B^2", &["A".to_owned(), "B".to_owned()]).unwrap();
        for (formula, plots, runs, eta, want) in [
            (&interactions, 1, 2, 1.0, 4.734247228263234),
            (&interactions, 2, 2, 1.0, 6.120541589383125),
            (&interactions, 2, 2, 4.0, 3.9233170120469048),
            (&quadratic, 2, 3, 1.0, 6.586631319307724),
        ] {
            let options = SplitPlotAugmentOptions {
                whole_plot_ratio: eta,
                starts: 30,
                ..SplitPlotAugmentOptions::new(plots, runs)
            };
            let a = augment_split_plot(&existing, &labels, formula, &[Hard, Easy], &options).unwrap();
            assert!(
                (a.log_det_after - want).abs() < 1e-9,
                "{plots} × {runs}, η = {eta}: {} vs {want}",
                a.log_det_after
            );
        }
        let a =
            augment_split_plot(&existing, &labels, &interactions, &[Hard, Easy], &SplitPlotAugmentOptions::new(1, 2))
                .unwrap();
        assert!((a.log_det_before - 3.347952867143343).abs() < 1e-12);
    }

    #[test]
    fn randomized_order_respects_the_plots() {
        use ChangeLevel::{Easy, Hard, VeryHard};
        let f = ModelFormula::main_effects(3);
        let mut o = SplitPlotOptions::new(12, 3);
        o.subplots = 6;
        o.starts = 3;
        let d = split_plot_design(&f, &[VeryHard, Hard, Easy], &o).unwrap();
        let order = d.run_order(Some(5));
        let mut sorted = order.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..12).collect::<Vec<_>>());
        // Each whole plot and subplot occupies consecutive positions.
        let sp = d.structure.subplot.as_ref().unwrap();
        for labels in [&d.structure.whole_plot, sp] {
            let seq: Vec<usize> = order.iter().map(|&i| labels[i]).collect();
            let mut changes = seq.windows(2).filter(|w| w[0] != w[1]).count();
            changes += 1;
            let mut distinct = seq.clone();
            distinct.sort_unstable();
            distinct.dedup();
            assert_eq!(changes, distinct.len(), "{seq:?}");
        }
        let t = d.to_table("sp", &TableOptions { randomize: Some(5), ..TableOptions::default() }).unwrap();
        let names: Vec<&str> = t.columns().iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["Run", "Std order", "Whole Plot", "Subplot", "A", "B", "C", "Y"]);
        let wp = t.column(2).unwrap().as_numeric().unwrap();
        assert_eq!(wp[0], 1.0, "whole plots numbered in run order");
        assert_eq!(t.column(2).unwrap().level, MeasurementLevel::Nominal);
    }
}
