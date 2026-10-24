//! Bayesian models bound to a dataset: linear models and ANOVA with
//! g-priors ([`stat_bayes::linear`]), and generalized linear and
//! hierarchical models sampled by the No-U-Turn sampler
//! ([`stat_bayes::glm`]). Both are [`crate::BayesRequest`] variants.
//!
//! * **Linear model** (`"bayesian-linear-model"`): effects are coded as in
//!   the linear model, except that categorical factors use orthonormal
//!   contrasts (each level effect then has the same prior) and the effect
//!   list is closed under heredity. Every model that keeps heredity gets a
//!   Bayes factor against the intercept-only model and a posterior
//!   probability; effects get inclusion probabilities and model-averaged
//!   estimates.
//! * **Bayesian GLM** (`"bayesian-model"`): fixed effects coded as in the
//!   linear model and group (random) effects built exactly as the mixed
//!   model builds them ([`crate::mixed_analysis`]), with weakly informative
//!   priors. The report shows posterior summaries with convergence
//!   diagnostics, sampler diagnostics, PSIS-LOO and WAIC, trace, rank,
//!   autocorrelation and density plots of one chosen parameter, and a
//!   posterior predictive check.

use crate::bayes_analysis::{
    DIGITS, col_json, column_index, column_list, column_name, format_bf, number, opt_col_json, optional_column, pct,
    read_level, section,
};
use crate::format::format_number;
use crate::mixed_analysis::{MixedRequest, RandomEffectSpec};
use crate::model_data::{ModelData, PredictorData};
use crate::project::{effects_json, invalid, linear_effects, member, obj, s};
use crate::report::{
    Annotations, PlotPane, PlotSpec, PointGroups, ReportSection, SectionContent, SectionKind, TextTable,
};
use stat_bayes::bayes_factor::evidence_label;
use stat_bayes::glm::{
    BayesGlmFit, BayesGlmModel, CoefficientPrior, GlmFamily, GroupTerm, ParameterKind, ParameterSummary, Priors,
    ScalePrior, fit_bayes_glm,
};
use stat_bayes::linear::{
    ContinuousPrior, Effect, EffectKind, LinearSpec, ModelPrior, bayesian_linear_model, orthonormal_contrasts,
};
use stat_bayes::nuts::NutsSettings;
use stat_core::Result;
use stat_data::{DataTable, JsonValue, RowStateManager};
use stat_linear::{effect_name, hierarchical_closure, main_effects, natural_order, parent_indices};
use stat_mixed::MixedFamily;

/// Default scale of the Zellner–Siow prior on continuous effects (`√2⁄4`, "medium").
pub const DEFAULT_CONTINUOUS_SCALE: f64 = std::f64::consts::SQRT_2 / 4.0;
/// Default scale of categorical effects (½, "medium").
pub const DEFAULT_CATEGORICAL_SCALE: f64 = 0.5;

// ---------------------------------------------------------------------------
// Linear model.

/// Settings of a Bayesian linear model or ANOVA.
#[derive(Debug, Clone, PartialEq)]
pub struct BayesLinearRequest {
    /// Response column (numeric).
    pub response: usize,
    /// Predictor columns.
    pub predictors: Vec<usize>,
    /// Effects as `(position in predictors, power)` products; empty means
    /// main effects. Missing lower-order effects are added (heredity).
    pub effects: Vec<Vec<(usize, u8)>>,
    /// Prior of continuous effects.
    pub continuous_prior: ContinuousPrior,
    /// Prior scale of categorical effects.
    pub r_categorical: f64,
    /// Prior over models.
    pub model_prior: ModelPrior,
    /// Fit only the full model (no model comparison).
    pub full_model_only: bool,
    /// Credible level.
    pub level: f64,
    /// Importance draws per model with several categorical effects.
    pub draws: usize,
    /// Seed of the importance sampler and model search.
    pub seed: u64,
}

impl BayesLinearRequest {
    /// Default settings: main effects, "medium" prior scales, uniform model prior.
    #[must_use]
    pub fn new(response: usize, predictors: Vec<usize>) -> Self {
        Self {
            response,
            predictors,
            effects: Vec::new(),
            continuous_prior: ContinuousPrior::ZellnerSiow { r: DEFAULT_CONTINUOUS_SCALE },
            r_categorical: DEFAULT_CATEGORICAL_SCALE,
            model_prior: ModelPrior::Uniform,
            full_model_only: false,
            level: 0.95,
            draws: 10_000,
            seed: 1,
        }
    }

    /// Every column the request uses.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        std::iter::once(self.response).chain(self.predictors.iter().copied()).collect()
    }

    /// The request with its columns renumbered by `f`.
    #[must_use]
    pub fn map_columns(&self, f: &dyn Fn(usize) -> usize) -> Self {
        Self { response: f(self.response), predictors: self.predictors.iter().map(|&p| f(p)).collect(), ..self.clone() }
    }

    pub(crate) fn to_json(&self, table: &DataTable) -> JsonValue {
        let (prior, value) = match self.continuous_prior {
            ContinuousPrior::ZellnerSiow { r } => ("zellner-siow", r),
            ContinuousPrior::HyperG { a } => ("hyper-g", a),
        };
        obj([
            ("analysis", s("bayesian-linear-model")),
            ("response", col_json(table, self.response)),
            ("predictors", JsonValue::Array(self.predictors.iter().map(|&p| col_json(table, p)).collect())),
            ("effects", effects_json(&self.effects)),
            ("continuous_prior", s(prior)),
            ("continuous_scale", JsonValue::number(value)),
            ("categorical_scale", JsonValue::number(self.r_categorical)),
            ("model_prior", s(if self.model_prior == ModelPrior::Uniform { "uniform" } else { "beta-binomial" })),
            ("full_model_only", JsonValue::Bool(self.full_model_only)),
            ("level", JsonValue::number(self.level)),
            ("draws", JsonValue::number(self.draws as f64)),
            ("seed", JsonValue::number(self.seed as f64)),
        ])
    }

    pub(crate) fn from_json(v: &JsonValue, table: &DataTable) -> Result<Self> {
        let value = number(v, "continuous_scale", f64::NAN);
        let continuous_prior = match v.get("continuous_prior").and_then(JsonValue::as_str).unwrap_or("zellner-siow") {
            "zellner-siow" => {
                ContinuousPrior::ZellnerSiow { r: if value.is_nan() { DEFAULT_CONTINUOUS_SCALE } else { value } }
            }
            "hyper-g" => ContinuousPrior::HyperG { a: if value.is_nan() { 3.0 } else { value } },
            other => return Err(invalid(format!("bayesian-linear-model: unknown prior {other:?}"))),
        };
        let model_prior = match v.get("model_prior").and_then(JsonValue::as_str).unwrap_or("uniform") {
            "uniform" => ModelPrior::Uniform,
            "beta-binomial" => ModelPrior::BetaBinomial,
            other => return Err(invalid(format!("bayesian-linear-model: unknown model prior {other:?}"))),
        };
        Ok(Self {
            response: column_index(v, "response", table)?,
            predictors: column_list(v, "predictors", table)?,
            effects: linear_effects(v)?,
            continuous_prior,
            r_categorical: number(v, "categorical_scale", DEFAULT_CATEGORICAL_SCALE),
            model_prior,
            full_model_only: v.get("full_model_only").and_then(JsonValue::as_bool).unwrap_or(false),
            level: read_level(v, "bayesian-linear-model")?,
            draws: v.get("draws").and_then(JsonValue::as_usize).filter(|d| *d >= 100).unwrap_or(10_000),
            seed: v.get("seed").and_then(JsonValue::as_usize).unwrap_or(1) as u64,
        })
    }
}

fn power_suffix(p: u8) -> String {
    match p {
        1 => String::new(),
        2 => "²".to_owned(),
        3 => "³".to_owned(),
        p => format!("^{p}"),
    }
}

/// Code one effect over `rows`: products of continuous powers and
/// orthonormal contrasts of categorical factors, with one reported
/// parameter per level combination (or one slope).
fn code_effect(data: &ModelData, effect: &[(usize, u8)], rows: &[usize]) -> Result<Effect> {
    let names: Vec<&str> = data.predictor_names.iter().map(String::as_str).collect();
    let n = rows.len();
    let mut columns: Vec<Vec<f64>> = vec![vec![1.0; n]];
    // Each parameter: label pieces and coefficients over the columns.
    let mut parameters: Vec<(Vec<String>, Vec<f64>)> = vec![(Vec::new(), vec![1.0])];
    let mut categorical = false;
    for &(pos, power) in effect {
        let name = names.get(pos).copied().ok_or_else(|| invalid("an effect refers to a missing predictor"))?;
        match &data.predictors[pos] {
            PredictorData::Continuous(v) => {
                for c in &mut columns {
                    for (k, &r) in rows.iter().enumerate() {
                        c[k] *= v[r].powi(i32::from(power));
                    }
                }
                for p in &mut parameters {
                    p.0.push(format!("{name}{}", power_suffix(power)));
                }
            }
            PredictorData::Categorical(labels) => {
                if power > 1 {
                    return Err(invalid(format!("{name} is categorical, so it cannot be raised to a power")));
                }
                categorical = true;
                let mut levels: Vec<String> = rows.iter().map(|&r| labels[r].clone()).collect();
                levels.sort_by(|a, b| natural_order(a, b));
                levels.dedup();
                if levels.len() < 2 {
                    return Err(invalid(format!("{name} has only one level in the rows used")));
                }
                let q = orthonormal_contrasts(levels.len());
                let codes: Vec<usize> =
                    rows.iter().map(|&r| levels.iter().position(|l| *l == labels[r]).unwrap_or(0)).collect();
                let m = levels.len() - 1;
                let mut next = Vec::with_capacity(columns.len() * m);
                for c in &columns {
                    for j in 0..m {
                        next.push(c.iter().zip(&codes).map(|(v, &g)| v * q[g][j]).collect());
                    }
                }
                columns = next;
                let mut np = Vec::with_capacity(parameters.len() * levels.len());
                for (label, coef) in &parameters {
                    for (l, level) in levels.iter().enumerate() {
                        let mut lab = label.clone();
                        lab.push(format!("{name}[{level}]"));
                        let c: Vec<f64> = coef.iter().flat_map(|a| q[l].iter().map(move |b| a * b)).collect();
                        np.push((lab, c));
                    }
                }
                parameters = np;
            }
        }
    }
    Ok(Effect {
        name: effect_name(&names, effect),
        kind: if categorical { EffectKind::Categorical } else { EffectKind::Continuous },
        columns,
        parents: Vec::new(),
        parameters: parameters.into_iter().map(|(l, c)| (l.join(" × "), c)).collect(),
    })
}

pub(crate) fn run_linear(
    table: &DataTable,
    states: &RowStateManager,
    r: &BayesLinearRequest,
) -> Result<(String, Vec<ReportSection>)> {
    let data = ModelData::gather(table, states, r.response, &r.predictors, false)?;
    let rows = data.rows_used.clone();
    let y: Vec<f64> = rows.iter().map(|&i| data.y_all[i]).collect();
    let requested = if r.effects.is_empty() { main_effects(r.predictors.len()) } else { r.effects.clone() };
    let closed = hierarchical_closure(&requested);
    let added = closed.len() - requested.len();
    let parents = parent_indices(&closed);
    let mut effects = Vec::with_capacity(closed.len());
    for (e, p) in closed.iter().zip(parents) {
        let mut coded = code_effect(&data, e, &rows)?;
        coded.parents = p;
        effects.push(coded);
    }
    let spec = LinearSpec {
        y,
        effects,
        continuous_prior: r.continuous_prior,
        r_categorical: r.r_categorical,
        model_prior: r.model_prior,
        full_model_only: r.full_model_only,
        level: r.level,
        draws: r.draws,
        seed: r.seed,
    };
    let fit = bayesian_linear_model(&spec)?;
    let names: Vec<String> = spec.effects.iter().map(|e| e.name.clone()).collect();
    let anova = spec.effects.iter().all(|e| e.kind == EffectKind::Categorical);
    let model_name = |m: &[usize]| -> String {
        if m.is_empty() {
            "Intercept only".to_owned()
        } else {
            m.iter().map(|&e| names[e].as_str()).collect::<Vec<_>>().join(" + ")
        }
    };
    let mut out = Vec::new();
    let best_ln = fit.models.first().map_or(0.0, |m| m.ln_bf);
    let mut models =
        TextTable::new(["Model", "P(model)", "P(model | data)", "BF₁₀", "BF against best", "R²", "Error %"]);
    for m in fit.models.iter().take(20) {
        models.push([
            model_name(&m.effects),
            format_number(m.prior, 4),
            format_number(m.posterior, 4),
            format_bf(m.ln_bf.exp()),
            format_bf((m.ln_bf - best_ln).exp()),
            format_number(m.r_squared, 4),
            if m.error > 0.0 { format!("{:.2}", 100.0 * m.error) } else { String::new() },
        ]);
    }
    let search = if r.full_model_only {
        "full model against the intercept-only model".to_owned()
    } else if fit.enumerated {
        format!("all {} models that keep heredity", fit.models.len())
    } else {
        format!("{} models visited by model composition sampling (MC³)", fit.visited)
    };
    let shown = if fit.models.len() > 20 { "; the 20 most probable shown" } else { "" };
    out.push(section(
        SectionKind::BayesModels,
        format!("Models ({search}{shown}; BF₁₀ against the intercept-only model)"),
        SectionContent::Table(models),
    ));
    if !r.full_model_only {
        let mut inc = TextTable::new(["Effect", "P(included)", "P(included | data)", "Inclusion BF", "Evidence"]);
        for (e, i) in fit.inclusion.iter().enumerate() {
            inc.push([
                names[e].clone(),
                format_number(i.prior, 4),
                format_number(i.posterior, 4),
                // A posterior probability that rounds to 1 makes the factor
                // only a bound (the engine holds it at 1 − ε).
                if i.bf > 1e12 { "> 1e12".to_owned() } else { format_bf(i.bf) },
                evidence_label(i.bf).to_owned(),
            ]);
        }
        out.push(section(
            SectionKind::BayesInclusion,
            "Effects (inclusion probability over all models; an interaction counts only with its lower-order effects)",
            SectionContent::Table(inc),
        ));
        out.push(inclusion_plot(&names, &fit.inclusion.iter().map(|i| i.posterior).collect::<Vec<_>>()));
    }
    let p = pct(r.level);
    let estimates = |list: &[stat_bayes::linear::Estimate], averaged: bool| -> TextTable {
        let mut columns = vec![
            "Parameter".to_owned(),
            "Mean".to_owned(),
            "SD".to_owned(),
            format!("Lower {p}"),
            format!("Upper {p}"),
        ];
        if averaged {
            columns.push("P(zero)".to_owned());
        }
        let mut t = TextTable::new(columns);
        for e in list {
            // An interval end at the point mass of models without the
            // effect is zero up to rounding.
            let snap = |v: f64| if v.abs() < 1e-9 * (e.mean.abs() + e.sd) { 0.0 } else { v };
            let mut row = vec![
                e.label.clone(),
                format_number(e.mean, DIGITS),
                format_number(e.sd, DIGITS),
                format_number(snap(e.interval.0), DIGITS),
                format_number(snap(e.interval.1), DIGITS),
            ];
            if averaged {
                row.push(format_number(e.p_zero, 4));
            }
            t.push(row);
        }
        t
    };
    if !r.full_model_only {
        out.push(section(
            SectionKind::BayesEstimates,
            "Model-Averaged Estimates (level effects and slopes; P(zero): probability the effect is left out)",
            SectionContent::Table(estimates(&fit.averaged, true)),
        ));
    }
    let best = fit.models.first().map_or_else(String::new, |m| model_name(&m.effects));
    out.push(section(
        SectionKind::BayesBestEstimates,
        format!(
            "Estimates in {} ({best})",
            if r.full_model_only { "the Full Model" } else { "the Most Probable Model" }
        ),
        SectionContent::Table(estimates(&fit.best, false)),
    ));
    let continuous = match r.continuous_prior {
        ContinuousPrior::ZellnerSiow { r } => {
            format!("Zellner–Siow (multivariate Cauchy) with scale r = {}", format_number(r, 4))
        }
        ContinuousPrior::HyperG { a } => format!("hyper-g with a = {}", format_number(a, 4)),
    };
    let model_prior = match r.model_prior {
        ModelPrior::Uniform => "every model equally likely",
        ModelPrior::BetaBinomial => "beta-binomial(1, 1) on the number of effects",
    };
    let heredity = if added > 0 {
        format!(" {added} lower-order effect(s) were added so every interaction or power has its parents.")
    } else {
        String::new()
    };
    out.push(section(
        SectionKind::BayesPrior,
        "Prior",
        SectionContent::Note(format!(
            "Continuous effects: {continuous}. Categorical effects (orthonormal contrasts, one g each): Cauchy scale \
             r = {}. Flat prior on the intercept and Jeffreys on σ². Models: {model_prior}. {} rows used.{heredity}",
            format_number(r.r_categorical, 4),
            fit.n
        )),
    ));
    let what = if anova { "Bayesian ANOVA" } else { "Bayesian Linear Model" };
    Ok((format!("{what}: {}", data.response_name), out))
}

/// Posterior inclusion probability of each effect, as a dot on a line
/// from 0 (effects top to bottom).
fn inclusion_plot(names: &[String], p: &[f64]) -> ReportSection {
    let k = names.len();
    let y = |e: usize| (k - e) as f64;
    let spec = PlotSpec {
        x_label: "Posterior inclusion probability".to_owned(),
        y_label: "Effect".to_owned(),
        segments: p.iter().enumerate().map(|(e, &v)| vec![[0.0, y(e)], [v, y(e)]]).collect(),
        annotations: Some(Annotations {
            groups: vec!["P(included | data)".to_owned()],
            items: p.iter().enumerate().map(|(e, &v)| (v, y(e), format_number(v, 3), 0)).collect(),
            labels: Vec::new(),
        }),
        y_ticks: Some(names.iter().enumerate().map(|(e, n)| (y(e), n.clone())).collect()),
        line: Some(vec![[0.5, 0.5], [0.5, k as f64 + 0.5]]),
        line_name: Some("Even odds (0.5)".to_owned()),
        ..PlotSpec::default()
    };
    section(SectionKind::BayesInclusionPlot, "Inclusion Probabilities", SectionContent::Plot(Box::new(spec)))
}

// ---------------------------------------------------------------------------
// Generalized linear and hierarchical models.

/// Project-file key of a family.
#[must_use]
pub fn family_key(f: GlmFamily) -> &'static str {
    match f {
        GlmFamily::Normal => "normal",
        GlmFamily::Binomial => "binomial",
        GlmFamily::Poisson => "poisson",
        GlmFamily::NegativeBinomial => "negative-binomial",
    }
}

/// Every family, in menu order.
pub const FAMILIES: [GlmFamily; 4] =
    [GlmFamily::Normal, GlmFamily::Binomial, GlmFamily::Poisson, GlmFamily::NegativeBinomial];

/// Plain name of a family with its link.
#[must_use]
pub fn family_label(f: GlmFamily) -> &'static str {
    match f {
        GlmFamily::Normal => "Normal (identity link)",
        GlmFamily::Binomial => "Binomial (logit link)",
        GlmFamily::Poisson => "Poisson (log link)",
        GlmFamily::NegativeBinomial => "Negative binomial (log link)",
    }
}

fn family_from_key(key: &str) -> Option<GlmFamily> {
    FAMILIES.into_iter().find(|f| family_key(*f) == key)
}

/// Project-file key of a scale prior.
#[must_use]
pub fn scale_prior_key(p: ScalePrior) -> &'static str {
    match p {
        ScalePrior::Exponential => "exponential",
        ScalePrior::HalfNormal => "half-normal",
        ScalePrior::HalfCauchy => "half-cauchy",
        ScalePrior::HalfT3 => "half-t3",
    }
}

/// Every scale prior, in menu order.
pub const SCALE_PRIORS: [ScalePrior; 4] =
    [ScalePrior::Exponential, ScalePrior::HalfNormal, ScalePrior::HalfT3, ScalePrior::HalfCauchy];

/// Plain name of a scale prior.
#[must_use]
pub fn scale_prior_label(p: ScalePrior) -> &'static str {
    match p {
        ScalePrior::Exponential => "Exponential",
        ScalePrior::HalfNormal => "Half-normal",
        ScalePrior::HalfCauchy => "Half-Cauchy",
        ScalePrior::HalfT3 => "Half-t (3 DF)",
    }
}

/// Settings of a Bayesian generalized linear or hierarchical model.
#[derive(Debug, Clone, PartialEq)]
pub struct BayesModelRequest {
    /// Response column (successes when `trials` is given; a two-valued
    /// column for a binomial response without trials).
    pub response: usize,
    /// Fixed-effect predictors (may be empty).
    pub predictors: Vec<usize>,
    /// Fixed effects as `(position, power)` products; empty means main effects.
    pub effects: Vec<Vec<(usize, u8)>>,
    /// Group (random) effects: grouping column, nesting, intercept and
    /// slopes (sampled independently; `correlated` is ignored).
    pub random: Vec<RandomEffectSpec>,
    /// Response distribution.
    pub family: GlmFamily,
    /// Binomial: numbers of trials.
    pub trials: Option<usize>,
    /// Offset on the link scale (e.g. log exposure).
    pub offset: Option<usize>,
    /// Normal: known standard deviation of each row (meta-analysis); no
    /// residual SD is estimated.
    pub known_sd: Option<usize>,
    /// Priors.
    pub priors: Priors,
    /// Chains (run in parallel).
    pub chains: usize,
    /// Warm-up iterations per chain.
    pub warmup: usize,
    /// Kept draws per chain.
    pub draws: usize,
    /// Seed (chain `k` uses a stream derived from it).
    pub seed: u64,
    /// Target acceptance rate of the step-size adaptation.
    pub target_accept: f64,
    /// Credible level.
    pub level: f64,
    /// Parameter shown in the trace, rank, autocorrelation and density plots
    /// (`None`: the first).
    pub plot_parameter: Option<String>,
}

impl BayesModelRequest {
    /// Default settings: weakly informative autoscaled priors, 4 chains of
    /// 1000 warm-up and 1000 kept draws.
    #[must_use]
    pub fn new(response: usize, predictors: Vec<usize>, family: GlmFamily) -> Self {
        let n = NutsSettings::default();
        Self {
            response,
            predictors,
            effects: Vec::new(),
            random: Vec::new(),
            family,
            trials: None,
            offset: None,
            known_sd: None,
            priors: Priors::default(),
            chains: n.chains,
            warmup: n.warmup,
            draws: n.draws,
            seed: n.seed,
            target_accept: n.target_accept,
            level: 0.95,
            plot_parameter: None,
        }
    }

    /// Every column the request uses.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        let mut c = vec![self.response];
        c.extend(&self.predictors);
        for r in &self.random {
            c.extend(r.columns());
        }
        c.extend(self.trials);
        c.extend(self.offset);
        c.extend(self.known_sd);
        c
    }

    /// The request with its columns renumbered by `f`.
    #[must_use]
    pub fn map_columns(&self, f: &dyn Fn(usize) -> usize) -> Self {
        let list = |v: &[usize]| v.iter().map(|&i| f(i)).collect::<Vec<_>>();
        Self {
            response: f(self.response),
            predictors: list(&self.predictors),
            random: self
                .random
                .iter()
                .map(|r| RandomEffectSpec {
                    group: f(r.group),
                    within: list(&r.within),
                    slopes: list(&r.slopes),
                    ..r.clone()
                })
                .collect(),
            trials: self.trials.map(f),
            offset: self.offset.map(f),
            known_sd: self.known_sd.map(f),
            ..self.clone()
        }
    }

    pub(crate) fn to_json(&self, table: &DataTable) -> JsonValue {
        let cols = |v: &[usize]| JsonValue::Array(v.iter().map(|&i| col_json(table, i)).collect());
        let (coef, df) = match self.priors.coefficient {
            CoefficientPrior::Normal => ("normal", JsonValue::Null),
            CoefficientPrior::StudentT(df) => ("student-t", JsonValue::number(df)),
            CoefficientPrior::Cauchy => ("cauchy", JsonValue::Null),
        };
        let p = &self.priors;
        obj([
            ("analysis", s("bayesian-model")),
            ("response", col_json(table, self.response)),
            ("predictors", cols(&self.predictors)),
            ("effects", effects_json(&self.effects)),
            (
                "random",
                JsonValue::Array(
                    self.random
                        .iter()
                        .map(|r| {
                            obj([
                                ("group", col_json(table, r.group)),
                                ("within", cols(&r.within)),
                                ("intercept", JsonValue::Bool(r.intercept)),
                                ("slopes", cols(&r.slopes)),
                            ])
                        })
                        .collect(),
                ),
            ),
            ("family", s(family_key(self.family))),
            ("trials", opt_col_json(table, self.trials)),
            ("offset", opt_col_json(table, self.offset)),
            ("known_sd", opt_col_json(table, self.known_sd)),
            ("coefficient_prior", s(coef)),
            ("coefficient_df", df),
            ("coefficient_scale", JsonValue::number(p.coefficient_scale)),
            ("intercept_location", p.intercept_location.map_or(JsonValue::Null, JsonValue::number)),
            ("intercept_scale", JsonValue::number(p.intercept_scale)),
            ("sd_prior", s(scale_prior_key(p.sd))),
            ("sd_scale", JsonValue::number(p.sd_scale)),
            ("autoscale", JsonValue::Bool(p.autoscale)),
            ("chains", JsonValue::number(self.chains as f64)),
            ("warmup", JsonValue::number(self.warmup as f64)),
            ("draws", JsonValue::number(self.draws as f64)),
            ("seed", JsonValue::number(self.seed as f64)),
            ("target_accept", JsonValue::number(self.target_accept)),
            ("level", JsonValue::number(self.level)),
            ("plot_parameter", self.plot_parameter.as_deref().map_or(JsonValue::Null, s)),
        ])
    }

    pub(crate) fn from_json(v: &JsonValue, table: &DataTable) -> Result<Self> {
        let name = member(v, "family")?.as_str().unwrap_or_default();
        let family =
            family_from_key(name).ok_or_else(|| invalid(format!("bayesian-model: unknown family {name:?}")))?;
        let random = v
            .get("random")
            .and_then(JsonValue::as_array)
            .unwrap_or_default()
            .iter()
            .map(|r| {
                Ok(RandomEffectSpec {
                    group: column_index(r, "group", table)?,
                    within: column_list(r, "within", table)?,
                    intercept: r.get("intercept").and_then(JsonValue::as_bool).unwrap_or(true),
                    slopes: column_list(r, "slopes", table)?,
                    correlated: false,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let d = Priors::default();
        let coefficient = match v.get("coefficient_prior").and_then(JsonValue::as_str).unwrap_or("normal") {
            "normal" => CoefficientPrior::Normal,
            "student-t" => CoefficientPrior::StudentT(number(v, "coefficient_df", 3.0)),
            "cauchy" => CoefficientPrior::Cauchy,
            other => return Err(invalid(format!("bayesian-model: unknown coefficient prior {other:?}"))),
        };
        let sd_name = v.get("sd_prior").and_then(JsonValue::as_str).unwrap_or("exponential");
        let sd = SCALE_PRIORS
            .into_iter()
            .find(|p| scale_prior_key(*p) == sd_name)
            .ok_or_else(|| invalid(format!("bayesian-model: unknown SD prior {sd_name:?}")))?;
        let n = NutsSettings::default();
        let count = |key: &str, default: usize, min: usize| {
            v.get(key).and_then(JsonValue::as_usize).filter(|c| *c >= min).unwrap_or(default)
        };
        Ok(Self {
            response: column_index(v, "response", table)?,
            predictors: column_list(v, "predictors", table)?,
            effects: linear_effects(v)?,
            random,
            family,
            trials: optional_column(v, "trials", table)?,
            offset: optional_column(v, "offset", table)?,
            known_sd: optional_column(v, "known_sd", table)?,
            priors: Priors {
                coefficient,
                coefficient_scale: number(v, "coefficient_scale", d.coefficient_scale),
                intercept_location: v.get("intercept_location").and_then(JsonValue::as_f64),
                intercept_scale: number(v, "intercept_scale", d.intercept_scale),
                sd,
                sd_scale: number(v, "sd_scale", d.sd_scale),
                autoscale: v.get("autoscale").and_then(JsonValue::as_bool).unwrap_or(true),
            },
            chains: count("chains", n.chains, 1).min(16),
            warmup: count("warmup", n.warmup, 20),
            draws: count("draws", n.draws, 20),
            seed: v.get("seed").and_then(JsonValue::as_usize).unwrap_or(1) as u64,
            target_accept: number(v, "target_accept", n.target_accept).clamp(0.5, 0.999),
            level: read_level(v, "bayesian-model")?,
            plot_parameter: v.get("plot_parameter").and_then(JsonValue::as_str).map(str::to_owned),
        })
    }
}

fn mixed_family(f: GlmFamily) -> MixedFamily {
    match f {
        GlmFamily::Normal => MixedFamily::Normal,
        GlmFamily::Binomial => MixedFamily::Binomial,
        GlmFamily::Poisson => MixedFamily::Poisson,
        GlmFamily::NegativeBinomial => MixedFamily::NegativeBinomial,
    }
}

fn summary_table(params: &[&ParameterSummary], level: f64) -> TextTable {
    let p = pct(level);
    let mut t = TextTable::new([
        "Parameter".to_owned(),
        "Mean".to_owned(),
        "SD".to_owned(),
        "Median".to_owned(),
        format!("Lower {p}"),
        format!("Upper {p}"),
        format!("HPD lower {p}"),
        format!("HPD upper {p}"),
        "R̂".to_owned(),
        "Bulk ESS".to_owned(),
        "Tail ESS".to_owned(),
        "MCSE".to_owned(),
    ]);
    for q in params {
        let d = &q.diagnostics;
        t.push([
            q.name.clone(),
            format_number(q.mean, DIGITS),
            format_number(q.sd, DIGITS),
            format_number(q.median, DIGITS),
            format_number(q.interval.0, DIGITS),
            format_number(q.interval.1, DIGITS),
            format_number(q.hpd.0, DIGITS),
            format_number(q.hpd.1, DIGITS),
            format!("{:.3}", d.rhat),
            format!("{:.0}", d.ess_bulk),
            format!("{:.0}", d.ess_tail),
            format_number(d.mcse_mean, 3),
        ]);
    }
    t
}

/// The response's name and the model's title.
fn model_title(family: GlmFamily, hierarchical: bool, response: &str) -> String {
    let f = match family {
        GlmFamily::Normal => "Normal",
        GlmFamily::Binomial => "Logistic",
        GlmFamily::Poisson => "Poisson",
        GlmFamily::NegativeBinomial => "Negative Binomial",
    };
    let h = if hierarchical { "Hierarchical " } else { "" };
    format!("Bayesian {h}{f} Model: {response}")
}

pub(crate) fn run_model(
    table: &DataTable,
    states: &RowStateManager,
    r: &BayesModelRequest,
) -> Result<(String, Vec<ReportSection>, Vec<String>)> {
    if r.known_sd.is_some() && r.family != GlmFamily::Normal {
        return Err(invalid("a column of known standard deviations applies to the normal family only"));
    }
    if r.random.iter().any(|g| g.slopes.iter().any(|s| r.random.iter().any(|h| h.group == *s))) {
        return Err(invalid("a grouping column cannot also be a slope"));
    }
    let mixed = MixedRequest {
        response: r.response,
        predictors: r.predictors.clone(),
        effects: r.effects.clone(),
        random: r.random.clone(),
        family: mixed_family(r.family),
        trials: r.trials,
        offset: r.offset,
        reml: true,
        quadrature_points: 1,
        slices: None,
        slopes: None,
        frequency: None,
        weight: None,
        robust: false,
        residual_groups: None,
        df_method: stat_mixed::DfMethod::Satterthwaite,
        order: Vec::new(),
    };
    let built = crate::mixed_analysis::build_model(table, states, &mixed)?;
    let rows = built.rows.clone();
    let m = &built.model;
    let known_sd = match r.known_sd {
        Some(c) => {
            let col = table.column(c).ok_or_else(|| invalid("no such column"))?;
            let v = col
                .as_numeric()
                .ok_or_else(|| invalid(format!("the known SD column {:?} must be numeric", col.name)))?;
            let sd: Vec<f64> = rows.iter().map(|&i| v[i]).collect();
            if sd.iter().any(|s| !(s.is_finite() && *s > 0.0)) {
                return Err(invalid(format!("every row used needs a positive standard deviation in {:?}", col.name)));
            }
            Some(sd)
        }
        None => None,
    };
    let x: Vec<Vec<f64>> = (1..m.x.ncols()).map(|j| m.x.col(j).to_vec()).collect();
    let groups: Vec<GroupTerm> = m
        .random
        .iter()
        .map(|t| GroupTerm {
            name: t.group.clone(),
            levels: t.levels.clone(),
            codes: t.codes.clone(),
            effects: t.effects.clone(),
            z: (0..t.z.ncols()).map(|j| t.z.col(j).to_vec()).collect(),
        })
        .collect();
    let model = BayesGlmModel {
        y: m.y.clone(),
        x,
        x_names: m.x_names[1..].to_vec(),
        trials: if r.family == GlmFamily::Binomial { m.weights.clone() } else { Vec::new() },
        offset: m.offset.clone(),
        known_sd,
        family: r.family,
        groups,
    };
    let settings = NutsSettings {
        chains: r.chains.max(1),
        warmup: r.warmup,
        draws: r.draws,
        seed: r.seed,
        target_accept: r.target_accept,
        ..NutsSettings::default()
    };
    let fit = fit_bayes_glm(&model, &r.priors, &settings, r.level)?;
    let response = built.data.response_name.clone();
    let title = model_title(r.family, !model.groups.is_empty(), &response);
    let mut out = Vec::new();

    let mut info = TextTable::new(["Item", "Value"]);
    let mut response_text = response.clone();
    if let Some(b) = &built.data.binary {
        response_text = format!("{response} = {} (against {})", b.event, b.other);
    } else if let Some(t) = r.trials {
        response_text = format!("{response} successes out of {}", column_name(table, t));
    }
    info.push(["Response".to_owned(), response_text]);
    info.push(["Family".to_owned(), family_label(r.family).to_owned()]);
    if let Some(c) = r.known_sd {
        info.push(["Known standard deviations".to_owned(), column_name(table, c)]);
    }
    if let Some(c) = r.offset {
        info.push(["Offset".to_owned(), column_name(table, c)]);
    }
    info.push(["Rows used".to_owned(), rows.len().to_string()]);
    if built.n_missing > 0 {
        info.push(["Rows left out for missing values".to_owned(), built.n_missing.to_string()]);
    }
    for g in &model.groups {
        info.push([
            format!("Groups: {}", g.name),
            format!("{} levels; effects {}", g.levels.len(), g.effects.join(", ")),
        ]);
    }
    info.push([
        "Sampling".to_owned(),
        format!(
            "No-U-Turn sampler, {} chains × {} draws after {} warm-up iterations (seed {})",
            settings.chains, settings.draws, settings.warmup, settings.seed
        ),
    ]);
    out.push(section(SectionKind::BayesData, "Model", SectionContent::Table(info)));

    let mut priors = TextTable::new(["Parameter", "Prior"]);
    for (a, b) in &fit.prior_notes {
        priors.push([a.clone(), b.clone()]);
    }
    let scale = if r.priors.autoscale { "weakly informative, scaled to the data" } else { "as set" };
    let slopes = if r.random.iter().any(|g| !g.slopes.is_empty()) {
        "; group intercepts and slopes are independent"
    } else {
        ""
    };
    out.push(section(SectionKind::BayesPrior, format!("Priors ({scale}{slopes})"), SectionContent::Table(priors)));

    let main: Vec<&ParameterSummary> = fit.parameters.iter().filter(|p| p.kind != ParameterKind::GroupEffect).collect();
    let effects: Vec<&ParameterSummary> =
        fit.parameters.iter().filter(|p| p.kind == ParameterKind::GroupEffect).collect();
    out.push(section(
        SectionKind::BayesPosterior,
        format!(
            "Posterior Summary (equal-tailed and highest-density {} intervals; R̂ and ESS rank-normalized)",
            pct(r.level)
        ),
        SectionContent::Table(summary_table(&main, r.level)),
    ));
    if !effects.is_empty() {
        out.push(section(
            SectionKind::BayesGroupEffects,
            "Group Effects (deviations from the fixed effects, shrunk toward zero)",
            SectionContent::Table(summary_table(&effects, r.level)),
        ));
    }
    out.extend(sampler_sections(&fit, &settings));
    out.push(loo_section(&fit));
    if let Some(p) = interval_plot(&fit, r.level) {
        out.push(p);
    }
    let names: Vec<String> = main.iter().chain(&effects).map(|p| p.name.clone()).collect();
    let chosen = r.plot_parameter.as_ref().and_then(|n| fit.parameters.iter().position(|p| p.name == *n)).unwrap_or(0);
    out.extend(diagnostic_plots(&fit, chosen, r.level));
    let ticks = built
        .data
        .binary
        .as_ref()
        .filter(|_| r.trials.is_none())
        .map(|b| vec![(0.0, b.other.clone()), (1.0, b.event.clone())]);
    out.push(predictive_check(&fit, &model.y, &response, ticks));
    Ok((title, out, names))
}

fn sampler_sections(fit: &BayesGlmFit, settings: &NutsSettings) -> Vec<ReportSection> {
    let mut t = TextTable::new([
        "Chain",
        "Step size",
        "Mean acceptance",
        "Divergent transitions",
        "Maximum tree depth hits",
        "E-BFMI",
    ]);
    let mut divergent = 0;
    let mut depth = 0;
    let mut low_bfmi = 0;
    for (k, c) in fit.chains.iter().enumerate() {
        let acc = c.accept.iter().sum::<f64>() / c.accept.len().max(1) as f64;
        let (d, h, e) = (c.divergences(), c.max_depth_hits(settings.max_depth), c.e_bfmi());
        divergent += d;
        depth += h;
        if e < 0.3 {
            low_bfmi += 1;
        }
        t.push([
            (k + 1).to_string(),
            format_number(c.step_size, 4),
            format!("{acc:.3}"),
            d.to_string(),
            h.to_string(),
            format!("{e:.3}"),
        ]);
    }
    let mut warnings = Vec::new();
    let worst_rhat = fit.parameters.iter().map(|p| p.diagnostics.rhat).fold(0.0, f64::max);
    let low_ess = fit
        .parameters
        .iter()
        .filter(|p| p.diagnostics.ess_bulk.min(p.diagnostics.ess_tail) < 100.0 * fit.chains.len() as f64)
        .count();
    if divergent > 0 {
        warnings.push(format!(
            "{divergent} divergent transitions: the posterior has regions the sampler could not follow, so estimates \
             may be biased. Raise the target acceptance rate, or reconsider weakly identified parameters."
        ));
    }
    if worst_rhat > 1.01 {
        warnings.push(format!(
            "The largest R̂ is {worst_rhat:.3} (above 1.01): the chains disagree. Run more draws, or check the model."
        ));
    }
    if low_ess > 0 {
        warnings.push(format!(
            "{low_ess} parameter(s) have fewer than {} effective draws (bulk or tail): intervals are imprecise. Run \
             more draws.",
            100 * fit.chains.len()
        ));
    }
    if depth > 0 {
        warnings.push(format!(
            "{depth} transitions hit the maximum tree depth: the sampler is slow here but the draws are valid."
        ));
    }
    if low_bfmi > 0 {
        warnings.push(format!("{low_bfmi} chain(s) have E-BFMI below 0.3: the energy is poorly explored."));
    }
    let status = if warnings.is_empty() { "no problems found" } else { "see the warnings" };
    let mut out =
        vec![section(SectionKind::BayesSampler, format!("Sampler Diagnostics ({status})"), SectionContent::Table(t))];
    if !warnings.is_empty() {
        out.push(section(SectionKind::BayesWarnings, "Sampler Warnings", SectionContent::Note(warnings.join("\n"))));
    }
    out
}

fn loo_section(fit: &BayesGlmFit) -> ReportSection {
    let l = &fit.loo;
    let mut t = TextTable::new(["Measure", "Estimate", "Std error"]);
    t.push([
        "Expected log predictive density (LOO)".to_owned(),
        format_number(l.elpd_loo, DIGITS),
        format_number(l.se_elpd_loo, 4),
    ]);
    t.push(["Effective number of parameters (LOO)".to_owned(), format_number(l.p_loo, DIGITS), String::new()]);
    t.push([
        "LOO information criterion".to_owned(),
        format_number(l.looic, DIGITS),
        format_number(2.0 * l.se_elpd_loo, 4),
    ]);
    t.push([
        "Expected log predictive density (WAIC)".to_owned(),
        format_number(l.elpd_waic, DIGITS),
        format_number(l.se_elpd_waic, 4),
    ]);
    t.push(["Effective number of parameters (WAIC)".to_owned(), format_number(l.p_waic, DIGITS), String::new()]);
    t.push(["WAIC".to_owned(), format_number(l.waic, DIGITS), format_number(2.0 * l.se_elpd_waic, 4)]);
    let bad = l.pareto_k.iter().filter(|&&k| k > 0.7).count();
    let ok = l.pareto_k.iter().filter(|&&k| k > 0.5 && k <= 0.7).count();
    let note = if bad > 0 {
        format!("{bad} rows have Pareto k above 0.7, so their LOO terms are unreliable")
    } else if ok > 0 {
        format!("all Pareto k ≤ 0.7 ({ok} between 0.5 and 0.7)")
    } else {
        "all Pareto k ≤ 0.5".to_owned()
    };
    section(
        SectionKind::BayesLoo,
        format!(
            "Predictive Accuracy (Pareto-smoothed importance-sampling LOO and WAIC; {note}; lower criteria are better)"
        ),
        SectionContent::Table(t),
    )
}

/// Coefficients (except the intercept) as means with credible intervals.
fn interval_plot(fit: &BayesGlmFit, level: f64) -> Option<ReportSection> {
    let params: Vec<&ParameterSummary> =
        fit.parameters.iter().filter(|p| p.kind == ParameterKind::Coefficient && p.name != "Intercept").collect();
    if params.is_empty() {
        return None;
    }
    let k = params.len();
    let y = |i: usize| (k - i) as f64;
    let spec = PlotSpec {
        x_label: "Coefficient".to_owned(),
        y_label: String::new(),
        segments: params.iter().enumerate().map(|(i, p)| vec![[p.interval.0, y(i)], [p.interval.1, y(i)]]).collect(),
        annotations: Some(Annotations {
            groups: vec!["Posterior mean".to_owned()],
            items: params.iter().enumerate().map(|(i, p)| (p.mean, y(i), String::new(), 0)).collect(),
            labels: Vec::new(),
        }),
        y_ticks: Some(params.iter().enumerate().map(|(i, p)| (y(i), p.name.clone())).collect()),
        line: Some(vec![[0.0, 0.5], [0.0, k as f64 + 0.5]]),
        line_name: Some("Zero".to_owned()),
        ..PlotSpec::default()
    };
    Some(section(
        SectionKind::BayesIntervals,
        format!("Coefficients (posterior means with equal-tailed {} intervals)", pct(level)),
        SectionContent::Plot(Box::new(spec)),
    ))
}

/// Chains side by side, each in its own pane of width `width` and gap 10%.
fn panes(chains: usize, width: f64) -> Vec<PlotPane> {
    (0..chains)
        .map(|c| {
            let x0 = c as f64 * width * 1.1;
            PlotPane { label: format!("Chain {}", c + 1), x_min: x0, x_max: x0 + width }
        })
        .collect()
}

fn chain_groups(lines: Vec<Vec<[f64; 2]>>) -> PointGroups {
    PointGroups {
        title: "Chain".to_owned(),
        labels: (1..=lines.len()).map(|c| c.to_string()).collect(),
        membership: Vec::new(),
        lines,
    }
}

fn diagnostic_plots(fit: &BayesGlmFit, p: usize, level: f64) -> Vec<ReportSection> {
    let Some(param) = fit.parameters.get(p) else { return Vec::new() };
    let chains = &fit.draws[p];
    let name = &param.name;
    let d = chains.first().map_or(0, Vec::len);
    if d < 2 {
        return Vec::new();
    }
    let width = d as f64;
    let step = (d / 400).max(1);
    let trace: Vec<Vec<[f64; 2]>> = chains
        .iter()
        .enumerate()
        .map(|(c, v)| {
            v.iter().enumerate().step_by(step).map(|(i, &x)| [c as f64 * width * 1.1 + i as f64, x]).collect()
        })
        .collect();
    let ticks = |w: f64, first: &str, last: &str| -> Vec<(f64, String)> {
        (0..chains.len())
            .flat_map(|c| {
                let x0 = c as f64 * w * 1.1;
                [(x0, first.to_owned()), (x0 + w, last.to_owned())]
            })
            .collect()
    };
    let mut out = vec![section(
        SectionKind::BayesTrace,
        format!("Trace of {name} (draws after warm-up, one pane per chain; the chains should look alike)"),
        SectionContent::Plot(Box::new(PlotSpec {
            x_label: "Draw".to_owned(),
            y_label: name.clone(),
            groups: Some(chain_groups(trace)),
            panes: panes(chains.len(), width),
            x_ticks: Some(ticks(width, "1", &d.to_string())),
            ..PlotSpec::default()
        })),
    )];
    // Rank histograms: 20 bins of the pooled ranks.
    let bins = 20;
    let total = (d * chains.len()) as f64;
    let ranks = stat_bayes::diagnostics::chain_ranks(chains);
    let rank_lines: Vec<Vec<[f64; 2]>> = ranks
        .iter()
        .enumerate()
        .map(|(c, rk)| {
            let mut counts = vec![0usize; bins];
            for &v in rk {
                let b = (((v - 0.5) / total) * bins as f64).floor().clamp(0.0, (bins - 1) as f64) as usize;
                counts[b] += 1;
            }
            let x0 = c as f64 * 1.1;
            let mut line = Vec::with_capacity(2 * bins);
            for (b, &n) in counts.iter().enumerate() {
                line.push([x0 + b as f64 / bins as f64, n as f64]);
                line.push([x0 + (b + 1) as f64 / bins as f64, n as f64]);
            }
            line
        })
        .collect();
    let expected = d as f64 / bins as f64;
    let span = chains.len() as f64 * 1.1;
    out.push(section(
        SectionKind::BayesRank,
        format!("Rank Histograms of {name} (ranks among all chains' draws; flat histograms mean the chains agree)"),
        SectionContent::Plot(Box::new(PlotSpec {
            x_label: "Rank (fraction of all draws)".to_owned(),
            y_label: "Draws per bin".to_owned(),
            groups: Some(chain_groups(rank_lines)),
            line: Some(vec![[0.0, expected], [span - 0.1, expected]]),
            line_name: Some("Expected if uniform".to_owned()),
            panes: panes(chains.len(), 1.0),
            x_ticks: Some(ticks(1.0, "0", "1")),
            ..PlotSpec::default()
        })),
    ));
    let max_lag = 30.min(d - 1);
    let lw = max_lag as f64;
    let acf: Vec<Vec<[f64; 2]>> = chains
        .iter()
        .enumerate()
        .map(|(c, v)| {
            stat_bayes::diagnostics::autocorrelation(v, max_lag)
                .into_iter()
                .enumerate()
                .map(|(lag, a)| [c as f64 * lw * 1.1 + lag as f64, a])
                .collect()
        })
        .collect();
    let bars = acf.iter().flatten().map(|p| vec![[p[0], 0.0], [p[0], p[1]]]).collect();
    out.push(section(
        SectionKind::BayesAutocorrelation,
        format!("Autocorrelation of {name} by lag (one pane per chain; fast decay means efficient sampling)"),
        SectionContent::Plot(Box::new(PlotSpec {
            x_label: "Lag".to_owned(),
            y_label: "Autocorrelation".to_owned(),
            groups: Some(chain_groups(acf)),
            segments: bars,
            line: Some(vec![[0.0, 0.0], [chains.len() as f64 * lw * 1.1 - 0.1 * lw, 0.0]]),
            line_name: Some("Zero".to_owned()),
            panes: panes(chains.len(), lw),
            x_ticks: Some(ticks(lw, "0", &max_lag.to_string())),
            ..PlotSpec::default()
        })),
    ));
    let pooled: Vec<f64> = chains.iter().flatten().copied().collect();
    let density = kde(&pooled, None, 160);
    out.push(section(
        SectionKind::BayesDensity,
        format!("Posterior of {name} (kernel density of all draws; equal-tailed {} interval marked)", pct(level)),
        SectionContent::Plot(Box::new(crate::bayes_analysis::density_plot(
            name,
            density,
            None,
            param.interval,
            level,
            None,
        ))),
    ));
    out
}

/// Gaussian kernel density of `x` on `points + 1` grid points (Silverman's
/// bandwidth), over `range` or the data's range widened by three bandwidths.
fn kde(x: &[f64], range: Option<(f64, f64)>, points: usize) -> Vec<[f64; 2]> {
    let (lo, hi, h) = kde_grid(x);
    let (lo, hi) = range.unwrap_or((lo, hi));
    let n = x.len() as f64;
    let c = 1.0 / (n * h * (2.0 * std::f64::consts::PI).sqrt());
    (0..=points)
        .map(|i| {
            let g = lo + (hi - lo) * i as f64 / points as f64;
            [g, c * x.iter().map(|v| (-0.5 * ((g - v) / h).powi(2)).exp()).sum::<f64>()]
        })
        .collect()
}

fn kde_grid(x: &[f64]) -> (f64, f64, f64) {
    let n = x.len() as f64;
    let mean = x.iter().sum::<f64>() / n;
    let sd = (x.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n - 1.0).max(1.0)).sqrt();
    let mut s = x.to_vec();
    s.sort_by(f64::total_cmp);
    let q = |p: f64| s[((s.len() - 1) as f64 * p).round() as usize];
    let iqr = q(0.75) - q(0.25);
    let spread = if iqr > 0.0 { sd.min(iqr / 1.34) } else { sd };
    let h = (0.9 * spread * n.powf(-0.2)).max(1e-9 * (1.0 + mean.abs()));
    (s[0] - 3.0 * h, s[s.len() - 1] + 3.0 * h, h)
}

/// The observed response's distribution against those of replicated data
/// sets: frequencies of each value for small counts, kernel densities
/// otherwise.
fn predictive_check(fit: &BayesGlmFit, y: &[f64], response: &str, ticks: Option<Vec<(f64, String)>>) -> ReportSection {
    let reps: Vec<&Vec<f64>> = fit.replicates.iter().take(200).collect();
    let integer = |v: &[f64]| v.iter().all(|x| (x - x.round()).abs() < 1e-9);
    let max = y.iter().chain(reps.iter().flat_map(|r| r.iter())).fold(0.0_f64, |a, &b| a.max(b));
    let discrete = integer(y) && reps.iter().all(|r| integer(r)) && max <= 40.0 && y.iter().all(|v| *v >= 0.0);
    let quantile = |mut v: Vec<f64>, p: f64| -> f64 {
        v.sort_by(f64::total_cmp);
        v[((v.len() - 1) as f64 * p).round() as usize]
    };
    let (observed, lower, upper, x_label, y_label, what) = if discrete {
        let k = max as usize;
        let freq = |v: &[f64]| -> Vec<f64> {
            let mut f = vec![0.0; k + 1];
            for &x in v {
                f[x.round() as usize] += 1.0;
            }
            f
        };
        let obs = freq(y);
        let rep: Vec<Vec<f64>> = reps.iter().map(|r| freq(r)).collect();
        let pts = |f: &dyn Fn(usize) -> f64| -> Vec<[f64; 2]> { (0..=k).map(|v| [v as f64, f(v)]).collect() };
        let lo = pts(&|v| quantile(rep.iter().map(|r| r[v]).collect(), 0.025));
        let hi = pts(&|v| quantile(rep.iter().map(|r| r[v]).collect(), 0.975));
        (pts(&|v| obs[v]), lo, hi, response.to_owned(), "Rows with this value".to_owned(), "frequencies")
    } else {
        let mut all: Vec<f64> = y.to_vec();
        for r in &reps {
            all.extend(r.iter());
        }
        let (lo, hi, _) = kde_grid(y);
        let (alo, ahi, _) = kde_grid(&all);
        let range = (lo.min(alo), hi.max(ahi));
        let obs = kde(y, Some(range), 120);
        let rep: Vec<Vec<[f64; 2]>> = reps.iter().map(|r| kde(r, Some(range), 120)).collect();
        let band = |p: f64| -> Vec<[f64; 2]> {
            (0..=120).map(|i| [obs[i][0], quantile(rep.iter().map(|r| r[i][1]).collect(), p)]).collect()
        };
        let (lower, upper) = (band(0.025), band(0.975));
        (obs, lower, upper, response.to_owned(), "Density".to_owned(), "densities")
    };
    let spec = PlotSpec {
        x_label,
        y_label,
        line: Some(observed),
        line_name: Some("Observed data".to_owned()),
        band: Some((lower, upper)),
        band_name: Some("95% of replicated data sets".to_owned()),
        x_ticks: ticks,
        ..PlotSpec::default()
    };
    section(
        SectionKind::BayesPredictive,
        format!(
            "Posterior Predictive Check (observed {what} against {} data sets drawn from the posterior; the observed \
             curve should lie within the band)",
            reps.len()
        ),
        SectionContent::Plot(Box::new(spec)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bayes_analysis::{BayesReport, BayesRequest};
    use stat_data::Column;

    fn eight_schools() -> DataTable {
        DataTable::new(
            "schools",
            vec![
                Column::text("School", ["A", "B", "C", "D", "E", "F", "G", "H"].map(str::to_owned).to_vec()),
                Column::numeric("Effect", vec![28., 8., -3., 7., -1., 1., 18., 12.]),
                Column::numeric("SE", vec![15., 10., 16., 11., 9., 11., 10., 18.]),
            ],
        )
        .unwrap()
    }

    fn round_trip(t: &DataTable, r: &BayesRequest) {
        let v = r.to_json(t);
        let key = v.get("analysis").and_then(JsonValue::as_str).unwrap().to_owned();
        assert_eq!(&BayesRequest::from_json(&key, &v, t).unwrap(), r);
    }

    #[test]
    fn hierarchical_model_report() {
        let t = eight_schools();
        let mut r = BayesModelRequest::new(1, Vec::new(), GlmFamily::Normal);
        r.known_sd = Some(2);
        r.random = vec![RandomEffectSpec { correlated: false, ..RandomEffectSpec::intercept(0) }];
        r.priors = Priors {
            intercept_location: Some(0.0),
            intercept_scale: 5.0,
            sd: ScalePrior::HalfCauchy,
            sd_scale: 5.0,
            autoscale: false,
            ..Priors::default()
        };
        r.target_accept = 0.95;
        r.plot_parameter = Some("SD(Intercept | School)".to_owned());
        let req = BayesRequest::Model(Box::new(r));
        round_trip(&t, &req);
        let rep = BayesReport::run(&t, &RowStateManager::new(8), &req).unwrap();
        assert_eq!(rep.title(), "Bayesian Hierarchical Normal Model: Effect");
        let kinds: Vec<SectionKind> = rep.sections().iter().map(|s| s.kind).collect();
        for k in [
            SectionKind::BayesPosterior,
            SectionKind::BayesGroupEffects,
            SectionKind::BayesSampler,
            SectionKind::BayesLoo,
            SectionKind::BayesTrace,
            SectionKind::BayesRank,
            SectionKind::BayesAutocorrelation,
            SectionKind::BayesPredictive,
        ] {
            assert!(kinds.contains(&k), "{k:?} missing");
        }
        assert!(rep.parameters.contains(&"School[A]: Intercept".to_owned()));
        let trace = rep.sections().into_iter().find(|s| s.kind == SectionKind::BayesTrace).unwrap();
        assert!(trace.title.contains("SD(Intercept | School)"));
        let SectionContent::Plot(p) = trace.content else { panic!() };
        assert_eq!(p.panes.len(), 4);
    }

    #[test]
    fn logistic_model_report() {
        let x: Vec<f64> = (0..40).map(|i| f64::from(i) / 4.0).collect();
        let y: Vec<String> = (0..40).map(|i| if (i * 7) % 10 < i / 4 { "Yes" } else { "No" }.to_owned()).collect();
        let t = DataTable::new("l", vec![Column::numeric("Dose", x), Column::text("Response", y)]).unwrap();
        let mut r = BayesModelRequest::new(1, vec![0], GlmFamily::Binomial);
        r.draws = 300;
        r.warmup = 300;
        let req = BayesRequest::Model(Box::new(r));
        round_trip(&t, &req);
        let rep = BayesReport::run(&t, &RowStateManager::new(40), &req).unwrap();
        assert_eq!(rep.title(), "Bayesian Logistic Model: Response");
        let SectionContent::Table(info) = &rep.sections()[0].content else { panic!() };
        assert_eq!(info.rows[0][1], "Response = Yes (against No)");
        let SectionContent::Table(post) = &rep.sections()[2].content else { panic!() };
        assert_eq!(post.rows[1][0], "Dose");
        assert!(post.rows[1][1].parse::<f64>().unwrap() > 0.0, "the event becomes likelier with dose");
    }

    #[test]
    fn linear_model_report() {
        // Two factors and a covariate.
        let n = 36;
        let a: Vec<String> = (0..n).map(|i| ["a1", "a2", "a3"][i % 3].to_owned()).collect();
        let b: Vec<String> = (0..n).map(|i| ["b1", "b2"][(i / 3) % 2].to_owned()).collect();
        let x: Vec<f64> = (0..n).map(|i| ((i * 37) % 11) as f64).collect();
        let y: Vec<f64> = (0..n).map(|i| 2.0 * (i % 3) as f64 + 0.3 * x[i] + ((i * 53) % 7) as f64 / 3.0).collect();
        let t = DataTable::new(
            "lm",
            vec![Column::numeric("Y", y), Column::text("A", a), Column::text("B", b), Column::numeric("X", x)],
        )
        .unwrap();
        let mut r = BayesLinearRequest::new(0, vec![1, 2, 3]);
        r.effects = vec![vec![(0, 1), (1, 1)]];
        r.draws = 2000;
        let req = BayesRequest::Linear(r);
        round_trip(&t, &req);
        let rep = BayesReport::run(&t, &RowStateManager::new(n), &req).unwrap();
        assert_eq!(rep.title(), "Bayesian ANOVA: Y");
        let s = rep.sections();
        let SectionContent::Table(models) = &s[0].content else { panic!() };
        assert_eq!(models.rows[0][0], "A");
        let SectionContent::Table(inc) = &s[1].content else { panic!() };
        // Heredity added A and B ahead of A × B.
        let names: Vec<&str> = inc.rows.iter().map(|r| r[0].as_str()).collect();
        assert_eq!(names, ["A", "B", "A × B"]);
        assert!(s.last().unwrap().title == "Prior");
        let SectionContent::Note(prior) = &s.last().unwrap().content else { panic!() };
        assert!(prior.contains("2 lower-order effect(s) were added"));
        let full = BayesLinearRequest::new(0, vec![1, 3]);
        let rep = BayesReport::run(&t, &RowStateManager::new(n), &BayesRequest::Linear(full)).unwrap();
        assert_eq!(rep.title(), "Bayesian Linear Model: Y");
        let SectionContent::Table(est) =
            &rep.sections().into_iter().find(|s| s.kind == SectionKind::BayesEstimates).unwrap().content
        else {
            panic!()
        };
        assert!(est.rows.iter().any(|r| r[0] == "A[a3]"));
        assert!(est.rows.iter().any(|r| r[0] == "X"));
    }
}
