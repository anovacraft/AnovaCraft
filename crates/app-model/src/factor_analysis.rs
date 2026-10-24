//! Exploratory factor analysis bound to a dataset
//! ([`stat_multivariate::factor`]).
//!
//! The rows complete in every chosen column and not excluded give the
//! correlation (or covariance) matrix. The number of factors is the request's,
//! or by default the number of eigenvalues of the correlation matrix above 1
//! (Kaiser's guideline; above the mean eigenvalue for a covariance matrix),
//! reduced if maximum likelihood would have negative degrees of freedom.
//!
//! The report shows the eigenvalues and scree plot, the test of fit
//! (maximum likelihood), unrotated and rotated loadings (pattern and
//! structure, and the factor correlations, for an oblique rotation),
//! communalities and uniquenesses, the variance each factor accounts for and
//! a plot of the variables' loadings on factors 1 and 2. Factor scores
//! (regression or Bartlett) can be saved as columns.

use stat_core::{Result, StatError};
use stat_data::{DataTable, JsonValue, RowStateManager};
use stat_multivariate::{Extraction, FactorFit, FactorOptions, Rotation, ScoreMethod, factor_analysis};

use crate::format::{format_number, format_p_value};
use crate::report::{Annotations, PlotSpec, ReportSection, SectionContent, SectionKind, TextTable};
use crate::report_columns::ReportColumn;

const DIGITS: usize = 4;

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

fn section(kind: SectionKind, title: &str, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.to_owned(), content }
}

/// Stable keys and display names of the settings.
fn extraction_key(e: Extraction) -> &'static str {
    match e {
        Extraction::MaximumLikelihood => "maximum-likelihood",
        Extraction::PrincipalAxis => "principal-axis",
    }
}

/// Display name of an extraction method.
#[must_use]
pub fn extraction_label(e: Extraction) -> &'static str {
    match e {
        Extraction::MaximumLikelihood => "Maximum likelihood",
        Extraction::PrincipalAxis => "Principal axis (iterated communalities)",
    }
}

fn rotation_key(r: Rotation) -> &'static str {
    match r {
        Rotation::None => "none",
        Rotation::Varimax => "varimax",
        Rotation::Promax => "promax",
    }
}

/// Display name of a rotation.
#[must_use]
pub fn rotation_label(r: Rotation) -> &'static str {
    match r {
        Rotation::None => "None",
        Rotation::Varimax => "Varimax (orthogonal)",
        Rotation::Promax => "Promax (oblique)",
    }
}

fn score_key(s: ScoreMethod) -> &'static str {
    match s {
        ScoreMethod::Regression => "regression",
        ScoreMethod::Bartlett => "bartlett",
    }
}

/// Display name of a factor-score method.
#[must_use]
pub fn score_label(s: ScoreMethod) -> &'static str {
    match s {
        ScoreMethod::Regression => "Regression (Thomson)",
        ScoreMethod::Bartlett => "Bartlett (weighted least squares)",
    }
}

/// Columns and settings of a factor analysis.
#[derive(Debug, Clone, PartialEq)]
pub struct FactorRequest {
    /// Numeric columns (three or more).
    pub columns: Vec<usize>,
    /// Number of factors (`None`: eigenvalues above 1).
    pub factors: Option<usize>,
    /// Extraction method.
    pub extraction: Extraction,
    /// Rotation.
    pub rotation: Rotation,
    /// Analyse the covariance matrix instead of the correlation matrix.
    pub covariance: bool,
    /// How saved factor scores are computed.
    pub scores: ScoreMethod,
    /// Power of the promax target.
    pub promax_power: f64,
}

impl FactorRequest {
    /// Maximum likelihood on the correlation matrix, factors with eigenvalue
    /// above 1, varimax, regression scores, promax power 4.
    #[must_use]
    pub fn new(columns: Vec<usize>) -> Self {
        Self {
            columns,
            factors: None,
            extraction: Extraction::MaximumLikelihood,
            rotation: Rotation::Varimax,
            covariance: false,
            scores: ScoreMethod::Regression,
            promax_power: 4.0,
        }
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

    /// The project-file form (`"analysis": "factor-analysis"`).
    #[must_use]
    pub fn to_json(&self, table: &DataTable) -> JsonValue {
        crate::project::obj([
            ("analysis", crate::project::s("factor-analysis")),
            ("columns", crate::correlation_analysis::columns_json(&self.columns, table)),
            ("factors", self.factors.map_or(JsonValue::Null, |k| JsonValue::Number(k as f64))),
            ("extraction", crate::project::s(extraction_key(self.extraction))),
            ("rotation", crate::project::s(rotation_key(self.rotation))),
            ("matrix", crate::project::s(if self.covariance { "covariance" } else { "correlation" })),
            ("scores", crate::project::s(score_key(self.scores))),
            ("promax_power", JsonValue::number(self.promax_power)),
        ])
    }

    /// Read the project-file form; missing settings take their defaults.
    ///
    /// # Errors
    /// Missing or unknown columns, or an unknown setting.
    pub fn from_json(v: &JsonValue, table: &DataTable) -> Result<Self> {
        let d = Self::new(crate::correlation_analysis::json_columns(v, "columns", table)?);
        let text = |k: &str| v.get(k).and_then(JsonValue::as_str);
        let extraction = match text("extraction") {
            None => d.extraction,
            Some("maximum-likelihood") => Extraction::MaximumLikelihood,
            Some("principal-axis") => Extraction::PrincipalAxis,
            Some(o) => return Err(invalid(format!("factor analysis: unknown extraction {o:?}"))),
        };
        let rotation = match text("rotation") {
            None => d.rotation,
            Some("none") => Rotation::None,
            Some("varimax") => Rotation::Varimax,
            Some("promax") => Rotation::Promax,
            Some(o) => return Err(invalid(format!("factor analysis: unknown rotation {o:?}"))),
        };
        let covariance = match text("matrix") {
            None | Some("correlation") => false,
            Some("covariance") => true,
            Some(o) => return Err(invalid(format!("factor analysis: unknown matrix {o:?}"))),
        };
        let scores = match text("scores") {
            None => d.scores,
            Some("regression") => ScoreMethod::Regression,
            Some("bartlett") => ScoreMethod::Bartlett,
            Some(o) => return Err(invalid(format!("factor analysis: unknown scores {o:?}"))),
        };
        Ok(Self {
            factors: v.get("factors").and_then(JsonValue::as_usize),
            extraction,
            rotation,
            covariance,
            scores,
            promax_power: v.get("promax_power").and_then(JsonValue::as_f64).unwrap_or(d.promax_power),
            ..d
        })
    }
}

/// A computed factor analysis.
#[derive(Debug, Clone)]
pub struct FactorAnalysisReport {
    request: FactorRequest,
    names: Vec<String>,
    n_rows: usize,
    rows: Vec<usize>,
    data: Vec<Vec<f64>>,
    means: Vec<f64>,
    scales: Vec<f64>,
    /// With the default number of factors: how many eigenvalues are above
    /// their mean (more than the factors fitted when the number was reduced
    /// so the model is identified).
    automatic: Option<usize>,
    fit: FactorFit,
    n_excluded: usize,
    n_missing: usize,
}

impl FactorAnalysisReport {
    /// Run the analysis.
    ///
    /// # Errors
    /// Fewer than three columns, a missing or non-numeric column, a constant
    /// column, and errors from [`factor_analysis`] (a singular matrix, too
    /// many factors, too few rows).
    pub fn run(table: &DataTable, states: &RowStateManager, request: &FactorRequest) -> Result<Self> {
        if request.columns.len() < 3 {
            return Err(invalid("factor analysis needs at least three columns"));
        }
        let mut names = Vec::new();
        let mut values = Vec::new();
        for &i in &request.columns {
            let c = table.column(i).ok_or_else(|| invalid(format!("no column {i}")))?;
            values.push(c.as_numeric().ok_or_else(|| invalid(format!("{} must be numeric", c.name)))?);
            names.push(c.name.clone());
        }
        let (mut n_excluded, mut n_missing) = (0, 0);
        let mut rows = Vec::new();
        for r in 0..table.n_rows() {
            if !values.iter().all(|v| v[r].is_finite()) {
                n_missing += 1;
            } else if states.is_excluded(r) {
                n_excluded += 1;
            } else {
                rows.push(r);
            }
        }
        let p = names.len();
        let n = rows.len();
        if n < 3 {
            return Err(StatError::InsufficientData { required: 3, found: n });
        }
        let data: Vec<Vec<f64>> = rows.iter().map(|&r| values.iter().map(|v| v[r]).collect()).collect();
        let means: Vec<f64> =
            (0..p).map(|j| stat_core::summation::sum_iter(data.iter().map(|r| r[j])) / n as f64).collect();
        let mut cov = vec![vec![0.0; p]; p];
        for a in 0..p {
            for b in a..p {
                let s = stat_core::summation::sum_iter(data.iter().map(|r| (r[a] - means[a]) * (r[b] - means[b])))
                    / (n as f64 - 1.0);
                cov[a][b] = s;
                cov[b][a] = s;
            }
        }
        let sd: Vec<f64> = (0..p).map(|j| cov[j][j].sqrt()).collect();
        if let Some(j) = (0..p).find(|&j| sd[j].is_nan() || sd[j] <= 0.0) {
            return Err(invalid(format!("{} is constant in the rows analysed", names[j])));
        }
        let corr: Vec<Vec<f64>> = (0..p).map(|a| (0..p).map(|b| cov[a][b] / (sd[a] * sd[b])).collect()).collect();
        let matrix = if request.covariance { cov } else { corr.clone() };
        let mut automatic = None;
        let factors = match request.factors {
            Some(k) => k,
            None => {
                let (ev, _) = stat_core::eigen::symmetric_eigen(&matrix)?;
                let mean = ev.iter().sum::<f64>() / p as f64;
                let above = ev.iter().filter(|&&l| l > mean).count();
                automatic = Some(above);
                let mut k = above.max(1).min(p - 1);
                if request.extraction == Extraction::MaximumLikelihood {
                    while k > 1 && (p - k) * (p - k) < p + k {
                        k -= 1;
                    }
                }
                k
            }
        };
        let options = FactorOptions {
            extraction: request.extraction,
            factors,
            rotation: request.rotation,
            promax_power: request.promax_power,
            ..FactorOptions::new(factors)
        };
        let fit = factor_analysis(&matrix, n, &options)?;
        let scales = if request.covariance { vec![1.0; p] } else { sd };
        Ok(Self {
            request: request.clone(),
            names,
            n_rows: table.n_rows(),
            rows,
            data,
            means,
            scales,
            automatic,
            fit,
            n_excluded,
            n_missing,
        })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &FactorRequest {
        &self.request
    }

    /// The fitted model.
    #[must_use]
    pub fn fit(&self) -> &FactorFit {
        &self.fit
    }

    /// Report title naming the extraction method, e.g.
    /// `"Maximum Likelihood Factor Analysis: x, y, z"`.
    #[must_use]
    pub fn title(&self) -> String {
        let kind = match self.request.extraction {
            Extraction::MaximumLikelihood => "Maximum Likelihood Factor Analysis",
            Extraction::PrincipalAxis => "Principal Axis Factor Analysis",
        };
        if self.names.len() > 6 {
            format!("{kind}: {} columns", self.names.len())
        } else {
            format!("{kind}: {}", self.names.join(", "))
        }
    }

    /// Factor scores of every dataset row (NaN for rows not analysed), one
    /// vector per factor.
    ///
    /// # Errors
    /// From [`FactorFit::score_weights`].
    pub fn scores(&self, method: ScoreMethod) -> Result<Vec<Vec<f64>>> {
        let w = self.fit.score_weights(method)?;
        let m = self.fit.n_factors();
        let mut out = vec![vec![f64::NAN; self.n_rows]; m];
        for (row, &r) in self.data.iter().zip(&self.rows) {
            let z: Vec<f64> = row.iter().enumerate().map(|(j, x)| (x - self.means[j]) / self.scales[j]).collect();
            for (k, col) in out.iter_mut().enumerate() {
                col[r] = z.iter().zip(&w).map(|(zj, wj)| zj * wj[k]).sum();
            }
        }
        Ok(out)
    }

    /// Columns the report can add: `factor1`, `factor2`, … (scores by the
    /// request's method).
    #[must_use]
    pub fn saveable_columns(&self) -> Vec<ReportColumn> {
        let Ok(scores) = self.scores(self.request.scores) else { return Vec::new() };
        let how = match self.request.scores {
            ScoreMethod::Regression => "",
            ScoreMethod::Bartlett => " (Bartlett)",
        };
        scores
            .into_iter()
            .enumerate()
            .map(|(k, v)| {
                ReportColumn::continuous(
                    &format!("factor{}", k + 1),
                    &format!("Save factor {} scores{how}", k + 1),
                    format!("Factor {} score", k + 1),
                    v,
                )
            })
            .collect()
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let mut out = vec![self.summary(), self.eigenvalue_table(), self.scree()];
        out.extend(self.fit_test());
        out.push(self.loadings_table(SectionKind::FactorLoadings, "Unrotated Loadings", &self.fit.unrotated));
        if self.fit.options.rotation != Rotation::None && self.fit.n_factors() > 1 {
            if self.fit.oblique() {
                out.push(self.loadings_table(
                    SectionKind::FactorRotated,
                    &format!("Rotated Pattern Loadings ({})", rotation_label(self.fit.options.rotation)),
                    &self.fit.pattern,
                ));
                out.push(self.loadings_table(
                    SectionKind::FactorStructure,
                    if self.request.covariance {
                        "Structure Loadings (covariances of variables with factors)"
                    } else {
                        "Structure Loadings (correlations of variables with factors)"
                    },
                    &self.fit.structure,
                ));
                out.push(self.factor_correlations());
            } else {
                out.push(self.loadings_table(
                    SectionKind::FactorRotated,
                    &format!("Rotated Loadings ({})", rotation_label(self.fit.options.rotation)),
                    &self.fit.pattern,
                ));
            }
        }
        out.push(self.communalities());
        out.push(self.variance_explained());
        out.extend(self.loading_plot());
        out
    }

    fn summary(&self) -> ReportSection {
        let f = &self.fit;
        let mut t = TextTable::new(["Statistic", "Value"]);
        let mut push = |k: &str, v: String| t.push([k.to_owned(), v]);
        push("Columns", self.names.join(", "));
        push("Rows analysed", self.rows.len().to_string());
        if self.n_excluded > 0 {
            push("Rows excluded", self.n_excluded.to_string());
        }
        if self.n_missing > 0 {
            push("Rows with a missing value", self.n_missing.to_string());
        }
        push("Matrix", if self.request.covariance { "Covariances" } else { "Correlations" }.to_owned());
        push("Extraction", extraction_label(f.options.extraction).to_owned());
        push(
            "Factors",
            match self.automatic {
                Some(above) => {
                    let rule = if self.request.covariance {
                        "eigenvalues above their mean"
                    } else {
                        "eigenvalues of the correlation matrix above 1"
                    };
                    if above == f.n_factors() {
                        format!("{} ({rule})", f.n_factors())
                    } else {
                        format!(
                            "{} ({above} {rule}, adjusted to the range from 1 to the most the extraction can \
                             identify with {} columns)",
                            f.n_factors(),
                            self.names.len()
                        )
                    }
                }
                None => f.n_factors().to_string(),
            },
        );
        push(
            "Rotation",
            match f.options.rotation {
                Rotation::Promax => format!("Promax (power {}; oblique)", format_number(f.options.promax_power, 3)),
                Rotation::Varimax => "Varimax with Kaiser normalization (orthogonal)".to_owned(),
                Rotation::None => "None".to_owned(),
            },
        );
        push(
            "Iterations",
            format!(
                "{}{}",
                f.iterations,
                if f.converged { "" } else { " (did not converge; results are approximate)" }
            ),
        );
        let heywood: Vec<&str> =
            self.names.iter().zip(&f.heywood).filter(|(_, h)| **h).map(|(n, _)| n.as_str()).collect();
        if !heywood.is_empty() {
            push(
                "Heywood cases",
                format!(
                    "{} (communality reached the variance and was held at its bound; consider fewer factors)",
                    heywood.join(", ")
                ),
            );
        }
        push("Saved scores", score_label(self.request.scores).to_owned());
        section(SectionKind::FactorSummary, "Summary", SectionContent::Table(t))
    }

    fn eigenvalue_table(&self) -> ReportSection {
        let ev = &self.fit.eigenvalues;
        let total: f64 = ev.iter().sum();
        let mut t = TextTable::new(["Number", "Eigenvalue", "Percent", "Cumulative percent"]);
        let mut cum = 0.0;
        for (k, l) in ev.iter().enumerate() {
            cum += l;
            t.push([
                (k + 1).to_string(),
                format_number(*l, 5),
                format!("{:.2}", 100.0 * l / total),
                format!("{:.2}", 100.0 * cum / total),
            ]);
        }
        let of = if self.request.covariance { "Covariance" } else { "Correlation" };
        section(SectionKind::FactorEigenvalues, &format!("Eigenvalues of the {of} Matrix"), SectionContent::Table(t))
    }

    fn scree(&self) -> ReportSection {
        let ev = &self.fit.eigenvalues;
        let p = ev.len();
        let mean = ev.iter().sum::<f64>() / p as f64;
        let m = self.fit.n_factors();
        let plot = PlotSpec {
            x_label: "Number".to_owned(),
            y_label: "Eigenvalue".to_owned(),
            line: Some(ev.iter().enumerate().map(|(k, &l)| [(k + 1) as f64, l]).collect()),
            line_name: Some("Eigenvalue".to_owned()),
            point_name: Some("Eigenvalue".to_owned()),
            x_ticks: Some((1..=p).map(|k| (k as f64, k.to_string())).collect()),
            segments: vec![vec![[1.0, mean], [p as f64, mean]]],
            annotations: Some(Annotations {
                groups: vec!["Factors extracted".to_owned()],
                items: vec![(m as f64, ev[m - 1], format!("{m} factors"), 0)],
                labels: vec![(
                    p as f64,
                    mean,
                    if self.request.covariance { "mean".to_owned() } else { "1".to_owned() },
                )],
            }),
            ..PlotSpec::default()
        };
        section(SectionKind::FactorScree, "Scree Plot", SectionContent::Plot(Box::new(plot)))
    }

    fn fit_test(&self) -> Option<ReportSection> {
        let t = self.fit.fit_test?;
        let mut table = TextTable::new(["Statistic", "Value"]);
        table.push(["Discrepancy F".to_owned(), format_number(t.discrepancy, 5)]);
        table.push(["Chi-square (Bartlett correction)".to_owned(), format_number(t.chi_square, 5)]);
        table.push(["DF".to_owned(), format_number(t.df, 4)]);
        table.push([
            "Prob > ChiSq".to_owned(),
            t.p_value.map_or_else(|| "— (no degrees of freedom left)".to_owned(), format_p_value),
        ]);
        let title = format!("Test that {} Factors Are Sufficient", self.fit.n_factors());
        Some(section(SectionKind::FactorFitTest, &title, SectionContent::Table(table)))
    }

    fn loadings_table(&self, kind: SectionKind, title: &str, loadings: &[Vec<f64>]) -> ReportSection {
        let m = self.fit.n_factors();
        let mut heads = vec!["Variable".to_owned()];
        heads.extend((1..=m).map(|k| format!("Factor {k}")));
        let mut t = TextTable::new(heads);
        for (name, row) in self.names.iter().zip(loadings) {
            let mut cells = vec![name.clone()];
            cells.extend(row.iter().map(|v| format_number(*v, DIGITS)));
            t.push(cells);
        }
        section(kind, title, SectionContent::Table(t))
    }

    fn factor_correlations(&self) -> ReportSection {
        let m = self.fit.n_factors();
        let mut heads = vec!["Factor".to_owned()];
        heads.extend((1..=m).map(|k| format!("Factor {k}")));
        let mut t = TextTable::new(heads);
        for (k, row) in self.fit.factor_correlation.iter().enumerate() {
            let mut cells = vec![format!("Factor {}", k + 1)];
            cells.extend(row.iter().map(|v| format_number(*v, DIGITS)));
            t.push(cells);
        }
        section(SectionKind::FactorCorrelations, "Factor Correlations", SectionContent::Table(t))
    }

    fn communalities(&self) -> ReportSection {
        let f = &self.fit;
        let mut t = TextTable::new(["Variable", "Initial (SMC)", "Final communality", "Uniqueness", ""]);
        for (j, name) in self.names.iter().enumerate() {
            t.push([
                name.clone(),
                format_number(f.initial_communalities[j], DIGITS),
                format_number(f.communalities[j], DIGITS),
                format_number(f.uniquenesses[j], DIGITS),
                if f.heywood[j] { "Heywood case".to_owned() } else { String::new() },
            ]);
        }
        section(SectionKind::FactorCommunalities, "Communalities and Uniquenesses", SectionContent::Table(t))
    }

    fn variance_explained(&self) -> ReportSection {
        let f = &self.fit;
        let total = f.total_variance();
        let unrot = FactorFit::sums_of_squares(&f.unrotated);
        let rotated =
            (f.options.rotation != Rotation::None && f.n_factors() > 1).then(|| FactorFit::sums_of_squares(&f.pattern));
        let mut heads = vec!["Factor", "Variance (unrotated)", "Percent", "Cumulative percent"];
        if rotated.is_some() {
            heads.push(if f.oblique() { "Variance (rotated pattern)" } else { "Variance (rotated)" });
            if !f.oblique() {
                heads.push("Percent (rotated)");
            }
        }
        let mut t = TextTable::new(heads);
        let mut cum = 0.0;
        for k in 0..f.n_factors() {
            cum += unrot[k];
            let mut row = vec![
                format!("Factor {}", k + 1),
                format_number(unrot[k], 5),
                format!("{:.2}", 100.0 * unrot[k] / total),
                format!("{:.2}", 100.0 * cum / total),
            ];
            if let Some(r) = &rotated {
                row.push(format_number(r[k], 5));
                if !f.oblique() {
                    row.push(format!("{:.2}", 100.0 * r[k] / total));
                }
            }
            t.push(row);
        }
        let note = if f.oblique() { " (oblique factors overlap, so rotated variances do not add)" } else { "" };
        section(SectionKind::FactorVariance, &format!("Variance Explained{note}"), SectionContent::Table(t))
    }

    fn loading_plot(&self) -> Option<ReportSection> {
        let f = &self.fit;
        if f.n_factors() < 2 {
            return None;
        }
        let a = &f.pattern;
        let items: Vec<(f64, f64, String, usize)> =
            self.names.iter().zip(a).map(|(n, r)| (r[0], r[1], n.clone(), 0)).collect();
        let reach = a.iter().map(|r| r[0].abs().max(r[1].abs())).fold(1.0_f64, f64::max);
        let plot = PlotSpec {
            x_label: "Factor 1 loading".to_owned(),
            y_label: "Factor 2 loading".to_owned(),
            segments: {
                let mut s: Vec<Vec<[f64; 2]>> = a.iter().map(|r| vec![[0.0, 0.0], [r[0], r[1]]]).collect();
                s.push(vec![[-reach, 0.0], [reach, 0.0]]);
                s.push(vec![[0.0, -reach], [0.0, reach]]);
                s
            },
            annotations: Some(Annotations { groups: vec!["Variables".to_owned()], items, labels: Vec::new() }),
            ..PlotSpec::default()
        };
        let which = if f.options.rotation == Rotation::None || f.n_factors() < 2 {
            "unrotated"
        } else if f.oblique() {
            "rotated pattern"
        } else {
            "rotated"
        };
        Some(section(
            SectionKind::FactorLoadingPlot,
            &format!("Loading Plot (factor 1 by factor 2, {which})"),
            SectionContent::Plot(Box::new(plot)),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    /// Six ability tests from two latent factors (simulated).
    fn table() -> DataTable {
        let mut rng = stat_core::rng::Rng::new(41);
        let n = 200;
        let mut cols: Vec<Vec<f64>> = vec![Vec::new(); 6];
        for _ in 0..n {
            let (f1, f2) = (rng.normal(), rng.normal());
            for (j, c) in cols.iter_mut().enumerate() {
                let (a, b) = if j < 3 { (0.8, 0.1) } else { (0.1, 0.75) };
                c.push(50.0 + 10.0 * (a * f1 + b * f2 + 0.55 * rng.normal()));
            }
        }
        let mut columns: Vec<Column> =
            cols.into_iter().enumerate().map(|(j, v)| Column::numeric(format!("test{}", j + 1), v)).collect();
        columns.push(Column::text("label", (0..n).map(|i| format!("s{i}")).collect()));
        DataTable::new("abilities", columns).unwrap()
    }

    /// Four columns in two uncorrelated pairs: two eigenvalues above 1, but
    /// maximum likelihood identifies only one factor from four columns, and
    /// the summary says so rather than crediting the eigenvalue rule.
    #[test]
    fn reduced_factor_count_is_stated() {
        let mut rng = stat_core::rng::Rng::new(7);
        let mut cols: Vec<Vec<f64>> = vec![Vec::new(); 4];
        for _ in 0..150 {
            let (f1, f2) = (rng.normal(), rng.normal());
            for (j, c) in cols.iter_mut().enumerate() {
                c.push(if j < 2 { f1 } else { f2 } + 0.4 * rng.normal());
            }
        }
        let columns = cols.into_iter().enumerate().map(|(j, v)| Column::numeric(format!("x{j}"), v)).collect();
        let t = DataTable::new("pairs", columns).unwrap();
        let a =
            FactorAnalysisReport::run(&t, &RowStateManager::new(150), &FactorRequest::new((0..4).collect())).unwrap();
        assert_eq!(a.fit().n_factors(), 1);
        let SectionContent::Table(summary) = &a.sections()[0].content else { panic!() };
        let factors = &summary.rows.iter().find(|r| r[0] == "Factors").unwrap()[1];
        assert!(factors.starts_with("1 (2 eigenvalues of the correlation matrix above 1, adjusted"), "{factors}");
    }

    #[test]
    fn default_request_finds_two_factors_and_saves_scores() {
        let t = table();
        let mut rows = RowStateManager::new(t.n_rows());
        rows.set_excluded([3], true);
        let a = FactorAnalysisReport::run(&t, &rows, &FactorRequest::new((0..6).collect())).unwrap();
        assert_eq!(a.fit().n_factors(), 2, "two eigenvalues above 1");
        let kinds: Vec<SectionKind> = a.sections().iter().map(|s| s.kind).collect();
        assert_eq!(
            kinds,
            [
                SectionKind::FactorSummary,
                SectionKind::FactorEigenvalues,
                SectionKind::FactorScree,
                SectionKind::FactorFitTest,
                SectionKind::FactorLoadings,
                SectionKind::FactorRotated,
                SectionKind::FactorCommunalities,
                SectionKind::FactorVariance,
                SectionKind::FactorLoadingPlot,
            ]
        );
        // Varimax separates the two blocks of tests.
        let p = &a.fit().pattern;
        for j in 0..3 {
            assert!(p[j][0].abs() > 0.6 && p[j][1].abs() < 0.35, "{:?}", p[j]);
            assert!(p[j + 3][1].abs() > 0.6 && p[j + 3][0].abs() < 0.35, "{:?}", p[j + 3]);
        }
        let cols = a.saveable_columns();
        assert_eq!(cols.iter().map(|c| c.key.as_str()).collect::<Vec<_>>(), ["factor1", "factor2"]);
        assert!(cols[0].values[3].is_nan() && cols[0].values[0].is_finite());
        // Regression scores of standardized data have mean 0.
        let mean: f64 = cols[0].values.iter().filter(|v| v.is_finite()).sum::<f64>() / 199.0;
        assert!(mean.abs() < 1e-10);
        assert!(a.title().starts_with("Maximum Likelihood Factor Analysis: test1"), "{}", a.title());
    }

    #[test]
    fn promax_principal_axis_and_json_round_trip() {
        let t = table();
        let rows = RowStateManager::new(t.n_rows());
        let req = FactorRequest {
            factors: Some(2),
            extraction: Extraction::PrincipalAxis,
            rotation: Rotation::Promax,
            covariance: true,
            scores: ScoreMethod::Bartlett,
            ..FactorRequest::new((0..6).collect())
        };
        let a = FactorAnalysisReport::run(&t, &rows, &req).unwrap();
        let kinds: Vec<SectionKind> = a.sections().iter().map(|s| s.kind).collect();
        assert!(kinds.contains(&SectionKind::FactorStructure) && kinds.contains(&SectionKind::FactorCorrelations));
        assert!(!kinds.contains(&SectionKind::FactorFitTest));
        // With the covariance matrix the structure loadings are covariances.
        let structure_title = |a: &FactorAnalysisReport| {
            a.sections().into_iter().find(|s| s.kind == SectionKind::FactorStructure).unwrap().title
        };
        assert_eq!(structure_title(&a), "Structure Loadings (covariances of variables with factors)");
        let corr = FactorAnalysisReport::run(&t, &rows, &FactorRequest { covariance: false, ..req.clone() }).unwrap();
        assert_eq!(structure_title(&corr), "Structure Loadings (correlations of variables with factors)");
        assert!(a.saveable_columns()[0].label.contains("Bartlett"));
        let back = FactorRequest::from_json(&req.to_json(&t), &t).unwrap();
        assert_eq!(back, req);
        let d = FactorRequest::new(vec![0, 1, 2]);
        assert_eq!(FactorRequest::from_json(&d.to_json(&t), &t).unwrap(), d);
        assert!(FactorAnalysisReport::run(&t, &rows, &FactorRequest::new(vec![0, 1])).is_err());
        assert!(FactorAnalysisReport::run(&t, &rows, &FactorRequest::new(vec![0, 1, 6])).is_err(), "text");
        let too_many = FactorRequest { factors: Some(4), ..FactorRequest::new((0..6).collect()) };
        assert!(FactorAnalysisReport::run(&t, &rows, &too_many).is_err());
    }
}
