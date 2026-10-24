//! Nonlinear regression bound to the row state ([`stat_linear::nonlinear`]).
//!
//! The model is one of:
//! * a library form in one predictor (exponential, logistic, dose–response,
//!   growth, segmented line, peaks…, with automatic starting values);
//! * a model formula with named parameters, written in the formula language
//!   ([`crate::formula`]) with the table's columns and the parameters as
//!   variables (a formula in two columns is a surface `z = f(x, y)`);
//! * a differential equation `dy/dx = g(x, y; θ)` with `y(x₀)` a parameter,
//!   solved at the observed `x` for each evaluation ([`stat_core::ode`]);
//! * an implicit curve `F(x, y; θ) = 0` fitted by orthogonal distance
//!   ([`stat_linear::nonlinear::implicit`]).
//!
//! Options: a data-set column, with each parameter shared by every data set
//! or fitted to each ([`stat_linear::nonlinear::global`]); weights (`1/Y²`,
//! `1/Y` or a weight column); least squares, a robust (Cauchy-loss) fit, or
//! least squares after removing the outliers the robust fit identifies
//! ([`stat_linear::nonlinear::robust`]); and comparisons by the extra
//! sum-of-squares F test and AICc ([`stat_linear::nonlinear::compare`]):
//! one curve for all data sets, each unshared parameter made common, and an
//! alternative library form. Rows with a response but no predictor value are
//! read off the fitted curve (inverse prediction,
//! [`stat_linear::nonlinear::inverse`]).
//!
//! Excluded rows and rows missing the response, a predictor the model reads,
//! the data set or a valid weight are left out of the fit; predictions are
//! saved for every row the model can be computed on.
//!
//! Project-file form (columns by name; a missing setting keeps its default):
//!
//! ```text
//! {"analysis": "nonlinear-regression", "response": "Rate", "form": "michaelis_menten",
//!  "predictor": "Conc", "parameters": [{"name": "Vmax", "start": 200, "lower": 0, "shared": true}],
//!  "level": 0.95, "profile": true, "max_iterations": 200, "group": "Batch",
//!  "weighting": "inverse_y2", "method": "outliers", "q": 0.01, "compare": "logistic3"}
//! {"analysis": "nonlinear-regression", "response": "Rate", "formula": "Vmax * Conc / (Km + Conc)",
//!  "parameters": [{"name": "Vmax", "start": 200}, {"name": "Km", "start": 0.1}]}
//! {"analysis": "nonlinear-regression", "response": "C", "rate": "-k * C", "predictor": "Time",
//!  "initial": "C0", "start_x": 0, "parameters": [{"name": "k", "start": 0.1}, {"name": "C0", "start": 10}]}
//! {"analysis": "nonlinear-regression", "response": "Y", "equation": "(X - a)**2 + (Y - b)**2 - r**2",
//!  "parameters": [...]}
//! ```
//! `weighting` is `"equal"`, `"inverse_y2"`, `"inverse_y"` or a weight column
//! as `{"column": "W"}`; `method` is `"least_squares"`, `"robust"` or
//! `"outliers"`.

use crate::curve_templates::TemplateSettings;
use crate::format::{format_number, format_p_value};
use crate::formula::{ParameterFormula, column_reference, compile_with_parameters, references};
use crate::project::{SavedReport, invalid, member, s};
use crate::report::{
    ContourMap, PlotSpec, PointGroups, Polyline, ReportSection, RowPoint, SectionContent, SectionKind, TextTable,
};
use crate::report_columns::ReportColumn;
use crate::surface_analysis::contour_lines;
use crate::surface_profiler::{Limited, Observation, ProfileFactor, ProfileFactorKind, ProfilerOptions, SurfaceModel};
use stat_core::Result;
use stat_core::ode::{OdeOptions, solve_initial_value};
use stat_core::optimize::{BoundState, ResidualFn, numeric_jacobian};
use stat_data::{Column, DataTable, JsonValue, RowStateManager};
use stat_linear::TermValue;
use stat_linear::nonlinear::compare::{FitScore, aicc_probability, extra_sum_of_squares};
pub use stat_linear::nonlinear::forms::CurveForm;
use stat_linear::nonlinear::forms::FormModel;
use stat_linear::nonlinear::global::{GlobalModel, layout};
use stat_linear::nonlinear::implicit::ImplicitModel;
use stat_linear::nonlinear::inverse::{InversePrediction, inverse_predict};
use stat_linear::nonlinear::robust::{fit_robust, outliers};
use stat_linear::nonlinear::{ModelFunction, NonlinearFit, NonlinearOptions, fit_nonlinear};

mod templates;

const DIGITS: usize = 6;
const CURVE_POINTS: usize = 150;
const SURFACE_GRID: usize = 40;
const IMPLICIT_GRID: usize = 120;

/// A predicted value with its confidence band and prediction limits.
type BandPoint = (f64, (f64, f64), (f64, f64));
/// A fitted surface on a grid: x values, y values and the values by row of y.
type SurfaceGrid = (Vec<f64>, Vec<f64>, Vec<Vec<f64>>);

/// The model of a nonlinear regression.
#[derive(Debug, Clone, PartialEq)]
pub enum NonlinearModel {
    /// A library form in one predictor column.
    Library {
        /// The form.
        form: CurveForm,
        /// Predictor column.
        predictor: usize,
    },
    /// A model formula with named parameters.
    Formula {
        /// Formula text (columns and parameters as variables).
        formula: String,
        /// The columns the formula reads (kept so the column cannot be
        /// deleted while the report is open).
        columns: Vec<usize>,
    },
    /// A differential equation `d(response)/d(predictor) = rate`, whose
    /// solution starts from parameter `initial` at `start` (the smallest
    /// predictor value used when `None`).
    Differential {
        /// Rate formula in the predictor, the response and the parameters.
        rate: String,
        /// Predictor column (the variable of integration).
        predictor: usize,
        /// Name of the parameter that is the response's value at the start.
        initial: String,
        /// Predictor value where the solution starts.
        start: Option<f64>,
    },
    /// An implicit curve `equation = 0` in the response and one other column,
    /// fitted by orthogonal distance.
    Implicit {
        /// Equation formula (the curve is where it equals zero).
        equation: String,
        /// The columns the equation reads.
        columns: Vec<usize>,
    },
}

/// Name, starting value, bounds and sharing of one parameter.
#[derive(Debug, Clone, PartialEq)]
pub struct ParameterSetting {
    /// Name (an identifier).
    pub name: String,
    /// Starting value; `None` for the automatic one (library forms only).
    pub start: Option<f64>,
    /// Lower bound, if any.
    pub lower: Option<f64>,
    /// Upper bound, if any.
    pub upper: Option<f64>,
    /// With a data-set column: one value for every data set (`true`) or one
    /// per data set.
    pub shared: bool,
}

impl ParameterSetting {
    /// An unbounded, shared parameter.
    #[must_use]
    pub fn new(name: impl Into<String>, start: Option<f64>) -> Self {
        Self { name: name.into(), start, lower: None, upper: None, shared: true }
    }
}

/// Observation weights.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Weighting {
    /// Equal weights (ordinary least squares).
    Equal,
    /// `1/Y²`: constant relative (percentage) error.
    InverseResponseSquared,
    /// `1/Y`: variance proportional to the mean, as for Poisson counts.
    InverseResponse,
    /// Weights from a column (proportional to 1/variance).
    Column(usize),
}

impl Weighting {
    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Equal => "Equal weights",
            Self::InverseResponseSquared => "1/Y² (relative error)",
            Self::InverseResponse => "1/Y (Poisson)",
            Self::Column(_) => "Weight column",
        }
    }
}

/// How the parameters are estimated.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FitMethod {
    /// Least squares.
    LeastSquares,
    /// Robust fit with the Cauchy (Lorentzian) loss.
    Robust,
    /// Least squares after removing the outliers of a robust fit, found by
    /// the false-discovery-rate rule at level `q`.
    RemoveOutliers {
        /// Q, the false discovery rate (0.01 recommended).
        q: f64,
    },
}

impl FitMethod {
    /// Display name.
    #[must_use]
    pub fn label(self) -> String {
        match self {
            Self::LeastSquares => "Least squares".to_owned(),
            Self::Robust => "Robust (Cauchy loss)".to_owned(),
            Self::RemoveOutliers { q } => {
                format!("Least squares after removing outliers (Q = {}%)", format_number(100.0 * q, 4))
            }
        }
    }
}

/// What to fit.
#[derive(Debug, Clone, PartialEq)]
pub struct NonlinearRequest {
    /// Response column.
    pub response: usize,
    /// The model.
    pub model: NonlinearModel,
    /// One entry per parameter, in model order.
    pub parameters: Vec<ParameterSetting>,
    /// Confidence level of the intervals and bands.
    pub confidence_level: f64,
    /// Compute profile t intervals.
    pub profile_intervals: bool,
    /// Iteration limit.
    pub max_iterations: usize,
    /// Data-set column for a global fit (`None`: one data set).
    pub group: Option<usize>,
    /// Observation weights.
    pub weighting: Weighting,
    /// Estimation method.
    pub method: FitMethod,
    /// An alternative library form to compare with (library models with one
    /// data set).
    pub compare_with: Option<CurveForm>,
    /// The assay curve template the request was made from, if any: the
    /// report adds the template's derived quantities and sections
    /// ([`crate::curve_templates`]).
    pub template: Option<Box<TemplateSettings>>,
}

impl NonlinearRequest {
    /// Default iteration limit.
    pub const DEFAULT_ITERATIONS: usize = 200;

    fn with_model(response: usize, model: NonlinearModel, parameters: Vec<ParameterSetting>) -> Self {
        Self {
            response,
            model,
            parameters,
            confidence_level: 0.95,
            profile_intervals: true,
            max_iterations: Self::DEFAULT_ITERATIONS,
            group: None,
            weighting: Weighting::Equal,
            method: FitMethod::LeastSquares,
            compare_with: None,
            template: None,
        }
    }

    /// Library form `form` of `response` on `predictor`, automatic starting
    /// values, no bounds, 95% intervals with profile intervals.
    #[must_use]
    pub fn library(response: usize, predictor: usize, form: CurveForm) -> Self {
        Self::with_model(
            response,
            NonlinearModel::Library { form, predictor },
            form.parameter_names().iter().map(|n| ParameterSetting::new(*n, None)).collect(),
        )
    }

    /// Model formula `formula` of `response` with parameters
    /// `(name, starting value)`.
    #[must_use]
    pub fn formula(table: &DataTable, response: usize, formula: &str, parameters: &[(&str, f64)]) -> Self {
        Self::with_model(
            response,
            NonlinearModel::Formula { formula: formula.to_owned(), columns: references(table, formula) },
            parameters.iter().map(|(n, v)| ParameterSetting::new(*n, Some(*v))).collect(),
        )
    }

    /// Differential equation `d(response)/d(predictor) = rate` with
    /// parameters `(name, starting value)`, one of which, `initial`, is the
    /// response at the smallest predictor value. Profile intervals are off
    /// (each needs many solutions of the equation).
    #[must_use]
    pub fn differential(
        response: usize,
        predictor: usize,
        rate: &str,
        initial: &str,
        parameters: &[(&str, f64)],
    ) -> Self {
        let mut r = Self::with_model(
            response,
            NonlinearModel::Differential { rate: rate.to_owned(), predictor, initial: initial.to_owned(), start: None },
            parameters.iter().map(|(n, v)| ParameterSetting::new(*n, Some(*v))).collect(),
        );
        r.profile_intervals = false;
        r
    }

    /// Implicit curve `equation = 0` in `response` and other columns, with
    /// parameters `(name, starting value)`.
    #[must_use]
    pub fn implicit(table: &DataTable, response: usize, equation: &str, parameters: &[(&str, f64)]) -> Self {
        let columns = references(table, equation).into_iter().filter(|&c| c != response).collect();
        Self::with_model(
            response,
            NonlinearModel::Implicit { equation: equation.to_owned(), columns },
            parameters.iter().map(|(n, v)| ParameterSetting::new(*n, Some(*v))).collect(),
        )
    }

    /// Every column the request reads.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        let mut c = vec![self.response];
        match &self.model {
            NonlinearModel::Library { predictor, .. } | NonlinearModel::Differential { predictor, .. } => {
                c.push(*predictor);
            }
            NonlinearModel::Formula { columns, .. } | NonlinearModel::Implicit { columns, .. } => {
                c.extend(columns.iter().copied());
            }
        }
        c.extend(self.group);
        if let Weighting::Column(w) = self.weighting {
            c.push(w);
        }
        if let Some(t) = &self.template {
            for j in t.columns() {
                if !c.contains(&j) {
                    c.push(j);
                }
            }
        }
        c
    }

    /// Pass every column index through `f`.
    pub fn map_columns(&mut self, f: &impl Fn(usize) -> usize) {
        self.response = f(self.response);
        match &mut self.model {
            NonlinearModel::Library { predictor, .. } | NonlinearModel::Differential { predictor, .. } => {
                *predictor = f(*predictor);
            }
            NonlinearModel::Formula { columns, .. } | NonlinearModel::Implicit { columns, .. } => {
                columns.iter_mut().for_each(|c| *c = f(*c));
            }
        }
        self.group = self.group.map(f);
        if let Weighting::Column(w) = &mut self.weighting {
            *w = f(*w);
        }
        if let Some(t) = &mut self.template {
            t.map_columns(f);
        }
    }
}

/// A [`ModelFunction`] that evaluates a model formula on table rows.
struct FormulaModel<'a> {
    formula: ParameterFormula,
    table: &'a DataTable,
    rows: Vec<usize>,
    /// First evaluation failure (for the error message).
    failure: Option<(usize, String)>,
}

impl ModelFunction for FormulaModel<'_> {
    fn n_observations(&self) -> usize {
        self.rows.len()
    }
    fn values(&mut self, theta: &[f64], out: &mut [f64]) -> bool {
        match self.formula.evaluate(self.table, &self.rows, theta, out) {
            Ok(()) => out.iter().all(|v| v.is_finite()),
            Err(e) => {
                self.failure.get_or_insert(e);
                false
            }
        }
    }
}

/// A differential-equation model: the solution at fixed predictor values.
struct OdeModel<'a> {
    rate: ParameterFormula,
    mini: &'a DataTable,
    /// Predictor and response columns of `mini`.
    columns: (usize, usize),
    initial: usize,
    x0: f64,
    x: Vec<f64>,
    failure: Option<String>,
}

impl OdeModel<'_> {
    fn solve(&mut self, theta: &[f64], xs: &[f64]) -> std::result::Result<Vec<f64>, String> {
        let (pc, rc) = self.columns;
        let (rate, mini) = (&mut self.rate, self.mini);
        let mut failure = None;
        let mut rhs = |x: f64, y: &[f64], dy: &mut [f64]| {
            let value_of = |j: usize| {
                if j == pc {
                    Some(x)
                } else if j == rc {
                    Some(y[0])
                } else {
                    None
                }
            };
            match rate.evaluate_with(mini, 0, &value_of, theta) {
                Ok(v) => {
                    dy[0] = v;
                    true
                }
                Err(e) => {
                    failure.get_or_insert(e);
                    false
                }
            }
        };
        match solve_initial_value(&mut rhs, self.x0, &[theta[self.initial]], xs, &OdeOptions::default()) {
            Ok(sol) => Ok(sol.values.into_iter().map(|v| v[0]).collect()),
            Err(e) => Err(failure.map_or_else(|| e.to_string(), |f| format!("{e} ({f})"))),
        }
    }
}

impl ModelFunction for OdeModel<'_> {
    fn n_observations(&self) -> usize {
        self.x.len()
    }
    fn values(&mut self, theta: &[f64], out: &mut [f64]) -> bool {
        let xs = std::mem::take(&mut self.x);
        let result = self.solve(theta, &xs);
        self.x = xs;
        match result {
            Ok(v) => {
                out.copy_from_slice(&v);
                true
            }
            Err(e) => {
                self.failure.get_or_insert(e);
                false
            }
        }
    }
}

/// The model, resolved against the table.
#[derive(Debug, Clone)]
enum Kind {
    Library {
        form: CurveForm,
    },
    Formula {
        text: String,
    },
    /// Columns are indices in the mini table.
    Ode {
        text: String,
        predictor: usize,
        response: usize,
        initial: usize,
        x0: f64,
    },
    Implicit {
        text: String,
        x: usize,
        y: usize,
    },
}

/// What a fit produced.
#[derive(Debug, Clone)]
struct FitOutcome {
    fit: NonlinearFit,
    /// `map[g][k]`: global index of data set `g`'s parameter `k`.
    map: Vec<Vec<usize>>,
    /// Owner `(k, data set)` of each global parameter.
    owner: Vec<(usize, Option<usize>)>,
    /// Positions (into the used rows) fitted.
    positions: Vec<usize>,
    /// Robust scale, for a robust fit.
    robust_scale: Option<f64>,
}

/// The data and model of a fit, from which fits of subsets, other sharing
/// and other forms are made.
#[derive(Clone)]
struct Setup<'t> {
    table: &'t DataTable,
    kind: Kind,
    /// One-row table of the model's columns (differential and implicit
    /// models are evaluated on it).
    mini: DataTable,
    settings: Vec<ParameterSetting>,
    /// Data-set labels (one, empty, without a data-set column).
    groups: Vec<String>,
    /// Used rows, ordered by data set.
    rows: Vec<usize>,
    group_of: Vec<usize>,
    /// Predictor (library and differential), horizontal column (implicit).
    x: Vec<f64>,
    y: Vec<f64>,
    /// Weights (empty for equal).
    w: Vec<f64>,
    max_iterations: usize,
    confidence_level: f64,
}

impl Setup<'_> {
    fn names(&self, owner: &[(usize, Option<usize>)]) -> Vec<String> {
        owner
            .iter()
            .map(|&(k, g)| match g {
                Some(g) if self.groups.len() > 1 => format!("{} ({})", self.settings[k].name, self.groups[g]),
                _ => self.settings[k].name.clone(),
            })
            .collect()
    }

    /// Positions of `positions` in data set `g`.
    fn of_group(&self, positions: &[usize], g: usize) -> Vec<usize> {
        positions.iter().copied().filter(|&p| self.group_of[p] == g).collect()
    }

    /// Starting values of the global parameters `owner`, from a related fit
    /// `hint` when given.
    fn starts(
        &self,
        positions: &[usize],
        owner: &[(usize, Option<usize>)],
        hint: Option<&FitOutcome>,
    ) -> Result<Vec<f64>> {
        let k = self.settings.len();
        let auto = self.settings.iter().any(|p| p.start.is_none());
        let pooled: Vec<f64> = match (&self.kind, auto) {
            (Kind::Library { form }, true) => {
                let x: Vec<f64> = positions.iter().map(|&p| self.x[p]).collect();
                let y: Vec<f64> = positions.iter().map(|&p| self.y[p]).collect();
                form.starting_values(&x, &y)?
            }
            (Kind::Library { form }, false) => {
                form.check(&positions.iter().map(|&p| self.x[p]).collect::<Vec<_>>())?;
                vec![0.0; k]
            }
            _ => vec![0.0; k],
        };
        let mut per_group: Vec<Option<Vec<f64>>> = vec![None; self.groups.len()];
        if let (Kind::Library { form }, true) = (&self.kind, auto) {
            if self.groups.len() > 1 {
                for (g, slot) in per_group.iter_mut().enumerate() {
                    let pos = self.of_group(positions, g);
                    let x: Vec<f64> = pos.iter().map(|&p| self.x[p]).collect();
                    let y: Vec<f64> = pos.iter().map(|&p| self.y[p]).collect();
                    *slot = form.starting_values(&x, &y).ok();
                }
            }
        }
        let mut out = Vec::with_capacity(owner.len());
        for &(j, g) in owner {
            let p = &self.settings[j];
            let from_hint = hint.and_then(|h| {
                let est = h.fit.estimates();
                if let Some(i) = h.owner.iter().position(|&o| o == (j, g)) {
                    return Some(est[i]);
                }
                if g.is_some() {
                    return h.owner.iter().position(|&o| o == (j, None)).map(|i| est[i]);
                }
                let copies: Vec<f64> =
                    h.owner.iter().enumerate().filter(|(_, o)| o.0 == j).map(|(i, _)| est[i]).collect();
                (!copies.is_empty()).then(|| copies.iter().sum::<f64>() / copies.len() as f64)
            });
            let value = match (from_hint, p.start) {
                (Some(v), _) => v,
                (None, Some(v)) => v,
                (None, None) => {
                    let v = g.and_then(|g| per_group[g].as_ref().map(|s| s[j])).unwrap_or(pooled[j]);
                    v.clamp(p.lower.unwrap_or(f64::NEG_INFINITY), p.upper.unwrap_or(f64::INFINITY))
                }
            };
            if !value.is_finite() {
                return Err(invalid(format!("enter a starting value for {}", p.name)));
            }
            out.push(value);
        }
        Ok(out)
    }

    /// Fit the rows at `positions` with sharing `shared`.
    fn fit(
        &self,
        positions: &[usize],
        shared: &[bool],
        hint: Option<&FitOutcome>,
        robust: bool,
        inference: bool,
    ) -> Result<FitOutcome> {
        let n_groups = self.groups.len();
        let shared: Vec<bool> = if n_groups == 1 { vec![true; shared.len()] } else { shared.to_vec() };
        let (map, owner) = layout(&shared, n_groups);
        let start = self.starts(positions, &owner, hint)?;
        let names = self.names(&owner);
        let mut options = NonlinearOptions::new(names);
        options.confidence_level = self.confidence_level;
        options.max_iterations = self.max_iterations;
        options.profile_intervals = inference;
        options.skewness = inference;
        if self.settings.iter().any(|p| p.lower.is_some() || p.upper.is_some()) {
            options.lower = owner.iter().map(|&(k, _)| self.settings[k].lower.unwrap_or(f64::NEG_INFINITY)).collect();
            options.upper = owner.iter().map(|&(k, _)| self.settings[k].upper.unwrap_or(f64::INFINITY)).collect();
        }
        // Positions in data-set order (the global model stacks data sets).
        let groups: Vec<Vec<usize>> = (0..n_groups).map(|g| self.of_group(positions, g)).collect();
        let ordered: Vec<usize> = groups.iter().flatten().copied().collect();
        if !self.w.is_empty() {
            options.weights = ordered.iter().map(|&p| self.w[p]).collect();
        }
        let y: Vec<f64> = match self.kind {
            Kind::Implicit { .. } => vec![0.0; ordered.len()],
            _ => ordered.iter().map(|&p| self.y[p]).collect(),
        };
        let xs: Vec<Vec<f64>> = groups.iter().map(|g| g.iter().map(|&p| self.x[p]).collect()).collect();
        let ys: Vec<Vec<f64>> = groups.iter().map(|g| g.iter().map(|&p| self.y[p]).collect()).collect();
        let mut parts: Vec<Box<dyn ModelFunction + '_>> = Vec::with_capacity(n_groups);
        let names: Vec<String> = self.settings.iter().map(|p| p.name.clone()).collect();
        for g in 0..n_groups {
            match &self.kind {
                Kind::Library { form } => parts.push(Box::new(FormModel { form: *form, x: &xs[g] })),
                Kind::Formula { text } => {
                    let compiled = compile_with_parameters(self.table, text, &names)
                        .map_err(|e| invalid(format!("model formula: {e}")))?;
                    let rows = groups[g].iter().map(|&p| self.rows[p]).collect();
                    parts.push(Box::new(FormulaModel { formula: compiled, table: self.table, rows, failure: None }));
                }
                Kind::Ode { text, predictor, response, initial, x0 } => {
                    let rate = compile_with_parameters(&self.mini, text, &names)
                        .map_err(|e| invalid(format!("rate formula: {e}")))?;
                    parts.push(Box::new(OdeModel {
                        rate,
                        mini: &self.mini,
                        columns: (*predictor, *response),
                        initial: *initial,
                        x0: *x0,
                        x: xs[g].clone(),
                        failure: None,
                    }));
                }
                Kind::Implicit { text, x, y } => {
                    let mut eq = compile_with_parameters(&self.mini, text, &names)
                        .map_err(|e| invalid(format!("equation: {e}")))?;
                    let (xc, yc) = (*x, *y);
                    let mini = &self.mini;
                    let f = move |_: usize, a: f64, b: f64, t: &[f64]| {
                        let value_of = |j: usize| {
                            if j == xc {
                                Some(a)
                            } else if j == yc {
                                Some(b)
                            } else {
                                None
                            }
                        };
                        eq.evaluate_with(mini, 0, &value_of, t).ok()
                    };
                    parts.push(Box::new(ImplicitModel::new(Box::new(f), &xs[g], &ys[g])));
                }
            }
        }
        let mut model = GlobalModel::new(parts, map.clone(), owner.len())?;
        let (fit, robust_scale) = if robust {
            let r = fit_robust(&mut model, &y, &start, &options)?;
            (r.fit, Some(r.scale))
        } else {
            (fit_nonlinear(&mut model, &y, &start, &options).map_err(|e| self.explain(e))?, None)
        };
        Ok(FitOutcome { fit, map, owner, positions: ordered, robust_scale })
    }

    fn explain(&self, e: stat_core::StatError) -> stat_core::StatError {
        if e.to_string().contains("starting values") {
            let what = match self.kind {
                Kind::Library { .. } => return e,
                Kind::Formula { .. } => "model formula",
                Kind::Ode { .. } => "differential equation",
                Kind::Implicit { .. } => "equation",
            };
            invalid(format!("the {what} cannot be computed at the starting values: {e}"))
        } else {
            e
        }
    }
}

/// One row of the comparison table.
#[derive(Debug, Clone)]
struct Comparison {
    label: String,
    simple: String,
    complex: String,
    outcome: std::result::Result<(FitScore, FitScore), String>,
}

/// A formula model compiled for one set of points, reused while the points
/// stay the same (as in the derivatives of [`Predictor::with_limits`]).
#[derive(Default)]
struct Evaluator {
    cache: Option<Compiled>,
}

struct Compiled {
    points: Vec<Vec<f64>>,
    table: DataTable,
    formula: ParameterFormula,
}

/// The fitted model as a function of its varied columns, for curves,
/// predictions and profilers.
#[derive(Debug, Clone)]
struct Predictor {
    kind: Kind,
    /// Model columns varied by curves and profilers (numeric; table indices).
    columns: Vec<usize>,
    names: Vec<String>,
    /// One-row table of every column the model reads (text columns hold the
    /// first used row's text).
    mini: DataTable,
    /// The varied columns' indices in `mini`.
    mini_columns: Vec<usize>,
    parameter_names: Vec<String>,
    map: Vec<Vec<usize>>,
}

impl Predictor {
    /// Model values for data set `g` at `points` (one value per varied
    /// column each) with global parameters `theta`; `None` where the model
    /// cannot be computed.
    fn values(&self, g: usize, points: &[Vec<f64>], theta: &[f64]) -> Option<Vec<f64>> {
        let mut ev = self.evaluator();
        self.values_with(&mut ev, g, points, theta)
    }

    /// An empty compiled-model cache for [`Self::values_with`].
    fn evaluator(&self) -> Evaluator {
        Evaluator::default()
    }

    /// [`Self::values`] with the model compiled by [`Self::evaluator`].
    fn values_with(&self, ev: &mut Evaluator, g: usize, points: &[Vec<f64>], theta: &[f64]) -> Option<Vec<f64>> {
        let local: Vec<f64> = self.map[g].iter().map(|&i| theta[i]).collect();
        match &self.kind {
            Kind::Library { form } => Some(points.iter().map(|p| form.value(p[0], &local)).collect()),
            Kind::Formula { text } => {
                // The formula reads columns through `col()` from the table it
                // was compiled for, so it is compiled for the points' table,
                // once for repeated calls at the same points.
                if ev.cache.as_ref().is_none_or(|c| c.points != points) {
                    let n = points.len();
                    let columns: Vec<Column> = self
                        .mini
                        .columns()
                        .iter()
                        .enumerate()
                        .map(|(j, c)| match self.mini_columns.iter().position(|&m| m == j) {
                            Some(k) => Column::numeric(c.name.clone(), points.iter().map(|p| p[k]).collect()),
                            None => match c.as_numeric() {
                                Some(v) => Column::numeric(c.name.clone(), vec![v[0]; n]),
                                None => Column::text(c.name.clone(), vec![c.level_text(0); n]),
                            },
                        })
                        .collect();
                    let table = DataTable::new("grid", columns).ok()?;
                    let formula = compile_with_parameters(&table, text, &self.parameter_names).ok()?;
                    ev.cache = Some(Compiled { points: points.to_vec(), table, formula });
                }
                let c = ev.cache.as_mut()?;
                let rows: Vec<usize> = (0..points.len()).collect();
                let mut out = vec![0.0; points.len()];
                c.formula.evaluate(&c.table, &rows, &local, &mut out).ok()?;
                Some(out)
            }
            Kind::Ode { text, predictor, response, initial, x0 } => {
                let rate = compile_with_parameters(&self.mini, text, &self.parameter_names).ok()?;
                let mut m = OdeModel {
                    rate,
                    mini: &self.mini,
                    columns: (*predictor, *response),
                    initial: *initial,
                    x0: *x0,
                    x: Vec::new(),
                    failure: None,
                };
                let xs: Vec<f64> = points.iter().map(|p| p[0]).collect();
                m.solve(&local, &xs).ok()
            }
            Kind::Implicit { .. } => None,
        }
    }

    /// Values with confidence and prediction limits for data set `g`.
    fn with_limits(
        &self,
        fit: &NonlinearFit,
        g: usize,
        points: &[Vec<f64>],
        weight: &dyn Fn(f64) -> f64,
    ) -> Option<Vec<BandPoint>> {
        let mut ev = self.evaluator();
        self.with_limits_with(&mut ev, fit, g, points, weight)
    }

    /// [`Self::with_limits`] with the model compiled by [`Self::evaluator`].
    fn with_limits_with(
        &self,
        ev: &mut Evaluator,
        fit: &NonlinearFit,
        g: usize,
        points: &[Vec<f64>],
        weight: &dyn Fn(f64) -> f64,
    ) -> Option<Vec<BandPoint>> {
        let theta = fit.estimates();
        let values = self.values_with(ev, g, points, &theta)?;
        let mut problem =
            ResidualFn::new(points.len(), |t: &[f64], out: &mut [f64]| match self.values_with(ev, g, points, t) {
                Some(v) => {
                    out.copy_from_slice(&v);
                    v.iter().all(|x| x.is_finite())
                }
                None => false,
            });
        let mut jac = stat_core::Matrix::zeros(points.len(), theta.len());
        numeric_jacobian(&mut problem, &theta, &values, &[], &[], &mut jac)?;
        let mut out = Vec::with_capacity(points.len());
        for (i, &v) in values.iter().enumerate() {
            let grad = jac.row(i);
            let mean = fit.mean_interval(v, &grad).ok()?;
            let pred = fit.prediction_interval_weighted(v, &grad, weight(v)).ok()?;
            out.push((v, mean, pred));
        }
        Some(out)
    }
}

/// A computed nonlinear regression.
#[derive(Debug, Clone)]
pub struct NonlinearAnalysis {
    request: NonlinearRequest,
    response_name: String,
    /// Model text with parameter names (for display).
    model_text: String,
    /// Predictor name when the model has one predictor.
    x_name: Option<String>,
    group_name: Option<String>,
    groups: Vec<String>,
    outcome: FitOutcome,
    starts: Vec<f64>,
    automatic_start: Vec<bool>,
    /// Rows used in the fit, in the fit's order.
    rows_used: Vec<usize>,
    /// Data set of each used row.
    group_used: Vec<usize>,
    /// Horizontal value of each used row (one-predictor and implicit models).
    x_used: Vec<f64>,
    y_used: Vec<f64>,
    /// Predicted value of every table row (NaN where it cannot be computed).
    predicted_all: Vec<f64>,
    /// Signed distance to the curve of each used row (implicit models).
    distances: Vec<f64>,
    /// Fitted curve with confidence and prediction limits (one predictor,
    /// one data set).
    curve: Option<Curve>,
    /// Curve of each data set (several data sets).
    group_curves: Vec<Polyline>,
    /// Implicit curve polylines.
    implicit_lines: Vec<Polyline>,
    /// Fitted surface over two columns: grid columns, rows and values.
    surface: Option<SurfaceGrid>,
    formula: std::result::Result<String, String>,
    predictor: Predictor,
    /// Ranges and means of the varied columns over the used rows.
    factor_ranges: Vec<(f64, f64, f64)>,
    comparisons: Vec<Comparison>,
    comparison_note: Option<String>,
    /// Rows removed as outliers: row, predictor (NaN without one), response
    /// and robust-fit residual.
    outliers: Vec<(usize, f64, f64, f64)>,
    /// Values of the varied columns of each used row.
    observed_settings: Vec<Vec<f64>>,
    /// `(row, data set, prediction)` for rows with a response but no
    /// predictor.
    inverse: Vec<(usize, usize, InversePrediction)>,
    n_excluded: usize,
    n_missing: usize,
    /// Names of the table's columns (assay templates).
    column_names: Vec<String>,
    /// Template x column, second column (as text) and dilution factor of
    /// every table row (empty without a template).
    x_values: Vec<f64>,
    second_text: Vec<String>,
    dilution_values: Vec<f64>,
    template_extras: templates::TemplateExtras,
}

#[derive(Debug, Clone)]
struct Curve {
    line: Vec<[f64; 2]>,
    mean: (Vec<[f64; 2]>, Vec<[f64; 2]>),
    prediction: (Vec<[f64; 2]>, Vec<[f64; 2]>),
}

fn section(kind: SectionKind, title: impl Into<String>, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.into(), content }
}

/// A number literal that the formula language reads as a float.
fn float_literal(x: f64) -> String {
    let t = crate::script_text::number(x);
    if t.chars().all(|c| c.is_ascii_digit() || c == '-') { format!("{t}.0") } else { t }
}

fn percent(p: f64) -> String {
    format_number(100.0 * p, 4)
}

fn grid(lo: f64, hi: f64, n: usize) -> Vec<f64> {
    if hi.is_nan() || lo.is_nan() || hi <= lo {
        return vec![lo];
    }
    (0..n).map(|i| lo + (hi - lo) * i as f64 / (n - 1) as f64).collect()
}

fn span(v: &[f64]) -> (f64, f64) {
    v.iter().filter(|x| x.is_finite()).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &x| (a.min(x), b.max(x)))
}

impl NonlinearAnalysis {
    /// Fit the model.
    ///
    /// # Errors
    /// Missing or non-numeric columns, a formula that does not compile or
    /// cannot be computed at the starting values, a missing starting value,
    /// too few usable rows, invalid weights, and errors from
    /// [`fit_nonlinear`] (for example a singular Jacobian at the solution).
    #[allow(clippy::too_many_lines)]
    pub fn run(table: &DataTable, states: &RowStateManager, request: &NonlinearRequest) -> Result<Self> {
        let column = |i: usize| table.column(i).ok_or_else(|| invalid(format!("no column {i}")));
        let numeric = |i: usize| -> Result<(&str, &[f64])> {
            let c = column(i)?;
            Ok((c.name.as_str(), c.as_numeric().ok_or_else(|| invalid(format!("{} must be numeric", c.name)))?))
        };
        let (response_name, y_all) = numeric(request.response)?;
        let names: Vec<String> = request.parameters.iter().map(|p| p.name.clone()).collect();
        if names.is_empty() {
            return Err(invalid("the model needs at least one parameter"));
        }
        let n = table.n_rows();
        // The model's columns and how each row is read.
        let (kind, model_text, mut x_name, x_col, model_columns): (
            Kind,
            String,
            Option<String>,
            Option<usize>,
            Vec<usize>,
        ) = match &request.model {
            NonlinearModel::Library { form, predictor } => {
                if names.len() != form.parameter_names().len() {
                    return Err(invalid(format!(
                        "the {} model has {} parameters",
                        form.label(),
                        form.parameter_names().len()
                    )));
                }
                let (x_name, _) = numeric(*predictor)?;
                (
                    Kind::Library { form: *form },
                    form.formula(x_name, &names),
                    Some(x_name.to_owned()),
                    Some(*predictor),
                    vec![*predictor],
                )
            }
            NonlinearModel::Formula { formula, .. } => {
                let compiled = compile_with_parameters(table, formula, &names)
                    .map_err(|e| invalid(format!("model formula: {e}")))?;
                let used = compiled.columns(table);
                if used.contains(&request.response) {
                    return Err(invalid(format!("the model formula uses the response {response_name:?}")));
                }
                let one = (used.len() == 1 && table.columns()[used[0]].as_numeric().is_some()).then(|| used[0]);
                (
                    Kind::Formula { text: formula.clone() },
                    formula.clone(),
                    one.map(|j| table.columns()[j].name.clone()),
                    one,
                    used,
                )
            }
            NonlinearModel::Differential { rate, predictor, initial, .. } => {
                let (x_name, _) = numeric(*predictor)?;
                if *predictor == request.response {
                    return Err(invalid("the predictor and the response must be different columns"));
                }
                let compiled =
                    compile_with_parameters(table, rate, &names).map_err(|e| invalid(format!("rate formula: {e}")))?;
                let used = compiled.columns(table);
                if used.iter().any(|&j| j != *predictor && j != request.response)
                    || !compiled.reads_columns_only_by_name(table)
                {
                    return Err(invalid(format!(
                        "the rate may use only {x_name} and {response_name}, written as plain names, and the parameters"
                    )));
                }
                let k = names
                    .iter()
                    .position(|p| p == initial)
                    .ok_or_else(|| invalid(format!("the initial value {initial:?} must be one of the parameters")))?;
                (
                    Kind::Ode { text: rate.clone(), predictor: 0, response: 1, initial: k, x0: f64::NAN },
                    format!("d{response_name}/d{x_name} = {rate}"),
                    Some(x_name.to_owned()),
                    Some(*predictor),
                    vec![*predictor],
                )
            }
            NonlinearModel::Implicit { equation, .. } => {
                let compiled =
                    compile_with_parameters(table, equation, &names).map_err(|e| invalid(format!("equation: {e}")))?;
                let used = compiled.columns(table);
                let others: Vec<usize> = used.iter().copied().filter(|&j| j != request.response).collect();
                if !used.contains(&request.response)
                    || others.len() != 1
                    || table.columns()[others[0]].as_numeric().is_none()
                    || !compiled.reads_columns_only_by_name(table)
                {
                    return Err(invalid(format!(
                        "the equation must use {response_name} and one other numeric column, written as plain names"
                    )));
                }
                let xc = others[0];
                (
                    Kind::Implicit { text: equation.clone(), x: 0, y: 1 },
                    format!("{equation} = 0"),
                    Some(table.columns()[xc].name.clone()),
                    Some(xc),
                    vec![xc],
                )
            }
        };
        let library = matches!(kind, Kind::Library { .. });
        if !library {
            for p in &request.parameters {
                if p.start.is_none() {
                    return Err(invalid(format!("enter a starting value for {}", p.name)));
                }
            }
        }
        // Rows.
        let group_col = request.group.map(column).transpose()?;
        let weight_col = match request.weighting {
            Weighting::Column(w) => Some(numeric(w)?.1),
            _ => None,
        };
        let numeric_model: Vec<&[f64]> =
            model_columns.iter().filter_map(|&j| table.columns()[j].as_numeric()).collect();
        let x_all = x_col.and_then(|j| table.columns()[j].as_numeric());
        let row_ok = |row: usize| -> bool {
            y_all[row].is_finite()
                && numeric_model.iter().all(|v| v[row].is_finite())
                && group_col.is_none_or(|c| !c.level_text(row).is_empty())
                && weight_col.is_none_or(|w| w[row].is_finite() && w[row] > 0.0)
        };
        let (mut n_excluded, mut n_missing) = (0, 0);
        let mut used = Vec::new();
        for row in 0..n {
            if !row_ok(row) {
                n_missing += 1;
            } else if states.is_excluded(row) {
                n_excluded += 1;
            } else {
                used.push(row);
            }
        }
        if used.is_empty() {
            return Err(invalid("no usable rows"));
        }
        // Data sets: numeric labels ascending, text in order of appearance.
        let label = |row: usize| group_col.map_or_else(String::new, |c| c.level_text(row));
        let mut groups: Vec<String> = Vec::new();
        for &r in &used {
            let l = label(r);
            if !groups.contains(&l) {
                groups.push(l);
            }
        }
        if let Some(v) = group_col.and_then(Column::as_numeric) {
            let mut keyed: Vec<(f64, String)> = groups
                .iter()
                .map(|l| (used.iter().find(|&&r| label(r) == *l).map_or(f64::NAN, |&r| v[r]), l.clone()))
                .collect();
            keyed.sort_by(|a, b| a.0.total_cmp(&b.0));
            groups = keyed.into_iter().map(|(_, l)| l).collect();
        }
        let group_index = |row: usize| groups.iter().position(|g| *g == label(row));
        used.sort_by_key(|&r| group_index(r).unwrap_or(usize::MAX));
        let group_of: Vec<usize> = used.iter().map(|&r| group_index(r).unwrap_or(0)).collect();
        let x: Vec<f64> = used.iter().map(|&r| x_all.map_or(f64::NAN, |v| v[r])).collect();
        let y: Vec<f64> = used.iter().map(|&r| y_all[r]).collect();
        let w: Vec<f64> = match request.weighting {
            Weighting::Equal => Vec::new(),
            Weighting::Column(_) => used.iter().map(|&r| weight_col.map_or(1.0, |v| v[r])).collect(),
            Weighting::InverseResponseSquared | Weighting::InverseResponse => {
                if let Some(i) = y.iter().position(|v| *v == 0.0) {
                    return Err(invalid(format!(
                        "{} weights need responses that are not zero (row {})",
                        request.weighting.label(),
                        used[i] + 1
                    )));
                }
                let sq = request.weighting == Weighting::InverseResponseSquared;
                y.iter().map(|v| if sq { 1.0 / (v * v) } else { 1.0 / v.abs() }).collect()
            }
        };
        // The one-row model table (differential and implicit models).
        let mini_source: Vec<usize> = match &request.model {
            NonlinearModel::Differential { predictor, .. } => vec![*predictor, request.response],
            NonlinearModel::Implicit { .. } => vec![x_col.unwrap_or(0), request.response],
            _ => model_columns.clone(),
        };
        let mini = DataTable::new(
            "model",
            mini_source
                .iter()
                .map(|&j| {
                    let c = &table.columns()[j];
                    match c.as_numeric() {
                        Some(v) => Column::numeric(c.name.clone(), vec![v[used[0]]]),
                        None => Column::text(c.name.clone(), vec![c.level_text(used[0])]),
                    }
                })
                .collect(),
        )?;
        let kind = match kind {
            Kind::Ode { text, predictor, response, initial, .. } => {
                let start = match &request.model {
                    NonlinearModel::Differential { start, .. } => *start,
                    _ => None,
                };
                let x0 = start.unwrap_or_else(|| span(&x).0);
                if !x0.is_finite() {
                    return Err(invalid("give the predictor value where the solution starts"));
                }
                Kind::Ode { text, predictor, response, initial, x0 }
            }
            k => k,
        };
        let setup = Setup {
            table,
            kind: kind.clone(),
            mini: mini.clone(),
            settings: request.parameters.clone(),
            groups: groups.clone(),
            rows: used.clone(),
            group_of: group_of.clone(),
            x: x.clone(),
            y: y.clone(),
            w: w.clone(),
            max_iterations: request.max_iterations,
            confidence_level: request.confidence_level,
        };
        let shared: Vec<bool> = request.parameters.iter().map(|p| p.shared).collect();
        let all: Vec<usize> = (0..used.len()).collect();
        let mut outliers_found = Vec::new();
        let outcome = match request.method {
            FitMethod::LeastSquares => setup.fit(&all, &shared, None, false, true)?,
            FitMethod::Robust => setup.fit(&all, &shared, None, true, true)?,
            FitMethod::RemoveOutliers { q } => {
                let robust = setup.fit(&all, &shared, None, true, false)?;
                let wr: Vec<f64> = robust
                    .positions
                    .iter()
                    .zip(&robust.fit.residuals)
                    .map(|(&p, r)| r * w.get(p).map_or(1.0, |v| v.sqrt()))
                    .collect();
                let found = outliers(&wr, robust.fit.n_estimated, q)?;
                let removed: Vec<usize> = found.iter().map(|&i| robust.positions[i]).collect();
                outliers_found = found
                    .iter()
                    .map(|&i| {
                        let p = robust.positions[i];
                        (used[p], x[p], y[p], robust.fit.residuals[i])
                    })
                    .collect::<Vec<_>>();
                outliers_found.sort_by_key(|o| o.0);
                let keep: Vec<usize> = all.iter().copied().filter(|p| !removed.contains(p)).collect();
                setup.fit(&keep, &shared, Some(&robust), false, true)?
            }
        };
        // Comparisons.
        let mut comparisons = Vec::new();
        let mut comparison_note = None;
        if request.method == FitMethod::Robust {
            comparison_note =
                Some("Model comparisons need a least-squares fit; none are made for a robust fit.".into());
        } else {
            let score = |f: &NonlinearFit| FitScore { sse: f.sse, n: f.n, parameters: f.n_estimated };
            let main_score = score(&outcome.fit);
            let mut compare =
                |label: String, simple: String, complex: String, alt: Result<FitOutcome>, alt_simple: bool| {
                    let outcome = alt.map_err(|e| e.to_string()).map(|a| {
                        let s = score(&a.fit);
                        if alt_simple { (s, main_score) } else { (main_score, s) }
                    });
                    comparisons.push(Comparison { label, simple, complex, outcome });
                };
            if groups.len() > 1 && shared.iter().any(|s| !s) {
                let unshared: Vec<usize> = (0..shared.len()).filter(|&k| !shared[k]).collect();
                let question = if let [k] = unshared[..] {
                    format!("Does {} differ between data sets?", request.parameters[k].name)
                } else {
                    "Do the data sets differ? (one curve for all)".to_owned()
                };
                compare(
                    question,
                    "One curve for all data sets".to_owned(),
                    "This fit".to_owned(),
                    setup.fit(&outcome.positions, &vec![true; shared.len()], Some(&outcome), false, false),
                    true,
                );
                for (k, _) in shared.iter().enumerate().filter(|(_, s)| !**s) {
                    let mut more = shared.clone();
                    more[k] = true;
                    if more == vec![true; shared.len()] && shared.iter().filter(|s| !**s).count() == 1 {
                        continue; // Same as the comparison above.
                    }
                    let name = &request.parameters[k].name;
                    compare(
                        format!("Does {name} differ between data sets?"),
                        format!("One {name} for all data sets"),
                        "This fit".to_owned(),
                        setup.fit(&outcome.positions, &more, Some(&outcome), false, false),
                        true,
                    );
                }
            }
            if let Some(t) = &request.template {
                templates::comparisons(t, &setup, &outcome, &shared, &mut compare);
            }
            if let (Some(alt), Kind::Library { form }) = (request.compare_with, &kind) {
                if groups.len() > 1 {
                    comparison_note = Some("The comparison with another curve is made only with one data set.".into());
                } else if alt != *form {
                    let alt_setup = Setup {
                        kind: Kind::Library { form: alt },
                        settings: alt.parameter_names().iter().map(|n| ParameterSetting::new(*n, None)).collect(),
                        ..setup.clone()
                    };
                    let fitted =
                        alt_setup.fit(&outcome.positions, &vec![true; alt.parameter_names().len()], None, false, false);
                    let alt_simple = alt.parameter_names().len() < form.parameter_names().len();
                    let (simple, complex) =
                        if alt_simple { (alt.label(), form.label()) } else { (form.label(), alt.label()) };
                    compare(
                        format!("{} or {}?", form.label(), alt.label()),
                        simple.to_owned(),
                        complex.to_owned(),
                        fitted,
                        alt_simple,
                    );
                }
            }
        }
        let template_extras = request
            .template
            .as_ref()
            .map(|t| templates::extras(t, table, states, request, &setup, &outcome))
            .unwrap_or_default();
        let fit = &outcome.fit;
        let theta = fit.estimates();
        // The model as a function of its varied columns.
        let varied: Vec<usize> = match &kind {
            Kind::Formula { .. } => {
                if model_columns.iter().all(|&j| table.columns()[j].as_numeric().is_some()) {
                    model_columns.clone()
                } else {
                    Vec::new()
                }
            }
            Kind::Implicit { .. } => Vec::new(),
            _ => vec![x_col.unwrap_or(0)],
        };
        let predictor = Predictor {
            kind: kind.clone(),
            columns: varied.clone(),
            names: varied.iter().map(|&j| table.columns()[j].name.clone()).collect(),
            mini: mini.clone(),
            mini_columns: varied.iter().filter_map(|&j| mini_source.iter().position(|&m| m == j)).collect(),
            parameter_names: names.clone(),
            map: outcome.map.clone(),
        };
        let rows_used: Vec<usize> = outcome.positions.iter().map(|&p| used[p]).collect();
        let group_used: Vec<usize> = outcome.positions.iter().map(|&p| group_of[p]).collect();
        let x_used: Vec<f64> = outcome.positions.iter().map(|&p| x[p]).collect();
        let y_used: Vec<f64> = outcome.positions.iter().map(|&p| y[p]).collect();
        let observed_settings: Vec<Vec<f64>> = rows_used
            .iter()
            .map(|&r| varied.iter().map(|&j| table.columns()[j].as_numeric().map_or(f64::NAN, |v| v[r])).collect())
            .collect();
        let factor_ranges: Vec<(f64, f64, f64)> = varied
            .iter()
            .map(|&j| {
                let v = table.columns()[j].as_numeric().unwrap_or(&[]);
                let vals: Vec<f64> = rows_used.iter().map(|&r| v[r]).collect();
                let (lo, hi) = span(&vals);
                (lo, hi, vals.iter().sum::<f64>() / vals.len().max(1) as f64)
            })
            .collect();
        // Predictions for every row.
        let mut predicted_all = vec![f64::NAN; n];
        if !varied.is_empty() {
            let cols: Vec<&[f64]> = varied.iter().filter_map(|&j| table.columns()[j].as_numeric()).collect();
            for g in 0..groups.len() {
                let rows: Vec<usize> = (0..n)
                    .filter(|&r| {
                        cols.iter().all(|c| c[r].is_finite()) && (group_col.is_none() || group_index(r) == Some(g))
                    })
                    .collect();
                if rows.is_empty() {
                    continue;
                }
                let points: Vec<Vec<f64>> = rows.iter().map(|&r| cols.iter().map(|c| c[r]).collect()).collect();
                if let Some(v) = predictor.values(g, &points, &theta) {
                    for (&r, v) in rows.iter().zip(v) {
                        predicted_all[r] = v;
                    }
                }
            }
        }
        if let Kind::Formula { text } = &kind {
            // Rows a formula reads text from are computed on the table itself.
            if varied.is_empty() {
                let mut compiled = compile_with_parameters(table, text, &names).map_err(|e| invalid(e.to_string()))?;
                let mut one = [0.0];
                for r in 0..n {
                    let g = if group_col.is_some() { group_index(r) } else { Some(0) };
                    let Some(g) = g else { continue };
                    let local: Vec<f64> = outcome.map[g].iter().map(|&i| theta[i]).collect();
                    if compiled.evaluate(table, &[r], &local, &mut one).is_ok() {
                        predicted_all[r] = one[0];
                    }
                }
            }
        }
        // Weight of a new observation with fitted value v, for prediction limits.
        let weighting = request.weighting;
        let weight_at = move |v: f64| match weighting {
            Weighting::InverseResponseSquared if v != 0.0 => 1.0 / (v * v),
            Weighting::InverseResponse if v != 0.0 => 1.0 / v.abs(),
            _ => 1.0,
        };
        // Curves.
        let (mut curve, mut group_curves) = (None, Vec::new());
        if varied.len() == 1 && x_name.is_some() {
            for g in 0..groups.len() {
                let xg: Vec<f64> =
                    x_used.iter().zip(&group_used).filter(|(_, gg)| **gg == g).map(|(v, _)| *v).collect();
                let (lo, hi) = span(&xg);
                let points: Vec<Vec<f64>> = grid(lo, hi, CURVE_POINTS).into_iter().map(|v| vec![v]).collect();
                if groups.len() == 1 {
                    if let Some(l) = predictor.with_limits(fit, 0, &points, &weight_at) {
                        let mut c = Curve {
                            line: Vec::new(),
                            mean: (Vec::new(), Vec::new()),
                            prediction: (Vec::new(), Vec::new()),
                        };
                        for (p, (v, m, pr)) in points.iter().zip(l) {
                            if !(v.is_finite() && m.0.is_finite() && pr.0.is_finite()) {
                                continue;
                            }
                            c.line.push([p[0], v]);
                            c.mean.0.push([p[0], m.0]);
                            c.mean.1.push([p[0], m.1]);
                            c.prediction.0.push([p[0], pr.0]);
                            c.prediction.1.push([p[0], pr.1]);
                        }
                        curve = (c.line.len() > 1).then_some(c);
                    }
                } else {
                    let line = predictor
                        .values(g, &points, &theta)
                        .map(|v| points.iter().zip(v).filter(|(_, v)| v.is_finite()).map(|(p, v)| [p[0], v]).collect())
                        .unwrap_or_default();
                    group_curves.push(line);
                }
            }
        }
        // Implicit curves: the zero contour of the equation over the data.
        let mut implicit_lines = Vec::new();
        let mut distances = Vec::new();
        if let Kind::Implicit { text, x: xc, y: yc } = &kind {
            distances = fit.residuals.iter().map(|r| -r).collect();
            let (x_lo, x_hi) = span(&x_used);
            let (y_lo, y_hi) = span(&y_used);
            let pad = |lo: f64, hi: f64| {
                let d = 0.1 * (hi - lo).max(1e-12);
                (lo - d, hi + d)
            };
            let ((x_lo, x_hi), (y_lo, y_hi)) = (pad(x_lo, x_hi), pad(y_lo, y_hi));
            let gx = grid(x_lo, x_hi, IMPLICIT_GRID);
            let gy = grid(y_lo, y_hi, IMPLICIT_GRID);
            if let Ok(mut eq) = compile_with_parameters(&mini, text, &names) {
                for g in 0..groups.len() {
                    let local: Vec<f64> = outcome.map[g].iter().map(|&i| theta[i]).collect();
                    let values: Vec<Vec<f64>> = gy
                        .iter()
                        .map(|&b| {
                            gx.iter()
                                .map(|&a| {
                                    let value_of = |j: usize| {
                                        if j == *xc {
                                            Some(a)
                                        } else if j == *yc {
                                            Some(b)
                                        } else {
                                            None
                                        }
                                    };
                                    eq.evaluate_with(&mini, 0, &value_of, &local).unwrap_or(f64::NAN)
                                })
                                .collect()
                        })
                        .collect();
                    let (dx, dy) = (gx[1] - gx[0], gy[1] - gy[0]);
                    for line in contour_lines(&values, 0.0, |i| i as f64) {
                        implicit_lines.push(line.iter().map(|&[u, v]| [x_lo + u * dx, y_lo + v * dy]).collect());
                    }
                }
            }
            x_name = Some(table.columns()[x_col.unwrap_or(0)].name.clone());
        }
        // Surface over two columns.
        let surface = if varied.len() == 2 && groups.len() == 1 {
            let gx = grid(factor_ranges[0].0, factor_ranges[0].1, SURFACE_GRID);
            let gy = grid(factor_ranges[1].0, factor_ranges[1].1, SURFACE_GRID);
            let points: Vec<Vec<f64>> = gy.iter().flat_map(|&b| gx.iter().map(move |&a| vec![a, b])).collect();
            predictor
                .values(0, &points, &theta)
                .filter(|_| gx.len() > 1 && gy.len() > 1)
                .map(|v| (gx.clone(), gy.clone(), v.chunks(gx.len()).map(<[f64]>::to_vec).collect()))
        } else {
            None
        };
        // Inverse prediction: rows with a response and no predictor value.
        let mut inverse = Vec::new();
        if let (Some(xv), 1) = (x_all, varied.len()) {
            if !matches!(kind, Kind::Implicit { .. }) {
                for r in 0..n {
                    if !(y_all[r].is_finite() && xv[r].is_nan()) || states.is_excluded(r) {
                        continue;
                    }
                    if group_col.is_some_and(|c| c.level_text(r).is_empty()) {
                        continue;
                    }
                    let Some(g) = (if group_col.is_some() { group_index(r) } else { Some(0) }) else { continue };
                    let xg: Vec<f64> =
                        x_used.iter().zip(&group_used).filter(|(_, gg)| **gg == g).map(|(v, _)| *v).collect();
                    let range = span(&xg);
                    let w0 = match request.weighting {
                        Weighting::InverseResponseSquared => 1.0 / (y_all[r] * y_all[r]),
                        Weighting::InverseResponse => 1.0 / y_all[r].abs(),
                        Weighting::Column(_) => weight_col.map_or(1.0, |c| if c[r] > 0.0 { c[r] } else { 1.0 }),
                        Weighting::Equal => 1.0,
                    };
                    let mut ev = predictor.evaluator();
                    let mut at = |v: f64| {
                        let l = predictor.with_limits_with(&mut ev, fit, g, &[vec![v]], &|_| w0)?;
                        let (value, _, (lo, _)) = l[0];
                        Some((value, value - lo))
                    };
                    inverse.push((r, g, inverse_predict(y_all[r], range, &mut at)));
                }
            }
        }
        // Prediction formula.
        let formula =
            Self::prediction_formula_text(&kind, x_name.as_deref(), &names, &theta, &outcome, &groups, group_col);
        let starts = setup.starts(&all, &outcome.owner, None).unwrap_or_else(|_| vec![f64::NAN; outcome.owner.len()]);
        let automatic_start = outcome.owner.iter().map(|&(k, _)| request.parameters[k].start.is_none()).collect();
        Ok(Self {
            request: request.clone(),
            response_name: response_name.to_owned(),
            model_text,
            x_name,
            group_name: group_col.map(|c| c.name.clone()),
            groups,
            outcome,
            starts,
            automatic_start,
            rows_used,
            group_used,
            x_used,
            y_used,
            predicted_all,
            distances,
            curve,
            group_curves,
            implicit_lines,
            surface,
            formula,
            predictor,
            factor_ranges,
            comparisons,
            comparison_note,
            outliers: outliers_found,
            observed_settings,
            inverse,
            n_excluded,
            n_missing,
            column_names: table.columns().iter().map(|c| c.name.clone()).collect(),
            x_values: request
                .template
                .as_ref()
                .and_then(|t| table.column(t.x)?.as_numeric().map(<[f64]>::to_vec))
                .unwrap_or_default(),
            second_text: request
                .template
                .as_ref()
                .and_then(|t| table.column(t.second?))
                .map(|c| (0..n).map(|r| c.level_text(r)).collect())
                .unwrap_or_default(),
            dilution_values: request
                .template
                .as_ref()
                .and_then(|t| table.column(t.dilution?)?.as_numeric().map(<[f64]>::to_vec))
                .unwrap_or_default(),
            template_extras,
        })
    }

    /// The prediction formula: the library form or formula with the
    /// estimates written in, one branch per data set.
    fn prediction_formula_text(
        kind: &Kind,
        x_name: Option<&str>,
        names: &[String],
        theta: &[f64],
        outcome: &FitOutcome,
        groups: &[String],
        group_col: Option<&Column>,
    ) -> std::result::Result<String, String> {
        let body = |g: usize| -> std::result::Result<String, String> {
            let local: Vec<f64> = outcome.map[g].iter().map(|&i| theta[i]).collect();
            match kind {
                Kind::Library { form } => {
                    let x_ref = column_reference(x_name.unwrap_or("x"));
                    Ok(form.formula(&x_ref, &local.iter().map(|&v| float_literal(v)).collect::<Vec<_>>()))
                }
                Kind::Formula { text } => {
                    let lets: String =
                        names.iter().zip(&local).map(|(n, v)| format!("let {n} = {};\n", float_literal(*v))).collect();
                    Ok(format!("{lets}{}", text.trim()))
                }
                Kind::Ode { .. } => Err("a differential-equation model has no closed form".to_owned()),
                Kind::Implicit { .. } => Err("an implicit curve does not predict one column from others".to_owned()),
            }
        };
        if groups.len() == 1 {
            return body(0);
        }
        let Some(col) = group_col else { return body(0) };
        let reference = column_reference(&col.name);
        let mut text = String::new();
        for (g, label) in groups.iter().enumerate() {
            let test = if col.as_numeric().is_some() {
                format!("{reference} == {}", float_literal(label.parse::<f64>().unwrap_or(f64::NAN)))
            } else {
                format!("{reference} == {label:?}")
            };
            let keyword = if g == 0 { "if" } else { " else if" };
            text.push_str(&format!("{keyword} {test} {{\n{}\n}}", body(g)?));
        }
        text.push_str(" else {\nmissing()\n}");
        Ok(text)
    }

    /// Profilers of a new nonlinear report: all off.
    #[must_use]
    pub fn default_profilers() -> ProfilerOptions {
        ProfilerOptions { show_profiler: false, ..ProfilerOptions::default() }
    }

    /// The statistical fit.
    #[must_use]
    pub fn fit(&self) -> &NonlinearFit {
        &self.outcome.fit
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &NonlinearRequest {
        &self.request
    }

    /// Rows used in the fit.
    #[must_use]
    pub fn rows_used(&self) -> &[usize] {
        &self.rows_used
    }

    /// Rows removed as outliers.
    #[must_use]
    pub fn outlier_rows(&self) -> Vec<usize> {
        self.outliers.iter().map(|o| o.0).collect()
    }

    /// Inverse predictions `(row, prediction)`.
    #[must_use]
    pub fn inverse_predictions(&self) -> Vec<(usize, InversePrediction)> {
        self.inverse.iter().map(|&(r, _, p)| (r, p)).collect()
    }

    /// The fitted model as a formula column: the library form with the
    /// estimates written in, or the model formula preceded by
    /// `let <parameter> = <estimate>;` for each parameter (one branch per
    /// data set); `Err` with the reason for models that have none.
    pub fn prediction_formula(&self) -> std::result::Result<&str, &str> {
        match &self.formula {
            Ok(f) => Ok(f),
            Err(e) => Err(e),
        }
    }

    /// The model as a surface model for the profilers, when it predicts the
    /// response from numeric columns.
    #[must_use]
    pub fn surface_model(&self) -> Option<&dyn SurfaceModel> {
        (!self.predictor.columns.is_empty()).then_some(self as &dyn SurfaceModel)
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        if let Some(t) = &self.request.template {
            let x = self.column_names.get(t.x).cloned().unwrap_or_default();
            let sets = match &self.group_name {
                Some(g) => format!(", {} data sets by {g}", self.groups.len()),
                None => String::new(),
            };
            return format!("{}: {} by {x}{sets}", t.template.label(), self.response_name);
        }
        let by = |what: &str| match &self.group_name {
            Some(g) => format!("{what}, {} data sets by {g}", self.groups.len()),
            None => what.to_owned(),
        };
        match (&self.request.model, &self.x_name) {
            (NonlinearModel::Library { form, .. }, Some(x)) => {
                format!("Nonlinear Regression: {} by {x} ({})", self.response_name, by(form.label()))
            }
            (NonlinearModel::Differential { .. }, Some(x)) => {
                format!("Nonlinear Regression: {} by {x} ({})", self.response_name, by("differential equation"))
            }
            (NonlinearModel::Implicit { .. }, Some(x)) => {
                format!("Nonlinear Regression: {} and {x} ({})", self.response_name, by("implicit curve"))
            }
            _ if self.group_name.is_some() => {
                format!("Nonlinear Regression: {} ({})", self.response_name, by("formula"))
            }
            _ => format!("Nonlinear Regression: {}", self.response_name),
        }
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let mut out = vec![self.summary()];
        if !self.fit().warnings.is_empty() {
            out.push(section(
                SectionKind::NonlinearWarnings,
                "Warnings",
                SectionContent::Note(self.fit().warnings.join("\n")),
            ));
        }
        let mut template = self.template_sections().into_iter();
        if self.request.template.is_some() {
            out.extend(template.next());
        }
        out.push(self.estimates());
        out.extend(template);
        if self.fit().parameters.len() > 1 {
            out.push(self.correlation());
        }
        if !self.comparisons.is_empty() || self.comparison_note.is_some() {
            out.push(self.comparison_section());
        }
        if matches!(self.request.method, FitMethod::RemoveOutliers { .. }) {
            out.push(self.outlier_section());
        }
        out.push(self.curve_plot());
        if self.surface.is_some() {
            out.push(self.surface_plot());
        }
        out.push(self.residual_plot());
        if !self.inverse.is_empty() {
            out.push(self.inverse_section());
        }
        out.push(crate::report::prediction_formula_section(self.formula.clone().map_err(invalid)));
        out
    }

    fn summary(&self) -> ReportSection {
        let f = self.fit();
        let mut t = TextTable::new(["Statistic", "Value"]);
        let mut row = |a: &str, b: String| t.push([a.to_owned(), b]);
        let model = match self.request.model {
            NonlinearModel::Differential { .. } | NonlinearModel::Implicit { .. } => self.model_text.clone(),
            _ => format!("{} = {}", self.response_name, self.model_text),
        };
        row("Model", model);
        if let NonlinearModel::Differential { initial, .. } = &self.request.model {
            if let Kind::Ode { x0, .. } = self.predictor.kind {
                let x = self.x_name.clone().unwrap_or_default();
                row(
                    "Initial value",
                    format!("{} = {initial} at {x} = {}", self.response_name, format_number(x0, DIGITS)),
                );
            }
        }
        if let Some(g) = &self.group_name {
            row("Data sets", format!("{} (by {g}): {}", self.groups.len(), self.groups.join(", ")));
            let shared: Vec<&str> =
                self.request.parameters.iter().filter(|p| p.shared).map(|p| p.name.as_str()).collect();
            row("Shared by all data sets", if shared.is_empty() { "none".to_owned() } else { shared.join(", ") });
        }
        row("Observations used", f.n.to_string());
        if self.n_excluded > 0 {
            row("Rows excluded", self.n_excluded.to_string());
        }
        if self.n_missing > 0 {
            row("Rows with missing values", self.n_missing.to_string());
        }
        if self.request.weighting != Weighting::Equal {
            row("Weighting", self.request.weighting.label().to_owned());
        }
        if self.request.method != FitMethod::LeastSquares {
            row("Fitting method", self.request.method.label());
        }
        if let Some(scale) = self.outcome.robust_scale {
            row("Robust standard deviation of the residuals", format_number(scale, DIGITS));
        }
        if matches!(self.request.method, FitMethod::RemoveOutliers { .. }) {
            row("Outliers removed", self.outliers.len().to_string());
        }
        let n_fixed = f.parameters.len() - f.n_estimated;
        row(
            "Parameters",
            if n_fixed == 0 {
                f.parameters.len().to_string()
            } else {
                format!("{} ({} estimated, {n_fixed} fixed by equal bounds)", f.parameters.len(), f.n_estimated)
            },
        );
        let (sse, se) = match (f.weighted, &self.request.model) {
            (_, NonlinearModel::Implicit { .. }) => {
                ("Sum of squared distances to the curve", "Residual standard deviation (distance)")
            }
            (true, _) => ("Weighted sum of squared errors", "Residual standard error (weight 1)"),
            (false, _) => ("Sum of squared errors (SSE)", "Residual standard error"),
        };
        row(sse, format_number(f.sse, 8));
        row("Residual degrees of freedom", f.df.to_string());
        row(se, format_number(f.residual_se, DIGITS));
        let score = FitScore { sse: f.sse, n: f.n, parameters: f.n_estimated };
        if score.aicc().is_finite() {
            row("AICc", format_number(score.aicc(), DIGITS));
        }
        row("Convergence", f.termination.label().to_owned());
        row("Iterations", f.iterations.to_string());
        row("Relative offset at the solution", format_number(f.relative_offset, 3));
        row(
            "Derivatives",
            if f.analytic_derivatives { "analytic (library form)" } else { "central differences" }.to_owned(),
        );
        section(SectionKind::NonlinearSummary, "Fit Summary", SectionContent::Table(t))
    }

    fn estimates(&self) -> ReportSection {
        let f = self.fit();
        let level = percent(f.confidence_level);
        let mut heads = vec![
            "Parameter".to_owned(),
            "Estimate".to_owned(),
            "Std Error".to_owned(),
            "t Ratio".to_owned(),
            "p-value".to_owned(),
            format!("Wald lower {level}%"),
            format!("Wald upper {level}%"),
        ];
        if self.request.profile_intervals {
            heads.push(format!("Profile lower {level}%"));
            heads.push(format!("Profile upper {level}%"));
        }
        let skew = f.parameters.iter().any(|p| p.skewness.is_finite());
        if skew {
            heads.push("Skewness (Hougaard)".to_owned());
        }
        heads.push("Start".to_owned());
        let mut t = TextTable::new(heads);
        for (j, p) in f.parameters.iter().enumerate() {
            let missing = |v: f64| if v.is_finite() { format_number(v, DIGITS) } else { "—".to_owned() };
            let mut cells = vec![
                match p.bound {
                    _ if p.fixed => format!("{} (fixed)", p.name),
                    BoundState::Free => p.name.clone(),
                    BoundState::Lower => format!("{} (at lower bound)", p.name),
                    BoundState::Upper => format!("{} (at upper bound)", p.name),
                },
                format_number(p.estimate, DIGITS),
                missing(p.std_error),
                missing(p.t_ratio),
                if p.p_value.is_finite() { format_p_value(p.p_value) } else { "—".to_owned() },
                missing(p.wald_interval.0),
                missing(p.wald_interval.1),
            ];
            if self.request.profile_intervals {
                let side = |v: Option<f64>| v.map_or_else(|| "open".to_owned(), |v| format_number(v, DIGITS));
                match p.profile_interval {
                    Some(i) => {
                        cells.push(side(i.lower));
                        cells.push(side(i.upper));
                    }
                    None => cells.extend(["—".to_owned(), "—".to_owned()]),
                }
            }
            if skew {
                cells.push(if p.skewness.is_finite() { format_number(p.skewness, 3) } else { "—".to_owned() });
            }
            let start = format_number(self.starts[j], DIGITS);
            cells.push(if self.automatic_start[j] { format!("{start} (automatic)") } else { start });
            t.push(cells);
        }
        section(SectionKind::NonlinearEstimates, "Parameter Estimates", SectionContent::Table(t))
    }

    fn correlation(&self) -> ReportSection {
        let f = self.fit();
        let names: Vec<String> = f.parameters.iter().map(|p| p.name.clone()).collect();
        let mut t = TextTable::new(std::iter::once(String::new()).chain(names.iter().cloned()));
        for (i, name) in names.iter().enumerate() {
            let mut cells = vec![name.clone()];
            for j in 0..names.len() {
                let c = f.correlation[(i, j)];
                cells.push(if c.is_finite() { format_number(c, 4) } else { "—".to_owned() });
            }
            t.push(cells);
        }
        section(SectionKind::NonlinearCorrelation, "Correlation of Estimates", SectionContent::Table(t))
    }

    fn comparison_section(&self) -> ReportSection {
        let mut t = TextTable::new([
            "Question",
            "Simpler model",
            "More complex model",
            "SSE (simpler)",
            "SSE (more complex)",
            "F",
            "df",
            "p-value",
            "AICc (simpler)",
            "AICc (more complex)",
            "Probability the more complex model is better",
        ]);
        for c in &self.comparisons {
            match &c.outcome {
                Ok((a, b)) => {
                    let test = extra_sum_of_squares(*a, *b);
                    let num = |v: f64| if v.is_finite() { format_number(v, DIGITS) } else { "—".to_owned() };
                    t.push([
                        c.label.clone(),
                        c.simple.clone(),
                        c.complex.clone(),
                        num(a.sse),
                        num(b.sse),
                        test.as_ref().map_or_else(|_| "—".to_owned(), |f| format_number(f.f, DIGITS)),
                        test.as_ref()
                            .map_or_else(|_| "—".to_owned(), |f| format!("{}, {}", f.df_numerator, f.df_denominator)),
                        test.as_ref().map_or_else(|_| "—".to_owned(), |f| format_p_value(f.p_value)),
                        num(a.aicc()),
                        num(b.aicc()),
                        {
                            let p = aicc_probability(*a, *b);
                            if p.is_finite() { format!("{}%", format_number(100.0 * p, 4)) } else { "—".to_owned() }
                        },
                    ]);
                }
                Err(e) => {
                    let mut cells = vec![c.label.clone(), c.simple.clone(), c.complex.clone()];
                    cells.push(format!("could not be fitted: {e}"));
                    cells.extend(std::iter::repeat_n("—".to_owned(), 7));
                    t.push(cells);
                }
            }
        }
        if let Some(note) = &self.comparison_note {
            if self.comparisons.is_empty() {
                return section(
                    SectionKind::NonlinearComparison,
                    "Model Comparison",
                    SectionContent::Note(note.clone()),
                );
            }
        }
        section(
            SectionKind::NonlinearComparison,
            "Model Comparison (extra sum-of-squares F test, valid when the simpler model is a special case of the other; AICc)",
            SectionContent::Table(t),
        )
    }

    fn outlier_section(&self) -> ReportSection {
        if self.outliers.is_empty() {
            return section(
                SectionKind::NonlinearOutliers,
                "Outliers",
                SectionContent::Note("No outliers were found; the fit uses every row.".to_owned()),
            );
        }
        let with_x = self.outliers.iter().any(|o| o.1.is_finite());
        let mut heads = vec!["Row".to_owned()];
        if with_x {
            heads.push(self.x_name.clone().unwrap_or_default());
        }
        heads.extend([self.response_name.clone(), "Residual (robust fit)".to_owned()]);
        let mut t = TextTable::new(heads);
        for &(row, x, y, r) in &self.outliers {
            let mut cells = vec![(row + 1).to_string()];
            if with_x {
                cells.push(format_number(x, DIGITS));
            }
            cells.push(format_number(y, DIGITS));
            cells.push(format_number(r, DIGITS));
            t.push(cells);
        }
        section(
            SectionKind::NonlinearOutliers,
            format!("Outliers (removed before the final least-squares fit; {})", self.request.method.label()),
            SectionContent::Table(t),
        )
    }

    fn group_membership(&self) -> Option<PointGroups> {
        (self.groups.len() > 1).then(|| PointGroups {
            title: self.group_name.clone().unwrap_or_default(),
            labels: self.groups.clone(),
            membership: self.group_used.clone(),
            lines: self.group_curves.clone(),
        })
    }

    fn curve_plot(&self) -> ReportSection {
        let level = percent(self.fit().confidence_level);
        if matches!(self.request.model, NonlinearModel::Implicit { .. }) {
            let points = self
                .rows_used
                .iter()
                .zip(self.x_used.iter().zip(&self.y_used))
                .map(|(&row, (&x, &y))| RowPoint { row, x, y });
            let spec = PlotSpec {
                x_label: self.x_name.clone().unwrap_or_default(),
                y_label: self.response_name.clone(),
                points: points.collect(),
                segments: self.implicit_lines.clone(),
                groups: self.group_membership(),
                ..PlotSpec::default()
            };
            return section(
                SectionKind::NonlinearCurve,
                "Fitted Curve (thin lines: the curve)",
                SectionContent::Plot(Box::new(spec)),
            );
        }
        let one_predictor = self.x_name.is_some() && (self.curve.is_some() || !self.group_curves.is_empty());
        if one_predictor {
            let x_name = self.x_name.clone().unwrap_or_default();
            let points = self
                .rows_used
                .iter()
                .zip(self.x_used.iter().zip(&self.y_used))
                .map(|(&row, (&x, &y))| RowPoint { row, x, y })
                .collect();
            if let Some(curve) = &self.curve {
                let spec = PlotSpec {
                    x_label: x_name,
                    y_label: self.response_name.clone(),
                    points,
                    line: Some(curve.line.clone()),
                    line_name: Some("Fitted curve".to_owned()),
                    band: Some(curve.mean.clone()),
                    band_name: Some(format!("{level}% confidence band for the mean")),
                    segments: vec![curve.prediction.0.clone(), curve.prediction.1.clone()],
                    ..PlotSpec::default()
                };
                return section(
                    SectionKind::NonlinearCurve,
                    format!("Fitted Curve ({level}% confidence band; thin lines: {level}% prediction limits)"),
                    SectionContent::Plot(Box::new(spec)),
                );
            }
            let spec = PlotSpec {
                x_label: x_name,
                y_label: self.response_name.clone(),
                points,
                groups: self.group_membership(),
                ..PlotSpec::default()
            };
            return section(
                SectionKind::NonlinearCurve,
                "Fitted Curves by Data Set",
                SectionContent::Plot(Box::new(spec)),
            );
        }
        // Several predictors: observed against predicted.
        let points: Vec<RowPoint> = self
            .rows_used
            .iter()
            .zip(self.fit().fitted.iter().zip(&self.y_used))
            .map(|(&row, (&p, &y))| RowPoint { row, x: p, y })
            .collect();
        let (lo, hi) = points
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(p.x.min(p.y)), b.max(p.x.max(p.y))));
        let spec = PlotSpec {
            x_label: format!("Predicted {}", self.response_name),
            y_label: self.response_name.clone(),
            points,
            line: Some(vec![[lo, lo], [hi, hi]]),
            line_name: Some("Observed = predicted".to_owned()),
            groups: self.group_membership().map(|g| PointGroups { lines: Vec::new(), ..g }),
            ..PlotSpec::default()
        };
        section(SectionKind::NonlinearCurve, "Observed vs Predicted", SectionContent::Plot(Box::new(spec)))
    }

    fn surface_plot(&self) -> ReportSection {
        let Some((gx, gy, values)) = &self.surface else {
            return section(SectionKind::NonlinearSurface, "Fitted Surface", SectionContent::Note(String::new()));
        };
        let map = ContourMap::new(stat_viz::ContourField::from_grid(gx, gy, values), 10, self.response_name.clone());
        let (_, step) = map.levels();
        let points = self.surface_points();
        let spec = PlotSpec {
            x_label: self.predictor.names[0].clone(),
            y_label: self.predictor.names[1].clone(),
            points,
            point_name: Some("Observations".to_owned()),
            contour: Some(map),
            ..PlotSpec::default()
        };
        section(
            SectionKind::NonlinearSurface,
            format!("Fitted Surface of {} (contours every {})", self.response_name, format_number(step, 6)),
            SectionContent::Plot(Box::new(spec)),
        )
    }

    fn surface_points(&self) -> Vec<RowPoint> {
        self.rows_used.iter().zip(&self.observed_settings).map(|(&row, s)| RowPoint { row, x: s[0], y: s[1] }).collect()
    }

    fn residual_plot(&self) -> ReportSection {
        let implicit = matches!(self.request.model, NonlinearModel::Implicit { .. });
        let points: Vec<RowPoint> = if implicit {
            self.rows_used
                .iter()
                .zip(self.x_used.iter().zip(&self.distances))
                .map(|(&row, (&x, &d))| RowPoint { row, x, y: d })
                .collect()
        } else {
            self.rows_used
                .iter()
                .zip(self.fit().fitted.iter().zip(&self.fit().residuals))
                .map(|(&row, (&p, &r))| RowPoint { row, x: p, y: r })
                .collect()
        };
        let (lo, hi) = points.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(p.x), b.max(p.x)));
        let spec = PlotSpec {
            x_label: if implicit {
                self.x_name.clone().unwrap_or_default()
            } else {
                format!("Predicted {}", self.response_name)
            },
            y_label: if implicit { "Signed distance to the curve".to_owned() } else { "Residual".to_owned() },
            points,
            line: Some(vec![[lo, 0.0], [hi, 0.0]]),
            line_name: Some("Zero".to_owned()),
            groups: self.group_membership().map(|g| PointGroups { lines: Vec::new(), ..g }),
            ..PlotSpec::default()
        };
        let title = if implicit { "Distances to the Curve" } else { "Residuals vs Predicted" };
        section(SectionKind::NonlinearResiduals, title, SectionContent::Plot(Box::new(spec)))
    }

    fn inverse_section(&self) -> ReportSection {
        let level = percent(self.fit().confidence_level);
        let x = self.x_name.clone().unwrap_or_else(|| "x".to_owned());
        let mut heads = vec!["Row".to_owned()];
        if self.groups.len() > 1 {
            heads.push(self.group_name.clone().unwrap_or_default());
        }
        heads.extend([
            self.response_name.clone(),
            format!("Predicted {x}"),
            format!("Lower {level}%"),
            format!("Upper {level}%"),
            "Note".to_owned(),
        ]);
        let mut t = TextTable::new(heads);
        for &(row, g, p) in &self.inverse {
            let side = |v: Option<f64>| v.map_or_else(|| "open".to_owned(), |v| format_number(v, DIGITS));
            let mut cells = vec![(row + 1).to_string()];
            if self.groups.len() > 1 {
                cells.push(self.groups[g].clone());
            }
            cells.push(format_number(p.response, DIGITS));
            match p.estimate {
                Some(e) => {
                    cells.push(format_number(e, DIGITS));
                    cells.push(side(p.lower));
                    cells.push(side(p.upper));
                    cells.push(if p.several_solutions {
                        "the curve reaches this response more than once; the lowest solution is shown".to_owned()
                    } else if p.lower.is_none() || p.upper.is_none() {
                        "the interval reaches the end of the standards' range".to_owned()
                    } else {
                        String::new()
                    });
                }
                None => {
                    cells.extend(["—".to_owned(), "—".to_owned(), "—".to_owned()]);
                    cells.push("the curve does not reach this response within the standards' range".to_owned());
                }
            }
            t.push(cells);
        }
        section(
            SectionKind::NonlinearInverse,
            format!("Inverse Prediction of {x} ({level}% intervals for one new observation)"),
            SectionContent::Table(t),
        )
    }

    /// Columns the report can add: `predicted` (every row the model can be
    /// computed on), `prediction_formula` (the fitted model as a formula
    /// column, when it has one), `residual` (rows used in the fit; the
    /// signed distance to the curve for an implicit model) and
    /// `inverse_predicted` (rows read off the curve).
    #[must_use]
    pub fn saveable_columns(&self) -> Vec<ReportColumn> {
        let n = self.predicted_all.len();
        let name = format!("Predicted {}", self.response_name);
        let mut residual = vec![f64::NAN; n];
        let implicit = matches!(self.request.model, NonlinearModel::Implicit { .. });
        let res: &[f64] = if implicit { &self.distances } else { &self.fit().residuals };
        for (&r, &e) in self.rows_used.iter().zip(res) {
            residual[r] = e;
        }
        let mut out = Vec::new();
        if !implicit {
            out.push(ReportColumn::continuous(
                "predicted",
                "Save predicted values",
                name.clone(),
                self.predicted_all.clone(),
            ));
            if let Ok(f) = &self.formula {
                out.push(ReportColumn {
                    formula: Some(f.clone()),
                    ..ReportColumn::continuous(
                        "prediction_formula",
                        "Save prediction formula",
                        format!("{name} formula"),
                        self.predicted_all.clone(),
                    )
                });
            }
            out.push(ReportColumn::continuous(
                "residual",
                "Save residuals",
                format!("Residual {}", self.response_name),
                residual,
            ));
        } else {
            out.push(ReportColumn::continuous(
                "residual",
                "Save distances to the curve",
                format!("Distance to curve ({})", self.response_name),
                residual,
            ));
        }
        if !self.inverse.is_empty() {
            let mut v = vec![f64::NAN; n];
            for &(r, _, p) in &self.inverse {
                v[r] = p.estimate.unwrap_or(f64::NAN);
            }
            let x = self.x_name.clone().unwrap_or_default();
            out.push(ReportColumn::continuous(
                "inverse_predicted",
                "Save inverse predictions",
                format!("Inverse predicted {x}"),
                v,
            ));
        }
        out
    }
}

impl SurfaceModel for NonlinearAnalysis {
    fn response_name(&self) -> &str {
        &self.response_name
    }

    fn profile_factors(&self) -> Vec<ProfileFactor> {
        let mut out: Vec<ProfileFactor> = self
            .predictor
            .names
            .iter()
            .zip(&self.factor_ranges)
            .map(|(name, &(lo, hi, _))| ProfileFactor {
                name: name.clone(),
                kind: ProfileFactorKind::Continuous { range: (lo, hi) },
            })
            .collect();
        if self.groups.len() > 1 {
            out.push(ProfileFactor {
                name: self.group_name.clone().unwrap_or_default(),
                kind: ProfileFactorKind::Categorical { levels: self.groups.clone() },
            });
        }
        out
    }

    fn default_profile_settings(&self) -> Vec<TermValue> {
        let mut s: Vec<TermValue> = self.factor_ranges.iter().map(|r| TermValue::Number(r.2)).collect();
        if self.groups.len() > 1 {
            s.push(TermValue::Level(0));
        }
        s
    }

    fn predict_settings(&self, points: &[Vec<TermValue>]) -> Result<Vec<f64>> {
        Ok(self.predict_with_limits(points)?.into_iter().map(|p| p.0).collect())
    }

    fn predict_with_limits(&self, points: &[Vec<TermValue>]) -> Result<Vec<Limited>> {
        let k = self.predictor.columns.len();
        let mut by_group: Vec<Vec<(usize, Vec<f64>)>> = vec![Vec::new(); self.groups.len()];
        for (i, s) in points.iter().enumerate() {
            if s.len() != k + usize::from(self.groups.len() > 1) {
                return Err(invalid("the settings do not match the model's columns"));
            }
            let values: Vec<f64> = s[..k]
                .iter()
                .map(|v| match v {
                    TermValue::Number(x) => *x,
                    TermValue::Level(_) => f64::NAN,
                })
                .collect();
            let g = match s.get(k) {
                Some(TermValue::Level(g)) if *g < self.groups.len() => *g,
                _ => 0,
            };
            by_group[g].push((i, values));
        }
        let mut out = vec![(f64::NAN, None); points.len()];
        for (g, items) in by_group.iter().enumerate() {
            if items.is_empty() {
                continue;
            }
            let pts: Vec<Vec<f64>> = items.iter().map(|(_, v)| v.clone()).collect();
            match self.predictor.with_limits(self.fit(), g, &pts, &|_| 1.0) {
                Some(l) => {
                    for ((i, _), (v, m, _)) in items.iter().zip(l) {
                        out[*i] = (v, Some(m));
                    }
                }
                None => {
                    if let Some(v) = self.predictor.values(g, &pts, &self.fit().estimates()) {
                        for ((i, _), v) in items.iter().zip(v) {
                            out[*i] = (v, None);
                        }
                    }
                }
            }
        }
        Ok(out)
    }

    fn confidence_level(&self) -> Option<f64> {
        Some(self.fit().confidence_level)
    }

    fn observations(&self) -> Vec<Observation> {
        self.rows_used
            .iter()
            .enumerate()
            .map(|(i, &row)| {
                let mut settings: Vec<TermValue> =
                    self.observed_settings[i].iter().map(|&v| TermValue::Number(v)).collect();
                if self.groups.len() > 1 {
                    settings.push(TermValue::Level(self.group_used[i]));
                }
                Observation { row, settings, response: self.y_used[i] }
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Project-file form
// ---------------------------------------------------------------------------

/// The JSON form of a nonlinear regression report.
pub(crate) fn to_json(request: &NonlinearRequest, profilers: ProfilerOptions, table: &DataTable) -> JsonValue {
    let col = |i: usize| s(table.column(i).map_or("", |c| c.name.as_str()));
    let num = JsonValue::number;
    let opt = |v: Option<f64>| v.map_or(JsonValue::Null, num);
    let mut members = vec![("analysis", s("nonlinear-regression")), ("response", col(request.response))];
    match &request.model {
        NonlinearModel::Library { form, predictor } => {
            members.push(("form", s(form.key())));
            members.push(("predictor", col(*predictor)));
        }
        NonlinearModel::Formula { formula, .. } => members.push(("formula", s(formula))),
        NonlinearModel::Differential { rate, predictor, initial, start } => {
            members.push(("rate", s(rate)));
            members.push(("predictor", col(*predictor)));
            members.push(("initial", s(initial)));
            members.push(("start_x", opt(*start)));
        }
        NonlinearModel::Implicit { equation, .. } => members.push(("equation", s(equation))),
    }
    let parameters = request
        .parameters
        .iter()
        .map(|p| {
            JsonValue::Object(vec![
                ("name".to_owned(), s(&p.name)),
                ("start".to_owned(), opt(p.start)),
                ("lower".to_owned(), opt(p.lower)),
                ("upper".to_owned(), opt(p.upper)),
                ("shared".to_owned(), JsonValue::Bool(p.shared)),
            ])
        })
        .collect();
    members.push(("parameters", JsonValue::Array(parameters)));
    members.push(("level", num(request.confidence_level)));
    members.push(("profile", JsonValue::Bool(request.profile_intervals)));
    members.push(("max_iterations", num(request.max_iterations as f64)));
    if let Some(g) = request.group {
        members.push(("group", col(g)));
    }
    members.push((
        "weighting",
        match request.weighting {
            Weighting::Equal => s("equal"),
            Weighting::InverseResponseSquared => s("inverse_y2"),
            Weighting::InverseResponse => s("inverse_y"),
            Weighting::Column(c) => JsonValue::Object(vec![("column".to_owned(), col(c))]),
        },
    ));
    match request.method {
        FitMethod::LeastSquares => members.push(("method", s("least_squares"))),
        FitMethod::Robust => members.push(("method", s("robust"))),
        FitMethod::RemoveOutliers { q } => {
            members.push(("method", s("outliers")));
            members.push(("q", num(q)));
        }
    }
    if let Some(f) = request.compare_with {
        members.push(("compare", s(f.key())));
    }
    if let Some(t) = &request.template {
        let mut m = vec![("key".to_owned(), s(t.template.key())), ("x".to_owned(), col(t.x))];
        if let Some(j) = t.second {
            m.push(("second".to_owned(), col(j)));
        }
        if let Some(j) = t.dilution {
            m.push(("dilution".to_owned(), col(j)));
        }
        let constants = t
            .template
            .all_constants()
            .iter()
            .zip(&t.constants)
            .map(|(c, v)| (c.key.to_owned(), if v.is_finite() { num(*v) } else { JsonValue::Null }))
            .collect();
        m.push(("constants".to_owned(), JsonValue::Object(constants)));
        m.push(("cv_limit".to_owned(), num(t.cv_limit)));
        m.push(("tolerance".to_owned(), num(t.acceptance.tolerance_percent)));
        m.push(("end_tolerance".to_owned(), num(t.acceptance.end_tolerance_percent)));
        m.push(("min_fraction".to_owned(), num(t.acceptance.min_fraction)));
        m.push(("min_levels".to_owned(), num(t.acceptance.min_levels as f64)));
        if let Some(st) = &t.standard {
            m.push(("standard".to_owned(), s(st)));
        }
        members.push(("template", JsonValue::Object(m)));
    }
    members.push(("show_profiler", JsonValue::Bool(profilers.show_profiler)));
    members.push(("show_surface_profiler", JsonValue::Bool(profilers.show_surface_profiler)));
    members.push(("show_observed", JsonValue::Bool(profilers.show_observed)));
    JsonValue::Object(members.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

/// Read a nonlinear regression report from its JSON form.
pub(crate) fn from_json(v: &JsonValue, table: &DataTable) -> Result<SavedReport> {
    let find =
        |name: &str| table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")));
    let col = |key: &str| -> Result<usize> {
        let name = member(v, key)?.as_str().ok_or_else(|| invalid(format!("report {key}: expected a column name")))?;
        find(name)
    };
    let response = col("response")?;
    let text = |key: &str| v.get(key).and_then(JsonValue::as_str);
    let mut request = if let Some(formula) = text("formula") {
        NonlinearRequest::formula(table, response, formula, &[])
    } else if let Some(rate) = text("rate") {
        let mut r =
            NonlinearRequest::differential(response, col("predictor")?, rate, text("initial").unwrap_or(""), &[]);
        r.profile_intervals = true;
        if let NonlinearModel::Differential { start, .. } = &mut r.model {
            *start = v.get("start_x").and_then(JsonValue::as_f64);
        }
        r
    } else if let Some(equation) = text("equation") {
        NonlinearRequest::implicit(table, response, equation, &[])
    } else if let Some(tv) = v.get("template").filter(|t| !matches!(t, JsonValue::Null)) {
        // A template alone (as a script writes it): its formula, starts,
        // bounds and weighting from the data.
        let group = text("group").map(find).transpose()?;
        template_from_json(tv, table)?.request(table, response, group)?
    } else {
        let key = text("form").unwrap_or("");
        let form =
            CurveForm::from_key(key).ok_or_else(|| invalid(format!("nonlinear regression: unknown form {key:?}")))?;
        NonlinearRequest::library(response, col("predictor")?, form)
    };
    let opt = |p: &JsonValue, key: &str| p.get(key).and_then(JsonValue::as_f64);
    match v.get("parameters") {
        None | Some(JsonValue::Null) => {}
        Some(JsonValue::Array(items)) => {
            let settings: Vec<ParameterSetting> = items
                .iter()
                .map(|p| {
                    let name = p
                        .get("name")
                        .and_then(JsonValue::as_str)
                        .ok_or_else(|| invalid("nonlinear regression: each parameter needs a name"))?;
                    Ok(ParameterSetting {
                        name: name.to_owned(),
                        start: opt(p, "start"),
                        lower: opt(p, "lower"),
                        upper: opt(p, "upper"),
                        shared: p.get("shared").and_then(JsonValue::as_bool).unwrap_or(true),
                    })
                })
                .collect::<Result<_>>()?;
            if let NonlinearModel::Library { form, .. } = request.model {
                if settings.len() != form.parameter_names().len() {
                    return Err(invalid(format!(
                        "nonlinear regression: the {} model has {} parameters",
                        form.label(),
                        form.parameter_names().len()
                    )));
                }
            }
            request.parameters = settings;
        }
        Some(_) => return Err(invalid("nonlinear regression: parameters must be a list")),
    }
    if let Some(level) = v.get("level").and_then(JsonValue::as_f64) {
        request.confidence_level = level;
    }
    if let Some(b) = v.get("profile").and_then(JsonValue::as_bool) {
        request.profile_intervals = b;
    }
    if let Some(k) = v.get("max_iterations").and_then(JsonValue::as_usize) {
        request.max_iterations = k;
    }
    if let Some(g) = text("group") {
        request.group = Some(find(g)?);
    }
    request.weighting = match v.get("weighting") {
        None | Some(JsonValue::Null) => request.weighting,
        Some(w) => match w.as_str() {
            Some("equal") => Weighting::Equal,
            Some("inverse_y2") => Weighting::InverseResponseSquared,
            Some("inverse_y") => Weighting::InverseResponse,
            Some(other) => return Err(invalid(format!("nonlinear regression: unknown weighting {other:?}"))),
            None => {
                let name = w
                    .get("column")
                    .and_then(JsonValue::as_str)
                    .ok_or_else(|| invalid("nonlinear regression: weighting needs a column"))?;
                Weighting::Column(find(name)?)
            }
        },
    };
    request.method = match text("method") {
        None | Some("least_squares") => FitMethod::LeastSquares,
        Some("robust") => FitMethod::Robust,
        Some("outliers") => FitMethod::RemoveOutliers { q: v.get("q").and_then(JsonValue::as_f64).unwrap_or(0.01) },
        Some(other) => return Err(invalid(format!("nonlinear regression: unknown method {other:?}"))),
    };
    if let FitMethod::RemoveOutliers { q } = request.method {
        if !(q > 0.0 && q < 1.0) {
            return Err(invalid("nonlinear regression: q must be between 0 and 1"));
        }
    }
    if let Some(key) = text("compare") {
        request.compare_with = Some(
            CurveForm::from_key(key).ok_or_else(|| invalid(format!("nonlinear regression: unknown form {key:?}")))?,
        );
    }
    if let Some(tv) = v.get("template").filter(|t| !matches!(t, JsonValue::Null)) {
        request.template = Some(Box::new(template_from_json(tv, table)?));
    }
    let defaults = ProfilerOptions::default();
    let flag = |key: &str, d: bool| v.get(key).and_then(JsonValue::as_bool).unwrap_or(d);
    let profilers = ProfilerOptions {
        show_profiler: flag("show_profiler", false),
        show_surface_profiler: flag("show_surface_profiler", defaults.show_surface_profiler),
        show_observed: flag("show_observed", defaults.show_observed),
    };
    Ok(SavedReport::Nonlinear { request, profilers })
}

/// The template settings of a report's JSON form.
fn template_from_json(v: &JsonValue, table: &DataTable) -> Result<TemplateSettings> {
    use crate::curve_templates::CurveTemplate;
    let key = v.get("key").and_then(JsonValue::as_str).unwrap_or("");
    let template =
        CurveTemplate::from_key(key).ok_or_else(|| invalid(format!("curve template: unknown template {key:?}")))?;
    let col = |key: &str| -> Result<Option<usize>> {
        match v.get(key).and_then(JsonValue::as_str) {
            None => Ok(None),
            Some(name) => {
                table.find(name).map(Some).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")))
            }
        }
    };
    let x = col("x")?.ok_or_else(|| invalid("curve template: x column missing"))?;
    let mut t = TemplateSettings::new(template, x);
    t.second = col("second")?;
    t.dilution = col("dilution")?;
    if let Some(c) = v.get("constants") {
        for (spec, slot) in template.all_constants().iter().zip(t.constants.iter_mut()) {
            match c.get(spec.key) {
                Some(JsonValue::Null) => *slot = f64::NAN,
                Some(value) => {
                    *slot = value
                        .as_f64()
                        .ok_or_else(|| invalid(format!("curve template: {} must be a number", spec.key)))?;
                }
                None => {}
            }
        }
    }
    let f = |key: &str| v.get(key).and_then(JsonValue::as_f64);
    if let Some(x) = f("cv_limit") {
        t.cv_limit = x;
    }
    if let Some(x) = f("tolerance") {
        t.acceptance.tolerance_percent = x;
    }
    if let Some(x) = f("end_tolerance") {
        t.acceptance.end_tolerance_percent = x;
    }
    if let Some(x) = f("min_fraction") {
        t.acceptance.min_fraction = x;
    }
    if let Some(x) = v.get("min_levels").and_then(JsonValue::as_usize) {
        t.acceptance.min_levels = x;
    }
    t.standard = v.get("standard").and_then(JsonValue::as_str).map(str::to_owned);
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    /// Treloar's Puromycin data (treated cells), as in Bates & Watts (1988).
    fn table() -> DataTable {
        DataTable::new(
            "puromycin",
            vec![
                Column::numeric(
                    "Conc",
                    vec![0.02, 0.02, 0.06, 0.06, 0.11, 0.11, 0.22, 0.22, 0.56, 0.56, 1.10, 1.10, f64::NAN],
                ),
                Column::numeric(
                    "Rate",
                    vec![76.0, 47.0, 97.0, 107.0, 123.0, 139.0, 159.0, 152.0, 191.0, 201.0, 207.0, 200.0, 150.0],
                ),
            ],
        )
        .unwrap()
    }

    #[test]
    fn library_and_formula_give_the_same_fit() {
        let t = table();
        let rows = RowStateManager::new(t.n_rows());
        let lib =
            NonlinearAnalysis::run(&t, &rows, &NonlinearRequest::library(1, 0, CurveForm::MichaelisMenten)).unwrap();
        let req = NonlinearRequest::formula(&t, 1, "Vmax * Conc / (K + Conc)", &[("Vmax", 200.0), ("K", 0.1)]);
        assert_eq!(req.columns(), vec![1, 0]);
        let frm = NonlinearAnalysis::run(&t, &rows, &req).unwrap();
        for (a, b) in lib.fit().estimates().iter().zip(frm.fit().estimates()) {
            assert!((a - b).abs() < 1e-7 * a.abs(), "{a} vs {b}");
        }
        // Published: Vmax 212.7, K 0.06412 (Bates & Watts 1988).
        assert!((lib.fit().parameters[0].estimate - 212.68).abs() < 0.01);
        assert_eq!(lib.fit().n, 12);
        assert!(lib.fit().analytic_derivatives && !frm.fit().analytic_derivatives);
        let s = lib.sections();
        let kinds: Vec<SectionKind> = s.iter().map(|x| x.kind).collect();
        assert_eq!(
            kinds,
            vec![
                SectionKind::NonlinearSummary,
                SectionKind::NonlinearEstimates,
                SectionKind::NonlinearCorrelation,
                SectionKind::NonlinearCurve,
                SectionKind::NonlinearResiduals,
                SectionKind::NonlinearInverse,
                SectionKind::PredictionFormula,
            ]
        );
        let SectionContent::Plot(p) = &s[3].content else { panic!() };
        assert_eq!(p.points.len(), 12);
        let band = p.band.as_ref().unwrap();
        let line = p.line.as_ref().unwrap();
        for i in 0..line.len() {
            assert!(band.0[i][1] < line[i][1] && line[i][1] < band.1[i][1]);
            assert!(p.segments[0][i][1] < band.0[i][1] && p.segments[1][i][1] > band.1[i][1]);
        }
        // The formula model reads one column, so it also gets the curve.
        let fs = frm.sections();
        let SectionContent::Plot(fp) = &fs[3].content else { panic!() };
        assert!(fp.band.is_some());
        for (a, b) in fp.band.as_ref().unwrap().0.iter().zip(&band.0) {
            assert!((a[1] - b[1]).abs() < 1e-4 * b[1].abs(), "{a:?} vs {b:?}");
        }
    }

    #[test]
    fn saved_formulas_reproduce_the_predictions() {
        let mut t = table();
        let rows = RowStateManager::new(t.n_rows());
        let requests = [
            NonlinearRequest::library(1, 0, CurveForm::MichaelisMenten),
            NonlinearRequest::formula(&t, 1, "Vmax * Conc / (K + Conc)", &[("Vmax", 200.0), ("K", 0.1)]),
        ];
        for req in requests {
            let a = NonlinearAnalysis::run(&t, &rows, &req).unwrap();
            let cols = a.saveable_columns();
            let keys: Vec<&str> = cols.iter().map(|c| c.key.as_str()).collect();
            // Row 13 has a rate and no concentration: it is read off the curve.
            assert_eq!(keys, ["predicted", "prediction_formula", "residual", "inverse_predicted"]);
            assert!(cols[0].values[12].is_nan(), "row with missing Conc");
            let formula = cols[1].formula.clone().unwrap();
            let eval = crate::formula::evaluate(&t, None, &formula, 0..t.n_rows()).unwrap();
            for (r, v) in eval.values.iter().enumerate().take(12) {
                let crate::formula::FormulaValue::Number(v) = v else { panic!("{formula}: {v:?}") };
                assert!((v - cols[0].values[r]).abs() < 1e-9 * v.abs(), "{formula}");
            }
            for r in 0..12 {
                let y = t.column(1).unwrap().as_numeric().unwrap()[r];
                assert!((cols[0].values[r] + cols[2].values[r] - y).abs() < 1e-9);
            }
            let _ = &mut t;
        }
    }

    #[test]
    fn errors_and_project_form() {
        let t = table();
        let rows = RowStateManager::new(t.n_rows());
        // Missing start, clashing name, bad formula.
        let mut req = NonlinearRequest::formula(&t, 1, "a * Conc", &[("a", 1.0)]);
        req.parameters[0].start = None;
        assert!(NonlinearAnalysis::run(&t, &rows, &req).unwrap_err().to_string().contains("starting value"));
        let req = NonlinearRequest::formula(&t, 1, "Conc * 2", &[("Conc", 1.0)]);
        assert!(NonlinearAnalysis::run(&t, &rows, &req).unwrap_err().to_string().contains("same name as a column"));
        let req = NonlinearRequest::formula(&t, 1, "a * Conc +", &[("a", 1.0)]);
        assert!(NonlinearAnalysis::run(&t, &rows, &req).is_err());
        let req = NonlinearRequest::formula(&t, 1, "a * ln(Conc - 1)", &[("a", 1.0)]);
        let e = NonlinearAnalysis::run(&t, &rows, &req).unwrap_err().to_string();
        assert!(e.contains("starting values"), "{e}");

        // Excluded rows leave the fit; the report says how many.
        let mut ex = RowStateManager::new(t.n_rows());
        ex.set_excluded([0, 1], true);
        let a = NonlinearAnalysis::run(&t, &ex, &NonlinearRequest::library(1, 0, CurveForm::MichaelisMenten)).unwrap();
        assert_eq!(a.fit().n, 10);
        assert!(a.saveable_columns()[0].values[0].is_finite(), "excluded rows are still predicted");

        // Bounds and the JSON round trip.
        let mut req = NonlinearRequest::library(1, 0, CurveForm::MichaelisMenten);
        req.parameters[0].upper = Some(200.0);
        req.parameters[1].start = Some(0.05);
        req.profile_intervals = false;
        let a = NonlinearAnalysis::run(&t, &rows, &req).unwrap();
        assert_eq!(a.fit().parameters[0].bound, BoundState::Upper);
        assert!(a.sections().iter().any(|s| s.kind == SectionKind::NonlinearWarnings));
        // A parameter fixed by equal bounds is not estimated: one residual
        // degree of freedom more than with both estimated.
        let mut fixed = NonlinearRequest::library(1, 0, CurveForm::MichaelisMenten);
        fixed.parameters[1].lower = Some(0.06);
        fixed.parameters[1].upper = Some(0.06);
        let a = NonlinearAnalysis::run(&t, &rows, &fixed).unwrap();
        let free =
            NonlinearAnalysis::run(&t, &rows, &NonlinearRequest::library(1, 0, CurveForm::MichaelisMenten)).unwrap();
        assert_eq!(a.fit().df, free.fit().df + 1);
        let SectionContent::Table(summary) = &a.sections()[0].content else { panic!() };
        assert!(summary.rows.iter().any(|r| r[1] == "2 (1 estimated, 1 fixed by equal bounds)"), "{summary:?}");
        // The same with a typed formula, whose derivatives are numeric: the
        // fixed parameter has no room for a difference step, which used to
        // fail the fit at the starting values. The estimate matches the
        // library form's (analytic derivatives).
        let mut typed = NonlinearRequest::formula(&t, 1, "Vmax * Conc / (K + Conc)", &[("Vmax", 200.0), ("K", 0.06)]);
        typed.parameters[1].lower = Some(0.06);
        typed.parameters[1].upper = Some(0.06);
        let b = NonlinearAnalysis::run(&t, &rows, &typed).unwrap();
        assert_eq!(b.fit().df, a.fit().df);
        let (va, vb) = (a.fit().parameters[0].estimate, b.fit().parameters[0].estimate);
        assert!((va - vb).abs() < 1e-6 * va.abs(), "{va} vs {vb}");
        assert_eq!(b.fit().parameters[1].estimate, 0.06);
        for r in [req, NonlinearRequest::formula(&t, 1, "Vmax * Conc / (K + Conc)", &[("Vmax", 200.0), ("K", 0.1)])] {
            let json = to_json(&r, NonlinearAnalysis::default_profilers(), &t);
            let SavedReport::Nonlinear { request, .. } = from_json(&json, &t).unwrap() else { panic!() };
            assert_eq!(request, r);
        }
    }

    fn close(a: f64, b: f64, rel: f64, what: &str) {
        assert!((a - b).abs() <= rel * b.abs().max(1e-12), "{what}: {a} vs {b}");
    }

    /// Two straight lines: the test that the slopes differ is the classical
    /// parallel-lines F test of analysis of covariance, computed here by
    /// ordinary least squares with indicator columns.
    #[test]
    fn comparing_straight_lines_is_the_parallel_lines_test() {
        let x = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 3.5];
        let y = vec![2.3, 4.1, 5.8, 8.4, 9.9, 12.2, 1.1, 2.9, 3.9, 5.2, 6.1, 7.8, 7.0];
        let g: Vec<String> = (0..13).map(|i| if i < 6 || i == 12 { "A" } else { "B" }.to_owned()).collect();
        let t = DataTable::new(
            "lines",
            vec![Column::numeric("X", x.clone()), Column::numeric("Y", y.clone()), Column::text("Line", g.clone())],
        )
        .unwrap();
        let rows = RowStateManager::new(t.n_rows());
        let mut req = NonlinearRequest::library(1, 0, CurveForm::StraightLine);
        req.group = Some(2);
        for p in &mut req.parameters {
            p.shared = false;
        }
        let a = NonlinearAnalysis::run(&t, &rows, &req).unwrap();
        assert_eq!(a.fit().parameters.len(), 4);
        // OLS by QR: separate lines vs parallel lines.
        let ind: Vec<f64> = g.iter().map(|l| f64::from(u8::from(l == "B"))).collect();
        let ones = vec![1.0; 13];
        let xb: Vec<f64> = x.iter().zip(&ind).map(|(a, b)| a * b).collect();
        let sse = |cols: Vec<Vec<f64>>| {
            let m = stat_core::Matrix::from_columns(&cols).unwrap();
            let qr = stat_core::QrDecomposition::new(m.clone()).unwrap();
            let b = qr.solve_least_squares(&y).unwrap();
            (0..13).map(|i| (y[i] - (0..cols.len()).map(|j| cols[j][i] * b[j]).sum::<f64>()).powi(2)).sum::<f64>()
        };
        let full = sse(vec![ones.clone(), ind.clone(), x.clone(), xb]);
        let parallel = sse(vec![ones.clone(), ind.clone(), x.clone()]);
        let one = sse(vec![ones, x.clone()]);
        close(a.fit().sse, full, 1e-9, "separate lines");
        let f_slopes = (parallel - full) / (full / 9.0);
        let f_all = ((one - full) / 2.0) / (full / 9.0);
        let slope = a.comparisons.iter().find(|c| c.label.contains("Slope")).unwrap();
        let (s0, s1) = slope.outcome.clone().unwrap();
        close(s0.sse, parallel, 1e-9, "parallel lines");
        close(extra_sum_of_squares(s0, s1).unwrap().f, f_slopes, 1e-8, "F slopes");
        let all = &a.comparisons[0];
        let (s0, s1) = all.outcome.clone().unwrap();
        close(extra_sum_of_squares(s0, s1).unwrap().f, f_all, 1e-8, "F one line");
        assert!(a.sections().iter().any(|s| s.kind == SectionKind::NonlinearComparison));
        // The grouped prediction formula reproduces each line.
        let cols = a.saveable_columns();
        let formula = cols.iter().find(|c| c.key == "prediction_formula").unwrap().formula.clone().unwrap();
        let eval = crate::formula::evaluate(&t, None, &formula, 0..t.n_rows()).unwrap();
        for (r, v) in eval.values.iter().enumerate() {
            let crate::formula::FormulaValue::Number(v) = v else { panic!("{formula}: {v:?}") };
            close(*v, cols[0].values[r], 1e-9, "grouped formula");
        }
    }

    #[test]
    fn global_fit_with_nothing_shared_equals_separate_fits() {
        let t = crate::sample::nonlinear::seedling_growth();
        let rows = RowStateManager::new(t.n_rows());
        let mut req = NonlinearRequest::library(2, 0, CurveForm::Logistic3);
        req.group = Some(1);
        for p in &mut req.parameters {
            p.shared = false;
        }
        let both = NonlinearAnalysis::run(&t, &rows, &req).unwrap();
        for (g, label) in ["A", "B"].iter().enumerate() {
            let mut ex = RowStateManager::new(t.n_rows());
            let other: Vec<usize> = (0..t.n_rows()).filter(|&r| t.columns()[1].level_text(r) != *label).collect();
            ex.set_excluded(other, true);
            let one = NonlinearAnalysis::run(&t, &ex, &NonlinearRequest::library(2, 0, CurveForm::Logistic3)).unwrap();
            for k in 0..3 {
                let i = both.outcome.owner.iter().position(|&o| o == (k, Some(g))).unwrap();
                close(both.fit().parameters[i].estimate, one.fit().parameters[k].estimate, 1e-6, "separate fits");
            }
        }
        // One curve for all: the ungrouped fit.
        let single = NonlinearAnalysis::run(&t, &rows, &NonlinearRequest::library(2, 0, CurveForm::Logistic3)).unwrap();
        let (s0, _) = both.comparisons[0].outcome.clone().unwrap();
        close(s0.sse, single.fit().sse, 1e-8, "one curve");
        // Sharing the asymptote: fewer parameters, curves per data set.
        req.parameters[0].shared = true;
        let shared = NonlinearAnalysis::run(&t, &rows, &req).unwrap();
        assert_eq!(shared.fit().parameters.len(), 5);
        assert_eq!(shared.fit().parameters[0].name, "Asymptote");
        assert_eq!(shared.fit().parameters[1].name, "Rate (A)");
        let SectionContent::Plot(p) = &shared.curve_plot().content else { panic!() };
        assert_eq!(p.groups.as_ref().unwrap().lines.len(), 2);
        // Profilers: a continuous factor and the data set.
        let m = shared.surface_model().unwrap();
        assert_eq!(m.profile_factors().len(), 2);
        let pred = m.predict_settings(&[vec![TermValue::Number(20.0), TermValue::Level(1)]]).unwrap();
        let theta = shared.fit().estimates();
        let local: Vec<f64> = shared.outcome.map[1].iter().map(|&i| theta[i]).collect();
        close(pred[0], CurveForm::Logistic3.value(20.0, &local), 1e-12, "profiler prediction");
    }

    #[test]
    fn weights_outliers_and_inverse_prediction() {
        let t = table();
        let rows = RowStateManager::new(t.n_rows());
        // 1/Y² weights: the weighted reference fit (stat-linear's SciPy case).
        let mut req = NonlinearRequest::library(1, 0, CurveForm::MichaelisMenten);
        req.weighting = Weighting::InverseResponseSquared;
        let a = NonlinearAnalysis::run(&t, &rows, &req).unwrap();
        close(a.fit().parameters[0].estimate, 206.961013245904, 1e-7, "weighted Vmax");
        close(a.fit().parameters[1].std_error, 0.008783719922822853, 1e-5, "weighted se Km");
        // A gross outlier is found and removed.
        let mut t2 = table();
        t2.set_value(3, 1, stat_data::CellValue::Number(160.0)).unwrap();
        let mut req = NonlinearRequest::library(1, 0, CurveForm::MichaelisMenten);
        req.method = FitMethod::RemoveOutliers { q: 0.01 };
        let b = NonlinearAnalysis::run(&t2, &rows, &req).unwrap();
        assert_eq!(b.outlier_rows(), vec![3]);
        assert_eq!(b.fit().n, 11);
        assert!(b.sections().iter().any(|s| s.kind == SectionKind::NonlinearOutliers));
        req.method = FitMethod::Robust;
        let c = NonlinearAnalysis::run(&t2, &rows, &req).unwrap();
        close(c.fit().parameters[0].estimate, 216.61025058472944, 1e-5, "robust Vmax");
        // Row 13 has a response and no concentration: read off the curve.
        let a =
            NonlinearAnalysis::run(&t, &rows, &NonlinearRequest::library(1, 0, CurveForm::MichaelisMenten)).unwrap();
        let inv = a.inverse_predictions();
        assert_eq!(inv.len(), 1);
        let (row, p) = inv[0];
        assert_eq!(row, 12);
        let x0 = p.estimate.unwrap();
        close(CurveForm::MichaelisMenten.value(x0, &a.fit().estimates()), 150.0, 1e-9, "inverse");
        assert!(p.lower.unwrap() < x0 && x0 < p.upper.unwrap());
        assert!(a.saveable_columns().iter().any(|c| c.key == "inverse_predicted" && (c.values[12] - x0).abs() < 1e-12));
    }

    #[test]
    fn differential_equation_matches_the_closed_form() {
        // First-order decay: dC/dt = −k·C, C(0) = C0, i.e. C0·exp(−k·t).
        let time = vec![0.0, 0.5, 1.0, 2.0, 3.0, 4.0, 6.0, 8.0];
        let conc = vec![10.2, 7.6, 6.1, 3.6, 2.4, 1.3, 0.6, 0.25];
        let t = DataTable::new("decay", vec![Column::numeric("Time", time), Column::numeric("C", conc)]).unwrap();
        let rows = RowStateManager::new(t.n_rows());
        let mut ode = NonlinearRequest::differential(1, 0, "-k * C", "C0", &[("k", 0.3), ("C0", 9.0)]);
        ode.profile_intervals = true;
        let a = NonlinearAnalysis::run(&t, &rows, &ode).unwrap();
        let b = NonlinearAnalysis::run(&t, &rows, &NonlinearRequest::library(1, 0, CurveForm::Exponential)).unwrap();
        close(a.fit().parameters[0].estimate, -b.fit().parameters[1].estimate, 1e-6, "k");
        close(a.fit().parameters[1].estimate, b.fit().parameters[0].estimate, 1e-6, "C0");
        close(a.fit().parameters[0].std_error, b.fit().parameters[1].std_error, 1e-4, "se k");
        assert!(a.fit().parameters[0].profile_interval.is_some());
        assert!(a.prediction_formula().is_err());
        let SectionContent::Plot(p) = &a.curve_plot().content else { panic!() };
        assert!(p.band.is_some());
        // The rate may only use the predictor and the response.
        let bad = NonlinearRequest::differential(1, 0, "-k * C * Other", "C0", &[("k", 0.3), ("C0", 9.0)]);
        assert!(NonlinearAnalysis::run(&t, &rows, &bad).is_err());
    }

    #[test]
    fn implicit_circle_and_surface() {
        let v = parse_reference();
        let (x, y) = (v.0, v.1);
        let t = DataTable::new("circle", vec![Column::numeric("X", x), Column::numeric("Y", y)]).unwrap();
        let rows = RowStateManager::new(t.n_rows());
        let req =
            NonlinearRequest::implicit(&t, 1, "(X - a)**2 + (Y - b)**2 - r**2", &[("a", 1.5), ("b", -0.5), ("r", 2.5)]);
        let a = NonlinearAnalysis::run(&t, &rows, &req).unwrap();
        for (j, e) in v.2.iter().enumerate() {
            close(a.fit().parameters[j].estimate.abs(), e.abs(), 1e-6, "circle");
        }
        assert!(a.surface_model().is_none());
        let SectionContent::Plot(p) = &a.curve_plot().content else { panic!() };
        assert!(!p.segments.is_empty());
        // A surface z = a + b·X + c·Y².
        let xs: Vec<f64> = (0..20).map(|i| f64::from(i % 5)).collect();
        let ys: Vec<f64> = (0..20).map(|i| f64::from(i / 5)).collect();
        let z: Vec<f64> = xs
            .iter()
            .zip(&ys)
            .enumerate()
            .map(|(i, (a, b))| 1.0 + 2.0 * a + 0.5 * b * b + 0.01 * (i % 3) as f64)
            .collect();
        let t = DataTable::new(
            "surface",
            vec![Column::numeric("X", xs), Column::numeric("Y", ys), Column::numeric("Z", z)],
        )
        .unwrap();
        let rows = RowStateManager::new(t.n_rows());
        let req = NonlinearRequest::formula(&t, 2, "p + q * X + s * Y * Y", &[("p", 0.0), ("q", 1.0), ("s", 1.0)]);
        let a = NonlinearAnalysis::run(&t, &rows, &req).unwrap();
        assert!(a.sections().iter().any(|s| s.kind == SectionKind::NonlinearSurface));
        let m = a.surface_model().unwrap();
        let pr = m.predict_with_limits(&[vec![TermValue::Number(2.0), TermValue::Number(3.0)]]).unwrap();
        let e = a.fit().estimates();
        close(pr[0].0, e[0] + 2.0 * e[1] + 9.0 * e[2], 1e-10, "surface prediction");
        assert!(pr[0].1.is_some());
    }

    /// The circle case of stat-linear's reference file.
    fn parse_reference() -> (Vec<f64>, Vec<f64>, Vec<f64>) {
        let v =
            stat_data::parse_json(include_str!("../../stat-linear/tests/data/curve_fitting_reference.json")).unwrap();
        let c = v.get("circle").unwrap();
        let nums = |k: &str| c.get(k).unwrap().as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
        (nums("x"), nums("y"), nums("estimates"))
    }

    #[test]
    fn options_round_trip_and_comparison_with_another_form() {
        let t = crate::sample::nonlinear::seedling_growth();
        let rows = RowStateManager::new(t.n_rows());
        let mut req = NonlinearRequest::library(2, 0, CurveForm::Logistic3);
        req.compare_with = Some(CurveForm::Logistic4);
        let a = NonlinearAnalysis::run(&t, &rows, &req).unwrap();
        assert_eq!(a.comparisons.len(), 1);
        let (s0, s1) = a.comparisons[0].outcome.clone().unwrap();
        assert_eq!((s0.parameters, s1.parameters), (3, 4));
        assert!(s1.sse <= s0.sse);
        let mut r = req.clone();
        r.group = Some(1);
        r.weighting = Weighting::Column(0);
        r.method = FitMethod::RemoveOutliers { q: 0.05 };
        r.parameters[1].shared = false;
        let ode = NonlinearRequest::differential(2, 0, "k * (A - x)", "x", &[("k", 0.1), ("A", 1.0), ("x", 1.0)]);
        let imp = NonlinearRequest::implicit(&t, 2, "Day - a", &[("a", 1.0)]);
        for r in [r, ode, imp] {
            let json = to_json(&r, NonlinearAnalysis::default_profilers(), &t);
            let SavedReport::Nonlinear { request, profilers } = from_json(&json, &t).unwrap() else { panic!() };
            assert_eq!(request, r);
            assert_eq!(profilers, NonlinearAnalysis::default_profilers());
        }
    }
}
