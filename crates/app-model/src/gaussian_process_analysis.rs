//! Gaussian-process regression bound to a dataset
//! ([`stat_predictive::gaussian_process`]): a smooth surface through a
//! continuous response, for computer experiments and sequential
//! optimization, with predictions and their uncertainty everywhere.
//!
//! **Inputs.** Each continuous predictor is scaled to `[0, 1]` over the
//! training rows' range (its length scale is then a fraction of that range);
//! each level of a categorical predictor becomes a 0/1 indicator with its
//! own length scale. Rows missing a predictor or the response take no role.
//!
//! **Fit.** Maximum likelihood over the signal variance, one length scale
//! per input and the nugget (estimated, or fixed as a fraction of the
//! response's variance), from several starts. Held-out predictions are the
//! closed-form leave-one-out predictions of the training rows, or k-fold
//! refits when the request asks for folds; validation and test rows are
//! predicted by the final fit.
//!
//! **Sections.** Summary, length scales (with each input's relative
//! sensitivity `ℓ_min / ℓ_j`), fit by role, actual by held-out predicted,
//! and contour plots of the prediction and its standard error over the
//! first two continuous predictors (the others at their profiler settings).
//! The Response Explorer and 3-D profiler show 95% intervals for the mean.

use stat_core::{Result, StatError};
use stat_data::{DataTable, JsonValue, RowStateManager};
use stat_linear::TermValue;
use stat_predictive::{
    CovarianceKernel, Feature, GaussianProcess, GpOptions, Nugget, Role, Value, fit_gaussian_process, fold_assignment,
};

use crate::format::format_number;
use crate::prediction::{
    FeatureSet, InputEncoding, PredictionFormulas, Predictions, Predictor, PredictorInfo, Response, RoleAssignment,
    ValidationMethod, fit_measures_section, prediction_columns, setting_value,
};
use crate::report::{Annotations, PlotSpec, Polyline, ReportSection, RowPoint, SectionContent, SectionKind, TextTable};
use crate::surface_analysis::contour_lines;
use crate::surface_profiler::{Limited, Observation, ProfileFactor, SurfaceModel};

const DIGITS: usize = 6;
/// Normal quantile of the 95% intervals.
const Z95: f64 = 1.959_963_984_540_054;
/// Grid points per side of the contour plots.
const GRID: usize = 41;

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// Columns and settings of a Gaussian-process model.
#[derive(Debug, Clone, PartialEq)]
pub struct GaussianProcessRequest {
    /// Continuous response column.
    pub response: usize,
    /// Predictor columns.
    pub predictors: Vec<usize>,
    /// Training / validation / test roles.
    pub validation: ValidationMethod,
    /// Correlation function.
    pub kernel: CovarianceKernel,
    /// A fixed nugget as a fraction of the response's variance (`None`:
    /// estimated).
    pub nugget: Option<f64>,
    /// Starts of the likelihood search.
    pub starts: usize,
    /// Seed of the starts.
    pub seed: u64,
}

impl GaussianProcessRequest {
    /// Matérn 5/2, nugget estimated, ten starts, every row training.
    #[must_use]
    pub fn new(response: usize, predictors: Vec<usize>) -> Self {
        Self {
            response,
            predictors,
            validation: ValidationMethod::None,
            kernel: CovarianceKernel::Matern52,
            nugget: None,
            starts: 10,
            seed: 1,
        }
    }

    /// Columns read.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        let mut c = vec![self.response];
        c.extend(&self.predictors);
        c.extend(self.validation.columns());
        c
    }

    /// The same request with column indices passed through `f`.
    #[must_use]
    pub fn map_columns(mut self, f: impl Fn(usize) -> usize) -> Self {
        self.response = f(self.response);
        self.predictors.iter_mut().for_each(|c| *c = f(*c));
        self.validation = self.validation.map_columns(&f);
        self
    }

    /// The engine options.
    #[must_use]
    pub fn options(&self) -> GpOptions {
        GpOptions {
            kernel: self.kernel,
            nugget: self.nugget.map_or(Nugget::Estimated, Nugget::Fixed),
            starts: self.starts,
            seed: self.seed,
            ..GpOptions::default()
        }
    }

    /// Check the settings.
    ///
    /// # Errors
    /// A negative or non-finite nugget, or a start count outside 1–100.
    pub fn validate(&self) -> Result<()> {
        if self.nugget.is_some_and(|v| !(v >= 0.0 && v.is_finite())) {
            return Err(invalid("the nugget must not be negative"));
        }
        if self.starts == 0 || self.starts > 100 {
            return Err(invalid("use 1 to 100 starts"));
        }
        Ok(())
    }
}

/// Unit-range scaling of continuous predictors and indicators of
/// categorical ones, from the predictors' training ranges.
pub(crate) fn unit_encoding(predictors: &[PredictorInfo]) -> InputEncoding {
    let mut numeric = Vec::new();
    let mut width = Vec::new();
    for p in predictors {
        match &p.levels {
            Some(levels) => {
                numeric.push(None);
                width.push(levels.len());
            }
            None => {
                let span = p.range.1 - p.range.0;
                numeric.push(Some((p.range.0, if span > 0.0 && span.is_finite() { span } else { 1.0 })));
                width.push(1);
            }
        }
    }
    InputEncoding { numeric, width }
}

/// Names of the model inputs (a categorical predictor gives one per level).
pub(crate) fn input_names(predictors: &[PredictorInfo]) -> Vec<String> {
    let mut names = Vec::new();
    for p in predictors {
        match &p.levels {
            Some(levels) => names.extend(levels.iter().map(|l| format!("{} = {l}", p.name))),
            None => names.push(p.name.clone()),
        }
    }
    names
}

/// The summary rows describing one fitted process (shared with sequential
/// optimization).
pub(crate) fn process_rows(gp: &GaussianProcess, fixed_nugget: Option<f64>) -> Vec<[String; 2]> {
    let mut out = Vec::new();
    let mut row = |a: &str, b: String| out.push([a.to_owned(), b]);
    let formula = match gp.kernel {
        CovarianceKernel::SquaredExponential => "σ² exp(−½ Σ dⱼ²/ℓⱼ²)",
        CovarianceKernel::Matern52 => "σ² (1 + √5 ρ + 5ρ²/3) exp(−√5 ρ), ρ² = Σ dⱼ²/ℓⱼ²",
    };
    row("Covariance", format!("{}: {formula}", gp.kernel.label()));
    row("Signal standard deviation σ", format_number(gp.hyper.signal_variance.sqrt() * gp.y_scale, DIGITS));
    row(
        "Noise standard deviation (nugget)",
        format!(
            "{} ({})",
            format_number(gp.noise_sd(), DIGITS),
            match fixed_nugget {
                None => "estimated".to_owned(),
                Some(v) => format!("fixed at {} of the response variance", format_number(v, 4)),
            }
        ),
    );
    row("Log likelihood", format_number(gp.log_likelihood_original(), DIGITS));
    let starts: Vec<String> = gp
        .start_log_likelihoods
        .iter()
        .map(|v| if v.is_finite() { format_number(*v, 5) } else { "failed".to_owned() })
        .collect();
    row(
        "Starts",
        format!(
            "{}: log likelihoods (standardized response) {}; kept start {}",
            starts.len(),
            starts.join(", "),
            gp.best_start + 1
        ),
    );
    row(
        "Search",
        format!(
            "projected BFGS, {} iterations{}",
            gp.iterations,
            if gp.converged { "" } else { " (stopped at the iteration limit)" }
        ),
    );
    out
}

/// A contour section of `values` over a `GRID × GRID` grid spanning `xs`
/// and `ys`, with `points` as brushable observations.
pub(crate) fn contour_section(
    kind: SectionKind,
    title: &str,
    names: (&str, &str),
    (xs, ys): (&[f64], &[f64]),
    grid: &[Vec<f64>],
    points: Vec<RowPoint>,
) -> ReportSection {
    let (lo, hi) = grid
        .iter()
        .flatten()
        .filter(|v| v.is_finite())
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &v| (a.min(v), b.max(v)));
    let mut segments: Vec<Polyline> = Vec::new();
    let mut labels = Vec::new();
    let mut step = 0.0;
    if hi > lo {
        step = crate::axis_settings::nice_step((hi - lo) / 10.0);
        let (x0, dx) = (xs[0], xs[1] - xs[0]);
        let (y0, dy) = (ys[0], ys[1] - ys[0]);
        let mut k = (lo / step).floor() + 1.0;
        while k * step < hi && labels.len() < 44 {
            let level = k * step;
            let lines = contour_lines(grid, level, |i| i as f64);
            if let Some(longest) = lines.iter().max_by_key(|l| l.len()) {
                let [u, v] = longest[longest.len() / 2];
                labels.push((x0 + u * dx, y0 + v * dy, format_number(level, 6)));
            }
            for line in lines {
                segments.push(line.iter().map(|&[u, v]| [x0 + u * dx, y0 + v * dy]).collect());
            }
            k += 1.0;
        }
    }
    let spec = PlotSpec {
        x_label: names.0.to_owned(),
        y_label: names.1.to_owned(),
        points,
        point_name: Some("Observations".to_owned()),
        annotations: Some(Annotations { groups: Vec::new(), items: Vec::new(), labels }),
        segments,
        ..PlotSpec::default()
    };
    let title =
        if step > 0.0 { format!("{title} (contours every {})", format_number(step, 6)) } else { title.to_owned() };
    ReportSection { kind, title, content: SectionContent::Plot(Box::new(spec)) }
}

/// A fitted Gaussian-process model.
#[derive(Debug, Clone)]
pub struct GaussianProcessAnalysis {
    request: GaussianProcessRequest,
    response_name: String,
    response: Response,
    roles: RoleAssignment,
    predictors: Vec<PredictorInfo>,
    features: Vec<Feature>,
    encoding: InputEncoding,
    gp: GaussianProcess,
    predictions: Predictions,
    held_out: Predictions,
    k_fold: bool,
}

impl GaussianProcessAnalysis {
    /// Fit and evaluate the model.
    ///
    /// # Errors
    /// No predictors, the response among them or not continuous, invalid
    /// settings or validation method, fewer than three complete training
    /// rows (or more than 2,000), or a covariance matrix that cannot be
    /// factored.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &GaussianProcessRequest) -> Result<Self> {
        if request.predictors.is_empty() {
            return Err(invalid("choose at least one predictor"));
        }
        if request.predictors.contains(&request.response) {
            return Err(invalid("the response cannot also be a predictor"));
        }
        request.validate()?;
        let included: Vec<usize> = (0..table.n_rows()).filter(|&r| !states.is_excluded(r)).collect();
        let response = Response::read(table, request.response, &included)?;
        let Response::Continuous(y) = &response else {
            return Err(invalid("a Gaussian process needs a continuous response"));
        };
        let y = y.clone();
        let response_name = table.column(request.response).map_or(String::new(), |c| c.name.clone());
        let mut roles = RoleAssignment::assign(table, states, &response, &request.validation)?;
        let all_train = roles.rows(Role::Training);
        let FeatureSet { predictors, features } = FeatureSet::read(table, &request.predictors, &all_train)?;
        for (r, role) in roles.roles.iter_mut().enumerate() {
            if role.is_some() && features.iter().any(|f| f.value(r) == Value::Missing) {
                *role = None;
            }
        }
        let train = roles.rows(Role::Training);
        if train.len() < 3 {
            return Err(invalid("a Gaussian process needs at least three training rows with every predictor present"));
        }
        // Ranges over the complete training rows.
        let FeatureSet { predictors, features } = if train.len() == all_train.len() {
            FeatureSet { predictors, features }
        } else {
            FeatureSet::read(table, &request.predictors, &train)?
        };
        let encoding = unit_encoding(&predictors);
        let inputs: Vec<Option<Vec<f64>>> =
            (0..table.n_rows()).map(|r| encoding.encode(&|j| features[j].value(r))).collect();
        let fit = |rows: &[usize]| -> Result<GaussianProcess> {
            let x: Vec<Vec<f64>> = rows.iter().map(|&r| inputs[r].clone().unwrap_or_default()).collect();
            let yv: Vec<f64> = rows.iter().map(|&r| y[r]).collect();
            fit_gaussian_process(&x, &yv, &request.options())
        };
        let gp = fit(&train)?;
        let predictions =
            Predictions::Values(inputs.iter().map(|x| x.as_ref().map_or(f64::NAN, |x| gp.predict(x).0)).collect());
        let mut held = vec![f64::NAN; table.n_rows()];
        let k_fold = roles.folds.is_some();
        match roles.folds {
            Some((folds, seed)) => {
                let folds = folds.min(train.len()).max(2);
                let fold_of = fold_assignment(train.len(), folds, seed)?;
                for f in 0..folds {
                    let fit_rows: Vec<usize> =
                        train.iter().zip(&fold_of).filter(|(_, k)| **k != f).map(|(&r, _)| r).collect();
                    if fit_rows.len() < 2 {
                        continue;
                    }
                    let Ok(m) = fit(&fit_rows) else { continue };
                    for (&r, _) in train.iter().zip(&fold_of).filter(|(_, k)| **k == f) {
                        if let Some(x) = &inputs[r] {
                            held[r] = m.predict(x).0;
                        }
                    }
                }
            }
            None => {
                for (&r, (m, _)) in train.iter().zip(gp.leave_one_out()) {
                    held[r] = m;
                }
            }
        }
        Ok(Self {
            request: request.clone(),
            response_name,
            response,
            roles,
            predictors,
            features,
            encoding,
            gp,
            predictions,
            held_out: Predictions::Values(held),
            k_fold,
        })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &GaussianProcessRequest {
        &self.request
    }

    /// The fitted process (on unit-scaled inputs).
    #[must_use]
    pub fn process(&self) -> &GaussianProcess {
        &self.gp
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        let names: Vec<&str> = self.predictors.iter().map(|p| p.name.as_str()).collect();
        format!("Gaussian Process ({}): {} ~ {}", self.gp.kernel.label(), self.response_name, names.join(" + "))
    }

    /// Predicted mean and standard deviation of the mean for a row of
    /// predictor values (`None` when one is missing or unknown).
    fn predict_values(&self, value: &dyn Fn(usize) -> Value) -> Option<(f64, f64)> {
        self.encoding.encode(value).map(|x| self.gp.predict(&x))
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let mut out = vec![self.summary_section(), self.length_scale_section(), fit_measures_section(self)];
        out.push(self.actual_by_predicted());
        out.extend(self.contour_sections());
        out
    }

    fn summary_section(&self) -> ReportSection {
        let mut t = TextTable::new(["Statistic", "Value"]);
        t.push(["Response".to_owned(), self.response_name.clone()]);
        for role in Role::ALL {
            let n = self.roles.rows(role).len();
            if n > 0 || role == Role::Training {
                t.push([format!("{} rows", role.label()), n.to_string()]);
            }
        }
        let categorical = self.predictors.iter().filter(|p| p.levels.is_some()).count();
        t.push([
            "Inputs".to_owned(),
            format!(
                "{} ({} continuous predictors scaled to 0–1 over their training range{})",
                self.encoding.inputs(),
                self.predictors.len() - categorical,
                if categorical > 0 {
                    format!("; {categorical} categorical as one indicator per level")
                } else {
                    String::new()
                }
            ),
        ]);
        t.push([
            "Mean".to_owned(),
            format!("constant: the training mean {} (response standardized)", format_number(self.gp.y_mean, DIGITS)),
        ]);
        for row in process_rows(&self.gp, self.request.nugget) {
            t.push(row);
        }
        t.push(["Seed".to_owned(), self.request.seed.to_string()]);
        ReportSection {
            kind: SectionKind::GaussianProcessSummary,
            title: "Gaussian Process Summary".to_owned(),
            content: SectionContent::Table(t),
        }
    }

    fn length_scale_section(&self) -> ReportSection {
        let mut t =
            TextTable::new(["Input", "Length scale (fraction of range)", "Length scale (data units)", "Sensitivity"]);
        let ls = &self.gp.hyper.length_scales;
        let shortest = ls.iter().copied().fold(f64::INFINITY, f64::min);
        let names = input_names(&self.predictors);
        let mut k = 0;
        for (j, p) in self.predictors.iter().enumerate() {
            for _ in 0..self.encoding.width[j] {
                let data = match (&p.levels, self.encoding.numeric[j]) {
                    (None, Some((_, span))) => format_number(ls[k] * span, 4),
                    _ => "—".to_owned(),
                };
                t.push([names[k].clone(), format_number(ls[k], 4), data, format_number(shortest / ls[k], 3)]);
                k += 1;
            }
        }
        ReportSection {
            kind: SectionKind::GaussianProcessLengthScales,
            title: "Length Scales (short: the response changes quickly with the input; sensitivity 1 = shortest)"
                .to_owned(),
            content: SectionContent::Table(t),
        }
    }

    fn held_label(&self) -> &'static str {
        if self.k_fold { "Cross-validation" } else { "Leave-one-out" }
    }

    fn actual_by_predicted(&self) -> ReportSection {
        let Predictions::Values(held) = &self.held_out else { unreachable!("continuous response") };
        let Response::Continuous(y) = &self.response else { unreachable!("continuous response") };
        let points: Vec<RowPoint> = self
            .roles
            .rows(Role::Training)
            .into_iter()
            .filter(|&r| held[r].is_finite())
            .map(|r| RowPoint { row: r, x: held[r], y: y[r] })
            .collect();
        let (lo, hi) = points
            .iter()
            .flat_map(|p| [p.x, p.y])
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(v), b.max(v)));
        let spec = PlotSpec {
            x_label: format!("{} prediction of {}", self.held_label(), self.response_name),
            y_label: self.response_name.clone(),
            points,
            line: lo.is_finite().then(|| vec![[lo, lo], [hi, hi]]),
            line_name: Some("Perfect prediction".to_owned()),
            ..PlotSpec::default()
        };
        ReportSection {
            kind: SectionKind::GaussianProcessActualByPredicted,
            title: format!("Actual by {} Predicted (training rows)", self.held_label()),
            content: SectionContent::Plot(Box::new(spec)),
        }
    }

    /// The prediction and its standard error over the first two continuous
    /// predictors.
    fn contour_sections(&self) -> Vec<ReportSection> {
        let continuous: Vec<usize> =
            (0..self.predictors.len()).filter(|&j| self.predictors[j].levels.is_none()).collect();
        let [a, b, ..] = continuous[..] else { return Vec::new() };
        let (ra, rb) = (self.predictors[a].range, self.predictors[b].range);
        if !(ra.1 > ra.0 && rb.1 > rb.0) {
            return Vec::new();
        }
        let line =
            |(lo, hi): (f64, f64)| (0..GRID).map(|i| lo + (hi - lo) * i as f64 / (GRID - 1) as f64).collect::<Vec<_>>();
        let (xs, ys) = (line(ra), line(rb));
        let defaults = self.default_profile_settings();
        let mut mean = vec![vec![f64::NAN; GRID]; GRID];
        let mut se = vec![vec![f64::NAN; GRID]; GRID];
        for (iy, &v) in ys.iter().enumerate() {
            for (ix, &u) in xs.iter().enumerate() {
                let value = |j: usize| {
                    if j == a {
                        Value::Number(u)
                    } else if j == b {
                        Value::Number(v)
                    } else {
                        setting_value(&self.features[j], &defaults[j])
                    }
                };
                if let Some((m, s)) = self.predict_values(&value) {
                    mean[iy][ix] = m;
                    se[iy][ix] = s;
                }
            }
        }
        let points: Vec<RowPoint> = self
            .roles
            .rows(Role::Training)
            .into_iter()
            .filter_map(|r| match (self.features[a].value(r), self.features[b].value(r)) {
                (Value::Number(x), Value::Number(y)) => Some(RowPoint { row: r, x, y }),
                _ => None,
            })
            .collect();
        let held: Vec<String> = (0..self.predictors.len())
            .filter(|&j| j != a && j != b)
            .map(|j| {
                let p = &self.predictors[j];
                match (&p.levels, defaults[j]) {
                    (Some(l), TermValue::Level(k)) => format!("{} = {}", p.name, l.get(k).cloned().unwrap_or_default()),
                    (_, TermValue::Number(x)) => format!("{} = {}", p.name, format_number(x, 4)),
                    _ => p.name.clone(),
                }
            })
            .collect();
        let at = if held.is_empty() { String::new() } else { format!("; {}", held.join(", ")) };
        let names = (self.predictors[a].name.as_str(), self.predictors[b].name.as_str());
        vec![
            contour_section(
                SectionKind::GaussianProcessContours,
                &format!("Predicted {}{at}", self.response_name),
                names,
                (&xs, &ys),
                &mean,
                points.clone(),
            ),
            contour_section(
                SectionKind::GaussianProcessContours,
                &format!("Standard Error of the Predicted Mean{at}"),
                names,
                (&xs, &ys),
                &se,
                points,
            ),
        ]
    }

    /// Columns the report can save: predicted values, the standard error of
    /// the predicted mean, and the validation roles.
    #[must_use]
    pub fn saveable_columns(&self) -> Vec<crate::ReportColumn> {
        let mut out = prediction_columns(self, &PredictionFormulas::default());
        let Predictions::Values(predicted) = &self.predictions else { return out };
        let n = predicted.len();
        let se: Vec<f64> =
            (0..n).map(|r| self.predict_values(&|j| self.features[j].value(r)).map_or(f64::NAN, |p| p.1)).collect();
        out.insert(
            1,
            crate::ReportColumn::continuous(
                "std_error",
                "Save standard errors of the predicted mean",
                format!("StdErr Predicted {}", self.response_name),
                se,
            ),
        );
        out
    }
}

impl Predictor for GaussianProcessAnalysis {
    fn response_name(&self) -> &str {
        &self.response_name
    }

    fn response(&self) -> &Response {
        &self.response
    }

    fn roles(&self) -> &RoleAssignment {
        &self.roles
    }

    fn predictions(&self) -> &Predictions {
        &self.predictions
    }

    fn cross_validated(&self) -> Option<&Predictions> {
        Some(&self.held_out)
    }

    fn held_out_label(&self) -> &'static str {
        self.held_label()
    }
}

impl SurfaceModel for GaussianProcessAnalysis {
    fn response_name(&self) -> &str {
        &self.response_name
    }

    fn profile_factors(&self) -> Vec<ProfileFactor> {
        FeatureSet { predictors: self.predictors.clone(), features: Vec::new() }.profile_factors()
    }

    fn default_profile_settings(&self) -> Vec<TermValue> {
        self.predictors.iter().map(|p| p.default).collect()
    }

    fn predict_settings(&self, points: &[Vec<TermValue>]) -> Result<Vec<f64>> {
        Ok(self.predict_with_limits(points)?.into_iter().map(|p| p.0).collect())
    }

    fn predict_with_limits(&self, points: &[Vec<TermValue>]) -> Result<Vec<Limited>> {
        points
            .iter()
            .map(|s| {
                if s.len() != self.predictors.len() {
                    return Err(invalid("the settings do not match the predictors"));
                }
                let (m, sd) = self
                    .predict_values(&|j| setting_value(&self.features[j], &s[j]))
                    .ok_or_else(|| invalid("every predictor needs a value"))?;
                Ok((m, Some((m - Z95 * sd, m + Z95 * sd))))
            })
            .collect()
    }

    fn confidence_level(&self) -> Option<f64> {
        Some(0.95)
    }

    fn observations(&self) -> Vec<Observation> {
        let set = FeatureSet { predictors: Vec::new(), features: self.features.clone() };
        let Response::Continuous(y) = &self.response else { return Vec::new() };
        set.observations(&self.roles.rows(Role::Training), &|r| y[r])
    }

    fn profile_note(&self) -> Option<String> {
        Some(
            "Bands: 95% intervals for the mean (the process's own uncertainty; they widen away from the runs)."
                .to_owned(),
        )
    }
}

// ---- Project files and scripts ------------------------------------------------

/// The request's members after `response` and `predictors`, for project files.
#[must_use]
pub fn request_members_to_json(
    request: &GaussianProcessRequest,
    col: &dyn Fn(usize) -> JsonValue,
) -> Vec<(&'static str, JsonValue)> {
    vec![
        ("validation", request.validation.to_json(col)),
        ("kernel", JsonValue::String(request.kernel.key().to_owned())),
        ("nugget", request.nugget.map_or(JsonValue::Null, JsonValue::number)),
        ("starts", JsonValue::Number(request.starts as f64)),
        ("seed", JsonValue::Number(request.seed as f64)),
    ]
}

/// Read a kernel member (missing: Matérn 5/2).
///
/// # Errors
/// An unknown kernel.
pub(crate) fn kernel_from_json(v: &JsonValue) -> Result<CovarianceKernel> {
    match v.get("kernel").and_then(JsonValue::as_str) {
        None => Ok(CovarianceKernel::Matern52),
        Some(k) => CovarianceKernel::from_key(k).ok_or_else(|| {
            invalid(format!("Gaussian process: unknown kernel {k:?} (use \"matern52\" or \"squared-exponential\")"))
        }),
    }
}

/// Read a nugget member (missing or null: estimated).
///
/// # Errors
/// A negative nugget.
pub(crate) fn nugget_from_json(v: &JsonValue) -> Result<Option<f64>> {
    match v.get("nugget").and_then(JsonValue::as_f64) {
        None => Ok(None),
        Some(x) if x >= 0.0 && x.is_finite() => Ok(Some(x)),
        Some(_) => Err(invalid("Gaussian process: the nugget must not be negative")),
    }
}

/// Read a request from a project file's report entry (missing members take
/// their defaults).
///
/// # Errors
/// An unknown kernel or validation method, a missing column, or a negative
/// nugget.
pub fn request_from_json(
    v: &JsonValue,
    response: usize,
    predictors: Vec<usize>,
    find: &dyn Fn(&JsonValue) -> Result<usize>,
) -> Result<GaussianProcessRequest> {
    let mut r = GaussianProcessRequest::new(response, predictors);
    r.validation = ValidationMethod::from_json(v.get("validation"), find)?;
    r.kernel = kernel_from_json(v)?;
    r.nugget = nugget_from_json(v)?;
    if let Some(s) = v.get("starts").and_then(JsonValue::as_usize) {
        r.starts = s.clamp(1, 100);
    }
    r.seed = v.get("seed").and_then(crate::seed::seed_from_json).unwrap_or(r.seed);
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    /// 30 rows: y = sin(3a) + b² + 2·[g = "Q"] + noise, with a role column.
    fn table() -> DataTable {
        let mut rng = stat_core::rng::Rng::new(4);
        let (mut a, mut b, mut g, mut y, mut role) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for i in 0..30 {
            let (u, v) = (rng.next_f64() * 2.0, rng.next_f64() * 10.0);
            let q = i % 3 == 0;
            a.push(u);
            b.push(v);
            g.push(if q { "Q" } else { "P" }.to_owned());
            y.push((3.0 * u).sin() + 0.02 * v * v + if q { 2.0 } else { 0.0 } + 0.01 * rng.normal());
            role.push(if i % 5 == 4 { "Validation" } else { "Training" }.to_owned());
        }
        DataTable::new(
            "gp",
            vec![
                Column::numeric("a", a),
                Column::numeric("b", b),
                Column::text("g", g),
                Column::numeric("y", y),
                Column::text("role", role),
            ],
        )
        .unwrap()
    }

    #[test]
    fn fits_predicts_and_profiles() {
        let t = table();
        let rows = RowStateManager::new(t.n_rows());
        let req = GaussianProcessRequest::new(3, vec![0, 1, 2]);
        let a = GaussianProcessAnalysis::run(&t, &rows, &req).unwrap();
        assert_eq!(a.title(), "Gaussian Process (Matérn 5/2): y ~ a + b + g");
        let s = a.sections();
        let kinds: Vec<SectionKind> = s.iter().map(|x| x.kind).collect();
        assert_eq!(
            kinds,
            [
                SectionKind::GaussianProcessSummary,
                SectionKind::GaussianProcessLengthScales,
                SectionKind::FitByRole,
                SectionKind::GaussianProcessActualByPredicted,
                SectionKind::GaussianProcessContours,
                SectionKind::GaussianProcessContours,
            ]
        );
        let SectionContent::Table(ls) = &s[1].content else { panic!() };
        assert_eq!(ls.rows.len(), 4); // a, b, g = P, g = Q
        let SectionContent::Table(fit) = &s[2].content else { panic!() };
        assert_eq!(fit.rows[1][0], "Leave-one-out");
        let r2 = fit.rows[1][4].parse::<f64>().unwrap();
        assert!(r2 > 0.95, "leave-one-out R² {r2}");
        // Profiler: intervals contain the mean and are narrow at a run.
        let mut at_run = a.default_profile_settings();
        at_run[0] = TermValue::Number(t.column(0).unwrap().as_numeric().unwrap()[0]);
        at_run[1] = TermValue::Number(t.column(1).unwrap().as_numeric().unwrap()[0]);
        at_run[2] = TermValue::Level(1);
        let p = a.predict_with_limits(&[at_run]).unwrap();
        let (m, Some((lo, hi))) = p[0] else { panic!() };
        assert!(lo < m && m < hi && hi - lo < 0.2, "{p:?}");
        let y0 = t.column(3).unwrap().as_numeric().unwrap()[0];
        assert!((m - y0).abs() < 0.05, "{m} vs {y0}");
        let keys: Vec<String> = a.saveable_columns().into_iter().map(|c| c.key).collect();
        assert_eq!(keys, ["predicted", "std_error", "validation"]);
        // Same seed, same report.
        let b = GaussianProcessAnalysis::run(&t, &rows, &req).unwrap();
        assert_eq!(a.sections(), b.sections());
    }

    #[test]
    fn validation_rows_folds_and_json() {
        let t = table();
        let rows = RowStateManager::new(t.n_rows());
        let req = GaussianProcessRequest {
            validation: ValidationMethod::Column(4),
            kernel: CovarianceKernel::SquaredExponential,
            ..GaussianProcessRequest::new(3, vec![0, 1, 2])
        };
        let a = GaussianProcessAnalysis::run(&t, &rows, &req).unwrap();
        let SectionContent::Table(fit) = &a.sections()[2].content else { panic!() };
        let labels: Vec<&str> = fit.rows.iter().map(|r| r[0].as_str()).collect();
        assert_eq!(labels, ["Training", "Validation", "Leave-one-out"]);
        let r2 = fit.rows[1][4].parse::<f64>().unwrap();
        assert!(r2 > 0.9, "validation R² {r2}");
        let folds = GaussianProcessRequest {
            validation: ValidationMethod::KFold { folds: 3, seed: 2 },
            nugget: Some(1e-6),
            starts: 3,
            ..GaussianProcessRequest::new(3, vec![0, 1])
        };
        let b = GaussianProcessAnalysis::run(&t, &rows, &folds).unwrap();
        let SectionContent::Table(fit) = &b.sections()[2].content else { panic!() };
        assert_eq!(fit.rows.last().unwrap()[0], "Cross-validation");
        // JSON round trip.
        let col = |i: usize| JsonValue::String(t.column(i).unwrap().name.clone());
        let v = JsonValue::Object(
            request_members_to_json(&folds, &col).into_iter().map(|(k, v)| (k.to_owned(), v)).collect(),
        );
        let find = |c: &JsonValue| Ok(t.find(c.as_str().unwrap()).unwrap());
        assert_eq!(request_from_json(&v, folds.response, folds.predictors.clone(), &find).unwrap(), folds);
        let bad = JsonValue::Object(vec![("kernel".to_owned(), JsonValue::String("cubic".to_owned()))]);
        assert!(request_from_json(&bad, 3, vec![0], &find).is_err());
        // A categorical response is refused.
        assert!(GaussianProcessAnalysis::run(&t, &rows, &GaussianProcessRequest::new(2, vec![0])).is_err());
    }
}
