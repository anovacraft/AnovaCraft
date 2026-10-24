//! Missing-value imputation of numeric columns, bound to the row state
//! ([`stat_multivariate::imputation`]).
//!
//! Rows that are not excluded take part; excluded rows are left as they are
//! (their missing values stay missing in the saved columns). The report
//! describes where values are missing (counts per column, the distinct
//! missing-data patterns as a table and a map), the method's estimates and
//! convergence, and lists the imputed values with a scatter plot of the
//! first two columns marking rows that received one. Each completed column,
//! and a count of imputed values per row, can be saved as new columns.

use stat_core::{Result, StatError};
use stat_data::{DataTable, JsonValue, RowStateManager};
use stat_multivariate::imputation::{
    Center, EmImputation, EmOptions, MissingPatterns, RankChoice, SvdImputation, SvdOptions, choose_rank, em_normal,
    impute_center, missing_patterns, svd_impute,
};

use crate::format::format_number;
use crate::heat_map::HeatMapSpec;
use crate::report::{PlotSpec, PointGroups, ReportSection, RowPoint, SectionContent, SectionKind, TextTable};
use crate::report_columns::ReportColumn;

/// Most imputed values listed in the report's table.
pub const LISTED_VALUES: usize = 500;
/// Most patterns drawn in the pattern map.
pub const MAPPED_PATTERNS: usize = 40;

/// How missing values are filled in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImputationMethod {
    /// Each column's observed mean.
    Mean,
    /// Each column's observed median.
    Median,
    /// Conditional means under a multivariate normal fitted by the EM algorithm.
    NormalEm,
    /// An iterated low-rank (SVD) fit.
    LowRank,
    /// Multiple imputation by chained equations (`m` completed data sets).
    ChainedEquations,
    /// Multiple imputation under a multivariate normal model (data augmentation).
    NormalModel,
}

impl ImputationMethod {
    /// Every method, in menu order.
    pub const ALL: [Self; 6] =
        [Self::NormalEm, Self::LowRank, Self::Mean, Self::Median, Self::ChainedEquations, Self::NormalModel];

    /// `true` for the methods that make several completed data sets.
    #[must_use]
    pub fn is_multiple(self) -> bool {
        matches!(self, Self::ChainedEquations | Self::NormalModel)
    }

    /// Label for dialogs and reports.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Mean => "Column mean",
            Self::Median => "Column median",
            Self::NormalEm => "Multivariate normal (EM)",
            Self::LowRank => "Low-rank SVD",
            Self::ChainedEquations => "Multiple imputation: chained equations",
            Self::NormalModel => "Multiple imputation: multivariate normal",
        }
    }

    /// The project-file name.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Mean => "mean",
            Self::Median => "median",
            Self::NormalEm => "normal-em",
            Self::LowRank => "low-rank-svd",
            Self::ChainedEquations => "multiple-chained-equations",
            Self::NormalModel => "multiple-normal-model",
        }
    }

    /// The method named `key`.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.key() == key)
    }
}

/// Columns, method and settings.
#[derive(Debug, Clone, PartialEq)]
pub struct ImputationRequest {
    /// Numeric columns to impute (and to impute from).
    pub columns: Vec<usize>,
    /// The method.
    pub method: ImputationMethod,
    /// Rank of the low-rank fit; `None` chooses it by holding out observed values.
    pub rank: Option<usize>,
    /// Soft-threshold of the singular values (low-rank fit; 0 for none;
    /// above 0 it replaces `regularize`).
    pub shrinkage: f64,
    /// Shrink the low-rank fit's singular values by the estimated noise
    /// variance (regularized iterative PCA, Josse & Husson 2012), which keeps
    /// the iteration from drifting. On by default.
    pub regularize: bool,
    /// Standardize the columns before the low-rank fit.
    pub standardize: bool,
    /// Most iterations (EM and low-rank fit).
    pub max_iterations: usize,
    /// Convergence tolerance (EM and low-rank fit).
    pub tolerance: f64,
    /// Seed for choosing the rank, or of the first chain of a multiple
    /// imputation.
    pub seed: u64,
    /// Settings of a multiple imputation (its engine follows `method`).
    pub multiple: crate::multiple_imputation::MiSettings,
}

impl ImputationRequest {
    /// Default most iterations.
    pub const DEFAULT_MAX_ITERATIONS: usize = 1000;
    /// Default tolerance.
    pub const DEFAULT_TOLERANCE: f64 = 1e-8;
    /// Default seed.
    pub const DEFAULT_SEED: u64 = 1;
    /// Largest rank tried when choosing it.
    pub const MAX_CHOSEN_RANK: usize = 10;
    /// Share of observed values held out when choosing the rank.
    pub const HELD_OUT_SHARE: f64 = 0.1;

    /// EM imputation of `columns` with default settings.
    #[must_use]
    pub fn new(columns: Vec<usize>) -> Self {
        Self {
            columns,
            method: ImputationMethod::NormalEm,
            rank: None,
            shrinkage: 0.0,
            regularize: true,
            standardize: true,
            max_iterations: Self::DEFAULT_MAX_ITERATIONS,
            tolerance: Self::DEFAULT_TOLERANCE,
            seed: Self::DEFAULT_SEED,
            multiple: crate::multiple_imputation::MiSettings::default(),
        }
    }

    /// The multiple-imputation settings with the engine `method` names.
    #[must_use]
    pub fn mi_settings(&self) -> crate::multiple_imputation::MiSettings {
        use crate::multiple_imputation::MiEngine;
        let engine = if self.method == ImputationMethod::NormalModel {
            MiEngine::NormalModel
        } else {
            MiEngine::ChainedEquations
        };
        crate::multiple_imputation::MiSettings { engine, ..self.multiple.clone() }
    }

    /// For a multiple imputation, the table operation that makes its
    /// stacked table: the same columns (by name), settings and seed.
    #[must_use]
    pub fn stacked_operation(&self, table: &DataTable) -> Option<crate::table_ops::TableOperation> {
        self.method.is_multiple().then(|| crate::table_ops::TableOperation::MultipleImputation {
            columns: self.columns.iter().filter_map(|&c| table.column(c).map(|c| c.name.clone())).collect(),
            settings: self.mi_settings(),
            seed: self.seed,
        })
    }

    /// Every column the request reads.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        self.columns.clone()
    }

    /// Pass every column index through `f`.
    pub fn map_columns(&mut self, f: &impl Fn(usize) -> usize) {
        self.columns.iter_mut().for_each(|c| *c = f(*c));
    }

    /// The project-file form (`"analysis": "imputation"`).
    #[must_use]
    pub fn to_json(&self, table: &DataTable) -> JsonValue {
        let v = crate::project::obj([
            ("analysis", crate::project::s("imputation")),
            ("columns", crate::correlation_analysis::columns_json(&self.columns, table)),
            ("method", crate::project::s(self.method.key())),
            ("rank", self.rank.map_or(JsonValue::Null, |r| JsonValue::number(r as f64))),
            ("shrinkage", JsonValue::number(self.shrinkage)),
            ("regularize", JsonValue::Bool(self.regularize)),
            ("standardize", JsonValue::Bool(self.standardize)),
            ("max_iterations", JsonValue::number(self.max_iterations as f64)),
            ("tolerance", JsonValue::number(self.tolerance)),
            ("seed", JsonValue::number(self.seed as f64)),
        ]);
        match v {
            JsonValue::Object(mut members) if self.method.is_multiple() => {
                // The engine follows "method"; the rest are the settings.
                members.extend(self.multiple.json_members().into_iter().filter(|(k, _)| k != "engine"));
                JsonValue::Object(members)
            }
            v => v,
        }
    }

    /// Read the project-file form; missing settings take their defaults.
    ///
    /// # Errors
    /// Missing or unknown columns, an unknown method.
    pub fn from_json(v: &JsonValue, table: &DataTable) -> Result<Self> {
        let d = Self::new(crate::correlation_analysis::json_columns(v, "columns", table)?);
        let method = match v.get("method").and_then(JsonValue::as_str) {
            None => d.method,
            Some(k) => {
                ImputationMethod::from_key(k).ok_or_else(|| invalid(format!("unknown imputation method {k:?}")))?
            }
        };
        Ok(Self {
            method,
            rank: v.get("rank").and_then(JsonValue::as_usize),
            shrinkage: v.get("shrinkage").and_then(JsonValue::as_f64).unwrap_or(d.shrinkage),
            regularize: v.get("regularize").and_then(JsonValue::as_bool).unwrap_or(d.regularize),
            standardize: v.get("standardize").and_then(JsonValue::as_bool).unwrap_or(d.standardize),
            max_iterations: v.get("max_iterations").and_then(JsonValue::as_usize).unwrap_or(d.max_iterations),
            tolerance: v.get("tolerance").and_then(JsonValue::as_f64).unwrap_or(d.tolerance),
            seed: v.get("seed").and_then(crate::seed::seed_from_json).unwrap_or(d.seed),
            multiple: if method.is_multiple() {
                let mut m = crate::multiple_imputation::MiSettings::from_json(v)?;
                m.engine = crate::multiple_imputation::MiSettings::default().engine;
                if v.get("iterations").is_none() && method == ImputationMethod::NormalModel {
                    m.iterations = crate::multiple_imputation::MiEngine::NormalModel.default_iterations();
                }
                m
            } else {
                d.multiple.clone()
            },
            ..d
        })
    }
}

/// What the chosen method found.
#[derive(Debug, Clone)]
enum Fit {
    Center(Vec<f64>),
    Em(Box<EmImputation>),
    LowRank { fit: SvdImputation, rank: usize, choice: Option<RankChoice> },
    Multiple(Box<crate::multiple_imputation::MiRun>),
}

/// A computed imputation.
#[derive(Debug, Clone)]
pub struct ImputationReport {
    request: ImputationRequest,
    names: Vec<String>,
    /// The request's columns as they are in the table (all rows; infinite
    /// values as NaN), so saved columns keep excluded rows' values.
    original: Vec<Vec<f64>>,
    /// Rows used (not excluded), ascending.
    rows: Vec<usize>,
    /// The data of `rows` (NaN missing).
    data: Vec<Vec<f64>>,
    /// Completed data of `rows`.
    completed: Vec<Vec<f64>>,
    patterns: MissingPatterns,
    fit: Fit,
    n_excluded: usize,
    /// Missing values in excluded rows (left as they are).
    n_excluded_missing: usize,
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

fn section(kind: SectionKind, title: &str, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.to_owned(), content }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

impl ImputationReport {
    /// Impute the missing values of the request's columns.
    ///
    /// # Errors
    /// No column, a missing or non-numeric column, and the method's own
    /// errors (a column with too few observed values or no spread, a
    /// singular covariance matrix, a rank that is not below the number of
    /// columns).
    pub fn run(table: &DataTable, states: &RowStateManager, request: &ImputationRequest) -> Result<Self> {
        if request.columns.is_empty() {
            return Err(invalid("choose at least one column"));
        }
        if request.method == ImputationMethod::LowRank && request.columns.len() < 2 {
            return Err(invalid("low-rank imputation needs at least two columns"));
        }
        if request.method.is_multiple() {
            return Self::run_multiple(table, states, request);
        }
        let mut names = Vec::new();
        let mut values = Vec::new();
        for &i in &request.columns {
            let c = table.column(i).ok_or_else(|| invalid(format!("no column {i}")))?;
            values.push(c.as_numeric().ok_or_else(|| invalid(format!("{} must be numeric", c.name)))?);
            names.push(c.name.clone());
        }
        let (mut rows, mut n_excluded, mut n_excluded_missing) = (Vec::new(), 0, 0);
        for r in 0..table.n_rows() {
            if states.is_excluded(r) {
                n_excluded += 1;
                n_excluded_missing += values.iter().filter(|v| !v[r].is_finite()).count();
            } else {
                rows.push(r);
            }
        }
        // Infinite values count as missing too.
        let data: Vec<Vec<f64>> = rows
            .iter()
            .map(|&r| values.iter().map(|v| if v[r].is_finite() { v[r] } else { f64::NAN }).collect())
            .collect();
        let patterns = missing_patterns(&data)?;
        let svd_options = |rank| SvdOptions {
            rank,
            shrinkage: request.shrinkage,
            regularize: request.regularize,
            standardize: request.standardize,
            max_iterations: request.max_iterations,
            tolerance: request.tolerance,
        };
        let (completed, fit) = match request.method {
            ImputationMethod::Mean | ImputationMethod::Median => {
                let center = if request.method == ImputationMethod::Mean { Center::Mean } else { Center::Median };
                let (completed, fills) = impute_center(&data, center)?;
                (completed, Fit::Center(fills))
            }
            ImputationMethod::NormalEm => {
                let options = EmOptions { max_iterations: request.max_iterations, tolerance: request.tolerance };
                let fit = em_normal(&data, &options)?;
                (fit.completed.clone(), Fit::Em(Box::new(fit)))
            }
            ImputationMethod::LowRank => {
                let choice = match request.rank {
                    Some(_) => None,
                    None => Some(choose_rank(
                        &data,
                        ImputationRequest::MAX_CHOSEN_RANK,
                        ImputationRequest::HELD_OUT_SHARE,
                        request.seed,
                        &svd_options(1),
                    )?),
                };
                let rank = request.rank.or(choice.as_ref().map(|c| c.best)).unwrap_or(1);
                let fit = svd_impute(&data, &svd_options(rank))?;
                (fit.completed.clone(), Fit::LowRank { fit, rank, choice })
            }
            ImputationMethod::ChainedEquations | ImputationMethod::NormalModel => {
                return Self::run_multiple(table, states, request);
            }
        };
        Ok(Self {
            request: request.clone(),
            names,
            original: values
                .iter()
                .map(|v| v.iter().map(|&x| if x.is_finite() { x } else { f64::NAN }).collect())
                .collect(),
            rows,
            data,
            completed,
            patterns,
            fit,
            n_excluded,
            n_excluded_missing,
        })
    }

    /// A multiple imputation: the shared missing-value sections use the
    /// first imputation as the "completed" data.
    fn run_multiple(table: &DataTable, states: &RowStateManager, request: &ImputationRequest) -> Result<Self> {
        let run = crate::multiple_imputation::MiRun::run(
            table,
            states,
            &request.columns,
            &request.mi_settings(),
            request.seed,
        )?;
        let n_excluded = table.n_rows() - run.rows.len();
        let mut n_excluded_missing = 0;
        for r in (0..table.n_rows()).filter(|&r| states.is_excluded(r)) {
            n_excluded_missing += request
                .columns
                .iter()
                .filter(|&&c| table.column(c).is_some_and(|col| col.cell_text(r).trim().is_empty()))
                .count();
        }
        let patterns = missing_patterns(&run.data)?;
        let completed = run.result.imputations.first().cloned().unwrap_or_else(|| run.data.clone());
        Ok(Self {
            request: request.clone(),
            names: run.info.iter().map(|c| c.name.clone()).collect(),
            original: Vec::new(),
            rows: run.rows.clone(),
            data: run.data.clone(),
            completed,
            patterns,
            fit: Fit::Multiple(Box::new(run)),
            n_excluded,
            n_excluded_missing,
        })
    }

    /// The multiple imputation, for a report of that kind.
    #[must_use]
    pub fn multiple(&self) -> Option<&crate::multiple_imputation::MiRun> {
        match &self.fit {
            Fit::Multiple(run) => Some(run),
            _ => None,
        }
    }

    /// The table operation that makes this multiple imputation's stacked
    /// table (the same columns, settings and seed, so the table holds the
    /// imputations the report shows). `None` for other methods.
    #[must_use]
    pub fn stacked_operation(&self, table: &DataTable) -> Option<crate::table_ops::TableOperation> {
        self.multiple().and_then(|_| self.request.stacked_operation(table))
    }

    /// The request this report was run with.
    #[must_use]
    pub fn request(&self) -> &ImputationRequest {
        &self.request
    }

    /// The missing-data patterns of the rows used.
    #[must_use]
    pub fn patterns(&self) -> &MissingPatterns {
        &self.patterns
    }

    /// The completed values of column `j` (of the request) for every table
    /// row: observed values as they are, imputed values filled in, NaN in
    /// excluded rows that lack a value.
    #[must_use]
    pub fn completed_column(&self, j: usize) -> Vec<f64> {
        let mut out = self.original[j].clone();
        for (k, &r) in self.rows.iter().enumerate() {
            out[r] = self.completed[k][j];
        }
        out
    }

    /// `(table row, column of the request, imputed value)` of every imputed cell.
    #[must_use]
    pub fn imputed_cells(&self) -> Vec<(usize, usize, f64)> {
        let mut cells = Vec::new();
        for (k, &r) in self.rows.iter().enumerate() {
            for j in 0..self.names.len() {
                if self.data[k][j].is_nan() {
                    cells.push((r, j, self.completed[k][j]));
                }
            }
        }
        cells
    }

    /// Report title, e.g. `"Imputation (Multivariate normal (EM)): x, y"`.
    #[must_use]
    pub fn title(&self) -> String {
        format!("Missing-Value Imputation ({}): {}", self.request.method.label(), self.names.join(", "))
    }

    /// Columns the report can add: `imputed1`, `imputed2`, … (each column
    /// with its missing values filled in) and `count` (imputed values per row).
    #[must_use]
    pub fn saveable_columns(&self) -> Vec<ReportColumn> {
        if self.multiple().is_some() {
            // A multiple imputation is saved as a stacked table instead.
            return Vec::new();
        }
        let mut out = Vec::new();
        for (j, name) in self.names.iter().enumerate() {
            out.push(ReportColumn::continuous(
                &format!("imputed{}", j + 1),
                &format!("Save {name} with imputed values"),
                format!("{name} imputed"),
                self.completed_column(j),
            ));
        }
        let mut count = vec![f64::NAN; self.original.first().map_or(0, Vec::len)];
        for (k, &r) in self.rows.iter().enumerate() {
            count[r] = self.data[k].iter().filter(|x| x.is_nan()).count() as f64;
        }
        let mut flag = ReportColumn::continuous(
            "count",
            "Save number of imputed values per row",
            "Imputed values".to_owned(),
            count,
        );
        flag.level = stat_data::MeasurementLevel::Ordinal;
        out.push(flag);
        out
    }

    /// Report sections: missing values by column, the pattern table and
    /// map, the method's estimates and convergence, the imputed values and
    /// a plot of the first two columns.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let mut out: Vec<ReportSection> = self.warning().into_iter().collect();
        out.extend(self.summary());
        out.push(self.pattern_table());
        if self.patterns.patterns.len() > 1 || self.patterns.n_missing_cells() > 0 {
            out.push(self.pattern_map());
        }
        if let Some(run) = self.multiple() {
            out.extend(run.sections());
            return out;
        }
        out.extend(self.estimates());
        out.push(self.imputed_table());
        if let Some(plot) = self.plot() {
            out.push(plot);
        }
        out
    }

    /// A warning, shown first, when an iterative fit stopped before converging.
    fn warning(&self) -> Option<ReportSection> {
        let (iterations, advice) = match &self.fit {
            Fit::Center(_) | Fit::Multiple(_) => return None,
            Fit::Em(fit) if !fit.converged => {
                (fit.iterations, "Increase the most iterations, or check for columns that are nearly collinear.")
            }
            Fit::LowRank { fit, .. } if !fit.converged => (
                fit.iterations,
                if self.request.shrinkage == 0.0 && !self.request.regularize {
                    "Turn on regularization, lower the rank, or increase the most iterations."
                } else {
                    "Lower the rank, increase the shrinkage, or increase the most iterations."
                },
            ),
            _ => return None,
        };
        Some(section(
            SectionKind::ImputationWarning,
            "Warning: Not Converged",
            SectionContent::Note(format!(
                "⚠ The iteration stopped after {} without settling, so the imputed values are not a converged \
                 fit and may be unreliable. {advice}",
                plural(iterations, "iteration", "iterations")
            )),
        ))
    }

    fn summary(&self) -> Vec<ReportSection> {
        let multiple = self.multiple();
        let after_label = if multiple.is_some() { "Mean over imputations" } else { "Mean after imputation" };
        let mut t = TextTable::new(["Column", "Missing", "% missing", "Observed mean", after_label]);
        let n = self.rows.len();
        for (j, name) in self.names.iter().enumerate() {
            let miss = self.patterns.missing_per_column[j];
            let mean = |x: &[f64]| x.iter().sum::<f64>() / x.len() as f64;
            let categorical = multiple
                .is_some_and(|m| m.info[j].kind != stat_multivariate::multiple_imputation::ColumnKind::Continuous);
            let obs: Vec<f64> =
                if categorical { Vec::new() } else { self.data.iter().map(|r| r[j]).filter(|x| !x.is_nan()).collect() };
            let after: Vec<f64> = match multiple {
                _ if categorical => Vec::new(),
                Some(m) => m.result.imputations.iter().flat_map(|d| d.iter().map(move |r| r[j])).collect(),
                None => self.completed.iter().map(|r| r[j]).collect(),
            };
            t.push([
                name.clone(),
                miss.to_string(),
                format_number(if n == 0 { 0.0 } else { 100.0 * miss as f64 / n as f64 }, 3),
                if obs.is_empty() { String::new() } else { format_number(mean(&obs), 6) },
                if after.is_empty() { String::new() } else { format_number(mean(&after), 6) },
            ]);
        }
        let mut notes = vec![format!(
            "{} of {} have every value; {} missing in all.",
            self.patterns.n_complete(),
            plural(n, "row", "rows"),
            plural(self.patterns.n_missing_cells(), "value is", "values are"),
        )];
        if self.n_excluded > 0 {
            let left = if self.n_excluded_missing > 0 {
                format!(
                    "; their {} left missing",
                    plural(self.n_excluded_missing, "missing value is", "missing values are")
                )
            } else {
                String::new()
            };
            notes.push(format!("{} not used{left}.", plural(self.n_excluded, "excluded row", "excluded rows")));
        }
        notes.push(match self.request.method {
            ImputationMethod::ChainedEquations => {
                let m = self.multiple().map_or(0, crate::multiple_imputation::MiRun::m);
                format!(
                    "{m} completed data sets, each drawn by chained equations: every column with missing values \
                     has its own model given the other columns, redrawn in turn for {} (see Imputation Models). \
                     Analyse the stacked table (Make stacked table in the options menu) and pool the results \
                     with Rubin's rules; values are assumed missing at random.",
                    plural(self.request.multiple.iterations, "iteration", "iterations")
                )
            }
            ImputationMethod::NormalModel => {
                let m = self.multiple().map_or(0, crate::multiple_imputation::MiRun::m);
                format!(
                    "{m} completed data sets drawn from a multivariate normal model by data augmentation \
                     ({} per imputation, starting from the maximum-likelihood fit). Analyse the stacked table \
                     (Make stacked table in the options menu) and pool the results with Rubin's rules; values \
                     are assumed missing at random.",
                    plural(self.request.multiple.iterations, "iteration", "iterations")
                )
            }
            ImputationMethod::Mean | ImputationMethod::Median => {
                "Filling in a column's centre keeps its mean (or median) but understates its spread and its \
                 correlations with other columns."
                    .to_owned()
            }
            ImputationMethod::NormalEm => "Imputed values are conditional means given each row's observed values, \
                 under a multivariate normal distribution fitted by maximum likelihood (EM algorithm); values are \
                 assumed missing at random."
                .to_owned(),
            ImputationMethod::LowRank => {
                let shrink = if self.request.shrinkage > 0.0 {
                    " Its singular values are soft-thresholded by λ (soft-impute)."
                } else if self.request.regularize {
                    " Its singular values are shrunk by the estimated noise variance (regularized iterative \
                     PCA), which keeps the refitting from drifting."
                } else {
                    " Its singular values are not shrunk, so with more than one component the refitting can \
                     drift."
                };
                format!(
                    "Imputed values come from column means plus a low-rank fit of the (standardized) data, \
                     refitted until the imputed values stop changing.{shrink}"
                )
            }
        });
        vec![
            section(SectionKind::MissingSummary, "Missing Values", SectionContent::Table(t)),
            section(SectionKind::ImputationNote, "About These Results", SectionContent::Note(notes.join(" "))),
        ]
    }

    fn pattern_label(&self, missing: &[bool]) -> String {
        let names: Vec<&str> = self.names.iter().zip(missing).filter(|(_, m)| **m).map(|(n, _)| n.as_str()).collect();
        if names.is_empty() { "(none)".to_owned() } else { names.join(", ") }
    }

    fn pattern_table(&self) -> ReportSection {
        let mut t = TextTable::new(["Pattern", "Rows", "% of rows", "Missing columns", "Number missing"]);
        let n = self.rows.len().max(1);
        for (k, p) in self.patterns.patterns.iter().enumerate() {
            t.push([
                (k + 1).to_string(),
                p.rows.len().to_string(),
                format_number(100.0 * p.rows.len() as f64 / n as f64, 3),
                self.pattern_label(&p.missing),
                p.n_missing().to_string(),
            ]);
        }
        let monotone = if self.patterns.is_monotone() { "monotone" } else { "not monotone" };
        section(SectionKind::MissingPatterns, &format!("Missing-Data Patterns ({monotone})"), SectionContent::Table(t))
    }

    fn pattern_map(&self) -> ReportSection {
        let shown: Vec<_> = self.patterns.patterns.iter().take(MAPPED_PATTERNS).collect();
        let rows = shown.iter().enumerate().map(|(k, p)| format!("{}: {} rows", k + 1, p.rows.len())).collect();
        let values: Vec<Vec<f64>> =
            shown.iter().map(|p| p.missing.iter().map(|&m| if m { 1.0 } else { 0.0 }).collect()).collect();
        let text = shown
            .iter()
            .map(|p| p.missing.iter().map(|&m| if m { "missing".to_owned() } else { String::new() }).collect())
            .collect();
        let spec =
            HeatMapSpec { rows, columns: self.names.clone(), values, text, measure: "Missing".to_owned(), limit: 1.0 };
        let title = if self.patterns.patterns.len() > MAPPED_PATTERNS {
            format!("Pattern Map (the {MAPPED_PATTERNS} most frequent patterns)")
        } else {
            "Pattern Map".to_owned()
        };
        section(SectionKind::MissingPatternMap, &title, SectionContent::HeatMap(spec))
    }

    fn estimates(&self) -> Vec<ReportSection> {
        match &self.fit {
            Fit::Center(fills) => {
                let what = if self.request.method == ImputationMethod::Mean { "Mean" } else { "Median" };
                let mut t = TextTable::new(["Column", &format!("{what} filled in")]);
                for (name, v) in self.names.iter().zip(fills) {
                    t.push([name.clone(), format_number(*v, 6)]);
                }
                vec![section(SectionKind::ImputationEstimates, "Values Filled In", SectionContent::Table(t))]
            }
            Fit::Em(fit) => {
                let mut out = Vec::new();
                let p = self.names.len();
                let mut header = vec!["Column".to_owned(), "Mean".to_owned(), "Std Dev".to_owned()];
                header.extend(self.names.iter().map(|n| format!("r with {n}")));
                let mut t = TextTable::new(header);
                let sd: Vec<f64> = (0..p).map(|j| fit.covariance[j][j].sqrt()).collect();
                for j in 0..p {
                    let mut row = vec![self.names[j].clone(), format_number(fit.mean[j], 6), format_number(sd[j], 6)];
                    row.extend((0..p).map(|k| format_number(fit.covariance[j][k] / (sd[j] * sd[k]), 4)));
                    t.push(row);
                }
                let est = section(
                    SectionKind::ImputationEstimates,
                    "Estimated Means, Standard Deviations and Correlations (maximum likelihood)",
                    SectionContent::Table(t),
                );
                let mut c = TextTable::new(["Statistic", "Value"]);
                c.push(["Iterations".to_owned(), fit.iterations.to_string()]);
                c.push(["Converged".to_owned(), if fit.converged { "Yes" } else { "No" }.to_owned()]);
                c.push(["Last change".to_owned(), format_number(fit.change.last().copied().unwrap_or(0.0), 3)]);
                c.push([
                    "Log-likelihood".to_owned(),
                    format_number(fit.log_likelihood.last().copied().unwrap_or(f64::NAN), 8),
                ]);
                c.push(["Rows used".to_owned(), fit.n_used.to_string()]);
                let line: Vec<[f64; 2]> = fit.log_likelihood.iter().enumerate().map(|(i, &l)| [i as f64, l]).collect();
                out.extend([
                    est,
                    section(SectionKind::ImputationConvergence, "Convergence", SectionContent::Table(c)),
                    convergence_plot("Log-likelihood", line),
                ]);
                out
            }
            Fit::Multiple(_) => Vec::new(),
            Fit::LowRank { fit, rank, choice } => {
                let mut out = Vec::new();
                if let Some(choice) = choice {
                    let mut t = TextTable::new(["Rank", "Held-out RMSE (SD units)", "Chosen"]);
                    for (r, e) in choice.ranks.iter().zip(&choice.rmse) {
                        t.push([
                            r.to_string(),
                            format_number(*e, 4),
                            if *r == choice.best { "✓ chosen" } else { "" }.to_owned(),
                        ]);
                    }
                    out.push(section(
                        SectionKind::ImputationRank,
                        &format!("Rank Choice ({} observed values held out)", choice.n_held_out),
                        SectionContent::Table(t),
                    ));
                }
                let mut c = TextTable::new(["Statistic", "Value"]);
                c.push(["Rank".to_owned(), rank.to_string()]);
                if self.request.shrinkage > 0.0 {
                    c.push(["Shrinkage λ".to_owned(), format_number(self.request.shrinkage, 6)]);
                } else {
                    c.push(["Regularized".to_owned(), if self.request.regularize { "Yes" } else { "No" }.to_owned()]);
                }
                if let Some(s2) = fit.noise_variance {
                    c.push(["Noise variance per value (σ̂²)".to_owned(), format_number(s2, 6)]);
                }
                c.push([
                    "Columns standardized".to_owned(),
                    if self.request.standardize { "Yes" } else { "No" }.to_owned(),
                ]);
                c.push(["Iterations".to_owned(), fit.iterations.to_string()]);
                c.push(["Converged".to_owned(), if fit.converged { "Yes" } else { "No" }.to_owned()]);
                c.push([
                    "Last relative change".to_owned(),
                    format_number(fit.change.last().copied().unwrap_or(0.0), 3),
                ]);
                let total: f64 = fit.singular_values.iter().map(|s| s * s).sum();
                let kept: f64 = fit.singular_values.iter().take(*rank).map(|s| s * s).sum();
                if total > 0.0 {
                    c.push([
                        "Variation in the kept components".to_owned(),
                        format!("{}%", format_number(100.0 * kept / total, 4)),
                    ]);
                }
                out.push(section(SectionKind::ImputationConvergence, "Convergence", SectionContent::Table(c)));
                let line: Vec<[f64; 2]> = fit
                    .change
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| **c > 0.0)
                    .map(|(i, &c)| [(i + 1) as f64, c.log10()])
                    .collect();
                out.push(convergence_plot("log₁₀ relative change", line));
                out
            }
        }
    }

    fn imputed_table(&self) -> ReportSection {
        let cells = self.imputed_cells();
        let mut t = TextTable::new(["Row", "Column", "Imputed value"]);
        for &(r, j, v) in cells.iter().take(LISTED_VALUES) {
            t.push([(r + 1).to_string(), self.names[j].clone(), format_number(v, 6)]);
        }
        let title = if cells.len() > LISTED_VALUES {
            format!("Imputed Values (first {LISTED_VALUES} of {})", cells.len())
        } else {
            format!("Imputed Values ({})", cells.len())
        };
        section(SectionKind::ImputedValues, &title, SectionContent::Table(t))
    }

    fn plot(&self) -> Option<ReportSection> {
        if self.names.len() < 2 || self.rows.is_empty() {
            return None;
        }
        let mut points = Vec::new();
        let mut membership = Vec::new();
        for (k, &r) in self.rows.iter().enumerate() {
            let (x, y) = (self.completed[k][0], self.completed[k][1]);
            if !(x.is_finite() && y.is_finite()) {
                continue;
            }
            points.push(RowPoint { row: r, x, y });
            let group = match (self.data[k][0].is_nan(), self.data[k][1].is_nan()) {
                (false, false) => 0,
                (true, false) => 1,
                (false, true) => 2,
                (true, true) => 3,
            };
            membership.push(group);
        }
        let (a, b) = (&self.names[0], &self.names[1]);
        let spec = PlotSpec {
            x_label: a.clone(),
            y_label: b.clone(),
            points,
            groups: Some(PointGroups {
                title: "Values".to_owned(),
                labels: vec![
                    "Both observed".to_owned(),
                    format!("{a} imputed"),
                    format!("{b} imputed"),
                    "Both imputed".to_owned(),
                ],
                membership,
                lines: Vec::new(),
            }),
            ..PlotSpec::default()
        };
        Some(section(
            SectionKind::ImputedScatter,
            &format!("{b} by {a}, Imputed Values Marked"),
            SectionContent::Plot(Box::new(spec)),
        ))
    }
}

fn convergence_plot(what: &str, line: Vec<[f64; 2]>) -> ReportSection {
    let spec = PlotSpec {
        x_label: "Iteration".to_owned(),
        y_label: what.to_owned(),
        line: Some(line),
        line_name: Some(what.to_owned()),
        ..PlotSpec::default()
    };
    section(SectionKind::ImputationConvergencePlot, "Convergence Plot", SectionContent::Plot(Box::new(spec)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    fn table() -> DataTable {
        let nan = f64::NAN;
        DataTable::new(
            "t",
            vec![
                Column::numeric("x", vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]),
                Column::numeric("y", vec![2.1, nan, 6.2, 7.9, nan, 12.1, 13.8, 16.2]),
                Column::numeric("z", vec![1.0, 0.5, nan, 2.0, 2.5, 2.0, 3.5, 3.0]),
                Column::text("g", vec!["a".into(); 8]),
            ],
        )
        .unwrap()
    }

    #[test]
    fn em_report_fills_every_missing_value() {
        let t = table();
        let rows = RowStateManager::new(t.n_rows());
        let r = ImputationReport::run(&t, &rows, &ImputationRequest::new(vec![0, 1, 2])).unwrap();
        let cells = r.imputed_cells();
        assert_eq!(cells.iter().map(|c| (c.0, c.1)).collect::<Vec<_>>(), vec![(1, 1), (2, 2), (4, 1)]);
        // y is close to 2x, so its imputed values follow x.
        assert!((cells[0].2 - 4.0).abs() < 0.5, "{cells:?}");
        let cols = r.saveable_columns();
        assert_eq!(cols.len(), 4);
        assert_eq!(cols[1].name, "y imputed");
        assert!(cols[1].values.iter().all(|v| v.is_finite()));
        assert_eq!(cols[3].values[1], 1.0);
        assert_eq!(cols[3].values[0], 0.0);
        let kinds: Vec<SectionKind> = r.sections().iter().map(|s| s.kind).collect();
        assert!(kinds.contains(&SectionKind::MissingPatternMap));
        assert!(kinds.contains(&SectionKind::ImputedScatter));
        assert!(kinds.contains(&SectionKind::ImputationConvergencePlot));
    }

    #[test]
    fn excluded_rows_are_left_alone() {
        let t = table();
        let mut rows = RowStateManager::new(t.n_rows());
        rows.set_excluded(vec![1], true);
        let req = ImputationRequest { method: ImputationMethod::Mean, ..ImputationRequest::new(vec![1]) };
        let r = ImputationReport::run(&t, &rows, &req).unwrap();
        let col = &r.saveable_columns()[0];
        assert!(col.values[1].is_nan(), "excluded row keeps its missing value");
        assert!(col.values[4].is_finite());
        assert_eq!(col.values[0], 2.1);
    }

    #[test]
    fn low_rank_chooses_a_rank_and_round_trips_json() {
        let t = table();
        let rows = RowStateManager::new(t.n_rows());
        let req = ImputationRequest { method: ImputationMethod::LowRank, ..ImputationRequest::new(vec![0, 1, 2]) };
        let r = ImputationReport::run(&t, &rows, &req).unwrap();
        assert!(r.sections().iter().any(|s| s.kind == SectionKind::ImputationRank));
        let back = ImputationRequest::from_json(&req.to_json(&t), &t).unwrap();
        assert_eq!(back, req);
        let fixed = ImputationRequest { rank: Some(1), shrinkage: 0.5, ..req };
        assert_eq!(ImputationRequest::from_json(&fixed.to_json(&t), &t).unwrap(), fixed);
    }

    /// A 40 × 4 standard-normal table with 20% of its values missing (`WILD`
    /// of `stat-multivariate/tests/data/imputation_reference.py`), on which the
    /// unregularized rank-2 iteration drifts.
    const WILD: [[f64; 4]; 40] = [
        [2.04, -2.56, f64::NAN, f64::NAN],
        [-0.45, -0.22, -2.02, -0.23],
        [-0.87, 3.32, 0.23, -0.35],
        [-0.28, -0.67, -1.06, -0.39],
        [0.48, f64::NAN, 0.96, -0.2],
        [0.02, 1.55, f64::NAN, -0.51],
        [-0.18, 0.54, 1.94, -0.27],
        [-0.24, 1.0, -0.89, -0.29],
        [0.88, 0.58, 0.09, 0.67],
        [-2.83, 1.02, f64::NAN, -1.67],
        [0.28, 0.7, -0.44, -1.08],
        [0.03, -0.05, f64::NAN, 0.75],
        [0.19, 1.11, f64::NAN, f64::NAN],
        [0.58, 0.58, -0.21, -0.78],
        [0.23, f64::NAN, 0.69, 0.49],
        [-1.64, f64::NAN, -0.96, 0.76],
        [-2.03, -0.91, 0.71, 1.16],
        [-2.16, -0.5, f64::NAN, -0.61],
        [1.59, -1.19, f64::NAN, -1.05],
        [1.41, -0.02, -0.37, -1.72],
        [1.68, 0.75, 0.75, 1.14],
        [0.35, -0.64, -0.8, -0.8],
        [1.37, -1.46, -0.6, -0.32],
        [f64::NAN, f64::NAN, f64::NAN, -1.73],
        [-0.0, 1.21, 0.76, f64::NAN],
        [-0.32, 0.29, -0.24, 0.82],
        [-0.79, 0.13, -0.11, 0.54],
        [0.22, 2.55, 1.5, 1.5],
        [f64::NAN, -0.34, -0.61, 0.53],
        [f64::NAN, 1.17, 1.07, f64::NAN],
        [f64::NAN, -0.8, 0.04, 0.64],
        [2.05, -0.2, 0.77, 0.16],
        [f64::NAN, 0.74, 1.37, -1.08],
        [-0.19, -0.81, 1.5, 0.66],
        [-0.31, -0.45, f64::NAN, -0.7],
        [f64::NAN, 0.48, 2.46, -0.25],
        [f64::NAN, -1.17, -1.34, 0.52],
        [0.85, 0.01, f64::NAN, 0.12],
        [f64::NAN, -1.53, -0.46, 0.11],
        [f64::NAN, -0.48, -0.82, -0.33],
    ];

    fn wild_table() -> DataTable {
        let cols = (0..4).map(|j| Column::numeric(format!("x{j}"), WILD.iter().map(|r| r[j]).collect())).collect();
        DataTable::new("w", cols).unwrap()
    }

    #[test]
    fn low_rank_is_regularized_by_default_and_warns_when_it_does_not_settle() {
        let t = wild_table();
        let rows = RowStateManager::new(t.n_rows());
        let req = ImputationRequest {
            method: ImputationMethod::LowRank,
            rank: Some(2),
            ..ImputationRequest::new(vec![0, 1, 2, 3])
        };
        assert!(req.regularize);
        let r = ImputationReport::run(&t, &rows, &req).unwrap();
        let Fit::LowRank { fit, .. } = &r.fit else { panic!() };
        assert!(fit.converged);
        assert!(r.sections().iter().all(|s| s.kind != SectionKind::ImputationWarning));
        let worst = r.imputed_cells().iter().map(|c| c.2.abs()).fold(0.0, f64::max);
        assert!(worst < 5.0, "imputed values stay within the data's range ({worst})");
        // Old behaviour: no shrinkage; the warning comes first.
        let plain = ImputationRequest { regularize: false, ..req.clone() };
        let r = ImputationReport::run(&t, &rows, &plain).unwrap();
        let Fit::LowRank { fit, .. } = &r.fit else { panic!() };
        assert!(!fit.converged);
        let first = &r.sections()[0];
        assert_eq!(first.kind, SectionKind::ImputationWarning);
        assert!(matches!(&first.content, SectionContent::Note(n) if n.contains("Turn on regularization")));
        // Too few iterations to settle: warned even when regularized.
        let short = ImputationRequest { max_iterations: 2, ..req };
        let r = ImputationReport::run(&t, &rows, &short).unwrap();
        assert_eq!(r.sections()[0].kind, SectionKind::ImputationWarning);
        assert_eq!(ImputationRequest::from_json(&plain.to_json(&t), &t).unwrap(), plain);
    }

    #[test]
    fn text_columns_are_refused() {
        let t = table();
        let rows = RowStateManager::new(t.n_rows());
        assert!(ImputationReport::run(&t, &rows, &ImputationRequest::new(vec![3])).is_err());
        assert!(ImputationReport::run(&t, &rows, &ImputationRequest::new(vec![])).is_err());
    }
}
