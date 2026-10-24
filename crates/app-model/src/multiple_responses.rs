//! Several responses modelled on the same factors, explored and optimized
//! together: linked Response Explorer rows, desirability, a search for the
//! best settings, and simulation of factor variation and response noise.
//!
//! Each response gets an ordinary least-squares model of the same form
//! (main effects, plus two-factor interactions, plus squares of continuous
//! factors) on the same predictors, fitted to the rows where that response
//! and the predictors are present and not excluded.
//!
//! * **Term selection** ([`MultipleResponsesRequest::selection`]): instead
//!   of fitting every term, each response's terms are chosen by its own
//!   stepwise search ([`crate::stepwise_analysis`], Efroymson 1960; Draper &
//!   Smith 1998, ch. 15) among the same candidates (the model form's terms,
//!   or a list chosen in the stepwise dialog), with the same direction,
//!   criterion and heredity. Every selected model keeps all the factors, so
//!   the linked explorer, desirability, best settings and simulation work
//!   on the selected models unchanged; a factor no selected term of a
//!   response uses is flat in that response's row. A response for which no
//!   term is selected is predicted by its mean (with the interval
//!   `ȳ ± t₀.₉₇₅,ₙ₋₁ s/√n`), constant in every factor.
//!
//! * **Linked profiles** ([`MultipleResponsesAnalysis::profiles`]): one
//!   Response Explorer row per response over the same factor settings, the
//!   factors' ranges being the union over the models; then a row of the
//!   overall desirability `D` as each factor moves.
//! * **Desirability** (Derringer & Suich 1980; [`stat_doe::desirability`]):
//!   each response maximized, minimized or held near a target between
//!   limits; `D` is the importance-weighted geometric mean. By default each
//!   response is maximized between its smallest and largest observed value.
//! * **Best settings** ([`MultipleResponsesAnalysis::best_settings`]):
//!   maximize `D` over the factor box by Nelder–Mead from a Latin hypercube
//!   of starts plus the current settings, for every combination of
//!   categorical levels (up to 64 combinations; beyond that the categorical
//!   factors stay at their current levels). Where `D = 0` the search climbs
//!   the mean of the individual desirabilities instead, so it can leave a
//!   region where some response is unacceptable.
//! * **Simulation** ([`MultipleResponsesAnalysis::simulate`]): each
//!   continuous factor varies around its setting (fixed, normal with a given
//!   SD, uniform or triangular over ± a half-width); each response gets
//!   normal noise with a given SD; `runs` runs are drawn from a seeded
//!   generator. Reported: each response's simulated mean and SD, and the
//!   share of runs outside its specification limits, with a 95% Wilson
//!   score interval; overall, the share of runs with any response outside.
//!   [`MultipleResponsesAnalysis::robust_settings`] searches the factor box
//!   for the settings with the lowest overall defect rate, using the same
//!   random draws at every candidate (common random numbers).
//! * **Constraints** ([`MultipleResponsesRequest::constraints`]): linear
//!   limits and disallowed combinations on the continuous factors, written
//!   in their units as for designs ([`stat_doe::Region::parse`]). The
//!   best-settings and robust-settings searches only return settings that
//!   satisfy them: a violated constraint adds a penalty of 2 plus the
//!   violation (in coded units) to the objective, which lies in [−1, 1]
//!   otherwise, and infeasible end points are dropped.
//! * **Specification region** ([`SimulationSetup::region`]): a box of
//!   continuous-factor settings (an operating window). At every corner the
//!   probability that all responses meet their specification limits is
//!   `Πᵢ [Φ((Uᵢ − ŷᵢ)/σᵢ) − Φ((Lᵢ − ŷᵢ)/σᵢ)]` with the noise SDs `σᵢ`
//!   (responses independent; a missing limit is ±∞; with `σᵢ = 0` the factor
//!   is 1 inside the limits and 0 outside), checked by simulating the noise
//!   (the same draws at every corner); the worst corner is the region's
//!   guaranteed probability (in the spirit of Peterson 2004, "A posterior
//!   predictive approach to multiple response surface optimization",
//!   *J. Quality Technology* 36(2), and del Castillo 2007, *Process
//!   Optimization: A Statistical Approach*, ch. 7, with the parameters taken
//!   as known). A contour plot over the first two varying factors shows each
//!   response's limits with the box drawn on it.

use stat_core::distributions::{ContinuousDistribution, Normal};
use stat_core::optimize::{latin_hypercube, minimize_in_box};
use stat_core::rng::Rng;
use stat_core::{Result, StatError};
use stat_data::{Column, DataTable, JsonValue, MeasurementLevel, RowStateManager};
use stat_doe::{Desirability, Factor, Goal, Region, overall_desirability};
use stat_linear::{Heredity, ModelDesign, StepCriterion, StepDirection, TermValue};

use crate::format::format_number;
use crate::linear_analysis::{Coded, LinearModelAnalysis, LinearModelSpec, Profile, ProfilePanel};
use crate::report::{
    Annotations, BarChartSpec, BarItem, PlotSpec, Polyline, ReportSection, SectionContent, SectionKind, TextTable,
};
use crate::stepwise_analysis::{StepwiseAnalysis, StepwiseRequest, search_description};
use crate::surface_profiler::{CURVE_POINTS, Limited, Observation, ProfileFactor, ProfileFactorKind, SurfaceModel};

const DIGITS: usize = 6;
/// Most categorical level combinations the best-settings search tries.
const MAX_COMBINATIONS: usize = 64;
/// Solutions listed by the best-settings search.
const TOP_SOLUTIONS: usize = 5;
/// Most runs a simulation may draw.
pub const MAX_RUNS: usize = 1_000_000;
/// Most continuous factors that may vary over a specification region
/// (`2^8 = 256` corners).
pub const MAX_REGION_FACTORS: usize = 8;
/// Most simulated runs per corner of a specification region.
const REGION_RUNS: usize = 5_000;
/// Grid of the specification-limit contour plot.
const CONTOUR_GRID: usize = 60;

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// The terms every response's model contains.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelForm {
    /// Main effects only.
    MainEffects,
    /// Main effects and all two-factor interactions.
    Interactions,
    /// Main effects, two-factor interactions and squares of continuous
    /// factors (a full second-order response surface).
    Quadratic,
}

impl ModelForm {
    /// Every form, in menu order.
    pub const ALL: [Self; 3] = [Self::MainEffects, Self::Interactions, Self::Quadratic];

    /// Identifier in project files and scripts.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::MainEffects => "main-effects",
            Self::Interactions => "interactions",
            Self::Quadratic => "quadratic",
        }
    }

    /// The form with identifier `key`.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.key() == key)
    }

    /// Menu label.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::MainEffects => "Main effects",
            Self::Interactions => "Main effects and two-factor interactions",
            Self::Quadratic => "Full quadratic (interactions and squares)",
        }
    }
}

/// Stepwise selection of each response's terms (see the module
/// documentation).
#[derive(Debug, Clone, PartialEq)]
pub struct TermSelection {
    /// Search direction.
    pub direction: StepDirection,
    /// Selection criterion.
    pub criterion: StepCriterion,
    /// Parent effects an interaction or power needs in the model; missing
    /// parents are added to the candidates.
    pub heredity: Heredity,
    /// Candidate effects as `(position in the predictors, power)` products;
    /// empty: the terms of the request's model form.
    pub candidates: Vec<Vec<(usize, u8)>>,
}

impl Default for TermSelection {
    /// Mixed search by p-values (enter below 0.15, remove above 0.15) with
    /// strong heredity, among the model form's terms.
    fn default() -> Self {
        Self {
            direction: StepDirection::Mixed,
            criterion: StepCriterion::PValue { enter: 0.15, remove: 0.15 },
            heredity: Heredity::Strong,
            candidates: Vec::new(),
        }
    }
}

/// A factor setting as stored in a request: a number, or a categorical
/// factor's level by its label.
#[derive(Debug, Clone, PartialEq)]
pub enum FactorSetting {
    /// Value of a continuous factor.
    Number(f64),
    /// Level label of a categorical factor.
    Level(String),
}

/// How a continuous factor varies around its setting in a simulation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FactorVariation {
    /// Always at its setting.
    Fixed,
    /// Normal with mean at the setting and this standard deviation.
    Normal {
        /// Standard deviation.
        sd: f64,
    },
    /// Uniform on setting ± `half_width`.
    Uniform {
        /// Half the width of the interval.
        half_width: f64,
    },
    /// Triangular on setting ± `half_width`, peaked at the setting.
    Triangular {
        /// Half the width of the interval.
        half_width: f64,
    },
}

impl FactorVariation {
    /// Identifier in project files and scripts.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Fixed => "fixed",
            Self::Normal { .. } => "normal",
            Self::Uniform { .. } => "uniform",
            Self::Triangular { .. } => "triangular",
        }
    }

    /// The SD or half-width (0 when fixed).
    #[must_use]
    pub fn spread(self) -> f64 {
        match self {
            Self::Fixed => 0.0,
            Self::Normal { sd } => sd,
            Self::Uniform { half_width } | Self::Triangular { half_width } => half_width,
        }
    }

    /// The variation `key` with spread `spread`.
    #[must_use]
    pub fn from_key(key: &str, spread: f64) -> Option<Self> {
        Some(match key {
            "fixed" => Self::Fixed,
            "normal" => Self::Normal { sd: spread },
            "uniform" => Self::Uniform { half_width: spread },
            "triangular" => Self::Triangular { half_width: spread },
            _ => return None,
        })
    }

    /// A value around `center` from a uniform draw `u ∈ [0, 1)` and a
    /// standard normal draw `z` (so the same draws can be reused at other
    /// centres).
    fn value(self, center: f64, u: f64, z: f64) -> f64 {
        match self {
            Self::Fixed => center,
            Self::Normal { sd } => center + sd * z,
            Self::Uniform { half_width } => center + half_width * (2.0 * u - 1.0),
            Self::Triangular { half_width } => {
                stat_core::rng::triangular_quantile(u, center - half_width, center, center + half_width)
            }
        }
    }
}

/// Simulation settings.
#[derive(Debug, Clone, PartialEq)]
pub struct SimulationSetup {
    /// Number of simulated runs.
    pub runs: usize,
    /// Seed of the random draws.
    pub seed: u64,
    /// Variation of each factor (one per predictor; categorical factors stay fixed).
    pub factors: Vec<FactorVariation>,
    /// Standard deviation of the normal noise added to each response.
    pub noise: Vec<f64>,
    /// Lower and upper specification limit of each response.
    pub limits: Vec<(Option<f64>, Option<f64>)>,
    /// A box of settings (low, high per predictor; categorical factors and
    /// factors with low = high stay at their current setting) whose corners
    /// are checked for the probability of meeting every specification;
    /// `None` for no region.
    pub region: Option<Vec<(f64, f64)>>,
}

impl SimulationSetup {
    /// No variation, no noise, no limits, 10 000 runs, seed 1, for
    /// `factors` predictors and `responses` responses.
    #[must_use]
    pub fn new(factors: usize, responses: usize) -> Self {
        Self {
            runs: 10_000,
            seed: 1,
            factors: vec![FactorVariation::Fixed; factors],
            noise: vec![0.0; responses],
            limits: vec![(None, None); responses],
            region: None,
        }
    }
}

/// Several responses modelled on the same predictors.
#[derive(Debug, Clone, PartialEq)]
pub struct MultipleResponsesRequest {
    /// Response columns (numeric), at least one.
    pub responses: Vec<usize>,
    /// Predictor columns, at least one.
    pub predictors: Vec<usize>,
    /// Terms of every model.
    pub form: ModelForm,
    /// Desirability of each response; `None` maximizes it between its
    /// smallest and largest observed value.
    pub goals: Vec<Option<Desirability>>,
    /// Current factor settings, one per predictor (empty, or not matching
    /// the factors: the starting settings).
    pub settings: Vec<FactorSetting>,
    /// Simulation, when set up.
    pub simulation: Option<SimulationSetup>,
    /// Constraints on the continuous factors for the best-settings and
    /// robust-settings searches, one per line in the factors' units (see
    /// [`stat_doe::Region::parse`]); empty for none.
    pub constraints: String,
    /// Each response's terms chosen by its own stepwise search; `None` fits
    /// every term of the model form.
    pub selection: Option<TermSelection>,
}

impl MultipleResponsesRequest {
    /// Columns read.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        self.responses.iter().chain(&self.predictors).copied().collect()
    }

    /// The same request with every column index passed through `f`.
    #[must_use]
    pub fn map_columns(mut self, f: impl Fn(usize) -> usize) -> Self {
        self.responses.iter_mut().for_each(|c| *c = f(*c));
        self.predictors.iter_mut().for_each(|c| *c = f(*c));
        self
    }
}

/// One point found by the best-settings search.
#[derive(Debug, Clone, PartialEq)]
pub struct Solution {
    /// Factor settings.
    pub settings: Vec<TermValue>,
    /// Predicted value of each response.
    pub predicted: Vec<f64>,
    /// Desirability of each response (NaN without a goal).
    pub desirability: Vec<f64>,
    /// Overall desirability.
    pub overall: f64,
}

/// One response's simulation summary.
#[derive(Debug, Clone, PartialEq)]
pub struct SimulatedResponse {
    /// Response name.
    pub name: String,
    /// Simulated values, one per run.
    pub values: Vec<f64>,
    /// Mean of the simulated values.
    pub mean: f64,
    /// Standard deviation of the simulated values.
    pub sd: f64,
    /// Specification limits.
    pub limits: (Option<f64>, Option<f64>),
    /// Runs below the lower limit.
    pub below: usize,
    /// Runs above the upper limit.
    pub above: usize,
}

/// The result of a simulation.
#[derive(Debug, Clone, PartialEq)]
pub struct Simulation {
    /// Factor values of each run (one vector per factor; categorical
    /// factors hold their level index).
    pub factors: Vec<Vec<f64>>,
    /// Each response.
    pub responses: Vec<SimulatedResponse>,
    /// Runs with any response outside its limits.
    pub defects: usize,
    /// Number of runs.
    pub runs: usize,
    /// The specification region's corners, when one is set up (or why it
    /// could not be evaluated).
    pub region: Option<std::result::Result<SpecRegionCheck, String>>,
}

/// One point of a specification region.
#[derive(Debug, Clone, PartialEq)]
pub struct RegionPoint {
    /// Factor settings.
    pub settings: Vec<TermValue>,
    /// Predicted value of each response.
    pub predicted: Vec<f64>,
    /// Probability that every response meets its limits (normal noise).
    pub probability: f64,
    /// Share of simulated runs meeting every limit.
    pub simulated: f64,
    /// Simulated runs.
    pub runs: usize,
}

/// The probability of meeting every specification over a box of settings.
#[derive(Debug, Clone, PartialEq)]
pub struct SpecRegionCheck {
    /// Predictor positions that vary over the box.
    pub varying: Vec<usize>,
    /// Low and high of each varying factor.
    pub bounds: Vec<(f64, f64)>,
    /// The corners (the first varying factor alternating fastest).
    pub corners: Vec<RegionPoint>,
    /// The current settings.
    pub center: RegionPoint,
    /// Index of the corner with the lowest probability.
    pub worst: usize,
}

/// `(rate, lower, upper)`: the share `x / n` with its 95% Wilson score
/// interval (Wilson, *JASA* 22, 1927).
#[must_use]
pub fn wilson_interval(x: usize, n: usize) -> (f64, f64, f64) {
    if n == 0 {
        return (f64::NAN, f64::NAN, f64::NAN);
    }
    let z = Normal::standard().quantile(0.975).unwrap_or(1.959_963_984_540_054);
    let (p, n) = (x as f64 / n as f64, n as f64);
    let denom = 1.0 + z * z / n;
    let center = (p + z * z / (2.0 * n)) / denom;
    let half = z * (p * (1.0 - p) / n + z * z / (4.0 * n * n)).sqrt() / denom;
    (p, (center - half).max(0.0), (center + half).min(1.0))
}

/// A response's model as a fixed equation, for fast predictions (`None`
/// design: the mean only).
#[derive(Debug, Clone)]
struct Equation {
    design: Option<ModelDesign>,
    intercept: f64,
    slopes: Vec<f64>,
}

impl Equation {
    fn predict(&self, settings: &[TermValue]) -> f64 {
        let Some(design) = &self.design else { return self.intercept };
        match design.encode(settings) {
            Ok(row) => self.intercept + row.iter().zip(&self.slopes).map(|(x, b)| x * b).sum::<f64>(),
            Err(_) => f64::NAN,
        }
    }
}

/// A response for which term selection kept no term: its mean, constant in
/// every factor.
#[derive(Debug, Clone)]
pub struct MeanOnly {
    name: String,
    mean: f64,
    sd: f64,
    observations: Vec<Observation>,
}

impl MeanOnly {
    /// The mean of `response` over the rows a model of it on `predictors`
    /// would use.
    fn new(table: &DataTable, rows: &RowStateManager, response: usize, predictors: &[usize]) -> Result<Self> {
        let spec =
            LinearModelSpec { response, predictors: predictors.to_vec(), effects: Vec::new(), splines: Vec::new() };
        let coded = Coded::new(table, rows, &spec)?;
        if coded.y.is_empty() {
            return Err(invalid(format!("{:?} has no rows to fit", coded.response_name)));
        }
        let n = coded.y.len() as f64;
        let mean = stat_core::summation::mean(&coded.y).unwrap_or(f64::NAN);
        let sd = if coded.y.len() > 1 {
            (coded.y.iter().map(|y| (y - mean).powi(2)).sum::<f64>() / (n - 1.0)).sqrt()
        } else {
            f64::NAN
        };
        Ok(Self { name: coded.response_name.clone(), mean, sd, observations: coded.observations() })
    }

    /// Number of rows.
    #[must_use]
    pub fn n(&self) -> usize {
        self.observations.len()
    }

    /// Mean of the response.
    #[must_use]
    pub fn mean(&self) -> f64 {
        self.mean
    }

    /// Standard deviation of the response (the model's root mean square error).
    #[must_use]
    pub fn sd(&self) -> f64 {
        self.sd
    }

    /// The 95% confidence interval of the mean, `ȳ ± t₀.₉₇₅,ₙ₋₁ s/√n`.
    #[must_use]
    pub fn confidence_interval(&self) -> (f64, f64) {
        let n = self.n();
        if n < 2 {
            return (f64::NAN, f64::NAN);
        }
        let t = stat_core::distributions::StudentsT::new((n - 1) as f64)
            .and_then(|d| d.quantile(0.975))
            .unwrap_or(f64::NAN);
        let half = t * self.sd / (n as f64).sqrt();
        (self.mean - half, self.mean + half)
    }
}

/// One response's model: the fitted linear model, or its mean when term
/// selection kept no term.
#[derive(Debug, Clone)]
pub enum ResponseModel {
    /// A least-squares fit.
    Fitted(Box<LinearModelAnalysis>),
    /// The mean only.
    MeanOnly(MeanOnly),
}

impl ResponseModel {
    /// The fitted linear model (`None` for the mean only).
    #[must_use]
    pub fn linear(&self) -> Option<&LinearModelAnalysis> {
        match self {
            Self::Fitted(m) => Some(m),
            Self::MeanOnly(_) => None,
        }
    }

    /// Response name.
    #[must_use]
    pub fn response_name(&self) -> &str {
        match self {
            Self::Fitted(m) => m.response_name(),
            Self::MeanOnly(m) => &m.name,
        }
    }

    /// Root mean square error (the standard deviation for the mean only).
    #[must_use]
    pub fn root_mean_square_error(&self) -> f64 {
        match self {
            Self::Fitted(m) => m.fit().summary().root_mean_square_error,
            Self::MeanOnly(m) => m.sd,
        }
    }

    /// The rows used, as profiler observations.
    #[must_use]
    pub fn observations(&self) -> Vec<Observation> {
        match self {
            Self::Fitted(m) => m.observations(),
            Self::MeanOnly(m) => m.observations.clone(),
        }
    }

    fn equation(&self) -> Equation {
        match self {
            Self::Fitted(m) => {
                let b = m.fit().coefficients();
                Equation { design: Some(m.design().clone()), intercept: b[0], slopes: b[1..].to_vec() }
            }
            Self::MeanOnly(m) => Equation { design: None, intercept: m.mean, slopes: Vec::new() },
        }
    }
}

/// One response's model with the factors shared by all responses (so every
/// row of the linked explorer spans the same ranges).
struct Shared<'a> {
    model: &'a ResponseModel,
    factors: &'a [ProfileFactor],
    defaults: &'a [TermValue],
}

impl SurfaceModel for Shared<'_> {
    fn response_name(&self) -> &str {
        self.model.response_name()
    }
    fn profile_factors(&self) -> Vec<ProfileFactor> {
        self.factors.to_vec()
    }
    fn default_profile_settings(&self) -> Vec<TermValue> {
        match self.model {
            ResponseModel::Fitted(m) => m.default_profile_settings(),
            ResponseModel::MeanOnly(_) => self.defaults.to_vec(),
        }
    }
    fn predict_settings(&self, points: &[Vec<TermValue>]) -> Result<Vec<f64>> {
        match self.model {
            ResponseModel::Fitted(m) => m.predict_settings(points),
            ResponseModel::MeanOnly(m) => Ok(vec![m.mean; points.len()]),
        }
    }
    fn predict_with_limits(&self, points: &[Vec<TermValue>]) -> Result<Vec<Limited>> {
        match self.model {
            ResponseModel::Fitted(m) => m.predict_with_limits(points),
            ResponseModel::MeanOnly(m) => Ok(vec![(m.mean, Some(m.confidence_interval())); points.len()]),
        }
    }
    fn confidence_level(&self) -> Option<f64> {
        match self.model {
            ResponseModel::Fitted(m) => m.confidence_level(),
            ResponseModel::MeanOnly(_) => Some(0.95),
        }
    }
    fn observations(&self) -> Vec<Observation> {
        self.model.observations()
    }
}

/// Fitted models of several responses and their joint exploration.
#[derive(Debug, Clone)]
pub struct MultipleResponsesAnalysis {
    request: MultipleResponsesRequest,
    models: Vec<ResponseModel>,
    /// Each response's stepwise search (empty without term selection).
    searches: Vec<StepwiseAnalysis>,
    equations: Vec<Equation>,
    factors: Vec<ProfileFactor>,
    goals: Vec<Desirability>,
    settings: Vec<TermValue>,
    defaults: Vec<TermValue>,
    /// Constraints over the continuous factors (in their order), coded by
    /// the factors' ranges.
    region: Option<Region>,
}

impl MultipleResponsesAnalysis {
    /// Fit every response's model (after its stepwise search, with term
    /// selection).
    ///
    /// # Errors
    /// No responses or predictors, a response also used as a predictor, a
    /// goal that is not valid, simulation settings of the wrong size or with
    /// a negative spread, categorical factors whose levels differ between
    /// responses, candidate effects that name no predictor, no term selected
    /// for any response, and any error searching or fitting a model.
    pub fn run(table: &DataTable, rows: &RowStateManager, request: &MultipleResponsesRequest) -> Result<Self> {
        if request.responses.is_empty() {
            return Err(invalid("choose at least one response"));
        }
        if request.predictors.is_empty() {
            return Err(invalid("choose at least one factor"));
        }
        if request.responses.iter().any(|r| request.predictors.contains(r)) {
            return Err(invalid("a response cannot also be a factor"));
        }
        let continuous: Vec<bool> = request
            .predictors
            .iter()
            .map(|&i| {
                table
                    .column(i)
                    .map(|c| c.as_numeric().is_some() && c.level == MeasurementLevel::Continuous)
                    .ok_or_else(|| invalid(format!("no column {i}")))
            })
            .collect::<Result<_>>()?;
        let effects = match &request.selection {
            Some(sel) if !sel.candidates.is_empty() => {
                if sel.candidates.iter().flatten().any(|&(p, _)| p >= request.predictors.len()) {
                    return Err(invalid("a candidate effect names a predictor that is not chosen"));
                }
                sel.candidates.clone()
            }
            _ => effects(request.form, &continuous),
        };
        let name = |response: usize| table.column(response).map_or("?", |c| c.name.as_str()).to_owned();
        let mut searches = Vec::new();
        let models = request
            .responses
            .iter()
            .map(|&response| {
                let spec = LinearModelSpec {
                    response,
                    predictors: request.predictors.clone(),
                    effects: effects.clone(),
                    splines: Vec::new(),
                };
                let Some(sel) = &request.selection else {
                    return LinearModelAnalysis::run(table, rows, &spec)
                        .map(|m| ResponseModel::Fitted(Box::new(m)))
                        .map_err(|e| invalid(format!("the model of {:?} could not be fitted: {e}", name(response))));
                };
                let search = StepwiseAnalysis::run_keeping_predictors(
                    table,
                    rows,
                    &StepwiseRequest {
                        candidates: spec,
                        direction: sel.direction,
                        criterion: sel.criterion,
                        heredity: sel.heredity,
                    },
                )
                .map_err(|e| invalid(format!("the stepwise search for {:?} failed: {e}", name(response))))?;
                let model = match search.selected() {
                    Some(m) => ResponseModel::Fitted(Box::new(m.clone())),
                    None => ResponseModel::MeanOnly(MeanOnly::new(table, rows, response, &request.predictors)?),
                };
                searches.push(search);
                Ok(model)
            })
            .collect::<Result<Vec<_>>>()?;
        let fitted: Vec<&LinearModelAnalysis> = models.iter().filter_map(ResponseModel::linear).collect();
        if fitted.is_empty() {
            return Err(invalid(
                "no term was selected for any response; loosen the selection criterion or choose other factors",
            ));
        }
        let factors = shared_factors(&fitted)?;
        let defaults = fitted[0].default_profile_settings();
        let equations = models.iter().map(ResponseModel::equation).collect();
        let goals = request
            .goals
            .iter()
            .chain(std::iter::repeat(&None))
            .zip(&models)
            .map(|(goal, model)| match goal {
                Some(d) => d.validate().map(|()| *d),
                None => Ok(default_goal(model)),
            })
            .collect::<Result<Vec<_>>>()?;
        if let Some(sim) = &request.simulation {
            if sim.factors.len() != request.predictors.len()
                || sim.noise.len() != request.responses.len()
                || sim.limits.len() != request.responses.len()
            {
                return Err(invalid("the simulation settings do not match the factors and responses"));
            }
            if sim.runs == 0 || sim.runs > MAX_RUNS {
                return Err(invalid(format!("a simulation needs between 1 and {MAX_RUNS} runs")));
            }
            if sim.factors.iter().any(|f| !(f.spread() >= 0.0 && f.spread().is_finite()))
                || sim.noise.iter().any(|s| !(*s >= 0.0 && s.is_finite()))
            {
                return Err(invalid("simulation spreads and noise must be zero or positive"));
            }
            if let Some(region) = &sim.region {
                if region.len() != request.predictors.len() {
                    return Err(invalid("the specification region needs a low and high for every factor"));
                }
                if region.iter().any(|(lo, hi)| !(lo.is_finite() && hi.is_finite() && lo <= hi)) {
                    return Err(invalid("each factor's region low must be a number at most its high"));
                }
            }
        }
        let region = if request.constraints.trim().is_empty() {
            None
        } else {
            let continuous: Vec<Factor> = factors
                .iter()
                .filter_map(|f| match f.kind {
                    ProfileFactorKind::Continuous { range: (lo, hi) } => Some(Factor::new(f.name.clone(), lo, hi)),
                    ProfileFactorKind::Categorical { .. } => None,
                })
                .collect();
            if continuous.is_empty() {
                return Err(invalid("constraints need continuous factors"));
            }
            Some(Region::parse(&request.constraints, &continuous)?)
        };
        let mut out = Self {
            request: request.clone(),
            models,
            searches,
            equations,
            factors,
            goals,
            settings: Vec::new(),
            defaults,
            region,
        };
        out.settings = out.resolve(&request.settings).unwrap_or_else(|| out.defaults.clone());
        Ok(out)
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &MultipleResponsesRequest {
        &self.request
    }

    /// Each response's model.
    #[must_use]
    pub fn models(&self) -> &[ResponseModel] {
        &self.models
    }

    /// Each response's stepwise search (empty without term selection).
    #[must_use]
    pub fn searches(&self) -> &[StepwiseAnalysis] {
        &self.searches
    }

    /// The factors (ranges are the union over the responses' fits).
    #[must_use]
    pub fn factors(&self) -> &[ProfileFactor] {
        &self.factors
    }

    /// Each response's desirability (the request's, or the default).
    #[must_use]
    pub fn goals(&self) -> &[Desirability] {
        &self.goals
    }

    /// The current settings.
    #[must_use]
    pub fn settings(&self) -> &[TermValue] {
        &self.settings
    }

    /// The starting settings: continuous factors at their means and
    /// categorical factors at their most common level (in the first
    /// response's fit).
    #[must_use]
    pub fn default_settings(&self) -> &[TermValue] {
        &self.defaults
    }

    /// Response names.
    #[must_use]
    pub fn response_names(&self) -> Vec<String> {
        self.models.iter().map(|m| m.response_name().to_owned()).collect()
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        let factors: Vec<&str> = self.factors.iter().map(|f| f.name.as_str()).collect();
        let model = match &self.request.selection {
            None => self.request.form.label().to_lowercase(),
            Some(sel) if sel.candidates.is_empty() => {
                format!("terms selected stepwise from {}", self.request.form.label().to_lowercase())
            }
            Some(sel) => format!("terms selected stepwise from {} candidates", sel.candidates.len()),
        };
        format!("Several Responses: {} ~ {} ({model})", self.response_names().join(", "), factors.join(" + "))
    }

    /// `true` if `settings` has one value of the right kind per factor.
    #[must_use]
    pub fn settings_fit(&self, settings: &[TermValue]) -> bool {
        settings.len() == self.factors.len()
            && self.factors.iter().zip(settings).all(|(f, v)| match (&f.kind, v) {
                (ProfileFactorKind::Continuous { .. }, TermValue::Number(x)) => x.is_finite(),
                (ProfileFactorKind::Categorical { levels }, TermValue::Level(l)) => *l < levels.len(),
                _ => false,
            })
    }

    /// Request settings as model settings (`None` if they do not fit).
    #[must_use]
    pub fn resolve(&self, settings: &[FactorSetting]) -> Option<Vec<TermValue>> {
        if settings.len() != self.factors.len() {
            return None;
        }
        self.factors
            .iter()
            .zip(settings)
            .map(|(f, s)| match (&f.kind, s) {
                (ProfileFactorKind::Continuous { .. }, FactorSetting::Number(x)) if x.is_finite() => {
                    Some(TermValue::Number(*x))
                }
                (ProfileFactorKind::Categorical { levels }, FactorSetting::Level(l)) => {
                    levels.iter().position(|v| v == l).map(TermValue::Level)
                }
                _ => None,
            })
            .collect()
    }

    /// Model settings as request settings (levels by label).
    #[must_use]
    pub fn to_request_settings(&self, settings: &[TermValue]) -> Vec<FactorSetting> {
        self.factors
            .iter()
            .zip(settings)
            .map(|(f, v)| match (&f.kind, v) {
                (ProfileFactorKind::Categorical { levels }, TermValue::Level(l)) => {
                    FactorSetting::Level(levels.get(*l).cloned().unwrap_or_default())
                }
                (_, TermValue::Number(x)) => FactorSetting::Number(*x),
                (_, TermValue::Level(l)) => FactorSetting::Number(*l as f64),
            })
            .collect()
    }

    /// Predicted value of each response at `settings`.
    #[must_use]
    pub fn predict(&self, settings: &[TermValue]) -> Vec<f64> {
        self.equations.iter().map(|e| e.predict(settings)).collect()
    }

    /// Desirability of each response (NaN without a goal) and the overall
    /// desirability, for predictions `predicted`.
    #[must_use]
    pub fn desirability(&self, predicted: &[f64]) -> (Vec<f64>, f64) {
        let d: Vec<f64> = self.goals.iter().zip(predicted).map(|(g, &y)| g.of(y)).collect();
        let weights: Vec<f64> = self.goals.iter().map(|g| g.importance).collect();
        let overall = overall_desirability(&d, &weights);
        (d, overall)
    }

    /// `true` if at least one response has a goal (enters the overall desirability).
    #[must_use]
    pub fn has_goal(&self) -> bool {
        self.goals.iter().any(|g| g.goal != Goal::None)
    }

    /// The linked Response Explorer at `settings`: one profile per response
    /// over the shared factors, then (when a response has a goal) the
    /// overall desirability along each factor.
    ///
    /// # Errors
    /// Settings that do not fit the factors; prediction errors.
    pub fn profiles(&self, settings: &[TermValue]) -> Result<Vec<Profile>> {
        if !self.settings_fit(settings) {
            return Err(invalid("the settings do not match the factors"));
        }
        let mut out = self
            .models
            .iter()
            .map(|model| {
                crate::surface_profiler::profile(
                    &Shared { model, factors: &self.factors, defaults: &self.defaults },
                    settings,
                )
            })
            .collect::<Result<Vec<_>>>()?;
        if self.has_goal() {
            out.push(self.desirability_profile(settings));
        }
        Ok(out)
    }

    fn overall_at(&self, settings: &[TermValue]) -> f64 {
        self.desirability(&self.predict(settings)).1
    }

    /// The overall desirability along each factor, the others held at `settings`.
    fn desirability_profile(&self, settings: &[TermValue]) -> Profile {
        let with = |j: usize, v: TermValue| {
            let mut s = settings.to_vec();
            s[j] = v;
            s
        };
        let panels = self
            .factors
            .iter()
            .enumerate()
            .map(|(j, f)| match &f.kind {
                ProfileFactorKind::Continuous { range: (lo, hi) } => {
                    let n = if hi > lo { CURVE_POINTS } else { 1 };
                    let curve = (0..n)
                        .map(|i| {
                            let x = lo + (hi - lo) * i as f64 / (n.max(2) - 1) as f64;
                            [x, self.overall_at(&with(j, TermValue::Number(x)))]
                        })
                        .collect();
                    let value = match settings[j] {
                        TermValue::Number(x) => x,
                        TermValue::Level(_) => f64::NAN,
                    };
                    ProfilePanel::Continuous { name: f.name.clone(), curve, band: None, range: (*lo, *hi), value }
                }
                ProfileFactorKind::Categorical { levels } => ProfilePanel::Categorical {
                    name: f.name.clone(),
                    levels: levels.clone(),
                    predictions: (0..levels.len())
                        .map(|l| (self.overall_at(&with(j, TermValue::Level(l))), None))
                        .collect(),
                    selected: match settings[j] {
                        TermValue::Level(l) => l,
                        TermValue::Number(_) => 0,
                    },
                },
            })
            .collect();
        Profile {
            response: "Desirability".to_owned(),
            panels,
            predicted: self.overall_at(settings),
            confidence_interval: None,
            confidence_level: None,
            y_range: (-0.05, 1.05),
            note: None,
        }
    }

    /// The best settings found by maximizing the overall desirability (see
    /// the module documentation), best first, at most five, distinct.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] when no response has a goal.
    pub fn best_settings(&self) -> Result<Vec<Solution>> {
        if !self.has_goal() {
            return Err(invalid("give at least one response a goal to search for the best settings"));
        }
        let continuous: Vec<usize> = (0..self.factors.len()).filter(|&j| self.factors[j].is_continuous()).collect();
        let (lower, upper): (Vec<f64>, Vec<f64>) = continuous
            .iter()
            .map(|&j| match self.factors[j].kind {
                ProfileFactorKind::Continuous { range } => range,
                ProfileFactorKind::Categorical { .. } => (0.0, 0.0),
            })
            .unzip();
        let mut candidates: Vec<Solution> = Vec::new();
        let mut rng = Rng::new(20_240_917);
        let n_starts = (4 * continuous.len()).clamp(8, 24);
        for base in self.level_combinations() {
            let at = |u: &[f64]| {
                let mut s = base.clone();
                for (k, &j) in continuous.iter().enumerate() {
                    s[j] = TermValue::Number(u[k]);
                }
                s
            };
            let mut starts: Vec<Vec<f64>> = vec![
                continuous
                    .iter()
                    .map(|&j| match self.settings[j] {
                        TermValue::Number(x) => x,
                        TermValue::Level(_) => 0.0,
                    })
                    .collect(),
            ];
            starts.extend(
                latin_hypercube(n_starts, continuous.len(), &mut rng)
                    .into_iter()
                    .map(|u| u.iter().zip(lower.iter().zip(&upper)).map(|(t, (lo, hi))| lo + t * (hi - lo)).collect()),
            );
            if let Some(region) = &self.region {
                // Starts inside the constrained region too.
                let mut r = Rng::new(20_240_918);
                for _ in 0..n_starts / 2 {
                    if let Ok(z) = region.random_point(&mut r) {
                        starts.push(
                            z.iter().zip(lower.iter().zip(&upper)).map(|(c, (lo, hi))| uncode(*c, *lo, *hi)).collect(),
                        );
                    }
                }
            }
            for x0 in starts {
                let mut objective = |u: &[f64]| {
                    let (d, overall) = self.desirability(&self.predict(&at(u)));
                    let value = if overall > 0.0 {
                        -overall
                    } else {
                        // Climb the individual desirabilities out of a D = 0 region.
                        let known: Vec<f64> = d.into_iter().filter(|v| !v.is_nan()).collect();
                        1.0 - known.iter().sum::<f64>() / known.len().max(1) as f64
                    };
                    let v = self.violation(u, &lower, &upper);
                    if v > 0.0 { value + 2.0 + v } else { value }
                };
                let m = minimize_in_box(&mut objective, &x0, &lower, &upper, 400 * (continuous.len() + 1));
                if self.violation(&m.x, &lower, &upper) > 0.0 {
                    continue;
                }
                let settings = at(&m.x);
                let predicted = self.predict(&settings);
                let (desirability, overall) = self.desirability(&predicted);
                candidates.push(Solution { settings, predicted, desirability, overall });
            }
        }
        if candidates.is_empty() {
            return Err(invalid("no settings within the factor ranges satisfy the constraints"));
        }
        candidates.sort_by(|a, b| b.overall.total_cmp(&a.overall));
        let mut out: Vec<Solution> = Vec::new();
        for c in candidates {
            if out.len() == TOP_SOLUTIONS {
                break;
            }
            if !out.iter().any(|s| self.same_point(&s.settings, &c.settings)) {
                out.push(c);
            }
        }
        Ok(out)
    }

    /// How far continuous-factor values `u` (in the order of the continuous
    /// factors, between `lower` and `upper`) violate the constraints: the sum
    /// of the linear constraints' shortfalls in coded units plus 1 per
    /// disallowed combination met; 0 inside (or without constraints).
    fn violation(&self, u: &[f64], lower: &[f64], upper: &[f64]) -> f64 {
        let Some(region) = &self.region else { return 0.0 };
        let z: Vec<f64> = u.iter().zip(lower.iter().zip(upper)).map(|(x, (lo, hi))| code(*x, *lo, *hi)).collect();
        let linear: f64 =
            region.linear().iter().map(|c| (-c.slack(&z) - stat_doe::constraints::TOLERANCE).max(0.0)).sum();
        let excluded = region.exclusions().iter().filter(|e| e.excludes(&z)).count() as f64;
        linear + excluded
    }

    /// `true` if `settings` satisfy the constraints (always without any).
    #[must_use]
    pub fn satisfies_constraints(&self, settings: &[TermValue]) -> bool {
        let (lower, upper) = self.continuous_bounds();
        let u: Vec<f64> = self
            .factors
            .iter()
            .zip(settings)
            .filter(|(f, _)| f.is_continuous())
            .map(|(_, v)| match v {
                TermValue::Number(x) => *x,
                TermValue::Level(_) => f64::NAN,
            })
            .collect();
        self.violation(&u, &lower, &upper) == 0.0
    }

    /// The constraints as written, one per entry (empty without any).
    #[must_use]
    pub fn constraint_lines(&self) -> Vec<String> {
        self.region.as_ref().map(|r| r.describe().to_vec()).unwrap_or_default()
    }

    /// Lowest and highest value of each continuous factor.
    fn continuous_bounds(&self) -> (Vec<f64>, Vec<f64>) {
        self.factors
            .iter()
            .filter_map(|f| match f.kind {
                ProfileFactorKind::Continuous { range } => Some(range),
                ProfileFactorKind::Categorical { .. } => None,
            })
            .unzip()
    }

    /// Whether two settings are the same point (same levels, continuous
    /// factors within 1% of their ranges).
    fn same_point(&self, a: &[TermValue], b: &[TermValue]) -> bool {
        self.factors.iter().zip(a.iter().zip(b)).all(|(f, pair)| match (&f.kind, pair) {
            (ProfileFactorKind::Continuous { range: (lo, hi) }, (TermValue::Number(x), TermValue::Number(y))) => {
                (x - y).abs() <= 0.01 * (hi - lo).max(f64::EPSILON)
            }
            (_, (TermValue::Level(x), TermValue::Level(y))) => x == y,
            _ => false,
        })
    }

    /// The settings to search from for each combination of categorical
    /// levels (only the current levels when there are too many).
    fn level_combinations(&self) -> Vec<Vec<TermValue>> {
        let categorical: Vec<(usize, usize)> = self
            .factors
            .iter()
            .enumerate()
            .filter_map(|(j, f)| match &f.kind {
                ProfileFactorKind::Categorical { levels } => Some((j, levels.len())),
                ProfileFactorKind::Continuous { .. } => None,
            })
            .collect();
        let total: usize = categorical.iter().map(|&(_, n)| n).product();
        if categorical.is_empty() || total > MAX_COMBINATIONS {
            return vec![self.settings.clone()];
        }
        let mut out = Vec::with_capacity(total);
        let mut at = vec![0usize; categorical.len()];
        for _ in 0..total {
            let mut s = self.settings.clone();
            for (&(j, _), &l) in categorical.iter().zip(&at) {
                s[j] = TermValue::Level(l);
            }
            out.push(s);
            for k in (0..at.len()).rev() {
                at[k] += 1;
                if at[k] < categorical[k].1 {
                    break;
                }
                at[k] = 0;
            }
        }
        out
    }

    fn setup(&self) -> Result<&SimulationSetup> {
        self.request.simulation.as_ref().ok_or_else(|| invalid("set up the simulation first"))
    }

    /// Simulate the request's simulation setup around `settings`.
    ///
    /// # Errors
    /// No simulation set up, or settings that do not fit the factors.
    pub fn simulate(&self, settings: &[TermValue]) -> Result<Simulation> {
        let setup = self.setup()?;
        if !self.settings_fit(settings) {
            return Err(invalid("the settings do not match the factors"));
        }
        let draws = Draws::new(setup, self.factors.len(), self.models.len());
        let mut factors = vec![Vec::with_capacity(setup.runs); self.factors.len()];
        let mut values = vec![Vec::with_capacity(setup.runs); self.models.len()];
        let mut defects = 0;
        let mut run_settings = settings.to_vec();
        for r in 0..setup.runs {
            draws.settings(r, settings, setup, &mut run_settings);
            for (j, v) in run_settings.iter().enumerate() {
                factors[j].push(match v {
                    TermValue::Number(x) => *x,
                    TermValue::Level(l) => *l as f64,
                });
            }
            let mut bad = false;
            for (i, e) in self.equations.iter().enumerate() {
                let y = e.predict(&run_settings) + setup.noise[i] * draws.noise(r, i);
                bad |= outside(y, setup.limits[i]);
                values[i].push(y);
            }
            defects += usize::from(bad);
        }
        let responses = values
            .into_iter()
            .zip(self.response_names())
            .zip(&setup.limits)
            .map(|((v, name), &limits)| {
                let n = v.len() as f64;
                let mean = v.iter().sum::<f64>() / n;
                let sd = if v.len() > 1 {
                    (v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1.0)).sqrt()
                } else {
                    0.0
                };
                let below = limits.0.map_or(0, |l| v.iter().filter(|&&x| x < l).count());
                let above = limits.1.map_or(0, |u| v.iter().filter(|&&x| x > u).count());
                SimulatedResponse { name, values: v, mean, sd, limits, below, above }
            })
            .collect();
        let region = setup.region.as_ref().map(|_| self.spec_region(settings).map_err(|e| e.to_string()));
        Ok(Simulation { factors, responses, defects, runs: setup.runs, region })
    }

    /// Probability that every response meets its limits at `settings`: the
    /// normal-noise product and the share of `draws` runs inside.
    fn region_point(
        &self,
        settings: Vec<TermValue>,
        setup: &SimulationSetup,
        draws: &Draws,
        runs: usize,
    ) -> RegionPoint {
        let predicted = self.predict(&settings);
        let normal = Normal::standard();
        let probability = predicted
            .iter()
            .zip(&setup.noise)
            .zip(&setup.limits)
            .map(|((&y, &sd), &(lo, hi))| {
                if lo.is_none() && hi.is_none() {
                    return 1.0;
                }
                if sd == 0.0 {
                    return if outside(y, (lo, hi)) { 0.0 } else { 1.0 };
                }
                let cdf = |limit: f64| normal.cdf((limit - y) / sd).unwrap_or(f64::NAN);
                hi.map_or(1.0, cdf) - lo.map_or(0.0, cdf)
            })
            .product();
        let inside = (0..runs)
            .filter(|&r| {
                predicted
                    .iter()
                    .enumerate()
                    .all(|(i, &y)| !outside(y + setup.noise[i] * draws.noise(r, i), setup.limits[i]))
            })
            .count();
        RegionPoint { settings, predicted, probability, simulated: inside as f64 / runs as f64, runs }
    }

    /// The probability of meeting every specification at the corners of the
    /// simulation's region and at `settings` (see the module documentation).
    ///
    /// # Errors
    /// No simulation or region set up, no specification limit, settings that
    /// do not fit, or more than [`MAX_REGION_FACTORS`] varying factors.
    pub fn spec_region(&self, settings: &[TermValue]) -> Result<SpecRegionCheck> {
        let setup = self.setup()?;
        let bounds = setup.region.as_ref().ok_or_else(|| invalid("set up the specification region first"))?;
        if !self.settings_fit(settings) {
            return Err(invalid("the settings do not match the factors"));
        }
        if setup.limits.iter().all(|l| l.0.is_none() && l.1.is_none()) {
            return Err(invalid("give at least one response a specification limit"));
        }
        let varying: Vec<usize> =
            (0..self.factors.len()).filter(|&j| self.factors[j].is_continuous() && bounds[j].0 < bounds[j].1).collect();
        if varying.is_empty() {
            return Err(invalid("give at least one continuous factor a region low below its high"));
        }
        if varying.len() > MAX_REGION_FACTORS {
            return Err(invalid(format!("at most {MAX_REGION_FACTORS} factors can vary over the region")));
        }
        let runs = setup.runs.min(REGION_RUNS);
        let draws = Draws::new(&SimulationSetup { runs, ..setup.clone() }, self.factors.len(), self.models.len());
        let corners: Vec<RegionPoint> = (0..1usize << varying.len())
            .map(|c| {
                let mut s = settings.to_vec();
                for (b, &j) in varying.iter().enumerate() {
                    let (lo, hi) = bounds[j];
                    s[j] = TermValue::Number(if c >> b & 1 == 1 { hi } else { lo });
                }
                self.region_point(s, setup, &draws, runs)
            })
            .collect();
        let worst =
            (0..corners.len()).min_by(|&a, &b| corners[a].probability.total_cmp(&corners[b].probability)).unwrap_or(0);
        let center = self.region_point(settings.to_vec(), setup, &draws, runs);
        Ok(SpecRegionCheck { bounds: varying.iter().map(|&j| bounds[j]).collect(), varying, corners, center, worst })
    }

    /// Settings of the continuous factors (categorical ones stay at their
    /// current levels) with the lowest simulated overall defect rate, from
    /// the current settings; at most 2 000 runs per candidate, the same
    /// random draws at every candidate. Ties are broken towards responses
    /// centred between their limits.
    ///
    /// # Errors
    /// No simulation set up, or no response with a specification limit.
    pub fn robust_settings(&self) -> Result<Vec<TermValue>> {
        let setup = self.setup()?;
        if setup.limits.iter().all(|l| l.0.is_none() && l.1.is_none()) {
            return Err(invalid("give at least one response a specification limit"));
        }
        let search = SimulationSetup { runs: setup.runs.min(2_000), ..setup.clone() };
        let draws = Draws::new(&search, self.factors.len(), self.models.len());
        let continuous: Vec<usize> = (0..self.factors.len()).filter(|&j| self.factors[j].is_continuous()).collect();
        let (lower, upper): (Vec<f64>, Vec<f64>) = continuous
            .iter()
            .map(|&j| match self.factors[j].kind {
                ProfileFactorKind::Continuous { range } => range,
                ProfileFactorKind::Categorical { .. } => (0.0, 0.0),
            })
            .unzip();
        let at = |u: &[f64]| {
            let mut s = self.settings.clone();
            for (k, &j) in continuous.iter().enumerate() {
                s[j] = TermValue::Number(u[k]);
            }
            s
        };
        let mut run_settings = self.settings.clone();
        let mut objective = |u: &[f64]| {
            let center = at(u);
            let (mut defects, mut spread) = (0usize, 0.0);
            for r in 0..search.runs {
                draws.settings(r, &center, &search, &mut run_settings);
                let mut bad = false;
                for (i, e) in self.equations.iter().enumerate() {
                    let y = e.predict(&run_settings) + search.noise[i] * draws.noise(r, i);
                    bad |= outside(y, search.limits[i]);
                    if let (Some(l), Some(h)) = search.limits[i] {
                        spread += ((y - 0.5 * (l + h)) / (0.5 * (h - l))).powi(2);
                    }
                }
                defects += usize::from(bad);
            }
            let n = search.runs as f64;
            let v = self.violation(u, &lower, &upper);
            defects as f64 / n + 1e-3 * spread / (n * self.equations.len() as f64) + if v > 0.0 { 2.0 + v } else { 0.0 }
        };
        let mut rng = Rng::new(search.seed ^ 0x5EED);
        let mut starts: Vec<Vec<f64>> = vec![
            continuous
                .iter()
                .map(|&j| match self.settings[j] {
                    TermValue::Number(x) => x,
                    TermValue::Level(_) => 0.0,
                })
                .collect(),
        ];
        starts.extend(
            latin_hypercube(4, continuous.len(), &mut rng)
                .into_iter()
                .map(|u| u.iter().zip(lower.iter().zip(&upper)).map(|(t, (lo, hi))| lo + t * (hi - lo)).collect()),
        );
        let best = starts
            .iter()
            .map(|x0| minimize_in_box(&mut objective, x0, &lower, &upper, 150 * (continuous.len() + 1)))
            .min_by(|a, b| a.value.total_cmp(&b.value))
            .map(|m| m.x)
            .unwrap_or_default();
        Ok(at(&best))
    }

    /// The simulated runs as a table: one column per factor (categorical
    /// factors as their level labels) and per response.
    ///
    /// # Errors
    /// Table construction errors (not expected).
    pub fn simulation_table(&self, sim: &Simulation) -> Result<DataTable> {
        let mut columns = Vec::new();
        for (f, values) in self.factors.iter().zip(&sim.factors) {
            columns.push(match &f.kind {
                ProfileFactorKind::Categorical { levels } => {
                    let mut c = Column::text(
                        f.name.clone(),
                        values.iter().map(|&l| levels.get(l as usize).cloned().unwrap_or_default()).collect(),
                    );
                    c.level = MeasurementLevel::Nominal;
                    c
                }
                ProfileFactorKind::Continuous { .. } => Column::numeric(f.name.clone(), values.clone()),
            });
        }
        for r in &sim.responses {
            columns.push(Column::numeric(r.name.clone(), r.values.clone()));
        }
        DataTable::new("Simulated runs", columns)
    }

    /// Report sections: the fits, the current settings and their
    /// desirability, the best settings found, and the simulation (when set up).
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let best = self.has_goal().then(|| self.best_settings().map_err(|e| e.to_string()));
        let sim = self.request.simulation.as_ref().map(|_| self.simulate(&self.settings).map_err(|e| e.to_string()));
        self.sections_with(
            best.as_ref().map(|b| b.as_deref().map_err(String::as_str)),
            sim.as_ref().map(|s| s.as_ref().map_err(String::as_str)),
        )
    }

    /// Report sections with an already computed best-settings search and
    /// simulation (`None` leaves the section out; an error becomes a note):
    /// the fits, the current settings, their desirability (when a response
    /// has a goal), the best settings and the simulation.
    #[must_use]
    pub fn sections_with(
        &self,
        best: Option<std::result::Result<&[Solution], &str>>,
        simulation: Option<std::result::Result<&Simulation, &str>>,
    ) -> Vec<ReportSection> {
        let mut out = vec![self.fits_section()];
        out.extend(self.selection_sections());
        out.push(self.settings_section());
        if self.has_goal() {
            out.push(self.desirability_section());
        }
        match best {
            Some(Ok(best)) => out.push(self.best_section(best)),
            Some(Err(e)) => out.push(note(SectionKind::BestSettings, "Best Settings", e.to_owned())),
            None => {}
        }
        match simulation {
            Some(Ok(sim)) => out.extend(self.simulation_sections(sim)),
            Some(Err(e)) => out.push(note(SectionKind::SimulationSummary, "Simulation", e.to_owned())),
            None => {}
        }
        out
    }

    /// Move the current settings to `settings` (ignored, returning `false`,
    /// when they do not fit the factors).
    pub fn set_settings(&mut self, settings: &[TermValue]) -> bool {
        let fits = self.settings_fit(settings);
        if fits {
            self.settings = settings.to_vec();
            self.request.settings = self.to_request_settings(settings);
        }
        fits
    }

    fn fits_section(&self) -> ReportSection {
        let mut t = TextTable::new(["Response", "R²", "Adjusted R²", "Root mean square error", "Observations"]);
        for m in &self.models {
            match m {
                ResponseModel::Fitted(m) => {
                    let s = m.fit().summary();
                    t.push([
                        m.response_name().to_owned(),
                        format_number(s.r_squared, DIGITS),
                        format_number(s.adj_r_squared, DIGITS),
                        format_number(s.root_mean_square_error, DIGITS),
                        s.n_observations.to_string(),
                    ]);
                }
                ResponseModel::MeanOnly(m) => t.push([
                    format!("{} (mean only)", m.name),
                    format_number(0.0, DIGITS),
                    format_number(0.0, DIGITS),
                    format_number(m.sd, DIGITS),
                    m.n().to_string(),
                ]),
            }
        }
        ReportSection {
            kind: SectionKind::ResponseFits,
            title: "Model Fits".to_owned(),
            content: SectionContent::Table(t),
        }
    }

    /// The term-selection summary (one row per response) and each
    /// response's step history; empty without term selection.
    fn selection_sections(&self) -> Vec<ReportSection> {
        let Some(sel) = &self.request.selection else { return Vec::new() };
        let criterion = match sel.criterion {
            StepCriterion::Bic => "BIC",
            _ => "AIC",
        };
        let mut t = TextTable::new([
            "Response".to_owned(),
            "Selected effects".to_owned(),
            "Effects".to_owned(),
            "Steps".to_owned(),
            format!("{criterion} of the selected model"),
        ]);
        for search in &self.searches {
            let chosen = search.selected_effect_names();
            let result = search.result();
            let last = result.steps.last().map_or(result.start_criterion, |s| s.criterion);
            t.push([
                search.response_name().to_owned(),
                if chosen.is_empty() { "none (mean only)".to_owned() } else { chosen.join(", ") },
                chosen.len().to_string(),
                result.steps.len().to_string(),
                format_number(last, DIGITS),
            ]);
        }
        let mut notes = Vec::new();
        if let Some(first) = self.searches.first() {
            notes.push(format!("Candidates: {}.", first.effect_names().join(", ")));
            if !first.parents_added().is_empty() {
                notes.push(format!("Parent effects added for heredity: {}.", first.parents_added().join(", ")));
            }
        }
        notes.push(
            "Each response's search is separate; p-values of the selected models do not account for the search."
                .to_owned(),
        );
        let mut out = vec![
            ReportSection {
                kind: SectionKind::TermSelection,
                title: format!("Term Selection ({})", search_description(sel.direction, sel.criterion, sel.heredity)),
                content: SectionContent::Table(t),
            },
            note(SectionKind::StepwiseSettings, "Selection Candidates", notes.join(" ")),
        ];
        for (i, search) in self.searches.iter().enumerate() {
            let mut h = search.history();
            h.kind = SectionKind::ResponseStepHistory(i);
            h.title = format!("Step History: {}", search.response_name());
            out.push(h);
        }
        out
    }

    fn setting_text(&self, j: usize, v: &TermValue) -> String {
        match (&self.factors[j].kind, v) {
            (ProfileFactorKind::Categorical { levels }, TermValue::Level(l)) => {
                levels.get(*l).cloned().unwrap_or_default()
            }
            (_, TermValue::Number(x)) => format_number(*x, DIGITS),
            (_, TermValue::Level(l)) => l.to_string(),
        }
    }

    fn settings_section(&self) -> ReportSection {
        let mut t = TextTable::new(["Factor", "Setting", "Low", "High"]);
        for (j, (f, v)) in self.factors.iter().zip(&self.settings).enumerate() {
            let (lo, hi) = match &f.kind {
                ProfileFactorKind::Continuous { range: (lo, hi) } => {
                    (format_number(*lo, DIGITS), format_number(*hi, DIGITS))
                }
                ProfileFactorKind::Categorical { levels } => (format!("{} levels", levels.len()), String::new()),
            };
            t.push([f.name.clone(), self.setting_text(j, v), lo, hi]);
        }
        ReportSection {
            kind: SectionKind::CurrentSettings,
            title: "Current Settings".to_owned(),
            content: SectionContent::Table(t),
        }
    }

    fn desirability_section(&self) -> ReportSection {
        let predicted = self.predict(&self.settings);
        let (d, overall) = self.desirability(&predicted);
        let mut t = TextTable::new([
            "Response",
            "Predicted",
            "Goal",
            "Lower limit",
            "Target",
            "Upper limit",
            "Importance",
            "Desirability",
        ]);
        for ((m, g), (&y, &di)) in self.models.iter().zip(&self.goals).zip(predicted.iter().zip(&d)) {
            let (goal, target) = goal_text(g.goal);
            let none = g.goal == Goal::None;
            let num = |v: f64| if none { String::new() } else { format_number(v, DIGITS) };
            t.push([
                m.response_name().to_owned(),
                format_number(y, DIGITS),
                goal.to_owned(),
                num(g.low),
                target.map_or(String::new(), |v| format_number(v, DIGITS)),
                num(g.high),
                num(g.importance),
                if di.is_nan() { String::new() } else { format_number(di, 4) },
            ]);
        }
        t.push([
            "Overall".to_owned(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            format_number(overall, 4),
        ]);
        ReportSection {
            kind: SectionKind::DesirabilityTable,
            title: "Desirability at Current Settings (overall: weighted geometric mean)".to_owned(),
            content: SectionContent::Table(t),
        }
    }

    fn best_section(&self, best: &[Solution]) -> ReportSection {
        let names = self.response_names();
        let headers: Vec<String> = ["Rank".to_owned(), "Desirability".to_owned()]
            .into_iter()
            .chain(self.factors.iter().map(|f| f.name.clone()))
            .chain(names.iter().map(|n| format!("Predicted {n}")))
            .collect();
        let mut t = TextTable::new(headers);
        for (k, s) in best.iter().enumerate() {
            let row: Vec<String> = [(k + 1).to_string(), format_number(s.overall, 4)]
                .into_iter()
                .chain(s.settings.iter().enumerate().map(|(j, v)| self.setting_text(j, v)))
                .chain(s.predicted.iter().map(|&y| format_number(y, DIGITS)))
                .collect();
            t.push(row);
        }
        let within = match self.constraint_lines().len() {
            0 => "the factor ranges".to_owned(),
            1 => "the factor ranges and 1 constraint".to_owned(),
            c => format!("the factor ranges and {c} constraints"),
        };
        ReportSection {
            kind: SectionKind::BestSettings,
            title: format!("Best Settings (highest overall desirability within {within})"),
            content: SectionContent::Table(t),
        }
    }

    /// The simulation summary table and one histogram per response.
    #[must_use]
    pub fn simulation_sections(&self, sim: &Simulation) -> Vec<ReportSection> {
        let mut t = TextTable::new([
            "Response",
            "Mean",
            "SD",
            "Lower spec",
            "Upper spec",
            "Below",
            "Above",
            "Defect rate",
            "95% interval",
        ]);
        let limit = |l: Option<f64>| l.map_or(String::new(), |v| format_number(v, DIGITS));
        let rate = |x: usize| {
            let (p, lo, hi) = wilson_interval(x, sim.runs);
            (format_number(p, 4), format!("[{}, {}]", format_number(lo, 4), format_number(hi, 4)))
        };
        for r in &sim.responses {
            let (p, ci) = rate(r.below + r.above);
            t.push([
                r.name.clone(),
                format_number(r.mean, DIGITS),
                format_number(r.sd, DIGITS),
                limit(r.limits.0),
                limit(r.limits.1),
                r.below.to_string(),
                r.above.to_string(),
                p,
                ci,
            ]);
        }
        let (p, ci) = rate(sim.defects);
        t.push([
            "Any response".to_owned(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            p,
            ci,
        ]);
        let mut out = vec![ReportSection {
            kind: SectionKind::SimulationSummary,
            title: format!(
                "Simulation: {} runs at the current settings (defect = outside a specification limit)",
                sim.runs
            ),
            content: SectionContent::Table(t),
        }];
        match &sim.region {
            Some(Ok(check)) => out.extend(self.region_sections(check)),
            Some(Err(e)) => out.push(note(SectionKind::SpecRegion, "Specification Region", e.clone())),
            None => {}
        }
        for (i, r) in sim.responses.iter().enumerate() {
            out.push(ReportSection {
                kind: SectionKind::SimulatedResponse(i),
                title: format!("Simulated {}", r.name),
                content: SectionContent::Bars(histogram(r)),
            });
        }
        out
    }

    /// The specification region's table of corners and its contour plot.
    #[must_use]
    pub fn region_sections(&self, check: &SpecRegionCheck) -> Vec<ReportSection> {
        let names = self.response_names();
        let headers: Vec<String> = ["Point".to_owned()]
            .into_iter()
            .chain(check.varying.iter().map(|&j| self.factors[j].name.clone()))
            .chain(names.iter().map(|n| format!("Predicted {n}")))
            .chain(["P(all within spec)".to_owned(), "Simulated".to_owned(), "95% interval".to_owned()])
            .collect();
        let mut t = TextTable::new(headers);
        let row = |label: String, p: &RegionPoint| -> Vec<String> {
            let inside = (p.simulated * p.runs as f64).round() as usize;
            let (_, lo, hi) = wilson_interval(inside, p.runs);
            [label]
                .into_iter()
                .chain(check.varying.iter().map(|&j| self.setting_text(j, &p.settings[j])))
                .chain(p.predicted.iter().map(|&y| format_number(y, DIGITS)))
                .chain([
                    format_number(p.probability, 4),
                    format_number(p.simulated, 4),
                    format!("[{}, {}]", format_number(lo, 4), format_number(hi, 4)),
                ])
                .collect()
        };
        for (c, p) in check.corners.iter().enumerate() {
            let label =
                if c == check.worst { format!("Corner {} (worst)", c + 1) } else { format!("Corner {}", c + 1) };
            t.push(row(label, p));
        }
        t.push(row("Current settings".to_owned(), &check.center));
        let worst = &check.corners[check.worst];
        let mut out = vec![ReportSection {
            kind: SectionKind::SpecRegion,
            title: format!(
                "Specification Region: probability of meeting every limit at the {} corners (worst {}, normal noise; \
                 simulated with {} runs per corner)",
                check.corners.len(),
                format_number(worst.probability, 4),
                worst.runs
            ),
            content: SectionContent::Table(t),
        }];
        if let Some(plot) = self.region_contours(check) {
            out.push(plot);
        }
        out
    }

    /// Specification-limit contours over the first two varying factors (or
    /// the first varying factor and another continuous one), the others at
    /// the current settings, with the region's box and corners.
    fn region_contours(&self, check: &SpecRegionCheck) -> Option<ReportSection> {
        let setup = self.request.simulation.as_ref()?;
        let ix = *check.varying.first()?;
        let iy = check
            .varying
            .get(1)
            .copied()
            .or_else(|| (0..self.factors.len()).find(|&j| j != ix && self.factors[j].is_continuous()))?;
        let range = |j: usize| match self.factors[j].kind {
            ProfileFactorKind::Continuous { range } => range,
            ProfileFactorKind::Categorical { .. } => (0.0, 1.0),
        };
        let ((x0, x1), (y0, y1)) = (range(ix), range(iy));
        let n = CONTOUR_GRID;
        let coord = |i: usize| i as f64 / n as f64;
        let at = |u: f64, v: f64| {
            let mut s = check.center.settings.clone();
            s[ix] = TermValue::Number(x0 + u * (x1 - x0));
            s[iy] = TermValue::Number(y0 + v * (y1 - y0));
            s
        };
        let grids: Vec<Vec<Vec<f64>>> = {
            let mut g = vec![vec![vec![0.0; n + 1]; n + 1]; self.models.len()];
            for j in 0..=n {
                for i in 0..=n {
                    for (r, y) in self.predict(&at(coord(i), coord(j))).into_iter().enumerate() {
                        g[r][j][i] = y;
                    }
                }
            }
            g
        };
        let to_actual = |[u, v]: [f64; 2]| [x0 + u * (x1 - x0), y0 + v * (y1 - y0)];
        let mut segments: Vec<Polyline> = Vec::new();
        let mut labels = Vec::new();
        for (r, name) in self.response_names().iter().enumerate() {
            let (lo, hi) = setup.limits[r];
            for (limit, which) in [(lo, "lower"), (hi, "upper")] {
                let Some(level) = limit else { continue };
                let lines = crate::surface_analysis::contour_lines(&grids[r], level, coord);
                if let Some(longest) = lines.iter().max_by_key(|l| l.len()) {
                    let [x, y] = to_actual(longest[longest.len() / 2]);
                    labels.push((x, y, format!("{name} {which} {}", format_number(level, 4))));
                }
                segments.extend(lines.into_iter().map(|l| l.into_iter().map(to_actual).collect()));
            }
        }
        let bound = |j: usize| -> (f64, f64) {
            check.varying.iter().position(|&v| v == j).map_or_else(
                || match check.center.settings[j] {
                    TermValue::Number(x) => (x, x),
                    TermValue::Level(_) => (0.0, 0.0),
                },
                |b| check.bounds[b],
            )
        };
        let (bx, by) = (bound(ix), bound(iy));
        let rectangle = vec![[bx.0, by.0], [bx.1, by.0], [bx.1, by.1], [bx.0, by.1], [bx.0, by.0]];
        let value = |p: &RegionPoint, j: usize| match p.settings[j] {
            TermValue::Number(x) => x,
            TermValue::Level(_) => f64::NAN,
        };
        let mut items: Vec<(f64, f64, String, usize)> = Vec::new();
        for (c, p) in check.corners.iter().enumerate() {
            let group = usize::from(c == check.worst);
            let (x, y) = (value(p, ix), value(p, iy));
            // Corners differing only in other factors land on the same spot:
            // label the spot with its lowest probability.
            if let Some(item) = items.iter_mut().find(|it| it.0 == x && it.1 == y) {
                if group == 1 {
                    *item = (x, y, format!("P = {}", format_number(p.probability, 3)), 1);
                }
                continue;
            }
            items.push((x, y, format!("P = {}", format_number(p.probability, 3)), group));
        }
        let others: Vec<String> = (0..self.factors.len())
            .filter(|&j| j != ix && j != iy)
            .map(|j| format!("{} = {}", self.factors[j].name, self.setting_text(j, &check.center.settings[j])))
            .collect();
        let mut title = "Specification Limits and Region".to_owned();
        if !others.is_empty() {
            title.push_str(&format!(" ({} at the current settings)", others.join(", ")));
        }
        let spec = PlotSpec {
            x_label: self.factors[ix].name.clone(),
            y_label: self.factors[iy].name.clone(),
            line: Some(rectangle),
            line_name: Some("Region".to_owned()),
            annotations: Some(Annotations {
                groups: vec!["Corner".to_owned(), "Worst corner".to_owned()],
                items,
                labels,
            }),
            segments,
            ..PlotSpec::default()
        };
        Some(ReportSection {
            kind: SectionKind::SpecRegionContours,
            title,
            content: SectionContent::Plot(Box::new(spec)),
        })
    }
}

fn note(kind: SectionKind, title: &str, text: String) -> ReportSection {
    ReportSection { kind, title: title.to_owned(), content: SectionContent::Note(text) }
}

/// Goal name and target value.
#[must_use]
pub fn goal_text(goal: Goal) -> (&'static str, Option<f64>) {
    match goal {
        Goal::Maximize => ("Maximize", None),
        Goal::Minimize => ("Minimize", None),
        Goal::Target(t) => ("Target", Some(t)),
        Goal::None => ("None", None),
    }
}

/// Coded value of `x` on `[lo, hi]` (−1 to +1).
fn code(x: f64, lo: f64, hi: f64) -> f64 {
    if hi > lo { (2.0 * x - lo - hi) / (hi - lo) } else { 0.0 }
}

/// Actual value of coded `z` on `[lo, hi]`.
fn uncode(z: f64, lo: f64, hi: f64) -> f64 {
    0.5 * (lo + hi) + 0.5 * z * (hi - lo)
}

fn outside(y: f64, (lo, hi): (Option<f64>, Option<f64>)) -> bool {
    lo.is_some_and(|l| y < l) || hi.is_some_and(|h| y > h)
}

/// The random draws of a simulation, stored so every candidate setting sees
/// the same ones: per run, a uniform and a normal per factor and a normal
/// per response.
struct Draws {
    uniform: Vec<f64>,
    normal: Vec<f64>,
    noise: Vec<f64>,
    factors: usize,
    responses: usize,
}

impl Draws {
    fn new(setup: &SimulationSetup, factors: usize, responses: usize) -> Self {
        let mut rng = Rng::new(setup.seed);
        let n = setup.runs;
        let mut uniform = Vec::with_capacity(n * factors);
        let mut normal = Vec::with_capacity(n * factors);
        let mut noise = Vec::with_capacity(n * responses);
        for _ in 0..n {
            for _ in 0..factors {
                uniform.push(rng.next_f64());
                normal.push(rng.normal());
            }
            for _ in 0..responses {
                noise.push(rng.normal());
            }
        }
        Self { uniform, normal, noise, factors, responses }
    }

    /// Settings of run `r` around `center`, written into `out`.
    fn settings(&self, r: usize, center: &[TermValue], setup: &SimulationSetup, out: &mut [TermValue]) {
        for (j, (c, o)) in center.iter().zip(out.iter_mut()).enumerate() {
            *o = match c {
                TermValue::Number(x) => TermValue::Number(setup.factors[j].value(
                    *x,
                    self.uniform[r * self.factors + j],
                    self.normal[r * self.factors + j],
                )),
                TermValue::Level(l) => TermValue::Level(*l),
            };
        }
    }

    fn noise(&self, r: usize, i: usize) -> f64 {
        self.noise[r * self.responses + i]
    }
}

/// A histogram of simulated values with the specification limits marked.
fn histogram(r: &SimulatedResponse) -> BarChartSpec {
    let finite: Vec<f64> = r.values.iter().copied().filter(|v| v.is_finite()).collect();
    let (lo, hi) = finite.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &v| (a.min(v), b.max(v)));
    let bins = 30usize;
    let width = if hi > lo { (hi - lo) / bins as f64 } else { 1.0 };
    let mut counts = vec![0usize; bins];
    for &v in &finite {
        let k = (((v - lo) / width).floor() as usize).min(bins - 1);
        counts[k] += 1;
    }
    let bars = counts
        .iter()
        .enumerate()
        .map(|(k, &count)| BarItem { center: lo + (k as f64 + 0.5) * width, width, rows: Vec::new(), unlinked: count })
        .collect();
    let mut references = Vec::new();
    if let Some(l) = r.limits.0 {
        references.push((l, "Lower spec".to_owned()));
    }
    if let Some(u) = r.limits.1 {
        references.push((u, "Upper spec".to_owned()));
    }
    BarChartSpec {
        x_label: r.name.clone(),
        y_label: "Runs".to_owned(),
        bars,
        x_ticks: None,
        curve: None,
        references,
        x_time: None,
    }
}

/// The effects of `form` over predictors whose continuity is `continuous`
/// (positions in the predictor list, as [`LinearModelSpec::effects`] takes them).
#[must_use]
pub fn effects(form: ModelForm, continuous: &[bool]) -> Vec<Vec<(usize, u8)>> {
    let k = continuous.len();
    let mut out: Vec<Vec<(usize, u8)>> = (0..k).map(|i| vec![(i, 1)]).collect();
    if form != ModelForm::MainEffects {
        for i in 0..k {
            for j in i + 1..k {
                out.push(vec![(i, 1), (j, 1)]);
            }
        }
    }
    if form == ModelForm::Quadratic {
        out.extend((0..k).filter(|&i| continuous[i]).map(|i| vec![(i, 2)]));
    }
    out
}

/// Maximize the response between its smallest and largest observed value.
fn default_goal(model: &ResponseModel) -> Desirability {
    let y: Vec<f64> = model.observations().iter().map(|o| o.response).collect();
    let (lo, hi) = y.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &v| (a.min(v), b.max(v)));
    if lo < hi { Desirability::new(Goal::Maximize, lo, hi) } else { Desirability::new(Goal::None, lo, hi) }
}

/// The factors shared by `models`: continuous ranges joined, categorical
/// levels identical.
fn shared_factors(models: &[&LinearModelAnalysis]) -> Result<Vec<ProfileFactor>> {
    let mut factors = models[0].profile_factors();
    for m in &models[1..] {
        for (f, g) in factors.iter_mut().zip(m.profile_factors()) {
            match (&mut f.kind, g.kind) {
                (ProfileFactorKind::Continuous { range }, ProfileFactorKind::Continuous { range: (lo, hi) }) => {
                    *range = (range.0.min(lo), range.1.max(hi));
                }
                (ProfileFactorKind::Categorical { levels }, ProfileFactorKind::Categorical { levels: other })
                    if *levels == other => {}
                _ => {
                    return Err(invalid(format!(
                        "the levels of {:?} differ between responses (some response is missing at a level); \
                         exclude those rows or analyse the responses separately",
                        f.name
                    )));
                }
            }
        }
    }
    Ok(factors)
}

fn object(members: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(members.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

fn optional_number(x: Option<f64>) -> JsonValue {
    x.map_or(JsonValue::Null, JsonValue::number)
}

/// The request's members other than its columns, for project files:
/// `"model"`, `"goals"`, `"settings"`, `"simulation"`, and when set
/// `"constraints"` and `"selection"` (`direction`, `criterion`, `enter`,
/// `remove`, `heredity` and, when chosen, `candidates` as lists of
/// `[predictor position, power]`).
#[must_use]
pub fn request_members_to_json(request: &MultipleResponsesRequest) -> Vec<(&'static str, JsonValue)> {
    let goals = request
        .goals
        .iter()
        .map(|g| match g {
            None => JsonValue::Null,
            Some(d) => {
                let (goal, target) = goal_text(d.goal);
                object(vec![
                    ("goal", JsonValue::String(goal.to_lowercase())),
                    ("low", JsonValue::number(d.low)),
                    ("target", optional_number(target)),
                    ("high", JsonValue::number(d.high)),
                    ("shape", JsonValue::number(d.shape)),
                    ("shape_above", JsonValue::number(d.shape_above)),
                    ("importance", JsonValue::number(d.importance)),
                ])
            }
        })
        .collect();
    let settings = request
        .settings
        .iter()
        .map(|s| match s {
            FactorSetting::Number(x) => JsonValue::number(*x),
            FactorSetting::Level(l) => JsonValue::String(l.clone()),
        })
        .collect();
    let simulation = request.simulation.as_ref().map_or(JsonValue::Null, |sim| {
        let mut members = vec![
            ("runs", JsonValue::Number(sim.runs as f64)),
            ("seed", JsonValue::Number(sim.seed as f64)),
            (
                "factors",
                JsonValue::Array(
                    sim.factors
                        .iter()
                        .map(|f| {
                            object(vec![
                                ("distribution", JsonValue::String(f.key().to_owned())),
                                ("spread", JsonValue::number(f.spread())),
                            ])
                        })
                        .collect(),
                ),
            ),
            ("noise", JsonValue::Array(sim.noise.iter().map(|&s| JsonValue::number(s)).collect())),
            ("lower_spec", JsonValue::Array(sim.limits.iter().map(|l| optional_number(l.0)).collect())),
            ("upper_spec", JsonValue::Array(sim.limits.iter().map(|l| optional_number(l.1)).collect())),
        ];
        if let Some(region) = &sim.region {
            members.push((
                "region",
                JsonValue::Array(
                    region
                        .iter()
                        .map(|&(lo, hi)| JsonValue::Array(vec![JsonValue::number(lo), JsonValue::number(hi)]))
                        .collect(),
                ),
            ));
        }
        object(members)
    });
    let mut out = vec![
        ("model", JsonValue::String(request.form.key().to_owned())),
        ("goals", JsonValue::Array(goals)),
        ("settings", JsonValue::Array(settings)),
        ("simulation", simulation),
    ];
    if !request.constraints.trim().is_empty() {
        out.push(("constraints", JsonValue::String(request.constraints.clone())));
    }
    if let Some(sel) = &request.selection {
        let mut members = crate::stepwise_analysis::search_to_json(sel.direction, sel.criterion, sel.heredity);
        if !sel.candidates.is_empty() {
            members.push(("candidates", crate::project::effects_json(&sel.candidates)));
        }
        out.push(("selection", object(members)));
    }
    out
}

/// The request from its project-file form, given its columns. Missing
/// members take their defaults (main effects, default goals, starting
/// settings, no simulation, every term fitted).
///
/// # Errors
/// An unknown model form, goal or distribution, or members of the wrong type.
pub fn request_from_json(
    v: &JsonValue,
    responses: Vec<usize>,
    predictors: Vec<usize>,
) -> Result<MultipleResponsesRequest> {
    let bad = |what: &str| invalid(format!("several responses: invalid {what}"));
    let form = match v.get("model").and_then(JsonValue::as_str) {
        None => ModelForm::MainEffects,
        Some(k) => ModelForm::from_key(k).ok_or_else(|| bad("model"))?,
    };
    let num = |g: &JsonValue, key: &str| g.get(key).and_then(JsonValue::as_f64);
    let goals = match v.get("goals").and_then(JsonValue::as_array) {
        None => vec![None; responses.len()],
        Some(list) => list
            .iter()
            .map(|g| {
                if matches!(g, JsonValue::Null) {
                    return Ok(None);
                }
                let goal = match g.get("goal").and_then(JsonValue::as_str) {
                    Some("maximize") => Goal::Maximize,
                    Some("minimize") => Goal::Minimize,
                    Some("target") => Goal::Target(num(g, "target").ok_or_else(|| bad("target"))?),
                    Some("none") => Goal::None,
                    _ => return Err(bad("goal")),
                };
                Ok(Some(Desirability {
                    goal,
                    low: num(g, "low").unwrap_or(f64::NAN),
                    high: num(g, "high").unwrap_or(f64::NAN),
                    shape: num(g, "shape").unwrap_or(1.0),
                    shape_above: num(g, "shape_above").unwrap_or(1.0),
                    importance: num(g, "importance").unwrap_or(1.0),
                }))
            })
            .collect::<Result<_>>()?,
    };
    let settings = match v.get("settings").and_then(JsonValue::as_array) {
        None => Vec::new(),
        Some(list) => list
            .iter()
            .map(|s| match s {
                JsonValue::Number(x) => Ok(FactorSetting::Number(*x)),
                JsonValue::String(l) => Ok(FactorSetting::Level(l.clone())),
                _ => Err(bad("settings")),
            })
            .collect::<Result<_>>()?,
    };
    let simulation = match v.get("simulation") {
        None | Some(JsonValue::Null) => None,
        Some(sim) => {
            let mut setup = SimulationSetup::new(predictors.len(), responses.len());
            if let Some(runs) = sim.get("runs").and_then(JsonValue::as_usize) {
                setup.runs = runs;
            }
            if let Some(seed) = sim.get("seed").and_then(crate::seed::seed_from_json) {
                setup.seed = seed;
            }
            if let Some(list) = sim.get("factors").and_then(JsonValue::as_array) {
                setup.factors = list
                    .iter()
                    .map(|f| {
                        let key = f.get("distribution").and_then(JsonValue::as_str).unwrap_or("fixed");
                        FactorVariation::from_key(key, num(f, "spread").unwrap_or(0.0))
                            .ok_or_else(|| bad("distribution"))
                    })
                    .collect::<Result<_>>()?;
            }
            let numbers = |key: &str| -> Option<Vec<Option<f64>>> {
                sim.get(key).and_then(JsonValue::as_array).map(|a| a.iter().map(JsonValue::as_f64).collect())
            };
            if let Some(noise) = numbers("noise") {
                setup.noise = noise.into_iter().map(|s| s.unwrap_or(0.0)).collect();
            }
            if let (Some(lo), Some(hi)) = (numbers("lower_spec"), numbers("upper_spec")) {
                setup.limits = lo.into_iter().zip(hi).collect();
            }
            if let Some(list) = sim.get("region").and_then(JsonValue::as_array) {
                setup.region = Some(
                    list.iter()
                        .map(|b| {
                            let pair = b.as_array().filter(|a| a.len() == 2).ok_or_else(|| bad("region"))?;
                            match (pair[0].as_f64(), pair[1].as_f64()) {
                                (Some(lo), Some(hi)) => Ok((lo, hi)),
                                _ => Err(bad("region")),
                            }
                        })
                        .collect::<Result<_>>()?,
                );
            }
            Some(setup)
        }
    };
    let constraints = v.get("constraints").and_then(JsonValue::as_str).unwrap_or_default().to_owned();
    let selection = match v.get("selection") {
        None | Some(JsonValue::Null) => None,
        Some(sel) => {
            let (direction, criterion, heredity) = crate::stepwise_analysis::search_from_json(sel, Heredity::Strong)?;
            let candidates = crate::project::linear_effects(&object(vec![(
                "effects",
                sel.get("candidates").cloned().unwrap_or(JsonValue::Array(Vec::new())),
            )]))?;
            Some(TermSelection { direction, criterion, heredity, candidates })
        }
    };
    Ok(MultipleResponsesRequest { responses, predictors, form, goals, settings, simulation, constraints, selection })
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_core::distributions::ContinuousDistribution;

    fn coating() -> (DataTable, MultipleResponsesRequest) {
        let s = (crate::sample::catalog().into_iter().find(|s| s.name.starts_with("Coating")).unwrap().build)();
        let crate::SavedReport::MultipleResponses { request } = &s.reports[0] else { panic!() };
        (s.table.clone(), (**request).clone())
    }

    #[test]
    fn models_match_separate_fits_and_profiles_link() {
        let (t, request) = coating();
        let rows = RowStateManager::new(t.n_rows());
        let a = MultipleResponsesAnalysis::run(&t, &rows, &request).unwrap();
        assert_eq!(a.models().len(), 3);
        // Each model is the ordinary quadratic fit of its response.
        let spec = LinearModelSpec {
            response: request.responses[1],
            predictors: request.predictors.clone(),
            effects: effects(ModelForm::Quadratic, &[true; 3]),
            splines: Vec::new(),
        };
        let alone = LinearModelAnalysis::run(&t, &rows, &spec).unwrap();
        assert_eq!(alone.fit().coefficients(), a.models()[1].linear().unwrap().fit().coefficients());
        // Fast predictions agree with the models' own.
        let s = a.settings().to_vec();
        for (m, y) in a.models().iter().filter_map(ResponseModel::linear).zip(a.predict(&s)) {
            let own = m.predict_settings(std::slice::from_ref(&s)).unwrap()[0];
            assert!((own - y).abs() < 1e-9 * own.abs().max(1.0));
        }
        // One explorer row per response plus desirability, all on the same ranges.
        let profiles = a.profiles(&s).unwrap();
        assert_eq!(profiles.len(), 4);
        assert_eq!(profiles[3].response, "Desirability");
        let range = |p: &Profile| match &p.panels[0] {
            ProfilePanel::Continuous { range, .. } => *range,
            ProfilePanel::Categorical { .. } => panic!(),
        };
        assert!(profiles.iter().all(|p| range(p) == range(&profiles[0])));
        let (d, overall) = a.desirability(&a.predict(&s));
        assert!((profiles[3].predicted - overall).abs() < 1e-12);
        assert!(d.iter().all(|v| (0.0..=1.0).contains(v)));
    }

    #[test]
    fn best_settings_beat_the_start_and_are_local_optima() {
        let (t, request) = coating();
        let a = MultipleResponsesAnalysis::run(&t, &RowStateManager::new(t.n_rows()), &request).unwrap();
        let best = a.best_settings().unwrap();
        assert!(!best.is_empty() && best.len() <= TOP_SOLUTIONS);
        assert!(best.windows(2).all(|w| w[0].overall >= w[1].overall));
        let start = a.desirability(&a.predict(a.settings())).1;
        assert!(best[0].overall >= start && best[0].overall > 0.5, "{} vs {start}", best[0].overall);
        // No small step in any factor improves on the best point.
        for j in 0..3 {
            let ProfileFactorKind::Continuous { range: (lo, hi) } = a.factors()[j].kind else { panic!() };
            for step in [-0.01, 0.01] {
                let mut s = best[0].settings.clone();
                let TermValue::Number(x) = s[j] else { panic!() };
                s[j] = TermValue::Number((x + step * (hi - lo)).clamp(lo, hi));
                assert!(a.desirability(&a.predict(&s)).1 <= best[0].overall + 1e-9);
            }
        }
        // Without goals there is nothing to search for.
        let none = MultipleResponsesRequest {
            goals: vec![Some(Desirability::new(Goal::None, 0.0, 1.0)); 3],
            ..request.clone()
        };
        let a = MultipleResponsesAnalysis::run(&t, &RowStateManager::new(t.n_rows()), &none).unwrap();
        assert!(a.best_settings().is_err());
        assert_eq!(a.profiles(a.settings()).unwrap().len(), 3);
    }

    /// One response y = b₀ + b₁x with x ~ N(m, s) and noise σ is normal with
    /// mean b₀ + b₁m and SD √(b₁²s² + σ²): the simulated defect rate must
    /// match the normal tail.
    #[test]
    fn simulated_defect_rate_matches_the_normal_result() {
        let x: Vec<f64> = (0..20).map(|i| f64::from(i) / 2.0).collect();
        let y: Vec<f64> = x.iter().enumerate().map(|(i, v)| 3.0 + 2.0 * v + 0.1 * ((i * 7) % 5) as f64).collect();
        let t = DataTable::new("t", vec![Column::numeric("x", x), Column::numeric("y", y)]).unwrap();
        let (m, s, sigma, usl) = (5.0, 0.5, 0.8, 15.0);
        let request = MultipleResponsesRequest {
            responses: vec![1],
            predictors: vec![0],
            form: ModelForm::MainEffects,
            goals: vec![None],
            settings: vec![FactorSetting::Number(m)],
            simulation: Some(SimulationSetup {
                runs: 200_000,
                seed: 9,
                factors: vec![FactorVariation::Normal { sd: s }],
                noise: vec![sigma],
                limits: vec![(None, Some(usl))],
                region: None,
            }),
            constraints: String::new(),
            selection: None,
        };
        let a = MultipleResponsesAnalysis::run(&t, &RowStateManager::new(20), &request).unwrap();
        let b = a.models()[0].linear().unwrap().fit().coefficients();
        let (mean, sd) = (b[0] + b[1] * m, (b[1] * b[1] * s * s + sigma * sigma).sqrt());
        let p = 1.0 - Normal::new(mean, sd).unwrap().cdf(usl).unwrap();
        let sim = a.simulate(a.settings()).unwrap();
        let rate = sim.defects as f64 / sim.runs as f64;
        let se = (p * (1.0 - p) / sim.runs as f64).sqrt();
        assert!((rate - p).abs() < 4.0 * se, "simulated {rate}, normal {p}");
        assert!((sim.responses[0].mean - mean).abs() < 4.0 * sd / (sim.runs as f64).sqrt());
        assert!((sim.responses[0].sd / sd - 1.0).abs() < 0.01);
        let (_, lo, hi) = wilson_interval(sim.defects, sim.runs);
        assert!(lo <= rate && rate <= hi);
        // Uniform and triangular spreads have SD h/√3 and h/√6.
        for (variation, factor_sd) in [
            (FactorVariation::Uniform { half_width: 1.2 }, 1.2 / 3.0_f64.sqrt()),
            (FactorVariation::Triangular { half_width: 1.2 }, 1.2 / 6.0_f64.sqrt()),
        ] {
            let mut r = request.clone();
            let sim_setup = r.simulation.as_mut().unwrap();
            sim_setup.factors = vec![variation];
            sim_setup.noise = vec![0.0];
            let a = MultipleResponsesAnalysis::run(&t, &RowStateManager::new(20), &r).unwrap();
            let sim = a.simulate(a.settings()).unwrap();
            assert!((sim.responses[0].sd / (b[1] * factor_sd) - 1.0).abs() < 0.01);
        }
        // The robust search moves the setting away from the upper limit.
        let robust = a.robust_settings().unwrap();
        let TermValue::Number(xr) = robust[0] else { panic!() };
        assert!(xr < m, "{xr}");
        let table = a.simulation_table(&sim).unwrap();
        assert_eq!((table.n_rows(), table.n_cols()), (200_000, 2));
    }

    #[test]
    fn sections_and_project_round_trip() {
        let (t, request) = coating();
        let rows = RowStateManager::new(t.n_rows());
        let a = MultipleResponsesAnalysis::run(&t, &rows, &request).unwrap();
        let kinds: Vec<SectionKind> = a.sections().iter().map(|s| s.kind).collect();
        assert_eq!(
            kinds,
            [
                SectionKind::ResponseFits,
                SectionKind::CurrentSettings,
                SectionKind::DesirabilityTable,
                SectionKind::BestSettings,
                SectionKind::SimulationSummary,
                SectionKind::SpecRegion,
                SectionKind::SpecRegionContours,
                SectionKind::SimulatedResponse(0),
                SectionKind::SimulatedResponse(1),
                SectionKind::SimulatedResponse(2),
            ]
        );
        let mut saved = request.clone();
        saved.settings = a.to_request_settings(&a.best_settings().unwrap()[0].settings);
        let report = crate::SavedReport::MultipleResponses { request: Box::new(saved.clone()) };
        let json = crate::project::report_to_json(&report, &t);
        assert_eq!(crate::project::report_from_json(&json, &t).unwrap(), report);
        let again = MultipleResponsesAnalysis::run(&t, &rows, &saved).unwrap();
        assert_eq!(again.settings(), &a.best_settings().unwrap()[0].settings[..]);
        // Older or hand-written requests may leave everything but the columns out.
        let bare = stat_data::JsonValue::Object(vec![
            ("analysis".into(), stat_data::JsonValue::String("several-responses".into())),
            ("responses".into(), stat_data::JsonValue::Array(vec![stat_data::JsonValue::String("Cost".into())])),
            ("predictors".into(), stat_data::JsonValue::Array(vec![stat_data::JsonValue::String("Time (min)".into())])),
        ]);
        let crate::SavedReport::MultipleResponses { request } = crate::project::report_from_json(&bare, &t).unwrap()
        else {
            panic!()
        };
        assert_eq!(request.form, ModelForm::MainEffects);
        assert!(request.simulation.is_none());
    }

    /// y = −(x₁ − 1)² − (x₂ − 1)², maximized: (1, 1) without constraints;
    /// with x₁ + x₂ ≤ 1 the closest point of that half-plane, (0.5, 0.5);
    /// adding x₁ − x₂ ≥ 0.2 moves it to the corner (0.6, 0.4).
    #[test]
    fn best_settings_respect_constraints() {
        let mut x1 = Vec::new();
        let mut x2 = Vec::new();
        for i in 0..5 {
            for j in 0..5 {
                x1.push(f64::from(i) / 4.0);
                x2.push(f64::from(j) / 4.0);
            }
        }
        let y: Vec<f64> = x1.iter().zip(&x2).map(|(a, b)| -(a - 1.0).powi(2) - (b - 1.0).powi(2)).collect();
        let t =
            DataTable::new("t", vec![Column::numeric("x1", x1), Column::numeric("x2", x2), Column::numeric("y", y)])
                .unwrap();
        let rows = RowStateManager::new(25);
        let request = |constraints: &str| MultipleResponsesRequest {
            responses: vec![2],
            predictors: vec![0, 1],
            form: ModelForm::Quadratic,
            goals: vec![Some(Desirability::new(Goal::Maximize, -2.0, 0.0))],
            settings: Vec::new(),
            simulation: None,
            constraints: constraints.to_owned(),
            selection: None,
        };
        let point = |c: &str| {
            let a = MultipleResponsesAnalysis::run(&t, &rows, &request(c)).unwrap();
            let best = a.best_settings().unwrap();
            assert!(best.iter().all(|s| a.satisfies_constraints(&s.settings)));
            let v: Vec<f64> = best[0]
                .settings
                .iter()
                .map(|s| match s {
                    TermValue::Number(x) => *x,
                    TermValue::Level(_) => f64::NAN,
                })
                .collect();
            (v, a)
        };
        let (free, _) = point("");
        assert!((free[0] - 1.0).abs() < 1e-3 && (free[1] - 1.0).abs() < 1e-3, "{free:?}");
        let (one, a) = point("x1 + x2 <= 1");
        assert!((one[0] - 0.5).abs() < 2e-3 && (one[1] - 0.5).abs() < 2e-3, "{one:?}");
        assert_eq!(a.constraint_lines(), ["x1 + x2 <= 1"]);
        assert!(a.sections().iter().any(|s| s.title.contains("within the factor ranges and 1 constraint")));
        let (two, _) = point("x1 + x2 <= 1\nx1 - x2 >= 0.2");
        assert!((two[0] - 0.6).abs() < 2e-3 && (two[1] - 0.4).abs() < 2e-3, "{two:?}");
        // An exclusion keeps the search out of a corner: the best is then on its edge.
        let (ex, _) = point("exclude x1 > 0.5 and x2 > 0.5");
        assert!(ex[0] <= 0.5 + 1e-6 || ex[1] <= 0.5 + 1e-6, "{ex:?}");
        assert!(MultipleResponsesAnalysis::run(&t, &rows, &request("x1 + z <= 1")).is_err());
        // Constraints no setting can meet.
        let none = MultipleResponsesAnalysis::run(&t, &rows, &request("x1 + x2 <= -1")).unwrap();
        assert!(none.best_settings().is_err());
        // Saved with the report.
        let report = crate::SavedReport::MultipleResponses { request: Box::new(request("x1 + x2 <= 1")) };
        let json = crate::project::report_to_json(&report, &t);
        assert_eq!(crate::project::report_from_json(&json, &t).unwrap(), report);
    }

    /// At each corner of the region the probability of meeting every limit
    /// is the product of normal tail probabilities; the simulated share
    /// agrees within sampling error, and the worst corner is the lowest.
    #[test]
    fn specification_region_matches_normal_tails() {
        let n = 25usize;
        let (mut x1, mut x2) = (Vec::new(), Vec::new());
        for i in 0..n {
            x1.push((i % 5) as f64);
            x2.push((i / 5) as f64);
        }
        let y1: Vec<f64> = x1.iter().zip(&x2).map(|(a, b)| 10.0 + 2.0 * a - b).collect();
        let y2: Vec<f64> = x1.iter().zip(&x2).map(|(a, b)| 5.0 + a * 0.5 + b).collect();
        let t = DataTable::new(
            "t",
            vec![
                Column::numeric("x1", x1),
                Column::numeric("x2", x2),
                Column::numeric("y1", y1),
                Column::numeric("y2", y2),
            ],
        )
        .unwrap();
        let (s1, s2) = (0.8, 0.5);
        let limits = vec![(Some(9.0), Some(16.0)), (None, Some(8.0))];
        let request = MultipleResponsesRequest {
            responses: vec![2, 3],
            predictors: vec![0, 1],
            form: ModelForm::MainEffects,
            goals: vec![None, None],
            settings: vec![FactorSetting::Number(2.0), FactorSetting::Number(2.0)],
            simulation: Some(SimulationSetup {
                runs: 5_000,
                seed: 4,
                factors: vec![FactorVariation::Fixed; 2],
                noise: vec![s1, s2],
                limits: limits.clone(),
                region: Some(vec![(1.5, 3.0), (1.0, 2.5)]),
            }),
            constraints: String::new(),
            selection: None,
        };
        let a = MultipleResponsesAnalysis::run(&t, &RowStateManager::new(n), &request).unwrap();
        let check = a.spec_region(a.settings()).unwrap();
        assert_eq!(check.corners.len(), 4);
        assert_eq!(check.varying, [0, 1]);
        let z = Normal::standard();
        let mut lowest = f64::INFINITY;
        for (c, p) in check.corners.iter().enumerate() {
            let (u, v) = (if c & 1 == 1 { 3.0 } else { 1.5 }, if c & 2 == 2 { 2.5 } else { 1.0 });
            assert_eq!(p.settings, vec![TermValue::Number(u), TermValue::Number(v)]);
            let (m1, m2) = (10.0 + 2.0 * u - v, 5.0 + 0.5 * u + v);
            let want =
                (z.cdf((16.0 - m1) / s1).unwrap() - z.cdf((9.0 - m1) / s1).unwrap()) * z.cdf((8.0 - m2) / s2).unwrap();
            assert!((p.probability - want).abs() < 1e-9, "corner {c}: {} vs {want}", p.probability);
            let se = (want * (1.0 - want) / p.runs as f64).sqrt().max(1e-4);
            assert!((p.simulated - want).abs() < 4.0 * se, "corner {c}: simulated {} vs {want}", p.simulated);
            lowest = lowest.min(want);
        }
        assert!((check.corners[check.worst].probability - lowest).abs() < 1e-12);
        let sim = a.simulate(a.settings()).unwrap();
        let sections = a.simulation_sections(&sim);
        let table = sections.iter().find(|s| s.kind == SectionKind::SpecRegion).unwrap();
        let SectionContent::Table(tt) = &table.content else { panic!() };
        assert_eq!(tt.rows.len(), 5);
        assert!(tt.rows.iter().any(|r| r[0].ends_with("(worst)")));
        let plot = sections.iter().find(|s| s.kind == SectionKind::SpecRegionContours).unwrap();
        let SectionContent::Plot(spec) = &plot.content else { panic!() };
        assert_eq!(spec.line.as_ref().unwrap().len(), 5, "the box");
        assert!(!spec.segments.is_empty(), "limit contours");
        assert_eq!(spec.annotations.as_ref().unwrap().items.iter().filter(|i| i.3 == 1).count(), 1);
        // Saved with the report.
        let report = crate::SavedReport::MultipleResponses { request: Box::new(request.clone()) };
        let json = crate::project::report_to_json(&report, &t);
        assert_eq!(crate::project::report_from_json(&json, &t).unwrap(), report);
        // A region needs a limit and a varying factor.
        let mut r = request.clone();
        r.simulation.as_mut().unwrap().region = Some(vec![(2.0, 2.0), (2.0, 2.0)]);
        let a = MultipleResponsesAnalysis::run(&t, &RowStateManager::new(n), &r).unwrap();
        assert!(a.spec_region(a.settings()).is_err());
        r.simulation.as_mut().unwrap().region = Some(vec![(3.0, 2.0), (2.0, 2.0)]);
        assert!(MultipleResponsesAnalysis::run(&t, &RowStateManager::new(n), &r).is_err());
    }

    #[test]
    fn categorical_factors_are_searched_level_by_level() {
        let n: usize = 24;
        let x: Vec<f64> = (0..n).map(|i| (i % 6) as f64).collect();
        let g: Vec<String> = (0..n).map(|i| ["a", "b"][(i / 6) % 2].to_owned()).collect();
        let y: Vec<f64> = (0..n).map(|i| x[i] + if g[i] == "b" { 3.0 } else { 0.0 } + 0.01 * (i % 3) as f64).collect();
        let t =
            DataTable::new("t", vec![Column::numeric("x", x), Column::text("g", g), Column::numeric("y", y)]).unwrap();
        let request = MultipleResponsesRequest {
            responses: vec![2],
            predictors: vec![0, 1],
            form: ModelForm::MainEffects,
            goals: vec![None],
            settings: vec![FactorSetting::Number(1.0), FactorSetting::Level("a".into())],
            simulation: None,
            constraints: String::new(),
            selection: None,
        };
        let a = MultipleResponsesAnalysis::run(&t, &RowStateManager::new(n), &request).unwrap();
        let best = a.best_settings().unwrap();
        assert_eq!(best[0].settings[1], TermValue::Level(1), "level b is higher");
        assert!(matches!(best[0].settings[0], TermValue::Number(v) if (v - 5.0).abs() < 1e-6));
        assert!(best[0].overall > 0.99, "{}", best[0].overall);
        assert!(
            MultipleResponsesAnalysis::run(
                &t,
                &RowStateManager::new(n),
                &MultipleResponsesRequest { responses: vec![0], ..request.clone() }
            )
            .is_err()
        );
    }

    /// Hald's cement data (Draper & Smith 1998, Appendix 15A) with two more
    /// responses: one driven by x3 alone and one unrelated to the factors.
    fn hald_three() -> DataTable {
        let x3 = [6.0, 15.0, 8.0, 8.0, 6.0, 9.0, 17.0, 22.0, 18.0, 4.0, 23.0, 9.0, 8.0];
        let wobble = [0.3, -0.2, 0.1, -0.4, 0.2, 0.0, -0.1, 0.3, -0.3, 0.1, 0.2, -0.2, 0.0];
        let cols = [
            ("x1", vec![7.0, 1.0, 11.0, 11.0, 7.0, 11.0, 3.0, 1.0, 2.0, 21.0, 1.0, 11.0, 10.0]),
            ("x2", vec![26.0, 29.0, 56.0, 31.0, 52.0, 55.0, 71.0, 31.0, 54.0, 47.0, 40.0, 66.0, 68.0]),
            ("x3", x3.to_vec()),
            ("x4", vec![60.0, 52.0, 20.0, 47.0, 33.0, 22.0, 6.0, 44.0, 22.0, 26.0, 34.0, 12.0, 12.0]),
            ("heat", vec![78.5, 74.3, 104.3, 87.6, 95.9, 109.2, 102.7, 72.5, 93.1, 115.9, 83.8, 113.3, 109.4]),
            ("strength", x3.iter().zip(&wobble).map(|(x, w)| 3.0 + 2.0 * x + w).collect()),
            ("shade", vec![5.1, 4.9, 5.3, 4.8, 5.0, 5.2, 4.7, 5.1, 4.9, 5.0, 5.2, 4.8, 5.0]),
        ];
        DataTable::new("hald", cols.into_iter().map(|(n, v)| Column::numeric(n, v)).collect()).unwrap()
    }

    fn hald_request(selection: Option<TermSelection>) -> MultipleResponsesRequest {
        MultipleResponsesRequest {
            responses: vec![4, 5, 6],
            predictors: vec![0, 1, 2, 3],
            form: ModelForm::MainEffects,
            goals: vec![
                Some(Desirability::new(Goal::Maximize, 70.0, 120.0)),
                Some(Desirability::new(Goal::Target(30.0), 10.0, 50.0)),
                Some(Desirability::new(Goal::Maximize, 4.5, 5.5)),
            ],
            settings: Vec::new(),
            simulation: None,
            constraints: String::new(),
            selection,
        }
    }

    /// Each response gets its own search: heat selects x1 and x2 (Draper &
    /// Smith's mixed search at α = 0.15), strength selects x3 alone, and
    /// shade selects nothing, so it is predicted by its mean with the
    /// t interval. Every response's search and model agree with a separate
    /// stepwise analysis of that response alone.
    #[test]
    fn stepwise_selection_per_response() {
        let t = hald_three();
        let rows = RowStateManager::new(t.n_rows());
        let selection = TermSelection::default();
        let a = MultipleResponsesAnalysis::run(&t, &rows, &hald_request(Some(selection.clone()))).unwrap();
        let chosen: Vec<Vec<String>> = a.searches().iter().map(StepwiseAnalysis::selected_effect_names).collect();
        assert_eq!(chosen, [vec!["x1", "x2"], vec!["x3"], Vec::<&str>::new()]);
        for (i, &response) in [4usize, 5, 6].iter().enumerate() {
            let alone = StepwiseAnalysis::run(
                &t,
                &rows,
                &StepwiseRequest {
                    candidates: LinearModelSpec {
                        response,
                        predictors: vec![0, 1, 2, 3],
                        effects: effects(ModelForm::MainEffects, &[true; 4]),
                        splines: Vec::new(),
                    },
                    direction: selection.direction,
                    criterion: selection.criterion,
                    heredity: selection.heredity,
                },
            )
            .unwrap();
            assert_eq!(alone.result().steps.len(), a.searches()[i].result().steps.len());
            match (alone.selected(), &a.models()[i]) {
                (Some(m), ResponseModel::Fitted(kept)) => {
                    // Same coefficients although the kept model lists every factor.
                    for (x, y) in m.fit().coefficients().iter().zip(kept.fit().coefficients()) {
                        assert!((x - y).abs() < 1e-9 * x.abs().max(1.0), "{x} vs {y}");
                    }
                    assert_eq!(kept.spec().predictors, [0, 1, 2, 3]);
                }
                (None, ResponseModel::MeanOnly(_)) => {}
                other => panic!("response {i}: {other:?}"),
            }
        }
        // Heat ≈ 52.5773 + 1.4683 x1 + 0.6623 x2 (Draper & Smith, eq. 15.2.3).
        let heat = a.models()[0].linear().unwrap().fit().coefficients();
        assert!((heat[0] - 52.577_349).abs() < 1e-5 && (heat[1] - 1.468_306).abs() < 1e-6);
        assert!((heat[2] - 0.662_250).abs() < 1e-6);
        // Shade: the mean with ȳ ± t(0.975; 12) s/√13.
        let ResponseModel::MeanOnly(shade) = &a.models()[2] else { panic!() };
        let y = [5.1, 4.9, 5.3, 4.8, 5.0, 5.2, 4.7, 5.1, 4.9, 5.0, 5.2, 4.8, 5.0];
        let mean = y.iter().sum::<f64>() / 13.0;
        let sd = (y.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / 12.0).sqrt();
        let half = 2.178_812_829_663_418 * sd / 13f64.sqrt();
        let (lo, hi) = shade.confidence_interval();
        assert!((shade.mean() - mean).abs() < 1e-12 && (shade.sd() - sd).abs() < 1e-12);
        assert!((lo - (mean - half)).abs() < 1e-9 && (hi - (mean + half)).abs() < 1e-9);
        // Predictions: heat moves with x1 and x2 only, shade not at all.
        let s = a.settings().to_vec();
        let mut moved = s.clone();
        moved[3] = TermValue::Number(50.0);
        moved[2] = TermValue::Number(20.0);
        let (p0, p1) = (a.predict(&s), a.predict(&moved));
        assert!((p0[0] - p1[0]).abs() < 1e-9 && (p0[2] - p1[2]).abs() < 1e-12);
        assert!((p0[1] - p1[1]).abs() > 1.0, "strength follows x3");
        assert!((p0[2] - mean).abs() < 1e-12);
        // The explorer, desirability and best settings use the selected models.
        let profiles = a.profiles(&s).unwrap();
        assert_eq!(profiles.len(), 4);
        let flat = |p: &Profile, j: usize| match &p.panels[j] {
            ProfilePanel::Continuous { curve, .. } => curve.iter().all(|q| (q[1] - curve[0][1]).abs() < 1e-9),
            ProfilePanel::Categorical { .. } => panic!(),
        };
        assert!(flat(&profiles[0], 2) && flat(&profiles[0], 3) && !flat(&profiles[0], 0));
        assert!((0..4).all(|j| flat(&profiles[2], j)));
        let best = a.best_settings().unwrap();
        assert!((best[0].predicted[1] - 30.0).abs() < 0.05, "strength on target: {:?}", best[0].predicted);
        // Report: a selection summary and one step history per response.
        let sections = a.sections();
        let kinds: Vec<SectionKind> = sections.iter().map(|s| s.kind).collect();
        assert_eq!(
            &kinds[..3],
            &[SectionKind::ResponseFits, SectionKind::TermSelection, SectionKind::StepwiseSettings]
        );
        assert!(kinds.contains(&SectionKind::ResponseStepHistory(2)));
        let SectionContent::Table(sel) = &sections[1].content else { panic!() };
        assert_eq!(sel.rows[2][1], "none (mean only)");
        assert!(a.title().contains("terms selected stepwise"), "{}", a.title());
    }

    #[test]
    fn term_selection_round_trips_and_validates() {
        let t = hald_three();
        let rows = RowStateManager::new(t.n_rows());
        let request = hald_request(Some(TermSelection {
            direction: StepDirection::Backward,
            criterion: StepCriterion::Bic,
            heredity: Heredity::Weak,
            candidates: vec![vec![(0, 1)], vec![(1, 1)], vec![(0, 1), (1, 1)]],
        }));
        let json = object(request_members_to_json(&request));
        let back = request_from_json(&json, request.responses.clone(), request.predictors.clone()).unwrap();
        assert_eq!(back, request);
        let a = MultipleResponsesAnalysis::run(&t, &rows, &request).unwrap();
        assert!(a.title().contains("from 3 candidates"), "{}", a.title());
        let bad = hald_request(Some(TermSelection { candidates: vec![vec![(7, 1)]], ..TermSelection::default() }));
        assert!(MultipleResponsesAnalysis::run(&t, &rows, &bad).is_err());
        // Nothing selected anywhere: no model to explore.
        let strict = MultipleResponsesRequest {
            responses: vec![6],
            goals: vec![None],
            ..hald_request(Some(TermSelection {
                direction: StepDirection::Forward,
                criterion: StepCriterion::PValue { enter: 1e-9, remove: 1e-9 },
                ..TermSelection::default()
            }))
        };
        let e = MultipleResponsesAnalysis::run(&t, &rows, &strict).unwrap_err().to_string();
        assert!(e.contains("no term was selected"), "{e}");
    }
}
