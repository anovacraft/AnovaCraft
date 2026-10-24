//! Reports pooled across multiply imputed data sets.
//!
//! A stacked imputation table ([`crate::multiple_imputation`]) holds the
//! original rows (imputation 0) and `m` completed copies. A report on it
//! whose scope pools by the imputation column ([`ReportScope::pool`]) is
//! fitted to each completed copy separately and the results are combined
//! by Rubin's rules ([`stat_inference::pooling`]): pooled estimates with
//! Barnard–Rubin degrees of freedom and their within/between variance
//! split, multi-parameter tests (D₁; for linear models also D₃), pooled R²
//! for linear models, and each imputation's own estimates. The report's
//! usual sections follow, computed from the original rows only
//! (imputation 0), which is the complete-case analysis.
//!
//! Supported reports and what is pooled:
//!
//! * linear models: coefficients (with one design, so centred terms and
//!   category levels mean the same in every imputation), D₁ and D₃ tests of
//!   each effect and of the whole model, R² and adjusted R² on Fisher's z
//!   scale (Harel 2009); complete-data degrees of freedom `n − p`;
//! * generalized linear and mixed models: coefficients and D₁ tests of each
//!   term, with `ν_com = n − p` (as R mice uses for these models);
//! * one-sample tests: the mean (`ν_com = n − 1`); two independent groups:
//!   the difference in means with the pooled variance (`ν_com = n₁ + n₂ − 2`),
//!   and with more groups each group's mean; two paired columns: the mean
//!   difference (`ν_com = n − 1`);
//! * correlations: each Pearson correlation on Fisher's z scale.
//!
//! [`ReportScope::pool`]: crate::report_scope::ReportScope::pool

use stat_core::{Matrix, Result, StatError};
use stat_data::{DataTable, RowStateManager, natural_order};
use stat_inference::pooling::{
    PooledEstimate, PooledTest, pool_correlation, pool_likelihood_ratio, pool_r_squared, pool_scalar, pool_wald,
};
use stat_linear::{OlsOptions, fit_linear_model};

use crate::compare::{GroupLayout, PairedLayout};
use crate::format::format_number;
use crate::linear_analysis::{Coded, borrow_predictors};
use crate::project::SavedReport;
use crate::report::{ReportSection, SectionContent, SectionKind, TextTable};
use crate::report_scope::apply_mask;

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

fn section(kind: SectionKind, title: &str, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.to_owned(), content }
}

/// Most terms listed by imputation in the drill-down table.
pub const TERMS_BY_IMPUTATION: usize = 12;

/// Whether reports like `saved` can be pooled across imputations.
#[must_use]
pub fn can_pool(saved: &SavedReport) -> bool {
    matches!(
        saved.unscoped(),
        SavedReport::Linear { .. }
            | SavedReport::Glm { .. }
            | SavedReport::Mixed { .. }
            | SavedReport::OneSample { .. }
            | SavedReport::Groups { .. }
            | SavedReport::Paired { .. }
            | SavedReport::Correlations { .. }
    )
}

/// The imputation numbers (1 and up) among the rows `rows` includes.
fn imputation_numbers(table: &DataTable, column: usize, rows: &RowStateManager) -> Vec<u64> {
    let Some(v) = table.column(column).and_then(|c| c.as_numeric()) else { return Vec::new() };
    let mut out: Vec<u64> = (0..table.n_rows())
        .filter(|&r| !rows.is_excluded(r) && v[r] >= 1.0 && v[r].fract() == 0.0)
        .map(|r| v[r] as u64)
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// `rows` with every row outside imputation `k` excluded too.
fn rows_of(table: &DataTable, column: usize, k: u64, rows: &RowStateManager) -> RowStateManager {
    let v = table.column(column).and_then(|c| c.as_numeric()).unwrap_or(&[]);
    let keep: Vec<bool> = (0..table.n_rows()).map(|r| v.get(r).is_some_and(|&x| x == k as f64)).collect();
    apply_mask(&keep, rows)
}

/// One imputation's fitted parameters.
#[derive(Debug, Clone)]
struct Fitted {
    /// Estimates.
    values: Vec<f64>,
    /// Their covariance.
    covariance: Vec<Vec<f64>>,
    /// Complete-data degrees of freedom.
    df_complete: f64,
    /// Rows used.
    n: usize,
    /// R² and adjusted R² (linear models).
    r_squared: Option<(f64, f64)>,
}

fn matrix_rows(m: &Matrix) -> Vec<Vec<f64>> {
    (0..m.nrows()).map(|i| m.row(i)).collect()
}

fn diagonal(values: &[f64]) -> Vec<Vec<f64>> {
    (0..values.len()).map(|i| (0..values.len()).map(|j| if i == j { values[i] } else { 0.0 }).collect()).collect()
}

/// Each imputation's likelihood-ratio statistic for D₃: with its own
/// estimates, and at the pooled estimates.
type LrStatistics = (Vec<f64>, Vec<f64>);

/// A least-squares fit: coefficients, covariance, residual sum of squares,
/// R² and adjusted R².
type OlsFit = (Vec<f64>, Vec<Vec<f64>>, f64, f64, f64);

/// What every imputation gave, ready to pool.
struct Pieces {
    /// What the estimates are ("Term", "Pair", "Group"…).
    label: &'static str,
    names: Vec<String>,
    /// Null value of each estimate for its t test.
    nulls: Vec<f64>,
    /// Imputation numbers, parallel to `fits`.
    numbers: Vec<u64>,
    fits: Vec<Fitted>,
    /// Effects as parameter positions, for D₁ tests (models).
    effects: Vec<(String, Vec<usize>)>,
    /// D₃ statistics `(own, at pooled)` per effect, parallel to `effects`,
    /// and for the whole model (linear models).
    likelihood_ratio: Option<(Vec<LrStatistics>, LrStatistics)>,
    /// Pooled on Fisher's z scale (correlations).
    correlations: bool,
}

/// The pooled sections for `report` on `table`, whose column `column`
/// numbers the imputations; `rows` are the table's row states narrowed by
/// the report's own row filter. Failures become a note.
#[must_use]
pub fn pooled_sections(
    report: &SavedReport,
    table: &DataTable,
    rows: &RowStateManager,
    column: usize,
) -> Vec<ReportSection> {
    match pieces(report.unscoped(), table, rows, column).and_then(|p| sections(&p)) {
        Ok(s) => s,
        Err(e) => vec![section(
            SectionKind::PooledNote,
            "Pooled Across Imputations",
            SectionContent::Note(format!("⚠ The analysis could not be pooled across imputations: {e}")),
        )],
    }
}

/// How many imputations a pooled report on `table` combines.
#[must_use]
pub fn imputation_count(table: &DataTable, column: usize, rows: &RowStateManager) -> usize {
    imputation_numbers(table, column, rows).len()
}

fn pieces(report: &SavedReport, table: &DataTable, rows: &RowStateManager, column: usize) -> Result<Pieces> {
    let numbers = imputation_numbers(table, column, rows);
    if numbers.len() < 2 {
        return Err(invalid("the table needs at least two imputations (Imputation values 1, 2, …)"));
    }
    let per: Vec<RowStateManager> = numbers.iter().map(|&k| rows_of(table, column, k, rows)).collect();
    let mut p = match report {
        SavedReport::Linear { spec, .. } => linear(table, rows, column, &numbers, spec)?,
        SavedReport::Glm { request, .. } => {
            let mut names = Vec::new();
            let mut effects = Vec::new();
            let fits = per
                .iter()
                .map(|r| {
                    let a = crate::GlmAnalysis::run(table, r, request)?;
                    let fit = a.fit();
                    names = fit.coefficients.iter().map(|c| c.name.clone()).collect();
                    effects = a.effect_columns();
                    Ok(Fitted {
                        values: fit.coefficients.iter().map(|c| c.estimate).collect(),
                        covariance: matrix_rows(&fit.covariance),
                        df_complete: fit.df_residual.max(1) as f64,
                        n: fit.fitted.len(),
                        r_squared: None,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            Pieces {
                label: "Term",
                nulls: vec![0.0; names.len()],
                names,
                numbers: Vec::new(),
                fits,
                effects,
                likelihood_ratio: None,
                correlations: false,
            }
        }
        SavedReport::Mixed { request, .. } => {
            let mut names = Vec::new();
            let mut effects = Vec::new();
            let fits = per
                .iter()
                .map(|r| {
                    let a = crate::MixedAnalysis::run(table, r, request)?;
                    let fit = a.fit();
                    names = fit.fixed.iter().map(|c| c.name.clone()).collect();
                    effects = a.effect_columns();
                    Ok(Fitted {
                        values: fit.fixed.iter().map(|c| c.estimate).collect(),
                        covariance: matrix_rows(&fit.fixed_covariance),
                        df_complete: (fit.n_obs.saturating_sub(fit.fixed.len())).max(1) as f64,
                        n: fit.n_obs,
                        r_squared: None,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            Pieces {
                label: "Term",
                nulls: vec![0.0; names.len()],
                names,
                numbers: Vec::new(),
                fits,
                effects,
                likelihood_ratio: None,
                correlations: false,
            }
        }
        SavedReport::OneSample { request } => {
            if request.group.is_some() {
                return Err(invalid(
                    "one-sample tests by group are not pooled; filter the report to one group instead",
                ));
            }
            let name = column_name(table, request.column)?;
            let fits = per.iter().map(|r| mean_fit(&values(table, request.column, r)?)).collect::<Result<Vec<_>>>()?;
            Pieces {
                label: "Estimate",
                names: vec![format!("Mean of {name}")],
                nulls: vec![request.null_mean],
                numbers: Vec::new(),
                fits,
                effects: Vec::new(),
                likelihood_ratio: None,
                correlations: false,
            }
        }
        SavedReport::Groups { request } => groups(table, &per, &request.layout, request.null_difference)?,
        SavedReport::Paired { request } => {
            let PairedLayout::Columns(cols) = &request.layout else {
                return Err(invalid("paired comparisons are pooled for two paired columns only"));
            };
            let [a, b] = cols.as_slice() else {
                return Err(invalid("paired comparisons are pooled for two paired columns only"));
            };
            let names = format!("{} − {}", column_name(table, *a)?, column_name(table, *b)?);
            let fits = per
                .iter()
                .map(|r| {
                    let (x, y) = (column_values(table, *a)?, column_values(table, *b)?);
                    let d: Vec<f64> = (0..table.n_rows())
                        .filter(|&i| !r.is_excluded(i) && x[i].is_finite() && y[i].is_finite())
                        .map(|i| x[i] - y[i])
                        .collect();
                    mean_fit(&d)
                })
                .collect::<Result<Vec<_>>>()?;
            Pieces {
                label: "Estimate",
                names: vec![format!("Mean difference {names}")],
                nulls: vec![request.null_difference],
                numbers: Vec::new(),
                fits,
                effects: Vec::new(),
                likelihood_ratio: None,
                correlations: false,
            }
        }
        SavedReport::Correlations { request } => {
            let cols = &request.columns;
            if cols.len() < 2 {
                return Err(invalid("choose at least two columns"));
            }
            let mut names = Vec::new();
            for (i, &a) in cols.iter().enumerate() {
                for &b in &cols[i + 1..] {
                    names.push(format!("{} with {}", column_name(table, a)?, column_name(table, b)?));
                }
            }
            let fits = per
                .iter()
                .map(|r| {
                    let mut values = Vec::new();
                    let mut ns = Vec::new();
                    for (i, &a) in cols.iter().enumerate() {
                        for &b in &cols[i + 1..] {
                            let (x, y) = (column_values(table, a)?, column_values(table, b)?);
                            let rows: Vec<usize> = (0..table.n_rows())
                                .filter(|&k| !r.is_excluded(k) && x[k].is_finite() && y[k].is_finite())
                                .collect();
                            values.push(pearson(
                                &rows.iter().map(|&k| x[k]).collect::<Vec<_>>(),
                                &rows.iter().map(|&k| y[k]).collect::<Vec<_>>(),
                            ));
                            ns.push(rows.len());
                        }
                    }
                    let n = ns.iter().copied().min().unwrap_or(0);
                    Ok(Fitted {
                        covariance: diagonal(&ns.iter().map(|&n| 1.0 / (n as f64 - 3.0)).collect::<Vec<_>>()),
                        values,
                        df_complete: f64::INFINITY,
                        n,
                        r_squared: None,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            Pieces {
                label: "Pair",
                nulls: vec![0.0; names.len()],
                names,
                numbers: Vec::new(),
                fits,
                effects: Vec::new(),
                likelihood_ratio: None,
                correlations: true,
            }
        }
        _ => return Err(invalid("this kind of report cannot be pooled across imputations")),
    };
    p.numbers = numbers;
    if p.fits.iter().any(|f| f.values.len() != p.names.len()) {
        return Err(invalid("the imputations gave models with different terms (a category is missing from one)"));
    }
    Ok(p)
}

fn column_name(table: &DataTable, c: usize) -> Result<String> {
    table.column(c).map(|c| c.name.clone()).ok_or_else(|| invalid(format!("no column {c}")))
}

fn column_values(table: &DataTable, c: usize) -> Result<&[f64]> {
    let col = table.column(c).ok_or_else(|| invalid(format!("no column {c}")))?;
    col.as_numeric().ok_or_else(|| invalid(format!("{} must be numeric", col.name)))
}

/// The finite values of column `c` in the rows `rows` includes.
fn values(table: &DataTable, c: usize, rows: &RowStateManager) -> Result<Vec<f64>> {
    let v = column_values(table, c)?;
    Ok((0..table.n_rows()).filter(|&r| !rows.is_excluded(r) && v[r].is_finite()).map(|r| v[r]).collect())
}

fn mean_var(v: &[f64]) -> (f64, f64) {
    let n = v.len() as f64;
    let mean = v.iter().sum::<f64>() / n;
    (mean, v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1.0))
}

/// The mean with variance `s²/n` and `ν_com = n − 1`.
fn mean_fit(v: &[f64]) -> Result<Fitted> {
    if v.len() < 2 {
        return Err(invalid("each imputation needs at least two values"));
    }
    let (mean, var) = mean_var(v);
    Ok(Fitted {
        values: vec![mean],
        covariance: vec![vec![var / v.len() as f64]],
        df_complete: (v.len() - 1) as f64,
        n: v.len(),
        r_squared: None,
    })
}

fn pearson(x: &[f64], y: &[f64]) -> f64 {
    let (mx, vx) = mean_var(x);
    let (my, vy) = mean_var(y);
    let n = x.len() as f64;
    let sxy = x.iter().zip(y).map(|(a, b)| (a - mx) * (b - my)).sum::<f64>() / (n - 1.0);
    sxy / (vx * vy).sqrt()
}

/// Two groups: the difference in means with the pooled variance; more
/// groups: each group's mean.
fn groups(table: &DataTable, per: &[RowStateManager], layout: &GroupLayout, null: f64) -> Result<Pieces> {
    // Each imputation's values by group, groups in natural order.
    let by_group = |r: &RowStateManager| -> Result<(Vec<String>, Vec<Vec<f64>>)> {
        match layout {
            GroupLayout::Columns(cols) => {
                let names = cols.iter().map(|&c| column_name(table, c)).collect::<Result<Vec<_>>>()?;
                let vals = cols.iter().map(|&c| values(table, c, r)).collect::<Result<Vec<_>>>()?;
                Ok((names, vals))
            }
            GroupLayout::Stacked { response, group } => {
                let y = column_values(table, *response)?;
                let g = table.column(*group).ok_or_else(|| invalid(format!("no column {group}")))?;
                let mut levels: Vec<String> =
                    (0..table.n_rows()).map(|k| g.level_text(k)).filter(|t| !t.is_empty()).collect();
                levels.sort_by(|a, b| natural_order(a, b));
                levels.dedup();
                let vals = levels
                    .iter()
                    .map(|l| {
                        (0..table.n_rows())
                            .filter(|&k| !r.is_excluded(k) && y[k].is_finite() && g.level_text(k) == *l)
                            .map(|k| y[k])
                            .collect()
                    })
                    .collect();
                Ok((levels, vals))
            }
        }
    };
    let mut names = Vec::new();
    let mut two = false;
    let fits = per
        .iter()
        .map(|r| {
            let (levels, vals) = by_group(r)?;
            if vals.iter().any(|v| v.len() < 2) {
                return Err(invalid("each group needs at least two values in every imputation"));
            }
            if levels.len() == 2 {
                two = true;
                names = vec![format!("Difference {} − {}", levels[0], levels[1])];
                let ((m1, v1), (m2, v2)) = (mean_var(&vals[0]), mean_var(&vals[1]));
                let (n1, n2) = (vals[0].len() as f64, vals[1].len() as f64);
                let sp2 = ((n1 - 1.0) * v1 + (n2 - 1.0) * v2) / (n1 + n2 - 2.0);
                Ok(Fitted {
                    values: vec![m1 - m2],
                    covariance: vec![vec![sp2 * (1.0 / n1 + 1.0 / n2)]],
                    df_complete: n1 + n2 - 2.0,
                    n: vals[0].len() + vals[1].len(),
                    r_squared: None,
                })
            } else {
                names = levels.iter().map(|l| format!("Mean of {l}")).collect();
                let fits: Vec<Fitted> = vals.iter().map(|v| mean_fit(v)).collect::<Result<_>>()?;
                // Each mean keeps its own ν_com; the smallest stands for all.
                Ok(Fitted {
                    values: fits.iter().map(|f| f.values[0]).collect(),
                    covariance: diagonal(&fits.iter().map(|f| f.covariance[0][0]).collect::<Vec<_>>()),
                    df_complete: fits.iter().map(|f| f.df_complete).fold(f64::INFINITY, f64::min),
                    n: fits.iter().map(|f| f.n).sum(),
                    r_squared: None,
                })
            }
        })
        .collect::<Result<Vec<_>>>()?;
    let nulls = if two { vec![null] } else { vec![0.0; names.len()] };
    Ok(Pieces {
        label: if two { "Estimate" } else { "Group" },
        names,
        nulls,
        numbers: Vec::new(),
        fits,
        effects: Vec::new(),
        likelihood_ratio: None,
        correlations: false,
    })
}

/// Gaussian log-likelihood at the residual sum of squares `rss` of `n` rows
/// (variance at its maximum-likelihood value `rss/n`).
fn normal_log_likelihood(rss: f64, n: usize) -> f64 {
    let n = n as f64;
    -0.5 * n * ((2.0 * std::f64::consts::PI).ln() + (rss / n).ln() + 1.0)
}

/// Residual sum of squares of `y` about `offset` after refitting only an
/// intercept, as R's `lm(y ~ 1, offset = …)` does in mice's D₃.
fn offset_rss(y: &[f64], offset: &[f64]) -> f64 {
    let e: Vec<f64> = y.iter().zip(offset).map(|(a, b)| a - b).collect();
    let mean = e.iter().sum::<f64>() / e.len() as f64;
    e.iter().map(|v| (v - mean).powi(2)).sum()
}

/// `X β` with an intercept column prepended to `x`.
fn linear_predictor(x: &Matrix, beta: &[f64], keep: &[usize]) -> Vec<f64> {
    (0..x.nrows()).map(|i| beta[0] + keep.iter().zip(&beta[1..]).map(|(&j, b)| x.col(j)[i] * b).sum::<f64>()).collect()
}

/// Columns of `x` kept by a reduced model, as a matrix.
fn reduced(x: &Matrix, keep: &[usize]) -> Result<Matrix> {
    let cols: Vec<&[f64]> = keep.iter().map(|&j| x.col(j)).collect();
    if cols.is_empty() { Ok(Matrix::zeros(x.nrows(), 0)) } else { Matrix::from_columns(&cols) }
}

/// Fit `y` on the columns `keep` of `x` (intercept-only when empty):
/// coefficients, covariance and residual sum of squares.
fn ols(x: &Matrix, y: &[f64], keep: &[usize]) -> Result<OlsFit> {
    if keep.is_empty() {
        let n = y.len() as f64;
        let (mean, var) = mean_var(y);
        let rss = var * (n - 1.0);
        return Ok((vec![mean], vec![vec![var / n]], rss, 0.0, 0.0));
    }
    let fit = fit_linear_model(&reduced(x, keep)?, y, &OlsOptions::default())?;
    let s = fit.summary();
    let rss: f64 = fit.residuals().iter().map(|r| r * r).sum();
    Ok((fit.coefficients(), matrix_rows(fit.covariance()), rss, s.r_squared, s.adj_r_squared))
}

/// A linear model fitted to every imputation with one design built from all
/// of them, with D₃ statistics for each effect and the whole model.
fn linear(
    table: &DataTable,
    rows: &RowStateManager,
    column: usize,
    numbers: &[u64],
    spec: &crate::LinearModelSpec,
) -> Result<Pieces> {
    let imp = column_values(table, column)?;
    let all: Vec<bool> = (0..table.n_rows()).map(|r| imp[r] >= 1.0).collect();
    let coded = Coded::with_rows(table, rows, spec, &|r| all[r])?;
    let predictors = borrow_predictors(&coded.predictor_names, &coded.predictor_data);
    let design = &coded.design;
    let y_all = column_values(table, spec.response)?;
    let p = design.column_names().len();
    let full: Vec<usize> = (0..p).collect();
    let mut data = Vec::new();
    for &k in numbers {
        let used: Vec<usize> = coded.rows_used.iter().copied().filter(|&r| imp[r] == k as f64).collect();
        if used.len() <= p + 1 {
            return Err(invalid(format!("imputation {k} has too few rows for the model")));
        }
        let x = design.matrix(&predictors, &used)?;
        let y: Vec<f64> = used.iter().map(|&r| y_all[r]).collect();
        data.push((x, y));
    }
    let mut fits = Vec::new();
    let mut full_rss = Vec::new();
    for (x, y) in &data {
        let (values, covariance, rss, r2, adj) = ols(x, y, &full)?;
        full_rss.push(rss);
        fits.push(Fitted {
            values,
            covariance,
            df_complete: (y.len() - p - 1).max(1) as f64,
            n: y.len(),
            r_squared: Some((r2, adj)),
        });
    }
    let m = fits.len() as f64;
    let pooled: Vec<f64> = (0..=p).map(|j| fits.iter().map(|f| f.values[j]).sum::<f64>() / m).collect();
    // D₃ of a reduced model keeping `keep` against the full model.
    let d3 = |keep: &[usize]| -> Result<LrStatistics> {
        let mut own = Vec::new();
        let mut reduced_fits = Vec::new();
        for ((x, y), rss1) in data.iter().zip(&full_rss) {
            let (beta0, _, rss0, _, _) = ols(x, y, keep)?;
            own.push(2.0 * (normal_log_likelihood(*rss1, y.len()) - normal_log_likelihood(rss0, y.len())));
            reduced_fits.push(beta0);
        }
        let pooled0: Vec<f64> = (0..=keep.len()).map(|j| reduced_fits.iter().map(|b| b[j]).sum::<f64>() / m).collect();
        let at = data
            .iter()
            .map(|(x, y)| {
                let l1 = normal_log_likelihood(offset_rss(y, &linear_predictor(x, &pooled, &full)), y.len());
                let l0 = normal_log_likelihood(offset_rss(y, &linear_predictor(x, &pooled0, keep)), y.len());
                2.0 * (l1 - l0)
            })
            .collect();
        Ok((own, at))
    };
    let effects: Vec<(String, Vec<usize>)> =
        design.effects().iter().map(|e| (e.name.clone(), e.columns.clone().map(|j| j + 1).collect())).collect();
    let per_effect = design
        .effects()
        .iter()
        .map(|e| d3(&full.iter().copied().filter(|j| !e.columns.contains(j)).collect::<Vec<_>>()))
        .collect::<Result<Vec<_>>>()?;
    let whole = d3(&[])?;
    let mut names = vec![stat_linear::INTERCEPT_NAME.to_owned()];
    names.extend(design.column_names().iter().cloned());
    Ok(Pieces {
        label: "Term",
        nulls: vec![0.0; names.len()],
        names,
        numbers: Vec::new(),
        fits,
        effects,
        likelihood_ratio: Some((per_effect, whole)),
        correlations: false,
    })
}

fn p_text(p: f64) -> String {
    crate::format::format_p_value(p)
}

/// Confidence level of pooled intervals.
const LEVEL: f64 = 0.95;

/// Each estimate pooled by Rubin's rules.
fn estimates(p: &Pieces) -> Result<Vec<PooledEstimate>> {
    (0..p.names.len())
        .map(|j| {
            let q: Vec<f64> = p.fits.iter().map(|f| f.values[j]).collect();
            let u: Vec<f64> = p.fits.iter().map(|f| f.covariance[j][j]).collect();
            pool_scalar(&q, &u, p.fits[0].df_complete, p.nulls[j], LEVEL)
        })
        .collect()
}

/// The pooled sections built from each imputation's results.
fn sections(p: &Pieces) -> Result<Vec<ReportSection>> {
    let m = p.fits.len();
    let level = LEVEL;
    let pooled = estimates(p)?;
    let mut out = Vec::new();
    let n_text = {
        let ns: Vec<usize> = p.fits.iter().map(|f| f.n).collect();
        let (lo, hi) = (ns.iter().min().copied().unwrap_or(0), ns.iter().max().copied().unwrap_or(0));
        if lo == hi { format!("{lo} rows each") } else { format!("{lo} to {hi} rows each") }
    };
    out.push(section(
        SectionKind::PooledNote,
        "Pooled Across Imputations",
        SectionContent::Note(format!(
            "Fitted to each of the {m} completed data sets ({n_text}) and combined by Rubin's rules, with \
             Barnard–Rubin degrees of freedom. The sections after the pooled ones analyse the original data \
             only (imputation 0), leaving out rows with missing values: the complete-case analysis, for \
             comparison."
        )),
    ));
    // Correlations are pooled on Fisher's z scale, so their variance table
    // shows that scale too (the same pooling as the estimates table).
    let correlations = if p.correlations {
        (0..p.names.len())
            .map(|j| {
                let r: Vec<f64> = p.fits.iter().map(|f| f.values[j]).collect();
                let n: Vec<usize> = p.fits.iter().map(|f| (1.0 / f.covariance[j][j] + 3.0).round() as usize).collect();
                pool_correlation(&r, &n, level)
            })
            .collect::<Result<Vec<_>>>()?
    } else {
        Vec::new()
    };
    if p.correlations {
        let mut t = TextTable::new([
            "Pair",
            "Correlation",
            "Lower 95%",
            "Upper 95%",
            "Prob>|t| (ρ = 0)",
            "Fraction of missing information",
        ]);
        for (name, c) in p.names.iter().zip(&correlations) {
            t.push([
                name.clone(),
                format_number(c.estimate, 4),
                format_number(c.interval.0, 4),
                format_number(c.interval.1, 4),
                p_text(c.z.p_value),
                format_number(c.z.fmi, 3),
            ]);
        }
        out.push(section(SectionKind::PooledEstimates, "Pooled Correlations (Fisher z)", SectionContent::Table(t)));
    } else {
        let mut t = TextTable::new([
            p.label,
            "Estimate",
            "Std Error",
            "t Ratio",
            "DF",
            "Prob>|t|",
            "Lower 95%",
            "Upper 95%",
            "Fraction of missing information",
        ]);
        for (name, e) in p.names.iter().zip(&pooled) {
            t.push([
                name.clone(),
                format_number(e.estimate, 6),
                format_number(e.std_error, 4),
                format_number(e.t, 4),
                format_number(e.df, 4),
                p_text(e.p_value),
                format_number(e.interval.0, 6),
                format_number(e.interval.1, 6),
                format_number(e.fmi, 3),
            ]);
        }
        let null_note = p.nulls.iter().any(|&v| v != 0.0);
        let title =
            if null_note { "Pooled Estimates (t tests against the hypothesized value)" } else { "Pooled Estimates" };
        out.push(section(SectionKind::PooledEstimates, title, SectionContent::Table(t)));
    }
    let mut v = TextTable::new([
        p.label,
        "Within (Ū)",
        "Between (B)",
        "Total (T)",
        "Relative increase (r)",
        "λ",
        "Fraction of missing information",
    ]);
    let variance: Vec<_> = if p.correlations { correlations.iter().map(|c| c.z).collect() } else { pooled };
    for (name, e) in p.names.iter().zip(&variance) {
        v.push([
            name.clone(),
            format_number(e.within, 4),
            format_number(e.between, 4),
            format_number(e.total, 4),
            format_number(e.riv, 4),
            format_number(e.lambda, 4),
            format_number(e.fmi, 3),
        ]);
    }
    let title = if p.correlations {
        "Variance Within and Between Imputations (Fisher z scale)"
    } else {
        "Variance Within and Between Imputations"
    };
    out.push(section(SectionKind::PooledVariance, title, SectionContent::Table(v)));
    if !p.effects.is_empty() {
        out.push(tests_table(p)?);
    }
    if p.fits.iter().all(|f| f.r_squared.is_some()) {
        let n: Vec<usize> = p.fits.iter().map(|f| f.n).collect();
        let r2: Vec<f64> = p.fits.iter().filter_map(|f| f.r_squared.map(|r| r.0)).collect();
        let adj: Vec<f64> = p.fits.iter().filter_map(|f| f.r_squared.map(|r| r.1.max(0.0))).collect();
        let mut t =
            TextTable::new(["Statistic", "Pooled", "Lower 95%", "Upper 95%", "Fraction of missing information"]);
        for (name, vals) in [("R²", &r2), ("Adjusted R²", &adj)] {
            if let Ok(c) = pool_r_squared(vals, &n, level) {
                t.push([
                    name.to_owned(),
                    format_number(c.estimate, 4),
                    format_number(c.interval.0, 4),
                    format_number(c.interval.1, 4),
                    format_number(c.z.fmi, 3),
                ]);
            }
        }
        out.push(section(SectionKind::PooledFit, "Pooled Fit (R² on Fisher's z scale)", SectionContent::Table(t)));
    }
    let shown = p.names.len().min(TERMS_BY_IMPUTATION);
    let mut header = vec!["Imputation".to_owned(), "Rows".to_owned()];
    header.extend(p.names.iter().take(shown).cloned());
    let mut t = TextTable::new(header);
    for (k, f) in p.numbers.iter().zip(&p.fits) {
        let mut row = vec![k.to_string(), f.n.to_string()];
        row.extend(f.values.iter().take(shown).map(|v| format_number(*v, 6)));
        t.push(row);
    }
    let title = if shown < p.names.len() {
        format!("Estimates by Imputation (first {shown} of {})", p.names.len())
    } else {
        "Estimates by Imputation".to_owned()
    };
    out.push(section(SectionKind::PooledByImputation, &title, SectionContent::Table(t)));
    Ok(out)
}

/// One pooled effect test: the effect, its parameter count, D₁ and (linear
/// models) D₃.
struct EffectTest {
    name: String,
    parameters: usize,
    wald: PooledTest,
    likelihood_ratio: Option<PooledTest>,
}

/// D₁ (and for linear models D₃) tests of each effect and, with more than
/// one effect, of the whole model.
fn effect_tests(p: &Pieces) -> Result<Vec<EffectTest>> {
    let df = p.fits[0].df_complete;
    let test = |name: &str, positions: &[usize], lr: Option<&LrStatistics>| -> Result<EffectTest> {
        let est: Vec<Vec<f64>> = p.fits.iter().map(|f| positions.iter().map(|&j| f.values[j]).collect()).collect();
        let cov: Vec<Vec<Vec<f64>>> = p
            .fits
            .iter()
            .map(|f| positions.iter().map(|&a| positions.iter().map(|&b| f.covariance[a][b]).collect()).collect())
            .collect();
        Ok(EffectTest {
            name: name.to_owned(),
            parameters: positions.len(),
            wald: pool_wald(&est, &cov, &vec![0.0; positions.len()], df)?,
            likelihood_ratio: lr.map(|(own, at)| pool_likelihood_ratio(own, at, positions.len())).transpose()?,
        })
    };
    let mut out = Vec::new();
    for (k, (name, positions)) in p.effects.iter().enumerate() {
        if !positions.is_empty() {
            out.push(test(name, positions, p.likelihood_ratio.as_ref().map(|(e, _)| &e[k]))?);
        }
    }
    if p.effects.len() > 1 {
        let all: Vec<usize> = (1..p.names.len()).collect();
        out.push(test("Whole model", &all, p.likelihood_ratio.as_ref().map(|(_, w)| w))?);
    }
    Ok(out)
}

fn tests_table(p: &Pieces) -> Result<ReportSection> {
    let with_d3 = p.likelihood_ratio.is_some();
    let mut header = vec!["Effect", "Parameters", "F (D₁)", "DF den", "Prob>F"];
    if with_d3 {
        header.extend(["F (D₃)", "DF den (D₃)", "Prob>F (D₃)"]);
    }
    let mut t = TextTable::new(header);
    for e in effect_tests(p)? {
        let mut row = vec![
            e.name,
            e.parameters.to_string(),
            format_number(e.wald.statistic, 4),
            format_number(e.wald.df_den, 4),
            p_text(e.wald.p_value),
        ];
        if let Some(d3) = e.likelihood_ratio {
            row.extend([format_number(d3.statistic, 4), format_number(d3.df_den, 4), p_text(d3.p_value)]);
        } else if with_d3 {
            row.extend([String::new(), String::new(), String::new()]);
        }
        t.push(row);
    }
    let title = if with_d3 {
        "Pooled Tests by Effect (D₁ Wald, D₃ likelihood ratio)"
    } else {
        "Pooled Tests by Effect (D₁ Wald)"
    };
    Ok(section(SectionKind::PooledTests, title, SectionContent::Table(t)))
}

#[cfg(test)]
mod tests;
