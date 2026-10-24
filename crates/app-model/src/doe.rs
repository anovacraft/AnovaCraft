//! Creating designed experiments from the application: a [`DesignRequest`]
//! names the design type, the factors and the export options, and
//! [`create_design`] builds it with [`stat_doe`] and turns it into a data table
//! ready to receive results. Mixture experiments, whose components are
//! proportions of a blend rather than factors with low and high settings,
//! have their own [`MixtureDesignRequest`] and [`create_mixture_design`].
//!
//! A split-plot design ([`DesignKind::SplitPlot`]) for hard-to-change
//! factors gets `Whole Plot` (and `Subplot`) columns; [`design_project`]
//! opens it with the design declared (whole-plot columns and model recorded)
//! and a project script that fits the mixed model analysing it.

use stat_core::{Result, StatError};
use stat_data::{Column, DataTable, MeasurementLevel};
pub use stat_doe::DsdOptions;
use stat_doe::{
    AxialDistance, CcdOptions, Design, Factor, MixtureDesign, ModelFormula, OptimalOptions, Region, Rng, ScheffeModel,
    TableOptions, Units, box_behnken, central_composite, d_optimal_in, default_factor_names, extreme_vertices,
    fractional_factorial, full_factorial, i_optimal_in, plackett_burman, simplex_centroid, simplex_lattice,
    symmetric_eigen, three_level_factorial, two_level_factorial,
};
use stat_doe::{CategoricalScreeningOptions, ScreeningFactor, categorical_screening, definitive_screening_with};
use stat_doe::{
    ChangeLevel, SplitPlotDesign, SplitPlotOptions,
    split_plot::{SUBPLOT_COLUMN, WHOLE_PLOT_COLUMN},
    split_plot_design,
};
use stat_doe::{SpaceFillingKind, space_filling_design};

/// The kind of design to create.
#[derive(Debug, Clone, PartialEq)]
pub enum DesignKind {
    /// `2^k` full factorial.
    TwoLevelFactorial,
    /// `3^k` full factorial.
    ThreeLevelFactorial,
    /// Full factorial with any number of levels per factor (mixed levels);
    /// the levels of each factor are evenly spaced from its low to its high
    /// setting.
    GeneralFactorial {
        /// Number of levels of each factor (at least 2), one per factor.
        levels: Vec<usize>,
    },
    /// `2^(k−p)` fractional factorial.
    FractionalFactorial {
        /// Fraction exponent `p`.
        p: usize,
    },
    /// Plackett–Burman screening design.
    PlackettBurman,
    /// Definitive screening design; its center runs are
    /// [`DesignRequest::center_points`] in all (default 1).
    DefinitiveScreening {
        /// At least this many runs beyond the smallest design, from a larger
        /// conference matrix (see [`stat_doe::DsdOptions`]).
        extra_runs: usize,
    },
    /// D-optimal screening design for two-level continuous factors and
    /// categorical factors of any number of levels (see
    /// [`stat_doe::categorical_screening`]). Categorical factors come out as
    /// nominal text columns with levels `L1`, `L2`, …; their low and high
    /// settings are ignored.
    CategoricalScreening {
        /// Levels of each factor: `None` for continuous, `Some(L)` for a
        /// categorical factor with `L` levels; one per factor.
        levels: Vec<Option<usize>>,
        /// Number of runs.
        runs: usize,
        /// Include every two-factor interaction in the model.
        interactions: bool,
        /// Random starts.
        starts: usize,
        /// Seed of the starts.
        seed: u64,
    },
    /// Central composite design.
    CentralComposite {
        /// Axial distance.
        alpha: AxialDistance,
        /// Scale so axial points are at the factor limits.
        inscribed: bool,
    },
    /// Box–Behnken design.
    BoxBehnken,
    /// D-optimal design by coordinate exchange.
    DOptimal {
        /// Model formula text (see [`ModelFormula::parse`]).
        formula: String,
        /// Number of runs.
        runs: usize,
        /// Random starts.
        starts: usize,
        /// Seed of the starts.
        seed: u64,
        /// Constraints on the factors in actual units, one per line (see
        /// [`stat_doe::Region::parse`]); empty for none.
        constraints: String,
    },
    /// I-optimal design by coordinate exchange: minimum average prediction
    /// variance over the region (for response-surface prediction).
    IOptimal {
        /// Model formula text (see [`ModelFormula::parse`]).
        formula: String,
        /// Number of runs.
        runs: usize,
        /// Random starts.
        starts: usize,
        /// Seed of the starts.
        seed: u64,
        /// Constraints on the factors in actual units, one per line (see
        /// [`stat_doe::Region::parse`]); empty for none.
        constraints: String,
    },
    /// D-optimal split-plot (or split-split-plot) design by coordinate
    /// exchange for factors that are hard to change (see
    /// [`stat_doe::split_plot`]).
    SplitPlot {
        /// Model formula text (see [`ModelFormula::parse`]).
        formula: String,
        /// Number of runs.
        runs: usize,
        /// Number of whole plots.
        whole_plots: usize,
        /// Number of subplots (used when some factors are very hard and
        /// others hard to change).
        subplots: usize,
        /// How hard each factor is to change, one per factor.
        changes: Vec<ChangeLevel>,
        /// Prior ratio of the whole-plot to the run variance (`η_w`).
        whole_plot_ratio: f64,
        /// Prior ratio of the subplot to the run variance (`η_s`).
        subplot_ratio: f64,
        /// Random starts.
        starts: usize,
        /// Seed of the starts.
        seed: u64,
    },
    /// Space-filling design for computer experiments (see
    /// [`stat_doe::space_filling`]): runs spread evenly over the factors'
    /// ranges, with no replicates or centre points.
    SpaceFilling {
        /// Criterion and construction.
        criterion: SpaceFillingKind,
        /// Number of runs (2 to 1000).
        runs: usize,
        /// Seed of the search.
        seed: u64,
    },
}

impl DesignKind {
    /// Short name for menus.
    #[must_use]
    pub fn label(&self) -> &'static str {
        match self {
            Self::TwoLevelFactorial => "Full factorial (2 levels)",
            Self::ThreeLevelFactorial => "Full factorial (3 levels)",
            Self::GeneralFactorial { .. } => "Full factorial (mixed levels)",
            Self::FractionalFactorial { .. } => "Fractional factorial",
            Self::PlackettBurman => "Plackett–Burman",
            Self::DefinitiveScreening { .. } => "Definitive screening",
            Self::CategoricalScreening { .. } => "Screening with categorical factors",
            Self::CentralComposite { .. } => "Central composite",
            Self::BoxBehnken => "Box–Behnken",
            Self::DOptimal { .. } => "D-optimal (coordinate exchange)",
            Self::IOptimal { .. } => "I-optimal (coordinate exchange)",
            Self::SplitPlot { .. } => "Split-plot D-optimal",
            Self::SpaceFilling { criterion, .. } => criterion.label(),
        }
    }

    /// Whether extra centre points can be requested (CCD and Box–Behnken take
    /// their own defaults when none are given). A definitive screening
    /// design takes its total number of center runs instead.
    #[must_use]
    pub fn accepts_center_points(&self) -> bool {
        !matches!(
            self,
            Self::DefinitiveScreening { .. }
                | Self::CategoricalScreening { .. }
                | Self::DOptimal { .. }
                | Self::IOptimal { .. }
                | Self::SplitPlot { .. }
                | Self::ThreeLevelFactorial
                | Self::GeneralFactorial { .. }
                | Self::SpaceFilling { .. }
        )
    }
}

/// Everything needed to create a design table.
#[derive(Debug, Clone, PartialEq)]
pub struct DesignRequest {
    /// Design type.
    pub kind: DesignKind,
    /// Factor names and low/high settings (their number is `k`).
    pub factors: Vec<Factor>,
    /// Centre points: `None` uses the design's default (none for factorial and
    /// screening designs).
    pub center_points: Option<usize>,
    /// Randomize the run order with this seed.
    pub randomize: Option<u64>,
    /// Units of the factor columns.
    pub units: Units,
    /// Name of the empty response column (none if blank).
    pub response: String,
}

impl DesignRequest {
    /// A request for `kind` with `k` factors named `A, B, …` at −1 / +1.
    #[must_use]
    pub fn new(kind: DesignKind, k: usize) -> Self {
        Self {
            kind,
            factors: default_factor_names(k).into_iter().map(Factor::coded).collect(),
            center_points: None,
            randomize: Some(1),
            units: Units::Actual,
            response: "Y".to_owned(),
        }
    }
}

/// Build the design described by `request`.
///
/// # Errors
/// Any error from the generator (unsupported factor count, unknown fraction,
/// invalid formula, too few runs…) or from the factor settings.
pub fn build_design(request: &DesignRequest) -> Result<Design> {
    let k = request.factors.len();
    if k == 0 {
        return Err(StatError::InvalidArgument { message: "add at least one factor".into() });
    }
    let extra_centers = |mut d: Design| {
        if let Some(c) = request.center_points {
            d.add_center_points(c);
            if c > 0 {
                d.description = format!("{} + {c} center runs", d.description);
            }
        }
        d
    };
    let design = match &request.kind {
        DesignKind::TwoLevelFactorial => extra_centers(two_level_factorial(k)?),
        DesignKind::ThreeLevelFactorial => three_level_factorial(k)?,
        DesignKind::GeneralFactorial { levels } => {
            if levels.len() != k {
                return Err(StatError::InvalidArgument {
                    message: format!("give a number of levels for each of the {k} factors"),
                });
            }
            full_factorial(levels)?
        }
        DesignKind::FractionalFactorial { p } => extra_centers(fractional_factorial(k, *p)?.design),
        DesignKind::PlackettBurman => extra_centers(plackett_burman(k)?),
        DesignKind::DefinitiveScreening { extra_runs } => definitive_screening_with(
            k,
            &DsdOptions { extra_runs: *extra_runs, center_runs: request.center_points.unwrap_or(1) },
        )?,
        DesignKind::CategoricalScreening { levels, runs, interactions, starts, seed } => {
            if levels.len() != k {
                return Err(StatError::InvalidArgument {
                    message: format!("give the kind of each of the {k} factors"),
                });
            }
            let kinds: Vec<ScreeningFactor> = levels
                .iter()
                .map(|l| l.map_or(ScreeningFactor::Continuous, |levels| ScreeningFactor::Categorical { levels }))
                .collect();
            let options = CategoricalScreeningOptions {
                runs: *runs,
                interactions: *interactions,
                starts: *starts,
                seed: *seed,
                max_passes: 100,
            };
            let d = categorical_screening(&kinds, &options)?.design;
            // Keep each categorical factor's 1 … L coding in actual units.
            let factors: Vec<Factor> = request
                .factors
                .iter()
                .zip(levels)
                .map(|(f, l)| match l {
                    Some(l) => Factor::new(f.name.clone(), 1.0, *l as f64),
                    None => f.clone(),
                })
                .collect();
            let mut d = d.with_factors(factors)?;
            let names: Vec<String> = request.factors.iter().map(|f| f.name.clone()).collect();
            let defaults = default_factor_names(k);
            for note in &mut d.notes {
                if let Some((head, rest)) = note.split_once(": levels 1 to ") {
                    if let Some(j) = defaults.iter().position(|n| n == head) {
                        *note = format!("{}: levels L1 to L{rest}", names[j]);
                    }
                }
            }
            return Ok(d);
        }
        DesignKind::CentralComposite { alpha, inscribed } => central_composite(
            k,
            &CcdOptions { alpha: *alpha, inscribed: *inscribed, center_points: request.center_points, fraction: None },
        )?,
        DesignKind::BoxBehnken => box_behnken(k, request.center_points)?,
        DesignKind::DOptimal { formula, runs, starts, seed, constraints }
        | DesignKind::IOptimal { formula, runs, starts, seed, constraints } => {
            let names: Vec<String> = request.factors.iter().map(|f| f.name.clone()).collect();
            let model = ModelFormula::parse(formula, &names)?;
            let region = Region::parse(constraints, &request.factors)?;
            let options = OptimalOptions { runs: *runs, starts: *starts, seed: *seed, max_passes: 100 };
            let mut d = if matches!(request.kind, DesignKind::IOptimal { .. }) {
                i_optimal_in(&model, &region, &options)?.design
            } else {
                d_optimal_in(&model, &region, &options)?.design
            };
            // Restate the model with the user's factor names.
            if let Some(n) = d.notes.first_mut() {
                *n = format!("Model: {}", model.describe(&names));
            }
            d
        }
        DesignKind::SpaceFilling { criterion, runs, seed } => space_filling_design(*criterion, *runs, k, *seed)?,
        DesignKind::SplitPlot { .. } => return Ok(build_split_plot(request)?.design),
    };
    design.with_factors(request.factors.clone())
}

/// Build the split-plot design described by `request`, with the request's
/// factor names and settings.
///
/// # Errors
/// [`StatError::InvalidArgument`] if `request` is not a split-plot request
/// or names no factors; errors from [`split_plot_design`] or the formula.
pub fn build_split_plot(request: &DesignRequest) -> Result<SplitPlotDesign> {
    let DesignKind::SplitPlot {
        formula,
        runs,
        whole_plots,
        subplots,
        changes,
        whole_plot_ratio,
        subplot_ratio,
        starts,
        seed,
    } = &request.kind
    else {
        return Err(StatError::InvalidArgument { message: "not a split-plot design".into() });
    };
    if request.factors.is_empty() {
        return Err(StatError::InvalidArgument { message: "add at least one factor".into() });
    }
    let names: Vec<String> = request.factors.iter().map(|f| f.name.clone()).collect();
    let model = ModelFormula::parse(formula, &names)?;
    let options = SplitPlotOptions {
        runs: *runs,
        whole_plots: *whole_plots,
        subplots: *subplots,
        whole_plot_ratio: *whole_plot_ratio,
        subplot_ratio: *subplot_ratio,
        starts: *starts,
        seed: *seed,
        max_passes: 100,
    };
    let mut d = split_plot_design(&model, changes, &options)?;
    // Restate the model and the change levels with the user's factor names.
    let defaults = default_factor_names(names.len());
    for note in &mut d.design.notes {
        if note.starts_with("Model: ") {
            *note = format!("Model: {}", model.describe(&names));
        } else if note.contains("to change (") {
            let (head, list) = note.split_once("): ").unwrap_or((note.as_str(), ""));
            let renamed: Vec<String> = list
                .trim_end_matches('.')
                .split(", ")
                .map(|n| defaults.iter().position(|d| d == n).map_or_else(|| n.to_owned(), |j| names[j].clone()))
                .collect();
            *note = format!("{head}): {}.", renamed.join(", "));
        }
    }
    d.design = d.design.with_factors(request.factors.clone())?;
    Ok(d)
}

/// Build the design and export it as a data table named after the design.
///
/// # Errors
/// As for [`build_design`], or if the response name clashes with a factor.
pub fn create_design(request: &DesignRequest) -> Result<(DataTable, Design)> {
    let response = request.response.trim();
    if request.factors.iter().any(|f| f.name == response) {
        return Err(StatError::InvalidArgument { message: format!("the response {response:?} is also a factor name") });
    }
    let options = TableOptions {
        units: request.units,
        randomize: request.randomize,
        responses: if response.is_empty() { vec![] } else { vec![response.to_owned()] },
        point_type_column: true,
    };
    let name = request.kind.label().to_owned();
    if matches!(request.kind, DesignKind::SplitPlot { .. }) {
        if request.factors.iter().any(|f| f.name == WHOLE_PLOT_COLUMN || f.name == SUBPLOT_COLUMN) {
            return Err(StatError::InvalidArgument {
                message: format!(
                    "{WHOLE_PLOT_COLUMN:?} and {SUBPLOT_COLUMN:?} name the plot columns; rename the factor"
                ),
            });
        }
        let d = build_split_plot(request)?;
        let table = d.to_table(&name, &options)?;
        return Ok((table, d.design));
    }
    let design = build_design(request)?;
    let mut table = design.to_table(&name, &options)?;
    if let DesignKind::CategoricalScreening { levels, .. } = &request.kind {
        table = categorical_columns(table, &request.factors, levels, request.units)?;
    }
    Ok((table, design))
}

/// The design table with each categorical factor's column replaced by a
/// nominal text column of level labels `L1`, `L2`, … (from the level numbers
/// in actual units, or the codes in coded units).
fn categorical_columns(
    table: DataTable,
    factors: &[Factor],
    levels: &[Option<usize>],
    units: Units,
) -> Result<DataTable> {
    let mut columns = table.columns().to_vec();
    for (f, l) in factors.iter().zip(levels) {
        let Some(l) = *l else { continue };
        let Some(j) = table.find(&f.name) else { continue };
        let values = columns[j].as_numeric().unwrap_or(&[]).to_vec();
        let labels = values
            .iter()
            .map(|&v| {
                let level = match units {
                    Units::Actual => (v.round().max(1.0) as usize).min(l) - 1,
                    Units::Coded => stat_doe::categorical_screening::level_of(v, l),
                };
                format!("L{}", level + 1)
            })
            .collect();
        columns[j] = Column::text(f.name.clone(), labels);
    }
    DataTable::new(table.name(), columns)
}

/// A split-plot design request from script settings (JSON): `factors`
/// (`[#{ name, low, high, changes }]` with `changes` `"easy"`, `"hard"` or
/// `"very-hard"`; low and high default to −1 and +1), `model` (formula over
/// the factor names, or `"main effects"`, `"interactions"` (default),
/// `"quadratic"`), `runs`, `whole_plots`, `subplots` (0), `ratio` (η_w, 1),
/// `subplot_ratio` (η_s, 1), `starts` (20), `seed` (1), `randomize` (a seed,
/// default 1, or `false`), `units` (`"actual"` or `"coded"`), `response`
/// (`"Y"`).
///
/// # Errors
/// Missing or malformed settings.
pub fn split_plot_request_from_json(v: &stat_data::JsonValue) -> Result<DesignRequest> {
    use stat_data::JsonValue;
    let invalid = |m: String| StatError::InvalidArgument { message: m };
    let list = v.get("factors").and_then(JsonValue::as_array).ok_or_else(|| invalid("give the factors".into()))?;
    let mut factors = Vec::new();
    let mut changes = Vec::new();
    for (i, f) in list.iter().enumerate() {
        let name = f
            .get("name")
            .and_then(JsonValue::as_str)
            .map_or_else(|| default_factor_names(list.len())[i].clone(), str::to_owned);
        let low = f.get("low").and_then(JsonValue::as_f64).unwrap_or(-1.0);
        let high = f.get("high").and_then(JsonValue::as_f64).unwrap_or(1.0);
        let level = f.get("changes").and_then(JsonValue::as_str).unwrap_or("easy");
        changes.push(
            ChangeLevel::from_key(level)
                .ok_or_else(|| invalid(format!("unknown change level {level:?} (use easy, hard or very-hard)")))?,
        );
        factors.push(Factor::new(name, low, high));
    }
    let count = |key: &str, default: usize| -> Result<usize> {
        match v.get(key) {
            None | Some(JsonValue::Null) => Ok(default),
            Some(j) => j.as_usize().ok_or_else(|| invalid(format!("{key}: expected a whole number"))),
        }
    };
    let ratio = |key: &str| v.get(key).and_then(JsonValue::as_f64).unwrap_or(1.0);
    let runs = count("runs", 0)?;
    if runs == 0 {
        return Err(invalid("give the number of runs".into()));
    }
    let randomize = match v.get("randomize") {
        None | Some(JsonValue::Null) => Some(1),
        Some(JsonValue::Bool(false)) => None,
        Some(JsonValue::Bool(true)) => Some(1),
        Some(j) => Some(crate::seed::seed_from_json(j).ok_or_else(|| invalid("randomize: a seed or false".into()))?),
    };
    let units = match v.get("units").and_then(JsonValue::as_str).unwrap_or("actual") {
        "coded" => Units::Coded,
        "actual" => Units::Actual,
        other => return Err(invalid(format!("units: {other:?} (use actual or coded)"))),
    };
    Ok(DesignRequest {
        kind: DesignKind::SplitPlot {
            formula: v.get("model").and_then(JsonValue::as_str).unwrap_or("interactions").to_owned(),
            runs,
            whole_plots: count("whole_plots", 0)?,
            subplots: count("subplots", 0)?,
            changes,
            whole_plot_ratio: ratio("ratio"),
            subplot_ratio: ratio("subplot_ratio"),
            starts: count("starts", 20)?,
            seed: match v.get("seed") {
                None | Some(JsonValue::Null) => 1,
                Some(j) => {
                    crate::seed::seed_from_json(j).ok_or_else(|| invalid("seed: expected a whole number".into()))?
                }
            },
        },
        factors,
        center_points: None,
        randomize,
        units,
        response: v.get("response").and_then(JsonValue::as_str).unwrap_or("Y").to_owned(),
    })
}

/// Settings shared by the script design functions: `randomize` (a seed,
/// default 1, or `false`), `units` (`"actual"` or `"coded"`) and `response`
/// (`"Y"`).
fn output_settings(v: &stat_data::JsonValue) -> Result<(Option<u64>, Units, String)> {
    use stat_data::JsonValue;
    let invalid = |m: &str| StatError::InvalidArgument { message: m.to_owned() };
    let randomize = match v.get("randomize") {
        None | Some(JsonValue::Null) | Some(JsonValue::Bool(true)) => Some(1),
        Some(JsonValue::Bool(false)) => None,
        Some(j) => Some(crate::seed::seed_from_json(j).ok_or_else(|| invalid("randomize: a seed or false"))?),
    };
    let units = match v.get("units").and_then(JsonValue::as_str).unwrap_or("actual") {
        "coded" => Units::Coded,
        "actual" => Units::Actual,
        _ => return Err(invalid("units: use actual or coded")),
    };
    Ok((randomize, units, v.get("response").and_then(JsonValue::as_str).unwrap_or("Y").to_owned()))
}

/// A whole-number setting `key`, or `default` when missing.
fn count_setting(v: &stat_data::JsonValue, key: &str, default: usize) -> Result<usize> {
    match v.get(key) {
        None | Some(stat_data::JsonValue::Null) => Ok(default),
        Some(j) => j
            .as_usize()
            .ok_or_else(|| StatError::InvalidArgument { message: format!("{key}: expected a whole number") }),
    }
}

/// A definitive screening design request from script settings (JSON):
/// `factors` (a count, or `[#{ name, low, high }]`), `extra_runs` (0),
/// `center_runs` (1), and the output settings `randomize`, `units`,
/// `response`.
///
/// # Errors
/// Missing or malformed settings.
pub fn definitive_screening_request_from_json(v: &stat_data::JsonValue) -> Result<DesignRequest> {
    let factors = factors_setting(v, false)?.0;
    let (randomize, units, response) = output_settings(v)?;
    Ok(DesignRequest {
        kind: DesignKind::DefinitiveScreening { extra_runs: count_setting(v, "extra_runs", 0)? },
        factors,
        center_points: Some(count_setting(v, "center_runs", 1)?),
        randomize,
        units,
        response,
    })
}

/// A screening design with categorical factors from script settings (JSON):
/// `factors` (`[#{ name, low, high }]` for continuous factors and
/// `#{ name, levels }` for categorical ones), `runs` (default
/// [`stat_doe::categorical_screening::default_runs`]), `model` (`"main
/// effects"` (default) or `"interactions"`), `starts` (20), `seed` (1), and
/// the output settings `randomize`, `units`, `response`.
///
/// # Errors
/// Missing or malformed settings.
pub fn categorical_screening_request_from_json(v: &stat_data::JsonValue) -> Result<DesignRequest> {
    use stat_data::JsonValue;
    let (factors, levels) = factors_setting(v, true)?;
    let interactions = match v.get("model").and_then(JsonValue::as_str).unwrap_or("main effects") {
        "main effects" | "main-effects" => false,
        "interactions" => true,
        other => {
            return Err(StatError::InvalidArgument {
                message: format!("model: {other:?} (use \"main effects\" or \"interactions\")"),
            });
        }
    };
    let kinds: Vec<ScreeningFactor> = levels
        .iter()
        .map(|l| l.map_or(ScreeningFactor::Continuous, |levels| ScreeningFactor::Categorical { levels }))
        .collect();
    let runs = count_setting(v, "runs", stat_doe::categorical_screening::default_runs(&kinds, interactions))?;
    let seed = match v.get("seed") {
        None | Some(JsonValue::Null) => 1,
        Some(j) => crate::seed::seed_from_json(j)
            .ok_or_else(|| StatError::InvalidArgument { message: "seed: expected a whole number".into() })?,
    };
    let (randomize, units, response) = output_settings(v)?;
    Ok(DesignRequest {
        kind: DesignKind::CategoricalScreening {
            levels,
            runs,
            interactions,
            starts: count_setting(v, "starts", 20)?,
            seed,
        },
        factors,
        center_points: None,
        randomize,
        units,
        response,
    })
}

/// `factors`: a count (factors `A, B, …` at −1/+1) or a list of
/// `#{ name, low, high }` (and, when `levels_allowed`, `#{ name, levels }`).
fn factors_setting(v: &stat_data::JsonValue, levels_allowed: bool) -> Result<(Vec<Factor>, Vec<Option<usize>>)> {
    use stat_data::JsonValue;
    let invalid = |m: String| StatError::InvalidArgument { message: m };
    match v.get("factors") {
        Some(j) if j.as_usize().is_some() => {
            let k = j.as_usize().unwrap_or(0);
            Ok((default_factor_names(k).into_iter().map(Factor::coded).collect(), vec![None; k]))
        }
        Some(JsonValue::Array(list)) => {
            let defaults = default_factor_names(list.len());
            let mut factors = Vec::new();
            let mut levels = Vec::new();
            for (i, f) in list.iter().enumerate() {
                let name = f.get("name").and_then(JsonValue::as_str).map_or_else(|| defaults[i].clone(), str::to_owned);
                let l = match f.get("levels") {
                    None | Some(JsonValue::Null) => None,
                    Some(_) if !levels_allowed => {
                        return Err(invalid(format!("{name}: this design takes continuous factors only")));
                    }
                    Some(j) => {
                        Some(j.as_usize().ok_or_else(|| invalid(format!("{name}: levels must be a whole number")))?)
                    }
                };
                let low = f.get("low").and_then(JsonValue::as_f64).unwrap_or(-1.0);
                let high = f.get("high").and_then(JsonValue::as_f64).unwrap_or(1.0);
                factors.push(Factor::new(name, low, high));
                levels.push(l);
            }
            Ok((factors, levels))
        }
        _ => Err(invalid("give the factors (a count, or a list of #{ name, low, high })".into())),
    }
}

/// Definitive screening designs for `k` factors compared across extra runs
/// and center runs ([`stat_doe::dsd_comparison_choices`] around `chosen`),
/// as a table with one row per choice.
///
/// # Errors
/// As [`stat_doe::compare_definitive_screening`].
pub fn dsd_comparison_table(k: usize, chosen: DsdOptions) -> Result<DataTable> {
    let rows = stat_doe::compare_definitive_screening(k, &stat_doe::dsd_comparison_choices(k, chosen))?;
    let num = |f: &dyn Fn(&stat_doe::DsdSummary) -> f64| rows.iter().map(f).collect::<Vec<f64>>();
    DataTable::new(
        format!("Definitive screening choices, {k} factors"),
        vec![
            Column::numeric("Extra runs", num(&|r| r.extra_runs as f64)),
            Column::numeric("Center runs", num(&|r| r.center_runs as f64)),
            Column::numeric("Runs", num(&|r| r.runs as f64)),
            Column::numeric("Error df (main effects)", num(&|r| r.error_df_main as f64)),
            Column::numeric("Error df (main + quadratic)", num(&|r| r.error_df_quadratic as f64)),
            Column::numeric("D-efficiency (main effects, %)", num(&|r| r.main_d_efficiency)),
            Column::numeric("Max |r| quadratic", num(&|r| r.max_quadratic_correlation)),
            Column::numeric("Max |r| interactions", num(&|r| r.max_interaction_correlation)),
            Column::numeric("Mean SE quadratic (σ)", num(&|r| r.quadratic_se)),
        ],
    )
}

/// Name of the project script that fits a split-plot design's mixed model.
pub const SPLIT_PLOT_SCRIPT: &str = "Fit split-plot model";

/// Create the design as a new document: its table and, for a split-plot
/// design, the design declared with its whole-plot (and subplot) columns and
/// model, and a project script ([`SPLIT_PLOT_SCRIPT`]) that fits the mixed
/// model once the responses are entered. Also returns the design's
/// description.
///
/// # Errors
/// As for [`create_design`].
pub fn design_project(request: &DesignRequest) -> Result<(crate::Project, String)> {
    let (table, design) = create_design(request)?;
    let mut project = crate::Project {
        rows: stat_data::RowStateManager::new(table.n_rows()),
        table,
        reports: Vec::new(),
        origin: None,
        external: None,
        scripts: Vec::new(),
    };
    if let DesignKind::SplitPlot { formula, .. } = &request.kind {
        let t = &project.table;
        let names: Vec<String> = request.factors.iter().map(|f| f.name.clone()).collect();
        let factors: Vec<usize> = names.iter().filter_map(|n| t.find(n)).collect();
        let responses: Vec<usize> = t.find(request.response.trim()).into_iter().collect();
        let mut declared = crate::DesignDeclarationRequest::new(&factors, &responses);
        declared.whole_plot = t.find(WHOLE_PLOT_COLUMN);
        declared.subplot = t.find(SUBPLOT_COLUMN);
        declared.model = ModelFormula::parse(formula, &names).map(|m| m.describe(&names)).unwrap_or_default();
        if let Some(analysis) = crate::declared_design::split_plot_analysis(t, &declared) {
            let call = crate::script_text::pretty(&crate::project::report_to_json(&analysis, t), 4);
            project.scripts.push(crate::ProjectScript {
                name: SPLIT_PLOT_SCRIPT.to_owned(),
                source: format!(
                    "// Fit the split-plot model once the responses are entered: the design's model with\n\
                     // random intercepts for the plots, by REML with Satterthwaite degrees of freedom.\n\
                     let t = current_table();\n\
                     t.analyze({call});\n"
                ),
                run_on_open: false,
            });
        }
        project.reports.push(crate::SavedReport::DeclaredDesign { request: declared });
    }
    // An optimal design built inside constraints opens with its evaluation
    // over that constrained region (the table alone does not record the
    // constraints). Coded tables are left out: the constraints are in actual
    // units.
    if let DesignKind::DOptimal { formula, constraints, .. } | DesignKind::IOptimal { formula, constraints, .. } =
        &request.kind
    {
        if !constraints.trim().is_empty() && request.units == Units::Actual {
            let t = &project.table;
            let names: Vec<String> = request.factors.iter().map(|f| f.name.clone()).collect();
            let factors: Option<Vec<crate::DeclaredFactor>> = request
                .factors
                .iter()
                .map(|f| {
                    t.find(&f.name).map(|column| crate::DeclaredFactor { column, low: Some(f.low), high: Some(f.high) })
                })
                .collect();
            if let Some(factors) = factors {
                let mut evaluation = crate::DesignEvaluationRequest::new(&[]);
                evaluation.factors = factors;
                evaluation.model = ModelFormula::parse(formula, &names).map(|m| m.describe(&names))?;
                evaluation.constraints = constraints.trim().to_owned();
                project.reports.push(crate::SavedReport::DesignEvaluation { request: evaluation });
            }
        }
    }
    Ok((project, design.description))
}

/// The kind of mixture design.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MixtureKind {
    /// Simplex-lattice `{q, m}`: every blend in steps of `1/m`.
    Lattice {
        /// Lattice degree `m` (1 to 6).
        degree: usize,
        /// Add the overall centroid if it is not on the lattice.
        centroid: bool,
    },
    /// Simplex-centroid: equal parts of every non-empty subset of components.
    Centroid,
    /// Extreme vertices of the region cut out by lower and upper bounds on
    /// the components ([`MixtureDesignRequest::lower`] and `upper`), with
    /// optional face centroids (McLean & Anderson 1966).
    ExtremeVertices {
        /// Centroids of faces up to this dimension: 0 none, 1 edges, 2 edges
        /// and two-dimensional faces.
        centroids: usize,
        /// Add the overall centroid (the mean of the vertices).
        overall: bool,
    },
}

/// Everything needed to create a mixture design table.
#[derive(Debug, Clone, PartialEq)]
pub struct MixtureDesignRequest {
    /// Design type.
    pub kind: MixtureKind,
    /// Component names (their number is `q`).
    pub components: Vec<String>,
    /// Add axial check blends (interior points for testing the model fit).
    pub axial: bool,
    /// Times the whole set of blends is run (≥ 1); replicates give a
    /// pure-error estimate.
    pub replicates: usize,
    /// What the components add up to in the table: 1 for proportions, 100 for
    /// percentages, or a batch amount.
    pub total: f64,
    /// Randomize the run order with this seed.
    pub randomize: Option<u64>,
    /// Name of the empty response column (none if blank).
    pub response: String,
    /// Lower bound of each component, in the units of `total` (used by
    /// [`MixtureKind::ExtremeVertices`]; empty means 0 for every component).
    pub lower: Vec<f64>,
    /// Upper bound of each component, in the units of `total` (empty means
    /// `total` for every component).
    pub upper: Vec<f64>,
}

impl MixtureDesignRequest {
    /// A request for `kind` with `q` components named `A, B, …` as proportions.
    #[must_use]
    pub fn new(kind: MixtureKind, q: usize) -> Self {
        Self {
            kind,
            components: default_factor_names(q),
            axial: false,
            replicates: 1,
            total: 1.0,
            randomize: Some(1),
            response: "Y".to_owned(),
            lower: Vec::new(),
            upper: Vec::new(),
        }
    }
}

/// Most replicates of a mixture design.
pub const MAX_MIXTURE_REPLICATES: usize = 10;

/// Build the mixture design described by `request` (blends in proportions,
/// replicated, in standard order).
///
/// # Errors
/// Errors from the generator (2 to 12 components, lattice degree 1 to 6),
/// blank or repeated component names, 0 or too many replicates, or a total
/// that is not positive.
pub fn build_mixture_design(request: &MixtureDesignRequest) -> Result<MixtureDesign> {
    let invalid = |m: &str| StatError::InvalidArgument { message: m.to_owned() };
    let q = request.components.len();
    let names: Vec<String> = request.components.iter().map(|c| c.trim().to_owned()).collect();
    if names.iter().any(String::is_empty) {
        return Err(invalid("every component needs a name"));
    }
    let mut sorted = names.clone();
    sorted.sort();
    if sorted.windows(2).any(|w| w[0] == w[1]) {
        return Err(invalid("component names must be different"));
    }
    if !(1..=MAX_MIXTURE_REPLICATES).contains(&request.replicates) {
        return Err(invalid("use 1 to 10 replicates"));
    }
    if !(request.total.is_finite() && request.total > 0.0) {
        return Err(invalid("the components' total must be a positive number"));
    }
    let mut d = match request.kind {
        MixtureKind::Lattice { degree, centroid } => simplex_lattice(q, degree, centroid, request.axial)?,
        MixtureKind::Centroid => simplex_centroid(q, request.axial)?,
        MixtureKind::ExtremeVertices { centroids, overall } => {
            let bound = |given: &[f64], default: f64| -> Result<Vec<f64>> {
                match given.len() {
                    0 => Ok(vec![default; q]),
                    n if n == q => Ok(given.iter().map(|b| b / request.total).collect()),
                    _ => Err(invalid("give a lower and an upper bound for every component")),
                }
            };
            let mut d =
                extreme_vertices(&bound(&request.lower, 0.0)?, &bound(&request.upper, 1.0)?, centroids, overall)?;
            if request.axial {
                // Check blends half-way between the overall centroid and each vertex.
                let n = d.runs.iter().zip(&d.blend_types).filter(|(_, t)| **t == stat_doe::BlendType::Vertex).count();
                let centre: Vec<f64> =
                    (0..q).map(|j| d.runs[..n].iter().map(|r| r[j]).sum::<f64>() / n as f64).collect();
                for i in 0..n {
                    let p = (0..q).map(|j| (d.runs[i][j] + centre[j]) / 2.0).collect();
                    d.runs.push(p);
                    d.blend_types.push(stat_doe::BlendType::Axial);
                }
                d.description.push_str(" with axial check blends");
            }
            d
        }
    };
    d.components = names;
    if request.replicates > 1 {
        let (runs, types) = (d.runs.clone(), d.blend_types.clone());
        for _ in 1..request.replicates {
            d.runs.extend(runs.iter().cloned());
            d.blend_types.extend(types.iter().copied());
        }
        d.description = format!("{}, {} replicates", d.description, request.replicates);
    }
    Ok(d)
}

/// Whether a Scheffé model can be fitted to a design's blends, with the
/// degrees of freedom left for error.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelSupport {
    /// The model.
    pub model: ScheffeModel,
    /// Number of terms.
    pub terms: usize,
    /// `true` if every term can be estimated from the blends.
    pub estimable: bool,
    /// Runs minus terms (when estimable).
    pub error_df: usize,
}

/// Which Scheffé models `design` supports (linear, quadratic, special cubic).
#[must_use]
pub fn mixture_model_support(design: &MixtureDesign) -> Vec<ModelSupport> {
    [ScheffeModel::Linear, ScheffeModel::Quadratic, ScheffeModel::SpecialCubic]
        .into_iter()
        .map(|model| {
            let rows: Vec<Vec<f64>> = design.runs.iter().map(|r| model.expand(r)).collect();
            let p = rows.first().map_or(0, Vec::len);
            // XᵀX is non-singular exactly when every term is estimable.
            let xtx: Vec<Vec<f64>> =
                (0..p).map(|a| (0..p).map(|b| rows.iter().map(|r| r[a] * r[b]).sum()).collect()).collect();
            let (values, _) = symmetric_eigen(&xtx);
            let largest = values.iter().copied().fold(0.0_f64, f64::max);
            let smallest = values.iter().copied().fold(f64::INFINITY, f64::min);
            let estimable = p > 0 && rows.len() >= p && smallest > 1e-10 * largest.max(f64::MIN_POSITIVE);
            ModelSupport { model, terms: p, estimable, error_df: if estimable { rows.len() - p } else { 0 } }
        })
        .collect()
}

/// Build the mixture design and export it as a data table: a run number, the
/// standard order (when randomized), one column per component scaled to the
/// requested total, a nominal "Blend type" column and the empty response.
///
/// # Errors
/// As for [`build_mixture_design`], or if the response name clashes with a
/// component.
pub fn create_mixture_design(request: &MixtureDesignRequest) -> Result<(DataTable, MixtureDesign)> {
    let design = build_mixture_design(request)?;
    let response = request.response.trim();
    if design.components.iter().any(|c| c == response) {
        return Err(StatError::InvalidArgument { message: format!("the response {response:?} is also a component") });
    }
    let n = design.n_runs();
    let mut order: Vec<usize> = (0..n).collect();
    if let Some(seed) = request.randomize {
        Rng::new(seed).shuffle(&mut order);
    }
    let mut columns = vec![Column::numeric("Run", (1..=n).map(|i| i as f64).collect())];
    if request.randomize.is_some() {
        let mut c = Column::numeric("Std order", order.iter().map(|&i| (i + 1) as f64).collect());
        c.level = MeasurementLevel::Ordinal;
        columns.push(c);
    }
    for (j, name) in design.components.iter().enumerate() {
        // Rounded to 12 significant places so 1/3 × 100 reads cleanly.
        let scale = |v: f64| {
            let x = v * request.total;
            let digits = 12 - x.abs().max(1e-300).log10().ceil() as i32;
            let f = 10f64.powi(digits.clamp(0, 15));
            (x * f).round() / f
        };
        columns.push(Column::numeric(name.clone(), order.iter().map(|&i| scale(design.runs[i][j])).collect()));
    }
    columns.push(Column::text("Blend type", order.iter().map(|&i| design.blend_types[i].label().to_owned()).collect()));
    if !response.is_empty() {
        columns.push(Column::numeric(response, vec![f64::NAN; n]));
    }
    let table = DataTable::new(&design.description, columns)?;
    Ok((table, design))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screening_extras() {
        // Extra runs and center runs of a definitive screening design.
        let mut req = DesignRequest::new(DesignKind::DefinitiveScreening { extra_runs: 4 }, 6);
        req.center_points = Some(3);
        let (t, d) = create_design(&req).unwrap();
        assert_eq!((t.n_rows(), d.n_runs()), (19, 19));
        let json = stat_data::parse_json(r#"{"factors": 6, "extra_runs": 4, "center_runs": 3}"#).unwrap();
        assert_eq!(definitive_screening_request_from_json(&json).unwrap(), req);
        let cmp = dsd_comparison_table(6, DsdOptions { extra_runs: 4, center_runs: 3 }).unwrap();
        assert_eq!(cmp.n_rows(), 6);
        assert_eq!(cmp.column(2).unwrap().as_numeric().unwrap(), &[13.0, 15.0, 17.0, 19.0, 21.0, 23.0]);

        // Categorical factors become text columns of level labels.
        let json = stat_data::parse_json(
            r#"{"factors": [{"name": "Temp", "low": 150, "high": 190}, {"name": "Supplier", "levels": 3},
                            {"name": "Line", "levels": 2}, {"name": "Speed", "low": 1, "high": 3}],
                "starts": 10, "randomize": 5}"#,
        )
        .unwrap();
        let req = categorical_screening_request_from_json(&json).unwrap();
        assert!(matches!(&req.kind, DesignKind::CategoricalScreening { runs: 12, interactions: false, .. }));
        let (t, d) = create_design(&req).unwrap();
        assert_eq!(t.n_rows(), 12);
        let supplier = t.column(t.find("Supplier").unwrap()).unwrap();
        assert_eq!(supplier.level, MeasurementLevel::Nominal);
        let mut labels: Vec<String> = (0..12).map(|r| supplier.cell_text(r)).collect();
        labels.sort();
        labels.dedup();
        assert_eq!(labels, ["L1", "L2", "L3"]);
        let temp = t.column(t.find("Temp").unwrap()).unwrap().as_numeric().unwrap();
        assert!(temp.iter().all(|v| *v == 150.0 || *v == 190.0));
        assert!(d.notes.iter().any(|n| n.starts_with("Supplier: levels L1 to L3 run 4, 4, 4 times")), "{:?}", d.notes);
        let mut coded = req.clone();
        coded.units = Units::Coded;
        let (tc, _) = create_design(&coded).unwrap();
        assert_eq!(
            (0..12).map(|r| tc.column(tc.find("Supplier").unwrap()).unwrap().cell_text(r)).collect::<Vec<_>>(),
            (0..12).map(|r| supplier.cell_text(r)).collect::<Vec<_>>()
        );
        let bad = stat_data::parse_json(r#"{"factors": [{"name": "A", "levels": 3}], "model": "cubic"}"#).unwrap();
        assert!(categorical_screening_request_from_json(&bad).is_err());
        let bad = stat_data::parse_json(r#"{"factors": [{"name": "A", "levels": 3}]}"#).unwrap();
        assert!(definitive_screening_request_from_json(&bad).is_err());
    }

    #[test]
    fn every_kind_builds_a_table() {
        let kinds = [
            (DesignKind::TwoLevelFactorial, 3, 8),
            (DesignKind::ThreeLevelFactorial, 2, 9),
            (DesignKind::FractionalFactorial { p: 1 }, 4, 8),
            (DesignKind::PlackettBurman, 7, 8),
            (DesignKind::DefinitiveScreening { extra_runs: 0 }, 6, 13),
            (DesignKind::CentralComposite { alpha: AxialDistance::FaceCentered, inscribed: false }, 3, 17),
            (DesignKind::BoxBehnken, 3, 15),
            (
                DesignKind::DOptimal {
                    formula: "A + B + A*B".into(),
                    runs: 6,
                    starts: 5,
                    seed: 3,
                    constraints: String::new(),
                },
                2,
                6,
            ),
            (
                DesignKind::IOptimal {
                    formula: "A + B + A*B + A^2 + B^2".into(),
                    runs: 9,
                    starts: 3,
                    seed: 2,
                    constraints: String::new(),
                },
                2,
                9,
            ),
        ];
        for (kind, k, n) in kinds {
            let req = DesignRequest::new(kind.clone(), k);
            let (t, d) = create_design(&req).unwrap();
            assert_eq!(t.n_rows(), n, "{kind:?}");
            assert_eq!(d.n_runs(), n);
            assert_eq!(t.column(1).unwrap().name, "Std order", "randomized by default");
            assert!(t.find("Y").is_some());
        }
    }

    /// A split-plot request opens with the plot columns, the declaration
    /// and the script that fits its model; the script settings parse into
    /// the same request.
    #[test]
    fn split_plot_design_project() {
        let json = stat_data::parse_json(
            r#"{"factors": [{"name": "Temp", "low": 150, "high": 190, "changes": "hard"},
                            {"name": "Time", "low": 10, "high": 30}],
                "model": "Temp + Time + Temp*Time", "runs": 8, "whole_plots": 4, "starts": 5,
                "response": "Yield"}"#,
        )
        .unwrap();
        let req = split_plot_request_from_json(&json).unwrap();
        let (project, description) = design_project(&req).unwrap();
        assert!(description.starts_with("Split-plot D-optimal, 2 factors, 4-term model (8 runs in 4 whole plots)"));
        let t = &project.table;
        let names: Vec<&str> = t.columns().iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["Run", "Std order", "Whole Plot", "Temp", "Time", "Yield"]);
        let (wp, temp) = (t.column(2).unwrap().as_numeric().unwrap(), t.column(3).unwrap().as_numeric().unwrap());
        for r in 0..8 {
            assert!((0..8).filter(|&q| wp[q] == wp[r]).all(|q| temp[q] == temp[r]));
            assert!(r == 0 || wp[r] >= wp[r - 1], "whole plots run one after another");
        }
        let crate::SavedReport::DeclaredDesign { request } = &project.reports[0] else { panic!() };
        assert_eq!((request.whole_plot, request.subplot), (Some(2), None));
        assert_eq!(request.model, "Temp + Time + Temp × Time");
        assert_eq!(project.scripts[0].name, SPLIT_PLOT_SCRIPT);
        assert!(project.scripts[0].source.contains("random"), "{}", project.scripts[0].source);
        let d = build_split_plot(&req).unwrap();
        assert!(d.design.notes.iter().any(|n| n == "Hard to change (fixed within each whole plot): Temp."));
        assert!(d.design.notes.iter().any(|n| n == "Model: Temp + Time + Temp × Time"));
        // Errors: a factor called "Whole Plot", an unknown change level.
        let mut bad = req.clone();
        bad.factors[1].name = "Whole Plot".into();
        assert!(create_design(&bad).is_err());
        let json = stat_data::parse_json(r#"{"factors": [{"changes": "soft"}], "runs": 4}"#).unwrap();
        assert!(split_plot_request_from_json(&json).is_err());
        // Ordinary designs open without reports or scripts.
        let (plain, _) = design_project(&DesignRequest::new(DesignKind::TwoLevelFactorial, 2)).unwrap();
        assert!(plain.reports.is_empty() && plain.scripts.is_empty());
    }

    #[test]
    fn factor_settings_names_and_center_points() {
        let mut req = DesignRequest::new(DesignKind::TwoLevelFactorial, 2);
        req.factors = vec![Factor::new("Temp", 150.0, 190.0), Factor::new("Time", 10.0, 30.0)];
        req.center_points = Some(3);
        req.randomize = None;
        req.response = "Yield".into();
        let (t, d) = create_design(&req).unwrap();
        assert_eq!(t.n_rows(), 7);
        assert!(d.description.contains("+ 3 center runs"));
        assert_eq!(t.column(1).unwrap().name, "Temp");
        assert_eq!(t.column(1).unwrap().as_numeric().unwrap()[..2], [150.0, 190.0]);
        assert_eq!(t.column(2).unwrap().as_numeric().unwrap()[6], 20.0, "center in actual units");
        assert_eq!(t.column(3).unwrap().name, "Point type");
        assert_eq!(t.column(4).unwrap().name, "Yield");
        req.response = "Temp".into();
        assert!(create_design(&req).is_err(), "response clashes with a factor");
    }

    #[test]
    fn d_optimal_uses_factor_names_in_formulas() {
        let mut req = DesignRequest::new(
            DesignKind::DOptimal {
                formula: "Temp + Time + Temp*Time + Temp^2".into(),
                runs: 8,
                starts: 5,
                seed: 1,
                constraints: String::new(),
            },
            2,
        );
        req.factors = vec![Factor::new("Temp", 0.0, 100.0), Factor::new("Time", 1.0, 5.0)];
        let (_, d) = create_design(&req).unwrap();
        assert_eq!(d.notes[0], "Model: Temp + Time + Temp × Time + Temp²");
        req.kind =
            DesignKind::DOptimal { formula: "A + B".into(), runs: 8, starts: 5, seed: 1, constraints: String::new() };
        assert!(build_design(&req).is_err(), "A and B are not factor names here");
    }

    /// Constraints in actual units: every run satisfies them, and they are
    /// listed in the design notes; errors name the line.
    #[test]
    fn optimal_designs_honour_constraints() {
        let constraints = "Temp + 4*Time <= 300\nexclude Temp < 160 and Time < 15".to_owned();
        let mut req = DesignRequest::new(
            DesignKind::DOptimal {
                formula: "Temp + Time + Temp*Time".into(),
                runs: 8,
                starts: 10,
                seed: 1,
                constraints: constraints.clone(),
            },
            2,
        );
        req.factors = vec![Factor::new("Temp", 140.0, 200.0), Factor::new("Time", 10.0, 30.0)];
        req.randomize = None;
        let (t, d) = create_design(&req).unwrap();
        let temp = t.column(t.find("Temp").unwrap()).unwrap().as_numeric().unwrap();
        let time = t.column(t.find("Time").unwrap()).unwrap().as_numeric().unwrap();
        for (a, b) in temp.iter().zip(time) {
            assert!(a + 4.0 * b <= 300.0 + 1e-6, "({a}, {b})");
            assert!(!(*a < 160.0 - 1e-6 && *b < 15.0 - 1e-6), "({a}, {b}) is disallowed");
        }
        assert!(temp.iter().zip(time).any(|(a, b)| (a + 4.0 * b - 300.0).abs() < 1e-6), "uses the boundary");
        assert!(
            d.notes.iter().any(|n| n
                == "Constraints (every run satisfies them): Temp + 4*Time <= 300; Exclude Temp < 160 and Time < 15."),
            "{:?}",
            d.notes
        );
        req.kind = DesignKind::IOptimal {
            formula: "Temp + Time + Temp*Time + Temp^2 + Time^2".into(),
            runs: 9,
            starts: 3,
            seed: 1,
            constraints,
        };
        let (_, d) = create_design(&req).unwrap();
        assert!(d.notes.iter().any(|n| n.contains("over the constrained region")), "{:?}", d.notes);
        req.kind = DesignKind::DOptimal {
            formula: "Temp + Time".into(),
            runs: 4,
            starts: 2,
            seed: 1,
            constraints: "Temp <= 150\nPressure > 2".into(),
        };
        let err = build_design(&req).unwrap_err().to_string();
        assert!(err.contains("constraint line 2: unknown factor \"Pressure\""), "{err}");
    }

    /// A constrained optimal design opens with its evaluation over the
    /// constrained region, set up with the design's factors, model and
    /// constraints.
    #[test]
    fn constrained_design_project_evaluates_over_its_region() {
        let constraints = "Temp + 4*Time <= 300\nexclude Temp < 160 and Time < 15".to_owned();
        let mut req = DesignRequest::new(
            DesignKind::IOptimal {
                formula: "Temp + Time + Temp*Time + Temp^2 + Time^2".into(),
                runs: 9,
                starts: 3,
                seed: 1,
                constraints: constraints.clone(),
            },
            2,
        );
        req.factors = vec![Factor::new("Temp", 140.0, 200.0), Factor::new("Time", 10.0, 30.0)];
        let (project, _) = design_project(&req).unwrap();
        let [crate::SavedReport::DesignEvaluation { request }] = project.reports.as_slice() else {
            panic!("{:?}", project.reports)
        };
        assert_eq!(request.constraints, constraints);
        assert_eq!(request.model, "Temp + Time + Temp × Time + Temp² + Time²");
        assert_eq!((request.factors[0].low, request.factors[0].high), (Some(140.0), Some(200.0)));
        let a = crate::DesignEvaluationAnalysis::run(
            &project.table,
            &stat_data::RowStateManager::new(project.table.n_rows()),
            request,
        )
        .unwrap();
        let e = a.evaluations().next().unwrap();
        // The largest variance is found inside the region (actual units).
        let (temp, time) = (170.0 + 30.0 * e.max_at[0], 20.0 + 10.0 * e.max_at[1]);
        assert!(temp + 4.0 * time <= 300.0 + 1e-6 && !(temp < 160.0 && time < 15.0), "({temp}, {time})");
        assert!(e.region_points.is_some());
        // Coded tables and unconstrained designs get no report.
        req.units = Units::Coded;
        assert!(design_project(&req).unwrap().0.reports.is_empty());
        req.units = Units::Actual;
        req.kind = DesignKind::DOptimal {
            formula: "Temp + Time".into(),
            runs: 4,
            starts: 2,
            seed: 1,
            constraints: String::new(),
        };
        assert!(design_project(&req).unwrap().0.reports.is_empty());
    }

    #[test]
    fn i_optimal_reports_prediction_variance() {
        let req = DesignRequest::new(
            DesignKind::IOptimal {
                formula: "A + B + A*B + A^2 + B^2".into(),
                runs: 10,
                starts: 4,
                seed: 1,
                constraints: String::new(),
            },
            2,
        );
        let (_, d) = create_design(&req).unwrap();
        assert!(d.description.starts_with("I-optimal"), "{}", d.description);
        assert!(d.notes.iter().any(|n| n.contains("Average prediction variance")), "{:?}", d.notes);
    }

    #[test]
    fn mixture_designs_become_tables() {
        // Simplex-lattice {3, 2}, two replicates, as percentages.
        let mut req = MixtureDesignRequest::new(MixtureKind::Lattice { degree: 2, centroid: false }, 3);
        req.components = vec!["Polyethylene".into(), "Polystyrene".into(), "Polypropylene".into()];
        req.replicates = 2;
        req.total = 100.0;
        req.randomize = None;
        req.response = "Elongation".into();
        let (t, d) = create_mixture_design(&req).unwrap();
        assert_eq!(d.n_runs(), 12);
        assert!(d.description.contains("2 replicates"));
        let names: Vec<&str> = (0..t.n_cols()).map(|j| t.column(j).unwrap().name.as_str()).collect();
        assert_eq!(names, ["Run", "Polyethylene", "Polystyrene", "Polypropylene", "Blend type", "Elongation"]);
        for r in 0..12 {
            let total: f64 = (1..=3).map(|j| t.column(j).unwrap().as_numeric().unwrap()[r]).sum();
            assert!((total - 100.0).abs() < 1e-9);
        }
        assert_eq!(t.column(4).unwrap().cell_text(0), "Pure blend");
        // Supports up to the quadratic model, with 6 error df from the replicates.
        let support = mixture_model_support(&d);
        assert!(support[0].estimable && support[1].estimable && !support[2].estimable);
        assert_eq!((support[1].terms, support[1].error_df), (6, 6));
        // Simplex centroid with axial blends, randomized, 4 components.
        let mut req = MixtureDesignRequest::new(MixtureKind::Centroid, 4);
        req.axial = true;
        let (t, d) = create_mixture_design(&req).unwrap();
        assert_eq!(d.n_runs(), 15 + 4);
        assert_eq!(t.column(1).unwrap().name, "Std order");
        assert!(mixture_model_support(&d).iter().all(|s| s.estimable), "centroid supports the special cubic");
        // Thirds read cleanly as percentages.
        let mut req = MixtureDesignRequest::new(MixtureKind::Lattice { degree: 3, centroid: false }, 3);
        req.total = 100.0;
        req.randomize = None;
        let (t, _) = create_mixture_design(&req).unwrap();
        let third = (100.0_f64 / 3.0 * 1e10).round() / 1e10;
        assert!(t.column(1).unwrap().as_numeric().unwrap().contains(&third));
        // Errors.
        let bad = |f: &dyn Fn(&mut MixtureDesignRequest)| {
            let mut r = MixtureDesignRequest::new(MixtureKind::Centroid, 3);
            f(&mut r);
            create_mixture_design(&r).is_err()
        };
        assert!(bad(&|r| r.components = vec!["A".into()]));
        assert!(bad(&|r| r.components[1] = "A".into()));
        assert!(bad(&|r| r.components[2] = " ".into()));
        assert!(bad(&|r| r.replicates = 0));
        assert!(bad(&|r| r.total = 0.0));
        assert!(bad(&|r| r.response = "B".into()));
        assert!(bad(&|r| r.kind = MixtureKind::Lattice { degree: 7, centroid: false }));
    }

    #[test]
    fn mixed_level_factorial_spreads_levels_over_the_range() {
        let mut req = DesignRequest::new(DesignKind::GeneralFactorial { levels: vec![2, 3, 4] }, 3);
        req.factors = vec![Factor::new("Supplier", 1.0, 2.0), Factor::new("Temp", 100.0, 200.0), Factor::coded("C")];
        req.randomize = None;
        let (t, d) = create_design(&req).unwrap();
        assert_eq!(t.n_rows(), 24);
        assert_eq!(d.description, "2×3×4 full factorial (24 runs)");
        let temp = t.column(t.find("Temp").unwrap()).unwrap().as_numeric().unwrap();
        let mut levels: Vec<f64> = temp.to_vec();
        levels.sort_by(f64::total_cmp);
        levels.dedup();
        assert_eq!(levels, [100.0, 150.0, 200.0]);
        let c = t.column(t.find("C").unwrap()).unwrap().as_numeric().unwrap();
        assert!(c.iter().any(|v| (v - (-1.0 / 3.0)).abs() < 1e-12), "four levels −1, −1/3, 1/3, 1");
        assert!(t.find("Point type").is_none(), "a single point type");
        req.kind = DesignKind::GeneralFactorial { levels: vec![2, 3] };
        assert!(create_design(&req).is_err(), "one level count per factor");
        req.kind = DesignKind::GeneralFactorial { levels: vec![2, 1, 2] };
        assert!(create_design(&req).is_err());
    }

    /// The flare example of McLean & Anderson (1966) in percentages.
    #[test]
    fn extreme_vertices_design_in_percent() {
        let mut req = MixtureDesignRequest::new(MixtureKind::ExtremeVertices { centroids: 2, overall: true }, 4);
        req.total = 100.0;
        req.lower = vec![40.0, 10.0, 10.0, 3.0];
        req.upper = vec![60.0, 50.0, 50.0, 8.0];
        req.randomize = None;
        let (t, d) = create_mixture_design(&req).unwrap();
        assert_eq!(d.n_runs(), 8 + 12 + 6 + 1);
        assert_eq!(t.column(5).unwrap().cell_text(0), "Vertex");
        assert_eq!(t.column(5).unwrap().cell_text(26), "Overall centroid");
        let a = t.column(1).unwrap().as_numeric().unwrap();
        assert!(a.iter().all(|&v| (40.0 - 1e-9..=60.0 + 1e-9).contains(&v)));
        assert!((t.column(2).unwrap().as_numeric().unwrap()[26] - 22.25).abs() < 1e-9);
        // Axial check blends: one per vertex.
        req.axial = true;
        assert_eq!(create_mixture_design(&req).unwrap().1.n_runs(), 27 + 8);
        req.upper = vec![60.0, 50.0];
        assert!(create_mixture_design(&req).is_err());
        req.upper = vec![45.0, 20.0, 20.0, 8.0];
        assert!(create_mixture_design(&req).is_err(), "the upper bounds sum to less than 100");
    }

    #[test]
    fn space_filling_designs_keep_factor_ranges() {
        for criterion in SpaceFillingKind::ALL {
            let mut req = DesignRequest::new(DesignKind::SpaceFilling { criterion, runs: 15, seed: 4 }, 3);
            req.factors =
                vec![Factor::new("Rate", 50.0, 150.0), Factor::new("Temp", 300.0, 400.0), Factor::new("Gap", 0.1, 0.2)];
            req.center_points = Some(3);
            let (t, d) = create_design(&req).unwrap();
            assert_eq!(t.n_rows(), 15, "no centre points: {criterion:?}");
            assert_eq!(t.name(), criterion.label());
            assert!(t.find("Point type").is_none());
            let rate = t.column(t.find("Rate").unwrap()).unwrap().as_numeric().unwrap();
            assert!(rate.iter().all(|v| (50.0..=150.0).contains(v)));
            assert!(d.notes.iter().any(|n| n.starts_with("Minimum distance")), "{:?}", d.notes);
        }
        let req =
            DesignRequest::new(DesignKind::SpaceFilling { criterion: SpaceFillingKind::Uniform, runs: 1, seed: 1 }, 2);
        assert!(create_design(&req).is_err());
    }

    #[test]
    fn errors_are_reported() {
        assert!(build_design(&DesignRequest::new(DesignKind::BoxBehnken, 2)).is_err());
        assert!(build_design(&DesignRequest::new(DesignKind::FractionalFactorial { p: 3 }, 4)).is_err());
        assert!(build_design(&DesignRequest::new(DesignKind::TwoLevelFactorial, 0)).is_err());
    }
}
