//! Multiple imputation of table columns ([`stat_multivariate::multiple_imputation`]),
//! bound to the row state, and the stacked table it saves.
//!
//! The columns to impute may be continuous (numeric, continuous level) or
//! categorical (text, or numbers marked nominal or ordinal; their levels in
//! natural order). Rows that are not excluded take part; excluded rows keep
//! their missing values. The result is `m` completed copies of those
//! columns, and the **stacked table** holds them as rows: an `Imputation`
//! column (0 for the original data, with its missing values, then 1 to `m`),
//! a `Row` column (the source row number) and every column of the source
//! table, the imputed columns filled in from imputation 1 on (the long form
//! of van Buuren 2018, §5.1.2). Analyses of that table can then be pooled
//! across imputations ([`crate::pooling`]).

use stat_core::{Result, StatError};
use stat_data::{Column, ColumnData, DataTable, JsonValue, MeasurementLevel, RowStateManager, natural_order};
use stat_inference::pooling::pool_scalar;
use stat_viz::density::{kde_1d, silverman_bandwidth};

use crate::format::format_number;
use crate::report::{PlotSpec, PointGroups, ReportSection, RowPoint, SectionContent, SectionKind, TextTable};
pub use stat_multivariate::multiple_imputation::{ColumnKind, FcsMethod};
use stat_multivariate::multiple_imputation::{
    DataAugmentationOptions, FcsOptions, MultipleImputation, chained_equations, default_methods,
    normal_data_augmentation,
};

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// Name of the stacked table's imputation-number column.
pub const IMPUTATION_COLUMN: &str = "Imputation";
/// Name of the stacked table's source-row column.
pub const ROW_COLUMN: &str = "Row";

/// Which model draws the imputations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MiEngine {
    /// Chained equations (one model per column).
    ChainedEquations,
    /// Joint multivariate normal model, by data augmentation.
    NormalModel,
}

impl MiEngine {
    /// The project-file name.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::ChainedEquations => "chained-equations",
            Self::NormalModel => "normal-model",
        }
    }

    /// Default iterations per chain.
    #[must_use]
    pub fn default_iterations(self) -> usize {
        match self {
            Self::ChainedEquations => 10,
            Self::NormalModel => 50,
        }
    }
}

/// Label of a chained-equations method.
#[must_use]
pub fn method_label(method: FcsMethod) -> &'static str {
    match method {
        FcsMethod::None => "Not imputed",
        FcsMethod::PredictiveMeanMatching => "Predictive mean matching",
        FcsMethod::BayesianRegression => "Bayesian linear regression",
        FcsMethod::Logistic => "Logistic regression",
        FcsMethod::Multinomial => "Multinomial logit",
    }
}

/// Every chained-equations method, in menu order.
pub const METHODS: [FcsMethod; 5] = [
    FcsMethod::PredictiveMeanMatching,
    FcsMethod::BayesianRegression,
    FcsMethod::Logistic,
    FcsMethod::Multinomial,
    FcsMethod::None,
];

fn method_key(method: FcsMethod) -> &'static str {
    match method {
        FcsMethod::None => "none",
        FcsMethod::PredictiveMeanMatching => "pmm",
        FcsMethod::BayesianRegression => "bayesian-regression",
        FcsMethod::Logistic => "logistic",
        FcsMethod::Multinomial => "multinomial",
    }
}

fn method_from_key(key: &str) -> Option<FcsMethod> {
    METHODS.into_iter().find(|m| method_key(*m) == key)
}

/// Settings of a multiple imputation. Per-column settings refer to the
/// chosen columns by position (0 = first chosen column).
#[derive(Debug, Clone, PartialEq)]
pub struct MiSettings {
    /// The model.
    pub engine: MiEngine,
    /// Number of imputations `m`.
    pub imputations: usize,
    /// Iterations per chain.
    pub iterations: usize,
    /// Donors for predictive mean matching.
    pub donors: usize,
    /// Method of each chosen column (chained equations); `None` entries,
    /// or a list shorter than the columns, take the default for the column.
    pub methods: Vec<Option<FcsMethod>>,
    /// `predictors[j][k]`: chosen column `k` predicts chosen column `j`
    /// (`None`: every other column).
    pub predictors: Option<Vec<Vec<bool>>>,
    /// Order in which chosen columns are visited (`None`: left to right).
    pub visit_order: Option<Vec<usize>>,
}

impl Default for MiSettings {
    fn default() -> Self {
        Self {
            engine: MiEngine::ChainedEquations,
            imputations: Self::DEFAULT_IMPUTATIONS,
            iterations: MiEngine::ChainedEquations.default_iterations(),
            donors: Self::DEFAULT_DONORS,
            methods: Vec::new(),
            predictors: None,
            visit_order: None,
        }
    }
}

impl MiSettings {
    /// Default number of imputations.
    pub const DEFAULT_IMPUTATIONS: usize = 20;
    /// Default donors.
    pub const DEFAULT_DONORS: usize = 5;
    /// Most imputations allowed.
    pub const MAX_IMPUTATIONS: usize = 500;

    /// Members for a report's or table operation's JSON object.
    pub(crate) fn json_members(&self) -> Vec<(String, JsonValue)> {
        let mut out = vec![
            ("engine".to_owned(), crate::project::s(self.engine.key())),
            ("imputations".to_owned(), JsonValue::number(self.imputations as f64)),
            ("iterations".to_owned(), JsonValue::number(self.iterations as f64)),
            ("donors".to_owned(), JsonValue::number(self.donors as f64)),
        ];
        if self.methods.iter().any(Option::is_some) {
            let list = self.methods.iter().map(|m| m.map_or(JsonValue::Null, |m| crate::project::s(method_key(m))));
            out.push(("column_methods".to_owned(), JsonValue::Array(list.collect())));
        }
        if let Some(p) = &self.predictors {
            let rows = p.iter().map(|r| JsonValue::Array(r.iter().map(|&b| JsonValue::Bool(b)).collect()));
            out.push(("predictors".to_owned(), JsonValue::Array(rows.collect())));
        }
        if let Some(order) = &self.visit_order {
            let list = order.iter().map(|&j| JsonValue::number(j as f64));
            out.push(("visit_order".to_owned(), JsonValue::Array(list.collect())));
        }
        out
    }

    /// Read the members [`MiSettings::json_members`] wrote; missing ones take
    /// their defaults.
    pub(crate) fn from_json(v: &JsonValue) -> Result<Self> {
        let engine = match v.get("engine").and_then(JsonValue::as_str) {
            None | Some("chained-equations") => MiEngine::ChainedEquations,
            Some("normal-model") => MiEngine::NormalModel,
            Some(k) => return Err(invalid(format!("unknown imputation engine {k:?}"))),
        };
        let d = Self::default();
        let methods = match v.get("column_methods").and_then(JsonValue::as_array) {
            None => Vec::new(),
            Some(list) => list
                .iter()
                .map(|m| match m {
                    JsonValue::Null => Ok(None),
                    m => m
                        .as_str()
                        .and_then(method_from_key)
                        .map(Some)
                        .ok_or_else(|| invalid("column_methods must name imputation methods")),
                })
                .collect::<Result<_>>()?,
        };
        let predictors = match v.get("predictors").and_then(JsonValue::as_array) {
            None => None,
            Some(rows) => Some(
                rows.iter()
                    .map(|r| {
                        r.as_array()
                            .ok_or_else(|| invalid("predictors must be a list of lists of true/false"))?
                            .iter()
                            .map(|b| b.as_bool().ok_or_else(|| invalid("predictors must be true or false")))
                            .collect()
                    })
                    .collect::<Result<_>>()?,
            ),
        };
        let visit_order = match v.get("visit_order").and_then(JsonValue::as_array) {
            None => None,
            Some(list) => Some(
                list.iter()
                    .map(|j| j.as_usize().ok_or_else(|| invalid("visit_order must list column positions")))
                    .collect::<Result<_>>()?,
            ),
        };
        Ok(Self {
            engine,
            imputations: v.get("imputations").and_then(JsonValue::as_usize).unwrap_or(d.imputations),
            iterations: v.get("iterations").and_then(JsonValue::as_usize).unwrap_or(engine.default_iterations()),
            donors: v.get("donors").and_then(JsonValue::as_usize).unwrap_or(d.donors),
            methods,
            predictors,
            visit_order,
        })
    }
}

/// One imputed column.
#[derive(Debug, Clone, PartialEq)]
pub struct MiColumn {
    /// Column name.
    pub name: String,
    /// Continuous or categorical (with its number of levels).
    pub kind: ColumnKind,
    /// Level labels of a categorical column, in code order.
    pub levels: Vec<String>,
}

/// The kind of column `c` of `table` and, for a categorical one, its level
/// labels in natural order with each row's code (`NaN` where missing). A
/// numeric continuous column gives its values.
fn code_column(table: &DataTable, c: usize) -> Result<(MiColumn, Vec<f64>)> {
    let col = table.column(c).ok_or_else(|| invalid(format!("no column {c}")))?;
    if matches!(col.data, ColumnData::DateTime { .. }) {
        return Err(invalid(format!("{} holds dates or times, which cannot be imputed", col.name)));
    }
    if let (Some(v), MeasurementLevel::Continuous) = (col.as_numeric(), col.level) {
        let values = v.iter().map(|&x| if x.is_finite() { x } else { f64::NAN }).collect();
        return Ok((MiColumn { name: col.name.clone(), kind: ColumnKind::Continuous, levels: Vec::new() }, values));
    }
    let texts: Vec<String> = (0..table.n_rows()).map(|r| col.cell_text(r).trim().to_owned()).collect();
    let mut levels: Vec<String> = texts.iter().filter(|t| !t.is_empty()).cloned().collect();
    levels.sort_by(|a, b| natural_order(a, b));
    levels.dedup();
    if levels.len() < 2 {
        return Err(invalid(format!("{} needs at least two observed categories to be imputed", col.name)));
    }
    let codes = texts
        .iter()
        .map(|t| {
            if t.is_empty() {
                f64::NAN
            } else {
                levels.binary_search_by(|l| natural_order(l, t)).map_or(f64::NAN, |i| i as f64)
            }
        })
        .collect();
    let kind = ColumnKind::Categorical { levels: levels.len() };
    Ok((MiColumn { name: col.name.clone(), kind, levels }, codes))
}

/// A computed multiple imputation of table columns.
#[derive(Debug, Clone)]
pub struct MiRun {
    /// The imputed columns (table indices).
    pub columns: Vec<usize>,
    /// Their names, kinds and levels.
    pub info: Vec<MiColumn>,
    /// Rows used (not excluded), ascending.
    pub rows: Vec<usize>,
    /// The data of the rows used (`NaN` missing; categorical columns as codes).
    pub data: Vec<Vec<f64>>,
    /// Method used per column (all [`FcsMethod::None`] for the normal model,
    /// which imputes every column with missing values).
    pub methods: Vec<FcsMethod>,
    /// Predictor matrix used (chained equations).
    pub predictors: Vec<Vec<bool>>,
    /// The completed data sets and chain traces.
    pub result: MultipleImputation,
    /// Settings.
    pub settings: MiSettings,
    /// Seed.
    pub seed: u64,
}

impl MiRun {
    /// Impute `columns` of `table` with `settings` and `seed`.
    ///
    /// # Errors
    /// No columns, an unknown or date-time column, a categorical column with
    /// fewer than two categories, a categorical column with the normal
    /// model, a number of imputations outside 1–500, settings of the wrong
    /// size, or any error of the imputation engines.
    pub fn run(
        table: &DataTable,
        states: &RowStateManager,
        columns: &[usize],
        settings: &MiSettings,
        seed: u64,
    ) -> Result<Self> {
        if columns.is_empty() {
            return Err(invalid("choose at least one column"));
        }
        if !(1..=MiSettings::MAX_IMPUTATIONS).contains(&settings.imputations) {
            return Err(invalid(format!("make between 1 and {} imputations", MiSettings::MAX_IMPUTATIONS)));
        }
        let mut info = Vec::new();
        let mut values = Vec::new();
        for &c in columns {
            let (i, v) = code_column(table, c)?;
            info.push(i);
            values.push(v);
        }
        let rows: Vec<usize> = (0..table.n_rows()).filter(|&r| !states.is_excluded(r)).collect();
        let data: Vec<Vec<f64>> = rows.iter().map(|&r| values.iter().map(|v| v[r]).collect()).collect();
        let kinds: Vec<ColumnKind> = info.iter().map(|i| i.kind).collect();
        let p = columns.len();
        let (result, methods, predictors) = match settings.engine {
            MiEngine::NormalModel => {
                if let Some(c) = info.iter().find(|i| i.kind != ColumnKind::Continuous) {
                    return Err(invalid(format!(
                        "{} is categorical: the multivariate normal model takes continuous columns only (use \
                         chained equations)",
                        c.name
                    )));
                }
                let options =
                    DataAugmentationOptions { m: settings.imputations, iterations: settings.iterations, seed };
                (normal_data_augmentation(&data, &options)?, vec![FcsMethod::None; p], Vec::new())
            }
            MiEngine::ChainedEquations => {
                let mut options = FcsOptions::new(&data, &kinds);
                options.m = settings.imputations;
                options.iterations = settings.iterations;
                options.donors = settings.donors;
                options.seed = seed;
                let defaults = default_methods(&data, &kinds);
                options.methods =
                    (0..p).map(|j| settings.methods.get(j).copied().flatten().unwrap_or(defaults[j])).collect();
                if let Some(pm) = &settings.predictors {
                    if pm.len() != p || pm.iter().any(|r| r.len() != p) {
                        return Err(invalid("the predictor matrix must have one row and column per chosen column"));
                    }
                    options.predictors.clone_from(pm);
                }
                if let Some(order) = &settings.visit_order {
                    if order.iter().any(|&j| j >= p) {
                        return Err(invalid("the visit order names a column that was not chosen"));
                    }
                    options.visit_order.clone_from(order);
                }
                for (j, m) in options.methods.iter().enumerate() {
                    if !m.suits(kinds[j]) {
                        return Err(invalid(format!(
                            "{} cannot be imputed by {}",
                            info[j].name,
                            method_label(*m).to_lowercase()
                        )));
                    }
                }
                let result = chained_equations(&data, &kinds, &options)?;
                (result, options.methods, options.predictors)
            }
        };
        Ok(Self {
            columns: columns.to_vec(),
            info,
            rows,
            data,
            methods,
            predictors,
            result,
            settings: settings.clone(),
            seed,
        })
    }

    /// Number of imputations.
    #[must_use]
    pub fn m(&self) -> usize {
        self.result.imputations.len()
    }

    /// The stacked table named `name`: `Imputation` (0 = original data, then
    /// 1 to `m`), `Row` (source row number, from 1), then every column of
    /// `table` with the imputed columns filled in. Also returns each stacked
    /// row's source row.
    ///
    /// # Errors
    /// Building the table fails (it cannot for a valid run).
    pub fn stacked(&self, table: &DataTable, name: &str) -> Result<(DataTable, Vec<usize>)> {
        let n = table.n_rows();
        let m = self.m();
        let copies = m + 1;
        let source_rows: Vec<usize> = (0..copies).flat_map(|_| 0..n).collect();
        // Position of each table row among the rows used.
        let mut used = vec![None; n];
        for (k, &r) in self.rows.iter().enumerate() {
            used[r] = Some(k);
        }
        let taken: Vec<&str> = table.columns().iter().map(|c| c.name.as_str()).collect();
        let imp_name = crate::table_ops::unique_name(IMPUTATION_COLUMN, taken.iter().copied());
        let row_name = crate::table_ops::unique_name(ROW_COLUMN, taken.iter().copied().chain([imp_name.as_str()]));
        let mut imp = Column::numeric(imp_name, (0..copies).flat_map(|l| std::iter::repeat_n(l as f64, n)).collect());
        imp.level = MeasurementLevel::Ordinal;
        let mut row = Column::numeric(row_name, source_rows.iter().map(|&r| (r + 1) as f64).collect());
        row.level = MeasurementLevel::Ordinal;
        let mut columns = vec![imp, row];
        for (c, col) in table.columns().iter().enumerate() {
            let mut out = Column { formula: None, ..col.clone() };
            match self.columns.iter().position(|&x| x == c) {
                None => {
                    out.data = repeat_data(&col.data, copies);
                }
                Some(j) => {
                    let info = &self.info[j];
                    let cell = |l: usize, r: usize| -> Option<f64> {
                        if l == 0 {
                            return None;
                        }
                        used[r].map(|k| self.result.imputations[l - 1][k][j])
                    };
                    out.data = match &col.data {
                        ColumnData::Numeric(v) => ColumnData::Numeric(
                            (0..copies)
                                .flat_map(|l| (0..n).map(move |r| (l, r)))
                                .map(|(l, r)| {
                                    if v[r].is_finite() {
                                        return v[r];
                                    }
                                    match (cell(l, r), info.kind) {
                                        (Some(x), ColumnKind::Continuous) => x,
                                        (Some(x), ColumnKind::Categorical { .. }) => {
                                            info.levels[x as usize].parse().unwrap_or(f64::NAN)
                                        }
                                        (None, _) => f64::NAN,
                                    }
                                })
                                .collect(),
                        ),
                        ColumnData::Text(v) => ColumnData::Text(
                            (0..copies)
                                .flat_map(|l| (0..n).map(move |r| (l, r)))
                                .map(|(l, r)| {
                                    if !v[r].trim().is_empty() {
                                        return v[r].clone();
                                    }
                                    cell(l, r).map_or_else(|| v[r].clone(), |x| info.levels[x as usize].clone())
                                })
                                .collect(),
                        ),
                        ColumnData::DateTime { .. } => repeat_data(&col.data, copies),
                    };
                }
            }
            columns.push(out);
        }
        Ok((DataTable::new(name, columns)?, source_rows))
    }
}

/// Most imputed columns given trace, strip and density plots.
pub const PLOTTED_COLUMNS: usize = 8;

fn section(kind: SectionKind, title: &str, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.to_owned(), content }
}

impl MiRun {
    /// Names of column `j`'s predictors.
    fn predictor_names(&self, j: usize) -> String {
        let names: Vec<&str> = (0..self.info.len())
            .filter(|&k| k != j && self.predictors.get(j).is_some_and(|r| r[k]))
            .map(|k| self.info[k].name.as_str())
            .collect();
        if names.is_empty() { "(intercept only)".to_owned() } else { names.join(", ") }
    }

    /// Rows (positions among the rows used) where column `j` is missing.
    fn missing_rows(&self, j: usize) -> Vec<usize> {
        (0..self.data.len()).filter(|&k| self.data[k][j].is_nan()).collect()
    }

    /// Report sections describing the imputation models, the pooled means,
    /// convergence and how imputed values compare with observed ones.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let mut out = vec![self.models_table()];
        if let Some(t) = self.pooled_means() {
            out.push(t);
        }
        let plotted: Vec<usize> =
            (0..self.info.len()).filter(|&j| self.result.imputed[j]).take(PLOTTED_COLUMNS).collect();
        for &j in &plotted {
            match self.info[j].kind {
                ColumnKind::Continuous => {
                    out.push(self.trace_plot(j, false));
                    out.push(self.trace_plot(j, true));
                    out.push(self.strip_plot(j));
                    if let Some(d) = self.density_plot(j) {
                        out.push(d);
                    }
                }
                ColumnKind::Categorical { levels } => {
                    if levels == 2 {
                        out.push(self.trace_plot(j, false));
                    }
                    out.push(self.category_table(j));
                }
            }
        }
        out
    }

    fn models_table(&self) -> ReportSection {
        let normal = self.settings.engine == MiEngine::NormalModel;
        let mut t = TextTable::new(["Column", "Kind", "Missing", "Imputed by", "Predictors", "Visited"]);
        let order = self.settings.visit_order.clone().unwrap_or_else(|| (0..self.info.len()).collect());
        for (j, c) in self.info.iter().enumerate() {
            let missing = self.missing_rows(j).len();
            let kind = match c.kind {
                ColumnKind::Continuous => "Continuous".to_owned(),
                ColumnKind::Categorical { levels } => format!("{levels} categories"),
            };
            let (method, predictors, visited) = if normal {
                let m = if missing > 0 { "Multivariate normal model" } else { "Not imputed" };
                (m.to_owned(), "(all columns jointly)".to_owned(), String::new())
            } else if self.result.imputed[j] {
                let at = order.iter().position(|&k| k == j).map_or(String::new(), |p| (p + 1).to_string());
                (method_label(self.methods[j]).to_owned(), self.predictor_names(j), at)
            } else {
                ("Not imputed".to_owned(), String::new(), String::new())
            };
            t.push([c.name.clone(), kind, missing.to_string(), method, predictors, visited]);
        }
        section(SectionKind::ImputationModels, "Imputation Models", SectionContent::Table(t))
    }

    /// Each continuous column's mean in every imputation, pooled by Rubin's
    /// rules (variance s²/n, ν_com = n − 1).
    fn pooled_means(&self) -> Option<ReportSection> {
        if self.m() < 2 {
            return None;
        }
        let mut t = TextTable::new([
            "Column",
            "Observed mean",
            "Pooled mean",
            "Std error",
            "Lower 95%",
            "Upper 95%",
            "Fraction of missing information",
        ]);
        let n = self.data.len();
        for (j, c) in self.info.iter().enumerate() {
            if c.kind != ColumnKind::Continuous || !self.result.imputed[j] || n < 2 {
                continue;
            }
            let (q, u): (Vec<f64>, Vec<f64>) = self
                .result
                .imputations
                .iter()
                .map(|d| {
                    let v: Vec<f64> = d.iter().map(|r| r[j]).collect();
                    let mean = v.iter().sum::<f64>() / n as f64;
                    let var = v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1) as f64;
                    (mean, var / n as f64)
                })
                .unzip();
            let obs: Vec<f64> = self.data.iter().map(|r| r[j]).filter(|x| !x.is_nan()).collect();
            let Ok(p) = pool_scalar(&q, &u, (n - 1) as f64, 0.0, 0.95) else { continue };
            t.push([
                c.name.clone(),
                format_number(obs.iter().sum::<f64>() / obs.len().max(1) as f64, 6),
                format_number(p.estimate, 6),
                format_number(p.std_error, 4),
                format_number(p.interval.0, 6),
                format_number(p.interval.1, 6),
                format_number(p.fmi, 3),
            ]);
        }
        (!t.rows.is_empty()).then(|| {
            section(
                SectionKind::ImputationPooledMeans,
                &format!("Means Pooled Across {} Imputations", self.m()),
                SectionContent::Table(t),
            )
        })
    }

    /// Mean (or standard deviation) of column `j`'s imputed values by
    /// iteration: a thin line per chain and their average.
    fn trace_plot(&self, j: usize, spread: bool) -> ReportSection {
        let traces = if spread { &self.result.chains.sds } else { &self.result.chains.means };
        let segments: Vec<Vec<[f64; 2]>> = traces
            .iter()
            .map(|chain| chain.iter().enumerate().map(|(t, v)| [t as f64, v[j]]).filter(|p| p[1].is_finite()).collect())
            .collect();
        let iterations = traces.first().map_or(0, Vec::len);
        let average: Vec<[f64; 2]> = (0..iterations)
            .map(|t| {
                let v: Vec<f64> = traces.iter().map(|c| c[t][j]).filter(|x| x.is_finite()).collect();
                [t as f64, v.iter().sum::<f64>() / v.len().max(1) as f64]
            })
            .filter(|p| p[1].is_finite())
            .collect();
        let c = &self.info[j];
        let what = match (spread, c.kind) {
            (true, _) => "Std dev of imputed values".to_owned(),
            (false, ColumnKind::Categorical { .. }) => {
                format!("Share imputed as {}", c.levels.get(1).map_or("", String::as_str))
            }
            (false, ColumnKind::Continuous) => "Mean of imputed values".to_owned(),
        };
        let spec = PlotSpec {
            x_label: "Iteration".to_owned(),
            y_label: what.clone(),
            line: Some(average),
            line_name: Some("Average over chains".to_owned()),
            segments,
            ..PlotSpec::default()
        };
        let title = format!("Convergence: {what} of {} (one line per imputation)", c.name);
        section(SectionKind::ImputationTrace, &title, SectionContent::Plot(Box::new(spec)))
    }

    /// Observed values at 0 and each imputation's imputed values at its
    /// number, spread a little sideways.
    fn strip_plot(&self, j: usize) -> ReportSection {
        let mut points = Vec::new();
        let mut membership = Vec::new();
        let jitter = |k: usize| ((k * 7919 % 101) as f64 / 100.0 - 0.5) * 0.36;
        for (k, row) in self.data.iter().enumerate() {
            if !row[j].is_nan() {
                points.push(RowPoint { row: self.rows[k], x: jitter(k), y: row[j] });
                membership.push(0);
            }
        }
        let missing = self.missing_rows(j);
        for (l, imp) in self.result.imputations.iter().enumerate() {
            for &k in &missing {
                points.push(RowPoint { row: self.rows[k], x: (l + 1) as f64 + jitter(k), y: imp[k][j] });
                membership.push(1);
            }
        }
        let name = &self.info[j].name;
        let ticks = (0..=self.m()).map(|l| (l as f64, if l == 0 { "Observed".to_owned() } else { l.to_string() }));
        let spec = PlotSpec {
            x_label: "Imputation".to_owned(),
            y_label: name.clone(),
            points,
            groups: Some(PointGroups {
                title: "Values".to_owned(),
                labels: vec!["Observed".to_owned(), "Imputed".to_owned()],
                membership,
                lines: Vec::new(),
            }),
            x_ticks: Some(ticks.collect()),
            ..PlotSpec::default()
        };
        section(
            SectionKind::ImputationStrip,
            &format!("Observed and Imputed Values of {name}"),
            SectionContent::Plot(Box::new(spec)),
        )
    }

    /// Kernel density of the observed values and of each imputation's
    /// imputed values (Silverman's bandwidth).
    fn density_plot(&self, j: usize) -> Option<ReportSection> {
        let observed: Vec<f64> = self.data.iter().map(|r| r[j]).filter(|x| !x.is_nan()).collect();
        let missing = self.missing_rows(j);
        if observed.len() < 2 || missing.len() < 2 {
            return None;
        }
        let imputed: Vec<Vec<f64>> =
            self.result.imputations.iter().map(|imp| missing.iter().map(|&k| imp[k][j]).collect()).collect();
        let all = observed.iter().chain(imputed.iter().flatten());
        let (lo, hi) = all.fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &x| (a.min(x), b.max(x)));
        let pad = 0.1 * (hi - lo).max(1e-9);
        let curve = |v: &[f64]| {
            let w = vec![1.0; v.len()];
            let h = silverman_bandwidth(v, &w);
            if h > 0.0 && h.is_finite() { kde_1d(v, &w, h, lo - pad, hi + pad, 128) } else { Vec::new() }
        };
        let name = &self.info[j].name;
        let spec = PlotSpec {
            x_label: name.clone(),
            y_label: "Density".to_owned(),
            line: Some(curve(&observed)),
            line_name: Some("Observed values".to_owned()),
            segments: imputed.iter().map(|v| curve(v)).collect(),
            ..PlotSpec::default()
        };
        Some(section(
            SectionKind::ImputationDensity,
            &format!("Densities of {name}: Observed (bold) and Each Imputation (thin)"),
            SectionContent::Plot(Box::new(spec)),
        ))
    }

    fn category_table(&self, j: usize) -> ReportSection {
        let c = &self.info[j];
        let levels = c.levels.len();
        let mut observed = vec![0usize; levels];
        for row in &self.data {
            if !row[j].is_nan() {
                observed[row[j] as usize] += 1;
            }
        }
        let missing = self.missing_rows(j);
        let mut imputed = vec![0usize; levels];
        for imp in &self.result.imputations {
            for &k in &missing {
                imputed[imp[k][j] as usize] += 1;
            }
        }
        let (no, ni) = (observed.iter().sum::<usize>().max(1) as f64, imputed.iter().sum::<usize>().max(1) as f64);
        let mut t = TextTable::new(["Category", "Observed", "% of observed", "% of imputed (all imputations)"]);
        for (l, label) in c.levels.iter().enumerate() {
            t.push([
                label.clone(),
                observed[l].to_string(),
                format_number(100.0 * observed[l] as f64 / no, 4),
                format_number(100.0 * imputed[l] as f64 / ni, 4),
            ]);
        }
        section(
            SectionKind::ImputationCategories,
            &format!("Categories of {}: Observed and Imputed", c.name),
            SectionContent::Table(t),
        )
    }
}

/// `copies` copies of a column's values, one after another.
fn repeat_data(data: &ColumnData, copies: usize) -> ColumnData {
    match data {
        ColumnData::Numeric(v) => ColumnData::Numeric(v.repeat(copies)),
        ColumnData::Text(v) => ColumnData::Text((0..copies).flat_map(|_| v.iter().cloned()).collect()),
        ColumnData::DateTime { values, display } => {
            ColumnData::DateTime { values: values.repeat(copies), display: *display }
        }
    }
}

/// The column of `table` holding imputation numbers: a numeric column named
/// [`IMPUTATION_COLUMN`] whose values are whole numbers from 0 with at least
/// two imputations (values ≥ 1). `None` when there is none.
#[must_use]
pub fn imputation_column(table: &DataTable) -> Option<usize> {
    let c = table.find(IMPUTATION_COLUMN)?;
    let v = table.column(c)?.as_numeric()?;
    let whole = v.iter().all(|x| x.is_finite() && *x >= 0.0 && x.fract() == 0.0);
    let mut seen: Vec<u64> = v.iter().filter(|x| **x >= 1.0).map(|x| *x as u64).collect();
    seen.sort_unstable();
    seen.dedup();
    (whole && seen.len() >= 2).then_some(c)
}

#[cfg(test)]
mod tests;
